//! HTTP-клиент к API 9router (localhost).
//!
//! - `/v1/models`     — health-check + список доступных моделей (комбо идут с
//!                      `owned_by: "combo"`).
//! - `/api/combos`    — список комбо (для дропдауна чата).
//! - `/v1/chat/completions` — OpenAI-совместимый чат (стриминг SSE / полный).

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::blocking::Client;

use super::client_error::{CloudStreamError, CLOUD_ERROR_FRAME_MARKER};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct ComboInfo {
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

/// Комбо — LLM (у 9router kind=llm по умолчанию; для чата нужны только такие).
impl ComboInfo {
    pub fn is_llm(&self) -> bool {
        match &self.kind {
            None => true,
            Some(k) => k.eq_ignore_ascii_case("llm"),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    pub stream: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CloudToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default)]
pub struct ChatCompletionResult {
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<CloudToolCall>,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct StreamDelta {
    pub content: String,
    pub reasoning: String,
}

impl Default for ChatRequest {
    fn default() -> Self {
        Self {
            model: String::new(),
            messages: Vec::new(),
            max_tokens: None,
            temperature: None,
            tools: None,
            tool_choice: None,
            stream: true,
        }
    }
}

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))
}

/// Жив ли шлюз (GET /v1/models). 401/403 = сервер жив, требует авторизацию —
/// считаем здоровым (иначе ForeignOccupant(0) на внешнем 9Router с API-ключом).
pub fn is_healthy(base_url: &str) -> bool {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let Ok(c) = client() else { return false };
    match c.get(&url).timeout(Duration::from_secs(4)).send() {
        Ok(resp) => {
            let s = resp.status();
            s.is_success() || s == reqwest::StatusCode::UNAUTHORIZED || s == reqwest::StatusCode::FORBIDDEN
        }
        Err(_) => false,
    }
}

/// Список комбо из `/api/combos` (только LLM).
///
/// Есть ключевой нюанс: `/api/combos` — внутренний роут дашборда и на
/// локальном сервере отвечает `401 Unauthorized` без куки-сессии дашборда
/// (Bearer не помогает). Поэтому при неуспехе пробуем fallback-источник —
/// OpenAI-совместимый `/v1/models` (`owned_by: "combo"`), который отдан
/// без авторизации. `api_key` отправляем, только если он задан.
///
/// Retry-петля (3 попытки с задержкой 600мс) нужна для ленивого старта сервера:
/// после открытия TCP-порта Next.js/9Router ещё 1-2 секунды инициализирует
/// внутренние роуты и SQLite, и первый запрос может упасть с 503/таймаутом.
pub fn get_combos(base_url: &str, api_key: Option<&str>) -> Result<Vec<ComboInfo>, String> {
    let mut last_err = String::new();
    for attempt in 1..=3 {
        match get_combos_once(base_url, api_key) {
            Ok(combos) => return Ok(combos),
            Err(e) => {
                last_err = e;
                if attempt < 3 {
                    log::warn!("get_combos: попытка {}/3 не удалась: {}, повтор через 600мс", attempt, last_err);
                    std::thread::sleep(Duration::from_millis(600));
                }
            }
        }
    }
    log::error!("get_combos: все 3 попытки не удались: {}", last_err);
    Err(last_err)
}

fn get_combos_once(base_url: &str, api_key: Option<&str>) -> Result<Vec<ComboInfo>, String> {
    let url = format!("{}/api/combos", base_url.trim_end_matches('/'));
    let mut req = client()?.get(&url).timeout(Duration::from_secs(8));
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        req = req.header("Authorization", format!("Bearer {}", key));
    }
    let resp = req
        .send()
        .map_err(|e| format!("Не удалось получить комбо 9router: {}", e))?;
    if resp.status().is_success() {
        let v: serde_json::Value = resp.json().ok().unwrap_or_default();
        let combos: Vec<ComboInfo> = v
            .get("combos")
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or_default();
        let llm: Vec<ComboInfo> = combos.into_iter().filter(|c| c.is_llm()).collect();
        if !llm.is_empty() {
            return Ok(llm);
        }
    }
    get_combos_from_models(base_url)
}

/// Fallback к списку комбо через `/v1/models` (`owned_by: "combo"`).
/// Эндпоинт открыт на localhost без авторизации; отдаёт только имена комбо.
fn get_combos_from_models(base_url: &str) -> Result<Vec<ComboInfo>, String> {
    let ids = get_models(base_url)?;
    if ids.is_empty() {
        return Err("9router не отдал комбо (добавьте их в дашборде 9Router).".to_string());
    }
    Ok(ids
        .into_iter()
        .map(|name| ComboInfo {
            name,
            kind: Some("llm".to_string()),
            models: Vec::new(),
        })
        .collect())
}

/// Список моделей из OpenAI-совместимого `/v1/models` (только комбо).
pub fn get_models(base_url: &str) -> Result<Vec<String>, String> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let resp = client()?
        .get(&url)
        .timeout(Duration::from_secs(8))
        .send()
        .map_err(|e| format!("Не удалось получить модели 9router: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("/v1/models: HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().map_err(|e| format!("Bad JSON моделей: {}", e))?;
    let mut out = Vec::new();
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                // Только комбо — регион контекста King Orch (локальные gguf не трогаем).
                if item.get("owned_by").and_then(|o| o.as_str()) == Some("combo") {
                    out.push(id.to_string());
                }
            }
        }
    }
    Ok(out)
}

/// Полный (не-стриминговый) ответ чата: `choices[0].message.content`.
pub fn chat_completion_nonstream(
    base_url: &str,
    api_key: Option<&str>,
    req: &ChatRequest,
) -> Result<String, String> {
    if let Err(e) = super::client_error::validate_request_shape(req) {
        log::error!("chat_completion_nonstream: невалидная форма запроса, не отправляю: {e}");
        return Err(e.to_string());
    }
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let mut body = req.clone();
    body.stream = false;
    let mut post = client()?
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(300));
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        post = post.header("Authorization", format!("Bearer {}", key));
    }
    let resp = post
        .send()
        .map_err(|e| format!("9router /v1/chat/completions: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(format!("/v1/chat/completions: HTTP {} {}", status, &text[..text.len().min(400)]));
    }
    let status = resp.status();
    let v: serde_json::Value = resp.json().map_err(|e| format!("Bad JSON ответа: {}", e))?;
    // 200 с телом-ошибкой (роутеры сообщают о квоте/рейт-лимите таким видом):
    // молча выкидывать запрещено — честная классификация вместо
    // «не содержит choices[0].message.content».
    if let Some(err) = v.get("error") {
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| {
                serde_json::to_string(err).unwrap_or_else(|_| "<неудобочитаемая ошибка>".into())
            });
        log::error!("chat_completion_nonstream: HTTP {status}, тело вернуло error-объект: {message}");
        return Err(format!(
            "Облако вернуло ошибку (HTTP {status}): {message} {CLOUD_ERROR_FRAME_MARKER}"
        ));
    }
    v.pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Ответ 9router не содержит choices[0].message.content".to_string())
}

/// Максимальный кусок сырого кадра в логах и ошибках: диагностика, не дамп.
const MAX_RAW_FRAME_CHARS: usize = 300;

fn raw_frame_of(line: &str) -> String {
    line.chars().take(MAX_RAW_FRAME_CHARS).collect()
}

/// Аномалии, накопленные при разборе SSE-тела. Нужны для классификации
/// результата: ошибка провайдера сильнее мусора, мусор сильнее пустоты.
#[derive(Debug, Default)]
pub(crate) struct SseScan {
    /// Обработано кадров `data:` без учёта `[DONE]` и пустых строк.
    frames: usize,
    /// Первый кадр с полем `error`: (сырой кадр, message из error-объекта).
    error_frame: Option<(String, Option<String>)>,
    /// Первый неотпарсable кадр `data:`.
    unparseable: Option<String>,
}

/// Ядро разбора SSE: тело стрима → `ChatCompletionResult` + сигналы об аномалиях.
/// Выделено ради юнит-тестов: гейты G1–G5 проверяются без поднятия HTTP-сервера.
pub(crate) fn accumulate_sse_body(
    body: &[u8],
    on_delta: &mut impl FnMut(StreamDelta),
) -> (ChatCompletionResult, SseScan) {
    let mut result = ChatCompletionResult::default();
    let mut tool_calls: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
    let mut scan = SseScan::default();
    for line in body.split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(line);
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with("data:") {
            continue;
        }
        let data = trimmed.trim_start_matches("data:").trim();
        if data == "[DONE]" {
            break;
        }
        let value: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => {
                // Молча выбрасывать кадр запрещено (core rules §2.2): фиксируем
                // первый такой кадр сырым текстом, чтобы классификация могла
                // вернуть Err вместо «пустого успеха».
                let raw = raw_frame_of(trimmed);
                log::warn!("SSE 200: кадр стрима не разобран ({}): {}", e, raw);
                if scan.unparseable.is_none() {
                    scan.unparseable = Some(raw);
                }
                continue;
            }
        };
        if value.get("error").is_some() {
            // Провайдер/роутер сообщил об ошибке уже ПОСЛЕ стартового 200
            // (типовой способ для квоты/рейт-лимита). Каждый такой кадр обязан
            // попасть в лог, по нему классифицируется Err.
            let raw = raw_frame_of(trimmed);
            let message = value
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
                .or_else(|| {
                    value
                        .pointer("/error/code")
                        .and_then(|c| c.as_i64())
                        .map(|code| format!("code {code}"))
                });
            log::error!("SSE 200: провайдер вернул error-кадр: {}", raw);
            if scan.error_frame.is_none() {
                scan.error_frame = Some((raw, message));
            }
            continue;
        }
        scan.frames += 1;
        if let Some(reason) = value
            .pointer("/choices/0/finish_reason")
            .and_then(|v| v.as_str())
        {
            result.finish_reason = reason.to_string();
        }
        let content = value
            .pointer("/choices/0/delta/content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let reasoning = value
            .pointer("/choices/0/delta/reasoning_content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !content.is_empty() || !reasoning.is_empty() {
            result.content.push_str(content);
            result.reasoning.push_str(reasoning);
            on_delta(StreamDelta {
                content: content.to_string(),
                reasoning: reasoning.to_string(),
            });
        }
        if let Some(items) = value
            .pointer("/choices/0/delta/tool_calls")
            .and_then(|v| v.as_array())
        {
            for item in items {
                let index = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let entry = tool_calls.entry(index).or_default();
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    if !id.is_empty() {
                        entry.0.push_str(id);
                    }
                }
                if let Some(name) = item.pointer("/function/name").and_then(|v| v.as_str()) {
                    entry.1.push_str(name);
                }
                if let Some(arguments) = item.pointer("/function/arguments").and_then(|v| v.as_str()) {
                    entry.2.push_str(arguments);
                }
            }
        }
    }
    result.tool_calls = tool_calls
        .into_values()
        .map(|(id, name, arguments)| CloudToolCall { id, name, arguments })
        .filter(|call| !call.name.is_empty())
        .collect();
    (result, scan)
}

/// Стриминговый чат (SSE). `on_delta(content)` вызывается на каждую единицу
/// текста; возвращает полный накопленный текст (включая reasoning_content).
pub fn chat_completion_stream(
    base_url: &str,
    api_key: Option<&str>,
    req: &ChatRequest,
    mut on_delta: impl FnMut(&str),
) -> Result<String, String> {
    let mut full = String::new();
    let result = chat_completion_stream_result(base_url, api_key, req, |delta| {
        if !delta.content.is_empty() {
            full.push_str(&delta.content);
        }
        if !delta.reasoning.is_empty() {
            full.push_str(&delta.reasoning);
        }
        if !delta.content.is_empty() {
            on_delta(&delta.content);
        }
        if !delta.reasoning.is_empty() {
            on_delta(&delta.reasoning);
        }
    })
    .map_err(|e| e.to_string())?;
    Ok(if full.is_empty() { result.content } else { full })
}

/// Классификация результата разбора SSE. Единственный источник правил
/// «когда Ok запрещён» — используется и в `chat_completion_stream_result`,
/// и в юнит-тестах (SSOT, §2.1: копий правил быть не должно).
pub(crate) fn classify_sse_result(
    result: ChatCompletionResult,
    scan: SseScan,
) -> Result<ChatCompletionResult, CloudStreamError> {
    // Порядок намеренный: ошибка провайдера сильнее мусора, мусор сильнее
    // пустоты — каждый Err несёт максимально конкретную причину из доступных.
    if let Some((raw_frame, message)) = scan.error_frame {
        return Err(CloudStreamError::InStreamError { raw_frame, message });
    }
    if result.content.is_empty()
        && result.reasoning.is_empty()
        && result.tool_calls.is_empty()
    {
        if let Some(raw_frame) = scan.unparseable {
            return Err(CloudStreamError::UnparseableFrame { raw_frame });
        }
        if result.finish_reason.trim().is_empty() {
            // Гейт G2/G5: «пустой успех» структурно запрещён как Ok —
            // именно он превратил инцидент 18.10.2026 в пять
            // недиагностируемых пустых ответов.
            log::warn!(
                "SSE 200: пустой успех — {} кадр(ов) без дельт, finish_reason отсутствует",
                scan.frames
            );
            return Err(CloudStreamError::EmptySuccess);
        }
        // finish_reason назван (content_filter/length/…): провайдер формально
        // объяснил отсутствие контента. Ok остаётся возможным, но телеметрия
        // обязана это показать.
        log::warn!(
            "SSE 200: дельт контента не было ({} кадр(ов)), finish_reason=\"{}\"",
            scan.frames,
            result.finish_reason
        );
    }
    Ok(result)
}

/// HTTP-слой стрима + финальная классификация.
pub fn chat_completion_stream_result(
    base_url: &str,
    api_key: Option<&str>,
    req: &ChatRequest,
    mut on_delta: impl FnMut(StreamDelta),
) -> Result<ChatCompletionResult, CloudStreamError> {
    if let Err(e) = super::client_error::validate_request_shape(req) {
        log::error!("chat_completion_stream_result: невалидная форма запроса, не отправляю: {e}");
        return Err(e);
    }
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let mut body = req.clone();
    body.stream = true;

    let mut post = client()
        .map_err(|e| CloudStreamError::Http { message: e })?
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(300));
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        post = post.header("Authorization", format!("Bearer {}", key));
    }
    let resp = post.send().map_err(|e| CloudStreamError::Http {
        message: format!("9router /v1/chat/completions: {}", e),
    })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(CloudStreamError::Http {
            message: format!(
                "/v1/chat/completions: HTTP {} {}",
                status, &text[..text.len().min(400)]
            ),
        });
    }

    let bytes = resp.bytes().map_err(|e| CloudStreamError::Http {
        message: format!("9router /v1/chat/completions (стрим не дочитан): {}", e),
    })?;
    let (result, scan) = accumulate_sse_body(&bytes, &mut on_delta);
    classify_sse_result(result, scan)
}

#[cfg(test)]
mod sse_tests {
    use super::*;
    use super::super::client_error::{
        CLOUD_EMPTY_STREAM_MARKER, CLOUD_ERROR_FRAME_MARKER, CLOUD_UNPARSEABLE_FRAME_MARKER,
    };

    fn no_delta(delta: StreamDelta) {
        let _ = delta;
    }

    fn accumulate(body: &str) -> (ChatCompletionResult, SseScan) {
        accumulate_sse_body(body.as_bytes(), &mut no_delta)
    }

    /// G4 (анти-регресс): обычный стрим с контентом и finish_reason — Ok,
    /// текст склеивается по всем дельтам.
    #[test]
    fn normal_stream_with_content_is_ok() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"при\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"вет\"},\"finish_reason\":\"stop\"}]}\n\n\
                    data: [DONE]\n\n";
        let (result, scan) = accumulate(body);
        assert_eq!(result.content, "привет");
        assert_eq!(result.finish_reason, "stop");
        assert_eq!(scan.frames, 2);
        assert!(scan.error_frame.is_none());
        assert!(scan.unparseable.is_none());
    }

    /// G4 (анти-регресс): tool_calls-стрим по-прежнему собирается из осколков.
    #[test]
    fn stream_with_tool_calls_is_ok() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":null}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"emit_signal\",\"arguments\":\"{\\\"key\\\"\"}}]}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"validator_report\\\"}\"}}]}}]}\n\n\
                    data: {\"choices\":[{\"finish_reason\":\"tool_calls\"}]}\n\n\
                    data: [DONE]\n\n";
        let (result, scan) = accumulate(body);
        assert_eq!(result.tool_calls.len(), 1);
        let call = &result.tool_calls[0];
        assert_eq!(call.name, "emit_signal");
        assert_eq!(call.arguments, "{\"key\":\"validator_report\"}");
        assert_eq!(result.finish_reason, "tool_calls");
        assert!(scan.error_frame.is_none());
    }

    /// G1: error-кадр в 200-стриме → Err(InStreamError), Display содержит
    /// message из error-объекта и машиночитаемый маркер.
    #[test]
    fn sse_error_frame_is_classified_as_in_stream_error() {
        let body = "data: {\"error\":{\"message\":\"You exceeded your current quota\",\"code\":429}}\n\n\
                    data: [DONE]\n\n";
        let (result, scan) = accumulate(body);
        assert_eq!(scan.frames, 0, "error-кадр не должен считаться обычным");
        assert!(result.content.is_empty());

        let err = classify_sse_result(result, scan);
        match err {
            Err(e) => {
                let text = e.to_string();
                assert!(text.contains("You exceeded your current quota"), "текст: {text}");
                assert!(text.contains(CLOUD_ERROR_FRAME_MARKER), "текст: {text}");
                assert!(matches!(e, CloudStreamError::InStreamError { .. }), "рен: {e:?}");
            }
            Ok(_) => panic!("error-кадр обязан запретить Ok («пустой успех»)"),
        }
    }

    /// G3: не-JSON кадр при отсутствии других данных → Err(UnparseableFrame)
    /// с сырым кадром и маркером (а не бесшумный Null-JSON).
    #[test]
    fn unparseable_frame_is_classified_when_nothing_recovered() {
        let body = "data: {сломанный json без кавычек}\n\n";
        let (result, scan) = accumulate(body);
        assert!(result.content.is_empty());
        assert!(result.reasoning.is_empty());
        assert!(scan.unparseable.is_some());

        let err = classify_sse_result(result, scan);
        match err {
            Err(e) => {
                let text = e.to_string();
                assert!(text.contains("сломанный json"), "текст: {text}");
                assert!(text.contains(CLOUD_UNPARSEABLE_FRAME_MARKER), "текст: {text}");
            }
            Ok(_) => panic!("мусорный стрим обязан вернуть Err, а не Ok"),
        }
    }

    /// Мусорный кадр при ВОССТАНОВЛЕННОМ контенте не считает весь запрос
    /// провалившимся: Ok, но кадр виден в логе (scan.unparseable заполнен).
    #[test]
    fn unparseable_frame_with_recovered_content_is_ok() {
        let body = "data: %%%мусор%%%\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"ок\"},\"finish_reason\":\"stop\"}]}\n\n";
        let (result, scan) = accumulate(body);
        assert_eq!(result.content, "ок");
        assert!(scan.unparseable.is_some(), "кадр обязан остаться в диагностике");
        assert!(classify_sse_result(result, scan).is_ok());
    }

    /// G2: стрим только с [DONE] / пустое тело → EmptySuccess, Ok запрещён.
    #[test]
    fn empty_stream_is_classified_as_empty_success() {
        for body in ["data: [DONE]\n\n", ""] {
            let (result, scan) = accumulate(body);
            assert_eq!(scan.frames, 0);
            match classify_sse_result(result, scan) {
                Err(e) => {
                    assert!(
                        matches!(e, CloudStreamError::EmptySuccess),
                        "для «{body}» ждём EmptySuccess, получено: {e:?}"
                    );
                    assert!(e.to_string().contains(CLOUD_EMPTY_STREAM_MARKER));
                }
                Ok(_) => panic!("«{body}» обязан вернуть Err («пустой успех» запрещён)"),
            }
        }
    }

    /// G5: инвариант Ok — обязан быть непустой payload: либо контент/reasoning/
    /// tool_calls, либо finish_reason, названный провайдером. Ok со всем
    /// пустым и finish_reason пустым невозможен (конструкционно, через Err).
    #[test]
    fn ok_requires_payload_or_finish_reason() {
        // finish_reason есть, контента нет → Ok разрешён (провайдер назвал причину).
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"length\"}]}\n\n";
        let (result, scan) = accumulate(body);
        assert!(classify_sse_result(result, scan).is_ok(), "finish_reason валидирует Ok");

        // Ни payload, ни finish_reason → Err EmptySuccess.
        match classify_sse_result(ChatCompletionResult::default(), SseScan::default()) {
            Err(CloudStreamError::EmptySuccess) => {}
            other => panic!("ожидался EmptySuccess, получено: {other:?}"),
        }

        // Контент восстановлен + finish_reason отсутствует → Ok валиден.
        let body_ok = "data: {\"choices\":[{\"delta\":{\"content\":\"д\"}}]}\n\n";
        let (r, scan) = accumulate(body_ok);
        assert!(classify_sse_result(r, scan).is_ok());
    }

    /// GP1 (инцидент 20.10.2026): `tool_choice` без `tools[]` — форма запроса
    /// невалидна; запрос до HTTP-слоя не доходит (не уходит на провайдер).
    #[test]
    fn tool_choice_without_tools_is_rejected() {
        let mut req = ChatRequest::default();
        req.model = "test".into();
        req.messages = vec![ChatMessage {
            role: "user".into(),
            content: serde_json::Value::String("привет".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        req.tool_choice = Some("required".into());

        assert!(super::super::client_error::validate_request_shape(&req).is_err());

        match super::super::client_error::validate_request_shape(&req) {
            Err(e) => {
                let text = e.to_string();
                assert!(text.contains("tool_choice"), "текст: {text}");
                assert!(text.contains("required"), "текст: {text}");
                assert!(
                    matches!(e, CloudStreamError::InvalidRequest { .. }),
                    "вариант: {e:?}"
                );
            }
            Ok(()) => panic!("невалидная форма обязана быть ошибкой"),
        }
    }

    /// GP1: `tools: []` — та же невалидность (пустые тулы аргумента не меняют).
    #[test]
    fn tool_choice_with_empty_tools_is_rejected() {
        let req = ChatRequest {
            model: "test".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: serde_json::Value::String("привет".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: Some(vec![]),
            tool_choice: Some("required".into()),
            ..Default::default()
        };
        assert!(super::super::client_error::validate_request_shape(&req).is_err());
    }

    /// GP1 (анти-регресс): нормальная форма (tools+choice / choice без тулов) не ломается.
    #[test]
    fn tool_choice_with_tools_is_valid() {
        let req_with_tools = ChatRequest {
            model: "test".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: serde_json::Value::String("привет".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: Some(vec![serde_json::json!({
                "type": "function",
                "function": {"name": "emit_signal", "parameters": {"type": "object", "properties": {}, "required": []}}
            })]),
            tool_choice: Some("auto".into()),
            ..Default::default()
        };
        assert!(super::super::client_error::validate_request_shape(&req_with_tools).is_ok());

        // choice отсутствует + tools нет — тоже валидная форма (обычный чат).
        let plain = ChatRequest {
            model: "test".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: serde_json::Value::String("привет".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            ..Default::default()
        };
        assert!(super::super::client_error::validate_request_shape(&plain).is_ok());
    }
}


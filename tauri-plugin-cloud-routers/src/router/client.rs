//! HTTP-API 9router (localhost) — боевой чат `/v1/chat/completions`.
//!
//! Служебные GET (models/combos/health) — в `combos.rs` (блокирующие, 4-10 с).
//!
//! Боевой чат — на async reqwest (перевод 21.10.2026, см.
//! `tasks/21.10.26 Таймаут облачных LLM (5→15 минут и честная ошибка).md`):
//! - **порог тишины** вместо общего дедлайна (`read_timeout`): живая очередь
//!   провайдера отзывается хоть байтами и может длиться сколько угодно; полная
//!   тишина дольше порога = мёртвое соединение → честный `TimedOut` с маркером
//!   `[cloud_timeout]` (общий request-timeout мучил всех «ошибкой на ровно
//!   300.0с»);
//! - **живой стрим**: тело читается `chunk()`-ами, SSE-редуктор (`super::sse`)
//!   отдаёт дельты вживую, а не после всего тела;
//! - **отмена**: `cancel_flag` опрашивается каждые 250 мс (`tokio::select!`),
//!   кнопка «Стоп» рвёт запрос → `Cancelled` с маркером `[cloud_cancelled]`;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

use super::client_error::{CloudStreamError, CLOUD_ERROR_FRAME_MARKER};
use super::sse::{SseReducer, SseScan};

/// Коннект к шлюзу: localhost — 15 с с запасом; мёртвый порт не вешает чат.
pub const CHAT_CONNECT_TIMEOUT_SECS: u64 = 15;
/// Порог тишины: ни одного байта от шлюза дольше этого — соединение мертво.
/// Обязан быть больше idle-лимитов нижних слоёв (READ_TIMEOUT шлюза 180 с) с
/// запасом: 600 с покрывает очереди, после которых шлюз уже бы отмер.
pub const CHAT_IDLE_READ_TIMEOUT_SECS: u64 = 600;
/// Поллинг cancel_flag: отклик «Стоп» — до четверти секунды.
pub const CANCEL_POLL_INTERVAL_MS: u64 = 250;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Value>>,
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
    pub tools: Option<Vec<Value>>,
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

// ── Runtime-мост (блокирующий вызов асинхронного кода) ─────────────────

/// Запуск async-движка из блокирующего контекста; эталон —
/// `tauri-plugin-downloader/src/engine/api.rs` (`download_blocking_inner`).
/// Уже в tokio-контексте — `block_in_place`, иначе — собственный
/// многопоточный рантайм (хост вызHKа всегда из sync-потока).
fn block_on<F, T>(fut: F) -> Result<T, CloudStreamError>
where
    F: std::future::Future<Output = Result<T, CloudStreamError>>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(h) => tokio::task::block_in_place(|| h.block_on(fut)),
        Err(_) => match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(rt) => rt.block_on(fut),
            Err(e) => Err(CloudStreamError::Http {
                message: format!("Не удалось создать tokio-рантайм: {e}"),
            }),
        },
    }
}

/// Клиент чата: connect + порог тишины. ОБЩЕГО дедлайна НЕТ.
fn build_chat_client(
    connect_secs: u64,
    idle_secs: u64,
) -> Result<reqwest::Client, CloudStreamError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(connect_secs))
        .read_timeout(Duration::from_secs(idle_secs))
        .build()
        .map_err(|e| CloudStreamError::Http { message: format!("HTTP-клиент: {e}") })
}

/// Ждать отмену: фьючерс, который завершается сразу как flag станет true.
async fn cancel_watcher(flag: &Arc<AtomicBool>) {
    let mut interval =
        tokio::time::interval(Duration::from_millis(CANCEL_POLL_INTERVAL_MS));
    interval.tick().await; // первый тик мгновенный
    loop {
        interval.tick().await;
        if flag.load(Ordering::SeqCst) {
            return;
        }
    }
}

fn map_send_result(
    r: Result<reqwest::Response, reqwest::Error>,
    idle_secs: u64,
) -> Result<reqwest::Response, CloudStreamError> {
    match r {
        Ok(resp) => Ok(resp),
        Err(e) if e.is_timeout() => {
            Err(CloudStreamError::TimedOut { idle_secs, phase: "headers" })
        }
        Err(e) => Err(CloudStreamError::Http {
            message: format!("9router /v1/chat/completions: {e}"),
        }),
    }
}

// ── Ядро стрима (асинхронное) ──────────────────────────────────────────

/// Сердце стрима: живой разбор SSE с отдачей дельт через колбэк.
#[allow(clippy::too_many_arguments)]
async fn stream_engine(
    base_url: &str,
    api_key: Option<&str>,
    req: &ChatRequest,
    on_delta: &mut (dyn FnMut(StreamDelta) + '_),
    cancel: Option<Arc<AtomicBool>>,
    connect_secs: u64,
    idle_secs: u64,
) -> Result<ChatCompletionResult, CloudStreamError> {
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let mut body = req.clone();
    body.stream = true;

    let client = build_chat_client(connect_secs, idle_secs)?;
    let mut post = client.post(&url).json(&body);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        post = post.bearer_auth(key);
    }

    // уже отменён ещё до отправки — не шлём
    let cancel = cancel.filter(|f| !f.load(Ordering::SeqCst));

    // 1) Фаза заголовков (в том числе очередь провайдера):
    let send_fut = post.send();
    tokio::pin!(send_fut);
    let mut watcher = cancel.as_ref().map(|f| Box::pin(cancel_watcher(f)));
    let resp = match watcher.as_mut() {
        Some(w) => tokio::select! {
            biased;
            _ = w.as_mut() => return Err(CloudStreamError::Cancelled { phase: "headers" }),
            r = send_fut.as_mut() => map_send_result(r, idle_secs)?,
        },
        None => map_send_result(send_fut.await, idle_secs)?,
    };

    // 2) Фаза тела: живой стрим по чанкам + отмена + порог тишины.
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(CloudStreamError::Http {
            message: format!(
                "/v1/chat/completions: HTTP {} {}",
                status,
                &text[..text.len().min(400)]
            ),
        });
    }

    let mut resp = resp;
    let mut watcher = cancel.as_ref().map(|f| Box::pin(cancel_watcher(f)));
    let mut reducer = SseReducer::new();
    loop {
        let chunk_fut = resp.chunk();
        tokio::pin!(chunk_fut);
        let chunk = match watcher.as_mut() {
            Some(w) => tokio::select! {
                biased;
                _ = w.as_mut() => return Err(CloudStreamError::Cancelled { phase: "body" }),
                c = chunk_fut.as_mut() => c,
            },
            None => chunk_fut.await,
        };
        match chunk {
            Ok(Some(bytes)) => {
                reducer.feed(&bytes, on_delta);
            }
            Ok(None) => {
                let (result, scan) = reducer.finish(on_delta);
                return classify_sse_result(result, scan);
            }
            Err(e) if e.is_timeout() => {
                return Err(CloudStreamError::TimedOut { idle_secs, phase: "body" });
            }
            Err(e) => {
                return Err(CloudStreamError::Http {
                    message: format!(
                        "9router /v1/chat/completions ({}): {e}",
                        super::client_error::STREAM_INTERRUPTED_PHRASE
                    ),
                });
            }
        }
    }
}

/// Публичное ядро стрима с отменой — вариант для хоза (кнопка «Стоп»).
pub fn chat_completion_stream_result_cancellable(
    base_url: &str,
    api_key: Option<&str>,
    req: &ChatRequest,
    on_delta: impl FnMut(StreamDelta),
    cancel: Option<Arc<AtomicBool>>,
) -> Result<ChatCompletionResult, CloudStreamError> {
    chat_completion_stream_result_limited(
        base_url,
        api_key,
        req,
        on_delta,
        cancel,
        CHAT_CONNECT_TIMEOUT_SECS,
        CHAT_IDLE_READ_TIMEOUT_SECS,
    )
}

/// Внутренняя: с постоянными уровнями, чтобы тесты могли сузить порог тишины.
#[allow(clippy::too_many_arguments)]
pub(crate) fn chat_completion_stream_result_limited(
    base_url: &str,
    api_key: Option<&str>,
    req: &ChatRequest,
    on_delta: impl FnMut(StreamDelta),
    cancel: Option<Arc<AtomicBool>>,
    connect_secs: u64,
    idle_secs: u64,
) -> Result<ChatCompletionResult, CloudStreamError> {
    if let Err(e) = super::client_error::validate_request_shape(req) {
        log::error!("chat_completion_stream_result: невалидная форма запроса, не отправляю: {e}");
        return Err(e);
    }
    let mut on_delta = on_delta;
    block_on(stream_engine(
        base_url,
        api_key,
        req,
        &mut on_delta,
        cancel,
        connect_secs,
        idle_secs,
    ))
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

/// Стриминговый чат прежней сигнатуры: возвращает собранный результат.
pub fn chat_completion_stream_result(
    base_url: &str,
    api_key: Option<&str>,
    req: &ChatRequest,
    on_delta: impl FnMut(StreamDelta),
) -> Result<ChatCompletionResult, CloudStreamError> {
    chat_completion_stream_result_cancellable(base_url, api_key, req, on_delta, None)
}

// ── Не-стриминговый чат (оставлен для совместимости) ──────────────────

/// Полный (не-стриминговый) ответ чата: `choices[0].message.content`.
///
/// Отмена работает только ДО отправки (флаг проверен перед вызовом ядра);
/// для полноценной отмены по кнопке «Стоп» используйте стрим
/// (`chat_completion_stream_result_cancellable`).
pub fn chat_completion_nonstream(
    base_url: &str,
    api_key: Option<&str>,
    req: &ChatRequest,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<String, String> {
    if let Err(e) = super::client_error::validate_request_shape(req) {
        log::error!("chat_completion_nonstream: невалидная форма запроса, не отправляю: {e}");
        return Err(e.to_string());
    }
    if let Some(f) = cancel.as_ref() {
        if f.load(Ordering::SeqCst) {
            log::info!("chat_completion_nonstream: запрос отменён до отправки");
            return Err(
                CloudStreamError::Cancelled { phase: "headers" }.to_string()
            );
        }
    }
    let base = base_url.to_string();
    let req = req.clone();
    block_on(async_engine_nonstream(
        base,
        api_key.map(|s| s.to_string()),
        req,
    ))
    .map_err(|e| e.to_string())
}

/// Не-стриминговое ядро: как стрим, но тело собирается в буфер и парсится JSON.
async fn async_engine_nonstream(
    base_url: String,
    api_key: Option<String>,
    req: ChatRequest,
) -> Result<String, CloudStreamError> {
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let mut body = req.clone();
    body.stream = false;

    let client = build_chat_client(CHAT_CONNECT_TIMEOUT_SECS, CHAT_IDLE_READ_TIMEOUT_SECS)?;
    let mut post = client.post(&url).json(&body);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        post = post.bearer_auth(key);
    }
    let resp = map_send_result(post.send().await, CHAT_IDLE_READ_TIMEOUT_SECS)?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(CloudStreamError::Http {
            message: format!(
                "/v1/chat/completions: HTTP {} {}",
                status,
                &text[..text.len().min(400)]
            ),
        });
    }
    let status = resp.status();
    let mut buf: Vec<u8> = Vec::new();
    let mut resp = resp;
    loop {
        match resp.chunk().await {
            Ok(Some(bytes)) => buf.extend_from_slice(&bytes),
            Ok(None) => break,
            Err(e) if e.is_timeout() => {
                return Err(CloudStreamError::TimedOut {
                    idle_secs: CHAT_IDLE_READ_TIMEOUT_SECS,
                    phase: "body",
                });
            }
            Err(e) => {
                return Err(CloudStreamError::Http {
                    message: format!("9router /v1/chat/completions (ответ не дочитан): {e}"),
                });
            }
        }
    }
    let v: serde_json::Value = serde_json::from_slice(&buf)
        .map_err(|e| CloudStreamError::Http { message: format!("Bad JSON ответа: {e}") })?;
    // 200 с телом-ошибкой (роутеры сообщают о квоте/рейт-лимите таким видом):
    // молча выкидывать запрещено — честная классификация вместо
    // «не содержит choices[0].message.content».
    if let Some(err) = v.get("error") {
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .map(str::to_string)
            .unwrap_or_else(||
                serde_json::to_string(err).unwrap_or_else(|_| "<неудобочитаемая ошибка>".into())
            );
        log::error!("chat_completion_nonstream: HTTP {status}, тело вернуло error-объект: {message}");
        return Err(CloudStreamError::Http {
            message: format!(
                "Облако вернуло ошибку (HTTP {status}): {message} {CLOUD_ERROR_FRAME_MARKER}"
            ),
        });
    }
    v.pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            CloudStreamError::Http {
                message: "Ответ 9router не содержит choices[0].message.content".into(),
            }
        })
}

// ── Единый классификатор («когда Ok запрещён») ────────────────────────

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
    let has_payload = !result.content.is_empty()
        || !result.reasoning.is_empty()
        || !result.tool_calls.is_empty();
    if !has_payload {
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
            return Err(CloudStreamError::EmptySuccess { frames: scan.frames });
        }
        // finish_reason назван (content_filter/length/…): провайдер формально
        // объяснил отсутствие контента. Ok остаётся возможным, но телеметрия
        // обязана это показать.
        log::warn!(
            "SSE 200: дельт контента не было ({} кадр(ов)), finish_reason=\"{}\"",
            scan.frames,
            result.finish_reason
        );
        return Ok(result);
    }
    // Контент есть. Обрыв = стрим закрылся, не доложив признака конца: ни
    // `[DONE]`, ни finish_reason (инцидент 25.10.2026: terminated-стримы
    // роутера). Тихо обрезанный Ok запрещён — downstream получает либо
    // полный ответ, либо Err. `[DONE]` при пустом finish_reason остаётся
    // валидным концом: часть провайдеров не шлёт finish_reason вовсе.
    if !scan.done_seen && result.finish_reason.trim().is_empty() {
        log::warn!(
            "SSE 200: стрим оборван — контент пришёл частично ({} кадр(ов)), ни [DONE], ни finish_reason",
            scan.frames
        );
        return Err(CloudStreamError::Truncated {
            partial_chars: result.content.chars().count() + result.reasoning.chars().count(),
        });
    }
    Ok(result)
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    /// Инвариант порогов: коннект короче тишины; тишина ≥ 2× шлюзового
    /// idle (180с) с запасом — чтобы живая очередь никогда не попала
    /// под локальный отказ раньше отвала нижнего слоя.
    #[test]
    fn timeout_constants_have_sane_ratio() {
        assert!(
            CHAT_CONNECT_TIMEOUT_SECS < CHAT_IDLE_READ_TIMEOUT_SECS,
            "коннект ({CHAT_CONNECT_TIMEOUT_SECS}с) обязан быть короче тишины ({}c)",
            CHAT_IDLE_READ_TIMEOUT_SECS
        );
        assert!(CHAT_CONNECT_TIMEOUT_SECS >= 5, "слишком быстрый коннект — рано подлый отказ");
        assert!(
            CHAT_IDLE_READ_TIMEOUT_SECS >= 360,
            "порог тишины {}с слишком мал для живых очередей",
            CHAT_IDLE_READ_TIMEOUT_SECS
        );
    }

    fn req_for(addr: std::net::SocketAddr) -> ChatRequest {
        let mut req = ChatRequest::default();
        req.model = "test".into();
        req.messages = vec![ChatMessage {
            role: "user".into(),
            content: serde_json::Value::String("привет".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let _ = addr;
        req
    }

    fn addr_of(listener: &TcpListener) -> std::net::SocketAddr {
        listener.local_addr().expect("local addr")
    }

    /// Заглушка: принять одно соединение и исполнить сценарий в отдельном потоке.
    fn stub(handler: impl FnOnce(TcpStream) + Send + 'static) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = addr_of(&listener);
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                handler(stream);
            }
        });
        addr
    }

    fn read_request(stream: &mut TcpStream) {
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf); // затвердеть до прочитки заголовков запроса
    }

    /// G3: заглушка принимает запрос и МОЛЧИТ — соединение живо, но ни
    /// одного байта ответа. Движок обязан вернуть TimedOut (phase=headers)
    /// с маркером [cloud_timeout] за счёт короткого порога тишины инъекции.
    #[test]
    fn silent_stub_yields_timed_out_headers() {
        let addr = stub(|mut stream| {
            read_request(&mut stream);
            std::thread::sleep(Duration::from_secs(5));
            // дальше ничего не пишем
        });
        let mut req = req_for(addr);
        req.stream = true;
        let base = format!("http://{}", addr);

        let started = std::time::Instant::now();
        let err = chat_completion_stream_result_limited(
            &base,
            None,
            &req,
            |_| {},
            None,
            2, // connect
            1, // порог тишины (инъекция для теста)
        )
        .expect_err("полная тишина обязана вернуть TimedOut");
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(4),
            "ожидание обязано прерваться порогом тишины (~1с), заняло {elapsed:?}"
        );
        let text = err.to_string();
        assert!(text.contains("[cloud_timeout]"), "текст: {text}");
        match err {
            CloudStreamError::TimedOut { phase, .. } => assert_eq!(phase, "headers"),
            other => panic!("ожидался TimedOut, получено: {other:?}"),
        }
    }

    /// G3: заглушка шлёт keep-alive-комментарии (": ping") раз в 300 мс
    /// в течение ~1.8 с, затем отдаёт нормальный кадр контента и закрывает
    /// стрим. Порог тишины 2 с: каждый байт живого потока СБРАСЫВАЕТ порог —
    /// запрос обязан завершиться Ok с контентом (а не TimedOut).
    #[test]
    fn keep_alive_stream_survives_silence_threshold() {
        let addr = stub(|mut stream| {
            read_request(&mut stream);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            );
            let _ = stream.flush();
            for _ in 0..6 {
                std::thread::sleep(Duration::from_millis(300));
                let _ = stream.write_all(b": ping\n\n");
                let _ = stream.flush();
            }
            let final_frame = "data: {\"choices\":[{\"delta\":{\"content\":\"конец\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            let _ = stream.write_all(final_frame.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Both);
        });
        let mut req = req_for(addr);
        req.stream = true;
        let base = format!("http://{}", addr);

        let started = std::time::Instant::now();
        let result = chat_completion_stream_result_limited(
            &base,
            None,
            &req,
            |_| {},
            None,
            5, // connect — с запасом
            2, // порог тишины 2 с: ping-и держат соединение живым
        );
        let elapsed = started.elapsed();

        let result = result.expect("живой поток обязан завершиться Ok");
        assert_eq!(result.content, "конец");
        assert_eq!(result.finish_reason, "stop");
        assert!(
            elapsed >= Duration::from_millis(1600),
            "стрим обязан длиться все ~1.8 с пингов: {elapsed:?}"
        );
    }

    /// G3: пользователь жмёт «Стоп» после первой дельты — запрос обязан
    /// вернуться как Cancelled (phase=body) быстро (≤4 c), а не дожидаться
    /// ни таймаута, ни конца стрима (30 кадров × 400 мс = 12 c).
    #[test]
    fn cancel_during_stream_returns_cancelled() {
        let addr = stub(|mut stream| {
            read_request(&mut stream);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            );
            let _ = stream.flush();
            for _ in 0..30 {
                std::thread::sleep(Duration::from_millis(400));
                let _ = stream.write_all(
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                );
                let _ = stream.flush();
            }
        });
        let mut req = req_for(addr);
        req.stream = true;
        let base = format!("http://{}", addr);

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_tap = cancel.clone();
        let started = std::time::Instant::now();
        let err = chat_completion_stream_result_limited(
            &base,
            None,
            &req,
            move |_delta| {
                // первая дельта дошла вживую — эмулируем нажатие «Стоп»
                cancel_tap.store(true, Ordering::SeqCst);
            },
            Some(cancel),
            5,  // connect
            10, // порог тишины много больше длительности сценария
        )
        .expect_err("после отмены обязан быть Err(Cancelled), а не Ok");
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(4),
            "отмена обязана сработать за секунды (поллинг 250 мс): {elapsed:?}"
        );
        let text = err.to_string();
        assert!(text.contains("Прервано пользователем"), "текст: {text}");
        assert!(text.contains("[cloud_cancelled]"), "текст: {text}");
        match err {
            CloudStreamError::Cancelled { phase } => assert_eq!(phase, "body"),
            other => panic!("ожидался Cancelled, получено: {other:?}"),
        }
    }
}

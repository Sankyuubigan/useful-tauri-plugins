//! Kiro / Amazon Q: адаптер не-OpenAI протокола.
//!
//! Спека — `docs/kiro-protocol.md`, каждый пункт со статусом доказательства.
//! Здесь только то, что подтверждено кодом 9Router; живые shape ответов AWS —
//! UNVERIFIED до зонда с логином пользователя.
//!
//! ## Что умеет адаптер
//!
//! - `models_request`: `GET q.{region}/ListAvailableModels?origin=AI_EDITOR[&profileArn=]`
//!   (+ `TokenType: API_KEY` для API-ключа), строгий разбор `models[]`;
//! - `chat_request`: `POST runtime.{region}.kiro.dev/generateAssistantResponse`
//!   (endpoints-порядок и заголовки — из 9Router), тело `conversationState`;
//! - `is_stream_response`: бинарный EventStream (`application/vnd.amazon.eventstream`).
//!
//! EventStream→SSE-декодер и OAuth Device Flow — следующие файлы этого шага,
//! здесь только каркас endpoints/заголовков/тел, чтобы шаги ревьюировались
//! по одному.

use axum::http::{HeaderMap, HeaderName, HeaderValue};
use cloud_routers_core::config::{DiscoveredModel, GatewayConfig, ProviderConfig};

use crate::protocol::{UpstreamRequest, VendorProtocol};
use crate::upstream::Route;

/// Регион по умолчанию. Совпадает с 9Router (`us-east-1` везде как дефолт).
pub const DEFAULT_REGION: &str = "us-east-1";

/// Порядок endpoints инференса (подтверждено: `transport.baseUrls` 9Router).
pub const CHAT_ENDPOINTS: &[&str] = &[
    "https://runtime.us-east-1.kiro.dev/generateAssistantResponse",
    "https://codewhisperer.us-east-1.amazonaws.com/generateAssistantResponse",
    "https://q.us-east-1.amazonaws.com/generateAssistantResponse",
];

/// Статусы, при которых пробуем следующий endpoint (подтверждено:
/// `KIRO_ENDPOINT_FALLBACK_STATUSES` = 401/403/404).
pub const ENDPOINT_FALLBACK_STATUSES: &[u16] = &[401, 403, 404];

/// Паттерн AWS-региона (подтверждено: `AWS_REGION_PATTERN` 9Router).
/// Защита от SSRF через подстановку региона в URL.
pub fn valid_region(region: &str) -> bool {
    let b = region.as_bytes();
    let mut parts = region.split('-');
    let (Some(p1), Some(p2), Some(p3), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    b.len() >= 9
        && b.len() <= 14
        && p1.len() == 2
        && p1.bytes().all(|c| c.is_ascii_lowercase())
        && !p2.is_empty()
        && p2.bytes().all(|c| c.is_ascii_lowercase())
        && (p3.len() == 1 || p3.len() == 2)
        && p3.bytes().all(|c| c.is_ascii_digit())
}

/// Собрать URL каталога моделей Kiro.
///
/// `profile_arn` — для OAuth (query-параметр, подтверждено `kiroModels.js`);
/// для API-ключа профиль не передаётся (ListAvailableProfiles его не отдаёт).
pub fn models_url(region: &str, profile_arn: Option<&str>) -> Result<String, String> {
    if !valid_region(region) {
        return Err(format!("некорректный AWS-регион «{region}»"));
    }
    let mut url = format!("https://q.{region}.amazonaws.com/ListAvailableModels?origin=AI_EDITOR");
    if let Some(arn) = profile_arn.map(str::trim).filter(|a| !a.is_empty()) {
        url.push_str("&profileArn=");
        url.push_str(&urlencoding(arn));
    }
    Ok(url)
}

/// Процент-энкодинг для query-параметра (без внешних зависимостей).
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub struct KiroProtocol;

impl KiroProtocol {
    fn headers(
        credential: &str,
        api_key_mode: bool,
        profile_arn: Option<&str>,
    ) -> Result<HeaderMap, String> {
        let mut h = HeaderMap::new();
        let put = |h: &mut HeaderMap, k: &'static str, v: &str| -> Result<(), String> {
            let hv = HeaderValue::from_str(v)
                .map_err(|_| format!("недопустимое значение заголовка {k}"))?;
            h.insert(HeaderName::from_static(k), hv);
            Ok(())
        };
        // Порядок и набор — из `buildHeaders` 9Router.
        put(&mut h, "content-type", "application/json")?;
        put(&mut h, "accept", "application/vnd.amazon.eventstream")?;
        put(&mut h, "authorization", &format!("Bearer {}", credential.trim()))?;
        if api_key_mode {
            put(&mut h, "tokentype", "API_KEY")?;
        } else {
            put(&mut h, "x-amz-sso-bearer", credential.trim())?;
        }
        put(&mut h, "x-amzn-kiro-agent-mode", "spec")?;
        put(&mut h, "x-amzn-codewhisperer-machine-id", "kiro-desktop")?;
        if let Some(arn) = profile_arn.map(str::trim).filter(|a| !a.is_empty()) {
            put(&mut h, "x-amzn-codewhisperer-profile-arn", arn)?;
        }
        put(&mut h, "user-agent", "AWS-SDK-JS/3.0.0 kiro-ide/1.0.0")?;
        put(&mut h, "x-amz-user-agent", "aws-sdk-js/3.0.0 kiro-ide/1.0.0")?;
        Ok(h)
    }
}

impl VendorProtocol for KiroProtocol {
    fn name(&self) -> &'static str {
        "kiro"
    }

    fn models_request(
        &self,
        provider: &ProviderConfig,
        config: &GatewayConfig,
        credential: &str,
    ) -> Result<UpstreamRequest, String> {
        let region = kiro_region(provider);
        let profile_arn = kiro_profile_arn(provider);
        let url = models_url(&region, profile_arn.as_deref())?;
        let api_key_mode = is_api_key_mode(provider);
        let mut headers = Self::headers(credential, api_key_mode, profile_arn.as_deref())?;
        // Релей: absolute target + заголовки поверх (та же схема, что OpenAI).
        let route = if provider.relay_id.is_some() {
            // Единый источник правила боковой настройки (проверка «настроен,
            // но без схемы» живёт в relay_route_base, а не в копиях).
            let relay = crate::upstream::relay_route_base(provider, config)?;
            let route = Route::Relay {
                url: format!("{relay}{}/ListAvailableModels", crate::upstream::RELAY_PATH_PREFIX),
                target_url: url,
                token: crate::upstream::relay_route_token(provider, config)?,
            };
            crate::upstream::apply_route_headers(&route, &mut headers);
            route
        } else {
            Route::Direct { url }
        };
        Ok(UpstreamRequest { route, method: reqwest::Method::GET, headers, body: Vec::new() })
    }

    fn parse_models(&self, body: &str) -> Result<Vec<DiscoveredModel>, String> {
        parse_kiro_models(body)
    }

    fn chat_request(
        &self,
        provider: &ProviderConfig,
        config: &GatewayConfig,
        credential: &str,
        _client_headers: &HeaderMap,
        payload: &serde_json::Value,
        upstream_model: &str,
    ) -> Result<UpstreamRequest, String> {
        let body = build_conversation(provider, payload, upstream_model)?;
        let bytes = serde_json::to_vec(&body)
            .map_err(|e| format!("Не удалось сериализовать тело Kiro-запроса: {e}"))?;
        let first = CHAT_ENDPOINTS[0].to_string();
        let (route, target) = if provider.relay_id.is_some() {
            let relay = crate::upstream::relay_route_base(provider, config)?;
            let target = first.clone();
            let route = Route::Relay {
                url: format!(
                    "{relay}{}/generateAssistantResponse",
                    crate::upstream::RELAY_PATH_PREFIX
                ),
                    target_url: target.clone(),
                    token: crate::upstream::relay_route_token(provider, config)?,
            };
            (route, target)
        } else {
            (Route::Direct { url: first.clone() }, first)
        };
        let mut headers =
            Self::headers(credential, is_api_key_mode(provider), kiro_profile_arn(provider).as_deref())?;
        if target.contains("://codewhisperer.") {
            headers.insert(
                HeaderName::from_static("x-amz-target"),
                HeaderValue::from_static("AmazonCodeWhispererStreamingService.GenerateAssistantResponse"),
            );
        }
        crate::upstream::apply_route_headers(&route, &mut headers);
        headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/json"),
        );
        Ok(UpstreamRequest { route, method: reqwest::Method::POST, headers, body: bytes })
    }

    fn is_stream_response(&self, content_type: &str) -> bool {
        content_type.starts_with("application/vnd.amazon.eventstream")
    }
}

/// Регион Kiro-провайдера: из OAuth-сессии провайдера недоступна здесь
/// (у адаптера нет диска), поэтому регион — из `base_url`-подсказки вида
/// `https://q.<region>.amazonaws.com` либо дефолт. Точное место — шаг 4:
/// регион хранится в сессии, сюда приедет через credential-контекст.
fn kiro_region(provider: &ProviderConfig) -> String {
    for prefix in ["https://q.", "http://q."] {
        if let Some(rest) = provider.base_url.trim().strip_prefix(prefix) {
            let region = rest.split('.').next().unwrap_or("").to_string();
            if valid_region(&region) {
                return region;
            }
        }
    }
    DEFAULT_REGION.to_string()
}

/// API-key режим: у провайдера есть ключи (а не OAuth-сессия).
fn is_api_key_mode(provider: &ProviderConfig) -> bool {
    matches!(
        provider.auth,
        cloud_routers_core::protocol::ProviderAuth::ApiKeys
    ) && provider.active_keys().next().is_some()
}

/// profileArn: из base_url-подсказки не извлекается; появится шагом 4 из сессии.
/// Пока — только явное поле провайдера через `extra` конфига? Нет: честно None.
fn kiro_profile_arn(_provider: &ProviderConfig) -> Option<String> {
    // Шаг 4: profileArn живёт в OAuth-сессии. Сюда он приедет как часть
    // credential-контекста, а не парсингом строк. Заглушка None — честное
    // «пока нет», а не угадывание.
    None
}

/// Разобрать `ListAvailableModels` / CodeWhisperer-ответ в модели.
///
/// Строгий разбор: записи без строкового id модели отбрасываются, пустой итог —
/// ошибка (молчаливый пустой список означал бы «у провайдера нет моделей»,
/// хотя на деле сломался ключ).
pub fn parse_kiro_models(body: &str) -> Result<Vec<DiscoveredModel>, String> {
    let root: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Ответ Kiro не является JSON: {e}"))?;
    let arr = root
        .get("models")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "В ответе Kiro нет массива \"models\"".to_string())?;
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for item in arr {
        let id = item
            .get("modelId")
            .or_else(|| item.get("id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(id) = id else { continue };
        if seen.iter().any(|s| s == id) {
            continue;
        }
        seen.push(id.to_string());
        out.push(DiscoveredModel {
            id: id.to_string(),
            object: Some("model".to_string()),
            created: None,
        });
    }
    if out.is_empty() {
        return Err("Kiro вернул пустой список моделей".to_string());
    }
    Ok(out)
}

/// Построить `conversationState` из OpenAI-тела (подтверждено:
/// `openai-to-kiro.js`).
///
/// Правила перевода (минимальный честный набор шага 3):
/// - `system` → `user` (Kiro не имеет system-роли в history);
/// - `tool` → `user` с `toolResults`;
/// - `assistant.tool_calls` → `toolUses`;
/// - `images` → опущены шагом 3 с явным предупреждением в журнал? Нет:
///   молчаливая потеря картинок — ложь. Шаг 3 принимает только текст+tools;
///   запись с картинками — ошибка с понятным текстом.
pub fn build_conversation(
    provider: &ProviderConfig,
    payload: &serde_json::Value,
    upstream_model: &str,
) -> Result<serde_json::Value, String> {
    let obj = payload
        .as_object()
        .ok_or_else(|| "Тело запроса должно быть JSON-объектом".to_string())?;
    let messages = obj
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "В теле нет массива \"messages\"".to_string())?;
    if messages.is_empty() {
        return Err("Массив \"messages\" пуст".to_string());
    }

    let mut history: Vec<serde_json::Value> = Vec::new();
    for m in messages {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = m.get("content");
        // Шаг 3: картинки запрещены явно, а не молча выкидываются.
        if content_has_images(content) {
            return Err(
                "Kiro-адаптер шага 3 не поддерживает картинки: уберите images из запроса".to_string(),
            );
        }
        let text = message_text(content);
        match role {
            "system" | "user" | "tool" => {
                let mut user_msg = serde_json::json!({
                    "content": text,
                    "modelId": upstream_model,
                    "origin": "AI_EDITOR",
                });
                if role == "tool" {
                    let tool_results = tool_results_of(m);
                    user_msg["userInputMessageContext"] =
                        serde_json::json!({ "toolResults": tool_results });
                }
                history.push(serde_json::json!({ "userInputMessage": user_msg }));
            }
            "assistant" => {
                let mut asst = serde_json::json!({ "content": text });
                let tool_uses = tool_uses_of(m);
                if !tool_uses.is_empty() {
                    asst["toolUses"] = serde_json::Value::Array(tool_uses);
                }
                history.push(serde_json::json!({ "assistantResponseMessage": asst }));
            }
            other => {
                return Err(format!(
                    "Kiro-адаптер не знает роль «{other}»: допустимы system/user/assistant/tool"
                ));
            }
        }
    }
    // Последнее user-сообщение — currentMessage, остальное — history.
    let current_idx = history.iter().rposition(|h| h.get("userInputMessage").is_some()).ok_or_else(|| {
        "В диалоге нет ни одного user/system/tool сообщения для currentMessage".to_string()
    })?;
    let current = history.remove(current_idx);
    let current_message = current.get("userInputMessage").cloned().unwrap_or(serde_json::json!({}));

    let mut conversation = serde_json::json!({
        "conversationState": {
            "chatTriggerType": "MANUAL",
            "conversationId": conversation_id(provider, upstream_model),
            "currentMessage": { "userInputMessage": current_message },
            "history": history,
        }
    });
    if let Some(arn) = kiro_profile_arn(provider) {
        conversation["profileArn"] = serde_json::Value::String(arn);
    }
    // max_tokens/temperature — только если клиент их прислал (не выдумываем).
    let mut inference = serde_json::Map::new();
    if let Some(mt) = obj.get("max_tokens").or_else(|| obj.get("max_completion_tokens")) {
        inference.insert("maxTokens".to_string(), mt.clone());
    }
    if let Some(t) = obj.get("temperature") {
        inference.insert("temperature".to_string(), t.clone());
    }
    if !inference.is_empty() {
        conversation["inferenceConfig"] = serde_json::Value::Object(inference);
    }
    Ok(conversation)
}

fn conversation_id(provider: &ProviderConfig, model: &str) -> String {
    // Уникальный id диалога на запрос: счётчик процесса + наносекунды.
    // Без внешних зависимостей (uuid тащить ради одной строки запрещено
    // минимализмом). Коллизия двух запросов в одну наносекунду невозможна
    // из-за счётчика; рестарт процесса не страшен — время уже другое.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}-{}-{}", provider.id, model, nanos, n)
}

fn content_has_images(content: Option<&serde_json::Value>) -> bool {
    match content {
        Some(serde_json::Value::Array(parts)) => parts.iter().any(|p| {
            p.get("type").and_then(|v| v.as_str()) == Some("image_url")
                || p.get("image_url").is_some()
        }),
        _ => false,
    }
}

fn message_text(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| {
                if p.get("type").and_then(|v| v.as_str()) == Some("text") {
                    p.get("text").and_then(|v| v.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn tool_results_of(m: &serde_json::Value) -> Vec<serde_json::Value> {
    // OpenAI tool-ответ: {role: tool, tool_call_id, content} → одна запись.
    let entry = serde_json::json!({
        "toolUseId": m.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or(""),
        "status": "SUCCESS",
        "content": {
            "text": message_text(m.get("content")),
        }
    });
    vec![entry]
}

fn tool_uses_of(m: &serde_json::Value) -> Vec<serde_json::Value> {
    m.get("tool_calls")
        .and_then(|v| v.as_array())
        .map(|calls| {
            calls
                .iter()
                .map(|c| {
                    let f = c.get("function").cloned().unwrap_or(serde_json::json!({}));
                    serde_json::json!({
                        "toolUseId": c.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        "name": f.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "input": f.get("arguments").and_then(|v| v.as_str()).unwrap_or(""),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kiro_provider() -> ProviderConfig {
        ProviderConfig {
            id: "kiro".into(),
            name: "Kiro".into(),
            base_url: String::new(),
            is_enabled: true,
            relay_id: None,
            keys: Vec::new(),
            models: vec![DiscoveredModel { id: "claude-sonnet-4.5".into(), ..Default::default() }],
            ..Default::default()
        }
    }

    #[test]
    fn region_validation_rejects_injection() {
        assert!(valid_region("us-east-1"));
        assert!(valid_region("eu-west-2"));
        assert!(!valid_region(""), "пустой регион недопустим");
        assert!(!valid_region("us-east-1.evil.com"), "точка — инъекция в URL");
        assert!(!valid_region("US-EAST-1"), "только строчные");
        assert!(!valid_region("q.us-east-1"), "только регион, не хост");
    }

    #[test]
    fn models_url_shape_matches_9router_proof() {
        let u = models_url("us-east-1", None).expect("url");
        assert_eq!(
            u,
            "https://q.us-east-1.amazonaws.com/ListAvailableModels?origin=AI_EDITOR"
        );
        let u2 = models_url("eu-west-1", Some("arn:aws:ab/c d")).expect("url");
        assert!(u2.contains("profileArn=arn%3Aaws%3Aab%2Fc%20d"), "{u2}");
        assert!(models_url("evil.com", None).is_err());
    }

    #[test]
    fn fallback_statuses_are_401_403_404() {
        assert_eq!(ENDPOINT_FALLBACK_STATUSES, &[401, 403, 404]);
        assert_eq!(CHAT_ENDPOINTS.len(), 3);
        assert!(CHAT_ENDPOINTS[0].contains("runtime."));
    }

    #[test]
    fn parse_kiro_models_reads_model_id_and_rejects_empty() {
        let raw = r#"{"models":[
            {"modelId":"claude-sonnet-4-5","modelName":"Sonnet","tokenLimits":{}},
            {"modelId":"glm-5"},
            {"noId":1},
            {"modelId":"glm-5"}
        ]}"#;
        let m = parse_kiro_models(raw).expect("parse");
        assert_eq!(m.len(), 2, "без id и дубликаты отброшены");
        assert_eq!(m[0].id, "claude-sonnet-4-5");
        assert_eq!(m[1].id, "glm-5");
        assert!(parse_kiro_models(r#"{"models":[]}"#).is_err(), "пустой список — ошибка");
        assert!(parse_kiro_models(r#"{"data":[]}"#).is_err(), "чужой формат отвергается");
    }

    #[test]
    fn conversation_converts_openai_roles() {
        let p = kiro_provider();
        let payload = serde_json::json!({
            "model": "kiro/claude-sonnet-4.5",
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "user", "content": "hi"},
            ]
        });
        let c = build_conversation(&p, &payload, "claude-sonnet-4.5").expect("conv");
        let state = &c["conversationState"];
        assert_eq!(state["chatTriggerType"], "MANUAL");
        // system ушёл в history как userInputMessage, последний user — current.
        assert_eq!(state["history"].as_array().expect("arr").len(), 1);
        assert_eq!(state["currentMessage"]["userInputMessage"]["content"], "hi");
        assert_eq!(
            state["currentMessage"]["userInputMessage"]["modelId"],
            "claude-sonnet-4.5"
        );
    }

    #[test]
    fn conversation_rejects_images_honestly() {
        let p = kiro_provider();
        let payload = serde_json::json!({
            "model": "kiro/m",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "see"},
                {"type": "image_url", "image_url": {"url": "data:..."}}
            ]}]
        });
        let err = build_conversation(&p, &payload, "m").expect_err("картинки запрещены");
        assert!(err.contains("картинки"), "{err}");
    }

    #[test]
    fn conversation_rejects_unknown_roles() {
        let p = kiro_provider();
        let payload = serde_json::json!({
            "model": "kiro/m",
            "messages": [{"role": "developer", "content": "x"}]
        });
        assert!(build_conversation(&p, &payload, "m").is_err());
    }

    #[test]
    fn models_request_uses_bearer_and_eventstream_accept() {
        let k = KiroProtocol;
        let cfg = GatewayConfig::default();
        let up =
            k.models_request(&kiro_provider(), &cfg, "tok123").expect("req");
        assert_eq!(up.method, reqwest::Method::GET);
        assert!(up.route.request_url().contains("ListAvailableModels?origin=AI_EDITOR"));
        let auth = up.headers.get("authorization").expect("auth");
        assert_eq!(auth.to_str().expect("str"), "Bearer tok123");
        assert_eq!(
            up.headers.get("accept").expect("accept").to_str().expect("str"),
            "application/vnd.amazon.eventstream"
        );
    }

    #[test]
    fn chat_request_posts_conversation_to_runtime_first() {
        let k = KiroProtocol;
        let cfg = GatewayConfig::default();
        let payload = serde_json::json!({
            "model": "kiro/m",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let up = k
            .chat_request(&kiro_provider(), &cfg, "tok", &HeaderMap::new(), &payload, "m")
            .expect("req");
        assert_eq!(up.route.request_url(), CHAT_ENDPOINTS[0]);
        let body: serde_json::Value = serde_json::from_slice(&up.body).expect("json");
        assert_eq!(body["conversationState"]["chatTriggerType"], "MANUAL");
    }
}

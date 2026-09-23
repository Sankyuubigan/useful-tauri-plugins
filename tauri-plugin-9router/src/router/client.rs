//! HTTP-клиент к API 9router (localhost).
//!
//! - `/v1/models`     — health-check + список доступных моделей (комбо идут с
//!                      `owned_by: "combo"`).
//! - `/api/combos`    — список комбо (для дропдауна чата).
//! - `/v1/chat/completions` — OpenAI-совместимый чат (стриминг SSE / полный).

use std::time::Duration;

use reqwest::blocking::Client;

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
    pub content: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub stream: bool,
}

impl Default for ChatRequest {
    fn default() -> Self {
        Self {
            model: String::new(),
            messages: Vec::new(),
            max_tokens: None,
            temperature: None,
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
pub fn get_combos(base_url: &str, api_key: Option<&str>) -> Result<Vec<ComboInfo>, String> {
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
    let v: serde_json::Value = resp.json().map_err(|e| format!("Bad JSON ответа: {}", e))?;
    v.pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Ответ 9router не содержит choices[0].message.content".to_string())
}

/// Стриминговый чат (SSE). `on_delta(content)` вызывается на каждую единицу
/// текста; возвращает полный накопленный текст (включая reasoning_content).
pub fn chat_completion_stream(
    base_url: &str,
    api_key: Option<&str>,
    req: &ChatRequest,
    mut on_delta: impl FnMut(&str),
) -> Result<String, String> {
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let mut body = req.clone();
    body.stream = true;

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

    let mut full = String::new();
    for line in resp.bytes().map_err(|e| e.to_string())?.split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(line);
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with("data:") {
            continue;
        }
        let data = trimmed.trim_start_matches("data:").trim();
        if data == "[DONE]" {
            break;
        }
        let v: serde_json::Value = serde_json::from_str(data).unwrap_or_default();
        if let Some(delta) = v.pointer("/choices/0/delta/content").and_then(|c| c.as_str()) {
            if !delta.is_empty() {
                full.push_str(delta);
                on_delta(delta);
            }
        }
        if let Some(rc) = v.pointer("/choices/0/delta/reasoning_content").and_then(|c| c.as_str()) {
            if !rc.is_empty() {
                full.push_str(rc);
                on_delta(rc);
            }
        }
    }

    Ok(full)
}
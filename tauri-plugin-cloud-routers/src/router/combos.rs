//! Служебные GET-запросы к 9router (localhost): модели, комбо, health.
//!
//! Эти операции короткие (штатный роутер отвечает за мс) и остаются на
//! блокирующем клиенте с жёсткими лимитами 4-10 с. Боевой чат — асинхронный
//! (`client.rs`, порог тишины + отмена + живой стрим).

use std::time::Duration;

/// Информация о комбо из `/api/combos` или `/v1/models` (`owned_by: "combo"`).
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

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
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

/// Список комбо с Retry-петлёй (3 попытки с задержкой 600мс) для ленивого
/// старта сервера: после открытия TCP-порта Next.js/9Router ещё 1-2 секунды
/// инициализирует внутренние роуты и SQLite, и первый запрос может упасть
/// с 503/таймаутом.
pub fn get_combos(base_url: &str, api_key: Option<&str>) -> Result<Vec<ComboInfo>, String> {
    let mut last_err = String::new();
    for attempt in 1..=3 {
        match get_combos_once(base_url, api_key) {
            Ok(v) => return Ok(v),
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

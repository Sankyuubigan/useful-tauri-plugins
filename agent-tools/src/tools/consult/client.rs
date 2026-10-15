//! HTTP-клиент к OpenAI-совместимому шлюзу 9Router (`POST /v1/chat/completions`).
//!
//! Настройки берутся из аргументов, затем из окружения, затем из дефолтов:
//! `ROUTER_BASE_URL` → `http://localhost:20128/v1`, `ROUTER_API_KEY` →
//! `9router-key`, `SMART_MODEL_NAME` → `smart-expert`.
//!
//! Клиент блокирующий (`reqwest::blocking`): инструмент работает как процесс
//! «спросил — получил ответ — вышел», асинхронный рантайм здесь не нужен.
//! Таймаут 300 с — с запасом к требуемым 180 с: промпт на 250k токенов умная
//! модель считает долго.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Таймаут HTTP-запроса, секунды (ТЗ требует не менее 180).
pub const TIMEOUT_SECS: u64 = 300;

/// Дефолтный адрес шлюза (OpenAI-совместимый префикс `/v1`).
pub const DEFAULT_BASE_URL: &str = "http://localhost:20128/v1";

/// Дефолтный ключ шлюза.
pub const DEFAULT_API_KEY: &str = "9router-key";

/// Дефолтное имя умной модели (combo в 9Router).
pub const DEFAULT_MODEL: &str = "smart-expert";

/// Ошибка обращения к шлюзу — сетевая/-API (exit 3).
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("не удалось обратиться к шлюзу {url}: {reason}")]
    Transport { url: String, reason: String },
    #[error("шлюз {url} ответил HTTP {status}: {body}")]
    Status {
        url: String,
        status: u16,
        body: String,
    },
    #[error("не удалось разобрать ответ шлюза: {0}")]
    Decode(String),
    #[error("в ответе шлюза нет choices[0].message.content: {0}")]
    NoContent(String),
}

/// Настройки обращения к шлюзу.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterConfig {
    /// Базовый адрес: может быть с `/v1` и без — [`RouterConfig::chat_url`] нормализует.
    pub base_url: String,
    /// Ключ (пустая строка — не отправлять заголовок Authorization).
    pub api_key: String,
    /// Имя модели/combo.
    pub model: String,
    /// Таймаут запроса.
    pub timeout: Duration,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: DEFAULT_API_KEY.to_string(),
            model: DEFAULT_MODEL.to_string(),
            timeout: Duration::from_secs(TIMEOUT_SECS),
        }
    }
}

impl RouterConfig {
    /// Собрать конфиг: явные значения > окружение > дефолты.
    ///
    /// Пустая строка из окружения считается незаданной — `ROUTER_API_KEY=""`
    /// должен означать «без ключа», а не «ключ из воздуха».
    pub fn resolve(
        base_url: Option<&str>,
        model: Option<&str>,
        api_key: Option<&str>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            base_url: pick(base_url, "ROUTER_BASE_URL", defaults.base_url),
            model: pick(model, "SMART_MODEL_NAME", defaults.model),
            api_key: pick(api_key, "ROUTER_API_KEY", defaults.api_key),
            timeout: defaults.timeout,
        }
    }

    /// Полный URL чата. База с `/v1` и без него дают один и тот же адрес —
    /// в репозитории базы роутеров хранятся без `/v1`, в ТЗ — с ним.
    pub fn chat_url(&self) -> String {
        let base = self.base_url.trim().trim_end_matches('/');
        if base.is_empty() {
            return format!("{DEFAULT_BASE_URL}/chat/completions");
        }
        if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
    }
}

fn pick(explicit: Option<&str>, env_key: &str, default: String) -> String {
    if let Some(value) = explicit.map(str::trim).filter(|v| !v.is_empty()) {
        return value.to_string();
    }
    std::env::var(env_key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or(default)
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Option<AssistantMessage>,
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    /// `content` может быть строкой или массивом частей (часть шлюзов так отдаёт).
    #[serde(default)]
    content: Option<serde_json::Value>,
}

impl AssistantMessage {
    /// Текст ответа: строка напрямую либо склейка частей `[{type,text}]`.
    fn text(&self) -> Option<String> {
        match &self.content {
            Some(serde_json::Value::String(text)) => Some(text.clone()),
            Some(serde_json::Value::Array(parts)) => {
                let joined: String = parts
                    .iter()
                    .filter_map(|part| {
                        part.get("text")
                            .and_then(serde_json::Value::as_str)
                            .or_else(|| part.as_str())
                    })
                    .collect();
                (!joined.is_empty()).then_some(joined)
            }
            _ => None,
        }
    }
}

/// Спросить умную модель и вернуть текст ответа.
pub fn ask(config: &RouterConfig, system_prompt: &str, report: &str) -> Result<String, ApiError> {
    let url = config.chat_url();
    let body = ChatRequest {
        model: &config.model,
        messages: vec![
            ChatMessage {
                role: "system",
                content: system_prompt,
            },
            ChatMessage {
                role: "user",
                content: report,
            },
        ],
        stream: false,
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(config.timeout)
        .build()
        .map_err(|e| ApiError::Transport {
            url: url.clone(),
            reason: e.to_string(),
        })?;

    let mut request = client.post(&url).json(&body);
    if !config.api_key.trim().is_empty() {
        request = request.bearer_auth(config.api_key.trim());
    }

    let response = request.send().map_err(|e| ApiError::Transport {
        url: url.clone(),
        reason: e.to_string(),
    })?;

    let status = response.status();
    if !status.is_success() {
        // Тело ошибки часто объясняет причину (нет комбо, неверный ключ) —
        // режем, чтобы не залить агенту мегабайты HTML.
        let body = response.text().unwrap_or_default();
        let body: String = body.chars().take(600).collect();
        return Err(ApiError::Status {
            url,
            status: status.as_u16(),
            body,
        });
    }

    let raw = response.text().map_err(|e| ApiError::Decode(e.to_string()))?;
    let parsed: ChatResponse =
        serde_json::from_str(&raw).map_err(|e| ApiError::Decode(format!("{}: {}", e, first_chars(&raw))))?;

    parsed
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message)
        .and_then(|message| message.text())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| ApiError::NoContent(first_chars(&raw)))
}

fn first_chars(raw: &str) -> String {
    raw.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_with_v1_gets_only_chat_completions() {
        let config = RouterConfig {
            base_url: "http://localhost:20128/v1".to_string(),
            ..RouterConfig::default()
        };
        assert_eq!(
            config.chat_url(),
            "http://localhost:20128/v1/chat/completions"
        );
    }

    #[test]
    fn base_without_v1_gets_v1_prefix() {
        let config = RouterConfig {
            base_url: "http://localhost:20128".to_string(),
            ..RouterConfig::default()
        };
        assert_eq!(
            config.chat_url(),
            "http://localhost:20128/v1/chat/completions"
        );
    }

    #[test]
    fn base_with_trailing_slash_is_normalized() {
        let config = RouterConfig {
            base_url: "http://localhost:20128/v1/".to_string(),
            ..RouterConfig::default()
        };
        assert_eq!(
            config.chat_url(),
            "http://localhost:20128/v1/chat/completions"
        );
    }

    #[test]
    fn empty_base_falls_back_to_default_endpoint() {
        let config = RouterConfig {
            base_url: "   ".to_string(),
            ..RouterConfig::default()
        };
        assert_eq!(
            config.chat_url(),
            "http://localhost:20128/v1/chat/completions"
        );
    }

    #[test]
    fn explicit_arguments_win_over_defaults() {
        let config = RouterConfig::resolve(
            Some("http://127.0.0.1:9/v1"),
            Some("google-low-tier"),
            Some("secret"),
        );
        assert_eq!(config.base_url, "http://127.0.0.1:9/v1");
        assert_eq!(config.model, "google-low-tier");
        assert_eq!(config.api_key, "secret");
    }

    #[test]
    fn blank_arguments_are_ignored() {
        let blank = RouterConfig::resolve(Some("  "), Some(""), None);
        let from_env = RouterConfig::resolve(None, None, None);
        assert_eq!(blank, from_env, "пустая строка не должна перекрывать env/дефолт");
    }

    #[test]
    fn timeout_is_not_below_the_required_180s() {
        assert!(RouterConfig::default().timeout >= Duration::from_secs(180));
    }

    #[test]
    fn assistant_content_accepts_string_and_parts() {
        let string_form = AssistantMessage {
            content: Some(serde_json::Value::String("ответ".into())),
        };
        assert_eq!(string_form.text().as_deref(), Some("ответ"));

        let parts_form = AssistantMessage {
            content: Some(serde_json::json!([
                { "type": "text", "text": "часть 1. " },
                { "type": "text", "text": "часть 2." },
            ])),
        };
        assert_eq!(parts_form.text().as_deref(), Some("часть 1. часть 2."));

        let empty = AssistantMessage {
            content: Some(serde_json::json!([])),
        };
        assert_eq!(empty.text(), None);
    }
}
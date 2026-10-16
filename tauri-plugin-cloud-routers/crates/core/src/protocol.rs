//! Протокол провайдера: каким языком шлюз говорит с апстримом.
//!
//! ## Зачем сущность, а не `if id == "kiro"`
//!
//! До этого модуля в системе не было понятия «протокол»: `resolve_route`
//! клеил `base_url + path`, `auth_header` всегда ставил Bearer, а
//! `parse_models_response` требовал `data[].id`. Любой провайдер с другим
//! протоколом (Kiro: CodeWhisperer JSON + EventStream) ломался «поздно и
//! молча»: принимался при добавлении, умирал на refresh/models. Сущность
//! делает такой отказ невозможным конструктивно: не-OpenAI провайдер обязан
//! объявить свой протокол, иначе не маршрутизируется.
//!
//! ## Что здесь, чего нет
//!
//! Здесь — только данные (какой протокол, чья сессия) и правила их чтения.
//! HTTP-вызовы, декодер EventStream, Device Flow живут в шлюзе (`infra/protocol`):
//! в `core` запрещены reqwest/axum/tauri (см. [`crate`] — дверь модуля).

use serde::{Deserialize, Serialize};

/// Протокол апстрима провайдера.
///
/// Дефолт — OpenAI: все существующие конфиги продолжают работать без миграции.
/// Не-OpenAI провайдер без адаптера в шлюзе не маршрутизируется — это явное
/// состояние, а не молчаливый отказ (см. `VendorProtocol::supported` в шлюзе).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VendorProtocol {
    /// OpenAI-совместимый: `GET {base}/models`, `POST {base}/v1/chat/completions`,
    /// Bearer-авторизация, SSE-поток.
    #[default]
    Openai,
    /// Kiro / Amazon Q: `ListAvailableModels`, `GenerateAssistantResponse`,
    /// бинарный EventStream. Спека — `crates/gateway/docs/kiro-protocol.md`.
    Kiro,
}

impl VendorProtocol {
    /// Человекочитаемое имя для дашборда.
    pub fn display_name(self) -> &'static str {
        match self {
            VendorProtocol::Openai => "OpenAI",
            VendorProtocol::Kiro => "Kiro (Amazon Q)",
        }
    }

    /// Парсинг из строки конфига/дашборда. Неизвестное значение — ошибка, а не
    /// молчаливый OpenAI: иначе опечатка в протоколе выглядела бы как
    /// «рабочий OpenAI-провайдер», который никогда не заработает.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_lowercase().as_str() {
            "openai" | "openai-compatible" | "" => Ok(VendorProtocol::Openai),
            "kiro" | "amazon-q" | "amazonq" => Ok(VendorProtocol::Kiro),
            other => Err(format!(
                "неизвестный протокол «{other}»: допустимы openai, kiro"
            )),
        }
    }
}

/// Способ авторизации провайдера.
///
/// `ApiKeys` — текущие ключи из `ProviderConfig::keys` (нулевая миграция).
/// `OAuth` — ссылка на сессию в `oauth_sessions.json`; сами токены в
/// `gateway.json` не хранятся никогда (разделение секретов по файлам: конфиг
/// читает дашборд целиком, сессии — только шлюз).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuth {
    /// Классические ключи (`ProviderConfig::keys`).
    #[default]
    ApiKeys,
    /// OAuth-сессия по id (см. [`OAuthSession`]).
    OAuth {
        /// Идентификатор сессии в `oauth_sessions.json`.
        session_id: String,
    },
}

impl ProviderAuth {
    /// Сессия привязана, но могла быть удалена: вызывающий обязан проверить
    /// наличие сессии и сообщить честно, а не молча откатиться на ключи.
    pub fn oauth_session_id(&self) -> Option<&str> {
        match self {
            ProviderAuth::ApiKeys => None,
            ProviderAuth::OAuth { session_id } => Some(session_id),
        }
    }
}

/// OAuth-сессия провайдера (`oauth_sessions.json`, рядом с `gateway.json`).
///
/// Секреты живут здесь, а не в `gateway.json`: конфиг целиком отдаётся в
/// дашборд (с масками), а сессии читает только процесс шлюза. Формат файла —
/// карта `session_id → OAuthSession`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OAuthSession {
    /// Кому принадлежит: id провайдера.
    pub provider_id: String,
    /// Какой OAuth-поток выдал токены.
    pub method: OAuthMethod,
    /// Живой access-токен (Bearer к апстриму).
    pub access_token: String,
    /// Refresh-токен. `None` — только у импортированных API-ключей Kiro
    /// (`authMethod: api_key` в 9Router: ключ валиден, рефрешить нечего).
    pub refresh_token: Option<String>,
    /// AWS-регион (`us-east-1`). Проверяется allowlist при создании.
    pub region: String,
    /// Профиль CodeWhisperer (`ListAvailableProfiles`), если выдавался.
    pub profile_arn: Option<String>,
    /// Email из JWT (для показа в дашборде), если извлекли.
    pub email: Option<String>,
    /// Unix-время истечения access-токена. `0` = неизвестно, рефреш по факту 401.
    pub expires_at_unix: u64,
    /// Данные потока для рефреша: clientId/clientSecret/startUrl/authMethod.
    /// Пусто — рефреш невозможен, только повторный вход.
    pub refresh_context: std::collections::BTreeMap<String, String>,
}

impl Default for OAuthSession {
    fn default() -> Self {
        Self {
            provider_id: String::new(),
            method: OAuthMethod::default(),
            access_token: String::new(),
            refresh_token: None,
            region: "us-east-1".to_string(),
            profile_arn: None,
            email: None,
            expires_at_unix: 0,
            refresh_context: std::collections::BTreeMap::new(),
        }
    }
}

/// Каким потоком получена сессия.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OAuthMethod {
    /// AWS Builder ID / IAM Identity Center через Device Code Flow.
    #[default]
    DeviceCode,
    /// Google/GitHub через `KIRO_AUTH_SERVICE` (Authorization Code + PKCE).
    Social,
    /// Вставленный вручную refresh-токен (`aorAAAAAG…`).
    Imported,
    /// API-ключ Kiro (`TokenType: API_KEY`): не OAuth строго, но хранится как
    /// сессия, чтобы чат брал токен из одного места.
    ApiKey,
}

impl OAuthMethod {
    pub fn display_name(self) -> &'static str {
        match self {
            OAuthMethod::DeviceCode => "AWS Builder ID",
            OAuthMethod::Social => "Google/GitHub",
            OAuthMethod::Imported => "импорт токена",
            OAuthMethod::ApiKey => "API-ключ",
        }
    }
}

/// Маскированная копия сессии для дашборда: секреты заменены флагами.
///
/// Правило: токены не покидают процесс шлюза. Дашборд получает только факт
/// наличия и метаданные — как `vercel_relay_token_set` для релея.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthSessionDto {
    pub session_id: String,
    pub provider_id: String,
    pub method: OAuthMethod,
    pub has_access_token: bool,
    pub has_refresh_token: bool,
    pub region: String,
    pub has_profile_arn: bool,
    pub email: Option<String>,
    pub expires_at_unix: u64,
    /// `true`, если рефреш возможен (есть refresh_token либо refresh_context).
    pub refreshable: bool,
}

impl OAuthSessionDto {
    pub fn mask(session_id: &str, s: &OAuthSession) -> Self {
        let has_refresh = s.refresh_token.as_deref().is_some_and(|t| !t.trim().is_empty());
        OAuthSessionDto {
            session_id: session_id.to_string(),
            provider_id: s.provider_id.clone(),
            method: s.method,
            has_access_token: !s.access_token.trim().is_empty(),
            has_refresh_token: has_refresh,
            region: s.region.clone(),
            has_profile_arn: s.profile_arn.as_deref().is_some_and(|a| !a.trim().is_empty()),
            email: s.email.clone(),
            expires_at_unix: s.expires_at_unix,
            refreshable: has_refresh || !s.refresh_context.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_defaults_to_openai() {
        let p = VendorProtocol::default();
        assert_eq!(p, VendorProtocol::Openai);
    }

    #[test]
    fn protocol_parse_accepts_aliases_and_rejects_unknown() {
        assert_eq!(VendorProtocol::parse("openai").expect("p"), VendorProtocol::Openai);
        assert_eq!(VendorProtocol::parse("").expect("p"), VendorProtocol::Openai);
        assert_eq!(VendorProtocol::parse("Kiro").expect("p"), VendorProtocol::Kiro);
        assert_eq!(VendorProtocol::parse("amazon-q").expect("p"), VendorProtocol::Kiro);
        assert!(VendorProtocol::parse("anthropic").is_err(), "неизвестный протокол обязан ошибаться");
    }

    #[test]
    fn auth_defaults_to_api_keys() {
        assert_eq!(ProviderAuth::default(), ProviderAuth::ApiKeys);
        assert_eq!(ProviderAuth::ApiKeys.oauth_session_id(), None);
        let o = ProviderAuth::OAuth { session_id: "s1".into() };
        assert_eq!(o.oauth_session_id(), Some("s1"));
    }

    #[test]
    fn session_mask_never_contains_secrets() {
        let s = OAuthSession {
            provider_id: "kiro".into(),
            method: OAuthMethod::DeviceCode,
            access_token: "secret-access".into(),
            refresh_token: Some("secret-refresh".into()),
            profile_arn: Some("arn:aws:…".into()),
            email: Some("u@example.com".into()),
            ..Default::default()
        };
        let dto = OAuthSessionDto::mask("s1", &s);
        let json = serde_json::to_string(&dto).expect("json");
        assert!(!json.contains("secret-access"), "access-токен утёк: {json}");
        assert!(!json.contains("secret-refresh"), "refresh-токен утёк: {json}");
        assert!(!json.contains("arn:aws"), "profile_arn утёк: {json}");
        assert!(dto.has_access_token && dto.has_refresh_token && dto.refreshable);
        assert_eq!(dto.email.as_deref(), Some("u@example.com"));
    }

    #[test]
    fn session_mask_marks_unrefreshable_without_context() {
        let s = OAuthSession { provider_id: "kiro".into(), ..Default::default() };
        let dto = OAuthSessionDto::mask("s1", &s);
        assert!(!dto.has_access_token);
        assert!(!dto.refreshable, "без токена и контекста рефреш невозможен");
    }
}

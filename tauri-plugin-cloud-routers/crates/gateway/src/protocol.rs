//! Протоколы вендоров: каким языком шлюз говорит с каждым апстримом.
//!
//! ## Зачем трейт, а не `if protocol == Kiro`
//!
//! До этого модуля протокол был захардкожен как единственная реальность:
//! `resolve_route` клеил `base_url + path`, `auth_header` всегда ставил Bearer,
//! `parse_models_response` требовал `data[].id`. Любой новый вендор превращался
//! бы в россыпь `if kiro` по `proxy.rs`/`upstream.rs` — то есть в костыль,
//! который следующий вендор ломает. Трейт делает добавление вендора
//! конструктивно чистым: новый файл с реализацией контракта, ноль правок в
//! существующем коде прокси.
//!
//! ## Граница движения
//!
//! Адаптер строит **запрос** (URL, метод, заголовки, тело) и разбирает
//! **ответ** (модели, поток). Ретраи по ключам, кулдауны, деактивация ключей,
//! маскировка ошибок — остаются в `proxy.rs`: это политика шлюза, одинаковая
//! для всех вендоров. Адаптер не знает про ключи вообще — ему дают готовый
//! bearer-токен строкой.
//!
//! ## OpenAI-адаптер — вынос, не переписывание
//!
//! `OpenAiProtocol` — побайтово то поведение, что было вшито в `proxy.rs` и
//! `upstream.rs`: те же пути, те же заголовки, тот же разбор. Гейт шага 2:
//! весь существующий сьют зелёный без изменений.

use axum::http::{HeaderMap, HeaderName, HeaderValue};
use cloud_routers_core::config::{GatewayConfig, ProviderConfig};
use cloud_routers_core::config::DiscoveredModel;

use crate::upstream::{Route, resolve_route};

/// Запрос к апстриму, готовый к отправке через `reqwest`.
///
/// Тело — байты (уже сериализованный JSON): адаптер сам решает формат.
pub struct UpstreamRequest {
    pub route: Route,
    pub method: reqwest::Method,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

/// Контракт вендора. Реализация обязана быть чистой функцией от
/// (провайдер, конфиг, токен, вход) — без чтения диска и глобального стейта.
pub trait VendorProtocol: Send + Sync {
    /// Какой протокол реализует (для диагностики).
    fn name(&self) -> &'static str;

    /// Запрос списка моделей. Токен — готовый секрет (ключ или access-токен).
    fn models_request(
        &self,
        provider: &ProviderConfig,
        config: &GatewayConfig,
        credential: &str,
    ) -> Result<UpstreamRequest, String>;

    /// Разобрать тело ответа списка моделей.
    fn parse_models(&self, body: &str) -> Result<Vec<DiscoveredModel>, String>;

    /// Запрос инференса. `payload` — исходное тело клиента (OpenAI-формат);
    /// адаптер переводит его в формат вендора.
    fn chat_request(
        &self,
        provider: &ProviderConfig,
        config: &GatewayConfig,
        credential: &str,
        client_headers: &HeaderMap,
        payload: &serde_json::Value,
        upstream_model: &str,
    ) -> Result<UpstreamRequest, String>;

    /// Ответ апстрима — потоковый? Решает по content-type ответа.
    ///
    /// Дефолт — OpenAI-правило (`text/event-stream`); Kiro переопределяет
    /// (бинарный EventStream).
    fn is_stream_response(&self, content_type: &str) -> bool {
        content_type.starts_with("text/event-stream")
    }
}

/// OpenAI-совместимый вендор: поведение, ранее вшитое в прокси напрямую.
pub struct OpenAiProtocol;

impl OpenAiProtocol {
    fn bearer(credential: &str) -> HeaderValue {
        crate::upstream::auth_header(credential)
    }
}

impl VendorProtocol for OpenAiProtocol {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn models_request(
        &self,
        provider: &ProviderConfig,
        config: &GatewayConfig,
        credential: &str,
    ) -> Result<UpstreamRequest, String> {
        let route = resolve_route(provider, config, "/models")?;
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("authorization"),
            Self::bearer(credential),
        );
        // Релей без этих заголовков отвечает 401 «missing or invalid
        // x-relay-token» (кейс 25.10.2026 — «Обновить модели» у Google): чат
        // ставит их через chat_request, а этот путь был единственным без них.
        crate::upstream::apply_route_headers(&route, &mut headers);
        Ok(UpstreamRequest {
            route,
            method: reqwest::Method::GET,
            headers,
            body: Vec::new(),
        })
    }

    fn parse_models(&self, body: &str) -> Result<Vec<DiscoveredModel>, String> {
        cloud_routers_core::models::parse_models_response(body)
    }

    fn chat_request(
        &self,
        provider: &ProviderConfig,
        config: &GatewayConfig,
        credential: &str,
        client_headers: &HeaderMap,
        payload: &serde_json::Value,
        upstream_model: &str,
    ) -> Result<UpstreamRequest, String> {
        // Единственное изменение тела — подстановка модели (бывший rewrite_body).
        let mut body = payload.clone();
        let Some(obj) = body.as_object_mut() else {
            return Err("Тело запроса должно быть JSON-объектом".to_string());
        };
        obj.insert(
            "model".to_string(),
            serde_json::Value::String(upstream_model.to_string()),
        );
        let bytes = serde_json::to_vec(&body)
            .map_err(|e| format!("Не удалось сериализовать тело запроса: {}", e))?;

        let route = resolve_route(provider, config, "/v1/chat/completions")?;
        let mut headers = crate::upstream::forward_headers(client_headers);
        headers.insert(
            HeaderName::from_static("authorization"),
            Self::bearer(credential),
        );
        crate::upstream::apply_route_headers(&route, &mut headers);
        // `content-type` только когда есть тело (бывшее правило build_request).
        if !bytes.is_empty() {
            headers.insert(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("application/json"),
            );
        }
        Ok(UpstreamRequest {
            route,
            method: reqwest::Method::POST,
            headers,
            body: bytes,
        })
    }
}

/// Выбрать адаптер по протоколу провайдера.
///
/// Неизвестного протокола здесь быть не может: `VendorProtocol::parse`
/// в core отвергает его при чтении конфига. Поэтому `unreachable!` запрещён —
/// возвращаем ошибку текстом.
pub fn adapter_for(
    protocol: cloud_routers_core::protocol::VendorProtocol,
) -> Result<Box<dyn VendorProtocol>, String> {
    use cloud_routers_core::protocol::VendorProtocol as P;
    match protocol {
        P::Openai => Ok(Box::new(OpenAiProtocol)),
        P::Kiro => Ok(Box::new(crate::protocol_kiro::KiroProtocol)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_routers_core::config::AccountKey;

    fn provider() -> ProviderConfig {
        ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            is_enabled: true,
            use_relay: false,
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-1".into(), is_active: true, name: None }],
            ..Default::default()
        }
    }

    #[test]
    fn openai_models_request_matches_legacy_behavior() {
        let p = OpenAiProtocol;
        let cfg = GatewayConfig::default();
        let req = p.models_request(&provider(), &cfg, "gsk-1").expect("req");
        assert_eq!(req.method, reqwest::Method::GET);
        assert_eq!(req.route.request_url(), "https://api.groq.com/openai/v1/models");
        assert_eq!(
            req.headers.get("authorization").expect("auth").to_str().expect("str"),
            "Bearer gsk-1"
        );
        assert!(req.body.is_empty(), "у GET /models тела быть не должно");
    }

    #[test]
    fn openai_models_request_carries_relay_headers() {
        // Регресс 25.10.2026: models_request строил relay-URL без
        // x-relay-token/x-target-url, и воркер отвечал 401 «missing or invalid
        // x-relay-token» на «Обновить модели» любого relay-провайдера.
        let p = OpenAiProtocol;
        let mut provider = provider();
        provider.use_relay = true;
        let cfg = GatewayConfig {
            vercel_relay_url: Some("https://relay.workers.dev".into()),
            vercel_relay_token: Some("s3cret".into()),
            ..Default::default()
        };
        let req = p.models_request(&provider, &cfg, "gsk-1").expect("req");
        assert!(req.route.is_relay());
        assert_eq!(req.route.request_url(), "https://relay.workers.dev/api/gateway/models");
        assert_eq!(
            req.headers
                .get(crate::upstream::HDR_RELAY_TOKEN)
                .expect("token")
                .to_str()
                .expect("str"),
            "s3cret"
        );
        assert_eq!(
            req.headers
                .get(crate::upstream::HDR_TARGET_URL)
                .expect("target")
                .to_str()
                .expect("str"),
            "https://api.groq.com/openai/v1/models"
        );
    }

    #[test]
    fn openai_chat_request_matches_legacy_behavior() {
        let p = OpenAiProtocol;
        let cfg = GatewayConfig::default();
        let payload = serde_json::json!({"model": "groq/llama", "messages": []});
        let req = p
            .chat_request(&provider(), &cfg, "gsk-1", &HeaderMap::new(), &payload, "llama")
            .expect("req");
        assert_eq!(req.method, reqwest::Method::POST);
        assert_eq!(
            req.route.request_url(),
            "https://api.groq.com/openai/v1/v1/chat/completions"
        );
        let body: serde_json::Value = serde_json::from_slice(&req.body).expect("json");
        assert_eq!(body["model"], "llama", "модель подставлена апстримная");
        assert_eq!(
            req.headers.get("content-type").expect("ct").to_str().expect("str"),
            "application/json"
        );
    }

    #[test]
    fn kiro_adapter_is_registered() {
        assert!(
            adapter_for(cloud_routers_core::protocol::VendorProtocol::Kiro).is_ok(),
            "Kiro-адаптер обязан быть зарегистрирован (шаг 3)"
        );
    }
}

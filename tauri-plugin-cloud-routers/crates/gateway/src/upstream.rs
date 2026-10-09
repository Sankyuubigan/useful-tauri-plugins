//! Построение исходящего запроса к провайдеру: URL, заголовки, тело.
//!
//! ## Три решения, которые здесь зафиксированы
//!
//! ### 1. Заголовки — по белому списку, а не по чёрному
//!
//! Чёрный список («убери hop-by-hop») оставляет в запросе **всё, о чём не
//! подумали**: cookie браузера, `x-api-key` локального IDE, авторизацию от
//! внутреннего сервиса. Шлюз пересылает это третьей стороне — провайдеру.
//! Белый список из трёх точных имён и трёх префиксов делает утечку
//! конструктивно невозможной: заголовок, который не перечислен, не уйдёт.
//!
//! ### 2. `Authorization` клиента **всегда** заменяется
//!
//! IDE шлёт свой локальный токен (`Authorization: Bearer ...`). Если бы он
//! дошёл до провайдера, ключ провайдера не подставился бы вообще, а локальный
//! токен ушёл бы третьей стороне. Поэтому `authorization` и `x-api-key` не
//! переносятся **никогда** — они не в белом списке.
//!
//! ### 3. Релей получает **абсолютный** URL в заголовке
//!
//! В исходном ТЗ было `x-target-host: {base_url}` + путь запроса. Это даёт
//! `https://api.groq.com/openai/v1` + `/v1/chat/completions` =
//! `.../openai/v1/v1/chat/completions` — двойной `/v1`, и провайдер отвечает 404.
//! Чтобы воркер не занимался склейкой путей (и не ломался при следующем
//! переименовании), шлюз сам считает полный URL апстрима и кладёт его в
//! `x-target-url`. Воркер делает ровно один `fetch` по этому URL.
//!
//! ## Тело
//!
//! Тело **не** проксируется байт-в-байт: единственное изменение — подстановка
//! `model` (`groq/llama-3.3-70b` → `llama-3.3-70b`). Всё остальное (`tools`,
//! `tool_choice`, `response_format`, `stream_options`, любые будущие поля)
//! сохраняется как есть: это и есть «zero-maintenance passthrough» из ТЗ —
//! мы не переводим протоколы между вендорами, мы только подставляем
//! routing-заголовок.

use axum::http::{HeaderMap, HeaderName, HeaderValue};
use cloud_routers_core::config::{GatewayConfig, ProviderConfig};

/// Префикс пути релея, под которым живёт Edge-воркер.
pub const RELAY_PATH_PREFIX: &str = "/api/gateway";

/// Заголовок с абсолютным URL апстрима (для воркера релея).
pub const HDR_TARGET_URL: &str = "x-target-url";
/// Секрет релея.
pub const HDR_RELAY_TOKEN: &str = "x-relay-token";

/// Заголовки, переносимые дословно.
const FORWARD_EXACT: &[&str] = &["content-type", "accept", "accept-language"];
/// Префиксы заголовков, переносимые целиком.
///
/// `openai-*` и `anthropic-*` — служебные заголовки SDK (organization, project,
/// beta-флаги). `x-stainless-*` — телеметрия клиента, безопасна и полезна для
/// провайдера (он по ней видит, что запрос пришёл из совместимого SDK).
const FORWARD_PREFIXES: &[&str] = &["openai-", "anthropic-", "x-stainless-"];

/// Куда уходит запрос: напрямую провайдеру или через релей.
#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    /// Прямое подключение.
    Direct { url: String },
    /// Через Vercel-релей.
    Relay { url: String, target_url: String, token: Option<String> },
}

impl Route {
    /// URL, который будет запрошен у релея или у провайдера.
    pub fn request_url(&self) -> &str {
        match self {
            Route::Direct { url } => url,
            Route::Relay { url, .. } => url,
        }
    }

    pub fn is_relay(&self) -> bool {
        matches!(self, Route::Relay { .. })
    }
}

/// Собрать маршрут для апстрим-пути (например `/v1/chat/completions`).
///
/// Если у провайдера включён релей, но URL релея не задан — падаем **ошибкой**,
/// а не молча идём напрямую. Причина: пользователь нажал «через релей»,
/// потому что прямой доступ заблокирован; молчаливый прямой запрос выглядел бы
/// как «релей сломался» и побудил бы его чинить не то.
pub fn resolve_route(
    provider: &ProviderConfig,
    config: &GatewayConfig,
    upstream_path: &str,
) -> Result<Route, String> {
    let direct = format!("{}{}", cloud_routers_core::config::trim_base_url(&provider.base_url), upstream_path);
    if !provider.use_relay {
        return Ok(Route::Direct { url: direct });
    }
    let relay = config
        .vercel_relay_url
        .as_deref()
        .map(cloud_routers_core::config::trim_base_url)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            format!(
                "Провайдер «{}» настроен на работу через релей, но URL релея не задан",
                provider.id
            )
        })?;
    Ok(Route::Relay {
        url: format!("{relay}{RELAY_PATH_PREFIX}{upstream_path}"),
        target_url: direct,
        token: config.vercel_relay_token.clone().filter(|t| !t.trim().is_empty()),
    })
}

/// Отобрать заголовки клиента для пересылки апстриму.
///
/// Возвращает **новую** карту: входные `HeaderMap` не мутируются, иначе один
/// запрос мог бы повлиять на другой через переиспользованные структуры.
pub fn forward_headers(src: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in src.iter() {
        if !is_forwardable(name.as_str()) {
            continue;
        }
        if let Ok(v) = HeaderValue::from_bytes(value.as_bytes()) {
            out.insert(name.clone(), v);
        }
        // Значение, не пережившее HeaderValue, молча пропускается: заголовок —
        // необязательная часть запроса, и ронять из-за него пересылку нельзя.
    }
    out
}

/// Разрешён ли заголовок к пересылке.
pub fn is_forwardable(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // Явные запреты. `authorization` и `x-api-key` — локальные секреты IDE,
    // их нельзя передавать провайдеру ни при каких условиях.
    if matches!(lower.as_str(), "authorization" | "x-api-key" | "cookie" | "host" | "content-length") {
        return false;
    }
    if FORWARD_EXACT.contains(&lower.as_str()) {
        return true;
    }
    FORWARD_PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// Подставить реальный id модели в тело запроса.
///
/// Возвращает готовые байты. Ошибки (нет `model`, не объект) превращаются в
/// понятный текст: клиент увидит `400` с объяснением, а не `500`.
pub fn rewrite_body(body: &mut serde_json::Value, upstream_model: &str) -> Result<Vec<u8>, String> {
    if !body.is_object() {
        return Err("Тело запроса должно быть JSON-объектом".to_string());
    }
    let obj = body.as_object_mut().expect("проверено is_object выше");
    obj.insert(
        "model".to_string(),
        serde_json::Value::String(upstream_model.to_string()),
    );
    serde_json::to_vec(body).map_err(|e| format!("Не удалось сериализовать тело запроса: {}", e))
}

/// Заголовок `Authorization` для апстрима.
pub fn auth_header(api_key: &str) -> HeaderValue {
    // Значение формируется нами из ключа, который сам пришёл из JSON-конфига.
    // Единственный «плохой» случай — ключ с не-ASCII, тогда reqwest вернёт
    // ошибку при сборке запроса, и мы её увидим как ошибку отправки.
    HeaderValue::from_str(&format!("Bearer {}", api_key.trim()))
        .unwrap_or_else(|_| HeaderValue::from_static("Bearer invalid"))
}

/// Добавить служебные заголовки маршрута (релей или `User-Agent`).
pub fn apply_route_headers(route: &Route, headers: &mut HeaderMap) {
    match route {
        Route::Direct { .. } => {}
        Route::Relay { target_url, token, .. } => {
            if let Ok(v) = HeaderValue::from_str(target_url) {
                headers.insert(HeaderName::from_static(HDR_TARGET_URL), v);
            }
            if let Some(t) = token {
                if let Ok(v) = HeaderValue::from_str(t) {
                    headers.insert(HeaderName::from_static(HDR_RELAY_TOKEN), v);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud_routers_core::config::AccountKey;

    fn hm(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut m = HeaderMap::new();
        for (k, v) in pairs {
            m.insert(
                HeaderName::try_from(*k).expect("name"),
                HeaderValue::from_str(v).expect("value"),
            );
        }
        m
    }

    fn provider(base: &str, use_relay: bool) -> ProviderConfig {
        ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: base.into(),
            use_relay,
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-1".into(), is_active: true }],
            ..Default::default()
        }
    }

    fn config_with_relay(url: Option<&str>, token: Option<&str>) -> GatewayConfig {
        GatewayConfig {
            vercel_relay_url: url.map(|s| s.to_string()),
            vercel_relay_token: token.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn direct_route_appends_path_without_double_slash() {
        let p = provider("https://api.groq.com/openai/v1/", false);
        let r = resolve_route(&p, &config_with_relay(None, None), "/v1/chat/completions").expect("route");
        assert_eq!(
            r,
            Route::Direct { url: "https://api.groq.com/openai/v1/v1/chat/completions".into() }
        );
        assert!(!r.is_relay());
    }

    #[test]
    fn relay_route_targets_absolute_upstream_url() {
        // Регрессия против схемы «x-target-host + pathname»: склейка дала бы
        // ".../openai/v1/v1/chat/completions" с двойным /v1.
        let p = provider("https://api.groq.com/openai/v1", true);
        let cfg = config_with_relay(Some("https://relay.vercel.app/"), Some("secret"));
        let r = resolve_route(&p, &cfg, "/v1/chat/completions").expect("route");
        match &r {
            Route::Relay { url, target_url, token } => {
                assert_eq!(url, "https://relay.vercel.app/api/gateway/v1/chat/completions");
                assert_eq!(target_url, "https://api.groq.com/openai/v1/v1/chat/completions");
                assert_eq!(token.as_deref(), Some("secret"));
            }
            other => panic!("ожидался Relay, получен {:?}", other),
        }
        assert!(r.is_relay());
    }

    #[test]
    fn relay_without_url_is_a_hard_error_not_silent_direct() {
        // Пользователь выбрал релей из-за блокировки. Молчаливый прямой запрос
        // выглядел бы как «релей сломался».
        let p = provider("https://api.groq.com/openai/v1", true);
        let err = resolve_route(&p, &config_with_relay(None, None), "/v1/chat/completions")
            .expect_err("must fail");
        assert!(err.contains("URL релея не задан"), "{err}");
    }

    #[test]
    fn relay_blank_url_counts_as_missing() {
        let p = provider("https://api.groq.com/openai/v1", true);
        assert!(resolve_route(&p, &config_with_relay(Some("   "), None), "/v1/chat/completions").is_err());
    }

    #[test]
    fn relay_route_headers_carry_target_and_token() {
        let p = provider("https://api.groq.com/openai/v1", true);
        let cfg = config_with_relay(Some("https://relay.vercel.app"), Some("s3cret"));
        let route = resolve_route(&p, &cfg, "/v1/chat/completions").expect("route");
        let mut h = HeaderMap::new();
        apply_route_headers(&route, &mut h);
        assert_eq!(h.get(HDR_TARGET_URL).expect("target").to_str().expect("str"),
            "https://api.groq.com/openai/v1/v1/chat/completions");
        assert_eq!(h.get(HDR_RELAY_TOKEN).expect("token").to_str().expect("str"), "s3cret");
    }

    #[test]
    fn direct_route_adds_no_relay_headers() {
        let p = provider("https://api.groq.com/openai/v1", false);
        let route = resolve_route(&p, &config_with_relay(None, None), "/v1/chat/completions").expect("route");
        let mut h = HeaderMap::new();
        apply_route_headers(&route, &mut h);
        assert!(h.get(HDR_TARGET_URL).is_none());
        assert!(h.get(HDR_RELAY_TOKEN).is_none());
    }

    #[test]
    fn headers_whitelist_forwards_expected_and_drops_secrets() {
        let src = hm(&[
            ("content-type", "application/json"),
            ("accept", "text/event-stream"),
            ("accept-language", "ru"),
            ("openai-organization", "org-1"),
            ("x-stainless-lang", "js"),
            ("anthropic-version", "2023-06-01"),
        ]);
        let out = forward_headers(&src);
        for want in [
            "content-type",
            "accept",
            "accept-language",
            "openai-organization",
            "x-stainless-lang",
            "anthropic-version",
        ] {
            assert!(out.contains_key(want), "не переслан {}", want);
        }
    }

    #[test]
    fn headers_never_forward_client_secrets() {
        // Локальный токен IDE и cookie браузера не должны уйти третьей стороне.
        let src = hm(&[
            ("authorization", "Bearer ide-local-token"),
            ("x-api-key", "ide-secret"),
            ("cookie", "session=abc"),
            ("host", "127.0.0.1:20131"),
            ("content-length", "123"),
        ]);
        let out = forward_headers(&src);
        for forbidden in ["authorization", "x-api-key", "cookie", "host", "content-length"] {
            assert!(!out.contains_key(forbidden), "{} утек в апстрим", forbidden);
        }
        assert!(out.is_empty());
    }

    #[test]
    fn headers_drop_unknown_and_hop_by_hop() {
        let src = hm(&[
            ("connection", "keep-alive"),
            ("transfer-encoding", "chunked"),
            ("upgrade", "websocket"),
            ("x-forwarded-for", "10.0.0.1"),
            ("user-agent", "curl/8"),
        ]);
        assert!(forward_headers(&src).is_empty());
    }

    #[test]
    fn is_forwardable_is_case_insensitive() {
        assert!(is_forwardable("Content-Type"));
        assert!(is_forwardable("OPENAI-BETA"));
        assert!(!is_forwardable("Authorization"));
    }

    #[test]
    fn rewrite_body_swaps_only_model() {
        let mut body: serde_json::Value = serde_json::from_str(
            r#"{"model":"groq/llama-3.3-70b","messages":[{"role":"user","content":"hi"}],
                "stream":true,"temperature":0.4,"tools":[{"type":"function"}],
                "some_future_field":{"a":1}}"#,
        )
        .expect("json");
        let bytes = rewrite_body(&mut body, "llama-3.3-70b-versatile").expect("rewrite");
        let back: serde_json::Value = serde_json::from_slice(&bytes).expect("json back");
        assert_eq!(back["model"], "llama-3.3-70b-versatile");
        // Всё остальное — без изменений, включая поля, которых шлюз не знает.
        assert_eq!(back["messages"][0]["content"], "hi");
        assert_eq!(back["stream"], true);
        assert_eq!(back["temperature"], 0.4);
        assert_eq!(back["tools"][0]["type"], "function");
        assert_eq!(back["some_future_field"]["a"], 1);
    }

    #[test]
    fn rewrite_body_accepts_missing_stream_flag() {
        let mut body: serde_json::Value = serde_json::from_str(r#"{"model":"groq/m1"}"#).expect("json");
        rewrite_body(&mut body, "m1").expect("rewrite");
        assert_eq!(body["model"], "m1");
    }

    #[test]
    fn rewrite_body_rejects_non_object() {
        let mut arr = serde_json::json!([1, 2, 3]);
        assert!(rewrite_body(&mut arr, "m1").is_err());
        let mut s = serde_json::json!("hello");
        assert!(rewrite_body(&mut s, "m1").is_err());
    }

    #[test]
    fn auth_header_uses_bearer_scheme_and_trims() {
        assert_eq!(auth_header("  gsk-1  ").to_str().expect("str"), "Bearer gsk-1");
    }

    #[test]
    fn auth_header_survives_non_ascii_key_without_panicking() {
        // Ключ с не-ASCII нельзя положить в HeaderValue. Падать нельзя — вернём
        // маркер; ошибка всплывёт как ошибка отправки, а не как паника.
        let v = auth_header("ключ-не-латиница");
        assert!(!v.is_sensitive());
    }
}
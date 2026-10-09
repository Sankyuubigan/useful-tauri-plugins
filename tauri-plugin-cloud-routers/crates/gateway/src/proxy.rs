//! Прокси инференса: `POST /v1/chat/completions`.
//!
//! ## Инвариант «нет ретрая после первого байта»
//!
//! Это единственное место, где легко сделать тихую ошибку. Когда шлюз отдаёт
//! SSE-поток, байты уходят клиенту (IDE) непрерывно. Если апстрим оборвался на
//! середине, повтор запроса другим ключом дал бы клиенту **второй** ответ,
//! склеенный с первым: две строки, два `finish_reason`, оборванный JSON.
//!
//! Поэтому ретрай живёт **строго вокруг** `send()` и проверки статуса, а как
//! только получен неретраибельный ответ, управление уходит в
//! [`build_passthrough_response`] и обратно не возвращается. Структурная
//! гарантия, а не проверка «если что» — её нельзя случайно сломать добавлением
//! `if` в другом месте.
//!
//! ## Что считается ретраибельным
//!
//! Только отказы, где виноват ключ или канал, а не запрос:
//! транспорт, `429`, `5xx`, `401/403`. Ошибки самого запроса (`400`, `404`,
//! `422`) возвращаются клиенту как есть: повтор с другим ключом дал бы тот же
//! ответ и только задержал ошибку.
//!
//! ## Порядок ключей
//!
//! `KeyRotation::candidates` отдаёт ключи в round-robin порядке, уже без
//! кулдаунов. Если живых ключей не осталось, делается **один** сброс кулдаунов
//! (иначе шлюз деградирует в вечный 503), и только потом — ошибка.

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use cloud_routers_core::config::{GatewayConfig, ProviderConfig, trim_base_url};
use cloud_routers_core::keys::{parse_retry_after, FailureKind};
use cloud_routers_core::models::resolve_combo;
use serde_json::json;

use crate::app::AppState;
use crate::upstream::{apply_route_headers, auth_header, forward_headers, resolve_route, rewrite_body, Route};

/// Максимальный размер тела запроса.
///
/// Стандартный лимит axum — 2 МБ, чего не хватает: запросы с картинками приходят
/// как base64 внутри JSON и легко переваливают за мегабайт. Обрезать такой
/// запрос молча нельзя — клиент получил бы внятную ошибку вместо работы.
pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// Тело ошибки в формате OpenAI API.
///
/// Формат обязателен: IDE (Cursor, Cline, Roo) парсят тело ошибки и показывают
/// пользователю `error.message`. Наша произвольная строка превратилась бы в
/// «неизвестная ошибка».
pub fn openai_error(status: StatusCode, message: impl Into<String>, kind: &str) -> Response {
    let body = json!({
        "error": {
            "message": message.into(),
            "type": kind,
            "param": serde_json::Value::Null,
            "code": serde_json::Value::Null,
        }
    });
    (status, axum::Json(body)).into_response()
}

/// Ошибка 404 для неизвестной модели.
///
/// В тексте перечислены известные комбо: клиенту полезно знать, что список
/// пуст из-за неверного id, а не из-за пустой конфигурации.
fn unknown_model(model: &str, known: &[String]) -> Response {
    let hint = if known.is_empty() {
        "в шлюзе нет ни одного настроенного провайдера с обнаруженными моделями".to_string()
    } else {
        format!("доступные модели: {}", known.join(", "))
    };
    openai_error(
        StatusCode::NOT_FOUND,
        format!("Модель «{}» не найдена. {}", model, hint),
        "invalid_request_error",
    )
}

/// Точка входа обработчика `POST /v1/chat/completions`.
pub async fn chat_completions(state: axum::extract::State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let mut payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return openai_error(
                StatusCode::BAD_REQUEST,
                format!("Тело запроса не является корректным JSON: {}", e),
                "invalid_request_error",
            )
        }
    };
    let Some(model) = payload.get("model").and_then(|v| v.as_str()).map(|s| s.to_string()) else {
        return openai_error(
            StatusCode::BAD_REQUEST,
            "В запросе отсутствует поле \"model\"",
            "invalid_request_error",
        );
    };
    if model.trim().is_empty() {
        return openai_error(
            StatusCode::BAD_REQUEST,
            "Поле \"model\" пустое",
            "invalid_request_error",
        );
    }

    let config = state.config();
    let known: Vec<String> = cloud_routers_core::models::list_combos(&config.providers)
        .into_iter()
        .map(|c| c.name)
        .collect();

    let resolved = resolve_combo(&config.providers, &model);

    // Клонируем провайдера сразу: дальше цикл ретрая работает без блокировки
    // конфигурации, иначе деактивация ключа внутри цикла встала бы в deadlock
    // на RwLock.
    let provider: ProviderConfig = match resolved.as_ref() {
        Some((p, _)) => (*p).clone(),
        None => return unknown_model(&model, &known),
    };
    // `resolve_combo` вернул id модели из списка обнаруженных — тот же самый,
    // что прислал клиент, но с тем же временем жизни, что и конфигурация.
    let upstream_model = match resolved {
        Some((_, m)) => m.to_string(),
        None => return unknown_model(&model, &known),
    };

    let route = match resolve_route(&provider, &config, "/v1/chat/completions") {
        Ok(r) => r,
        Err(e) => return openai_error(StatusCode::BAD_GATEWAY, e, "configuration_error"),
    };
    let request_body = match rewrite_body(&mut payload, &upstream_model) {
        Ok(b) => b,
        Err(e) => return openai_error(StatusCode::BAD_REQUEST, e, "invalid_request_error"),
    };
    let forwarded = forward_headers(&headers);

    // ── Кандидаты ────────────────────────────────────────────────────────────
    let provider_id = provider.id.clone();
    let mut candidates = state.rotation().candidates(&provider_id, &provider.keys);
    if candidates.is_empty() && state.rotation().reset_if_all_sleeping(&provider_id, &provider.keys) {
        state.events().warn(format!(
            "провайдер «{}»: все ключи были в кулдауне — сброшены кулдауны, пробуем снова",
            provider_id
        ));
        candidates = state.rotation().candidates(&provider_id, &provider.keys);
    }
    if candidates.is_empty() {
        let total = provider.keys.len();
        let active = provider.keys.iter().filter(|k| k.is_active).count();
        return openai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "У провайдера «{}» нет доступных ключей (всего {}, включено {}). Добавьте ключ или включите его в настройках шлюза.",
                provider_id, total, active
            ),
            "insufficient_quota",
        );
    }

    let mut last: Option<Response> = None;
    let mut attempted: Vec<String> = Vec::new();

    for key_id in candidates {
        let Some(key) = provider.keys.iter().find(|k| k.id == key_id) else {
            continue;
        };
        let key_id = key.id.clone();
        let api_key = key.key.clone();
        attempted.push(key_id.clone());

        let req = build_request(&state, &route, &forwarded, &api_key, reqwest::Method::POST, request_body.clone());
        state.events().info(format!(
            "запрос {} → провайдер «{}» модель «{}» ключ {}",
            model, provider_id, upstream_model, key_id
        ));

        let response = match state.http().execute(req).await {
            Ok(r) => r,
            Err(e) => {
                state.rotation().mark_failure(&provider_id, &key_id, FailureKind::Transport, None);
                state.events().warn(format!(
                    "ключ {} провайдера «{}»: транспортная ошибка ({}), пробуем следующий",
                    key_id, provider_id, e
                ));
                last = Some(openai_error(
                    StatusCode::BAD_GATEWAY,
                    format!("Не удалось связаться с провайдером «{}»: {}", provider_id, e),
                    "upstream_connection_error",
                ));
                continue;
            }
        };

        let status = response.status();
        match FailureKind::from_status(status.as_u16()) {
            Some(kind) => {
                // ── Отказ, который имеет смысл повторить ────────────────────
                let retry_after = parse_retry_after(response.headers().get("retry-after").and_then(|v| v.to_str().ok()));
                let detail = response.text().await.unwrap_or_default();
                state.rotation().mark_failure(&provider_id, &key_id, kind, retry_after);
                let cooldown = kind.cooldown(retry_after);

                if kind.deactivates_key() {
                    deactivate_key(&state, &provider_id, &key_id);
                    state.events().error(format!(
                        "ключ {} провайдера «{}» отклонён апстримом (HTTP {}) — ключ выключен",
                        key_id, provider_id, status.as_u16()
                    ));
                } else {
                    state.events().warn(format!(
                        "ключ {} провайдера «{}»: HTTP {}, пауза {} с, пробуем следующий",
                        key_id, provider_id, status.as_u16(), cooldown.as_secs()
                    ));
                }

                last = Some(upstream_error_response(
                    status,
                    &provider_id,
                    &key_id,
                    &detail,
                    kind,
                ));
                continue;
            }
            None => {
                // ── Точка невозврата: ответ отдан клиенту как есть ──────────
                // Всё, что ниже, не содержит ни одного `continue` по ключу:
                // байты уйдут клиенту, повтор приведёт к склейке двух ответов.
                let final_status = status;
                let passthrough = build_passthrough_response(response).await;
                let body_hint = summarize(passthrough.body_hint.as_deref());
                state.events().info(format!(
                    "ответ {} от провайдера «{}» ключ {} (HTTP {}){}",
                    model,
                    provider_id,
                    key_id,
                    final_status.as_u16(),
                    body_hint
                ));
                if attempted.len() > 1 {
                    state.events().warn(format!(
                        "запрос {} выполнен с ретраем по ключам: {}",
                        model,
                        attempted.join(" → ")
                    ));
                }
                return passthrough.response;
            }
        }
    }

    // ── Все ключи провайдера отказали ───────────────────────────────────────
    state.events().error(format!(
        "запрос {} не выполнен: отказали все ключи провайдера «{}» ({})",
        model,
        provider_id,
        attempted.join(", ")
    ));
    last.unwrap_or_else(|| {
        openai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("У провайдера «{}» не осталось ключей для запроса", provider_id),
            "insufficient_quota",
        )
    })
}

/// Что-то, что удалось отдать клиенту, плюс короткая выжимка для журнала.
struct Passthrough {
    response: Response,
    /// Первые символы тела, если оно не потоковое — попадают в журнал.
    body_hint: Option<String>,
}

/// Превратить ответ апстрима в ответ клиенту без буферизации потока.
///
/// Если апстрим отдал `text/event-stream`, тело переносится чанками через
/// `Body::from_stream(reqwest_stream)`: ни байта не копится в памяти, задержка
/// между токенами определяется только сетью. Иначе тело читается целиком — там
/// буферизация обязательна, иначе клиент не узнает длину ответа.
async fn build_passthrough_response(upstream: reqwest::Response) -> Passthrough {
    let status = upstream.status();
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let is_stream = content_type.starts_with("text/event-stream");
    if is_stream {
        let stream = upstream.bytes_stream();
        let response = Response::builder()
            .status(status)
            // Content-Type берём у апстрима: он знает, что именно прислал.
            .header("content-type", content_type)
            // Без content-length: длина потока неизвестна заранее. Любой
            // пересчитанный content-length «схлопнул» бы поток.
            .header("cache-control", "no-cache")
            .header("x-accel-buffering", "no")
            .body(Body::from_stream(stream))
            // Ошибка сборки возможна только при невалидном статус-коде, которого
            // здесь быть не может (status пришёл из reqwest). Отдаём 502, чтобы
            // клиент не увидел панику.
            .unwrap_or_else(|_| {
                openai_error(StatusCode::BAD_GATEWAY, "Не удалось собрать ответ", "gateway_error")
            });
        return Passthrough { response, body_hint: None };
    }

    match upstream.bytes().await {
        Ok(bytes) => {
            let hint = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]).to_string();
            let response = Response::builder()
                .status(status)
                .header(
                    "content-type",
                    if content_type.is_empty() { "application/json".to_string() } else { content_type },
                )
                .header("content-length", bytes.len())
                .body(Body::from(bytes))
                .unwrap_or_else(|_| {
                    openai_error(StatusCode::BAD_GATEWAY, "Не удалось собрать ответ", "gateway_error")
                });
            Passthrough { response, body_hint: Some(hint) }
        }
        Err(e) => Passthrough {
            response: openai_error(
                StatusCode::BAD_GATEWAY,
                format!("Не удалось прочитать ответ апстрима: {}", e),
                "upstream_read_error",
            ),
            body_hint: None,
        },
    }
}

/// Собрать запрос к апстриму из подготовленных частей.
///
/// `method` передаётся явно: `/chat/completions` — это `POST`, а `/models` —
/// `GET`. Раньше здесь был жёсткий `POST`, из-за чего опрос моделей получал
/// `405 Method Not Allowed` от любого апстрима (поймано интеграционным тестом
/// `fetch_models_*`).
fn build_request(
    state: &AppState,
    route: &Route,
    forwarded: &HeaderMap,
    api_key: &str,
    method: reqwest::Method,
    body: Vec<u8>,
) -> reqwest::Request {
    let mut req = state
        .http()
        .request(method, route.request_url())
        .header("authorization", auth_header(api_key));
    for (name, value) in forwarded.iter() {
        req = req.header(name, value);
    }
    let mut extra = HeaderMap::new();
    apply_route_headers(route, &mut extra);
    for (name, value) in extra.iter() {
        req = req.header(name, value);
    }
    // `content-type` задаём только когда есть тело: у `GET /models` его быть
    // не должно, а лишний заголовок сбивает некоторые прокси-апстримы.
    if !body.is_empty() {
        req = req.header("content-type", "application/json");
    }
    // `accept-encoding` намеренно не пересылается: клиентский клиент сжатия не
    // должен влиять на то, что получит шлюз. Ответ приходит без сжатия, и поток
    // читается без распаковки на лету.
    // `body()` уже проверяет, что тело — `Vec<u8>`, поэтому `build()` здесь не
// падает: единственная причина ошибки здесь — сетевой уровень, и она
// всплывёт при отправке с понятным текстом.
req.body(body).build().expect("тело — Vec<u8>; ошибку сборки дать не может")
}

/// Ответ клиенту по последней неуспешной попытке.
///
/// Тело апстрима возвращается как есть (там уже формат OpenAI), но с
/// добавленным указанием провайдера и ключа: пользователь должен видеть, чей
/// ключ отказал, иначе диагностика превращается в угадывание.
fn upstream_error_response(
    status: StatusCode,
    provider_id: &str,
    key_id: &str,
    detail: &str,
    kind: FailureKind,
) -> Response {
    let trimmed = detail.trim();
    if trimmed.is_empty() {
        return openai_error(
            status,
            format!(
                "Провайдер «{}» ответил HTTP {} (ключ {})",
                provider_id,
                status.as_u16(),
                key_id
            ),
            openai_error_type(kind),
        );
    }
    // Тело провайдера — JSON. Пытаемся разобрать и достроить наш ответ,
    // сохранив исходный текст в сообщении, если разбор не удался.
    let mut body: serde_json::Value = serde_json::from_str(trimmed)
        .unwrap_or_else(|_| json!({ "error": { "message": trimmed } }));
    let note = format!("[провайдер {}, ключ {}]", provider_id, key_id);
    if let Some(err) = body.get_mut("error").and_then(|e| e.as_object_mut()) {
        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or(trimmed);
        err.insert("message".to_string(), json!(format!("{} {}", note, msg)));
        err.entry("type").or_insert_with(|| json!(openai_error_type(kind)));
    } else {
        body = json!({ "error": { "message": format!("{} {}", note, trimmed), "type": openai_error_type(kind) } });
    }
    (status, axum::Json(body)).into_response()
}

/// Тип ошибки в терминах OpenAI, понятный IDE.
fn openai_error_type(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::Auth => "authentication_error",
        FailureKind::RateLimit => "rate_limit_error",
        FailureKind::Server => "api_error",
        FailureKind::Transport => "api_connection_error",
    }
}

/// Навсегда выключить ключ (апстрим сказал 401/403).
///
/// Запись в конфиг обязательна: иначе после перезапуска шлюза тот же мёртвый
/// ключ снова станет кандидатом, и пользователь получит тот же отказ по кругу.
fn deactivate_key(state: &AppState, provider_id: &str, key_id: &str) {
    let result = state.update_config(|config| {
        if let Some(provider) = config.provider_mut(provider_id) {
            if let Some(key) = provider.keys.iter_mut().find(|k| k.id == key_id) {
                key.is_active = false;
            }
        }
        Ok(())
    });
    if let Err(e) = result {
        state.events().error(format!(
            "не удалось сохранить деактивацию ключа {} провайдера «{}»: {}",
            key_id, provider_id, e
        ));
    }
}

/// Короткая выжимка тела ответа для журнала (одной строкой).
fn summarize(body: Option<&str>) -> String {
    match body {
        None => String::new(),
        Some(s) => {
            let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
            let short: String = one_line.chars().take(160).collect();
            if short.is_empty() {
                String::new()
            } else {
                format!(" · {}", short)
            }
        }
    }
}

/// `GET {base_url}/models` провайдера: реальный опрос, без хардкода.
///
/// Ошибка возвращается пользователю как есть: молчаливый пустой список означал
/// бы «у провайдера нет моделей», хотя на деле сломался ключ или сеть.
pub async fn fetch_provider_models(
    state: &AppState,
    provider: &ProviderConfig,
    config: &GatewayConfig,
) -> Result<Vec<cloud_routers_core::config::DiscoveredModel>, String> {
    let route = resolve_route(provider, config, "/models")?;
    let key = provider
        .active_keys()
        .map(|k| k.key.clone())
        .next()
        .ok_or_else(|| {
            format!("У провайдера «{}» нет включённых ключей", provider.id)
        })?;
    let req = build_request(&state, &route, &HeaderMap::new(), &key, reqwest::Method::GET, Vec::new());
    let response = state.http().execute(req).await.map_err(|e| {
        format!("Не удалось получить список моделей у «{}»: {}", provider.name, e)
    })?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "Провайдер «{}» ответил HTTP {} на /models: {}",
            provider.name,
            status.as_u16(),
            text.chars().take(300).collect::<String>()
        ));
    }
    cloud_routers_core::models::parse_models_response(&text)
}

/// Проверка связи с провайдером: замер латентности `GET /models`.
pub struct PingResult {
    pub latency_ms: u64,
    pub models_count: usize,
    pub via_relay: bool,
    pub target: String,
}

pub async fn ping_provider(
    state: &AppState,
    provider: &ProviderConfig,
    config: &GatewayConfig,
) -> Result<PingResult, String> {
    let route = resolve_route(provider, config, "/models")?;
    let key = provider
        .active_keys()
        .map(|k| k.key.clone())
        .next()
        .ok_or_else(|| format!("У провайдера «{}» нет включённых ключей", provider.id))?;
    let started = std::time::Instant::now();
    let req = build_request(state, &route, &HeaderMap::new(), &key, reqwest::Method::GET, Vec::new());
    let response = state.http().execute(req).await.map_err(|e| {
        format!("Не удалось связаться с провайдером «{}»: {}", provider.name, e)
    })?;
    let status = response.status();
    let latency_ms = started.elapsed().as_millis() as u64;
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "Провайдер «{}» ответил HTTP {} за {} мс",
            provider.name,
            status.as_u16(),
            latency_ms
        ));
    }
    let models_count = cloud_routers_core::models::parse_models_response(&text)
        .map(|m| m.len())
        .unwrap_or(0);
    Ok(PingResult {
        latency_ms,
        models_count,
        via_relay: route.is_relay(),
        target: trim_base_url(&provider.base_url),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use cloud_routers_core::config::AccountKey;

    #[tokio::test]
    async fn openai_error_shape_is_parseable_by_ides() {
        let r = openai_error(StatusCode::BAD_REQUEST, "плохой запрос", "invalid_request_error");
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"]["message"], "плохой запрос");
        assert_eq!(v["error"]["type"], "invalid_request_error");
        assert!(v["error"].get("param").is_some());
        assert!(v["error"].get("code").is_some());
    }

    #[tokio::test]
    async fn unknown_model_lists_available_combos() {
        let r = unknown_model("groq/nope", &["groq/m1".to_string(), "deepseek/m2".to_string()]);
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let bytes = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("groq/nope"));
        assert!(text.contains("groq/m1"), "{text}");
        assert!(text.contains("deepseek/m2"), "{text}");
    }

    #[tokio::test]
    async fn unknown_model_explains_empty_configuration() {
        let r = unknown_model("groq/m1", &[]);
        let bytes = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("нет ни одного настроенного провайдера"), "{text}");
    }

    #[tokio::test]
    async fn upstream_error_annotates_provider_and_key() {
        let detail = r#"{"error":{"message":"rate limited","type":"rate_limit_error"}}"#;
        let r = upstream_error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "groq",
            "k1",
            detail,
            FailureKind::RateLimit,
        );
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert!(v["error"]["message"].as_str().expect("msg").contains("провайдер groq"));
        assert!(v["error"]["message"].as_str().expect("msg").contains("ключ k1"));
        // Тип провайдера не затираем нашим.
        assert_eq!(v["error"]["type"], "rate_limit_error");
    }

    #[tokio::test]
    async fn upstream_error_survives_non_json_body() {
        let r = upstream_error_response(
            StatusCode::BAD_GATEWAY,
            "groq",
            "k1",
            "<html>502 Bad Gateway</html>",
            FailureKind::Server,
        );
        let bytes = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert!(v["error"]["message"].as_str().expect("msg").contains("Bad Gateway"));
        assert_eq!(v["error"]["type"], "api_error");
    }

    #[tokio::test]
    async fn upstream_error_handles_empty_body() {
        let r = upstream_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "groq",
            "k2",
            "   ",
            FailureKind::Server,
        );
        let bytes = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert!(v["error"]["message"].as_str().expect("msg").contains("HTTP 503"));
    }

    #[test]
    fn error_types_map_to_openai_vocabulary() {
        assert_eq!(openai_error_type(FailureKind::Auth), "authentication_error");
        assert_eq!(openai_error_type(FailureKind::RateLimit), "rate_limit_error");
        assert_eq!(openai_error_type(FailureKind::Server), "api_error");
        assert_eq!(openai_error_type(FailureKind::Transport), "api_connection_error");
    }

    #[test]
    fn summarize_keeps_log_single_line() {
        assert_eq!(summarize(None), "");
        assert_eq!(summarize(Some("   ")), "");
        let s = summarize(Some("{\"a\":1}\n   {\"b\": 2}"));
        assert!(s.starts_with(" · "), "{s}");
        assert!(!s.contains('\n'), "в журнале не должно быть переносов: {s:?}");
    }

    #[test]
    fn summarize_truncates_long_body() {
        let long = "x".repeat(5000);
        let s = summarize(Some(&long));
        assert!(s.chars().count() <= 165, "слишком длинно: {}", s.chars().count());
    }

    #[tokio::test]
    async fn missing_model_field_is_rejected_before_routing() {
        let state = AppState::new(&std::env::temp_dir().join("cr_gw_proxy_nomodel")).expect("state");
        let r = chat_completions(
            axum::extract::State(state),
            HeaderMap::new(),
            Bytes::from_static(br#"{"messages":[]}"#),
        )
        .await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert!(v["error"]["message"].as_str().expect("msg").contains("model"));
    }

    #[tokio::test]
    async fn malformed_json_is_rejected_with_400() {
        let state = AppState::new(&std::env::temp_dir().join("cr_gw_proxy_badjson")).expect("state");
        let r = chat_completions(
            axum::extract::State(state),
            HeaderMap::new(),
            Bytes::from_static(b"{not json"),
        )
        .await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unknown_combo_is_404_before_any_network_call() {
        let state = AppState::new(&std::env::temp_dir().join("cr_gw_proxy_404")).expect("state");
        let r = chat_completions(
            axum::extract::State(state),
            HeaderMap::new(),
            Bytes::from_static(br#"{"model":"groq/nope","messages":[]}"#),
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn provider_without_usable_keys_returns_503() {
        let dir = std::env::temp_dir().join("cr_gw_proxy_nokeys");
        let _ = std::fs::remove_dir_all(&dir);
        let mut cfg = cloud_routers_core::GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-1".into(), is_active: false }],
            models: vec![cloud_routers_core::DiscoveredModel { id: "m1".into(), ..Default::default() }],
            ..Default::default()
        });
        std::fs::create_dir_all(&dir).expect("mkdir");
        cloud_routers_core::save_config(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");

        let r = chat_completions(
            axum::extract::State(state),
            HeaderMap::new(),
            Bytes::from_static(br#"{"model":"groq/m1","messages":[]}"#),
        )
        .await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert!(v["error"]["message"].as_str().expect("msg").contains("включено 0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn relay_configured_without_url_surfaces_configuration_error() {
        let dir = std::env::temp_dir().join("cr_gw_proxy_relaycfg");
        let _ = std::fs::remove_dir_all(&dir);
        let mut cfg = cloud_routers_core::GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            use_relay: true,
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-1".into(), is_active: true }],
            models: vec![cloud_routers_core::DiscoveredModel { id: "m1".into(), ..Default::default() }],
            ..Default::default()
        });
        std::fs::create_dir_all(&dir).expect("mkdir");
        cloud_routers_core::save_config(&dir, &cfg).expect("save");
        let state = AppState::new(&dir).expect("state");

        let r = chat_completions(
            axum::extract::State(state),
            HeaderMap::new(),
            Bytes::from_static(br#"{"model":"groq/m1","messages":[]}"#),
        )
        .await;
        assert_eq!(r.status(), StatusCode::BAD_GATEWAY);
        let bytes = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"]["type"], "configuration_error");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
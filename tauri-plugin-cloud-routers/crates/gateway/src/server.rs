//! Сборка HTTP-поверхности шлюза.
//!
//! ## Раскладка
//!
//! | Маршрут | Кто ходит | Назначение
//! |---|---|---|
//! | `GET /health` | плагин, человек | проверка живости
//! | `GET /v1/models` | IDE | список комбо, OpenAI-формат
//! | `POST /v1/chat/completions` | IDE | прокси инференса
//! | `GET /api/combos` | хост (панель) | список комбо в формате плагина
//! | `GET /dashboard*` | человек, iframe хоста | настройки
//! | `/api/**` | дашборд | управление
//!
//! Два формата списка моделей дублируют друг друга **намеренно**: `/v1/models`
//! читают IDE, `/api/combos` — существующий плагин-контракт (`client.rs`), где
//! комбо выбираются по `kind == "llm"`. Оба строятся из одного
//! `list_combos()`, поэтому разойтись не могут.
//!
//! ## CORS
//!
//! `CorsLayer::permissive()` — по требованию: локальные IDE и расширения
//! браузера приходят со своим origin, и без CORS они не смогут опросить
//! `/v1/models`. Шлюз слушает **только** loopback, а не авторизацию запросов на
//! добавление/удаление провайдеров не распространяется — это следствие того же
//! решения, что и «любой источник»: страница в браузере может достучаться до
//! порта, как и IDE. Осознанная цена записана в README плагина.

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;

use cloud_routers_core::models::{list_combos, openai_models_list};

use crate::admin;
use crate::app::AppState;
use crate::proxy;

/// Ресурсы дашборда, вшитые в бинарь.
///
/// `include_str!`, а не чтение с диска: бинарь должен работать при переносе в
/// другую папку, а наличие файлов дашборда рядом с exe не гарантировано.
const DASHBOARD_HTML: &str = include_str!("../assets/dashboard/index.html");
const DASHBOARD_CSS: &str = include_str!("../assets/dashboard/style.css");
const DASHBOARD_JS: &str = include_str!("../assets/dashboard/app.js");

fn asset(content_type: &'static str, body: &'static str) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            // Шлюз обслуживает живые данные; кеш здесь приводил бы к
            // «я изменил настройку, а страница показывает старое».
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

async fn dashboard_html() -> Response {
    asset("text/html; charset=utf-8", DASHBOARD_HTML)
}

async fn dashboard_css() -> Response {
    asset("text/css; charset=utf-8", DASHBOARD_CSS)
}

async fn dashboard_js() -> Response {
    asset("text/javascript; charset=utf-8", DASHBOARD_JS)
}

/// `GET /health` — лёгкая проверка без чтения конфига.
async fn health(State(state): State<AppState>) -> Response {
    let config = state.config();
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "status": "ok",
            "version": cloud_routers_core::VERSION,
            "uptime_sec": state.uptime().as_secs(),
            "port": config.port,
            "providers": config.providers.len(),
        })),
    )
        .into_response()
}

/// `GET /v1/models` — список комбо в формате OpenAI.
///
/// `owned_by` обязан быть `"combo"`: клиент плагина отбрасывает всё с другим
/// значением, считая это локальными gguf-моделями.
async fn openai_models(State(state): State<AppState>) -> Response {
    let config = state.config();
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    axum::Json(openai_models_list(&config.providers, created)).into_response()
}

/// `GET /api/combos` — тот же список в формате плагина.
async fn combos(State(state): State<AppState>) -> Response {
    let config = state.config();
    axum::Json(serde_json::json!({ "combos": list_combos(&config.providers) })).into_response()
}

/// Собрать полный роутер.
pub fn build(state: AppState) -> Router {
    // Лимит тела: 64 МБ (см. proxy::MAX_BODY_BYTES) — запросы с картинками
    // приходят base64 внутри JSON. Стандартные 2 МБ axum их обрезали бы.
    let limit = DefaultBodyLimit::max(proxy::MAX_BODY_BYTES);
    // Маршрут клонируется: один и тот же обработчик висит на два пути
    // (`/v1/chat/completions` и legacy `/chat/completions`).
    let chat = post(proxy::chat_completions).layer(limit.clone());

    Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(openai_models))
        .route("/v1/chat/completions", chat.clone())
        // Традиционный путь без /v1 — часть клиентов (Aider, часть плагинов
        // IDE) ходит именно так.
        .route("/chat/completions", chat)
        .route("/api/combos", get(combos))
        .route("/dashboard", get(dashboard_html))
        .route("/dashboard/", get(dashboard_html))
        .route("/dashboard/style.css", get(dashboard_css))
        .route("/dashboard/app.js", get(dashboard_js))
        .route("/style.css", get(dashboard_css))
        .route("/app.js", get(dashboard_js))
        .route("/", get(dashboard_html))
        .merge(admin::router())
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .layer(limit)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use cloud_routers_core::config::{AccountKey, DiscoveredModel, ProviderConfig};
    use tower::ServiceExt;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cr_gw_server_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn state_with_combo(dir: &std::path::Path) -> AppState {
        std::fs::create_dir_all(dir).expect("mkdir");
        let mut cfg = cloud_routers_core::GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            keys: vec![AccountKey { id: "k1".into(), key: "gsk-1".into(), is_active: true }],
            models: vec![DiscoveredModel { id: "llama-3.3-70b-versatile".into(), ..Default::default() }],
            ..Default::default()
        });
        cloud_routers_core::save_config(dir, &cfg).expect("save");
        AppState::new(dir).expect("state")
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let r = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).expect("req"))
            .await
            .expect("response");
        let status = r.status();
        let bytes = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn health_reports_version_and_port() {
        let dir = tmp("health");
        let app = build(AppState::new(&dir).expect("state"));
        let (status, v) = get_json(app, "/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["status"], "ok");
        assert_eq!(v["version"], cloud_routers_core::VERSION);
        assert_eq!(v["port"], 20131);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Регрессия 17.10.2026: фон модалки объявлялся сокращением
    /// `inset: 0`, которое движок, не поддерживающий сокращение, отбрасывает
    /// целиком — подложка модалки оставалась `position: fixed` без отступов и
    /// «прилипала» к углу: вёрстка летит, модалок не видно. Теперь оверлей
    /// задан четырьмя явными отступами; запрет сокращения здесь — инвариант
    /// домена (отказ невозможен конструктивно), а не стилистика.
    #[test]
    fn dashboard_css_positions_modal_backdrop_with_explicit_offsets() {
        assert!(
            !DASHBOARD_CSS.contains("inset:"),
            "CSS дашборда использует сокращение `inset:` — движки, которые его не понимают, молча отбрасывают правило целиком"
        );
        assert!(
            DASHBOARD_CSS.contains(".gw-modal-backdrop"),
            "подложка модалки (gw-modal-backdrop) обязана объявляться в CSS дашборда"
        );
        for prop in ["top:", "right:", "bottom:", "left:"] {
            assert!(
                DASHBOARD_CSS.contains(prop),
                "в CSS дашборда отсутствует offset-свойство `{prop}`"
            );
        }
    }

    /// Правило «Namespace CSS» (crates/gateway/docs/README.md): КАЖДЫЙ класс
    /// в разметке дашборда обязан начинаться с префикса `gw-`. Причина —
    /// реальный кейс 22.10.2026: расширение браузера (XTranslate) на лету
    /// вставляет свой CSS, стилизующий свои всплывашки по голому `.modal`,
    /// и накрывает дашборд (окно сплющивается в 1px и уезжает в угол).
    /// Голые имена (`modal`, `card`, `toast`, `btn`…) — это чужая территория:
    /// любое совпадение класс-имён — это гонка с произвольным CSS окружения.
    /// Решение по правилу 00.2: отказ становится невозможен структурно —
    /// свои классы не пересекаются с чужими, тест не даёт регрессию.
    ///
    /// Сканер покрывает все три носителя имён: статический HTML, JS-шаблоны
    /// (`class="..."` генерируются кодом карточек), JS-селекторы/тогглы
    /// (`$$('.x')`, `classList.toggle('x')`) и селекторы CSS.
    #[test]
    fn dashboard_classes_live_in_gw_namespace() {
        fn dotted_tokens(source: &str) -> (usize, Vec<String>) {
            // Собирает ВСЕ `class="..."`-литералы токенами; возвращает
            // (количество собранных классов, список токенов-нарушителей).
            let mut count = 0usize;
            let mut offenders = Vec::new();
            let mut rest = source;
            while let Some(open) = rest.find("class=\"") {
                let after = &rest[open + 7..];
                match after.find('"') {
                    None => break,
                    Some(close) => {
                        for tok in after[..close].split_whitespace() {
                            // Имя CSS-класса — всегда начинается буквой/`_`/`-`.
                            // Всё прочее (`?`, `:`, фрагменты интерполяций
                            // `${...}` и хвосты тернарников типа `'gw-off'}`) —
                            // вычисляемые части шаблонов, а не имена классов;
                            // сюда идут только статические идентификаторы.
                            let mut cs = tok.chars();
                            let first = cs.next().map(|c| {
                                c.is_ascii_alphabetic() || c == '_'
                            });
                            if first != Some(true) {
                                continue;
                            }
                            count += 1;
                            if !tok.starts_with("gw-") {
                                offenders.push(tok.to_string());
                            }
                        }
                        rest = &after[close + 1..];
                    }
                }
            }
            (count, offenders)
        }

        let (html_n, html_bad) = dotted_tokens(DASHBOARD_HTML);
        assert!(
            html_bad.is_empty() && html_n >= 30,
            "в index.html классы вне пространства gw-: {html_bad:?} (собрано {html_n}) — правило «Namespace CSS» (docs/README.md)"
        );

        let (js_n, js_bad) = dotted_tokens(DASHBOARD_JS);
        assert!(
            js_bad.is_empty() && js_n >= 40,
            "в app.js сгенерированные шаблонами классы вне gw-: {js_bad:?} (собрано {js_n})"
        );

        // JS-селекторы: `$$('.x')` / `$('.x')` обязаны тоже жить в gw-.
        // Сканим только расширенные селекторы точкой; `$('#id')` — не классы.
        let mut sel_rest = DASHBOARD_JS;
        while let Some(open) = sel_rest.find("$('") {
            let after = &sel_rest[open + 3..];
            sel_rest = match after.find('\'') {
                None => break,
                Some(close) => {
                    let sel = &after[..close];
                    if let Some(tok) = sel.strip_prefix('.') {
                        let tok: String = tok
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                            .collect();
                        assert!(
                            tok.starts_with("gw-"),
                            "JS-селектор `.{tok}` вне пространства gw- (app.js)"
                        );
                    }
                    &after[close + 1..]
                }
            };
        }

        // classList.toggle('x') — динамические класс-имёна (busy/active).
        let mut tog_rest = DASHBOARD_JS;
        while let Some(open) = tog_rest.find("classList.toggle('") {
            let after = &tog_rest[open + "classList.toggle('".len()..];
            tog_rest = match after.find('\'') {
                None => break,
                Some(close) => {
                    let tok = &after[..close];
                    assert!(
                        tok.starts_with("gw-"),
                        "classList.toggle('{tok}') вне пространства gw- (app.js)"
                    );
                    &after[close + 1..]
                }
            };
        }

        // CSS: комментарии выброшены, дальше скан ВСЕГО файла слева направо.
        // Токен — `.` + буква/`_`/`-` + [alnum-_]; `rgba(0,0,0,.6)` отсекается
        // guard'ом: '.6' начинается с цифры и токеном не считается. Структура
        // (селектор vs тело) здесь не разыгрывается: точки в НУЛЕВЫХ значениях
        // никогда не начинают токен с буквы — вызовов «.word» вне селекторов
        // в этом CSS нет.
        let (css_n, css_bad) = {
            fn strip_css_comments(src: &str) -> String {
                let mut out = String::with_capacity(src.len());
                let mut rest = src;
                while let Some(open) = rest.find("/*") {
                    if open > 0 {
                        out.push_str(&rest[..open]);
                    }
                    match rest[open..].find("*/") {
                        None => return out,
                        Some(end) => rest = &rest[open + end + 2..],
                    }
                }
                out.push_str(rest);
                out
            }
            let css = strip_css_comments(DASHBOARD_CSS);
            let mut offenders = Vec::new();
            let mut count = 0usize;
            let chars: Vec<char> = css.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == '.'
                    && i + 1 < chars.len()
                    && (chars[i + 1].is_ascii_alphabetic()
                        || chars[i + 1] == '_'
                        || chars[i + 1] == '-')
                {
                    let start = i + 1;
                    let mut j = start;
                    while j < chars.len()
                        && (chars[j].is_ascii_alphanumeric()
                            || chars[j] == '_'
                            || chars[j] == '-')
                    {
                        j += 1;
                    }
                    count += 1;
                    let tok: String = chars[start..j].iter().collect();
                    if !tok.starts_with("gw-") {
                        offenders.push(tok);
                    }
                    i = j;
                } else {
                    i += 1;
                }
            }
            (count, offenders)
        };
        assert!(
            css_bad.is_empty() && css_n >= 25,
            "в style.css классы вне gw-: {css_bad:?} (собрано {css_n})"
        );
    }

    #[tokio::test]
    async fn v1_models_uses_owned_by_combo_for_host_compatibility() {
        let dir = tmp("models");
        let app = build(state_with_combo(&dir));
        let (status, v) = get_json(app, "/v1/models").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["object"], "list");
        let data = v["data"].as_array().expect("data");
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["id"], "groq/llama-3.3-70b-versatile");
        // Именно это значение ждёт клиент плагина; другое отфильтруется.
        assert_eq!(data[0]["owned_by"], "combo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn api_combos_matches_v1_models_ids() {
        // Два формата строятся из одного list_combos — разойтись не могут.
        let dir = tmp("combos");
        let state = state_with_combo(&dir);
        let (s1, v1) = get_json(build(state), "/v1/models").await;
        let (_, v2) = get_json(build(state_with_combo(&dir)), "/api/combos").await;
        assert_eq!(s1, StatusCode::OK);
        let ids1: Vec<&str> = v1["data"]
            .as_array()
            .expect("data")
            .iter()
            .map(|m| m["id"].as_str().expect("id"))
            .collect();
        let ids2: Vec<&str> = v2["combos"]
            .as_array()
            .expect("combos")
            .iter()
            .map(|c| c["name"].as_str().expect("name"))
            .collect();
        assert_eq!(ids1, ids2);
        assert_eq!(v2["combos"][0]["kind"], "llm");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn combos_empty_when_no_providers() {
        let dir = tmp("nocombo");
        let app = build(AppState::new(&dir).expect("state"));
        let (status, v) = get_json(app, "/api/combos").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["combos"].as_array().expect("combos").len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn dashboard_is_served_without_network() {
        let dir = tmp("dash");
        let app = build(AppState::new(&dir).expect("state"));
        let r = app
            .oneshot(Request::builder().uri("/dashboard").body(Body::empty()).expect("req"))
            .await
            .expect("response");
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            r.headers().get(header::CONTENT_TYPE).expect("ct"),
            "text/html; charset=utf-8"
        );
        assert_eq!(r.headers().get(header::CACHE_CONTROL).expect("cc"), "no-store");
        let body = to_bytes(r.into_body(), usize::MAX).await.expect("body");
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("<!DOCTYPE html>"), "дашборд должен быть полноценной страницей");
        assert!(html.contains("app.js"), "дашборд должен подключать свой скрипт");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn dashboard_assets_are_served_with_right_types() {
        let dir = tmp("assets");
        let app = build(AppState::new(&dir).expect("state"));
        for (uri, ct) in [
            ("/dashboard/style.css", "text/css; charset=utf-8"),
            ("/dashboard/app.js", "text/javascript; charset=utf-8"),
            ("/style.css", "text/css; charset=utf-8"),
            ("/app.js", "text/javascript; charset=utf-8"),
        ] {
            let r = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).expect("req"))
                .await
                .expect("response");
            assert_eq!(r.status(), StatusCode::OK, "{uri}");
            assert_eq!(r.headers().get(header::CONTENT_TYPE).expect("ct"), ct, "{uri}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn root_redirects_to_dashboard_content() {
        // Частая ошибка ввода: открыть http://127.0.0.1:20131 и увидеть 404.
        let dir = tmp("root");
        let app = build(AppState::new(&dir).expect("state"));
        let (status, _) = get_json(app, "/").await;
        assert_eq!(status, StatusCode::OK);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn cors_is_permissive_so_local_ides_work() {
        let dir = tmp("cors");
        let app = build(AppState::new(&dir).expect("state"));
        let r = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .header(header::ORIGIN, "vscode-webview://abc")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("response");
        let allow_origin = r
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("ACAO должен присутствовать для IDE из расширения");
        assert!(
            allow_origin == "*" || allow_origin == "vscode-webview://abc",
            "неожиданный ACAO: {:?}",
            allow_origin
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn preflight_for_post_chat_completions_is_allowed() {
        // Без preflight IDE не сможет отправить POST с телом и заголовками.
        let dir = tmp("preflight");
        let app = build(AppState::new(&dir).expect("state"));
        let r = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::OPTIONS)
                    .uri("/v1/chat/completions")
                    .header(header::ORIGIN, "http://localhost:5173")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("response");
        assert!(
            r.status() == StatusCode::OK || r.status().is_success(),
            "preflight должен проходить: {}",
            r.status()
        );
        assert!(r.headers().contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn legacy_chat_path_without_v1_is_served() {
        let dir = tmp("legacy");
        let app = build(state_with_combo(&dir));
        let r = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/chat/completions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"model":"groq/nope","messages":[]}"#))
                    .expect("req"),
            )
            .await
            .expect("response");
        // Не 404/405: маршрут существует, ответ — «модель не найдена».
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
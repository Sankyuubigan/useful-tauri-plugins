//! Интеграционные тесты прокси против локального мок-апстрима.
//!
//! ## Зачем именно так
//!
//! Модульные тесты проверяют чистые функции (ротация, разбор комбо, формы
//! ошибок). Но главные риски этого модуля — **взаимодействие** с настоящим
//! HTTP-сокетом: буферизация стрима, повтор по ключу, деактивация после `401`.
//! Их нельзя поймать тестом чистой функции — нужен апстрим, который ведёт себя
//! как вредный провайдер.
//!
//! Мок поднимается на `127.0.0.1` тем же `axum`, что и сам шлюз: тест идёт по
//! тому же коду, что и production, без подмен транспорта, которые могли бы
//! скрыть баг именно в стриминге.
//!
//! ## Две ошибки, которые здесь уже стоили времени
//!
//! 1. **Слушатель мока обязан быть неблокирующим.** `std::net::TcpListener`
//!    блокирующий, и `accept()` внутри `axum::serve` замораживал поток
//!    однопоточного рантайма теста — запросы «висли» молча, без ошибки.
//!    Поэтому только `tokio::net::TcpListener::bind`.
//!
//! 2. **Ни один `MutexGuard` не переживает `.await`.** Иначе будущее
//!    обработчика перестаёт быть `Send`, axum отказывается регистрировать
//!    обработчик — и сообщает об этом невнятно, через «trait `Handler` не
//!    реализован». Все блокировки берутся и отпускаются в пределах выражения.
//!
//! ## Чего здесь нет
//!
//! Настоящих LLM и настоящих API-ключей: тесты обязаны быть детерминированными
//! и не требовать сети. Проверка «пойдёт ли инференс у Groq» — ручной тест на
//! реальном ключе, а не юнит-тест.

#![cfg(test)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;

use cloud_routers_core::config::{
    AccountKey, ComboConfig, ComboMember, DiscoveredModel, GatewayConfig, ProviderConfig,
};
use tower::ServiceExt;

use crate::app::AppState;
use crate::proxy;

/// Ответ мока на `/models`.
fn models_body_json() -> String {
    r#"{"object":"list","data":[{"id":"m1","object":"model"},{"id":"m2","object":"model"}]}"#.to_string()
}

/// Канонический успешный ответ чата.
fn ok_chat_behavior() -> MockBehavior {
    MockBehavior {
        chat_status: 200,
        chat_content_type: "application/json".into(),
        chat_body: br#"{"ok":true}"#.to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    }
}

/// Поведение мока.
#[derive(Clone, Default)]
struct MockBehavior {
    models_status: u16,
    models_body: String,
    chat_status: u16,
    chat_content_type: String,
    chat_body: Vec<u8>,
    /// Задержка перед ответом на чат (мс).
    chat_delay_ms: u64,
    /// Задержка между кусками SSE-потока (мс).
    chat_chunk_delay_ms: u64,
}

impl MockBehavior {
    /// Поведение, которым мок отвечает на «неудачные» первые вызовы.
    fn error(status: u16, body: &str) -> MockBehavior {
        MockBehavior {
            chat_status: status,
            chat_content_type: "application/json".into(),
            chat_body: body.as_bytes().to_vec(),
            models_body: models_body_json(),
            ..Default::default()
        }
    }
}

/// Состояние мока: поведение, счётчики обращений, журнал запросов.
#[derive(Default)]
struct MockState {
    behavior: Mutex<MockBehavior>,
    /// Поведение для «неудачных» первых вызовов.
    failure: Mutex<MockBehavior>,
    /// Сколько первых вызовов `/chat/completions` должны отдать `failure`.
    ///
    /// Так тесты ротации **детерминированы**: «первый ключ получает 429,
    /// второй — 200» не зависит от того, успел ли фоновой поток переключить
    /// поведение. Вариант с переключением из отдельного потока гоняется: на
    /// медленной машине переключение не успевало, и тест «проходил» бы не по тем
    /// ключам, которые проверял.
    fail_first_calls: AtomicUsize,
    chat_calls: AtomicUsize,
    models_calls: AtomicUsize,
    chat_auth_headers: Mutex<Vec<String>>,
    chat_models: Mutex<Vec<String>>,
    chat_header_names: Mutex<Vec<Vec<String>>>,
}

impl MockState {
    fn set(&self, b: MockBehavior) {
        *self.behavior.lock().unwrap_or_else(|p| p.into_inner()) = b;
    }
    fn behavior(&self) -> MockBehavior {
        self.behavior.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    /// Поведение для текущего вызова: пока не исчерпан запас неудачных попыток,
    /// отдаём `failure`, иначе основное поведение.
    fn behavior_for_call(&self) -> MockBehavior {
        let remaining = self.fail_first_calls.load(Ordering::SeqCst);
        if remaining > 0 {
            self.fail_first_calls.store(remaining - 1, Ordering::SeqCst);
            return self.failure.lock().unwrap_or_else(|p| p.into_inner()).clone();
        }
        self.behavior()
    }
    fn auths(&self) -> Vec<String> {
        self.chat_auth_headers.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn models(&self) -> Vec<String> {
        self.chat_models.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn header_names(&self) -> Vec<Vec<String>> {
        self.chat_header_names.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn chat_count(&self) -> usize {
        self.chat_calls.load(Ordering::SeqCst)
    }
}

/// `GET /models` мока.
async fn mock_models(State(s): State<Arc<MockState>>) -> Response {
    s.models_calls.fetch_add(1, Ordering::SeqCst);
    let b = s.behavior();
    let status = StatusCode::from_u16(b.models_status).unwrap_or(StatusCode::OK);
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        b.models_body,
    )
        .into_response()
}

/// `POST /chat/completions` мока.
///
/// Порядок экстракторов обязателен для axum: сначала «дешёвые» части
/// (`State`, `HeaderMap`), затем **последним** читающий тело `Bytes`.
async fn mock_chat(
    State(s): State<Arc<MockState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    s.chat_calls.fetch_add(1, Ordering::SeqCst);
    let b = s.behavior_for_call();

    // Каждый guard живёт только внутри своего выражения — см. предупреждение
    // о `Send` в заголовке модуля.
    s.chat_auth_headers
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string(),
        );
    let names: Vec<String> = headers.keys().map(|k| k.as_str().to_ascii_lowercase()).collect();
    s.chat_header_names
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(names);

    // Реально пришедшая модель: по ней судим, что подстановка
    // combo_id -> апстрим-id работает.
    let model = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();
    s.chat_models
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(model);

    if b.chat_delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(b.chat_delay_ms)).await;
    }

    let status = StatusCode::from_u16(b.chat_status).unwrap_or(StatusCode::OK);
    if b.chat_content_type.starts_with("text/event-stream") {
        // Стримим чанками с задержкой: тест «первый чанк раньше конца» возможен
        // только при настоящей потоковой передаче, а не при буферизации.
        let chunk_delay = b.chat_chunk_delay_ms;
        let chunks = b.chat_body.clone();
        let stream = async_stream::stream! {
            let mut buf = chunks;
            while !buf.is_empty() {
                let take = buf.len().min(48).max(1);
                let piece: Vec<u8> = buf.drain(..take).collect();
                yield Ok::<_, std::io::Error>(bytes::Bytes::from(piece));
                if chunk_delay > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(chunk_delay)).await;
                }
            }
        };
        Response::builder()
            .status(status)
            .header(axum::http::header::CONTENT_TYPE, b.chat_content_type.clone())
            .body(Body::from_stream(stream))
            .expect("build")
    } else {
        (
            status,
            [(axum::http::header::CONTENT_TYPE, b.chat_content_type.clone())],
            b.chat_body.clone(),
        )
            .into_response()
    }
}

fn mock_router(state: Arc<MockState>) -> Router {
    // Тип состояния указан ЯВНО: обработчики с разным числом аргументов
    // (`State` против `State` + `HeaderMap` + `Bytes`) не дают выводу типа
    // согласоваться, а `.with_state(..)` его не сужает.
    Router::<Arc<MockState>>::new()
        .route("/models", get(mock_models))
        .route("/chat/completions", post(mock_chat))
        .route("/v1/chat/completions", post(mock_chat))
        // Нативный путь проверки Gemini-ключей: сегмент начинается с буквы —
        // это литерал для axum 0.7, а не параметр. Другие тесты по нему не ходят.
        .route("/v1beta/models/gemini-2.0-flash:countTokens", post(mock_chat))
        .with_state(state)
}

/// Поднять мок на свободном порту. Возвращает базовый URL и состояние.
async fn start_mock(behavior: MockBehavior) -> (String, Arc<MockState>) {
    let state = Arc::new(MockState::default());
    state.set(behavior);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let addr: SocketAddr = listener.local_addr().expect("addr");
    let router = mock_router(state.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://127.0.0.1:{}", addr.port()), state)
}

/// Поднять мок, где первые `fail_first` вызовов чата отдают `failure`.
async fn start_mock_with(
    behavior: MockBehavior,
    failure: MockBehavior,
    fail_first: usize,
) -> (String, Arc<MockState>) {
    let state = Arc::new(MockState::default());
    state.set(behavior);
    *state.failure.lock().unwrap_or_else(|p| p.into_inner()) = failure;
    state.fail_first_calls.store(fail_first, Ordering::SeqCst);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let addr: SocketAddr = listener.local_addr().expect("addr");
    let router = mock_router(state.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://127.0.0.1:{}", addr.port()), state)
}

/// Счётчик для уникальных имён временных каталогов.
///
/// Без него тесты, идущие параллельно (`--test-threads=4`), делили один каталог:
/// чужой `remove_dir_all` удалял каталог в середине `save`, и падало всё сразу с
/// «os error 2». Имя теперь уникально на каждый вызов.
static TMP_SEQ: AtomicUsize = AtomicUsize::new(0);

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    let seq = TMP_SEQ.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("cr_gw_it_{tag}_{seq}"));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn key(id: &str, secret: &str, active: bool) -> AccountKey {
    AccountKey { id: id.to_string(), key: secret.to_string(), is_active: active, name: None }
}

/// Состояние шлюза с одним провайдером, указывающим на `base_url`.
fn state_for(base_url: &str, keys: Vec<AccountKey>) -> (AppState, std::path::PathBuf) {
    let dir = tmp_dir(&format!("s{}_{}", keys.len(), keys.iter().map(|k| k.id.as_str()).collect::<Vec<_>>().join("")));
    let mut cfg = GatewayConfig::default();
    cfg.providers.push(ProviderConfig {
        id: "mock".into(),
        name: "Mock".into(),
        base_url: base_url.to_string(),
        is_enabled: true,
        relay_id: None,
        keys,
        // Модели уже «обнаружены»: без них провайдер не маршрутизируем, а цель
        // этих тестов — не опрос моделей, а прокси и ротация.
        models: vec![
            DiscoveredModel { id: "m1".into(), ..Default::default() },
            DiscoveredModel { id: "m2".into(), ..Default::default() },
        ],
        ..Default::default()
    });
    std::fs::create_dir_all(&dir).expect("mkdir");
    cloud_routers_core::save_config(&dir, &cfg).expect("save config");
    (AppState::new(&dir).expect("state"), dir)
}

async fn post_chat(state: AppState, body: serde_json::Value) -> Response {
    proxy::chat_completions(
        axum::extract::State(state),
        HeaderMap::new(),
        axum::body::Bytes::from(serde_json::to_vec(&body).expect("serialize")),
    )
    .await
}

async fn body_string(resp: Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    String::from_utf8_lossy(&bytes).to_string()
}

// ──────────────────────────── проксирование ────────────────────────────

#[tokio::test]
async fn json_passthrough_preserves_status_and_body() {
    // Содержимое ответа на русском: байтовый литерал `br#"…"#` допускает только
    // ASCII, поэтому UTF-8 собирается явно. Заодно проверяем, что шлюз не ломает
    // многобайтные ответы — реальные апстримы отдают такой текст постоянно.
    let reply = r#"{"id":"chatcmpl-1","choices":[{"message":{"content":"привет из апстрима"}}]}"#;
    let (base, mock) = start_mock(MockBehavior {
        chat_status: 200,
        chat_content_type: "application/json".into(),
        chat_body: reply.as_bytes().to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);

    let r = post_chat(
        state,
        serde_json::json!({"model":"mock/m1","messages":[{"role":"user","content":"привет"}],"stream":false}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let text = body_string(r).await;
    assert!(text.contains("chatcmpl-1"), "{text}");
    assert!(
        text.contains("привет из апстрима"),
        "многобайтный ответ обязан дойти без искажений: {text}"
    );
    assert_eq!(mock.chat_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn combo_id_is_rewritten_to_upstream_model_id() {
    // Клиент (IDE) присылает "mock/m1"; провайдер знает модель только как "m1".
    let (base, mock) = start_mock(MockBehavior {
        chat_status: 200,
        chat_content_type: "application/json".into(),
        chat_body: b"{}".to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);

    let r = post_chat(state, serde_json::json!({"model":"mock/m1","messages":[]})).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        mock.models(),
        vec!["m1".to_string()],
        "апстрим должен получить настоящий id модели, а не combo_id"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn extra_body_fields_are_forwarded_untouched() {
    // Инструменты, temperature, max_tokens и неизвестные поля должны дойти до
    // апстрима как есть: прокси не «переводит протоколы», он меняет только
    // поле model.
    let (base, mock) = start_mock(MockBehavior {
        chat_status: 200,
        chat_content_type: "application/json".into(),
        chat_body: b"{}".to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);

    let r = post_chat(
        state,
        serde_json::json!({
            "model": "mock/m2",
            "messages": [{"role":"user","content":"x"}],
            "temperature": 0.42,
            "max_tokens": 256,
            "tools": [{"type":"function","function":{"name":"f"}}],
            "tool_choice": "auto",
            "stream_options": {"include_usage": true},
            "some_future_field": {"a": 1}
        }),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(mock.models(), vec!["m2".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn client_secrets_are_not_forwarded_to_upstream() {
    // Локальный токен IDE и cookie браузера не должны уйти провайдеру.
    let (base, mock) = start_mock(MockBehavior {
        chat_status: 200,
        chat_content_type: "application/json".into(),
        chat_body: b"{}".to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);

    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        axum::http::HeaderValue::from_static("Bearer ide-local-token"),
    );
    headers.insert("x-api-key", axum::http::HeaderValue::from_static("ide-secret"));
    headers.insert(axum::http::header::COOKIE, axum::http::HeaderValue::from_static("s=abc"));
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );

    let body = serde_json::to_vec(&serde_json::json!({"model":"mock/m1","messages":[]})).expect("ser");
    let r = proxy::chat_completions(
        axum::extract::State(state),
        headers,
        axum::body::Bytes::from(body),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    let names = mock.header_names();
    let sent = &names[0];
    assert!(!sent.contains(&"cookie".to_string()), "cookie ушёл апстриму: {:?}", sent);
    assert!(!sent.contains(&"x-api-key".to_string()), "x-api-key ушёл апстриму: {:?}", sent);
    assert!(sent.contains(&"authorization".to_string()), "наш Authorization должен уйти");
    assert_eq!(
        mock.auths()[0],
        "Bearer sk-1",
        "токен IDE не должен заменять ключ провайдера"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ──────────────────────────── ротация ключей ────────────────────────────

#[tokio::test]
async fn rate_limit_on_first_key_transparently_retries_with_second() {
    // Главный сценарий лазейного фейловера: 429 на ключе №1, успех на ключе
    // №2. Клиент (IDE) не должен увидеть 429.
    //
    // Ровно ПЕРВЫЙ вызов получает 429, дальше — успех. Детерминированно:
    // round-robin отдаёт k1 первым, значит 429 получает именно k1.
    let (base, mock) = start_mock_with(
        ok_chat_behavior(),
        MockBehavior::error(429, r#"{"error":{"message":"rate limited","type":"rate_limit_error"}}"#),
        1,
    )
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true), key("k2", "sk-2", true)]);

    let r = post_chat(state, serde_json::json!({"model":"mock/m1","messages":[]})).await;
    assert_eq!(r.status(), StatusCode::OK, "клиент не должен видеть 429 провайдера");
    let text = body_string(r).await;
    assert!(text.contains("\"ok\":true"), "{text}");
    assert_eq!(mock.chat_count(), 2, "ожидались две попытки");

    // Ключи действительно разные — ротация отработала, а не повтор того же.
    let auths = mock.auths();
    assert_eq!(auths[0], "Bearer sk-1");
    assert_eq!(auths[1], "Bearer sk-2");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn unauthorized_key_is_deactivated_persisted_and_next_request_skips_it() {
    // 401 — не временная проблема: ключ выключается НА ДИСКЕ, чтобы после
    // перезапуска шлюза он не вернулся в ротацию.
    let (base, mock) = start_mock(MockBehavior {
        chat_status: 401,
        chat_content_type: "application/json".into(),
        chat_body: br#"{"error":{"message":"invalid api key","type":"authentication_error"}}"#.to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true), key("k2", "sk-2", true)]);

    let r = post_chat(state.clone(), serde_json::json!({"model":"mock/m1","messages":[]})).await;
    // Оба ключа вернут 401 → клиент увидит ошибку апстрима.
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(mock.chat_count(), 2, "оба ключа должны были попробоваться");

    // Оба ключа выключены в конфиге на диске.
    let on_disk = cloud_routers_core::load_config(&dir);
    let keys: Vec<(String, bool)> = on_disk.providers[0]
        .keys
        .iter()
        .map(|k| (k.id.clone(), k.is_active))
        .collect();
    assert_eq!(keys, vec![("k1".to_string(), false), ("k2".to_string(), false)], "{:?}", keys);

    // Следующий запрос не тратит время на мёртвые ключи.
    let calls_before = mock.chat_count();
    let r2 = post_chat(state, serde_json::json!({"model":"mock/m1","messages":[]})).await;
    assert_eq!(r2.status(), StatusCode::SERVICE_UNAVAILABLE, "без живых ключей честный 503");
    assert_eq!(
        mock.chat_count(),
        calls_before,
        "выключенные ключи не должны опрашиваться"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn single_401_key_fails_over_to_healthy_key() {
    let (base, mock) = start_mock_with(
        ok_chat_behavior(),
        MockBehavior::error(401, r#"{"error":{"message":"invalid api key","type":"authentication_error"}}"#),
        1,
    )
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-bad", true), key("k2", "sk-good", true)]);

    let r = post_chat(state, serde_json::json!({"model":"mock/m1","messages":[]})).await;
    assert_eq!(r.status(), StatusCode::OK);
    let auths = mock.auths();
    assert_eq!(auths[0], "Bearer sk-bad");
    assert_eq!(auths[1], "Bearer sk-good");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn client_error_is_not_retried_with_another_key() {
    // 400 — ошибка ЗАПРОСА, а не ключа. Повтор с другим ключом дал бы тот же
    // 400 и только задержал ответ.
    let (base, mock) = start_mock(MockBehavior {
        chat_status: 400,
        chat_content_type: "application/json".into(),
        chat_body: br#"{"error":{"message":"bad request","type":"invalid_request_error"}}"#.to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true), key("k2", "sk-2", true)]);

    let r = post_chat(state, serde_json::json!({"model":"mock/m1","messages":[]})).await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(mock.chat_count(), 1, "400 не должен ретраиться");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn upstream_error_body_is_reported_with_provider_and_key() {
    // Пользователь должен видеть, ЧЕЙ ключ отказал — иначе диагностика
    // превращается в угадывание.
    let (base, _mock) = start_mock(MockBehavior {
        chat_status: 500,
        chat_content_type: "application/json".into(),
        chat_body: br#"{"error":{"message":"upstream exploded","type":"api_error"}}"#.to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);

    let r = post_chat(state, serde_json::json!({"model":"mock/m1","messages":[]})).await;
    assert_eq!(r.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let text = body_string(r).await;
    assert!(text.contains("mock"), "должен быть назван провайдер: {text}");
    assert!(text.contains("k1"), "должен быть назван ключ: {text}");
    assert!(
        text.contains("upstream exploded"),
        "исходный текст провайдера должен сохраниться: {text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn unknown_model_returns_404_without_touching_network() {
    let (base, mock) = start_mock(MockBehavior { models_body: models_body_json(), ..Default::default() })
        .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);

    let r = post_chat(state, serde_json::json!({"model":"mock/не-существует","messages":[]})).await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(mock.chat_count(), 0, "апстрим не должен дёргаться");
    let text = body_string(r).await;
    assert!(text.contains("mock/m1"), "в ошибке полезен список доступных: {text}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ──────────────────────── пользовательские связки ────────────────────────

/// Состояние с двумя провайдерами (a, b — каждый на своём моке) и пользовательской
/// связкой `fast` из участников `a/m1` → `b/m1` (если `second` не задан — только `a/m1`).
#[allow(clippy::too_many_arguments)]
fn state_combo(
    a_base: &str,
    a_keys: Vec<AccountKey>,
    b_base: Option<&str>,
    b_keys: Vec<AccountKey>,
    a_key_ids: Option<Vec<String>>,
    b_key_ids: Option<Vec<String>>,
) -> (AppState, std::path::PathBuf) {
    let dir = tmp_dir("combo");
    let mut cfg = GatewayConfig::default();
    let mut members = vec![ComboMember {
        provider_id: "a".into(),
        model_id: "m1".into(),
        key_ids: a_key_ids,
    }];
    cfg.providers.push(ProviderConfig {
        id: "a".into(),
        name: "A".into(),
        base_url: a_base.to_string(),
        is_enabled: true,
        relay_id: None,
        keys: a_keys,
        models: vec![DiscoveredModel { id: "m1".into(), ..Default::default() }],
        ..Default::default()
    });
    if let Some(b_base) = b_base {
        members.push(ComboMember {
            provider_id: "b".into(),
            model_id: "m1".into(),
            key_ids: b_key_ids,
        });
        cfg.providers.push(ProviderConfig {
            id: "b".into(),
            name: "B".into(),
            base_url: b_base.to_string(),
            is_enabled: true,
            relay_id: None,
            keys: b_keys,
            models: vec![DiscoveredModel { id: "m1".into(), ..Default::default() }],
            ..Default::default()
        });
    }
    cfg.combos.push(ComboConfig { name: "fast".into(), members });
    std::fs::create_dir_all(&dir).expect("mkdir");
    cloud_routers_core::save_config(&dir, &cfg).expect("save config");
    (AppState::new(&dir).expect("state"), dir)
}

#[tokio::test]
async fn user_combo_routes_to_first_member_and_rewrites_model_id() {
    // Счастливый путь: запрос `model=fast` уходит первому участнику, и апстрим
    // получает НАСТОЯЩИЙ id модели (m1), а не имя связки.
    let (base_a, mock_a) = start_mock(ok_chat_behavior()).await;
    let (base_b, mock_b) = start_mock(ok_chat_behavior()).await;
    let (state, dir) = state_combo(
        &base_a,
        vec![key("ka1", "sk-a1", true)],
        Some(&base_b),
        vec![key("kb1", "sk-b1", true)],
        None,
        None,
    );

    let r = post_chat(state, serde_json::json!({"model":"fast","messages":[]})).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(mock_a.models(), vec!["m1".to_string()], "апстрим получает настоящий id");
    assert_eq!(mock_a.auths(), vec!["Bearer sk-a1"]);
    assert_eq!(mock_b.chat_count(), 0, "второй участник не должен опрашиваться при живом первом");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn user_combo_falls_over_to_second_member_when_first_exhausts_keys() {
    // Главный сценарий связки: первый участник отвечает ошибкой апстрима на все
    // свои ключи — запрос уходит второму, клиент видит успех.
    let (base_a, mock_a) = start_mock(MockBehavior::error(
        500,
        r#"{"error":{"message":"upstream exploded","type":"api_error"}}"#,
    ))
    .await;
    let (base_b, mock_b) = start_mock(ok_chat_behavior()).await;
    let (state, dir) = state_combo(
        &base_a,
        vec![key("ka1", "sk-a1", true)],
        Some(&base_b),
        vec![key("kb1", "sk-b1", true)],
        None,
        None,
    );

    let r = post_chat(state, serde_json::json!({"model":"fast","messages":[]})).await;
    assert_eq!(r.status(), StatusCode::OK, "fallback обязан быть прозрачным для клиента");
    let text = body_string(r).await;
    assert!(text.contains("\"ok\":true"), "ответ пришёл со второго участника: {text}");
    assert_eq!(mock_a.chat_count(), 1, "первый участник опрошен один раз (один ключ)");
    assert_eq!(mock_b.chat_count(), 1, "второй участник принял запрос");
    assert_eq!(mock_b.models(), vec!["m1".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn user_combo_key_subset_sends_only_allowed_key() {
    // key_ids=["k2"] → в Authorization уходит именно k2, k1 не тратит попытки.
    let (base_a, mock_a) = start_mock(ok_chat_behavior()).await;
    let (state, dir) = state_combo(
        &base_a,
        vec![key("k1", "sk-1", true), key("k2", "sk-2", true)],
        None,
        vec![],
        Some(vec!["k2".into()]),
        None,
    );

    let r = post_chat(state, serde_json::json!({"model":"fast","messages":[]})).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(mock_a.chat_count(), 1, "один разрешённый ключ — одна попытка");
    assert_eq!(mock_a.auths(), vec!["Bearer sk-2"], "разрешённое подмножество определяет ключ");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn user_combo_all_members_down_returns_last_upstream_error() {
    // Оба участника отказали: клиент получает ЧЕСТНУЮ последнюю ошибку апстрима,
    // а не выдуманный «успех» или пустой 503.
    let (base_a, _mock_a) = start_mock(MockBehavior::error(429, r#"{"error":{"message":"limited"}}"#)).await;
    let (base_b, _mock_b) = start_mock(MockBehavior::error(503, r#"{"error":{"message":"down"}}"#)).await;
    let (state, dir) = state_combo(
        &base_a,
        vec![key("ka1", "sk-a1", true)],
        Some(&base_b),
        vec![key("kb1", "sk-b1", true)],
        None,
        None,
    );

    let r = post_chat(state, serde_json::json!({"model":"fast","messages":[]})).await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    let text = body_string(r).await;
    assert!(text.contains("down"), "ошибка последнего участника доходит до клиента: {text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn user_combo_appears_in_models_lists_and_unknown_404_lists_it() {
    // /v1/models и /api/combos содержат связку; 404 по чужому имени перечисляет
    // её в подсказке (единый построитель list_all_combos).
    let (base_a, _mock) = start_mock(ok_chat_behavior()).await;
    let (state, dir) = state_combo(&base_a, vec![key("ka1", "sk-a1", true)], None, vec![], None, None);

    let app = crate::server::build(state);
    let r = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("response");
    assert_eq!(r.status(), StatusCode::OK);
    let v: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(r.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    let ids: Vec<&str> = v["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|m| m["id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids.first(), Some(&"fast"), "связка первая в списке: {ids:?}");
    assert!(ids.contains(&"a/m1"), "per-provider комбо не потеряны: {ids:?}");

    let r = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/v1/chat/completions")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"model":"nope","messages":[]}"#))
                .expect("req"),
        )
        .await
        .expect("response");
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    let text = body_string(r).await;
    assert!(text.contains("fast"), "404 перечисляет пользовательскую связку: {text}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ────────────────────────────── стриминг SSE ──────────────────────────────

#[tokio::test]
async fn sse_stream_is_passed_through_chunk_by_chunk() {
    // Ключевой тест «zero-copy»: первый чанк должен прийти ДО того, как апстрим
    // закончит отправку. При буферизации в память это невозможно.
    //
    // Содержимое на русском собираем из строкового литерала: байтовый `b"…"`
    // допускает только ASCII, а проверить нужно именно UTF-8 в потоке.
    let sse = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"При\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"вет\"}}]}\n\n",
        "data: [DONE]\n\n"
    )
    .as_bytes()
    .to_vec();
    let (base, _mock) = start_mock(MockBehavior {
        chat_status: 200,
        chat_content_type: "text/event-stream".into(),
        chat_body: sse,
        // Каждый кусок спит 120 мс: при буферизации весь ответ занял бы секунды.
        chat_chunk_delay_ms: 120,
        models_body: models_body_json(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);

    let r = post_chat(state, serde_json::json!({"model":"mock/m1","messages":[],"stream":true})).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        r.headers().get(axum::http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
        Some("text/event-stream"),
        "Content-Type проксируется от апстрима"
    );
    assert!(
        !r.headers().contains_key(axum::http::header::CONTENT_LENGTH),
        "у потока не может быть content-length"
    );
    assert_eq!(r.headers().get("cache-control").and_then(|v| v.to_str().ok()), Some("no-cache"));

    let started = std::time::Instant::now();
    let body = r.into_body();
    let mut stream = body.into_data_stream();
    use futures_util::StreamExt;

    // Первый фрагмент обязан прийти заметно раньше конца всего потока.
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), stream.next())
        .await
        .expect("первый чанк не пришёл")
        .expect("поток закрыт")
        .expect("ошибка чтения");
    let first_delay = started.elapsed();
    assert!(
        !first.is_empty(),
        "первый фрагмент не должен быть пустым — иначе это хвост буфера"
    );

    // Копим БАЙТЫ и декодируем один раз в конце. Так проверяется именно
    // побайтовая передача: шлюз не трогает содержимое, поэтому границы чанков
    // (а мок режет поток по 48 байт и может разрезать символ пополам) не должны
    // влиять на результат. Декодировать каждый фрагмент отдельно означало бы
    // проверить не шлюз, а собственную сборку строк в тесте.
    let mut raw: Vec<u8> = first.to_vec();
    while let Some(chunk) = stream.next().await {
        raw.extend_from_slice(&chunk.expect("chunk"));
    }
    let all = String::from_utf8(raw).expect("поток обязан прийти валидным UTF-8");
    // «При» и «вет» приходят РАЗНЫМИ чанками, поэтому в сыром потоке они не
    // соседние строки — искать их склеенными нельзя: шлюз не склеивает
    // дельты, IDE собирает текст сам.
    assert!(all.contains("При"), "первый чанк должен дойти: {all}");
    assert!(all.contains("вет"), "второй чанк должен дойти: {all}");
    assert!(all.contains("[DONE]"), "маркер завершения обязан дойти: {all}");

    // При буферизации всего ответа первый фрагмент пришёл бы только в конце
    // (~1 с). Верхняя граница — с запасом на медленную машину.
    assert!(
        first_delay < std::time::Duration::from_millis(900),
        "первый чанк пришёл через {:?} — похоже на буферизацию всего ответа",
        first_delay
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn long_stream_survives_without_total_request_timeout() {
    // Регрессия, из-за которой был бы сломан весь модуль инференса: общий
    // `Client::timeout` убивает поток на первой длинной генерации. Здесь ответ
    // идёт дольше, чем разумный «общий» таймаут, и обязан выжить.
    let sse = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n",
        "data: [DONE]\n\n"
    )
    .as_bytes()
    .to_vec();
    let (base, _mock) = start_mock(MockBehavior {
        chat_status: 200,
        chat_content_type: "text/event-stream".into(),
        chat_body: sse,
        // 4 куска по 400 мс = ~1.6 с ответа.
        chat_chunk_delay_ms: 400,
        models_body: models_body_json(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);

    let r = post_chat(state, serde_json::json!({"model":"mock/m1","messages":[],"stream":true})).await;
    assert_eq!(r.status(), StatusCode::OK);

    use futures_util::StreamExt;
    let mut stream = r.into_body().into_data_stream();
    let mut total = 0usize;
    while let Some(chunk) = stream.next().await {
        total += chunk.expect("chunk").len();
    }
    assert!(total > 0, "поток должен был что-то отдать");
    let _ = std::fs::remove_dir_all(&dir);
}

// ────────────────────────────── опрос моделей ──────────────────────────────

#[tokio::test]
async fn fetch_models_parses_upstream_and_reports_count() {
    let (base, _mock) = start_mock(MockBehavior { models_body: models_body_json(), ..Default::default() })
        .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);
    let config = state.config();
    let provider = config.providers[0].clone();

    let models = proxy::fetch_provider_models(&state, &provider, &config)
        .await
        .expect("fetch models");
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "m1");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn fetch_models_surfaces_provider_error_verbatim() {
    let (base, _mock) = start_mock(MockBehavior {
        models_status: 403,
        models_body: r#"{"error":"forbidden"}"#.to_string(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);
    let config = state.config();
    let provider = config.providers[0].clone();

    let err = proxy::fetch_provider_models(&state, &provider, &config)
        .await
        .expect_err("403 должен быть ошибкой");
    assert!(err.contains("403"), "{err}");
    // Молчаливый пустой список означал бы «у провайдера нет моделей».
    assert!(err.contains("Mock"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn ping_provider_reports_latency_and_model_count() {
    let (base, _mock) = start_mock(MockBehavior { models_body: models_body_json(), ..Default::default() })
        .await;
    let (state, dir) = state_for(&base, vec![key("k1", "sk-1", true)]);
    let config = state.config();
    let provider = config.providers[0].clone();

    let r = proxy::ping_provider(&state, &provider, &config).await.expect("ping");
    assert_eq!(r.models_count, 2);
    assert!(!r.via_relay);
    assert!(r.target.starts_with("http://127.0.0.1"), "{}", r.target);
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────── проверка Gemini-ключей ─────────────────────────

/// Нативный путь `:countTokens` обслуживает тот же mock_router (маршрут
/// добавлен в него): сегмент начинается с буквы — это литерал для axum 0.7,
/// а не параметр (параметр 0.7 — целый сегмент с ведущим `:`).

fn gemini_check_plan(base: &str) -> crate::key_check::CheckPlan {
    crate::key_check::CheckPlan {
        kind: crate::key_check::KeyCheckKind::GeminiCountTokens,
        models: vec!["gemini-2.0-flash".into()],
        via_relay: false,
        target_urls: vec![format!("{base}/v1beta/models/gemini-2.0-flash:countTokens")],
    }
}

/// План собирается вручную поверх мок-кода: is_gemini_provider отсёк бы
/// 127.0.0.1 адрес, а задача теста — сеть, заголовки и классификация.
///
/// Отказ «первый ключ — 401, второй — 200» детерминирован fail_first_calls:
/// проверка последовательна, по ключу за раз.
#[tokio::test]
async fn key_check_run_plan_verdicts_each_key_without_losing_any() {
    let ok_behavior = MockBehavior {
        chat_status: 200,
        chat_content_type: "application/json".into(),
        chat_body: br#"{"ok":true}"#.to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    };
    let failure_behavior = MockBehavior {
        chat_status: 401,
        chat_content_type: "application/json".into(),
        chat_body: br#"{"error":{"code":401,"message":"API key not valid."}}"#.to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    };
    let (base, mock) = start_mock_with(ok_behavior, failure_behavior, 1).await;
    let (state, dir) = state_for(
        &base,
        vec![
            key("k1", "AQ.Ab8-first", true),
            key("k2", "AQ.Ab8-second", true),
        ],
    );
    let provider = state.config().provider("mock").expect("provider").clone();

    let summary = crate::key_check::run_plan(&state, &provider, gemini_check_plan(&base))
        .await
        .expect("run plan");

    assert!(!summary.via_relay);
    assert_eq!(summary.models, vec!["gemini-2.0-flash".to_string()]);
    assert_eq!(summary.results[0].model, "gemini-2.0-flash", "модель дублируется в результате");
    assert_eq!(summary.results.len(), 2, "оба ключа обязаны попасть в свод: никакой ключ не теряется");
    assert_eq!(summary.results[0].key_id, "k1");
    assert_eq!(summary.results[0].verdict, crate::key_check::CheckVerdict::Invalid, "первый вызов — 401");
    assert_eq!(
        summary.results[1].verdict,
        crate::key_check::CheckVerdict::Valid,
        "второй вызов — 200"
    );
    let first = &summary.results[0];
    assert!(
        !first.masked.contains("AQ.Ab8-first"),
        "секрет целиком наружу не уходит: {}",
        first.masked
    );

    // Проверка — диагностика, а не ротация: is_active не изменился, ретрай
    // ключа не случился (только по одному запросу на ключ).
    assert_eq!(mock.chat_count(), 2, "запросов ровно по одному на ключ");
    let after = state.config();
    let after_provider = after.provider("mock").expect("provider after");
    assert!(
        after_provider.keys.iter().all(|k| k.is_active),
        "run_plan не должен деактивировать ключи: это делает только чат"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Ключ идёт как `x-goog-api-key`, а НЕ как Authorization: секрет не должен
/// уезжать в заголовок чужого стандарта (тот же контракт, что у юзерского
/// рецепта с PowerShell).
#[tokio::test]
async fn key_check_secret_travels_only_in_x_goog_api_key() {
    let (base, mock) = start_mock(MockBehavior {
        chat_status: 200,
        chat_content_type: "application/json".into(),
        chat_body: br#"{"totalTokens":1}"#.to_vec(),
        models_body: models_body_json(),
        ..Default::default()
    })
    .await;
    let (state, dir) = state_for(&base, vec![key("k1", "AQ.Ab8-secret", true)]);
    let provider = state.config().provider("mock").expect("provider").clone();

    crate::key_check::run_plan(&state, &provider, gemini_check_plan(&base))
        .await
        .expect("run plan");

    let headers = mock.header_names().remove(0);
    assert!(
        headers.iter().any(|h| h == "x-goog-api-key"),
        "x-goog-api-key обязателен: {:?}",
        headers
    );
    assert!(!headers.iter().any(|h| h == "authorization"), "Authorization запрещён: {:?}", headers);
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────── проверка ключей OpenRouter ─────────────────────────

/// Записи эхо-релея: что дошло (метод, цель, токен) — на каждый запрос.
#[derive(Default)]
struct RelayEcho {
    /// (метод, x-target-url, x-relay-token) каждого запроса.
    hits: Mutex<Vec<(String, String, String)>>,
}

impl RelayEcho {
    fn hits(&self) -> Vec<(String, String, String)> {
        self.hits.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

/// Эхо-релей: не ходит к цели (сеть до апстрима — не предмет теста), а
/// ЗАПИСЫВАЕТ заголовки релей-протокола и отвечает телом ключ-инфо. Предмет
/// проверки — что run_plan ДОСТАВИЛ x-target-url и x-relay-token слота.
async fn relay_echo(
    State(s): State<Arc<RelayEcho>>,
    method: axum::http::Method,
    headers: HeaderMap,
) -> Response {
    let get = |name: &str| {
        headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
    };
    s.hits
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push((
            method.as_str().to_ascii_uppercase(),
            get(crate::upstream::HDR_TARGET_URL),
            get(crate::upstream::HDR_RELAY_TOKEN),
        ));
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        r#"{"data":{"label":"my-key","is_free_tier":true,"usage":0.42,"usage_daily":0.03,"limit":10.0,"limit_remaining":9.97,"limit_reset":"daily"}}"#,
    )
        .into_response()
}

/// Поднять эхо-релей на свободном порту, вернуть базовый URL.
async fn relay_echo_serve(echo: Arc<RelayEcho>) -> String {
    let router = Router::<Arc<RelayEcho>>::new().fallback(relay_echo).with_state(echo.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind relay");
    let addr: SocketAddr = listener.local_addr().expect("addr");
    tokio::spawn(async move { let _ = axum::serve(listener, router).await; });
    format!("http://127.0.0.1:{}", addr.port())
}

/// Проверка ключей OpenRouter через релей: GET /key едет к эхо-релею с
/// абсолютной целью и токеном слота, ответ спарсивается, кап free-тира — 50.
/// Прямого запроса к openrouter.ai из теста быть не может (127.0.0.1-эхо) —
/// любой уход «напрямую» не дал бы тесту пройти.
#[tokio::test]
async fn openrouter_key_check_travels_through_relay_with_target_headers() {
    let echo = Arc::new(RelayEcho::default());
    let relay_base = relay_echo_serve(echo.clone()).await;

    let (state, dir) = state_for("https://openrouter.ai/api/v1", vec![key("k1", "sk-or-1", true)]);
    state
        .update_config(|c| {
            c.relays.push(cloud_routers_core::config::RelaySlot {
                id: "vercel".into(),
                kind: "vercel".into(),
                url: Some(relay_base),
                relay_secret: Some("s3cret".into()),
                ..Default::default()
            });
            if let Some(p) = c.provider_mut("mock") {
                p.relay_id = Some("vercel".into());
            }
            Ok(())
        })
        .expect("seed relay");

    let provider = state.config().provider("mock").expect("provider").clone();
    let config = state.config();
    let plan = crate::openrouter_check::build_openrouter_plan(&provider, &config).expect("plan");
    assert!(plan.via_relay, "план обязан идти через релей");

    let summary = crate::key_check::run_plan(&state, &provider, plan).await.expect("run plan");

    // Релей-протокол доставлен: метод GET, абсолютная цель {base}/key, токен слота.
    let hits = echo.hits();
    assert_eq!(hits.len(), 1, "один ключ — один запрос: {:?}", hits);
    assert_eq!(hits[0].0, "GET", "OpenRouter-проверка — GET, не POST");
    assert_eq!(hits[0].1, "https://openrouter.ai/api/v1/key", "цель абсолютная");
    assert_eq!(hits[0].2, "s3cret", "токен — секрет слота провайдера");

    // Ответ спарсился в структурные поля, вердикт валиден.
    let r0 = &summary.results[0];
    assert_eq!(r0.verdict, crate::key_check::CheckVerdict::Valid);
    assert_eq!(r0.model, "key", "маркер ключевой цели");
    let info = r0.openrouter.as_ref().expect("ключ-инфо парсится");
    assert_eq!(info.is_free_tier, Some(true));
    assert_eq!(info.usage_daily, Some(0.03));
    assert_eq!(info.limit, Some(10.0));
    assert_eq!(info.limit_remaining, Some(9.97));
    assert_eq!(info.limit_reset.as_deref(), Some("daily"));
    assert!(r0.detail.is_empty(), "JSON-тело валидного ответа в detail не нужно: {}", r0.detail);

    // Сводка счётчика: чат-запросов не было — счёт 0; кап от free-тира — 50.
    let fu = summary.free_usage.as_ref().expect("free_usage только для OpenRouter");
    assert_eq!(fu.provider_id, "mock");
    assert_eq!(fu.count, 0);
    assert_eq!(fu.cap, Some(crate::openrouter_check::FREE_DAILY_CAP_FREE_KEY));
    assert_eq!(fu.ymd, crate::free_usage::today_ymd());
    assert!(summary.via_relay);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Релей без секрета — отказ ДО сети с читаемой причиной (не прямой запрос).
#[tokio::test]
async fn openrouter_key_check_rejects_relay_without_secret() {
    let echo = Arc::new(RelayEcho::default());
    let relay_base = relay_echo_serve(echo.clone()).await;

    let (state, dir) = state_for("https://openrouter.ai/api/v1", vec![key("k1", "sk-or-1", true)]);
    state
        .update_config(|c| {
            c.relays.push(cloud_routers_core::config::RelaySlot {
                id: "cf".into(),
                kind: "cloudflare".into(),
                url: Some(relay_base),
                relay_secret: None,
                ..Default::default()
            });
            if let Some(p) = c.provider_mut("mock") {
                p.relay_id = Some("cf".into());
            }
            Ok(())
        })
        .expect("seed relay");

    let provider = state.config().provider("mock").expect("provider").clone();
    let config = state.config();
    let err = crate::openrouter_check::build_openrouter_plan(&provider, &config)
        .expect_err("секрет релея обязателен");
    assert!(err.contains("секрет"), "{err}");

    // До сети дело не дошло: эхо-релей не получил ни одного запроса.
    assert!(echo.hits().is_empty(), "прямой запрос молча не шёл: {:?}", echo.hits());
    let _ = std::fs::remove_dir_all(&dir);
}

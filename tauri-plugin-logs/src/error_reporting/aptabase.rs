//! HTTP-контракт Aptabase: события (`/api/v0/events`) и ошибки (`/api/v0/error`).
//! Скопирован с tauri-plugin-aptabase 1.0.0 (формат тел и заголовков).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};

use super::{ErrorReport, ReportingConfig, APP_VERSION, CONFIG};

static QUEUE: OnceLock<Mutex<VecDeque<Value>>> = OnceLock::new();
static SESSION: OnceLock<Mutex<SessionState>> = OnceLock::new();
static HTTP_SHARE: OnceLock<Option<reqwest::Client>> = OnceLock::new();
static FLUSHER_STARTED: AtomicBool = AtomicBool::new(false);
static FLUSHING: AtomicBool = AtomicBool::new(false);

const SESSION_TIMEOUT_SECS: i64 = 4 * 60 * 60;
const MAX_BATCH: usize = 25;
const FLUSH_INTERVAL: Duration = Duration::from_secs(10);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// ЕДИНЫЙ HTTP-клиент Aptabase (лениво, с тем же таймаутом).
///
/// Раньше `try_flush` и `send_error` строили новый `reqwest::Client` на каждый
/// вызов; при периодическом флаше (FLUSH_INTERVAL = 10 с) каждый POST открывал
/// НОВУЮ TCP/TLS-коннекцию, и reqwest/hyper логировал это на DEBUG
/// («starting new connection: https://eu.aptabase.com/») — строки сыпались в
/// файл-лог каждые ~10–20 с. Общий клиент переиспользует keep-alive пул:
/// коннект открывается только после простоя пула, а не на каждый флаш.
/// Единичные остаточные коннект-строки дожимает фильтр таргетов в logger.rs
/// (hyper и co. на WARN+ — их диагностика сбоев сети не теряется).
fn http_client() -> Option<&'static reqwest::Client> {
    HTTP_SHARE
        .get_or_init(|| reqwest::Client::builder().timeout(HTTP_TIMEOUT).build().ok())
        .as_ref()
}

#[derive(Default)]
struct SessionState {
    id: String,
    last_touch_secs: i64,
    seed: u64,
}

fn queue() -> &'static Mutex<VecDeque<Value>> {
    QUEUE.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn lock<T>(guard: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    guard.lock().unwrap_or_else(|p| p.into_inner())
}

fn session() -> &'static Mutex<SessionState> {
    SESSION.get_or_init(|| Mutex::new(SessionState::default()))
}

fn session_id() -> String {
    let now = Utc::now().timestamp();
    let mut s = lock(session());
    if s.id.is_empty() || now - s.last_touch_secs > SESSION_TIMEOUT_SECS {
        s.seed = s.seed.wrapping_add(1);
        s.id = format!("{}{:06}", now, s.seed.wrapping_mul(2654435761) % 1_000_000);
        s.last_touch_secs = now;
    } else {
        s.last_touch_secs = now;
    }
    s.id.clone()
}

/// Базовый URL региона по App-Key (`A-<REGION>-<SECRET>`).
pub fn region_base(app_key: &str) -> &'static str {
    match app_key.split('-').nth(1) {
        Some("US") => "https://us.aptabase.com",
        Some("DEV") => "http://localhost:3000",
        _ => "https://eu.aptabase.com",
    }
}

fn system_props() -> Value {
    let app_version = APP_VERSION.get().cloned().unwrap_or_default();
    json!({
        "isDebug": cfg!(debug_assertions),
        "osName": std::env::consts::OS,
        "osVersion": std::env::var("OS").unwrap_or_else(|_| std::env::consts::OS.to_string()),
        "locale": std::env::var("LANG").unwrap_or_else(|_| "ru-RU".to_string()),
        "engineName": "tauri",
        "engineVersion": tauri::VERSION,
        "appVersion": app_version,
        "sdkVersion": concat!("tauri-plugin-logs@", env!("CARGO_PKG_VERSION"))
    })
}

fn build_event(name: &str, props: Option<Value>) -> Value {
    json!({
        "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "sessionId": session_id(),
        "eventName": name,
        "systemProps": system_props(),
        "props": props
    })
}

/// Старт периодического флашера (идемпотентно).
pub fn start() {
    if FLUSHER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async {
        loop {
            tokio::time::sleep(FLUSH_INTERVAL).await;
            flush_periodic().await;
        }
    });
}

/// Ставит событие в очередь и сразу пробует отправить.
pub fn enqueue_event(name: &str, props: Option<Value>) {
    let ev = build_event(name, props);
    lock(queue()).push_back(ev);
    kick();
}

/// Если ничего ещё не летит — запускаем flush одним бэтчем.
fn kick() {
    if FLUSHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let fut = async {
        try_flush().await;
        FLUSHING.store(false, Ordering::SeqCst);
    };
    tauri::async_runtime::spawn(fut);
}

/// Периодический flush: досылает всё накопленное (используется флашером и при выходе).
pub async fn flush_periodic() {
    if FLUSHING.swap(true, Ordering::SeqCst) {
        return;
    }
    try_flush().await;
    FLUSHING.store(false, Ordering::SeqCst);
}

/// Сколько неудачных попыток отправки подряд переживаем, прежде чем отбросить
/// очередь. Инцидент 25.10.2026: при недоступности aptabase (TCP до
/// eu.aptabase.com:443 молча дропался) батч вечно возвращался в очередь —
/// флашер каждые ~20 с (сон 10 с + таймаут коннекта 10 с) строил новую
/// TCP-коннекцию, и «starting new connection» сыпались в файл-лог часами.
/// Кап делает такой вечный долбёж структурно невозможным, а сам отказ —
/// видимым в логе (core rules §2.2: тихое глотание ошибок запрещено).
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Неудачные попытки отправки подряд; успех сбрасывает в 0.
static SEND_FAILURES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

async fn try_flush() {
    let Some(cfg) = CONFIG.get() else { return };
    let Some(cfg) = cfg.as_ref() else { return };
    if cfg.app_key.is_empty() {
        return;
    }

    // Клиент строится лениво и переиспользуется (см. http_client).
    let Some(client) = http_client() else { return };
    let url = format!("{}/api/v0/events", region_base(&cfg.app_key));
    try_flush_with(client, &url, &cfg.app_key).await;
}

/// Тело флаша, отделённое от конфига ради тестов: тест гоняет его против
/// локального «отказного» порта, не трогая CONFIG.
async fn try_flush_with(client: &reqwest::Client, url: &str, app_key: &str) {
    loop {
        let batch: Vec<Value> = {
            let mut q = lock(queue());
            let n = q.len().min(MAX_BATCH);
            if n == 0 {
                break;
            }
            q.drain(..n).collect()
        };

        let body = json!(batch);
        match client
            .post(url)
            .header("App-Key", app_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                SEND_FAILURES.store(0, Ordering::SeqCst);
            }
            // 5xx и сеть — один класс отказа: «сейчас не ушло, виноват не батч».
            // Каждая попытка видна в логе (требование 25.10.2026: сетевая ошибка
            // обязана быть в логе), после капа очередь отбрасывается.
            Ok(resp) if resp.status().is_server_error() => {
                let status = resp.status().as_u16();
                requeue_front(batch);
                register_send_failure(format!("сервер aptabase ответил HTTP {status}")).await;
                break;
            }
            Ok(resp) => {
                // 4xx — батч невалиден для сервера: ретрай того же тела бессмыслен.
                let status = resp.status().as_u16();
                log::warn!(
                    "телеметрия: батч из {} событий отклонён (HTTP {status}) — отброшен",
                    batch.len()
                );
                SEND_FAILURES.store(0, Ordering::SeqCst);
            }
            Err(e) => {
                requeue_front(batch);
                register_send_failure(format!("сеть: {e}")).await;
                break;
            }
        }
    }
}

/// Учёт одной неудачной попытки: warn с причиной, а по достижении капа —
/// отброс очереди с итоговой строкой (сколько событий потеряно и почему).
async fn register_send_failure(reason: String) {
    let n = SEND_FAILURES.fetch_add(1, Ordering::SeqCst) + 1;
    log::warn!(
        "телеметрия: события не отправлены (попытка {n}/{MAX_CONSECUTIVE_FAILURES}): {reason}"
    );
    if n < MAX_CONSECUTIVE_FAILURES {
        return;
    }
    let dropped = {
        let mut q = lock(queue());
        let n = q.len();
        q.clear();
        n
    };
    log::warn!(
        "телеметрия: {dropped} событий отброшено — отправка не удалась {n} раз подряд ({reason})"
    );
    SEND_FAILURES.store(0, Ordering::SeqCst);
}

fn requeue_front(items: Vec<Value>) {
    let mut q = lock(queue());
    for item in items.into_iter().rev() {
        q.push_front(item);
    }
}

/// Усечение строки до лимита сервера. Сервер (`ErrorBody.cs` → `IsValid`)
/// отвергает ВЕСЬ отчёт (400) на превышающем лимит поле, поэтому режем на
/// границе `chars()` — частичная доставка лучше нулевой.
fn cap(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Тело ошибки для `/api/v0/error`. Чистая функция — тестовая точка.
///
/// Сервер принимает ровно поля `ErrorBody.cs`; неизвестные молча отбрасывает
/// (`project`, `breadcrumbs` — не доходят, см. docs/README.md), а превышение
/// любого лимита (errorMessage ≤5000, errorType ≤100, stackTrace ≤10000,
/// platform ≤30, osName ≤30, osVersion ≤100, appVersion ≤50, sdkVersion ≤40,
/// sessionId ≤100) → 400 на весь отчёт — поэтому все строки усекаются.
fn error_body(report: &ErrorReport, cfg: &ReportingConfig, app_version: &str, session_id: &str) -> Value {
    json!({
        "errorMessage": cap(&report.message, 5000),
        "errorType": cap(&report.error_type, 100),
        "stackTrace": report.stack.as_deref().map(|s| cap(s, 10_000)),
        "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "inBackground": false,
        "severity": report.severity,
        "kind": report.kind,
        "isDebug": cfg!(debug_assertions),
        "platform": cap("tauri", 30),
        "osName": cap(std::env::consts::OS, 30),
        "osVersion": cap(&std::env::var("OS").unwrap_or_else(|_| std::env::consts::OS.to_string()), 100),
        "appVersion": cap(app_version, 50),
        "sdkVersion": cap(concat!("tauri-plugin-logs@", env!("CARGO_PKG_VERSION")), 40),
        "sessionId": cap(session_id, 100),
        "project": cfg.project,
        "breadcrumbs": report.breadcrumbs,
    })
}

/// Отправка ошибки (`/api/v0/error`, + поле `project`).
pub fn send_error(cfg: ReportingConfig, report: ErrorReport) {
    let app_version = APP_VERSION.get().cloned().unwrap_or_default();
    let body = error_body(&report, &cfg, &app_version, &session_id());
    let url = format!("{}/api/v0/error", region_base(&cfg.app_key));
    let app_key = cfg.app_key;
    tauri::async_runtime::spawn(async move {
        // Единый ленивый клиент (см. http_client) — без новой коннекции на каждую ошибку.
        let Some(client) = http_client() else { return };
        let result = client
            .post(&url)
            .header("App-Key", &app_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;
        match result {
            Ok(resp) if resp.status().is_success() => {}
            Ok(resp) => log::warn!(
                "телеметрия: отчёт об ошибке не принят (HTTP {})",
                resp.status()
            ),
            Err(e) => log::warn!("телеметрия: отчёт об ошибке не отправлен: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_report() -> ErrorReport {
        ErrorReport {
            error_type: "SyntheticType".to_string(),
            message: "synthetic error message".to_string(),
            stack: None,
            severity: "error",
            kind: "handled",
            breadcrumbs: Vec::new(),
        }
    }

    fn synthetic_cfg() -> ReportingConfig {
        ReportingConfig {
            enabled: true,
            project: "synthetic-project".to_string(),
            app_key: "A-EU-test".to_string(),
            events: true,
        }
    }

    /// Обогащённое тело содержит контекст, которого раньше не было
    /// (GAP: дашборд не мог фильтровать по версии/ОС и связать с сессией).
    #[test]
    fn error_body_contains_context_fields() {
        let body = error_body(&synthetic_report(), &synthetic_cfg(), "26.10.14", "sess-123");
        assert_eq!(body["appVersion"], "26.10.14");
        assert_eq!(body["sessionId"], "sess-123");
        assert_eq!(body["osName"], std::env::consts::OS);
        assert_eq!(body["platform"], "tauri");
        assert_eq!(body["severity"], "error");
        assert_eq!(body["kind"], "handled");
        assert_eq!(body["project"], "synthetic-project");
        let sdk = body["sdkVersion"].as_str().expect("sdkVersion строка");
        assert!(sdk.starts_with("tauri-plugin-logs@"), "неожиданный sdkVersion: {sdk}");
        assert!(sdk.chars().count() <= 40, "sdkVersion длиннее лимита ErrorBody");
        let os_version = body["osVersion"].as_str().expect("osVersion строка");
        assert!(os_version.chars().count() <= 100);
    }

    /// sessionId берётся из той же сессии, что и события (единая сессия).
    #[test]
    fn error_body_session_matches_event_session() {
        let sid = session_id();
        let body = error_body(&synthetic_report(), &synthetic_cfg(), "26.10.14", &sid);
        assert_eq!(body["sessionId"], sid);
        assert!(!sid.is_empty());
    }

    /// Длинные поля усекаются до лимитов сервера — иначе весь отчёт
    /// отвергается с 400 (ErrorBody.cs IsValid), а не только это поле.
    #[test]
    fn error_body_truncates_to_server_limits() {
        let mut report = synthetic_report();
        report.message = "m".repeat(6000);
        report.error_type = "t".repeat(150);
        report.stack = Some("s".repeat(11_000));
        let body = error_body(&report, &synthetic_cfg(), &"v".repeat(60), &"x".repeat(120));
        assert_eq!(body["errorMessage"].as_str().unwrap().chars().count(), 5000);
        assert_eq!(body["errorType"].as_str().unwrap().chars().count(), 100);
        assert_eq!(body["stackTrace"].as_str().unwrap().chars().count(), 10_000);
        assert_eq!(body["appVersion"].as_str().unwrap().chars().count(), 50);
        assert_eq!(body["sessionId"].as_str().unwrap().chars().count(), 100);
    }

    #[test]
    fn region_selected_from_app_key() {
        assert_eq!(region_base("A-EU-123"), "https://eu.aptabase.com");
        assert_eq!(region_base("A-US-123"), "https://us.aptabase.com");
        assert_eq!(region_base("A-DEV-123"), "http://localhost:3000");
        assert_eq!(region_base("garbage"), "https://eu.aptabase.com");
    }

    #[test]
    fn event_has_required_fields() {
        let ev = build_event("app_started", None);
        assert_eq!(ev["eventName"], "app_started");
        assert!(ev["sessionId"].as_str().is_some());
        assert!(ev["timestamp"].as_str().is_some());
        assert!(ev["systemProps"]["appVersion"].is_string());
        assert!(ev["props"].is_null());
    }

    /// Оба места отправки пользуются ОДНИМ клиентом (корень спама
    /// «starting new connection» — новый клиент на каждый флаш).
    #[test]
    fn http_client_is_shared_across_calls() {
        let first = http_client();
        let second = http_client();
        // Клиент обязан построиться (builder не падает на типичной машине)…
        assert!(first.is_some(), "клиент не построился");
        assert!(second.is_some(), "клиент не построился");
        // …и это ровно та же статическая ссылка, без перестроения.
        assert!(std::ptr::eq(first.unwrap(), second.unwrap()));
    }

    /// Очередь — глобальная статика, а cargo гоняет тесты параллельно:
    /// `clear()` одного теста съедал батч другого. Сериализуем тесты,
    /// трогающие очередь, одним мьютексом.
    fn queue_lock() -> std::sync::MutexGuard<'static, ()> {
        static TEST_QUEUE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        lock(&TEST_QUEUE_LOCK)
    }

    /// Локальный сервер постоянных отказов (HTTP 599): детерминированная
    /// ветка server-error с тем же requeue+кап, что и сетевой Err. Слушатель
    /// живёт до конца теста — порт нельзя «украсть» параллельному тесту
    /// (первая версия с dropped-слушателем гонялась за ephemeral-портом).
    fn failure_server() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                use std::io::Read;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = std::io::Write::write_all(
                    &mut stream,
                    b"HTTP/1.1 599 Upstream Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            }
        });
        port
    }

    /// Инцидент 25.10.2026: недоставляемый батч вечно возвращался в очередь,
    /// и флашер долбился в aptabase каждые ~20 с часами. После капа из трёх
    /// неудач подряд очередь обязана очиститься, а счётчик — обнулиться:
    /// новых попыток (и коннект-строк в логе) больше не будет.
    #[test]
    fn queue_is_dropped_after_three_consecutive_failures() {
        let _guard = queue_lock();
        let port = failure_server();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(1500))
            .build()
            .expect("client");
        let url = format!("http://127.0.0.1:{port}/api/v0/events");

        lock(queue()).clear();
        SEND_FAILURES.store(0, Ordering::SeqCst);
        lock(queue()).push_back(build_event("test_event", None));

        // Попытки 1 и 2: батч возвращается в очередь, счётчик растёт.
        tauri::async_runtime::block_on(try_flush_with(&client, &url, "A-EU-test"));
        assert_eq!(lock(queue()).len(), 1, "батч обязан вернуться в очередь");
        assert_eq!(SEND_FAILURES.load(Ordering::SeqCst), 1);

        tauri::async_runtime::block_on(try_flush_with(&client, &url, "A-EU-test"));
        assert_eq!(lock(queue()).len(), 1);
        assert_eq!(SEND_FAILURES.load(Ordering::SeqCst), 2);

        // Попытка 3: кап достигнут — очередь очищена, счётчик сброшен.
        tauri::async_runtime::block_on(try_flush_with(&client, &url, "A-EU-test"));
        assert!(lock(queue()).is_empty(), "после капа очередь обязана очиститься");
        assert_eq!(SEND_FAILURES.load(Ordering::SeqCst), 0, "счётчик обязан сброситься");
    }

    /// Пустая очередь — ни одной попытки отправки, счётчик не растёт.
    #[test]
    fn empty_queue_makes_no_attempts() {
        let _guard = queue_lock();
        let port = failure_server();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(1500))
            .build()
            .expect("client");
        let url = format!("http://127.0.0.1:{port}/api/v0/events");

        lock(queue()).clear();
        SEND_FAILURES.store(0, Ordering::SeqCst);
        tauri::async_runtime::block_on(try_flush_with(&client, &url, "A-EU-test"));
        assert_eq!(SEND_FAILURES.load(Ordering::SeqCst), 0, "попыток быть не должно");
    }

    /// Успешный ответ сервера сбрасывает счётчик неудач.
    #[test]
    fn success_resets_failure_counter() {
        let _guard = queue_lock();
        // Локальный приёмник: отвечаем 200 на всё.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                // Читаем заголовки до конца (пустая строка), отвечаем 200.
                use std::io::Read;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = std::io::Write::write_all(
                    &mut stream,
                    b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            }
        });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(1500))
            .build()
            .expect("client");
        let url = format!("http://127.0.0.1:{port}/api/v0/events");

        lock(queue()).clear();
        SEND_FAILURES.store(2, Ordering::SeqCst);
        lock(queue()).push_back(build_event("test_event", None));
        tauri::async_runtime::block_on(try_flush_with(&client, &url, "A-EU-test"));
        assert!(lock(queue()).is_empty(), "батч ушёл — очередь пуста");
        assert_eq!(SEND_FAILURES.load(Ordering::SeqCst), 0, "успех обязан сбросить счётчик");
    }
}
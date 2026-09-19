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
static FLUSHER_STARTED: AtomicBool = AtomicBool::new(false);
static FLUSHING: AtomicBool = AtomicBool::new(false);

const SESSION_TIMEOUT_SECS: i64 = 4 * 60 * 60;
const MAX_BATCH: usize = 25;
const FLUSH_INTERVAL: Duration = Duration::from_secs(10);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

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

async fn try_flush() {
    let Some(cfg) = CONFIG.get() else { return };
    let Some(cfg) = cfg.as_ref() else { return };
    if cfg.app_key.is_empty() {
        return;
    }

    let client = match reqwest::Client::builder().timeout(HTTP_TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return,
    };
    let url = format!("{}/api/v0/events", region_base(&cfg.app_key));

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
            .post(&url)
            .header("App-Key", &cfg.app_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {}
            Ok(resp) if resp.status().is_server_error() => {
                requeue_front(batch);
                break;
            }
            Ok(_) => {
                // 4xx — отбрасываем, чтобы не зациклиться
            }
            Err(_) => {
                requeue_front(batch);
                break;
            }
        }
    }
}

fn requeue_front(items: Vec<Value>) {
    let mut q = lock(queue());
    for item in items.into_iter().rev() {
        q.push_front(item);
    }
}

/// Отправка ошибки (`/api/v0/error`, + поле `project`).
pub fn send_error(cfg: ReportingConfig, report: ErrorReport) {
    let url = format!("{}/api/v0/error", region_base(&cfg.app_key));
    let app_key = cfg.app_key;
    let body = json!({
        "errorMessage": report.message,
        "errorType": report.error_type,
        "stackTrace": report.stack,
        "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "inBackground": false,
        "severity": report.severity,
        "kind": report.kind,
        "isDebug": cfg!(debug_assertions),
        "project": cfg.project,
        "breadcrumbs": report.breadcrumbs,
    });
    tauri::async_runtime::spawn(async move {
        let client = match reqwest::Client::builder().timeout(HTTP_TIMEOUT).build() {
            Ok(c) => c,
            Err(_) => return,
        };
        let _ = client
            .post(&url)
            .header("App-Key", &app_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
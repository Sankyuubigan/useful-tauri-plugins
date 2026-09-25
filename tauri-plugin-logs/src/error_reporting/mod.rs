//! Опциональная отправка ошибок и аналитики в облако (Aptabase).
//!
//! Реализует контракт tauri-plugin-aptabase: события — `POST /api/v0/events`
//! (массив, заголовок `App-Key`), ошибки — `POST /api/v0/error` (+ поле
//! `project` для различения приложений на одном App-Key).
//!
//! Отправка включается блоком `reporting` в конфиге плагина и может
//! переключаться в рантайме через `set_reporting_enabled` (настройка
//! `allow_error_reports` хоста — единственный источник правды).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;

mod aptabase;

/// Опциональный блок конфига `plugins.logs.reporting` (`tauri.conf.json`).
#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct ReportingConfig {
    pub enabled: bool,
    pub project: String,
    pub app_key: String,
    pub events: bool,
}

impl Default for ReportingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            project: String::new(),
            app_key: String::new(),
            events: true,
        }
    }
}

pub(crate) static CONFIG: OnceLock<Option<ReportingConfig>> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
static APP_VERSION: OnceLock<String> = OnceLock::new();

/// Описатель ошибки для отправки в облако.
pub struct ErrorReport {
    pub error_type: String,
    pub message: String,
    pub stack: Option<String>,
    pub severity: &'static str,
    pub kind: &'static str,
    pub breadcrumbs: Vec<String>,
}

/// Ошибка, пришедшая от фронта через команду `track_error`.
#[derive(Deserialize)]
pub struct JsErrorReport {
    #[serde(rename = "errorType")]
    pub error_type: String,
    pub message: String,
    pub stack: Option<String>,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub kind: String,
    /// Хроника действий перед ошибкой (буфер breadcrumbs фронта).
    #[serde(default)]
    pub breadcrumbs: Option<Vec<String>>,
}

pub fn init(cfg: Option<ReportingConfig>, app_version: String) {
    let _ = APP_VERSION.set(app_version);
    let enabled = cfg.as_ref().map(|c| c.enabled).unwrap_or(false);
    if CONFIG.set(cfg).is_err() {
        // уже инициализировано (повторный setup в тестах) — сохраняем первый конфиг
    }
    ENABLED.store(enabled, Ordering::SeqCst);
    if enabled {
        aptabase::start();
    }
}

pub fn set_enabled(v: bool) {
    ENABLED.store(v, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

fn config() -> Option<ReportingConfig> {
    CONFIG.get()?.clone()
}

/// Ошибка уровня ERROR из бэкенд-кода (через `log::error!`) — kind `handled`.
pub fn report_handled(error_type: &str, message: &str) {
    if !is_enabled() {
        return;
    }
    let Some(cfg) = config() else { return };
    if cfg.app_key.is_empty() {
        return;
    }
    aptabase::send_error(
        cfg,
        ErrorReport {
            error_type: error_type.to_string(),
            message: message.to_string(),
            stack: None,
            severity: "error",
            kind: "handled",
            breadcrumbs: Vec::new(),
        },
    );
}

/// Паника — kind `crash`, severity `fatal`.
pub fn report_fatal(error_type: &str, message: &str, stack: &str) {
    if !is_enabled() {
        return;
    }
    let Some(cfg) = config() else { return };
    if cfg.app_key.is_empty() {
        return;
    }
    aptabase::send_error(
        cfg,
        ErrorReport {
            error_type: error_type.to_string(),
            message: message.to_string(),
            stack: if stack.is_empty() { None } else { Some(stack.to_string()) },
            severity: "fatal",
            kind: "crash",
            breadcrumbs: Vec::new(),
        },
    );
}

/// Ошибка, пришедшая с фронта через `track_error`.
pub fn report_js(report: JsErrorReport) {
    if !is_enabled() {
        return;
    }
    let Some(cfg) = config() else { return };
    if cfg.app_key.is_empty() {
        return;
    }
    aptabase::send_error(
        cfg,
        ErrorReport {
            error_type: report.error_type,
            message: report.message,
            stack: report.stack,
            severity: if report.severity == "fatal" { "fatal" } else { "error" },
            kind: match report.kind.as_str() {
                "crash" => "crash",
                "unhandled" => "unhandled",
                "taskException" => "taskException",
                _ => "handled",
            },
            breadcrumbs: report.breadcrumbs.unwrap_or_default(),
        },
    );
}

/// Аналитическое событие (batch в `/api/v0/events`).
pub fn track_event(name: &str, props: Option<Value>) {
    if !is_enabled() {
        return;
    }
    let Some(cfg) = config() else { return };
    if cfg.app_key.is_empty() || !cfg.events {
        return;
    }
    aptabase::enqueue_event(name, props);
}

pub fn flush_on_exit() {
    let _ = std::thread::Builder::new()
        .name("tauri-logs-flush".to_string())
        .spawn(|| {
            tauri::async_runtime::block_on(aptabase::flush_periodic());
        });
}

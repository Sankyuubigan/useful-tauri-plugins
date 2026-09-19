//! 🪵 tauri-plugin-logs — переиспользуемая вкладка «Логи» + единый `log::Log` +
//! опциональная отправка ошибок/аналитики в облако (Aptabase).
//!
//! ## Подключение в хосте
//!
//! `Cargo.toml`:
//! ```toml
//! tauri-plugin-logs = { path = "../../my-tauri-plugins/tauri-plugin-logs" }
//! ```
//!
//! `main.rs`:
//! ```ignore
//! fn main() {
//!     tauri_plugin_logs::early_init("king_orch.log"); // краш-лог с 1-й миллисекунды
//!     tauri_plugin_logs::early_log("INFO", "запуск…");
//!     tauri::Builder::default()
//!         .plugin(tauri_plugin_logs::init())
//!         .setup(|app| {
//!             // отключение облачной отправки по настройке хоста:
//!             // tauri_plugin_logs::set_reporting_enabled(false);
//!             Ok(())
//!         })
//!         .run(tauri::generate_context!())
//!         .expect("error while running tauri application");
//! }
//! ```
//!
//! `tauri.conf.json`:
//! ```json
//! "plugins": {
//!   "logs": {
//!     "last_logs": true,
//!     "log_file_name": "king_orch.log",
//!     "reporting": {
//!       "enabled": true,
//!       "project": "my-app",
//!       "app_key": "A-EU-xxxxxxxxxxxxxxxx",
//!       "events": true
//!     }
//!   }
//! }
//! ```
//!
//! `capabilities/default.json`: добавить `"logs:default"`.

use serde::Deserialize;
use serde_json::Value;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::Wry;

mod commands;
mod error_reporting;
mod flight;
mod logger;
mod path;

pub use error_reporting::ReportingConfig;

/// Конфиг плагина (`plugins.logs` в `tauri.conf.json` хоста).
#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    /// Писать dev-зеркало `test/last_logs.txt` (только если есть каталог `test/`).
    pub last_logs: bool,
    /// Имя exe-файла лога (например `king_orch.log`); `None` — файла нет.
    pub log_file_name: Option<String>,
    /// Опциональная облачная отправка ошибок/аналитики.
    pub reporting: Option<ReportingConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            last_logs: true,
            log_file_name: None,
            reporting: None,
        }
    }
}

/// Инициализация плагина: `.plugin(tauri_plugin_logs::init())`.
pub fn init() -> TauriPlugin<Wry, Config> {
    Builder::<Wry, Config>::new("logs")
        .setup(|app, api| {
            let cfg = api.config();
            logger::install(app, cfg.last_logs, cfg.log_file_name.as_deref());
            let version = app.package_info().version.to_string();
            error_reporting::init(cfg.reporting.clone(), version);
            Ok(())
        })
        .on_event(|_app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                error_reporting::flush_blocking();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_last_logs_path,
            commands::get_log_file_path,
            commands::log_frontend_event,
            commands::track_event,
            commands::track_error,
            commands::dump_frontend_error,
            commands::set_reporting_enabled,
            commands::save_logs_file
        ])
        .build()
}

/// Ранняя (pre-Tauri) инициализация краш-лога. Вызывать первой строкой `main()`.
/// Устанавливает panic-hook и начинает писать в `test/last_logs.txt` и
/// в exe-файл лога.
pub fn early_init(log_file_name: &str) {
    logger::early_init(log_file_name);
}

/// Ранняя запись строки лога (строки запуска до старта Tauri).
pub fn early_log(level: &str, msg: &str) {
    logger::early_log(level, msg);
}

/// Запись строки «снаружи» логгера (backend-код) через единый конвейер.
pub fn log_line(level: &str, msg: &str) {
    logger::write_regular(level, msg);
}

/// Рантайм-флаг облачной отправки (настройка хоста — единственный источник правды).
pub fn set_reporting_enabled(enabled: bool) {
    error_reporting::set_enabled(enabled);
}

/// Аналитическое событие (например `app_started`).
pub fn track_event(name: &str, props: Option<Value>) {
    error_reporting::track_event(name, props);
}
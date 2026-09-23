//! 📦 tauri-plugin-downloader — единый источник правды скачивания файлов.
//!
//! Многоуровневый движок с прогрессом (скорость/ETA), stall-детектом,
//! resume и отменой. Уровни (следующий — если предыдущий не сработал):
//!   1. reqwest (multi-chunk при Range, stall 30s, ретраи, resume)
//!   2. curl (libcurl crate: low-speed abort, resume, прогресс)
//!   3. PowerShell Invoke-WebRequest (скрытое окно, таймаут, поллинг размера)
//!   4. bitsadmin (WinINet, скрытое окно, таймаут, поллинг)
//!   5. MCP/Deno downloader.ts (скрытое окно, таймаут, поллинг)
//!   6. Chrome CDP (скрытое окно, таймаут, поллинг)
//!
//! ## Подключение в хосте
//!
//! `Cargo.toml`:
//! ```toml
//! tauri-plugin-downloader = { path = "../../my-tauri-plugins/tauri-plugin-downloader" }
//! ```
//!
//! `main.rs`: `.plugin(tauri_plugin_downloader::init())`
//! `capabilities/default.json`: `"downloader:default"`
//!
//! ## Rust API (для других крейтов/плагинов)
//! ```ignore
//! tauri_plugin_downloader::download(app, url, dest, opts).await?;
//! tauri_plugin_downloader::download_blocking(url, dest, opts)?;
//! ```

pub mod commands;
pub mod engine;

use serde::Deserialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::Wry;

/// Конфиг плагина (`plugins.downloader` в `tauri.conf.json` хоста).
///
/// Устойчив к отсутствию секции: если ключа `downloader` в `plugins` нет,
/// Tauri передаёт `null`, и наивная десериализация в структуру паникует на
/// старте хоста ("invalid type: null, expected struct Config"). Здесь `null`
/// трактуется как конфиг по умолчанию.
#[derive(Clone, Default)]
pub struct Config {}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Принимаем null (нет секции) или любой объект (поля пока нет).
        let _ = Option::<serde::de::IgnoredAny>::deserialize(deserializer)?;
        Ok(Config::default())
    }
}

/// Инициализация плагина: `.plugin(tauri_plugin_downloader::init())`.
pub fn init() -> TauriPlugin<Wry, Config> {
    Builder::<Wry, Config>::new("downloader")
        .setup(|app, _api| {
            engine::progress::set_app(app.clone());
            Ok(())
        })
        .on_event(|_app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                engine::progress::cancel_all();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::download_file,
            commands::download_bytes,
            commands::cancel_download,
            commands::list_active,
        ])
        .build()
}

pub use engine::api::{
    download, download_blocking, download_bytes_blocking, download_bytes_to_vec, DownloadOptions,
};

#[cfg(test)]
mod tests {
    use super::*;

    // Регресс: Tauri передаёт null, если ключа plugins.downloader нет в конфиге.
    // Наивный #[derive(Deserialize)] падал: "invalid type: null, expected struct Config".
    #[test]
    fn config_accepts_null() {
        let c: Config = serde_json::from_str("null").expect("null → Config::default()");
        let _ = c;
    }

    #[test]
    fn config_accepts_empty_object() {
        let _c: Config = serde_json::from_str("{}").expect("{} → Config::default()");
    }

    #[test]
    fn config_accepts_unknown_fields() {
        let _c: Config = serde_json::from_str(r#"{"future_flag": true}"#)
            .expect("unknown fields ignored");
    }
}

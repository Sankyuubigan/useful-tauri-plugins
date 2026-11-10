//! tauri-plugin-ffmpeg — переиспользуемый движок FFmpeg.
//!
//! Плагин владеет: автоскачиванием официальной сборки ffmpeg/ffprobe в папку,
//! переданную хостом, запуском ffmpeg/ffprobe как subprocess (без shell),
//! снятием JSON-метаданных через ffprobe.
//!
//! Плагин нейтрален: ни одного продуктового имени/пути хоста внутри. Все
//! engine-функции принимают каталог установки параметром (`dir: &Path`).
//!
//! ## Подключение в хосте
//!
//! `Cargo.toml`:
//! ```toml
//! tauri-plugin-ffmpeg = { path = "../../my-tauri-plugins/tauri-plugin-ffmpeg" }
//! ```
//!
//! `main.rs`:
//! ```ignore
//! fn main() {
//!     tauri::Builder::default()
//!         .plugin(tauri_plugin_ffmpeg::init())
//!         .run(tauri::generate_context!())
//! }
//! ```
//!
//! Нативный тул хоста вызывает engine-функции напрямую (без AppHandle):
//! `engine::ensure_ffmpeg(dir, &on_log)`, `engine::run_tool(exe, args, timeout)`,
//! `engine::probe_json(ffprobe, path)`.

pub mod commands;
pub mod engine;

use serde::Deserialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::Wry;

/// Конфиг из `tauri.conf.json` хоста (`plugins.ffmpeg`). Пока пустой —
/// все параметры движок получает аргументами (нейтральность).
#[derive(Clone, Default)]
pub struct Config {}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Секции может не быть / она может быть null — обязан переживать (PLUGIN_STANDARD §4.3).
        let _ = Option::<serde::de::IgnoredAny>::deserialize(deserializer)?;
        Ok(Config::default())
    }
}

/// Вызывается из хоста: `.plugin(tauri_plugin_ffmpeg::init())`.
pub fn init() -> TauriPlugin<Wry, Config> {
    Builder::<Wry, Config>::new("ffmpeg")
        .invoke_handler(tauri::generate_handler![commands::ffmpeg_status])
        .build()
}

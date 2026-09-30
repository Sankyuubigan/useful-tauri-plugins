//! tauri-plugin-ytdlp — переиспользуемый движок yt-dlp.
//!
//! Плагин владеет: установкой/обновлением yt-dlp.exe, проверкой статуса,
//! скачиванием YouTube-видео с прогрессом, управлением Deno и FFmpeg.
//!
//! ## Подключение в хосте
//!
//! `Cargo.toml`:
//! ```toml
//! tauri-plugin-ytdlp = { path = "../../my-tauri-plugins/tauri-plugin-ytdlp" }
//! ```
//!
//! `main.rs`:
//! ```ignore
//! fn main() {
//!     tauri::Builder::default()
//!         .plugin(tauri_plugin_ytdlp::init())
//!         .run(tauri::generate_context!())
//! }
//! ```
//!
//! `tauri.conf.json`:
//! ```json
//! "plugins": { "ytdlp": {} }
//! ```
//!
//! `capabilities/default.json`: добавить `"ytdlp:default"`.

pub mod commands;
pub mod engine;

use serde::Deserialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{RunEvent, Wry};

#[derive(Clone, Default)]
pub struct Config {
    pub data_dir_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ConfigInner {
    data_dir_name: Option<String>,
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Option::<ConfigInner>::deserialize(deserializer)?
            .map(|c| Config { data_dir_name: c.data_dir_name })
            .unwrap_or_default())
    }
}

pub fn init() -> TauriPlugin<Wry, Config> {
    Builder::<Wry, Config>::new("ytdlp")
        .setup(|_app, _api| {
            Ok(())
        })
        .on_event(|_app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                engine::downloader::kill_active_downloads();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_ytdlp_status,
            commands::install_ytdlp,
            commands::check_ytdlp_update,
            commands::install_ytdlp_update,
            commands::set_ytdlp_dir,
            commands::set_ffmpeg_path,
            commands::install_deno,
            commands::fetch_video_info,
            commands::download_video,
            commands::cancel_download,
        ])
        .build()
}

//! Движок yt-dlp: управление бинарником, конфиг, скачивание видео.

pub mod downloader;
pub mod ytdlp_manager;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;
use tauri::Manager;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct YtdlpConfig {
    #[serde(default)]
    pub ytdlp_dir: Option<String>,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub deno_dir: Option<String>,
    #[serde(default)]
    pub download_path: Option<String>,
    #[serde(default)]
    pub auth_mode: Option<String>,
    #[serde(default)]
    pub auth_browser: Option<String>,
    #[serde(default)]
    pub cookies_file: Option<String>,
    #[serde(default)]
    pub download_format: Option<String>,
    #[serde(default)]
    pub download_quality: Option<String>,
}

pub fn load_config(app: &AppHandle) -> YtdlpConfig {
    let path = config_path(app);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let mut cfg: YtdlpConfig = serde_json::from_str(&text).unwrap_or_default();
            // Merge defaults for missing fields
            if cfg.auth_mode.is_none() { cfg.auth_mode = Some("none".to_string()); }
            if cfg.auth_browser.is_none() { cfg.auth_browser = Some("chrome".to_string()); }
            if cfg.download_format.is_none() { cfg.download_format = Some("mp4".to_string()); }
            if cfg.download_quality.is_none() { cfg.download_quality = Some("max".to_string()); }
            cfg
        }
        Err(_) => YtdlpConfig {
            auth_mode: Some("none".to_string()),
            auth_browser: Some("chrome".to_string()),
            download_format: Some("mp4".to_string()),
            download_quality: Some("max".to_string()),
            ..Default::default()
        },
    }
}

pub fn save_config(app: &AppHandle, cfg: &YtdlpConfig) -> Result<(), String> {
    let path = config_path(app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Не удалось создать директорию: {e}"))?;
    }
    let json = serde_json::to_string_pretty(cfg)
        .map_err(|e| format!("Ошибка сериализации: {e}"))?;
    ko_json_store::write_atomic(&path, &json)
        .map_err(|e| format!("Ошибка записи конфига: {e}"))
}

fn config_path(app: &AppHandle) -> PathBuf {
    let base = app.path().app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("ytdlp_config.json")
}

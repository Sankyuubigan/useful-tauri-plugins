//! Tauri-команды плагина ytdlp: статус/установка/обновление yt-dlp,
//! скачивание видео, управление Deno и FFmpeg.

use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

use crate::engine::{self, downloader, ytdlp_manager};

#[derive(Serialize, Clone)]
pub struct YtdlpStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub exe_path: String,
    pub dir: String,
    pub ffmpeg_installed: bool,
    pub ffmpeg_path: Option<String>,
    pub deno_installed: bool,
    pub deno_path: Option<String>,
    pub message: String,
}

fn ytdlp_dir(app: &AppHandle) -> PathBuf {
    let cfg = engine::load_config(app);
    if let Some(p) = &cfg.ytdlp_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let exe_dir = match std::env::current_exe() {
        Ok(p) => p.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")),
        Err(_) => app
            .path()
            .executable_dir()
            .unwrap_or_else(|_| PathBuf::from(".")),
    };
    ytdlp_manager::default_dir(&exe_dir)
}

#[tauri::command]
pub async fn get_ytdlp_status(app: AppHandle) -> Result<YtdlpStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = ytdlp_dir(&app);
        ytdlp_manager::get_status(&dir, &app)
    })
    .await
    .map_err(|e| format!("Ошибка чтения статуса yt-dlp: {e}"))?
}

#[tauri::command]
pub async fn install_ytdlp(app: AppHandle) -> Result<YtdlpStatus, String> {
    let dir = ytdlp_dir(&app);
    let app_clone = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let log_cb = |msg: String| log::info!("[YTDLP] {msg}");
        ytdlp_manager::install_or_update(&dir, &log_cb)?;
        ytdlp_manager::get_status(&dir, &app_clone)
    })
    .await
    .map_err(|e| format!("Ошибка установки yt-dlp: {e}"))?
}

#[tauri::command]
pub async fn check_ytdlp_update(app: AppHandle) -> Result<Option<String>, String> {
    let dir = ytdlp_dir(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let log_cb = |msg: String| log::info!("[YTDLP] {msg}");
        ytdlp_manager::check_update(&dir, &log_cb)
    })
    .await
    .map_err(|e| format!("Ошибка проверки обновления: {e}"))?
}

#[tauri::command]
pub async fn install_ytdlp_update(app: AppHandle) -> Result<YtdlpStatus, String> {
    install_ytdlp(app).await
}

#[tauri::command]
pub fn set_ytdlp_dir(app: AppHandle, path: String) -> Result<YtdlpStatus, String> {
    let mut cfg = engine::load_config(&app);
    cfg.ytdlp_dir = Some(path);
    engine::save_config(&app, &cfg)?;
    let dir = ytdlp_dir(&app);
    ytdlp_manager::get_status(&dir, &app)
}

#[tauri::command]
pub fn set_ffmpeg_path(app: AppHandle, path: String) -> Result<YtdlpStatus, String> {
    let mut cfg = engine::load_config(&app);
    cfg.ffmpeg_path = Some(path);
    engine::save_config(&app, &cfg)?;
    let dir = ytdlp_dir(&app);
    ytdlp_manager::get_status(&dir, &app)
}

#[tauri::command]
pub async fn install_deno(app: AppHandle) -> Result<YtdlpStatus, String> {
    let dir = ytdlp_dir(&app);
    let app_clone = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let log_cb = |msg: String| log::info!("[YTDLP] {msg}");
        ytdlp_manager::install_deno(&dir, &log_cb)?;
        ytdlp_manager::get_status(&dir, &app_clone)
    })
    .await
    .map_err(|e| format!("Ошибка установки Deno: {e}"))?
}

#[tauri::command]
pub async fn fetch_video_info(app: AppHandle, url: String) -> Result<serde_json::Value, String> {
    let dir = ytdlp_dir(&app);
    tauri::async_runtime::spawn_blocking(move || {
        downloader::fetch_info(&dir, &url)
    })
    .await
    .map_err(|e| format!("Ошибка получения информации: {e}"))?
}

#[tauri::command]
pub async fn download_video(
    app: AppHandle,
    url: String,
    format: String,
    quality: String,
    output_dir: String,
    auth_mode: String,
    browser: Option<String>,
    cookies_file: Option<String>,
) -> Result<(), String> {
    let dir = ytdlp_dir(&app);
    downloader::start_download(
        app.clone(),
        dir,
        url,
        format,
        quality,
        output_dir,
        auth_mode,
        browser,
        cookies_file,
    )
    .await
}

#[tauri::command]
pub fn cancel_download(app: AppHandle) -> Result<(), String> {
    downloader::cancel_active();
    let _ = app.emit("ytdlp:download-cancelled", ());
    Ok(())
}

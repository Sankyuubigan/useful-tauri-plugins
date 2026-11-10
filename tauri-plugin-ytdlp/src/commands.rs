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
    pub proxy: Option<String>,
    /// Сохранённый источник кукисов (для префилла панели).
    pub cookie_browser: Option<String>,
    pub cookie_profile_dir: Option<String>,
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
pub fn set_ytdlp_proxy(app: AppHandle, proxy: String) -> Result<YtdlpStatus, String> {
    let mut cfg = engine::load_config(&app);
    cfg.proxy = if proxy.trim().is_empty() {
        None
    } else {
        Some(proxy.trim().to_string())
    };
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
    let cfg = engine::load_config(&app);
    let proxy = cfg.proxy;
    // «Из браузера» = сохранённый cookies.txt (механика dpi_gui). Файла нет —
    // честная ошибка с путём лечения, ДО запуска yt-dlp.
    let cookies_file = if auth_mode == "browser" {
        let saved = crate::browser_cookies::saved_cookies_path(&app);
        if !saved.is_file() {
            return Err(
                "Кукисы из браузера не сохранены: на вкладке Ютуб (настройки загрузчика) \
                 нажми «Забрать кукисы», затем повтори скачивание."
                    .to_string(),
            );
        }
        Some(saved.to_string_lossy().into_owned())
    } else {
        cookies_file
    };
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
        proxy,
    )
    .await
}

#[tauri::command]
pub fn cancel_download(app: AppHandle) -> Result<(), String> {
    downloader::cancel_active();
    let _ = app.emit("ytdlp:download-cancelled", ());
    Ok(())
}

// ===== Кукисы браузера (anti-bot YouTube) — механика dpi_gui (rookie) =====

/// Статус захвата кукисов для UI: количества и ноты. Значения кукисов не
/// возвращаются и в лог не пишутся.
#[derive(Serialize, Clone)]
pub struct GrabCookiesStatus {
    pub count: usize,
    pub source: String,
    pub notes: Vec<String>,
    pub file: String,
}

#[tauri::command]
pub async fn grab_ytdlp_cookies(
    app: AppHandle,
    browser: Option<String>,
    profile_dir: Option<String>,
) -> Result<GrabCookiesStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (outcome, path) =
            crate::browser_cookies::grab_and_save(&app, browser.as_deref(), profile_dir.as_deref())?;
        // Источник — в конфиг для префилла UI (даже при 0 кукисов, как в dpi_gui);
        // сбой сохранения конфига не роняет результат захвата.
        let mut cfg = engine::load_config(&app);
        cfg.cookie_browser = browser
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string());
        cfg.cookie_profile_dir = profile_dir
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string());
        if let Err(e) = engine::save_config(&app, &cfg) {
            log::warn!("[YTDLP] Источник кукисов не сохранён в конфиг: {e}");
        }
        Ok(GrabCookiesStatus {
            count: outcome.count,
            source: outcome.source,
            notes: outcome.notes,
            file: path.to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|e| format!("Ошибка потока: {e}"))?
}

// ===== Субтитры YouTube (SSOT-механика — engine::subtitles) =====

/// Подготовка транскрипта YouTube: yt-dlp → VTT → чистка спонсорских → чанки.
/// Тонкая обёртка для UI: механика — `engine::subtitles` (нативный тул хоста
/// вызывает engine-функции напрямую, без AppHandle).
#[tauri::command]
pub async fn prepare_transcript(
    app: AppHandle,
    url: String,
    chunk_size: Option<usize>,
    cookies_file: Option<String>,
) -> Result<engine::subtitles::PreparedTranscript, String> {
    let dir = ytdlp_dir(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let cookies = cookies_file.as_deref().map(std::path::Path::new);
        engine::subtitles::prepare_transcript(
            &dir,
            &url,
            chunk_size.unwrap_or(engine::subtitles::DEFAULT_CHUNK_SIZE),
            cookies,
        )
    })
    .await
    .map_err(|e| format!("Ошибка подготовки транскрипта: {e}"))?
    .map_err(|e| e.to_string())
}

/// Чанк транскрипта (1-based) из кеша модуля субтитров.
#[tauri::command]
pub async fn get_transcript_chunk(url: String, chunk_index: usize) -> Result<engine::subtitles::TranscriptChunk, String> {
    tauri::async_runtime::spawn_blocking(move || {
        engine::subtitles::get_transcript_chunk(&url, chunk_index)
    })
    .await
    .map_err(|e| format!("Ошибка чтения чанка транскрипта: {e}"))?
    .map_err(|e| e.to_string())
}

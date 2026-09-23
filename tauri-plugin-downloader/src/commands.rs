//! Tauri-команды плагина (тонкий слой → engine::api).

use crate::engine::api::{download, download_bytes_to_vec, DownloadOptions};
use crate::engine::progress;
use tauri::AppHandle;

/// Скачать файл в dest. Ожидает завершения; прогресс — событие `downloader:progress`.
#[tauri::command]
pub async fn download_file(
    _app: AppHandle,
    url: String,
    dest: String,
    opts: Option<DownloadOptions>,
) -> Result<(), String> {
    let opts = opts.unwrap_or_default();
    download(&url, dest, opts, None).await
}

/// Скачать файл во временный файл и вернуть байты (маленькие ответы: JSON и т.п.).
#[tauri::command]
pub async fn download_bytes(
    _app: AppHandle,
    url: String,
    opts: Option<DownloadOptions>,
) -> Result<Vec<u8>, String> {
    let opts = opts.unwrap_or_default();
    download_bytes_to_vec(&url, opts, None).await
}

/// Отменить активную загрузку по task_id. Возвращает true, если задача найдена.
#[tauri::command]
pub fn cancel_download(task_id: String) -> bool {
    progress::cancel(&task_id)
}

/// Список активных загрузок.
#[tauri::command]
pub fn list_active() -> Vec<serde_json::Value> {
    progress::list_active()
}

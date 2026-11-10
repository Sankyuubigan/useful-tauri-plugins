//! Tauri-команды плагина: статус движка для будущей UI/диагностики.
//! Тяжёлая логика — в `engine/`; команды только формируют отчёт.

use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Runtime};

/// Отчёт о состоянии установленного движка (camelCase для JS-стороны).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegStatus {
    pub installed: bool,
    pub ffmpeg_version: Option<String>,
    pub ffprobe_version: Option<String>,
    pub dir: String,
    pub bin_dir: String,
    pub message: String,
}

/// Статус FFmpeg в переданном каталоге установки (сам каталог `ffmpeg`,
/// не корень хранилища — резолвит хост, плагин нейтрален).
#[tauri::command]
pub fn ffmpeg_status<R: Runtime>(
    _app: AppHandle<R>,
    dir: String,
) -> FfmpegStatus {
    let dir = PathBuf::from(dir);
    let installed = crate::engine::installer::is_installed(&dir);
    let (ffmpeg_version, ffprobe_version) = if installed {
        (
            crate::engine::installer::tool_version(&crate::engine::installer::ffmpeg_exe(&dir)).ok(),
            crate::engine::installer::tool_version(&crate::engine::installer::ffprobe_exe(&dir)).ok(),
        )
    } else {
        (None, None)
    };
    let message = if installed {
        format!(
            "FFmpeg установлен (ffmpeg {})",
            ffmpeg_version.as_deref().unwrap_or("версия неизвестна")
        )
    } else {
        "FFmpeg не установлен. Он будет скачан автоматически при первом использовании.".to_string()
    };
    let bin_dir = dir.join("bin");
    FfmpegStatus {
        installed,
        ffmpeg_version,
        ffprobe_version,
        dir: dir.to_string_lossy().to_string(),
        bin_dir: bin_dir.to_string_lossy().to_string(),
        message,
    }
}

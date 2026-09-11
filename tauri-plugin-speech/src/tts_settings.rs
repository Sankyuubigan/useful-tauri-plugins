use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{Manager, Runtime};

/// Сохраняемые настройки TTS-движка CrispASR.
///
/// Сериализуется в `tts_settings.json` внутри app-config dir (100% локально, без облака).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TtsSettings {
    /// Папка, где живут бинари движка по бэкендам: `<engine_dir>/<backend>/crispasr.exe`.
    pub engine_dir: String,
    /// ОБЩАЯ папка моделей TTS. Внутри — подпапка на каждый пресет.
    pub models_dir: String,
    /// Выбранный тип бэкенда движка: "cpu" / "cuda" / "vulkan" / "cpu-legacy" / …
    pub engine_backend: String,
    /// Выбранный пресет TTS-модели.
    pub preset: String,
}

const FILE_NAME: &str = "tts_settings.json";

fn config_path<R: Runtime>(app: &tauri::AppHandle<R>) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| {
            std::env::current_exe()
                .unwrap_or_else(|_| PathBuf::from("."))
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf()
        })
        .join(FILE_NAME)
}

/// Загружает настройки из диска. При отсутствии/невалидном файле возвращает дефолтные.
pub fn load<R: Runtime>(app: &tauri::AppHandle<R>) -> TtsSettings {
    let path = config_path(app);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => TtsSettings::default(),
    }
}

/// Сохраняет настройки на диск.
pub fn save<R: Runtime>(
    app: &tauri::AppHandle<R>,
    settings: &TtsSettings,
) -> Result<(), String> {
    let path = config_path(app);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("не удалось сохранить настройки: {e}"))
}
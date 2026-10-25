//! Конфиг image-движка: читается/пишется в общий `app_config.json` хоста
//! field-preserving-merge (хостовые ключи НЕ затираются — тот же паттерн,
//! что у llama-engine `save_engine_config_file`).
//!
//! Ключи: `sdcpp_dir` (переопределение папки движка),
//! `image_engine_variant` ("auto"/cuda12/cpu/vulkan/rocm),
//! `image_bundle_dir` (папка скачанного бандла весов),
//! `selected_bundle` (активный бандл каталога — выбор юзера в UI).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ImageEngineConfig {
    #[serde(default)]
    pub sdcpp_dir: Option<String>,
    #[serde(default)]
    pub image_engine_variant: Option<String>,
    #[serde(default)]
    pub image_bundle_dir: Option<String>,
    #[serde(default)]
    pub selected_bundle: Option<String>,
    /// Папка видео-бандлов (MiniMax H3). De factо жил РЯДОМ с image-бандлами.
    #[serde(default)]
    pub video_bundle_dir: Option<String>,
    /// Активный каталог-Bar video-бандл (выбор юзера в UI).
    #[serde(default)]
    pub selected_video_bundle: Option<String>,
}

/// Путь к `app_config.json` хоста (runtime, через AppHandle).
pub fn get_config_path(_app: &AppHandle) -> PathBuf {
    // Конфиг image-engine лежит в каталоге хоста, но путь выбирает общая дверь
    // `ko-json-store`: прямой `app.path().app_data_dir()` приводил к тому, что
    // хост писал в один каталог, а плагин искал другой — и обе стороны
    // выглядели исправными, пока настройки не пропадали.
    ko_json_store::roots::app_dir(HOST_APP_IDENTIFIER, "").join("app_config.json")
}

/// Идентификатор приложения-хоста: имя папки данных King Orch.
const HOST_APP_IDENTIFIER: &str = "com.kingorch.app";

/// Читает image-ключи ДО создания Tauri-приложения (тулы оркестратора, рантайм
/// без AppHandle). Путь — тот же корень, что и у хоста.
pub fn load_image_config_early() -> ImageEngineConfig {
    let path = ko_json_store::roots::app_dir(HOST_APP_IDENTIFIER, "").join("app_config.json");
    match ko_json_store::read_text(&path) {
        Ok(Some(data)) => serde_json::from_str(&data).unwrap_or_default(),
        Ok(None) => ImageEngineConfig::default(),
        Err(e) => {
            log::warn!("[image-engine] конфиг {} не прочитан ({}): беру значения по умолчанию", path.display(), e);
            ImageEngineConfig::default()
        }
    }
}

/// Папка движка без AppHandle: явная из конфига, иначе `<exe>/sdcpp`.
pub fn engine_dir_early() -> PathBuf {
    let cfg = load_image_config_early();
    if let Some(p) = &cfg.sdcpp_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    crate::engine::sdcpp_installer::default_dir(&exe_dir)
}

/// Папка бандлов без AppHandle: явная из конфига, иначе `<exe>/image_models`.
/// Одна папка-пул для ВСЕХ бандлов (имена общих ролей совпадают, downloader
/// пропускает существующее) — per-bundle подпапки убрали 25.10.2026: они
/// порождали перекачку общих файлов и «3 файла есть, 1 нет» при переключении
/// активного бандла.
pub fn bundle_dir_early() -> PathBuf {
    let cfg = load_image_config_early();
    if let Some(p) = &cfg.image_bundle_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("image_models")
}

/// Папка видео-бандлов без AppHandle: явная из конфига, иначе `<exe>/video_models`.
/// Тот же пул-принцип, что у image: diffusion-файлы разных бандлов имеют
/// разные имена, общие роли (llm/vae/audio_vae) — одинаковые.
pub fn video_bundle_dir_early() -> PathBuf {
    let cfg = load_image_config_early();
    if let Some(p) = &cfg.video_bundle_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("video_models")
}

/// Читает ТОЛЬКО image-ключи (остальное игнорируется serde по умолчанию).
pub fn load_image_config(app: &AppHandle) -> ImageEngineConfig {
    if let Ok(data) = fs::read_to_string(get_config_path(app)) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        ImageEngineConfig::default()
    }
}

/// Сохраняет image-ключи field-preserving-merge.
pub fn save_image_config(app: &AppHandle, config: &ImageEngineConfig) -> Result<(), String> {
    let path = get_config_path(app);
    let image_value = serde_json::to_value(config).map_err(|e| e.to_string())?;
    ko_json_store::update_json(&path, move |root: &mut serde_json::Value| {
        if !root.is_object() {
            *root = serde_json::json!({});
        }
        if let (serde_json::Value::Object(root_map), serde_json::Value::Object(image_map)) =
            (root, image_value)
        {
            for (k, v) in image_map {
                root_map.insert(k, v);
            }
        }
    })
}

//! Конфиг image-движка: читается/пишется в общий `app_config.json` хоста
//! field-preserving-merge (хостовые ключи НЕ затираются — тот же паттерн,
//! что у llama-engine `save_engine_config_file`).
//!
//! Ключи: `sdcpp_dir` (переопределение папки движка),
//! `image_engine_variant` ("auto"/cuda12/cpu/vulkan/rocm),
//! `image_bundle_dir` (папка скачанного бандла весов).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ImageEngineConfig {
    #[serde(default)]
    pub sdcpp_dir: Option<String>,
    #[serde(default)]
    pub image_engine_variant: Option<String>,
    #[serde(default)]
    pub image_bundle_dir: Option<String>,
}

/// Путь к `app_config.json` хоста (runtime, через AppHandle).
pub fn get_config_path(app: &AppHandle) -> PathBuf {
    let base = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
    if !base.exists() {
        let _ = fs::create_dir_all(&base);
    }
    base.join("app_config.json")
}

/// Читает image-ключи ДО создания Tauri-приложения (тулы оркестратора, рантайм
/// без AppHandle). Имя папки данных — как у хоста King Orch.
pub fn load_image_config_early() -> ImageEngineConfig {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let path = base.join("com.kingorch.app").join("app_config.json");
    if let Ok(data) = fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        ImageEngineConfig::default()
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

/// Папка бандла без AppHandle: явная из конфига, иначе `<exe>/image_models/<name>`.
pub fn bundle_dir_early() -> PathBuf {
    let cfg = load_image_config_early();
    if let Some(p) = &cfg.image_bundle_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let name = crate::engine::models_catalog::default_bundle_entry()
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "qwen-image-2.1".to_string());
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("image_models").join(name)
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
pub fn save_image_config(app: &AppHandle, config: &ImageEngineConfig) {
    let path = get_config_path(app);
    let mut root: serde_json::Value = fs::read_to_string(&path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let image_value = serde_json::to_value(config).unwrap_or_else(|_| serde_json::json!({}));
    if let (serde_json::Value::Object(root_map), serde_json::Value::Object(image_map)) =
        (&mut root, image_value)
    {
        for (k, v) in image_map {
            root_map.insert(k, v);
        }
    }
    if let Ok(data) = serde_json::to_string_pretty(&root) {
        let _ = fs::write(path, data);
    }
}

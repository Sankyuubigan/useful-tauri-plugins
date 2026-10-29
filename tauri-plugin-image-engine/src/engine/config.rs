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
use std::path::{Path, PathBuf};
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
    /// Папка LoRA-файлов (NSFW-стек и позы). Явная — из конфига, иначе
    /// дефолт `image_lora_dir_early()` (`<KingOrchData>/image_loras`).
    #[serde(default)]
    pub image_lora_dir: Option<String>,
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

/// Имя папки общего хранилища данных хоста — зеркало
/// `infra::data_dir::DATA_DIR_NAME` хоста. Плагин не может импортировать хост,
/// поэтому приоритет пути повторён 1:1: явный `data_dir` из общего конфига →
/// диск с макс. свободным местом + `KingOrchData` → фоллбэк `<exe>/KingOrchData`.
pub const HOST_DATA_DIR_NAME: &str = "KingOrchData";

/// Нормализация пути хранилища (зеркало `data_dir::normalize` хоста): последний
/// компонент обязан быть KingOrchData (без учёта регистра), иначе приклеивается.
/// Чистая функция, без обращений к файловой системе.
fn normalize_data_root(raw: &str) -> PathBuf {
    let trimmed = raw.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return PathBuf::from(HOST_DATA_DIR_NAME);
    }
    let p = Path::new(trimmed);
    let needs_suffix = p
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| !n.eq_ignore_ascii_case(HOST_DATA_DIR_NAME))
        .unwrap_or(true);
    if needs_suffix {
        p.join(HOST_DATA_DIR_NAME)
    } else {
        p.to_path_buf()
    }
}

/// Диск с макс. свободным местом (зеркало `data_dir::best_drive_letter` хоста).
fn best_drive_letter() -> Option<String> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut best: Option<(String, u64)> = None;
    for disk in disks.list() {
        let mount = disk.mount_point().to_string_lossy().to_string();
        if mount.len() >= 2 && mount.as_bytes()[1] == b':' {
            let letter = mount[..1].to_uppercase();
            let available = disk.available_space();
            match &best {
                Some((_, best_avail)) if available <= *best_avail => {}
                _ => best = Some((letter, available)),
            }
        }
    }
    best.map(|(letter, _)| letter)
}

/// Корень общего хранилища данных хоста (`KingOrchData`) без AppHandle:
/// `data_dir` из общего app_config.json (тот же файл, что читают все методы
/// конфига) → диск с макс. свободным местом → фоллбэк `<exe>/KingOrchData`.
pub fn data_root() -> PathBuf {
    let path = ko_json_store::roots::app_dir(HOST_APP_IDENTIFIER, "").join("app_config.json");
    if let Ok(Some(text)) = ko_json_store::read_text(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(raw) = v.get("data_dir").and_then(|x| x.as_str()) {
                if !raw.trim().is_empty() {
                    return normalize_data_root(raw);
                }
            }
        }
    }
    match best_drive_letter() {
        Some(letter) => PathBuf::from(format!("{}:\\{}", letter, HOST_DATA_DIR_NAME)),
        None => std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(HOST_DATA_DIR_NAME),
    }
}

/// Дефолтная папка image-бандлов: `<хранилище>/image_models` (пул всех наборов).
pub fn default_image_bundle_dir() -> PathBuf {
    data_root().join("image_models")
}

/// Дефолтная папка видео-бандлов: `<хранилище>/video_models`.
pub fn default_video_bundle_dir() -> PathBuf {
    data_root().join("video_models")
}

/// Дефолтная папка LoRA: `<хранилище>/image_loras`.
pub fn default_image_lora_dir() -> PathBuf {
    data_root().join("image_loras")
}

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

/// Папка бандлов без AppHandle: явная из конфига, иначе
/// `<хранилище KingOrchData>/image_models` (пул всех наборов).
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
    default_image_bundle_dir()
}

/// Папка видео-бандлов без AppHandle: явная из конфига, иначе
/// `<хранилище KingOrchData>/video_models`. Тот же пул-принцип, что у image:
/// diffusion-файлы разных бандлов имеют разные имена, общие роли
/// (llm/vae/audio_vae/lora_draft) — одинаковые.
pub fn video_bundle_dir_early() -> PathBuf {
    let cfg = load_image_config_early();
    if let Some(p) = &cfg.video_bundle_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    default_video_bundle_dir()
}

/// Папка LoRA без AppHandle: явная из конфига, иначе
/// `<хранилище KingOrchData>/image_loras`. Тулы оркестратора и
/// tauri-команды используют early-версию (тот же app_config.json хоста).
pub fn image_lora_dir_early() -> PathBuf {
    let cfg = load_image_config_early();
    if let Some(p) = &cfg.image_lora_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    default_image_lora_dir()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Новый ключ image_lora_dir: serde-дефолт — None (пустой JSON читается
    /// без ошибок, старые конфиги не ломаются).
    #[test]
    fn image_lora_dir_key_defaults_to_none() {
        let cfg: ImageEngineConfig = serde_json::from_str("{}").expect("пустой JSON");
        assert!(cfg.image_lora_dir.is_none());
    }

    /// Зеркало тестов `data_dir::normalize` хоста: конвенция одна, ломаться
    /// должна одинаково.
    #[test]
    fn normalize_data_root_appends_suffix() {
        assert_eq!(normalize_data_root("D:\\foo"), PathBuf::from("D:\\foo\\KingOrchData"));
        assert_eq!(normalize_data_root("D:\\foo\\"), PathBuf::from("D:\\foo\\KingOrchData"));
        assert_eq!(normalize_data_root("D:\\foo\\bar"), PathBuf::from("D:\\foo\\bar\\KingOrchData"));
    }

    #[test]
    fn normalize_data_root_keeps_existing_suffix() {
        assert_eq!(
            normalize_data_root("D:\\foo\\KingOrchData"),
            PathBuf::from("D:\\foo\\KingOrchData")
        );
        assert_eq!(
            normalize_data_root("D:\\foo\\kingorchdata\\"),
            PathBuf::from("D:\\foo\\kingorchdata")
        );
    }

    #[test]
    fn normalize_data_root_no_partial_match_and_empty() {
        assert_eq!(
            normalize_data_root("D:\\MyKingOrchData"),
            PathBuf::from("D:\\MyKingOrchData\\KingOrchData")
        );
        assert_eq!(normalize_data_root(""), PathBuf::from("KingOrchData"));
        assert_eq!(normalize_data_root("   "), PathBuf::from("KingOrchData"));
    }
}

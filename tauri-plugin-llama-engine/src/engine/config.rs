//! Конфигурация движка llama.cpp (движковые ключи `app_config.json` хоста).
//!
//! Плагин и хост делят ОДИН файл `app_config.json`. Чтобы не затирать хостовые
//! ключи (theme, allow_error_reports, translator_*, …), плагин пишет ТОЛЬКО
//! движковые ключи через field-preserving merge (`EngineConfig`), а читает
//! полный JSON с default-значениями. Хост держит свой полный `AppConfig`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::AppHandle;
use tauri::Manager;

#[derive(Serialize, Deserialize, Clone)]
pub struct ModelParams {
    pub temperature: f32,
    pub top_k: u32,
    pub top_p: f32,
    pub min_p: f32,
    pub repetition_penalty: f32,
    pub presence_penalty: f32,
    #[serde(default)]
    pub dry_multiplier: f32,
    #[serde(default = "default_dry_base")]
    pub dry_base: f32,
    #[serde(default = "default_dry_allowed_length")]
    pub dry_allowed_length: i32,
    #[serde(default)]
    pub dry_penalty_last_n: i32,
    #[serde(default)]
    pub xtc_probability: f32,
    #[serde(default = "default_xtc_threshold")]
    pub xtc_threshold: f32,
}

fn default_dry_base() -> f32 { 1.75 }
fn default_dry_allowed_length() -> i32 { 2 }
fn default_xtc_threshold() -> f32 { 0.1 }

impl Default for ModelParams {
    fn default() -> Self {
        Self {
            temperature: 0.5,
            top_k: 40,
            top_p: 0.95,
            min_p: 0.1,
            repetition_penalty: 1.15,
            presence_penalty: 0.0,
            dry_multiplier: 0.8,
            dry_base: 1.75,
            dry_allowed_length: 2,
            dry_penalty_last_n: 256,
            xtc_probability: 0.0,
            xtc_threshold: 0.1,
        }
    }
}

/// Движковый подмножество `app_config.json`. Поле за полем совпадает с ключами
/// хостового `AppConfig`, чтобы оба участника видели один и тот же JSON.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct EngineConfig {
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub last_model: Option<String>,
    #[serde(default)]
    pub models_dir: Option<String>,
    #[serde(default)]
    pub model_params: HashMap<String, ModelParams>,
    #[serde(default)]
    pub mmproj_files: HashMap<String, String>,
    #[serde(default)]
    pub model_meta: HashMap<String, ModelMeta>,
    #[serde(default)]
    pub llamacpp_dir: Option<String>,
    /// Источник бинарей движка: "ggml-org" (дефолт) / "beellama" (KVarN).
    /// None = дефолт (ggml-org).
    #[serde(default)]
    pub engine_source: Option<String>,
    /// Предпочтение юзера: "auto" / "cpu" / "cuda-12.4" / "cuda-13.3" / "vulkan" /
    /// "hip-radeon". None = авто.
    #[serde(default)]
    pub engine_variant: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ModelMeta {
    #[serde(default)]
    pub uncen: bool,
    #[serde(default)]
    pub vision: bool,
    #[serde(default)]
    pub audio: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CatalogEntry {
    pub name: String,
    pub download_url: String,
    #[serde(default)]
    pub size_gb: Option<String>,
    #[serde(default)]
    pub tokenizer_id: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub mmproj_url: Option<String>,
    #[serde(default)]
    pub uncen: Option<bool>,
    #[serde(default)]
    pub vision: Option<bool>,
    #[serde(default)]
    pub audio: Option<bool>,
}

// ────────────────────────────── Путь к конфигу ──────────────────────────────

/// Имя папки данных приложения (APPDATA/<name>).
///
/// Используется только для чтения конфига до создания Tauri-приложения, когда
/// `AppHandle` ещё недоступен (движок `LlamaEngine` читает движковые ключи
/// конфига в рантайме без хендла). Хост обязан вызвать
/// [`set_app_data_dir_name`] в `main()` (или задать `plugins.llama-engine.data_dir_name`
/// в tauri.conf.json); по умолчанию — legacy-имя King Orch.
static APP_DATA_DIR_NAME: OnceLock<String> = OnceLock::new();

/// Задать имя папки данных приложения (обычно = `identifier` из tauri.conf.json
/// хоста). Вызывать в начале `main()` хоста:
/// `tauri_plugin_llama_engine::engine::config::set_app_data_dir_name("com.kingorch.app");`
pub fn set_app_data_dir_name(name: &str) {
    let _ = APP_DATA_DIR_NAME.set(name.to_string());
}

fn app_data_dir_name() -> &'static str {
    APP_DATA_DIR_NAME
        .get()
        .map(|s| s.as_str())
        .unwrap_or("com.kingorch.app") // legacy-значение King Orch (обратная совместимость)
}

/// Папка данных приложения без AppHandle (APPDATA/<name>).
pub fn app_data_dir_early() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(app_data_dir_name())
}

/// Путь к `app_config.json` (runtime, через AppHandle).
pub fn get_config_path(app: &AppHandle) -> PathBuf {
    let base = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
    if !base.exists() {
        let _ = fs::create_dir_all(&base);
    }
    base.join("app_config.json")
}

/// Читает конфиг ДО создания Tauri-приложения (движок, рантайм без AppHandle).
pub fn load_config_early() -> EngineConfig {
    let path = app_data_dir_early().join("app_config.json");
    if let Ok(data) = fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        EngineConfig::default()
    }
}

pub fn load_config(app: &AppHandle) -> EngineConfig {
    if let Ok(data) = fs::read_to_string(get_config_path(app)) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        EngineConfig::default()
    }
}

/// Сохраняет движковые ключи в `app_config.json` field-preserving-merge:
/// существующие хостовые ключи (theme, allow_error_reports, …) НЕ затираются.
pub fn save_config(app: &AppHandle, config: &EngineConfig) {
    let path = get_config_path(app);
    save_engine_config_file(&path, config);
}

/// Расширяемая для тестов версия сохранения (по явному пути).
pub fn save_engine_config_file(path: &Path, config: &EngineConfig) {
    let mut root: serde_json::Value = fs::read_to_string(path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let engine_value = serde_json::to_value(config).unwrap_or_else(|_| serde_json::json!({}));
    if let (serde_json::Value::Object(root_map), serde_json::Value::Object(engine_map)) =
        (&mut root, engine_value)
    {
        for (k, v) in engine_map {
            root_map.insert(k, v);
        }
    }
    if let Ok(data) = serde_json::to_string_pretty(&root) {
        let _ = fs::write(path, data);
    }
}

// ─────────────────────────────── Каталог моделей ─────────────────────────────

/// Встроенный каталог моделей (SSOT для ВСЕХ проектов; файл живёт в корне
/// плагина и компилируется внутрь крейта).
const EMBEDDED_CATALOG: &str = include_str!("../../models_catalog.json");

/// Возвращает встроенный каталог моделей. Единый источник правды — файл
/// `models_catalog.json` плагина; внешние файлы-оверайды не поддерживаются.
pub fn load_catalog(_app: &AppHandle) -> Vec<CatalogEntry> {
    serde_json::from_str(EMBEDDED_CATALOG).unwrap_or_default()
}

/// Ищет запись каталога по имени установленного файла модели.
pub fn find_catalog_entry_for_model<'a>(
    catalog: &'a [CatalogEntry],
    model_path: &str,
) -> Option<&'a CatalogEntry> {
    let stem = Path::new(model_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())?;
    for entry in catalog {
        let dl_stem = entry
            .download_url
            .split('/')
            .last()
            .and_then(|s| s.split('?').next())
            .and_then(|f| Path::new(f).file_stem())
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase());
        if dl_stem.as_deref() == Some(stem.as_str()) {
            return Some(entry);
        }
        let mmp_stem = entry
            .mmproj_url
            .as_ref()
            .map(|u| {
                u.split('/')
                    .last()
                    .and_then(|s| s.split('?').next())
                    .and_then(|f| Path::new(f).file_stem())
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default()
            });
        if mmp_stem.as_deref() == Some(stem.as_str()) {
            return Some(entry);
        }
    }
    None
}

pub fn auto_detect_mmproj(model_path: &str) -> Option<String> {
    let dir = Path::new(model_path).parent()?;

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.to_lowercase().contains("mmproj") && name.ends_with(".gguf") {
                return Some(entry.path().to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Проверяет, является ли файл mmproj (мультимодальный проектор), а НЕ языковой
/// моделью. Критерии: имя содержит "mmproj" ИЛИ GGUF `general.architecture` == `clip`.
pub fn is_mmproj_file(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if name.to_lowercase().contains("mmproj") {
        return true;
    }
    if let Some(arch) = crate::engine::llm_gguf::extract_string_from_gguf(path, "general.architecture") {
        if arch.trim().eq_ignore_ascii_case("clip") {
            return true;
        }
    }
    false
}

/// Для mmproj-файла ищет «братскую» LLM (см. детальный комментарий в исходнике хоста).
pub fn find_sibling_llm_for_mmproj(model_path: &str, catalog: &[CatalogEntry]) -> Option<String> {
    let dir = Path::new(model_path).parent()?;

    if let Some(entry) = find_catalog_entry_for_model(catalog, model_path) {
        let dl_name = entry
            .download_url
            .split('/')
            .last()
            .and_then(|s| s.split('?').next());
        if let Some(dl_name) = dl_name {
            let candidate = dir.join(dl_name);
            if candidate.exists() && !is_mmproj_file(&candidate.to_string_lossy()) {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }

    let mut candidates: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |e| e == "gguf")
                && !is_mmproj_file(&p.to_string_lossy())
            {
                candidates.push(p.to_string_lossy().to_string());
            }
        }
    }
    if candidates.len() == 1 {
        return candidates.pop();
    }
    None
}
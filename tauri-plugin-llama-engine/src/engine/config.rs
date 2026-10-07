//! Конфигурация движка llama.cpp (движковые ключи `app_config.json` плагина).
//!
//! Плагин хранит свой конфиг в СОБСТВЕННОМ каталоге `%APPDATA%\llama-engine\`,
//! не привязанном к хосту. Все проекты, использующие плагин, читают и пишут
//! один `app_config.json`. Хостовые ключи (theme, projects, …) живут в
//! отдельном конфиге хоста и не пересекаются с движковыми ключами плагина.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

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

/// Имя папки данных плагина (APPDATA/llama-engine).
///
/// Плагин хранит свой конфиг в СОБСТВЕННОМ каталоге, не привязанном к хосту.
/// Все проекты, использующие плагин, читают и пишут один `app_config.json`.
///
/// `set_app_data_dir_name` оставлена как no-op для обратной совместимости:
/// существующие хосты (King Orch) вызывают её в `main()`, компиляция не ломается,
/// но значение игнорируется — плагин всегда использует `"llama-engine"`.
pub fn set_app_data_dir_name(_name: &str) {
    // Deprecated no-op: плагин владеет своим каталогом.
    // Существующие хосты продолжают компилироваться, но значение игнорируется.
}

fn app_data_dir_name() -> &'static str {
    "llama-engine"
}

/// Папка данных плагина без AppHandle (APPDATA/llama-engine).
pub fn app_data_dir_early() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(app_data_dir_name())
}

/// Каталоги-доноры для авто-миграции: старые per-app хранилища движка.
const LEGACY_HOST_DIRS: &[&str] = &["com.kingorch.app", "com.reposcontrol.app"];

/// Разовый перенос движковых ключей из старого каталога хоста, если общего
/// конфига ещё нет. Не трогает хостовые ключи (projects, theme, …): берём
/// только те поля, что описаны в `EngineConfig`, и пишем в общий файл.
fn ensure_migrated(dest: &Path) {
    if dest.exists() {
        return;
    }
    if let Some(dir) = dest.parent() {
        if let Err(err) = fs::create_dir_all(dir) {
            log::warn!("[config] не удалось создать {}: {err}", dir.display());
            return;
        }
    }
    let Some(appdata) = dest.parent().and_then(Path::parent) else {
        return;
    };
    for host in LEGACY_HOST_DIRS {
        let src = appdata.join(host).join("app_config.json");
        let Ok(data) = fs::read_to_string(&src) else {
            continue;
        };
        let Ok(legacy) = serde_json::from_str::<EngineConfig>(&data) else {
            log::warn!("[config] {} не парсится как EngineConfig, пропускаю", src.display());
            continue;
        };
        match save_engine_config_file(dest, &legacy) {
            Ok(()) => {
                log::info!("[config] миграция движка: {} → {}", src.display(), dest.display());
            }
            Err(err) => log::warn!("[config] миграция не удалась: {err}"),
        }
        return;
    }
    log::info!("[config] общий конфиг создан пустым: {}", dest.display());
}

/// Путь к `app_config.json` плагина.
///
/// Использует тот же каталог, что и `load_config_early` — `app_data_dir_early()`.
/// Раньше здесь был `app.path().app_data_dir()` (каталог хоста), из-за чего
/// `load_config` и `load_config_early` читали РАЗНЫЕ файлы.
pub fn get_config_path(_app: &AppHandle) -> PathBuf {
    let path = app_data_dir_early().join("app_config.json");
    ensure_migrated(&path);
    path
}

/// Читает конфиг ДО создания Tauri-приложения (движок, рантайм без AppHandle).
pub fn load_config_early() -> EngineConfig {
    let path = app_data_dir_early().join("app_config.json");
    ensure_migrated(&path);
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
pub fn save_config(app: &AppHandle, config: &EngineConfig) -> Result<(), String> {
    let path = get_config_path(app);
    save_engine_config_file(&path, config)
}

/// Расширяемая для тестов версия сохранения (по явному пути).
pub fn save_engine_config_file(path: &Path, config: &EngineConfig) -> Result<(), String> {
    let engine_value = serde_json::to_value(config).map_err(|e| e.to_string())?;
    ko_json_store::update_json(path, move |root: &mut serde_json::Value| {
        if !root.is_object() {
            *root = serde_json::json!({});
        }
        if let (serde_json::Value::Object(root_map), serde_json::Value::Object(engine_map)) =
            (root, engine_value)
        {
            for (k, v) in engine_map {
                root_map.insert(k, v);
            }
        }
    })
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
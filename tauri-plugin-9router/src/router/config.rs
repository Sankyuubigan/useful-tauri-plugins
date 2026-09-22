//! Конфигурация плагина 9router — вложенная секция `nine_router` файла
//! `app_config.json` хоста.
//!
//! Плагин и хост делят ОДИН файл `app_config.json`. Чтобы не затирать ни
//! хостовые ключи (theme, allow_error_reports, translator_*, …), ни движковые
//! ключи llama-engine (models, llamacpp_dir, …), плагин пишет ТОЛЬКО вложенный
//! объект `nine_router` (field-preserving merge), а читает полный JSON.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::AppHandle;
use tauri::Manager;

/// Конфиг 9router (секция `nine_router` в `app_config.json`).
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct NineRouterConfig {
    /// Порт HTTP-шлюза (дашборд + /v1). По умолчанию — канонический порт 9router.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Переопределение папки установки (по умолчанию `<exe>/9router`).
    #[serde(default)]
    pub dir: Option<String>,
    /// Последняя установленная версия 9router (тег из npm).
    #[serde(default)]
    pub installed_version: Option<String>,
    /// Версия портативного Node.js (vX.Y.Z), на котором работает шлюз.
    #[serde(default)]
    pub node_version: Option<String>,
    /// Ленивый автозапуск: поднимать сервер, когда хост выбирает комбо 9router.
    #[serde(default = "default_true")]
    pub auto_start: bool,
    /// API-ключ 9router (для `/v1/chat/completions`). Опционально: для чтения
    /// комбо через `/v1/models` ключ не требуется.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

fn default_port() -> u16 { 20128 }
fn default_true() -> bool { true }

impl NineRouterConfig {
    /// Порт из конфига или дефолт.
    pub fn port_or_default(&self) -> u16 {
        if self.port == 0 { default_port() } else { self.port }
    }

    /// Базовый URL шлюза (localhost).
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port_or_default())
    }
}

// ────────────────────────────── Путь к конфигу ──────────────────────────────

/// Имя папки данных приложения (APPDATA/<name>). Хост обязан вызвать
/// `set_app_data_dir_name` в `main()` (или задать `plugins.9router.data_dir_name`
/// в tauri.conf.json); по умолчанию — legacy-имя King Orch.
static APP_DATA_DIR_NAME: OnceLock<String> = OnceLock::new();

/// Задать имя папки данных приложения. Вызывать в начале `main()` хоста.
pub fn set_app_data_dir_name(name: &str) {
    let _ = APP_DATA_DIR_NAME.set(name.to_string());
}

fn app_data_dir_name() -> &'static str {
    APP_DATA_DIR_NAME
        .get()
        .map(|s| s.as_str())
        .unwrap_or("com.kingorch.app")
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

/// Читает секцию `nine_router` из `app_config.json`.
pub fn load_config(app: &AppHandle) -> NineRouterConfig {
    let path = get_config_path(app);
    let root: serde_json::Value = fs::read_to_string(path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    serde_json::from_value(root.get("nine_router").cloned().unwrap_or_default())
        .unwrap_or_default()
}

/// Сохраняет секцию `nine_router` в `app_config.json` field-preserving merge:
/// существующие хостовые и движковые ключи НЕ затираются.
pub fn save_config(app: &AppHandle, config: &NineRouterConfig) {
    let path = get_config_path(app);
    save_config_file(&path, config);
}

/// Расширяемая для тестов версия сохранения (по явному пути).
pub fn save_config_file(path: &PathBuf, config: &NineRouterConfig) {
    let mut root: serde_json::Value = fs::read_to_string(path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let value = serde_json::to_value(config).unwrap_or_else(|_| serde_json::json!({}));
    if let serde_json::Value::Object(root_map) = &mut root {
        root_map.insert("nine_router".to_string(), value);
    }
    if let Ok(data) = serde_json::to_string_pretty(&root) {
        let _ = fs::write(path, data);
    }
}

/// Папка установки 9router: переопределение из конфига или `<exe>/9router`.
/// Правило cwd (global_ai_docs rules.md:83): пути — через current_exe().parent(),
/// НЕ через app.path().executable_dir() (в dev-режиме это cwd запуска).
pub fn router_dir(app: &AppHandle) -> PathBuf {
    let cfg = load_config(app);
    if let Some(p) = &cfg.dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    default_router_dir()
}

/// Дефолтная папка установки относительно exe (без AppHandle).
pub fn default_router_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("9router")
}

/// Папка с распакованным standalone-билдом 9router.
pub fn dist_dir(router_dir: &std::path::Path) -> PathBuf {
    router_dir.join("dist")
}

/// Путь к портативному node.exe.
pub fn node_exe(router_dir: &std::path::Path) -> PathBuf {
    router_dir.join("runtime").join("node.exe")
}

/// Путь к серверу standalone: `dist/app/custom-server.js` (prefer) или `server.js`.
pub fn server_script(dist: &std::path::Path) -> PathBuf {
    let custom = dist.join("app").join("custom-server.js");
    if custom.exists() {
        custom
    } else {
        dist.join("app").join("server.js")
    }
}
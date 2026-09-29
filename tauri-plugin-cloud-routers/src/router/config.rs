//! Конфигурация плагина cloud-routers — вложенная секция `cloud_routers` файла
//! `app_config.json` хоста.
//!
//! Плагин и хост делят ОДИН файл `app_config.json`. Чтобы не затирать ни
//! хостовые ключи (theme, allow_error_reports, translator_*, …), ни движковые
//! ключи llama-engine (models, llamacpp_dir, …), плагин пишет ТОЛЬКО вложенный
//! объект `cloud_routers` (field-preserving merge), а читает полный JSON.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::AppHandle;
use tauri::Manager;

use crate::commands::RouterId;

/// Конфиг роутера (секция `cloud_routers.<id>` в `app_config.json`).
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct RouterConfig {
    /// Порт HTTP-шлюза (дашборд + /v1).
    #[serde(default)]
    pub port: Option<u16>,
    /// Переопределение папки установки (по умолчанию `<exe>/cloud_routers/<id>`).
    #[serde(default)]
    pub dir: Option<String>,
    /// Последняя установленная версия роутера (тег из npm).
    #[serde(default)]
    pub installed_version: Option<String>,
    /// Версия портативного Node.js (vX.Y.Z), на котором работает шлюз.
    #[serde(default)]
    pub node_version: Option<String>,
    /// Ленивый автозапуск: поднимать сервер, когда хост выбирает комбо роутера.
    #[serde(default = "default_true")]
    pub auto_start: bool,
    /// API-ключ роутера (для `/v1/chat/completions`). Опционально.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

fn default_true() -> bool { true }

impl RouterConfig {
    /// Порт из конфига или дефолт по роутеру.
    pub fn port_or_default(&self, router_id: RouterId) -> u16 {
        if let Some(p) = self.port {
            if p != 0 { return p; }
        }
        router_id.default_port()
    }

    /// Базовый URL шлюза (localhost).
    pub fn base_url(&self, router_id: RouterId) -> String {
        format!("http://127.0.0.1:{}", self.port_or_default(router_id))
    }
}

// ────────────────────────────── Путь к конфигу ──────────────────────────────

/// Имя папки данных приложения (APPDATA/<name>). Хост обязан вызвать
/// `set_app_data_dir_name` в `main()` (или задать `plugins.cloud-routers.data_dir_name`
/// в tauri.conf.json); по умолчанию — legacy-имя King Orch.
static APP_DATA_DIR_NAME: OnceLock<String> = OnceLock::new();

/// Задать имя папки данных приложения. Вызывать в начале `main()` хоста.
pub fn set_app_data_dir_name(name: &str) {
    let _ = APP_DATA_DIR_NAME.set(name.to_string());
}

/// Путь к `app_config.json` (runtime, через AppHandle).
pub fn get_config_path(app: &AppHandle) -> PathBuf {
    let base = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
    if !base.exists() {
        let _ = fs::create_dir_all(&base);
    }
    base.join("app_config.json")
}

/// Читает секцию `cloud_routers.<id>` из `app_config.json`.
pub fn load_config(app: &AppHandle, router_id: RouterId) -> RouterConfig {
    let path = get_config_path(app);
    let root: serde_json::Value = fs::read_to_string(path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let cloud = root.get("cloud_routers").cloned().unwrap_or_default();
    serde_json::from_value(cloud.get(router_id.as_str()).cloned().unwrap_or_default())
        .unwrap_or_default()
}

/// Сохраняет секцию `cloud_routers.<id>` в `app_config.json` field-preserving merge:
/// существующие хостовые и движковые ключи НЕ затираются.
pub fn save_config(app: &AppHandle, router_id: RouterId, config: &RouterConfig) -> Result<(), String> {
    let path = get_config_path(app);
    save_config_file(&path, router_id, config)
}

/// Расширяемая для тестов версия сохранения (по явному пути).
pub fn save_config_file(path: &PathBuf, router_id: RouterId, config: &RouterConfig) -> Result<(), String> {
    let value = serde_json::to_value(config).map_err(|e| e.to_string())?;
    ko_json_store::update_json(path, move |root: &mut serde_json::Value| {
        if !root.is_object() {
            *root = serde_json::json!({});
        }
        if let serde_json::Value::Object(root_map) = root {
            let cloud = root_map
                .entry("cloud_routers".to_string())
                .or_insert_with(|| serde_json::json!({}));
            if let serde_json::Value::Object(cloud_map) = cloud {
                cloud_map.insert(router_id.as_str().to_string(), value);
            }
        }
    })
}

/// Папка установки роутера: переопределение из конфига или `<exe>/cloud_routers/<id>`.
/// Правило cwd (global_ai_docs rules.md:83): пути — через current_exe().parent(),
/// НЕ через app.path().executable_dir() (в dev-режиме это cwd запуска).
pub fn router_dir(app: &AppHandle, router_id: RouterId) -> PathBuf {
    let cfg = load_config(app, router_id);
    if let Some(p) = &cfg.dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    default_router_dir(router_id)
}

/// Дефолтная папка установки относительно exe (без AppHandle).
pub fn default_router_dir(router_id: RouterId) -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("cloud_routers").join(router_id.dir_name())
}

/// Папка с распакованным standalone-билдом роутера.
pub fn dist_dir(router_dir: &std::path::Path) -> PathBuf {
    router_dir.join("dist")
}

/// Путь к портативному node.exe.
pub fn node_exe(router_dir: &std::path::Path) -> PathBuf {
    router_dir.join("runtime").join("node.exe")
}

/// Путь к серверу standalone: `dist/app/custom-server.js` (prefer) или `server.js`.
pub fn server_script(dist: &std::path::Path, router_id: RouterId) -> PathBuf {
    let rel = router_id.server_script_relative();
    dist.join(rel)
}

// ────────────────────────────── Каталог данных ──────────────────────────────

/// Путь к каталогу данных роутера: единый для всех приложений (%APPDATA%/<id>).
pub fn router_data_dir(_app: &AppHandle, router_id: RouterId) -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let data_dir = base.join(router_id.dir_name());
    if !data_dir.exists() {
        let _ = fs::create_dir_all(&data_dir);
    }
    data_dir
}

// ────────────────────────────── ServerRecord (server.json) ──────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ServerRecord {
    pub pid: u32,
    pub port: u16,
    pub data_dir: String,
    pub started_at_unix: u64,
}

pub fn server_record_path(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("server.json")
}

pub fn read_server_record(data_dir: &std::path::Path) -> Option<ServerRecord> {
    let path = server_record_path(data_dir);
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn write_server_record(data_dir: &std::path::Path, pid: u32, port: u16) {
    let path = server_record_path(data_dir);
    let record = ServerRecord {
        pid,
        port,
        data_dir: data_dir.to_string_lossy().to_string(),
        started_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&record) {
        if let Err(e) = ko_json_store::write_atomic(&path, &json) {
            log::warn!("cloud-routers: ошибка записи server.json: {}", e);
        }
    }
}

pub fn clear_server_record(data_dir: &std::path::Path) {
    let path = server_record_path(data_dir);
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

/// Миграция старой секции `nine_router` в `cloud_routers.9router`.
pub fn migrate_legacy_config(app: &AppHandle) {
    let path = get_config_path(app);
    let root: serde_json::Value = fs::read_to_string(&path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if root.get("nine_router").is_some() && root.get("cloud_routers").is_none() {
        let nine = root.get("nine_router").cloned().unwrap_or_default();
        let _ = ko_json_store::update_json(&path, move |root: &mut serde_json::Value| {
            if let serde_json::Value::Object(root_map) = root {
                let cloud = root_map
                    .entry("cloud_routers".to_string())
                    .or_insert_with(|| serde_json::json!({}));
                if let serde_json::Value::Object(cloud_map) = cloud {
                    cloud_map.insert("9router".to_string(), nine.clone());
                }
                root_map.remove("nine_router");
            }
        });
        log::info!("cloud-routers: миграция nine_router → cloud_routers.9router завершена");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ports() {
        assert_eq!(RouterId::NineRouter.default_port(), 20128);
        assert_eq!(RouterId::ExtremeRouter.default_port(), 20129);
        assert_eq!(RouterId::OmniRoute.default_port(), 20130);
    }

    #[test]
    fn test_npm_packages() {
        assert_eq!(RouterId::NineRouter.npm_package(), "9router");
        assert_eq!(RouterId::ExtremeRouter.npm_package(), "@rsalmn/extremerouter");
        assert_eq!(RouterId::OmniRoute.npm_package(), "omniroute");
    }

    #[test]
    fn test_server_script_paths() {
        let dist = PathBuf::from("D:\\test\\dist");
        assert_eq!(server_script(&dist, RouterId::NineRouter), dist.join("app/custom-server.js"));
        assert_eq!(server_script(&dist, RouterId::ExtremeRouter), dist.join("app/custom-server.js"));
        assert_eq!(server_script(&dist, RouterId::OmniRoute), dist.join("dist/server.js"));
    }

    #[test]
    fn test_server_record() {
        let tmp = std::env::temp_dir().join("cloud_routers_test_server_record");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        write_server_record(&tmp, 1234, 20128);
        let rec = read_server_record(&tmp).unwrap();
        assert_eq!(rec.pid, 1234);
        assert_eq!(rec.port, 20128);
        clear_server_record(&tmp);
        assert!(read_server_record(&tmp).is_none());

        let _ = fs::remove_dir_all(&tmp);
    }
}
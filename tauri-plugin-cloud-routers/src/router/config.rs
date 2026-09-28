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

// ────────────────────────────── Каталог данных и миграция ──────────────────────────────

/// Папка данных роутера относительно каталога данных приложения (<app_data>/cloud_routers/<id>).
pub fn data_dir_under(app_data: &std::path::Path, router_id: RouterId) -> PathBuf {
    app_data.join("cloud_routers").join(router_id.dir_name())
}

/// Путь к каталогу данных роутера для текущего AppHandle (<app_data_dir>/cloud_routers/<id>).
pub fn router_data_dir(app: &AppHandle, router_id: RouterId) -> PathBuf {
    let base = app.path().app_data_dir().unwrap_or_else(|_| app_data_dir_early());
    let data_dir = data_dir_under(&base, router_id);
    if !data_dir.exists() {
        let _ = fs::create_dir_all(&data_dir);
    }
    data_dir
}

/// Вычисляет legacy-каталог данных роутера (старый %APPDATA%/9router без привязки к app_data).
pub fn legacy_dir_under(app_data: &std::path::Path, router_id: RouterId) -> Option<PathBuf> {
    let parent = app_data.parent()?;
    let legacy = parent.join(router_id.dir_name());
    if legacy == app_data || legacy == data_dir_under(app_data, router_id) {
        return None;
    }
    let backup_db = legacy.join("db-copy-backup").join("data.sqlite");
    let main_db = legacy.join("db").join("data.sqlite");
    if backup_db.exists() || main_db.exists() {
        Some(legacy)
    } else {
        None
    }
}

/// Рекурсивное копирование каталога (игнорирует отсутствие исходного пути).
pub fn copy_dir_allow_missing(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<u64> {
    if !src.exists() {
        return Ok(0);
    }
    let mut total_bytes = 0u64;
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if entry_type.is_dir() {
            total_bytes += copy_dir_allow_missing(&src_path, &dst_path)?;
        } else if entry_type.is_file() {
            total_bytes += fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(total_bytes)
}

/// Выполняет одноразовую миграцию данных из src в dst.
pub fn migrate_into(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<bool> {
    let marker = dst.join(".migrated-from-legacy");
    if marker.exists() || dst.join("db").join("data.sqlite").exists() {
        return Ok(false);
    }

    let backup_db = src.join("db-copy-backup").join("data.sqlite");
    let main_db = src.join("db").join("data.sqlite");
    if !backup_db.exists() && !main_db.exists() {
        return Ok(false);
    }

    fs::create_dir_all(dst)?;

    let lock = dst.join(".migrate.lock");
    if fs::OpenOptions::new().write(true).create_new(true).open(&lock).is_err() {
        log::warn!("cloud-routers: файл блокировки миграции уже существует, пропуск");
        return Ok(false);
    }
    let mut bytes_copied = 0u64;

    let dst_db_dir = dst.join("db");
    fs::create_dir_all(&dst_db_dir)?;

    let src_db_dir = if backup_db.exists() {
        src.join("db-copy-backup")
    } else {
        src.join("db")
    };
    bytes_copied += copy_dir_allow_missing(&src_db_dir, &dst_db_dir)?;

    bytes_copied += copy_dir_allow_missing(&src.join("auth"), &dst.join("auth"))?;
    bytes_copied += copy_dir_allow_missing(&src.join("runtime"), &dst.join("runtime"))?;

    for secret_file in &["jwt-secret", "machine-id"] {
        let f_src = src.join(secret_file);
        let f_dst = dst.join(secret_file);
        if f_src.exists() && !f_dst.exists() {
            if let Ok(b) = fs::copy(&f_src, &f_dst) {
                bytes_copied += b;
            }
        }
    }

    let _ = fs::remove_file(&lock);
    let _ = fs::write(&marker, src.to_string_lossy().as_bytes());
    log::info!(
        "cloud-routers: данные перенесены из {} → {} (всего {} байт)",
        src.display(),
        dst.display(),
        bytes_copied
    );
    Ok(true)
}

/// Авто-миграция legacy данных для AppHandle.
pub fn migrate_legacy_data_dir(app: &AppHandle, router_id: RouterId) {
    let base = app.path().app_data_dir().unwrap_or_else(|_| app_data_dir_early());
    let dst = data_dir_under(&base, router_id);
    if let Some(src) = legacy_dir_under(&base, router_id) {
        match migrate_into(&src, &dst) {
            Ok(true) => log::info!("cloud-routers: миграция legacy-данных завершена"),
            Ok(false) => {}
            Err(e) => log::warn!("cloud-routers: ошибка миграции legacy-данных: {}", e),
        }
    }
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
    fn test_data_dir_under() {
        let base = PathBuf::from("C:\\Roaming\\App");
        let d = data_dir_under(&base, RouterId::NineRouter);
        assert_eq!(d, PathBuf::from("C:\\Roaming\\App\\cloud_routers\\9router"));
    }

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
    fn test_migrate_into_and_server_record() {
        let tmp = std::env::temp_dir().join("cloud_routers_test_migrate");
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("legacy");
        let dst = tmp.join("new");

        let src_db = src.join("db");
        let _ = fs::create_dir_all(&src_db);
        let _ = fs::write(src_db.join("data.sqlite"), "sqlite-data");
        let _ = fs::write(src.join("jwt-secret"), "secret123");

        let migrated = migrate_into(&src, &dst).unwrap();
        assert!(migrated);
        assert!(dst.join("db").join("data.sqlite").exists());
        assert_eq!(fs::read_to_string(dst.join("jwt-secret")).unwrap(), "secret123");
        assert!(dst.join(".migrated-from-legacy").exists());

        let migrated_again = migrate_into(&src, &dst).unwrap();
        assert!(!migrated_again);

        write_server_record(&dst, 1234, 20128);
        let rec = read_server_record(&dst).unwrap();
        assert_eq!(rec.pid, 1234);
        assert_eq!(rec.port, 20128);
        clear_server_record(&dst);
        assert!(read_server_record(&dst).is_none());

        let _ = fs::remove_dir_all(&tmp);
    }
}
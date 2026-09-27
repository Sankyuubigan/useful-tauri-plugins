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
pub fn save_config(app: &AppHandle, config: &NineRouterConfig) -> Result<(), String> {
    let path = get_config_path(app);
    save_config_file(&path, config)
}

/// Расширяемая для тестов версия сохранения (по явному пути).
pub fn save_config_file(path: &PathBuf, config: &NineRouterConfig) -> Result<(), String> {
    let value = serde_json::to_value(config).map_err(|e| e.to_string())?;
    ko_json_store::update_json(path, move |root: &mut serde_json::Value| {
        if !root.is_object() {
            *root = serde_json::json!({});
        }
        if let serde_json::Value::Object(root_map) = root {
            root_map.insert("nine_router".to_string(), value);
        }
    })
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

// ────────────────────────────── Каталог данных и миграция ──────────────────────────────

/// Папка данных 9router относительно каталога данных приложения (<app_data>/9router).
pub fn data_dir_under(app_data: &std::path::Path) -> PathBuf {
    app_data.join("9router")
}

/// Путь к каталогу данных 9router для текущего AppHandle (<app_data_dir>/9router).
pub fn router_data_dir(app: &AppHandle) -> PathBuf {
    let base = app.path().app_data_dir().unwrap_or_else(|_| app_data_dir_early());
    let data_dir = data_dir_under(&base);
    if !data_dir.exists() {
        let _ = fs::create_dir_all(&data_dir);
    }
    data_dir
}

/// Вычисляет legacy-каталог данных 9router (старый %APPDATA%/9router без привязки к app_data).
pub fn legacy_dir_under(app_data: &std::path::Path) -> Option<PathBuf> {
    let parent = app_data.parent()?;
    let legacy = parent.join("9router");
    if legacy == app_data || legacy == data_dir_under(app_data) {
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
        log::warn!("9router: файл блокировки миграции уже существует, пропуск");
        return Ok(false);
    }
    let mut bytes_copied = 0u64;

    // 1. База данных (из db-copy-backup или db)
    let dst_db_dir = dst.join("db");
    fs::create_dir_all(&dst_db_dir)?;

    let src_db_dir = if backup_db.exists() {
        src.join("db-copy-backup")
    } else {
        src.join("db")
    };
    bytes_copied += copy_dir_allow_missing(&src_db_dir, &dst_db_dir)?;

    // 2. Дополнительные папки и файлы идентичности
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
        "9router: данные перенесены из {} → {} (всего {} байт)",
        src.display(),
        dst.display(),
        bytes_copied
    );
    Ok(true)
}

/// Авто-миграция legacy данных для AppHandle.
pub fn migrate_legacy_data_dir(app: &AppHandle) {
    let base = app.path().app_data_dir().unwrap_or_else(|_| app_data_dir_early());
    let dst = data_dir_under(&base);
    if let Some(src) = legacy_dir_under(&base) {
        match migrate_into(&src, &dst) {
            Ok(true) => log::info!("9router: миграция legacy-данных завершена"),
            Ok(false) => {}
            Err(e) => log::warn!("9router: ошибка миграции legacy-данных: {}", e),
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
            log::warn!("9router: ошибка записи server.json: {}", e);
        }
    }
}

pub fn clear_server_record(data_dir: &std::path::Path) {
    let path = server_record_path(data_dir);
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_dir_under() {
        let base = PathBuf::from("C:\\Roaming\\App");
        let d = data_dir_under(&base);
        assert_eq!(d, PathBuf::from("C:\\Roaming\\App\\9router"));
    }

    #[test]
    fn test_legacy_dir_under() {
        let tmp = std::env::temp_dir().join("9router_test_legacy");
        let _ = fs::create_dir_all(&tmp);
        let app_data = tmp.join("com.kingorch.app");
        let legacy = tmp.join("9router");
        let db_dir = legacy.join("db");
        let _ = fs::create_dir_all(&db_dir);
        let _ = fs::write(db_dir.join("data.sqlite"), "test");

        let found = legacy_dir_under(&app_data);
        assert_eq!(found, Some(legacy));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_migrate_into_and_server_record() {
        let tmp = std::env::temp_dir().join("9router_test_migrate");
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("legacy");
        let dst = tmp.join("new");

        let src_db = src.join("db");
        let _ = fs::create_dir_all(&src_db);
        let _ = fs::write(src_db.join("data.sqlite"), "sqlite-data");
        let _ = fs::write(src.join("jwt-secret"), "secret123");

        // Migrate
        let migrated = migrate_into(&src, &dst).unwrap();
        assert!(migrated);
        assert!(dst.join("db").join("data.sqlite").exists());
        assert_eq!(fs::read_to_string(dst.join("jwt-secret")).unwrap(), "secret123");
        assert!(dst.join(".migrated-from-legacy").exists());

        // Second call -> no-op
        let migrated_again = migrate_into(&src, &dst).unwrap();
        assert!(!migrated_again);

        // Server record roundtrip
        write_server_record(&dst, 1234, 20128);
        let rec = read_server_record(&dst).unwrap();
        assert_eq!(rec.pid, 1234);
        assert_eq!(rec.port, 20128);
        clear_server_record(&dst);
        assert!(read_server_record(&dst).is_none());

        let _ = fs::remove_dir_all(&tmp);
    }
}
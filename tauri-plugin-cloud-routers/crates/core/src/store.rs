//! Хранение конфигурации шлюза и идентичности процесса.
//!
//! ## Раскладка в каталоге данных
//!
//! ```text
//! <data_dir>/
//!   gateway.json          конфигурация (провайдеры, ключи, модели, релей)
//!   presets_cache.json    кэш каталога провайдеров (см. [`crate::presets`])
//!   server.json           pid/порт/версия живого шлюза
//!   gateway.log           stdout+stderr шлюза
//! ```
//!
//! Все записи — через `ko-json-store::write_atomic` (tmp → fsync → rename),
//! как требует `PLUGIN_STANDARD §2`: читатель никогда не увидит обрезанный JSON.
//! Собственный lock-файл здесь не нужен — файл принадлежит одному процессу
//! (шлюзу), читателей из хоста много, но они только читают.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::GatewayConfig;

/// Каталог данных роутера `gateway` внутри app-data хоста.
///
/// Хост передаёт свой `app_data_dir` (через `plugins.cloud-routers.data_dir_name`
/// либо значение по умолчанию), плагин добавляет подпапку роутера. Так каталоги
/// разных хостов, подключивших один и тот же плагин, не сшибаются.
pub fn resolve_data_dir(app_data_dir: &Path, dir_name: &str) -> PathBuf {
    app_data_dir.join("cloud_routers").join(dir_name)
}

/// Путь к файлу конфигурации.
pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("gateway.json")
}

/// Путь к кэшу каталога провайдеров.
pub fn presets_cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("presets_cache.json")
}

/// Прочитать конфигурацию.
///
/// Битый/отсутствующий файл **не является ошибкой**: возвращается конфиг по
/// умолчанию, в лог пишется предупреждение. Смысл: недовольный пользователь
/// должен увидеть пустой список провайдеров и добавить их заново, а не
/// получить падение шлюза на старте из-за одного лишнего пробела в JSON.
pub fn load(data_dir: &Path) -> GatewayConfig {
    let path = config_path(data_dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return GatewayConfig::default();
    };
    match serde_json::from_str::<GatewayConfig>(&raw) {
        Ok(cfg) => cfg,
        Err(e) => {
            log::warn!(
                "cloud-routers: {} не разобран ({}), используется конфиг по умолчанию",
                path.display(),
                e
            );
            GatewayConfig::default()
        }
    }
}

/// Сохранить конфигурацию атомарно.
pub fn save(data_dir: &Path, cfg: &GatewayConfig) -> Result<(), String> {
    let problems = cfg.validate();
    if !problems.is_empty() {
        return Err(format!("Конфигурация не прошла проверку: {}", problems.join("; ")));
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    ko_json_store::write_atomic(&config_path(data_dir), &json).map_err(|e| {
        format!("Не удалось записать {}: {}", config_path(data_dir).display(), e)
    })
}

/// Запись о живом процессе шлюза (`server.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerRecord {
    /// PID процесса шлюза.
    pub pid: u32,
    /// Порт, который он слушает.
    pub port: u16,
    /// Версия бинаря шлюза (для честного предупреждения «работает старая версия»).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Каталог данных, из которого запущен (диагностика при рассинхроне).
    #[serde(default)]
    pub data_dir: String,
    /// Unix-время старта.
    #[serde(default)]
    pub started_at_unix: u64,
}

pub fn server_record_path(data_dir: &Path) -> PathBuf {
    data_dir.join("server.json")
}

/// Прочитать запись о процессе. Отсутствие/битое значение — `None`, не ошибка.
pub fn read_server_record(data_dir: &Path) -> Option<ServerRecord> {
    let raw = std::fs::read_to_string(server_record_path(data_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Записать запись о процессе.
pub fn write_server_record(data_dir: &Path, pid: u32, port: u16, version: Option<String>) {
    let record = ServerRecord {
        pid,
        port,
        version,
        data_dir: data_dir.to_string_lossy().to_string(),
        started_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default(),
    };
    match serde_json::to_string_pretty(&record) {
        Ok(json) => {
            if let Err(e) = ko_json_store::write_atomic(&server_record_path(data_dir), &json) {
                log::warn!("cloud-routers: не удалось записать server.json: {}", e);
            }
        }
        Err(e) => log::warn!("cloud-routers: server.json не сериализуется: {}", e),
    }
}

/// Удалить запись о процессе (шлюз остановлен).
pub fn clear_server_record(data_dir: &Path) {
    let p = server_record_path(data_dir);
    if p.exists() {
        if let Err(e) = std::fs::remove_file(&p) {
            log::warn!("cloud-routers: не удалось удалить server.json: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AccountKey, DiscoveredModel, ProviderConfig};

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cr_core_store_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("create tmp");
        d
    }

    fn sample() -> GatewayConfig {
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(ProviderConfig {
            id: "groq".into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            is_enabled: true,
            use_relay: false,
            keys: vec![AccountKey { id: "k1".into(), key: "gsk_test".into(), is_active: true }],
            models: vec![DiscoveredModel { id: "llama-3.3-70b-versatile".into(), ..Default::default() }],
        });
        cfg
    }

    #[test]
    fn resolve_data_dir_nests_under_cloud_routers() {
        let p = resolve_data_dir(Path::new("/appdata/com.example.app"), "gateway");
        assert!(p.ends_with("cloud_routers/gateway"), "{}", p.display());
    }

    #[test]
    fn load_returns_default_when_file_absent() {
        let d = tmp_dir("absent");
        let cfg = load(&d);
        assert_eq!(cfg.port, crate::config::DEFAULT_PORT);
        assert!(cfg.providers.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn load_returns_default_and_survives_corrupt_file() {
        let d = tmp_dir("corrupt");
        std::fs::write(config_path(&d), "{ this is not json").expect("write");
        let cfg = load(&d);
        assert_eq!(cfg.port, crate::config::DEFAULT_PORT);
        // Файл НЕ перетирается: пользователь может починить его руками.
        assert!(config_path(&d).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let d = tmp_dir("roundtrip");
        let cfg = sample();
        save(&d, &cfg).expect("save");
        let back = load(&d);
        assert_eq!(back.port, cfg.port);
        assert_eq!(back.providers.len(), 1);
        assert_eq!(back.providers[0].id, "groq");
        assert_eq!(back.providers[0].keys[0].key, "gsk_test");
        assert_eq!(back.providers[0].models[0].id, "llama-3.3-70b-versatile");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn save_rejects_invalid_config_and_does_not_write() {
        let d = tmp_dir("invalid");
        let mut cfg = sample();
        cfg.providers[0].id = "bad/id".into();
        let err = save(&d, &cfg).expect_err("must reject");
        assert!(err.contains("разделитель"), "{err}");
        assert!(!config_path(&d).exists(), "невалидный конфиг не должен попасть на диск");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn save_leaves_no_tmp_files_behind() {
        let d = tmp_dir("notmp");
        save(&d, &sample()).expect("save");
        let leftovers: Vec<String> = std::fs::read_dir(&d)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "остались tmp-файлы: {:?}", leftovers);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn server_record_roundtrip_includes_version() {
        let d = tmp_dir("server");
        assert!(read_server_record(&d).is_none());
        write_server_record(&d, 4242, 20131, Some("0.1.0".into()));
        let rec = read_server_record(&d).expect("record");
        assert_eq!(rec.pid, 4242);
        assert_eq!(rec.port, 20131);
        assert_eq!(rec.version.as_deref(), Some("0.1.0"));
        assert!(!rec.data_dir.is_empty());
        assert!(rec.started_at_unix > 0);
        clear_server_record(&d);
        assert!(read_server_record(&d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn server_record_tolerates_old_shape_without_version() {
        // Запись, написанная прошлой версией шлюза, не должна ломать чтение.
        let d = tmp_dir("old");
        std::fs::write(server_record_path(&d), r#"{"pid":7,"port":20131}"#).expect("write");
        let rec = read_server_record(&d).expect("record");
        assert_eq!(rec.pid, 7);
        assert_eq!(rec.version, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn clear_server_record_is_idempotent() {
        let d = tmp_dir("clear");
        clear_server_record(&d); // файла нет — не паника
        clear_server_record(&d);
        let _ = std::fs::remove_dir_all(&d);
    }
}
//! Хранение конфигурации шлюза и идентичности процесса.
//!
//! ## Раскладка в каталоге данных
//!
//! ```text
//! <data_dir>/
//!   gateway.json          конфигурация (провайдеры, ключи, модели, релей)
//!   presets_cache.json    кэш каталога провайдеров (см. [`crate::presets`])
//!   oauth_sessions.json   OAuth-сессии (токены; см. [`crate::protocol`])
//!   server.json           pid/порт/версия живого процесса
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

/// Путь к OAuth-сессиям (`session_id → OAuthSession`, см. [`crate::protocol`]).
///
/// Отдельный файл, а не поле `gateway.json`: конфиг целиком отдаётся в
/// дашборд (с масками), а токены не должны покидать процесс шлюза.
pub fn oauth_sessions_path(data_dir: &Path) -> PathBuf {
    data_dir.join("oauth_sessions.json")
}

/// Прочитать OAuth-сессии. Отсутствие файла — пустая карта, битый файл —
/// пустая карта с `warn!` (тот же договор, что у конфига: не ронять шлюз).
pub fn load_oauth_sessions(
    data_dir: &Path,
) -> std::collections::BTreeMap<String, crate::protocol::OAuthSession> {
    let path = oauth_sessions_path(data_dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return std::collections::BTreeMap::new();
    };
    match serde_json::from_str(&raw) {
        Ok(map) => map,
        Err(e) => {
            log::warn!(
                "cloud-routers: {} не разобран ({}), сессии считаются отсутствующими",
                path.display(),
                e
            );
            std::collections::BTreeMap::new()
        }
    }
}

/// Сохранить OAuth-сессии атомарно.
pub fn save_oauth_sessions(
    data_dir: &Path,
    sessions: &std::collections::BTreeMap<String, crate::protocol::OAuthSession>,
) -> Result<(), String> {
    let json = serde_json::to_string_pretty(sessions).map_err(|e| e.to_string())?;
    ko_json_store::write_atomic(&oauth_sessions_path(data_dir), &json)
        .map_err(|e| format!("Не удалось записать {}: {}", oauth_sessions_path(data_dir).display(), e))
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
        Ok(mut cfg) => {
            migrate_legacy_presets_sync_url(&mut cfg, &path);
            cfg
        }
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

/// Миграция `presets_sync_url` (строка) → `presets_sources` (список).
///
/// Старое поле после замены попадает в `extra` (flatten) — здесь оно
/// разбирается один раз при загрузке:
///
/// - пустое → молча удаляется (поле заменено, значения не было);
/// - равно мёртвому [`crate::presets::LEGACY_SYNC_URL`] (404 с 2026-10) →
///   удаляется, используются источники по умолчанию;
/// - чужое (пользовательское) → становится первым элементом `presets_sources`,
///   если список пуст; если список уже задан — старое игнорируется с `warn!`
///   (два хозяина у одного смысла недопустимы).
fn migrate_legacy_presets_sync_url(cfg: &mut GatewayConfig, path: &Path) {
    let Some(legacy) = cfg.extra.remove("presets_sync_url") else { return };
    let url = legacy.as_str().map(str::trim).unwrap_or_default().to_string();
    if url.is_empty() {
        log::info!(
            "cloud-routers: {}: пустой presets_sync_url удалён (поле заменено presets_sources)",
            path.display()
        );
        return;
    }
    if url == crate::presets::LEGACY_SYNC_URL {
        log::info!(
            "cloud-routers: {}: мёртвый presets_sync_url удалён, используются источники по умолчанию",
            path.display()
        );
        return;
    }
    if cfg.presets_sources.is_empty() {
        log::info!(
            "cloud-routers: {}: presets_sync_url перенесён в presets_sources первым элементом",
            path.display()
        );
        cfg.presets_sources.push(url);
    } else {
        log::warn!(
            "cloud-routers: {}: presets_sync_url «{}» проигнорирован — задан presets_sources",
            path.display(),
            url
        );
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
            keys: vec![AccountKey { id: "k1".into(), key: "gsk_test".into(), is_active: true, name: None }],
            models: vec![DiscoveredModel { id: "llama-3.3-70b-versatile".into(), ..Default::default() }],
            ..Default::default()
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

    fn write_raw(d: &std::path::Path, raw: &str) {
        std::fs::write(config_path(d), raw).expect("write");
    }

    #[test]
    fn migration_moves_custom_legacy_url_first() {
        let d = tmp_dir("mig1");
        write_raw(&d, r#"{"port":20131,"providers":[],"presets_sync_url":"https://example.com/mine.json"}"#);
        let cfg = load(&d);
        assert_eq!(cfg.presets_sources, vec!["https://example.com/mine.json".to_string()]);
        assert!(!cfg.extra.contains_key("presets_sync_url"), "legacy-поле обязано исчезнуть");
        // Повторная загрузка идемпотентна: поле уже в списке, миграции нечего делать.
        let cfg2 = load(&d);
        assert_eq!(cfg2.presets_sources, cfg.presets_sources);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn migration_drops_dead_legacy_url() {
        let d = tmp_dir("mig2");
        write_raw(
            &d,
            &format!(
                "{{\"port\":20131,\"providers\":[],\"presets_sync_url\":\"{}\"}}",
                crate::presets::LEGACY_SYNC_URL
            ),
        );
        let cfg = load(&d);
        assert!(cfg.presets_sources.is_empty(), "мёртвый URL не должен попасть в список");
        assert_eq!(
            cfg.effective_presets_sources()[0],
            crate::presets::sources::MODELS_DEV_API,
            "пустой список = дефолтные источники"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn migration_prefers_explicit_sources_list() {
        let d = tmp_dir("mig3");
        write_raw(
            &d,
            r#"{"port":20131,"providers":[],"presets_sources":["https://example.com/a.json"],"presets_sync_url":"https://example.com/old.json"}"#,
        );
        let cfg = load(&d);
        assert_eq!(cfg.presets_sources, vec!["https://example.com/a.json".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn effective_sources_default_when_empty() {
        let cfg = GatewayConfig::default();
        let eff = cfg.effective_presets_sources();
        assert_eq!(eff.len(), crate::presets::sources::DEFAULT_SOURCES.len());
        assert_eq!(eff[0], crate::presets::sources::MODELS_DEV_API);
    }

    fn sample_session() -> crate::protocol::OAuthSession {
        crate::protocol::OAuthSession {
            provider_id: "kiro".into(),
            access_token: "tok".into(),
            ..Default::default()
        }
    }

    #[test]
    fn oauth_sessions_roundtrip() {
        let d = tmp_dir("oauth1");
        std::fs::create_dir_all(&d).expect("mkdir");
        let mut map = std::collections::BTreeMap::new();
        map.insert("s1".to_string(), sample_session());
        save_oauth_sessions(&d, &map).expect("save");
        let back = load_oauth_sessions(&d);
        assert_eq!(back.get("s1").map(|s| s.access_token.as_str()), Some("tok"));
        assert!(oauth_sessions_path(&d).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn oauth_sessions_missing_file_is_empty_not_error() {
        let d = tmp_dir("oauth2");
        std::fs::create_dir_all(&d).expect("mkdir");
        assert!(load_oauth_sessions(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn oauth_sessions_corrupt_file_is_empty_and_kept() {
        let d = tmp_dir("oauth3");
        std::fs::create_dir_all(&d).expect("mkdir");
        std::fs::write(oauth_sessions_path(&d), "{oops").expect("write");
        assert!(load_oauth_sessions(&d).is_empty(), "битый файл — не падение");
        assert!(oauth_sessions_path(&d).exists(), "битый файл не перетираем");
        let _ = std::fs::remove_dir_all(&d);
    }
}
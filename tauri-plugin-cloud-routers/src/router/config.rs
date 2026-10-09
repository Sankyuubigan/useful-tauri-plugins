//! Конфигурация плагина cloud-routers — вложенная секция `cloud_routers` файла
//! `app_config.json` САМОГО ПЛАГИНА (`APPDATA/cloud-routers/app_config.json`).
//!
//! ## Почему не в файле хоста
//!
//! Раньше плагин писал секцию `cloud_routers` в `app_config.json` хоста. Этот файл
//! лежит в per-app каталоге (`app_data_dir()` по `identifier` хоста), то есть у
//! каждого проекта он свой. Следствие: `cloud_routers.<id>.dir` — папка с
//! бинарём шлюза — тоже своя у каждого проекта, хотя сам процесс шлюза один на
//! машину. Второй проект не видел бы «свой» бинарь, не смог бы остановить
//! общий процесс и показал бы чужие комбо как недоступные.
//!
//! Теперь конфиг принадлежит плагину: один файл на все проекты — как у
//! `llama-engine` (`engine/config.rs:119-134`). Хостовые ключи (theme,
//! projects, …) и движковые (models, llamacpp_dir, …) остаются у их владельцев;
//! при первом чтении настройки роутеров переносятся из старого per-app файла
//! хоста (см. `ensure_migrated`), после чего плагин пишет только свой файл
//! (field-preserving merge).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

use crate::commands::RouterId;

/// Конфиг роутера (секция `cloud_routers.<id>` в `app_config.json`).
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct RouterConfig {
    /// Порт HTTP-шлюза (дашборд + /v1).
    #[serde(default)]
    pub port: Option<u16>,
    /// Переопределение папки установки (по умолчанию `APPDATA/cloud-routers/<id>`).
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

/// Имя папки данных плагина (`APPDATA/cloud-routers`).
///
/// Плагин хранит свой конфиг в СОБСТВЕННОМ каталоге, не привязанном к хосту:
/// все проекты, подключившие плагин, читают и пишут один и тот же файл. Иначе
/// `cloud_routers.<id>.dir` жил бы в per-app каталоге (`app_data_dir()` хоста —
/// это папка по его `identifier`), и второй проект увидел бы другой путь к
/// бинарю шлюза: процесс один, а «свой» у каждого хоста получается свой.
const APP_DATA_DIR_NAME: &str = "cloud-routers";

/// Задать имя папки данных приложения. No-op — как у `llama-engine`
/// (`engine/config.rs:127`): хост может звать это в `main()`, компиляция не
/// ломается, но значение игнорируется — плагин всегда использует
/// [`APP_DATA_DIR_NAME`].
pub fn set_app_data_dir_name(_name: &str) {}

/// Каталоги-доноры для авто-миграции: старые per-app `app_config.json` хостов.
/// Переносятся только ключи из [`MIGRATED_KEYS`] — хостовые ключи (theme,
/// projects, …) остаются у хоста.
const LEGACY_HOST_DIRS: &[&str] = &["com.kingorch.app", "com.reposcontrol.app"];

/// Ключи, которые переносятся из старого per-app конфига хоста.
const MIGRATED_KEYS: &[&str] = &["cloud_routers", "nine_router"];

/// Каталог данных плагина без `AppHandle` (`APPDATA/cloud-routers`).
pub fn app_data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_DATA_DIR_NAME)
}

/// Разовый перенос настроек из старого per-app конфига хоста, если общего файла
/// ещё нет. Существующий файл НЕ переписывается: иначе повторный запуск
/// плагина затирал бы путь, который пользователь задал в панели.
fn ensure_migrated(dest: &Path) {
    if dest.exists() {
        return;
    }
    if let Some(dir) = dest.parent() {
        if let Err(e) = fs::create_dir_all(dir) {
            log::warn!(
                "cloud-routers: не удалось создать {}: {}",
                dir.display(),
                e
            );
            return;
        }
    }
    let Some(appdata) = dest.parent().and_then(Path::parent) else {
        return;
    };
    for host in LEGACY_HOST_DIRS {
        let src = appdata.join(host).join("app_config.json");
        let Ok(raw) = fs::read_to_string(&src) else {
            continue;
        };
        let Ok(legacy) = serde_json::from_str::<serde_json::Value>(&raw) else {
            log::warn!(
                "cloud-routers: {} не разобран — настройки роутеров из него не перенесены",
                src.display()
            );
            continue;
        };
        let mut picked = serde_json::Map::new();
        for key in MIGRATED_KEYS {
            if let Some(v) = legacy.get(*key) {
                picked.insert((*key).to_string(), v.clone());
            }
        }
        if picked.is_empty() {
            continue;
        }
        let value = serde_json::Value::Object(picked);
        let json = match serde_json::to_string_pretty(&value) {
            Ok(json) => json,
            Err(e) => {
                log::warn!("cloud-routers: перенос настроек не удался: {}", e);
                return;
            }
        };
        if let Err(e) = ko_json_store::write_atomic(dest, &json) {
            log::warn!("cloud-routers: перенос настроек не удался: {}", e);
            return;
        }
        log::info!(
            "cloud-routers: настройки роутеров перенесены {} → {}",
            src.display(),
            dest.display()
        );
        return;
    }
    log::info!(
        "cloud-routers: общий конфиг создан пустым: {}",
        dest.display()
    );
}

/// Путь к `app_config.json` плагина (`APPDATA/cloud-routers/app_config.json`).
pub fn get_config_path(_app: &AppHandle) -> PathBuf {
    let path = app_data_dir().join("app_config.json");
    ensure_migrated(&path);
    path
}

/// Прочитать `app_config.json` целиком.
///
/// Ошибка чтения/разбора НЕ молчит: без `warn!` «конфиг сломан» и «конфига нет»
/// выглядели бы одинаково, и пользователь получал бы дефолтные значения, решив,
/// что настройки сохранены (core rules §2.2 «запрет лжи»). Отсутствующий файл —
/// не ошибка, его создадут позже.
fn load_root(app: &AppHandle) -> serde_json::Value {
    let path = get_config_path(app);
    let Ok(raw) = fs::read_to_string(&path) else {
        return serde_json::json!({});
    };
    serde_json::from_str(&raw).unwrap_or_else(|e| {
        log::warn!(
            "cloud-routers: {} не разобран ({}). Роутеры берут значения по умолчанию, \
             ваши настройки из этого файла сейчас НЕ применяются.",
            path.display(),
            e
        );
        serde_json::json!({})
    })
}

/// Читает секцию `cloud_routers.<id>` из `app_config.json`.
pub fn load_config(app: &AppHandle, router_id: RouterId) -> RouterConfig {
    let root = load_root(app);
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

/// Папка установки роутера из уже прочитанных значений (без `AppHandle`).
///
/// ## Приоритет
///
/// 1. `cloud_routers.<id>.dir` из конфига ПЛАГИНА — путь, выбранный
///    пользователем в панели. Выше всего: явное намерение человека сильнее
///    умолчания.
/// 2. `APPDATA/cloud-routers/<id>` — каталог самого плагина. Один на все
///    проекты: бинарь шлюза и node-роутеры лежат в общем месте, а не внутри
///    папки каждого приложения. Привязка к хосту (`<exe>/cloud_routers`,
///    назначение пути из конфига хоста) убрана намеренно — см. [`APP_DATA_DIR_NAME`].
///
/// ## Почему отдельная функция
///
/// [`router_dir`] требует `AppHandle`, который в юнит-тесте не построить.
/// Решение вынесено в чистую функцию, иначе приоритет — самое важное в
/// разрешении пути — остался бы непроверяемым: компилятор не поймал бы перестав-
/// ленный уровень, а ошибка всплыла бы у пользователя как «роутер поставился
/// не туда».
pub fn choose_router_dir(dir_override: Option<&str>, router_id: RouterId) -> PathBuf {
    if let Some(p) = dir_override.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(p);
    }
    default_router_dir(router_id)
}

/// Папка установки роутера.
pub fn router_dir(app: &AppHandle, router_id: RouterId) -> PathBuf {
    let cfg = load_config(app, router_id);
    choose_router_dir(cfg.dir.as_deref(), router_id)
}

/// Умолчательная папка установки: подкаталог каталога плагина, общий для всех
/// проектов (`APPDATA/cloud-routers/<id>`).
///
/// Путь НЕ зависит от хоста намеренно: процесс шлюза один на машину, и папка
/// с его бинарём обязана быть одна и та же из любого приложения — иначе
/// сверка «свой процесс / чужой» в `gateway_process::stop_server` отказала бы
/// работать у второго проекта.
pub fn default_router_dir(router_id: RouterId) -> PathBuf {
    app_data_dir().join(router_id.dir_name())
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
///
/// Шлюз сюда не попадает: у него нет ни `dist`, ни серверного скрипта. Функция
/// для него вернёт пустой относительный путь, поэтому вызывать её для шлюза
/// нельзя — в `commands::build_status` ветвление по виду роутера идёт раньше.
pub fn server_script(dist: &std::path::Path, router_id: RouterId) -> PathBuf {
    let rel = router_id.server_script_relative().unwrap_or_default();
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
    /// Версия бинаря шлюза.
    ///
    /// Нужна, чтобы отличать «работает шлюз из текущей сборки» от «остался
    /// процесс от прошлой версии плагина». Без поля хост принимал бы старое
    /// поведение за актуальное, а пользователь увидел бы расхождение только
    /// по косвенным признакам.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Каталог данных, из которого запущен (диагностика при рассинхроне).
    ///
    /// `default` обязателен: у записей, написанных прошлыми версиями плагина,
    /// этого поля нет. Без `default` десериализация падала бы, `read_server_record`
    /// вернул бы `None` — и **все** установленные роутеры выглядели бы
    /// неработающими после обновления.
    #[serde(default)]
    pub data_dir: String,
    /// Unix-время старта. У старых записей поля нет — `default` обязателен.
    #[serde(default)]
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

/// Записать запись о процессе.
///
/// `version = None` означает «версия неизвестна» (запись переписывается до
/// успешного bind). Это честнее, чем подставлять версию плагина: неизвестная
/// версия не должна выдаваться за актуальную.
pub fn write_server_record(
    data_dir: &std::path::Path,
    pid: u32,
    port: u16,
    version: Option<String>,
) {
    let path = server_record_path(data_dir);
    let record = ServerRecord {
        pid,
        port,
        version,
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

    /// Уникальный временный каталог: тесты идут в 4 потока, и общий каталог
    /// приводил бы к взаимному удалению папок.
    fn unique_temp_dir(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("{}_{}", tag, seq));
        let _ = fs::remove_dir_all(&p);
        p
    }

    /// Приоритет 1: путь, выбранный пользователем в панели, сильнее умолчания.
    /// Проверяется для обоих видов роутера — иначе «приоритет» оказался бы
    /// просто первым `if` где-то в середине.
    #[test]
    fn router_dir_override_wins_over_plugin_default() {
        assert_eq!(
            choose_router_dir(Some(r"D:\custom\9router"), RouterId::NineRouter),
            PathBuf::from(r"D:\custom\9router")
        );
        assert_eq!(
            choose_router_dir(Some(r"D:\custom\gateway"), RouterId::Gateway),
            PathBuf::from(r"D:\custom\gateway")
        );
    }

    /// Приоритет 2: умолчание — подкаталог КАТАЛОГА ПЛАГИНА, а не папки хоста.
    ///
    /// Это и есть свойство «шлюз общий для всех проектов»: путь к его бинарю не
    /// зависит от того, какое приложение сейчас открыто. Проверяется сравнением
    /// с [`app_data_dir`] — если бы дефолт снова считался от `current_exe`,
    /// тест упал бы, а не «просто работал бы у King Orch».
    #[test]
    fn router_dir_defaults_to_plugin_dir() {
        for router in RouterId::ALL.iter().copied() {
            let expected = default_router_dir(router);
            assert_eq!(expected.parent(), Some(app_data_dir().as_path()), "роутер {}", router);
            assert_eq!(
                expected.file_name().and_then(|n| n.to_str()),
                Some(router.dir_name()),
                "подкаталог обязан называться как dir_name роутера"
            );
        }
    }

    /// Пустая строка — не выбор. Раньше пустой `dir` молча уводил на дефолт,
    /// и пользователь не понимал, почему его путь проигнорирован.
    #[test]
    fn blank_override_is_treated_as_absent() {
        let default = default_router_dir(RouterId::Gateway);
        assert_eq!(
            choose_router_dir(Some(""), RouterId::Gateway),
            default,
            "пустой dir обязан уйти на следующий уровень"
        );
        assert_eq!(
            choose_router_dir(Some("   "), RouterId::Gateway),
            default,
            "пробельный dir — тоже не выбор"
        );
    }

    /// Перенос настроек из старого per-app конфига хоста.
    ///
    /// Без него переезд плагина в свой каталог тихо обнулил бы путь к бинарю
    /// шлюза: пользователь увидел бы «не установлен» при живом процессе.
    #[test]
    fn ensure_migrated_copies_router_keys_from_legacy_host_config() {
        let root = unique_temp_dir("cr_cfg_migrate");
        let appdata = root.join("appdata");
        let dest = appdata.join("cloud-routers").join("app_config.json");
        let legacy = appdata.join("com.kingorch.app").join("app_config.json");
        fs::create_dir_all(legacy.parent().expect("parent")).expect("mkdir");
        fs::write(
            &legacy,
            r#"{"theme":"dark","cloud_routers":{"gateway":{"dir":"D:\\gw"}},"nine_router":{"port":20128}}"#,
        )
        .expect("write legacy");

        ensure_migrated(&dest);

        let moved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&dest).expect("dest создан")).expect("json");
        assert_eq!(
            moved["cloud_routers"]["gateway"]["dir"], "D:\\gw",
            "путь шлюза обязан перенестись, иначе бинарь потеряется из вида"
        );
        assert_eq!(moved["nine_router"]["port"], 20128);
        assert!(
            moved.get("theme").is_none(),
            "хостовые ключи переносить нельзя — они принадлежат хосту"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Существующий общий конфиг не переписывается: иначе повторный запуск
    /// плагина затирал бы путь, заданный пользователем в панели.
    #[test]
    fn ensure_migrated_never_overwrites_existing_config() {
        let root = unique_temp_dir("cr_cfg_migrate_keep");
        let appdata = root.join("appdata");
        let dest = appdata.join("cloud-routers").join("app_config.json");
        let legacy = appdata.join("com.kingorch.app").join("app_config.json");
        fs::create_dir_all(legacy.parent().expect("parent")).expect("mkdir");
        fs::create_dir_all(dest.parent().expect("parent")).expect("mkdir");
        fs::write(&legacy, r#"{"cloud_routers":{"gateway":{"dir":"D:\\legacy"}}}"#).expect("write legacy");
        fs::write(&dest, r#"{"cloud_routers":{"gateway":{"dir":"D:\\mine"}}}"#).expect("write dest");

        ensure_migrated(&dest);

        let kept = fs::read_to_string(&dest).expect("dest на месте");
        assert!(
            kept.contains("mine") && !kept.contains("legacy"),
            "общий конфиг перезаписан: {}",
            kept
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn default_ports() {
        assert_eq!(RouterId::NineRouter.default_port(), 20128);
        assert_eq!(RouterId::ExtremeRouter.default_port(), 20129);
        assert_eq!(RouterId::OmniRoute.default_port(), 20130);
        assert_eq!(RouterId::Gateway.default_port(), 20131);
    }

    #[test]
    fn test_npm_packages() {
        // npm-пакет есть только у node-роутеров. У шлюза его нет by design, и
        // `None` здесь защищает `check_router_update` от похода по
        // несуществующему адресу.
        assert_eq!(RouterId::NineRouter.npm_package(), Some("9router"));
        assert_eq!(
            RouterId::ExtremeRouter.npm_package(),
            Some("@rsalmn/extremerouter")
        );
        assert_eq!(RouterId::OmniRoute.npm_package(), Some("omniroute"));
        assert_eq!(RouterId::Gateway.npm_package(), None);
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

        write_server_record(&tmp, 1234, 20128, Some("0.1.0".to_string()));
        let rec = read_server_record(&tmp).unwrap();
        assert_eq!(rec.pid, 1234);
        assert_eq!(rec.port, 20128);
        assert_eq!(rec.version.as_deref(), Some("0.1.0"));
        clear_server_record(&tmp);
        assert!(read_server_record(&tmp).is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn server_record_tolerates_legacy_shape_without_version() {
        // Запись от старой версии плагина не должна ломать чтение: у неё нет
        // поля version. Иначе после обновления все установленные роутеры
        // выглядели бы «не установлены».
        //
        // Каталог уникален по счётчику: тесты идут в 4 потока, и общий
        // временный каталог приводил к взаимному удалению папок.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let tmp = std::env::temp_dir().join(format!("cr_test_server_record_legacy_{seq}"));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(server_record_path(&tmp), r#"{"pid":7,"port":20128}"#).unwrap();
        // Диагностика в сообщении: без неё «не разобралось» не отличить от
        // «файла нет», а это разные баги.
        let path = server_record_path(&tmp);
        let raw = fs::read_to_string(&path).expect("файл записи должен читаться");
        let rec: ServerRecord = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("{:?} не разобрался: {}", path, e));
        assert_eq!(rec.pid, 7);
        assert_eq!(rec.version, None);
        assert_eq!(rec.pid, 7);
        assert_eq!(rec.version, None);
        let _ = fs::remove_dir_all(&tmp);
    }
}
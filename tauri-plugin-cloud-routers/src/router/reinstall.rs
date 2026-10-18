//! Переустановка роутера «с нуля»: остановить → снести установку и данные →
//! поставить последнюю версию заново.
//!
//! ## Зачем
//!
//! У пользователей 9router, попавших на экран логина с паролем, который никто
//! не знает (или забытый собственный), нет пути обратно, кроме установки
//! «с нуля». Кнопка «Установить» переустановкой не является: она достраивает
//! поверх существующих файлов и НЕ трогает каталог данных, где лежат настройки
//! дашборда (включая пароль) и сохранённые комбо. Здесь — честный «снести и
//! поставить».
//!
//! ## Что уничтожается (панель предупреждает пользователя перед вызовом)
//!
//! - папка установки (`router_dir`): `dist`, `dist_staging_*`, `dist_old_*`,
//!   `.cache`, `runtime` (portable node.exe), `server.log`;
//! - каталог данных (`router_data_dir`, например `%APPDATA%/9router`): SQLite
//!   настроек дашборда (requireLogin, пароль), `server.json` и прочие данные
//!   сервера;
//! - версии в конфиге плагина (`installed_version`, `node_version`),
//!   deprecated-флаг `pending_no_login` и запись о неудачном обновлении.
//!   Пользовательский
//!   путь установки (`dir`) и порт (`port`) НЕ трогаются.
//!
//! ## Инварианты
//!
//! - сервер остановлен ДО удаления файлов (иначе Windows держит node.exe);
//! - установка идёт через `installer::install_or_update(force=true)`; экран
//!   логина выключится автоматически первой же проверкой состояния сервера
//!   (`no_login.rs` — state-check, конфиг-флагов больше нет);
//! - вызов обязан происходить под `installer::install_guard` (check in
//!   command) и не может столкнуться с параллельной установкой или стартом.

use crate::commands::RouterId;
use crate::router::{config, installer};

/// Полная переустановка: остановка, снос установки и данных, установка заново.
///
/// `progress` — тот же анонсирующий колбэк, что у обычной установки: переустановка
/// может идти минуту, и панель показывает её ход.
pub fn reinstall(
    app: &tauri::AppHandle,
    router_id: RouterId,
    progress: installer::ProgressFn,
) -> Result<(), String> {
    log::info!("reinstall: {} — переустановка начата", router_id);

    let data_dir = config::router_data_dir(app, router_id);
    let cfg_before = config::load_config(app, router_id);
    let port = cfg_before.port_or_default(router_id);

    // 1. Остановить сервер. Единая точка остановки — как у кнопки «Остановить»
    //    (чужой процесс на порту НЕ убивается: отказ с причиной пользователю).
    crate::commands::stop_server_for(app, router_id, port, &data_dir)?;

    // 2. Снести папку установки: dist, staging-огрызки, кеш tgz, runtime.
    let dir = config::router_dir(app, router_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| {
            format!(
                "Не удалось удалить папку установки {}: {}",
                dir.display(),
                e
            )
        })?;
        log::info!(
            "reinstall: {} — папка установки удалена: {}",
            router_id,
            dir.display()
        );
    }

    // 3. Снести каталог данных: настройки дашборда (включая пароль), server.json.
    //    Именно это возвращает заблокированного на экране логина пользователя к
    //    чистому состоянию, из которого свежая установка продолжит без пароля.
    if data_dir.exists() {
        std::fs::remove_dir_all(&data_dir).map_err(|e| {
            format!(
                "Не удалось удалить каталог данных {}: {}",
                data_dir.display(),
                e
            )
        })?;
        log::info!(
            "reinstall: {} — каталог данных удалён: {}",
            router_id,
            data_dir.display()
        );
    }

    // 4. Сбросить версии в конфиге: установка видит «свежую» картину (роутер как
    //    не установленный), failed_update устарела, флаг логина сброшен —
    //    свежая установка сама поставит его заново. dir/port не трогаем.
    let mut cfg = config::load_config(app, router_id);
    cfg.installed_version = None;
    cfg.node_version = None;
    cfg.pending_no_login = false;
    cfg.failed_update = None;
    config::save_config(app, router_id, &cfg)?;

    // 5. Поставить последнюю версию заново (force: это переустановка, а не дострой).
    let info = installer::install_or_update(app, router_id, true, progress)?;
    log::info!(
        "reinstall: {} — переустановлен v{} ({})",
        router_id,
        info.version,
        info.path
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::test_scratch::scratch_dir;

    #[test]
    fn wipe_clears_versions_only() {
        // Пользовательские dir и port не должны сбрасываться операцией переустановки:
        // иначе «Переустановить» молча переезжал бы с выбранной пользователем папки.
        let tmp = scratch_dir("reinstall_cfg_reset");
        let path = tmp.join("app_config.json");
        let cfg = config::RouterConfig {
            port: Some(29999),
            dir: Some("C:\\custom\\9router".into()),
            installed_version: Some("1.2.3".into()),
            node_version: Some("v22.0.0".into()),
            api_key: Some("sk-test".into()),
            pending_no_login: true,
            ..Default::default()
        };
        config::save_config_file(&path, RouterId::NineRouter, &cfg).unwrap();
        // Воспроизводим ТЕЧНО строку reinstall(): загрузить текущий конфиг из
        // файла и очистить ТОЛЬКО четыре поля (versions/флаг/failed_update).
        let mut again = {
            let root = std::fs::read_to_string(&path).unwrap();
            let cloud = serde_json::from_str::<serde_json::Value>(&root)
                .unwrap()
                .get("cloud_routers")
                .cloned()
                .unwrap_or_default();
            serde_json::from_value::<config::RouterConfig>(
                cloud.get("9router").cloned().unwrap_or_default(),
            )
            .unwrap()
        };
        again.installed_version = None;
        again.node_version = None;
        again.pending_no_login = false;
        again.failed_update = None;
        config::save_config_file(&path, RouterId::NineRouter, &again).unwrap();
        let after = {
            let root = std::fs::read_to_string(&path).unwrap();
            let cloud = serde_json::from_str::<serde_json::Value>(&root)
                .unwrap()
                .get("cloud_routers")
                .cloned()
                .unwrap_or_default();
            serde_json::from_value::<config::RouterConfig>(
                cloud.get("9router").cloned().unwrap_or_default(),
            )
            .unwrap()
        };
        assert_eq!(after.installed_version, None);
        assert_eq!(after.node_version, None);
        assert!(!after.pending_no_login);
        assert_eq!(after.failed_update, None);
        // Пользовательские выборы сохранены:
        assert_eq!(after.port, Some(29999));
        assert_eq!(after.dir.as_deref(), Some("C:\\custom\\9router"));
        assert_eq!(after.api_key.as_deref(), Some("sk-test"));
    }
}

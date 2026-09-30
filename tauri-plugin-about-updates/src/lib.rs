use serde::Deserialize;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

/// Конфиг плагина. Задаётся в `tauri.conf.json` хоста:
/// ```json
/// "plugins": {
///   "about-updates": { "repo": "ВАШ_USER/ВАШ_РЕПО" }
/// }
/// ```
#[derive(Deserialize, Clone)]
pub struct Config {
    /// GitHub-репозиторий в формате "owner/repo".
    pub repo: String,
    /// Опциональная ссылка «Поддержать автора» (открывается в браузере).
    /// Если не задана — кнопка не показывается.
    #[serde(default)]
    pub support_url: Option<String>,
}

/// Состояние плагина, доступное командам во время выполнения.
pub(crate) struct PluginState {
    pub repo: String,
    pub support_url: Option<String>,
}

mod commands;
pub mod install_report;
pub mod installer;
pub mod models;
mod updater_rollback;

pub use models::ReleaseInfo;
/// Бэкап данных перед понижением/обновлением версии — общий для хоста и плагина
/// (SSOT: хост не держит копию этого кода у себя).
pub use updater_rollback::backup_before_rollback;

/// Инициализация плагина. Вызывается из хоста: `.plugin(tauri_plugin_about_updates::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R, Config> {
    Builder::<R, Config>::new("about-updates")
        .setup(|app, api| {
            let state = PluginState {
                repo: api.config().repo.clone(),
                support_url: api.config().support_url.clone(),
            };
            app.manage(state);

            // Вердикт по прошлой установке («откат состоялся / не состоялся»).
            // Именно в фоновой задаче, а не прямо здесь: глобальный логгер хоста
            // поднимается в setup'е плагина логов, который регистрируется позже,
            // и сообщение из setup этого плагина ушло бы в никуда.
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                install_report::resolve_pending(&handle);
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_release_history,
            commands::install_release,
            commands::get_install_report,
            commands::get_app_version,
            commands::get_support_url
        ])
        .build()
}

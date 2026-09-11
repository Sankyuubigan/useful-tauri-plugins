use serde::Deserialize;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

/// Конфиг плагина. Задаётся в `tauri.conf.json` хоста:
/// ```json
/// "plugins": {
///   "about-updates": { "repo": "ВАШ_USER/ВАШ_REPO" }
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
pub mod models;
mod updater_rollback;

pub use models::ReleaseInfo;

/// Инициализация плагина. Вызывается из хоста: `.plugin(tauri_plugin_about_updates::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R, Config> {
    Builder::<R, Config>::new("about-updates")
        .setup(|app, api| {
            let state = PluginState {
                repo: api.config().repo.clone(),
                support_url: api.config().support_url.clone(),
            };
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_release_history,
            commands::install_release,
            commands::get_app_version,
            commands::get_support_url
        ])
        .build()
}

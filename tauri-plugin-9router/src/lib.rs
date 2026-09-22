//! 🔀 tauri-plugin-9router — переиспользуемый шлюз облачных LLM через 9Router.
//!
//! 9Router — это локальный proxy/роутер (Next.js standalone, работает на Node.js),
//! который соединяет AI-инструменты с 40+ провайдерами и 100+ моделями: подписки
//! (Claude Code, Codex), дешёвые API (GLM, MiniMax, DeepSeek) и бесплатные
//! (Kiro, OpenCode Free, Vertex) с авто-fallback и RTK-сжатием токенов.
//!
//! Плагин владеет:
//! - **Установкой без системных зависимостей**: портативный `node.exe`
//!   (официальный zip с nodejs.org) + готовый standalone-билд 9router
//!   (тарболл с registry.npmjs.org). Юзеру не нужен ни Node.js, ни терминал.
//! - **Ленивым автозапуском по требованию**: сервер стартует только когда хост
//!   реально использует выбор (комбо 9router в чате), и гасится при закрытии
//!   приложения (Windows Job Object `KILL_ON_JOB_CLOSE` + реестр PID).
//! - **Доступом к комбо**: список комбо из `/api/combos` для дропдауна чата.
//! - **OpenAI-совместимым chat completions** со стримингом через Tauri-события.
//!
//! ## Подключение в хосте
//!
//! `Cargo.toml`:
//! ```toml
//! tauri-plugin-9router = { path = "../../my-tauri-plugins/tauri-plugin-9router" }
//! ```
//!
//! `main.rs`:
//! ```ignore
//! fn main() {
//!     tauri::Builder::default()
//!         .plugin(tauri_plugin_9router::init())
//!         // ...
//!         .run(tauri::generate_context!())
//! }
//! ```
//!
//! `capabilities/default.json`: добавить `"9router:default"`.
//!
//! Папка установки по умолчанию — `<exe>/9router` (правило cwd:
//! `std::env::current_exe().parent()`, не `app.path().executable_dir()`).
//! Можно переопределить ключом `nine_router.dir` в `app_config.json` хоста.

pub mod commands;
pub mod router;

pub use router::config::set_app_data_dir_name;

use serde::Deserialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{RunEvent, Wry};

/// Конфиг плагина (`plugins.9router` в `tauri.conf.json` хоста).
///
/// Устойчив к отсутствию секции: если ключа `9router` в `plugins` нет, Tauri
/// передаёт `null`, и наивная десериализация в структуру паникует на старте
/// хоста ("invalid type: null, expected struct Config"). Здесь `null`
/// трактуется как конфиг по умолчанию.
#[derive(Clone, Default)]
pub struct Config {
    /// Имя папки app-data (как у llama-engine). Пусто — значение хоста из
    /// `set_app_data_dir_name` (или fallback `com.kingorch.app`).
    pub data_dir_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ConfigInner {
    data_dir_name: Option<String>,
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Option::<ConfigInner>::deserialize(deserializer)?
            .map(|c| Config { data_dir_name: c.data_dir_name })
            .unwrap_or_default())
    }
}

/// Инициализация плагина: `.plugin(tauri_plugin_9router::init())`.
pub fn init() -> TauriPlugin<Wry, Config> {
    Builder::<Wry, Config>::new("9router")
        .setup(|_app, api| {
            if let Some(name) = &api.config().data_dir_name {
                if !name.is_empty() {
                    router::config::set_app_data_dir_name(name);
                }
            }
            Ok(())
        })
        .on_event(|_app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                // Гарантированное убийство сервера 9router на выходе
                // (Job Object KILL_ON_JOB_CLOSE + плановый килл по PID).
                router::process::kill_active_servers();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::install_or_update,
            commands::ensure_started,
            commands::stop,
            commands::set_router_dir,
            commands::get_combos,
            commands::set_api_key,
            commands::open_dashboard,
            commands::chat_completion,
        ])
        .build()
}
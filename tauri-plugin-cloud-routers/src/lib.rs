//! tauri-plugin-cloud-routers — переиспользуемый шлюз облачных LLM.
//!
//! Поддерживает несколько облачных роутеров (9Router, ExtremeRouter, OmniRoute)
//! через единый интерфейс. Каждый роутер — локальный proxy/роутер (Next.js
//! standalone, работает на Node.js), который соединяет AI-инструменты с
//! провайдерами и моделями с авто-fallback.
//!
//! Плагин владеет:
//! - **Установкой без системных зависимостей**: портативный `node.exe`
//!   (официальный zip с nodejs.org) + готовый standalone-билд роутера
//!   (тарболл с registry.npmjs.org). Юзеру не нужен ни Node.js, ни терминал.
//! - **Ленивый автозапуск по требованию**: сервер стартует только когда хост
//!   реально использует выбор (комбо роутера в чате), и продолжает работать
//!   после закрытия приложения (управляется через `server.json` и порты).
//! - **Доступом к комбо**: список комбо из `/api/combos` для дропдауна чата.
//! - **OpenAI-совместимым chat completions** со стримингом через Tauri-события.
//!
//! ## Подключение в хосте
//!
//! `Cargo.toml`:
//! ```toml
//! tauri-plugin-cloud-routers = { path = "../../my-tauri-plugins/tauri-plugin-cloud-routers" }
//! ```
//!
//! `main.rs`:
//! ```ignore
//! fn main() {
//!     tauri::Builder::default()
//!         .plugin(tauri_plugin_cloud_routers::init())
//!         // ...
//!         .run(tauri::generate_context!())
//! }
//! ```
//!
//! `capabilities/default.json`: добавить `"cloud-routers:default"`.
//!
//! Папка установки по умолчанию — `APPDATA/cloud-routers/<id>`: конфиг и
//! бинарники принадлежат плагину, а не хосту, поэтому путь одинаков из любого
//! проекта (переопределяется ключом `<id>.dir` в `APPDATA/cloud-routers/app_config.json`).

pub mod commands;
pub mod router;
pub mod spec;

pub use commands::RouterId;
pub use router::config::set_app_data_dir_name;
pub use spec::{RouterKind, RouterSpec, ROUTERS};

use serde::Deserialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::Wry;

/// Конфиг плагина (`plugins.cloud-routers` в `tauri.conf.json` хоста).
///
/// Устойчив к отсутствию секции: если ключа `cloud-routers` в `plugins` нет, Tauri
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

/// Инициализация плагина: `.plugin(tauri_plugin_cloud_routers::init())`.
pub fn init() -> TauriPlugin<Wry, Config> {
    Builder::<Wry, Config>::new("cloud-routers")
        .setup(|app, api| {
            if let Some(name) = &api.config().data_dir_name {
                if !name.is_empty() {
                    router::config::set_app_data_dir_name(name);
                }
            }
            let handle = app.clone();
            router::config::migrate_legacy_config(&handle);
            for router_id in spec::RouterId::ALL.iter().copied() {
                match router_id.kind() {
                    // Node-роутер: бесхозный инстанс останавливаем, чтобы не
                    // держать на порту процесс из прошлой сессии.
                    spec::RouterKind::NodeBundle { .. } => {
                        router::process::reconcile_server_state(&handle, router_id)
                    }
                    // Свой шлюз переживает закрытие хоста — значит, в новой
                    // сессии он закономерно остаётся живым, и его нужно принять,
                    // а не убить.
                    spec::RouterKind::NativeGateway { .. } => {
                        router::gateway_process::reconcile(&handle, router_id)
                    }
                }
            }
            Ok(())
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
            commands::check_router_update,
        ])
        .build()
}
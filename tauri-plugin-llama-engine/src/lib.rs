//! ⚙️ tauri-plugin-llama-engine — переиспользуемый движок llama.cpp.
//!
//! Движок инференса — ОТДЕЛЬНЫЙ процесс `llama-server.exe` (полный релиз
//! llama.cpp, папка `<exe>/llamacpp`), общение по HTTP (localhost, random-порт +
//! api-key). Приложение НЕ линкует llama.cpp нативно.
//!
//! Плагин владеет: установкой/обновлением выбранного варианта бекенда
//! (cuda / vulkan / cpu / hip-radeon), каталогом `models_catalog.json`
//! (embedded `include_str!` + опциональные внешние копии), списком моделей,
//! движковыми параметрами (те, что выставляет юзер в <llama-models-panel>),
//! авто-подбором mmproj и VRAM-уведомлениями.
//!
//! ## Подключение в хосте
//!
//! `Cargo.toml`:
//! ```toml
//! tauri-plugin-llama-engine = { path = "../../my-tauri-plugins/tauri-plugin-llama-engine" }
//! ```
//!
//! `main.rs`:
//! ```ignore
//! fn main() {
//!     tauri_plugin_llama_engine::engine::config::set_app_data_dir_name("com.kingorch.app");
//!     tauri::Builder::default()
//!         .plugin(tauri_plugin_llama_engine::init())
//!         // ...
//!         .run(tauri::generate_context!())
//! }
//! ```
//!
//! Фасад хоста (`src/infra/mod.rs`):
//! ```ignore
//! pub use tauri_plugin_llama_engine::engine::*;
//! ```
//!
//! `tauri.conf.json` (опционально, альтернатива `set_app_data_dir_name`):
//! ```json
//! "plugins": { "llama-engine": { "data_dir_name": "com.kingorch.app" } }
//! ```
//!
//! `capabilities/default.json`: добавить `"llama-engine:default"`.
//!
//! Синхронизация списков моделей с хостовым UI — CustomEvent на document:
//! `llama:models-changed` (payload — конфиг движка), хост перечитывает
//! `get_config()`.

pub mod commands;
pub mod engine;

use serde::Deserialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{RunEvent, Wry};

/// Конфиг плагина (`plugins.llama-engine` в `tauri.conf.json` хоста).
#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Config {
    /// Имя папки app-data (например `com.kingorch.app`); применяется до первой
    /// блокировки файла конфига. Пусто — остаётся значение хоста из
    /// `set_app_data_dir_name` (или fallback `com.kingorch.app`).
    pub data_dir_name: Option<String>,
}

/// Инициализация плагина: `.plugin(tauri_plugin_llama_engine::init())`.
pub fn init() -> TauriPlugin<Wry, Config> {
    Builder::<Wry, Config>::new("llama-engine")
        .setup(|app, api| {
            if let Some(name) = &api.config().data_dir_name {
                if !name.is_empty() {
                    engine::config::set_app_data_dir_name(name);
                }
            }
            engine::vram::start_forwarding(app);
            Ok(())
        })
        .on_event(|_app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                // Гарантированное убийство движков на выходе (Job Object + плановый килл).
                engine::process_util::kill_active_engines();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_engine_status,
            commands::install_llamacpp,
            commands::set_engine_variant,
            commands::check_engine_update,
            commands::install_engine_update,
            commands::remove_engine,
            commands::set_engine_dir,
            commands::get_models_catalog,
            commands::get_engine_config,
            commands::get_auto_download_info,
            commands::auto_download_default_model,
            commands::add_model,
            commands::remove_model,
            commands::delete_model_file,
            commands::get_mmproj_path,
            commands::ensure_mmproj,
            commands::get_model_capabilities,
            commands::get_all_capabilities,
            commands::estimate_prompt_memory,
            commands::get_model_params,
            commands::set_model_params,
            commands::reset_model_params,
            engine::downloader::download_model,
        ])
        .build()
}

/// Хелпер для хоста (setup): папка движка `<exe>/llamacpp` (или из конфига).
pub fn get_engine_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    commands::get_engine_dir(app)
}
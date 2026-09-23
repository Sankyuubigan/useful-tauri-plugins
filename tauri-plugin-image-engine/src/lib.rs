//! 🖼️ tauri-plugin-image-engine — переиспользуемый движок stable-diffusion.cpp.
//!
//! Инференс изображений — ОТДЕЛЬНЫЙ процесс `sd-server.exe` (полный релиз
//! leejet/stable-diffusion.cpp, папка `<exe>/sdcpp`), общение по HTTP
//! (localhost, random-порт, нативный async API `/sdcpp/v1/...`).
//! Приложение НЕ линкует stable-diffusion.cpp нативно (см. global_ai_docs,
//! правило «инференс только отдельным процессом»).
//!
//! Плагин владеет: установкой/обновлением выбранного варианта бекенда
//! (cuda12 / cpu / vulkan / rocm), каталогом `image_models_catalog.json`
//! (бандлы: diffusion + vae + text encoder + mmproj с пресетами и порогами
//! VRAM), скачиванием бандла, VRAM-preflight и генерацией/редактированием.
//!
//! ## Подключение в хосте
//!
//! `Cargo.toml`:
//! ```toml
//! tauri-plugin-image-engine = { path = "../../my-tauri-plugins/tauri-plugin-image-engine" }
//! ```
//!
//! `main.rs`:
//! ```ignore
//! fn main() {
//!     tauri::Builder::default()
//!         .plugin(tauri_plugin_image_engine::init())
//!         // ...
//!         .run(tauri::generate_context!())
//! }
//! ```
//!
//! `capabilities/default.json`: добавить `"image-engine:default"`.

pub mod commands;
pub mod engine;

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{RunEvent, Wry};

/// Инициализация плагина: `.plugin(tauri_plugin_image_engine::init())`.
pub fn init() -> TauriPlugin<Wry> {
    Builder::<Wry>::new("image-engine")
        .on_event(|_app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                // Гарантированное убийство движков на выходе (Job Object + плановый килл).
                engine::process_util::kill_active_engines();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_image_engine_status,
            commands::install_image_engine,
            commands::set_image_engine_variant,
            commands::check_image_engine_update,
            commands::install_image_engine_update,
            commands::remove_image_engine,
            commands::set_image_engine_dir,
            commands::get_image_models_catalog,
            commands::get_image_bundle_info,
            commands::validate_image_bundle_dir,
            commands::set_image_bundle_dir,
            commands::download_image_bundle,
            commands::remove_image_bundle,
            commands::estimate_image_memory,
            commands::generate_image,
            commands::edit_image,
        ])
        .build()
}

/// Хелпер для хоста (setup): папка движка `<exe>/sdcpp` (или из конфига).
pub fn get_image_engine_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    commands::get_image_engine_dir(app)
}

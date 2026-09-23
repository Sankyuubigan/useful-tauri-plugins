//! Движок изображений — инференс через ОТДЕЛЬНЫЙ процесс `sd-server.exe`
//! (полный релиз leejet/stable-diffusion.cpp), общение по HTTP localhost
//! (random-порт, нативный async API `/sdcpp/v1/img_gen` + poll jobs).
//! Приложение НЕ линкует stable-diffusion.cpp нативно.

pub mod config;
pub mod image_engine;
pub mod models_catalog;
pub mod preflight;
pub mod process_util;
pub mod sdcpp_installer;
pub mod sources;

pub use config::{bundle_dir_early, engine_dir_early, load_image_config, load_image_config_early, save_image_config, ImageEngineConfig};
pub use image_engine::{edit_image_files, generate_image_simple, ImageEngine, ImageGenResult};
pub use models_catalog::{
    bundle_disk_bytes, bundle_weights_files, default_bundle_entry, find_bundle_entry,
    load_image_catalog, BundleFile, ImageBundleEntry, ImagePreset,
};
pub use preflight::{
    estimate_image_memory_mb, preflight_check, MemoryEstimate, PreflightVerdict,
};
pub use process_util::kill_active_engines;

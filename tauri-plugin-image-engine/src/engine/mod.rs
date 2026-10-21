//! Движок изображений — инференс через ОТДЕЛЬНЫЙ процесс `sd-server.exe`
//! (полный релиз leejet/stable-diffusion.cpp), общение по HTTP localhost
//! (random-порт, нативный async API `/sdcpp/v1/img_gen` + poll jobs).
//! Приложение НЕ линкует stable-diffusion.cpp нативно.

pub mod ab_view;
pub mod bench;
pub mod bench_ext;
pub mod bench_prompts;
pub mod bench_upscale;
pub mod config;
pub mod image_engine;
pub mod krea_compare;
pub mod krea_view;
pub mod models_catalog;
pub mod preflight;
pub mod process_util;
pub mod ref_size;
pub mod sdcpp_installer;
pub mod sources;

pub use ab_view::{build_ab_artifacts, collect_pairs, make_ab_collage, AbPair};
pub use bench::{run_bench, BenchReport, BenchRun};
pub use bench_upscale::{
    denoise_tag, run_upscale_ab, upscale_file_name, UpscaleAbReport, UpscaleRun, UPSCALE_AB_STRENGTHS,
    UPSCALE_PROMPT_DEFAULT,
};
pub use bench_ext::{phash_diff_vs, phash_mean, run_edit_bench, run_hires_bench, PhashRow};
pub use bench_prompts::{DEFAULT_PROMPTS, EDIT_PROMPTS};
pub use config::{bundle_dir_early, engine_dir_early, load_image_config, load_image_config_early, save_image_config, ImageEngineConfig};
pub use image_engine::{
    edit_image_files, generate_image_simple, upscale_image_files, upscale_target_dims, ImageEngine,
    ImageGenResult, Img2Img, UPSCALE_STRENGTH_DEFAULT,
};
pub use krea_compare::{
    default_legs, download_sources, ensure_krea2_weights, krea2_entry, krea2_turbo_sigmas, load_sources,
    run_krea_compare, save_manifest, Krea2File, KreaCell, KreaCompareReport, KreaLeg, KreaSource,
    KREA2_MAX_PRE_SIDE, KREA2_STACK, KREA2_TURBO_CFG, KREA2_TURBO_MU, KREA2_TURBO_STEPS,
};
pub use krea_view::build_krea_html;
pub use models_catalog::{
    bundle_by_name, bundle_disk_bytes, bundle_weights_files, default_bundle_entry,
    find_bundle_entry, load_image_catalog, BundleFile, ImageBundleEntry, ImagePreset,
};
pub use preflight::{
    estimate_image_memory_mb, preflight_check, MemoryEstimate, PreflightVerdict,
};
pub use process_util::kill_active_engines;

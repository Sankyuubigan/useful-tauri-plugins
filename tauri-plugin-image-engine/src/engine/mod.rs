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
pub mod gen_guard;
pub mod image_engine;
pub mod krea_compare;
pub mod krea_view;
pub mod log_filter;
pub mod lora;
pub mod models_catalog;
pub mod preflight;
pub mod process_util;
pub mod refmod;
pub mod refmod_voice;
pub mod ref_size;
pub mod sdcpp_installer;
pub mod sources;
pub mod video_engine;
pub mod video_refine;

pub use ab_view::{build_ab_artifacts, collect_pairs, make_ab_collage, AbPair};
pub use bench::{run_bench, BenchReport, BenchRun};
pub use bench_upscale::{
    denoise_tag, run_upscale_ab, upscale_file_name, UpscaleAbReport, UpscaleRun, UPSCALE_AB_STRENGTHS,
    UPSCALE_PROMPT_DEFAULT,
};
pub use bench_ext::{phash_diff_vs, phash_mean, run_edit_bench, run_hires_bench, PhashRow};
pub use bench_prompts::{DEFAULT_PROMPTS, EDIT_PROMPTS};
pub use config::{
    bundle_dir_early, data_root, default_image_bundle_dir, default_lora_dir,
    default_video_bundle_dir, engine_dir_early, image_lora_dir_early, lora_dir_early,
    load_image_config, load_image_config_early, save_image_config, video_bundle_dir_early,
    video_lora_dir_early, ImageEngineConfig,
};
pub use gen_guard::MediaGenGuard;
pub use image_engine::{
    edit_image_files, generate_image_simple, plan_ref_edit, upscale_image_files,
    upscale_target_dims, ImageEngine, ImageGenResult, Img2Img, RefEditPlan, UPSCALE_STRENGTH_DEFAULT,
};
pub use lora::{
    clamp_multiplier, inject_edit_lora, lora_spawn_args, lora_spawn_args_for_base,
    resolve_lora_names, resolve_lora_names_for_base, resolve_lora_specs,
    resolve_lora_specs_for_base, scan_lora_applied_line, LoraSpec, LORA_MULTIPLIER_MAX,
};
pub use krea_compare::{
    default_legs, download_sources, ensure_krea2_weights, krea2_entry, krea2_turbo_sigmas, load_sources,
    run_krea_compare, save_manifest, Krea2File, KreaCell, KreaCompareReport, KreaLeg, KreaSource,
    KREA2_MAX_PRE_SIDE, KREA2_STACK, KREA2_TURBO_CFG, KREA2_TURBO_MU, KREA2_TURBO_STEPS,
};
pub use krea_view::build_krea_html;
pub use models_catalog::{
    bundle_by_name, bundle_disk_bytes, bundle_files_weights, bundle_weights_files,
    default_bundle_entry, default_video_bundle_entry, find_bundle_entry, load_image_catalog,
    load_video_catalog, ref_edit_contract, resolve_placement, validate_image_entry,
    video_bundle_by_name, video_bundle_disk_bytes, validate_video_entry, BundleFile,
    ImageBundleEntry, ImagePreset, RefEditContract, RefEditMode, VideoBundleEntry, VideoPlacement,
    VideoPreset,
};
pub use preflight::{
    estimate_image_memory_mb, preflight_check, MemoryEstimate, PreflightVerdict,
};
pub use process_util::kill_active_engines;
pub use refmod::{
    copy_card_voice_in, engine_ref_paths, list_refmod_cards, list_refmod_cards_in,
    load_refmod_card, load_refmod_card_in, normalize_frame, refmods_dir, refmods_prompt,
    refmods_prompt_in, save_refmod_card, save_refmod_card_in, save_refmod_card_images_in,
    set_card_voice, set_card_voice_in, thumbnail_data_url, thumbnail_data_url_in, update_voice_desc_in,
    validate_slug, write_card_voice_in, RefModBundle, RefModError, RefModMeta,
    FRAME_LONG_SIDE_MAX, FRAME_SIZE_MULTIPLE, FRAMES_MAX, FRAMES_PER_RUN, THUMBNAIL_LONG_SIDE,
};
pub use refmod_voice::{normalize_voice, VoiceWav, VOICE_WAV_FILE};
pub use video_engine::{
    adaptive_video_canvas, build_payload, build_spawn_args, file_to_b64, free_ram_gb,
    generate_video_simple, preflight_video, resolve_mode, resolve_video_dimensions,
    sampling_plan_for_quality, seconds_to_frames, VideoEngine,
    VideoGenResult, VideoLoraRef, VideoMode, VideoRequest, VideoSamplingPlan,
};
pub use video_refine::{generate_video_refine, refine_engine_supported, ENGINE_LOG_REASON};
pub use models_catalog::VideoRefinePreset;

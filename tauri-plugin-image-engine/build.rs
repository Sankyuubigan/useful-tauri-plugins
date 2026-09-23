const COMMANDS: &[&str] = &[
    // ── Движок sd.cpp ──
    "get_image_engine_status",
    "install_image_engine",
    "set_image_engine_variant",
    "check_image_engine_update",
    "install_image_engine_update",
    "remove_image_engine",
    "set_image_engine_dir",
    // ── Каталог бандлов ──
    "get_image_models_catalog",
    "get_image_bundle_info",
    "validate_image_bundle_dir",
    "set_image_bundle_dir",
    "download_image_bundle",
    "remove_image_bundle",
    "estimate_image_memory",
    // ── Генерация ──
    "generate_image",
    "edit_image",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .build();
}

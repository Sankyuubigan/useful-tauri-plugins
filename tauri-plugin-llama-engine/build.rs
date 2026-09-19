const COMMANDS: &[&str] = &[
    // ── Движок llama.cpp ──
    "get_engine_status",
    "install_llamacpp",
    "set_engine_variant",
    "check_engine_update",
    "install_engine_update",
    "remove_engine",
    "set_engine_dir",
    // ── Каталог и модели ──
    "get_models_catalog",
    "get_engine_config",
    "get_auto_download_info",
    "auto_download_default_model",
    "add_model",
    "remove_model",
    "delete_model_file",
    "get_mmproj_path",
    "ensure_mmproj",
    "get_model_capabilities",
    "get_all_capabilities",
    "estimate_prompt_memory",
    // ── Параметры сэмплинга ──
    "get_model_params",
    "set_model_params",
    "reset_model_params",
    // ── Скачивание (инструмент) ──
    "download_model",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .build();
}
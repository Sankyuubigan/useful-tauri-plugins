const COMMANDS: &[&str] = &[
    // ── Статус и провижининг ──
    "get_status",
    "download_model",
    "remove_model",
    // ── Инференс ──
    "decide",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .build();
}
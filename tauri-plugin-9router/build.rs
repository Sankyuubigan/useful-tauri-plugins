const COMMANDS: &[&str] = &[
    // ── 9Router gateway ──
    "get_status",
    "install_or_update",
    "ensure_started",
    "stop",
    "set_router_dir",
    "get_combos",
    "open_dashboard",
    "chat_completion",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .build();
}
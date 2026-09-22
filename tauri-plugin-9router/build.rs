const COMMANDS: &[&str] = &[
    // ── 9Router gateway ──
    "get_status",
    "install_or_update",
    "ensure_started",
    "stop",
    "set_router_dir",
    "get_combos",
    "set_api_key",
    "open_dashboard",
    "chat_completion",
    "check_router_update",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .build();
}
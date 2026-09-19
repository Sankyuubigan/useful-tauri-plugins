const COMMANDS: &[&str] = &["get_release_history", "install_release", "get_app_version", "get_support_url"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .build();
}

const COMMANDS: &[&str] = &[
    "download_file",
    "download_bytes",
    "cancel_download",
    "list_active",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .build();
}

const COMMANDS: &[&str] = &[
    "get_last_logs_path",
    "get_log_file_path",
    "log_frontend_event",
    "track_event",
    "track_error",
    "set_reporting_enabled",
    "save_logs_file",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .build();
}
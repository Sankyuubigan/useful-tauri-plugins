const COMMANDS: &[&str] = &[
    "get_ytdlp_status",
    "install_ytdlp",
    "check_ytdlp_update",
    "install_ytdlp_update",
    "set_ytdlp_dir",
    "set_ffmpeg_path",
    "install_deno",
    "fetch_video_info",
    "download_video",
    "cancel_download",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .build();
}

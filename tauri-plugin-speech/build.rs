const COMMANDS: &[&str] = &[
    "tts_speak",
    "tts_presets",
    "tts_capabilities",
    "tts_unload",
    "tts_save_wav",
    "tts_download_engine",
    "tts_download_model",
    "tts_engine_backends",
    "tts_list_models",
    "tts_list_voices",
    "tts_add_voice",
    "tts_delete_voice",
    "tts_update_voice",
    "tts_voice_avatar",
    "tts_voice_audio",
    "tts_voice_trimmed_audio",
    "tts_check_update",
    "tts_default_dirs",
    "tts_get_settings",
    "tts_save_settings",
    "stt_get_settings",
    "stt_save_settings",
    "stt_start",
    "stt_stop",
    "stt_get_status",
    "stt_inject_text",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
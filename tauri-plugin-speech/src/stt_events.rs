use tauri::{AppHandle, Emitter, Runtime};

use crate::stt::SttStatus;

/// Emit STT status event to frontend (for tray icon updates + GUI display).
pub fn emit_status<R: Runtime>(app: &AppHandle<R>, status: &SttStatus) {
    let label = match status {
        SttStatus::Stopped => "stopped",
        SttStatus::Starting => "starting",
        SttStatus::Listening => "listening",
        SttStatus::Recording => "recording",
        SttStatus::Transcribing => "transcribing",
        SttStatus::Error(_) => "error",
    };
    let _ = app.emit("stt-status", label);
}

/// Emit recognized text to frontend.
pub fn emit_result<R: Runtime>(app: &AppHandle<R>, text: &str, is_final: bool) {
    let _ = app.emit(
        "stt-result",
        serde_json::json!({ "text": text, "final": is_final }),
    );
}

/// Emit key press log to frontend (for hotkey configuration).
pub fn emit_key_log<R: Runtime>(app: &AppHandle<R>, key_name: &str, key_code: u32, pressed: bool) {
    let _ = app.emit(
        "stt-key",
        serde_json::json!({
            "name": key_name,
            "code": key_code,
            "pressed": pressed,
        }),
    );
}
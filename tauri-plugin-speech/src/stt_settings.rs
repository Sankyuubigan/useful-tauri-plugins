use serde::{Deserialize, Serialize};

/// Настройки системного голосового ввода (STT daemon).
///
/// Сохраняются в `stt_settings.json` рядом с exe (current_exe().parent()).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttSettings {
    /// rdev Key code for push-to-talk.
    pub hotkey_code: u32,
    /// Human-readable hotkey name for display (e.g. "Period", "F5").
    pub hotkey_name: String,
    /// Path to CrispASR executable (empty = auto-resolve from TTS engine dir).
    pub engine_exe: String,
    /// ASR backend name (default: "gigaam").
    pub backend: String,
    /// Model spec (-m flag): "auto" downloads gigaam-v3 GGUF on first run.
    pub model: String,
    /// WebSocket port (0 = auto-select random port).
    pub ws_port: u16,
    /// Stream step in ms (--stream-step, default 3000).
    pub stream_step_ms: u32,
    /// Stream window length in ms (--stream-length, default 10000).
    pub stream_length_ms: u32,
    /// Enable VAD (--vad flag).
    pub vad: bool,
}

impl Default for SttSettings {
    fn default() -> Self {
        Self {
            hotkey_code: 83, // rdev Key::Dot = '.' key (scancode 0x53 = 83)
            hotkey_name: "Period".into(),
            engine_exe: String::new(),
            backend: "gigaam".into(),
            model: "auto".into(),
            ws_port: 0,
            stream_step_ms: 3000,
            stream_length_ms: 10000,
            vad: true,
        }
    }
}

impl SttSettings {
    pub fn load() -> Self {
        let path = settings_path();
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(s) = serde_json::from_str(&data) {
                    return s;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = settings_path();
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize STT settings: {e}"))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("failed to write STT settings to {}: {e}", path.display()))?;
        Ok(())
    }
}

fn settings_path() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().unwrap_or(std::path::Path::new("."));
    dir.join("stt_settings.json")
}
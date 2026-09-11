use std::net::TcpStream;

use tungstenite::protocol::Message;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, WebSocket};

/// WebSocket client for CrispASR streaming STT.
///
/// Connects to `ws://127.0.0.1:{port}` and:
/// - Sends binary frames of float32 PCM (16kHz mono)
/// - Receives JSON lines: {"text":"...", "t0":..., "t1":..., "final":true/false}
pub struct SttWsClient {
    ws: Option<WebSocket<MaybeTlsStream<TcpStream>>>,
    port: u16,
}

#[derive(Debug, Clone)]
pub struct SttResult {
    pub text: String,
    pub is_final: bool,
    pub t0: f64,
    pub t1: f64,
}

impl SttWsClient {
    pub fn new(port: u16) -> Self {
        Self { ws: None, port }
    }

    /// Connect to CrispASR WebSocket server.
    pub fn connect(&mut self) -> Result<(), String> {
        let url = format!("ws://127.0.0.1:{}", self.port);
        let (ws, _) = connect(&url).map_err(|e| format!("WebSocket handshake failed: {e}"))?;
        self.ws = Some(ws);
        Ok(())
    }

    /// Send a chunk of audio (float32 PCM, 16kHz mono) to CrispASR.
    pub fn send_audio(&mut self, samples: &[f32]) -> Result<(), String> {
        if let Some(ws) = &mut self.ws {
            let bytes: Vec<u8> = samples
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect();
            ws.send(Message::Binary(bytes.into()))
                .map_err(|e| format!("WS send error: {e}"))?;
            Ok(())
        } else {
            Err("WebSocket not connected".into())
        }
    }

    /// Try to receive a transcription result (non-blocking).
    pub fn recv_result(&mut self) -> Option<SttResult> {
        if let Some(ws) = &mut self.ws {
            match ws.read() {
                Ok(Message::Text(text)) => self.parse_json(&text),
                Ok(Message::Binary(data)) => {
                    if let Ok(s) = String::from_utf8(data.to_vec()) {
                        self.parse_json(&s)
                    } else {
                        None
                    }
                }
                Ok(_) => None,
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    None
                }
                Err(e) => {
                    eprintln!("[stt] WS recv error: {e}");
                    None
                }
            }
        } else {
            None
        }
    }

    fn parse_json(&self, text: &str) -> Option<SttResult> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let t = v.get("type")?.as_str()?;
        match t {
            "partial" => Some(SttResult {
                text: v.get("text")?.as_str()?.to_string(),
                is_final: false,
                t0: v.get("t0")?.as_f64().unwrap_or(0.0),
                t1: v.get("t1")?.as_f64().unwrap_or(0.0),
            }),
            "final" => Some(SttResult {
                text: v.get("text")?.as_str()?.to_string(),
                is_final: true,
                t0: v.get("t0")?.as_f64().unwrap_or(0.0),
                t1: v.get("t1")?.as_f64().unwrap_or(0.0),
            }),
            _ => None,
        }
    }

    /// Send close frame and disconnect.
    pub fn disconnect(&mut self) {
        if let Some(mut ws) = self.ws.take() {
            let _ = ws.close(None);
        }
    }
}

impl Drop for SttWsClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}
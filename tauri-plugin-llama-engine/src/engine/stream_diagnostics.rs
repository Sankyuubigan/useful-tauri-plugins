use std::collections::VecDeque;
use std::error::Error as _;
use std::io;
use std::sync::{Arc, Mutex};

const STDERR_CAPACITY: usize = 100;
const STDERR_ERROR_LINES: usize = 20;
const STDERR_RECENT_LINES: usize = 30;

#[derive(Debug, Default)]
struct ServerTraceState {
    pid: Option<u32>,
    stderr: VecDeque<String>,
}

#[derive(Clone, Default)]
pub struct ServerTrace {
    state: Arc<Mutex<ServerTraceState>>,
}

impl ServerTrace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_process(&self, pid: u32) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.pid = Some(pid);
        state.stderr.clear();
    }

    pub fn push_stderr(&self, line: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.stderr.len() == STDERR_CAPACITY {
            state.stderr.pop_front();
        }
        state.stderr.push_back(line);
    }

    pub fn pid(&self) -> Option<u32> {
        self.state.lock().ok().and_then(|state| state.pid)
    }

    pub fn diagnostic_lines(&self) -> Vec<String> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        select_diagnostic_lines(
            state.stderr.iter().map(String::as_str).collect(),
            STDERR_ERROR_LINES,
            STDERR_RECENT_LINES,
        )
    }
}

fn select_diagnostic_lines(lines: Vec<&str>, error_limit: usize, recent_limit: usize) -> Vec<String> {
    let is_error = |line: &str| {
        let lower = line.to_lowercase();
        lower.contains("error") || lower.contains("failed") || lower.contains("oom")
            || lower.contains("exception") || lower.contains("fatal")
    };
    let mut errors = lines
        .iter()
        .rev()
        .filter(|line| is_error(line))
        .take(error_limit)
        .collect::<Vec<_>>();
    errors.reverse();
    let recent_start = lines.len().saturating_sub(recent_limit);
    let recent = lines[recent_start..].iter().rev().collect::<Vec<_>>();
    let mut selected = errors;
    for line in recent {
        if !selected.iter().any(|existing| *existing == line) {
            selected.push(line);
        }
    }
    selected.reverse();
    selected.into_iter().map(|line| line.to_string()).collect()
}

#[derive(Debug, Clone, Default)]
pub struct ResponseMeta {
    pub status: u16,
    pub http_version: String,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub transfer_encoding: Option<String>,
    pub connection: Option<String>,
}

impl ResponseMeta {
    pub fn from_response(response: &reqwest::blocking::Response) -> Self {
        let headers = response.headers();
        Self {
            status: response.status().as_u16(),
            http_version: format!("{:?}", response.version()),
            content_type: header(headers, "content-type"),
            content_length: headers
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok()),
            transfer_encoding: header(headers, "transfer-encoding"),
            connection: header(headers, "connection"),
        }
    }

    pub fn format(&self) -> String {
        format!(
            "HTTP {} {}; type={}; length={}; transfer={}; connection={}",
            self.status,
            self.http_version,
            self.content_type.as_deref().unwrap_or("нет"),
            self.content_length
                .map(|value| value.to_string())
                .unwrap_or_else(|| "нет".to_string()),
            self.transfer_encoding.as_deref().unwrap_or("нет"),
            self.connection.as_deref().unwrap_or("нет"),
        )
    }

    pub fn telemetry(&self) -> serde_json::Value {
        serde_json::json!({
            "http_status": self.status,
            "http_version": self.http_version,
            "content_type": self.content_type,
            "content_length": self.content_length,
            "transfer_encoding": self.transfer_encoding,
            "connection": self.connection,
        })
    }
}

fn header(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

#[derive(Debug, Clone, Default)]
pub struct StreamReaderError {
    pub message: String,
    pub kind: String,
    pub raw_os_error: Option<i32>,
}

impl StreamReaderError {
    pub fn from_io(error: &io::Error) -> Self {
        let mut message = error.to_string();
        let mut source = error.source();
        while let Some(error) = source {
            message.push_str(" → ");
            message.push_str(&error.to_string());
            source = error.source();
        }
        Self {
            message,
            kind: format!("{:?}", error.kind()),
            raw_os_error: error.raw_os_error(),
        }
    }

    pub fn format(&self) -> String {
        format!(
            "kind={}; os={}; message={}",
            self.kind,
            self.raw_os_error
                .map(|value| value.to_string())
                .unwrap_or_else(|| "нет".to_string()),
            self.message
        )
    }
}

#[derive(Debug, Clone)]
pub enum ServerProcessState {
    Running,
    Exited(Option<i32>),
    Missing,
    Unknown(String),
}

#[derive(Debug, Clone, Copy)]
pub enum ServerHealth {
    Healthy,
    Unavailable,
    NotChecked,
}

#[derive(Debug, Clone)]
pub struct ServerProcessSnapshot {
    pub pid: Option<u32>,
    pub process: ServerProcessState,
    pub health: ServerHealth,
}

impl ServerProcessSnapshot {
    pub fn format(&self) -> String {
        let process = match &self.process {
            ServerProcessState::Running => "running".to_string(),
            ServerProcessState::Exited(code) => format!(
                "exited(code={})",
                code.map(|value| value.to_string())
                    .unwrap_or_else(|| "нет".to_string())
            ),
            ServerProcessState::Missing => "missing".to_string(),
            ServerProcessState::Unknown(error) => format!("unknown({})", error),
        };
        let health = match self.health {
            ServerHealth::Healthy => "healthy",
            ServerHealth::Unavailable => "unavailable",
            ServerHealth::NotChecked => "not_checked",
        };
        format!(
            "pid={}; process={}; health={}",
            self.pid
                .map(|value| value.to_string())
                .unwrap_or_else(|| "нет".to_string()),
            process,
            health
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    pub first_event_ms: Option<u128>,
    pub first_token_ms: Option<u128>,
    pub last_data_age_ms: Option<u128>,
    pub raw_lines: u64,
    pub events: u64,
    pub bytes: u64,
    pub output_tokens: u32,
    pub reasoning_tokens: u32,
    pub finish_reason: String,
}

#[derive(Debug, Clone)]
pub struct StreamDiagnostics {
    pub reason: String,
    pub response: ResponseMeta,
    pub reader_error: Option<StreamReaderError>,
    pub server: ServerProcessSnapshot,
    pub elapsed_ms: u128,
    pub first_event_ms: Option<u128>,
    pub first_token_ms: Option<u128>,
    pub last_data_age_ms: Option<u128>,
    pub raw_lines: u64,
    pub events: u64,
    pub bytes: u64,
    pub output_tokens: u32,
    pub reasoning_tokens: u32,
    pub finish_reason: String,
    pub stderr_lines: Vec<String>,
}

impl StreamDiagnostics {
    pub fn signals(&self) -> Vec<String> {
        let mut signals = Vec::new();
        if self
            .response
            .content_length
            .is_some_and(|expected| self.bytes < expected)
        {
            signals.push("response_body_truncated".to_string());
        }
        match &self.server.process {
            ServerProcessState::Running => signals.push("server_running".to_string()),
            ServerProcessState::Exited(code) => signals.push(format!(
                "server_exited:{}",
                code.map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            )),
            ServerProcessState::Missing => signals.push("server_missing".to_string()),
            ServerProcessState::Unknown(error) => signals.push(format!("server_unknown:{}", error)),
        }
        match self.server.health {
            ServerHealth::Healthy => signals.push("health_ok".to_string()),
            ServerHealth::Unavailable => signals.push("health_failed".to_string()),
            ServerHealth::NotChecked => {}
        }
        signals
    }

    pub fn format(&self) -> String {
        let reader = self.reader_error.as_ref().map(StreamReaderError::format).unwrap_or_else(|| "нет".to_string());
        let signals = self.signals().join(",");
        let stderr = if self.stderr_lines.is_empty() {
            "нет".to_string()
        } else {
            self.stderr_lines.join(" | ")
        };
        format!(
            "STREAM_DIAGNOSTIC {} | response={} | reader={} | signals={} | elapsed_ms={} | first_event_ms={} | first_token_ms={} | last_data_age_ms={} | raw_lines={} | events={} | bytes={} | output_tokens={} | reasoning_tokens={} | finish={} | server={} | stderr_tail={}",
            self.reason,
            self.response.format(),
            reader,
            signals,
            self.elapsed_ms,
            optional_ms(self.first_event_ms),
            optional_ms(self.first_token_ms),
            optional_ms(self.last_data_age_ms),
            self.raw_lines,
            self.events,
            self.bytes,
            self.output_tokens,
            self.reasoning_tokens,
            self.finish_reason,
            self.server.format(),
            stderr,
        )
    }

    pub fn telemetry(&self) -> serde_json::Value {
        serde_json::json!({
            "reason": self.reason,
            "signals": self.signals(),
            "reader_kind": self.reader_error.as_ref().map(|error| error.kind.clone()),
            "reader_os_error": self.reader_error.as_ref().and_then(|error| error.raw_os_error),
            "response": self.response.telemetry(),
            "pid": self.server.pid,
            "process": match &self.server.process {
                ServerProcessState::Running => "running",
                ServerProcessState::Exited(_) => "exited",
                ServerProcessState::Missing => "missing",
                ServerProcessState::Unknown(_) => "unknown",
            },
            "exit_code": match &self.server.process {
                ServerProcessState::Exited(code) => *code,
                _ => None,
            },
            "health": match self.server.health {
                ServerHealth::Healthy => "healthy",
                ServerHealth::Unavailable => "unavailable",
                ServerHealth::NotChecked => "not_checked",
            },
            "elapsed_ms": self.elapsed_ms,
            "first_event_ms": self.first_event_ms,
            "first_token_ms": self.first_token_ms,
            "last_data_age_ms": self.last_data_age_ms,
            "raw_lines": self.raw_lines,
            "events": self.events,
            "bytes": self.bytes,
            "output_tokens": self.output_tokens,
            "reasoning_tokens": self.reasoning_tokens,
            "finish_reason": self.finish_reason,
        })
    }
}

fn optional_ms(value: Option<u128>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "нет".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    #[test]
    fn server_trace_is_bounded_and_prioritizes_errors() {
        let trace = ServerTrace::new();
        trace.begin_process(42);
        for index in 0..90 {
            trace.push_stderr(format!("recent {}", index));
        }
        trace.push_stderr("CUDA error: failed".to_string());
        for index in 90..100 {
            trace.push_stderr(format!("recent {}", index));
        }
        let lines = trace.diagnostic_lines();
        assert_eq!(trace.pid(), Some(42));
        assert!(lines.iter().any(|line| line.contains("CUDA error")));
        assert!(lines.len() <= 50);
    }

    #[test]
    fn response_meta_ignores_untrusted_and_secret_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("addr");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100\r\nConnection: close\r\nSet-Cookie: secret\r\nX-Api-Key: secret\r\n\r\ndata: x\n",
                )
                .expect("write");
        });
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("client")
            .get(format!("http://{}/v1/chat/completions", address))
            .send()
            .expect("response");
        let meta = ResponseMeta::from_response(&response);
        server.join().expect("server");
        let formatted = meta.format();
        assert!(formatted.contains("length=100"));
        assert!(!formatted.contains("secret"));
        let mut reader = BufReader::new(response);
        let mut line = String::new();
        let error = loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => panic!("truncated body returned EOF without error"),
                Ok(_) => {}
                Err(read_error) => break read_error,
            }
        };
        let typed = StreamReaderError::from_io(&error);
        assert!(
            typed.message.contains("end of file before message length reached"),
            "{}",
            typed.message
        );
    }

    #[test]
    fn diagnostics_format_contains_only_safe_fields() {
        let diagnostics = StreamDiagnostics {
            reason: "Ошибка чтения потока генерации".to_string(),
            response: ResponseMeta {
                status: 200,
                http_version: "HTTP/1.1".to_string(),
                content_type: Some("text/event-stream".to_string()),
                content_length: Some(100),
                transfer_encoding: None,
                connection: Some("close".to_string()),
            },
            reader_error: Some(StreamReaderError {
                message: "error decoding response body".to_string(),
                kind: "Other".to_string(),
                raw_os_error: None,
            }),
            server: ServerProcessSnapshot {
                pid: Some(42),
                process: ServerProcessState::Running,
                health: ServerHealth::Healthy,
            },
            elapsed_ms: 30000,
            first_event_ms: None,
            first_token_ms: None,
            last_data_age_ms: None,
            raw_lines: 0,
            events: 0,
            bytes: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            finish_reason: "нет".to_string(),
            stderr_lines: vec!["llama-server alive".to_string()],
        };
        let formatted = diagnostics.format();
        assert!(formatted.contains("STREAM_DIAGNOSTIC"));
        assert!(formatted.contains("response_body_truncated"));
        assert!(formatted.contains("process=running"));
        assert!(formatted.contains("first_token_ms=нет"));
        assert!(!formatted.contains("Authorization"));
        assert!(!formatted.contains("api_key"));
    }
}

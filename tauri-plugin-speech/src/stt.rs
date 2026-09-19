use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use reqwest::Client;
use tauri::{AppHandle, Runtime};

use crate::process_util::{kill_process_tree, JobGuard};

use crate::stt_settings::SttSettings;

const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);

/// Состояние STT daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttStatus {
    Stopped,
    Starting,
    Listening,
    Recording,
    Transcribing,
    Error(String),
}

/// Движок STT на базе CrispASR (gigaam backend).
///
/// CrispASR запускается как **отдельный prebuilt-процесс** (`crispasr.exe`) и общается
/// по WebSocket localhost (`--ws-port`). Нативная линковка запрещена глобальным правилом.
///
/// Это движковая часть STT (spawn/stop/status/ws_port). Оверлей (хоткей, микрофон,
/// push-to-talk стейт-машина) — ответственность хоста, в плагин не входит.
pub struct SttEngine {
    child: Mutex<Option<Child>>,
    job: Mutex<Option<JobGuard>>,
    ws_port: Mutex<u16>,
    status: Mutex<SttStatus>,
}

impl SttEngine {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            job: Mutex::new(None),
            ws_port: Mutex::new(0),
            status: Mutex::new(SttStatus::Stopped),
        }
    }

    fn pick_port() -> Option<u16> {
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let port = listener.local_addr().ok()?.port();
        Some(port)
    }

    fn kill_now(mut child: Child, job: Option<JobGuard>) {
        if let Some(j) = &job {
            j.terminate();
        }
        kill_process_tree(&mut child);
    }

    fn reap(&self) {
        let child = self
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let job = self.job.lock().unwrap_or_else(|e| e.into_inner()).take();
        match child {
            Some(c) => Self::kill_now(c, job),
            None => drop(job),
        }
    }

    pub fn ws_port(&self) -> u16 {
        *self.ws_port.lock().unwrap()
    }

    pub fn status(&self) -> SttStatus {
        self.status.lock().unwrap().clone()
    }

    /// Запускает сервер CrispASR для STT (gigaam backend, WebSocket).
    pub async fn ensure<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        settings: &SttSettings,
    ) -> Result<u16, String> {
        let engine_exe = if settings.engine_exe.is_empty() {
            crate::download::resolve_engine_exe(
                &crate::tts_settings::load(app).engine_dir,
                "cpu",
            )
            .to_string_lossy()
            .to_string()
        } else {
            settings.engine_exe.clone()
        };

        if engine_exe.is_empty() || !std::path::Path::new(&engine_exe).exists() {
            return Err("crispasr.exe not found (configure path in Settings -> TTS)".into());
        }

        // If already running with same port, reuse
        let running = self.child.lock().unwrap().is_some();
        if running && *self.ws_port.lock().unwrap() != 0 {
            return Ok(*self.ws_port.lock().unwrap());
        }
        if running {
            self.stop(app).await;
        }

        *self.status.lock().unwrap() = SttStatus::Starting;
        crate::stt_events::emit_status(app, &self.status.lock().unwrap());

        let ws_port = if settings.ws_port != 0 {
            settings.ws_port
        } else {
            Self::pick_port().ok_or_else(|| "failed to pick free port".to_string())?
        };

        let mut args = vec![
            "--server".into(),
            "--backend".into(),
            settings.backend.clone(),
            "-m".into(),
            settings.model.clone(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            ws_port.to_string(),
            "--ws-port".into(),
            ws_port.to_string(),
            "--no-watermark".into(),
            "--accept-marking-responsibility".into(),
            "--no-spoken-disclaimer".into(),
        ];

        if settings.vad {
            args.push("--vad".into());
        }

        // Streaming params
        args.push("--stream-step".into());
        args.push(settings.stream_step_ms.to_string());
        args.push("--stream-length".into());
        args.push(settings.stream_length_ms.to_string());

        crate::log::app_log(
            app,
            &format!("[stt] starting CrispASR: {} {:?}", engine_exe, args),
        );

        let mut cmd = Command::new(&engine_exe);
        cmd.args(&args);
        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        cmd.stdout(Stdio::null()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to start crispasr: {e}"))?;

        let job = JobGuard::assign(&child);

        // Capture stderr → logs
        let err_log = Arc::new(Mutex::new(Vec::<String>::new()));
        if let Some(stderr) = child.stderr.take() {
            let app_clone = app.clone();
            let err_log_clone = Arc::clone(&err_log);
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    let clean = crate::tts::sanitize_crispasr_line(&line);
                    crate::log::app_log(&app_clone, &format!("[stt] {clean}"));
                    if let Ok(mut buf) = err_log_clone.lock() {
                        if buf.len() < 200 {
                            buf.push(clean);
                        }
                    }
                }
            });
        }

        // Health check loop
        let client = Client::new();
        let health_url = format!("http://127.0.0.1:{ws_port}/health");
        let started = std::time::Instant::now();
        let mut ready = false;
        while started.elapsed() < HEALTH_TIMEOUT {
            if let Ok(Some(_)) = child.try_wait() {
                let log = err_log.lock().unwrap();
                let joined = log.join("\n");
                return Err(format!(
                    "CrispASR process exited immediately. Log: {joined}"
                ));
            }
            if let Ok(resp) = client
                .get(&health_url)
                .timeout(Duration::from_secs(2))
                .send()
                .await
            {
                if resp.status().is_success() {
                    ready = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        if !ready {
            Self::kill_now(child, job);
            return Err("timeout waiting for CrispASR health (120s)".into());
        }

        *self.child.lock().unwrap() = Some(child);
        *self.job.lock().unwrap() = job;
        *self.ws_port.lock().unwrap() = ws_port;
        *self.status.lock().unwrap() = SttStatus::Listening;
        crate::stt_events::emit_status(app, &self.status.lock().unwrap());

        crate::log::app_log(
            app,
            &format!("[stt] CrispASR ready on ws port {ws_port}"),
        );

        Ok(ws_port)
    }

    pub async fn stop<R: Runtime>(&self, app: &AppHandle<R>) {
        self.reap();
        *self.ws_port.lock().unwrap() = 0;
        *self.status.lock().unwrap() = SttStatus::Stopped;
        crate::stt_events::emit_status(app, &self.status.lock().unwrap());
    }

    /// Оффлайн batch-распознавание одного WAV-файла через HTTP
    /// (`POST /v1/audio/transcriptions`, multipart). Работает с ЛЮБЫМ бэкендом
    /// (не только whisper) — в отличие от WS-стриминга. Возвращает распознанный
    /// текст (response_format=json, поле `text`).
    ///
    /// `language` — ISO-639-1 код (например "en"); пустая строка = язык сервера.
    pub async fn transcribe(&self, wav_path: &str, language: &str) -> Result<String, String> {
        let port = *self.ws_port.lock().unwrap();
        if port == 0 {
            return Err("STT движок не запущен (вызовите ensure перед transcribe)".into());
        }
        let wav_path = std::path::Path::new(wav_path);
        if !wav_path.exists() {
            return Err(format!("STT: файл не найден: {}", wav_path.display()));
        }
        let bytes = std::fs::read(wav_path)
            .map_err(|e| format!("STT: не удалось прочитать {}: {e}", wav_path.display()))?;
        if bytes.len() < 44 {
            return Err("STT: файл слишком мал (не WAV?)".into());
        }

        let file_name = wav_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("segment.wav")
            .to_string();
        let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
        let mut form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("response_format", "json");
        if !language.trim().is_empty() {
            form = form.text("language", language.trim().to_string());
        }

        let url = format!("http://127.0.0.1:{port}/v1/audio/transcriptions");
        let client = Client::new();
        let resp = client
            .post(&url)
            .multipart(form)
            .timeout(Duration::from_secs(180))
            .send()
            .await
            .map_err(|e| format!("STT: ошибка запроса: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            return Err(format!("STT: сервер вернул {status}: {detail}"));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("STT: не удалось разобрать ответ: {e}"))?;
        let text = json["text"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        match text {
            Some(t) => Ok(t),
            None => Err(format!(
                "STT: пустой текст в ответе сервера: {}",
                json
            )),
        }
    }
}

impl Default for SttEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SttEngine {
    fn drop(&mut self) {
        self.reap();
    }
}
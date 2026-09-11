use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use reqwest::Client;
use tauri::{AppHandle, Emitter, Runtime};

use crate::process_util::{kill_process_tree, JobGuard};

use crate::download::preset_by_id;

const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);

/// Движок TTS на базе CrispASR.
///
/// CrispASR запускается как **отдельный prebuilt-процесс** (`crispasr.exe`) и общается
/// по HTTP localhost (OpenAI-совместимый `POST /v1/audio/speech`). Нативная линковка
/// крейта `crispasr` запрещена глобальным правилом `desktop_rust_tauri/rules.md §6.7`.
///
/// Всегда 100% локально: без API-ключей, без облака (требование пользователя).
pub struct TtsEngine {
    child: Mutex<Option<Child>>,
    /// Job Object (Windows), гарантирующий зачистку дерева процессов при выходе.
    job: Mutex<Option<JobGuard>>,
    port: Mutex<u16>,
    loaded_model: Mutex<String>,
    loaded_codec: Mutex<String>,
    loaded_voice: Mutex<String>,
    loaded_backend: Mutex<String>,
    /// Поддерживает ли загруженная модель явный выбор языка (`language`).
    /// Определяется политикой `backend_supports_language_param`. `Arc`, чтобы
    /// делить флаг между потоком перехвата stderr и async-методами.
    language_supported: Arc<Mutex<bool>>,
}

/// Собирает аргументы командной строки запуска сервера CrispASR.
///
/// **Архитектурная точка отключения watermark:** флаг `--no-watermark` добавляется
/// сюда безусловно, поэтому применяется ко ВСЕМ моделям/бэкендам (ensure() — единственный
/// spawn движка). `--no-spoken-disclaimer` добавлен для страховки. ВАЖНО: этот флаг
/// недостаточен сам по себе, если в теле запроса `POST /v1/audio/speech` присутствует
/// `consent_attestation` (оно заставляет движок вернуть слышимый дисклеймер) — поэтому
/// поле consent_attestation убрано из `build_speech_body`.
fn engine_launch_args(
    backend: &str,
    model: &str,
    codec_path: &str,
    port: u16,
    voice_dir: &str,
    startup_voice: &str,
) -> Vec<String> {
    let mut a = vec![
        "--server".to_string(),
        "--backend".to_string(),
        backend.to_string(),
        "-m".to_string(),
        model.to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        port.to_string(),
    ];
    if !codec_path.is_empty() {
        a.push("--codec-model".to_string());
        a.push(codec_path.to_string());
    }
    a.push("--no-watermark".to_string());
    // CrispASR не стартует с --no-watermark без явного принятия ответственности
    // за маркировку ИИ-контента оператором (приложением). Без этого флага сервер
    // падает сразу ("Refusing to start").
    a.push("--accept-marking-responsibility".to_string());
    a.push("--no-spoken-disclaimer".to_string());
    a.push("--voice-dir".to_string());
    a.push(voice_dir.to_string());
    if !startup_voice.is_empty() {
        a.push("--voice".to_string());
        a.push(startup_voice.to_string());
    }
    a
}

/// Строит JSON-тело запроса к `POST /v1/audio/speech`.
///
/// `clone` — идёт ли синтез с клонированным голосом (референс из WAV/GGUF).
/// В этом случае ОБЯЗАТЕЛЬНО добавляем `consent_attestation`: API движка
/// требует это поле для клонирования (иначе 400 `consent_required`, см. логи
/// chatterbox). Для обычных (не-clone) запросов поле НЕ добавляется — поведение
/// полностью идентично прежнему, никаких водяных знаков/дисклеймеров не добавляется.
fn build_speech_body(
    backend: &str,
    text: &str,
    voice: &str,
    instructions: &str,
    ref_text: &str,
    speed: f32,
    clone: bool,
    language: &str,
) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert("model".into(), serde_json::Value::String(backend.to_string()));
    body.insert("input".into(), serde_json::Value::String(text.to_string()));
    body.insert(
        "response_format".into(),
        serde_json::Value::String("mp3".into()),
    );
    if !voice.is_empty() {
        body.insert("voice".into(), serde_json::Value::String(voice.to_string()));
    }
    if !instructions.is_empty() {
        body.insert(
            "instructions".into(),
            serde_json::Value::String(instructions.to_string()),
        );
    }
    if !ref_text.is_empty() {
        body.insert(
            "ref_text".into(),
            serde_json::Value::String(ref_text.to_string()),
        );
    }
    if (speed - 1.0).abs() > f32::EPSILON {
        body.insert("speed".into(), serde_json::Value::from(speed));
    }
    if clone {
        body.insert(
            "consent_attestation".into(),
            serde_json::Value::String("I confirm I have the legal right to clone this voice.".into()),
        );
    }
    if !language.is_empty() {
        body.insert("language".into(), serde_json::Value::String(language.to_string()));
    }
    serde_json::Value::Object(body)
}

impl TtsEngine {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            job: Mutex::new(None),
            port: Mutex::new(0),
            loaded_model: Mutex::new(String::new()),
            loaded_codec: Mutex::new(String::new()),
            loaded_voice: Mutex::new(String::new()),
            loaded_backend: Mutex::new(String::new()),
            language_supported: Arc::new(Mutex::new(true)),
        }
    }

    /// Возвращает держит ли движок тот же набор моделей/голоса (для переиспользования).
    fn same_model(&self, model: &str, codec: &str, voice: &str, backend: &str) -> bool {
        let m = self.loaded_model.lock().unwrap();
        let c = self.loaded_codec.lock().unwrap();
        let v = self.loaded_voice.lock().unwrap();
        let b = self.loaded_backend.lock().unwrap();
        m.as_str() == model && c.as_str() == codec && v.as_str() == voice && b.as_str() == backend
    }

    fn pick_port() -> Option<u16> {
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let port = listener.local_addr().ok()?.port();
        Some(port)
    }

    /// Убивает движок: дёргает Job Object (весь процесс-трий целиком) и дублирует
    /// `kill_process_tree` (`taskkill /F /T` до репарентинга). `job` сбрасывается
    /// при выходе из области видимости (закрытие хендла → KILL_ON_JOB_CLOSE).
    fn kill_now(mut child: Child, job: Option<JobGuard>) {
        if let Some(j) = &job {
            j.terminate();
        }
        kill_process_tree(&mut child);
    }

    /// Берёт child+job из мьютексов (устойчиво к poison) и убивает движок.
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

    /// Запускает (или переиспользуется) сервер CrispASR для заданных model/codec/backend.
    ///
    /// Голос типа "ggupack" (GGUF voice-пак) грузится при старте через `--voice <путь>`,
    /// т.к. передача GGUF-пака в теле запроса сервер трактует как клонирование и требует
    /// `consent_attestation`. Голоса типа "named"/"clone" передаются в теле запроса.
    ///
    /// Логи stderr движка перенаправляются в Логи (событие `app-log` с префиксом
    /// `[crispasr]`), чтобы пользователь видел причину падения.
    pub async fn ensure<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        engine_exe: &str,
        backend: &str,
        models_dir: &str,
        preset_id: &str,
        startup_voice: &str,
    ) -> Result<(), String> {
        if engine_exe.is_empty() {
            return Err("не указан путь к движку crispasr.exe (откройте Настройки → ТТС)".into());
        }

        let preset = preset_by_id(preset_id)
            .ok_or_else(|| format!("неизвестный пресет TTS: {preset_id}"))?;

        let base = if models_dir.is_empty() {
            crate::download::default_models_dir()
        } else {
            std::path::PathBuf::from(models_dir)
        };
        let folder = base.join(preset_id);

        let model = folder.join(&preset.model_file);
        if !model.exists() {
            return Err(format!(
                "модель не найдена: {} (скачайте пресет в Настройках)",
                model.display()
            ));
        }

        let mut codec_path = String::new();
        if let Some((cf, _)) = &preset.codec {
            let p = folder.join(cf);
            if !p.exists() {
                return Err(format!("codec-модель не найдена: {}", p.display()));
            }
            codec_path = p.to_string_lossy().to_string();
        }

        let mut startup_voice_path = String::new();
        if !startup_voice.is_empty() {
            if !std::path::Path::new(startup_voice).exists() {
                return Err(format!("файл голоса не найден: {startup_voice}"));
            }
            startup_voice_path = startup_voice.to_string();
        }

        let running = self.child.lock().unwrap().is_some();
        if running && self.same_model(
            &model.to_string_lossy(),
            &codec_path,
            &startup_voice_path,
            backend,
        ) {
            return Ok(());
        }
        if running {
            self.stop().await;
        }

        if !std::path::Path::new(engine_exe).exists() {
            return Err(format!("движок не найден по пути: {engine_exe}"));
        }

        let port = Self::pick_port().ok_or_else(|| "не удалось выбрать свободный порт".to_string())?;

        let mut cmd = Command::new(engine_exe);
        let voice_dir = crate::voices::voices_root(models_dir);
        cmd.args(engine_launch_args(
            backend,
            &model.to_string_lossy(),
            &codec_path,
            port,
            &voice_dir.to_string_lossy(),
            &startup_voice_path,
        ));
        // Watermark дублируем через env (на случай старых версий бинаря, игнорирующих
        // флаг --no-watermark).
        cmd.env("CRISPASR_NO_WATERMARK", "1");
        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        cmd.stdout(Stdio::null()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("не удалось запустить движок '{engine_exe}': {e}"))?;

        // Назначаем процесс в Job Object (Windows): гарантирует зачистку всего
        // дерева процессов даже при насильственном закрытии приложения.
        let job = JobGuard::assign(&child);

        // Перенаправляем stderr движка в Логи + буфер (для понятных ошибок запуска).
        let err_log = Arc::new(Mutex::new(Vec::<String>::new()));
        if let Some(stderr) = child.stderr.take() {
            let app_clone = app.clone();
            let err_log_clone = Arc::clone(&err_log);
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    let clean = sanitize_crispasr_line(&line);
                    crate::log::app_log(&app_clone, &format!("[crispasr] {clean}"));
                    if let Ok(mut buf) = err_log_clone.lock() {
                        if buf.len() < 200 {
                            buf.push(clean.clone());
                        }
                    }
                }
            });
        }

        // Ждём готовности сервера (без авторизации).
        let client = Client::new();
        let health = format!("http://127.0.0.1:{port}/health");
        let started = std::time::Instant::now();
        let mut ready = false;
        while started.elapsed() < HEALTH_TIMEOUT {
            if let Ok(Some(_)) = child.try_wait() {
                let log = err_log.lock().unwrap();
                let joined = log.join("\n");
                if joined.contains("unknown backend") {
                    return Err(format!(
                        "Модель '{preset_id}' не поддерживается текущей версией движка CrispASR (unknown backend). Возможно, этот бэкенд ещё не реализован в установленной сборке — выберите другую модель или обновите движок TTS."
                    ));
                }
                return Err("процесс движка завершился сразу (см. Логи: ошибка запуска/модели)".into());
            }
            if let Ok(resp) = client
                .get(&health)
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
            return Err("таймаут ожидания готовности движка (120 c)".into());
        }

        // Язык: детерминированная политика по документации CrispASR.
        let lang_ok = backend_supports_language_param(backend);
        *self.language_supported.lock().unwrap() = lang_ok;
        emit_tts_caps(app, lang_ok);

        *self.child.lock().unwrap() = Some(child);
        *self.job.lock().unwrap() = job;
        *self.port.lock().unwrap() = port;
        *self.loaded_model.lock().unwrap() = model.to_string_lossy().to_string();
        *self.loaded_codec.lock().unwrap() = codec_path;
        *self.loaded_voice.lock().unwrap() = startup_voice_path;
        *self.loaded_backend.lock().unwrap() = backend.to_string();
        Ok(())
    }

    /// Синтезирует текст в MP3 и возвращает сырые байты.
    ///
    /// `voice` — имя спикера или путь к WAV для клонирования (передаётся в теле запроса,
    /// per-request; может быть пустым). Поле `consent_attestation` отправляется ТОЛЬКО
    /// когда `clone == true` (клонирование голоса). `language` — язык синтеза ("ru"/"en");
    /// пустая строка → авто-язык движка.
    pub async fn speak(
        &self,
        text: &str,
        voice: &str,
        instructions: &str,
        ref_text: &str,
        speed: f32,
        clone: bool,
        language: &str,
    ) -> Result<Vec<u8>, String> {
        let port = *self.port.lock().unwrap();
        let backend = self.loaded_backend.lock().unwrap().clone();
        if port == 0 {
            return Err("движок не запущен (вызовите ensure перед speak)".into());
        }

        let client = Client::new();
        let url = format!("http://127.0.0.1:{port}/v1/audio/speech");

        let effective_language = if *self.language_supported.lock().unwrap() {
            language.to_string()
        } else {
            String::new()
        };
        let body_value = build_speech_body(
            &backend,
            text,
            voice,
            instructions,
            ref_text,
            speed,
            clone,
            &effective_language,
        );

        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body_value)
            .send()
            .await
            .map_err(|e| format!("ошибка запроса TTS: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            return Err(format!("TTS ошибка {status}: {detail}"));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("ошибка чтения ответа TTS: {e}"))?;
        Ok(bytes.to_vec())
    }

    /// Регистрирует кастомный голос в запущенном сервере CrispASR и возвращает
    /// имя, под которым сервер его знает (ASCII, см. `ascii_voice_name`).
    ///
    /// Возвращает кортеж `(имя_на_сервере, был_ли_загружен_клон)`.
    pub async fn ensure_voice<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        models_dir: &str,
        voice_id: &str,
    ) -> Result<(String, bool), String> {
        if voice_id.is_empty() {
            return Ok((String::new(), false));
        }
        let root = crate::voices::voices_root(models_dir);
        let wav = root.join(voice_id).join("voice.wav");
        if !wav.exists() {
            return Ok((voice_id.to_string(), false));
        }
        let server_name = ascii_voice_name(voice_id);
        if self.voice_registered(&server_name).await {
            return Ok((server_name, true));
        }
        let txt = root.join(voice_id).join("ref_text.txt");
        let transcript = if txt.exists() {
            std::fs::read_to_string(&txt).unwrap_or_default()
        } else {
            String::new()
        };
        let (mono, rate) = crate::audio::decode_to_mono(&wav.to_string_lossy())
            .map_err(|e| format!("не удалось декодировать голос {}: {e}", wav.display()))?;
        let (mono, rate) = crate::voices::to_24k_mono(mono, rate);
        let tmp = root.join(voice_id).join("voice_24k.wav");
        crate::audio::wav::write_wav(&tmp.to_string_lossy(), &mono, rate)
            .map_err(|e| format!("не удалось записать 24кГц референс {}: {e}", tmp.display()))?;
        let bytes = std::fs::read(&tmp)
            .map_err(|e| format!("не удалось прочитать файл голоса {}: {e}", tmp.display()))?;
        let _ = std::fs::remove_file(&tmp);
        self.upload_voice(app, &server_name, &bytes, &transcript).await?;
        Ok((server_name, true))
    }

    /// Спрашивает сервер, зарегистрирован ли голос с именем `name` (GET /v1/voices).
    async fn voice_registered(&self, name: &str) -> bool {
        let port = *self.port.lock().unwrap();
        if port == 0 {
            return false;
        }
        let client = Client::new();
        let url = format!("http://127.0.0.1:{port}/v1/voices");
        if let Ok(resp) = client
            .get(&url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
        {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = json.get("voices").and_then(|v| v.as_array()) {
                    return arr
                        .iter()
                        .any(|v| v.get("name").and_then(|n| n.as_str()) == Some(name));
                }
            }
        }
        false
    }

    /// Загружает wav-голос в сервер (multipart POST /v1/voices).
    async fn upload_voice<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        name: &str,
        bytes: &[u8],
        transcript: &str,
    ) -> Result<(), String> {
        let port = *self.port.lock().unwrap();
        if port == 0 {
            return Err("движок не запущен (вызовите ensure перед speak)".into());
        }
        let client = Client::new();
        let url = format!("http://127.0.0.1:{port}/v1/voices?force=true");
        let part = reqwest::multipart::Part::bytes(bytes.to_vec())
            .file_name(format!("{name}.wav"));
        let mut form = reqwest::multipart::Form::new()
            .part("voice", part)
            .text("name", name.to_string())
            .text(
                "consent_attestation",
                "I confirm I have the legal right to clone this voice.",
            );
        if !transcript.trim().is_empty() {
            form = form.text("transcript", transcript.trim().to_string());
        }
        let resp = client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("ошибка регистрации голоса «{name}»: {e}"))?;
        if !resp.status().is_success() {
            let detail = resp.text().await.unwrap_or_default();
            return Err(format!("не удалось зарегистрировать голос «{name}»: {detail}"));
        }
        crate::log::app_log(
            app,
            &format!("[voices] голос «{name}» зарегистрирован в движке TTS"),
        );
        Ok(())
    }

    /// Останавливает движок (выгрузка моделей из VRAM).
    pub async fn stop(&self) {
        self.reap();
        *self.loaded_model.lock().unwrap() = String::new();
        *self.loaded_codec.lock().unwrap() = String::new();
        *self.loaded_voice.lock().unwrap() = String::new();
        *self.loaded_backend.lock().unwrap() = String::new();
        // Сбрасываем в оптимистичное значение: следующий ensure перепроверит.
        *self.language_supported.lock().unwrap() = true;
    }

    /// Возвращает, поддерживает ли загруженная модель явный выбор языка.
    pub fn supports_language(&self) -> bool {
        *self.language_supported.lock().unwrap()
    }
}

/// Уведомляет фронт об актуальных возможностях загруженного движка TTS.
fn emit_tts_caps<R: Runtime>(app: &AppHandle<R>, language: bool) {
    let _ = app.emit(
        "tts-caps",
        serde_json::json!({ "language": language }),
    );
}

/// Возвращает, принимает ли бэкенд явный выбор языка (`language` в
/// `POST /v1/audio/speech`).
///
/// По документации CrispASR language-agnostic (читают скрипт текста сами и
/// не нуждаются в поле) только: `voxcpm2`, `f5-tts`, `vibevoice`. Все остальные
/// либо кондиционируют язык, либо игнорируют поле без ошибки. Отправка `language`
/// «неподдерживающему» бэкенду безопасна — поэтому список ограничен только явно
/// language-agnostic моделями.
pub fn backend_supports_language_param(backend: &str) -> bool {
    !matches!(
        backend,
        "voxcpm2" | "voxcpm2-tts" | "f5-tts" | "vibevoice" | "vibevoice-tts"
    )
}

impl Default for TtsEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TtsEngine {
    fn drop(&mut self) {
        self.reap();
    }
}

/// Убирает ANSI-эскейп-последовательности и лишние `\r` из stderr движка, чтобы
/// `test/last_logs` оставался чистым текстом.
pub fn sanitize_crispasr_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // ANSI CSI: ESC [ ... <буква>. Пропускаем целиком.
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if c == '\r' {
            continue;
        }
        out.push(c);
    }
    out
}

/// Серверное имя голоса (для `POST /v1/voices` / поля `voice` в `/v1/audio/speech`).
///
/// CrispASR требует `name` по регэкспу `[a-zA-Z0-9_-]+` (только ASCII).
/// Пользовательские id могут содержать кириллицу/пробелы — такие имена сервер
/// отвергает (400). Поэтому: ASCII-фолдим (alnum→lower, `-` оставляем, остальное→`_`),
/// и если после фолда ничего не осталось (чисто не-ASCII id, напр. «Влад_без_текста»)
/// — берём стабильный ASCII-хэш FNV-1a, чтобы имя было уникальным и воспроизводимым.
pub(crate) fn ascii_voice_name(id: &str) -> String {
    let folded: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c == '-' {
                '-'
            } else {
                '_'
            }
        })
        .collect();
    let trimmed: String = folded.trim_matches('_').to_string();
    if !trimmed.is_empty() && trimmed.chars().any(|c| c.is_ascii_alphanumeric()) {
        trimmed
    } else {
        format!("v{:x}", fnv64(id.as_bytes()))
    }
}

fn fnv64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_ansi_and_cr() {
        let raw = "\r\x1b[31merror\x1b[0m: boom\r\n";
        let clean = sanitize_crispasr_line(raw);
        assert!(!clean.contains('\u{1b}'), "ANSI не удалён: {clean:?}");
        assert!(!clean.contains('\r'), "CR не удалён: {clean:?}");
        assert_eq!(clean, "error: boom\n");
    }

    #[test]
    fn ascii_voice_name_rules() {
        assert_eq!(ascii_voice_name("MorganFreeman"), "morganfreeman");
        assert_eq!(ascii_voice_name("voice-1"), "voice-1");
        assert_eq!(ascii_voice_name("My Voice"), "my_voice");
        let cyr = ascii_voice_name("Влад_без_текста");
        assert!(cyr.chars().all(|c| c.is_ascii()), "не-ASCII имя: {cyr}");
        assert!(cyr.starts_with('v'));
        assert_eq!(cyr, ascii_voice_name("Влад_без_текста"));
    }

    #[test]
    fn launch_args_disable_watermark_for_all_models() {
        for backend in ["cosyvoice3-tts", "qwen3-tts-1.7b-base", "f5-tts", "kokoro"] {
            let args = engine_launch_args(backend, "model.gguf", "", 18000, "C:\\voices", "");
            assert!(
                args.iter().any(|a| a == "--no-watermark"),
                "нет --no-watermark для бэкенда {backend}"
            );
            assert!(
                args.iter().any(|a| a == "--accept-marking-responsibility"),
                "нет --accept-marking-responsibility для бэкенда {backend}"
            );
            assert!(
                args.iter().any(|a| a == "--no-spoken-disclaimer"),
                "нет --no-spoken-disclaimer для бэкенда {backend}"
            );
            assert!(!args.iter().any(|a| a.contains("consent")));
        }
        let args = engine_launch_args("cosyvoice3-tts", "m.gguf", "c.gguf", 18001, "C:\\v", "ref.wav");
        assert!(args.iter().any(|a| a == "--voice"));
        assert!(args.iter().any(|a| a == "--no-watermark"));
    }

    #[test]
    fn speech_body_has_no_consent_attestation() {
        let body = build_speech_body("cosyvoice3-tts", "привет", "voice1", "", "транскрипт", 1.0, false, "");
        let obj = body.as_object().unwrap();
        assert!(!obj.contains_key("consent_attestation"), "consent_attestation всё ещё в теле запроса — будет ватермарк");
        assert_eq!(obj.get("response_format").unwrap(), &serde_json::Value::String("mp3".into()));
        assert_eq!(obj.get("input").unwrap(), &serde_json::Value::String("привет".into()));
        assert!(!obj.contains_key("speed"));
        assert!(obj.contains_key("voice"));

        let body2 = build_speech_body("qwen3-tts", "hi", "", "", "", 1.5, false, "");
        let obj2 = body2.as_object().unwrap();
        assert!(!obj2.contains_key("consent_attestation"));
        assert!(!obj2.contains_key("voice"));
        assert!(obj2.contains_key("speed"));
    }
}
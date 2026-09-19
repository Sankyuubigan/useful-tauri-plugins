use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use reqwest::Client;
use tauri::{AppHandle, Emitter, Runtime};

use crate::process_util::{kill_process_tree, JobGuard};

use crate::download::preset_by_id;

const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);

/// Аттестация ответственности за маркировку ИИ-контента (поле `marking_attestation`
/// в POST /v1/audio/speech). Требуется движком, чтобы уважать `spoken_disclaimer: false`.
/// Приложение уже стартует сервер с `--accept-marking-responsibility`, поэтому этот
/// текст — honest подстраховка, а не новый уровень ответственности.
const MARKING_ATTESTATION: &str =
    "The operator of this application accepts responsibility for AI-marking of the generated audio.";

/// Возвращает true, если строка stderr движка — маркер runaway («модель не выдала
/// EOS, результат должен быть отброшен»). Считаем по сигнатурам из qwen3_tts.cpp:
/// `text-proportional frame cap ... without emitting EOS` и
/// `ran to the KV ceiling without emitting EOS`, обе содержат "runaway".
fn is_runaway_line(line: &str) -> bool {
    line.contains("runaway") || line.contains("without emitting EOS")
}

/// Тайминг синтеза из лог-строки движка
/// `crispasr-server: synthesized X.Xs audio in Y.YYs (RTF=Z.ZZ) ...`.
///
/// Это **чистая генерация** по докам CrispASR (docs/benchmarking.md): в замер движка
/// не входят запуск процесса, загрузка GGUF, прогрев, post-обработка (ресемпл/
/// вотермарк/энкодинг `response_format`) и HTTP. Холодный первый запрос после
/// старта движка всё же включает ленивую загрузку сателлитов (campplus/s3tok
/// у cosyvoice3), поэтому эталонный прогон отбрасывает первый (cold) запрос.
#[derive(Clone, Copy, Debug)]
pub struct SynthTiming {
    /// Длительность синтезированного аудио в секундах (сообщает движок).
    pub audio_secs: f64,
    /// Время синтеза по самому движку (без холодного старта), секунд.
    pub gen_secs: f64,
}

impl SynthTiming {
    /// RTF = длительность аудио / время синтеза. `< 1.0` — быстрее реального времени.
    pub fn rtf(&self) -> f64 {
        if self.gen_secs <= 0.0 {
            0.0
        } else {
            self.audio_secs / self.gen_secs
        }
    }
}

/// Парсит `SynthTiming` из строки сервера на обеих ветках:
/// `synthesized 3.6s audio in 0.56s (RTF=0.16) voice='...'` (TTS-бэкенды).
fn parse_synth_timing(line: &str) -> Option<SynthTiming> {
    let rest = line.split_once("synthesized ")?.1;
    let audio = rest.split_once("s audio in ")?.0.trim().parse::<f64>().ok()?;
    let gen = rest
        .split_once("s audio in ")?
        .1
        .split_once("s (")?
        .0
        .trim()
        .parse::<f64>()
        .ok()?;
    Some(SynthTiming {
        audio_secs: audio,
        gen_secs: gen,
    })
}

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
    /// Счётчик runaway-генераций движка (строки stderr вида «...runaway...»).
    /// Монотонный `AtomicUsize`: speak() снимает снапшот ДО запроса и после —
    /// если счётчик вырос, результат запроса это мусор и его нельзя отдавать
    /// пользователю. Сбрасывается при перезапуске движка в `ensure()`.
    runaways: Arc<AtomicUsize>,
    /// Последний замер синтеза из строки движка `synthesized ...` (чистая
    /// генерация). Обнуляется перед каждым запросом и при перезапуске.
    synth_timing: Arc<Mutex<Option<SynthTiming>>>,
}

/// Собирает аргументы командной строки запуска сервера CrispASR.
///
/// **Архитектурная точка отключения watermark:** флаг `--no-watermark` добавляется
/// сюда безусловно, поэтому применяется ко ВСЕМ моделям/бэкендам (ensure() — единственный
/// spawn движка). `--no-spoken-disclaimer` — страховка для версий движка, которые его
/// понимают; надёжное отключение слышимого дисклеймера лежит в теле POST-запроса
/// (`spoken_disclaimer: false` + `marking_attestation`, см. `build_speech_body`).
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
    // qwen3-tts: на GPU talker склонен дивергировать и уходить в runaway (issue #337
    // движка); CPU даёт детерминированную («эталонную») траекторию. ВАЖНО: runaway
    // воспроизводится и на CPU (логи от 2026-09-12: дисклеймер 624 кадра, текст 1812 —
    // оба без EOS), поэтому CPU не панацея, а default. Оставляем CPU осознанно:
    // стабильно-медленно лучше, чем рандомно-быстро с гарантированным мусором.
    // Хорошие результаты приходят из короткого референса (3–10 см) и выбором seed.
    if backend.starts_with("qwen3-tts") {
        a.push("--gpu-backend".to_string());
        a.push("cpu".to_string());
    }
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
/// Для clone-запросов движок ТРЕБУЕТ `consent_attestation` (иначе 400
/// `consent_required`). Плюс, чтобы не слушать 40-50-секундный слышимый
/// AI-дисклеймер перед каждым клоном, отправляем `spoken_disclaimer: false`
/// с `marking_attestation` (оператор уже принял ответственность на уровне CLI
/// флагом `--accept-marking-responsibility`). Для обычных (не-clone) запросов
/// поля пакета не добавляются — поле `consent_attestation` не нужно, а
/// `spoken_disclaimer` для них по умолчанию false в движке.
fn build_speech_body(
    backend: &str,
    text: &str,
    voice: &str,
    instructions: &str,
    ref_text: &str,
    speed: f32,
    clone: bool,
    language: &str,
    source_lang: &str,
    seed: Option<u64>,
) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert("model".into(), serde_json::Value::String(backend.to_string()));
    body.insert("input".into(), serde_json::Value::String(text.to_string()));
    body.insert(
        "response_format".into(),
        serde_json::Value::String("wav".into()),
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
            // Подавляем слышимый AI-дисклеймер (движок читает его голосом по
            // умолчанию для каждого клона перед синтезом — 40-50 c «болтовни»
            // в начале каждого ответа). Opt-out уважается только при наличии
            // marking_attestation; старые сборки движка могут его игнорировать —
            // тогда runaway-дисклеймер словит счётчик runaways и запрос не
            // дойдёт до пользователя (см. speak()).
            body.insert(
                "spoken_disclaimer".into(),
                serde_json::Value::Bool(false),
            );
            body.insert(
                "marking_attestation".into(),
                serde_json::Value::String(MARKING_ATTESTATION.into()),
            );
        }
    if !language.is_empty() {
        body.insert("language".into(), serde_json::Value::String(language.to_string()));
    }
    if !source_lang.is_empty() {
        // Язык, на котором говорит КЛОН-РЕФЕРЕНС (не выходной язык). cosyvoice3
        // по нему решает, нужен ли cross-lingual синтез (EN-референс → RU-озвучка).
        body.insert(
            "source_lang".into(),
            serde_json::Value::String(source_lang.to_string()),
        );
    }
    if let Some(seed) = seed {
        // Полный seed задаёт и RAS-семплер cosyvoice3, и talker-последовательность
        // (см. tts_backend_cosyvoice3: cosyvoice3_tts_set_seed). Повтор с другим
        // seed меняет генерацию; для cosyvoice3 это единственный способ повлиять
        // на стохастику без смены входов (см. retry в PASS3).
        body.insert("seed".into(), serde_json::Value::from(seed));
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
            runaways: Arc::new(AtomicUsize::new(0)),
            synth_timing: Arc::new(Mutex::new(None)),
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
        // Свежий процесс = свежий счётчик runaway. Обнуляем ДО старта потока,
        // чтобы не "унаследовать" 0 из предыдущего процесса некорректно.
        let runaways_counter = Arc::clone(&self.runaways);
        runaways_counter.store(0, Ordering::SeqCst);
        // Замер синтеза тоже живёт внутри конкретного процесса движка.
        *self.synth_timing.lock().unwrap() = None;
        let synth_timing_slot = Arc::clone(&self.synth_timing);
        if let Some(stderr) = child.stderr.take() {
            let app_clone = app.clone();
            let err_log_clone = Arc::clone(&err_log);
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    let clean = sanitize_crispasr_line(&line);
                    crate::log::app_log(&app_clone, &format!("[crispasr] {clean}"));
                    if is_runaway_line(&clean) {
                        runaways_counter.fetch_add(1, Ordering::SeqCst);
                    }
                    if let Some(t) = parse_synth_timing(&clean) {
                        if let Ok(mut slot) = synth_timing_slot.lock() {
                            *slot = Some(t);
                        }
                    }
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

    /// Синтезирует текст в WAV (24 кГц mono) и возвращает байты.
    ///
    /// `voice` — имя спикера или путь к WAV для клонирования (передаётся в теле запроса,
    /// per-request; может быть пустым). При `clone == true` в тело добавляются
    /// `consent_attestation` + `spoken_disclaimer:false` + `marking_attestation`
    /// (см. `build_speech_body`). `language` — язык синтеза ("ru"/"en"); пустая
    /// строка → авто-язык движка. `source_lang` — язык, на котором говорит
    /// клон-референс (для cross-lingual clone, напр. `en`); пустая строка → авто.
    ///
    /// Гарантия качества выхода: если движок в stderr пометил генерацию как runaway
    /// («talker не выдал EOS, результат отброшен»), байты НЕ возвращаются — вместо
    /// файла с «тишиной/мусором» приходит понятная ошибка.
    ///
/// Повторный запрос с другим seed здесь НЕ делается — решает вызывающая сторона
    /// (PASS3 пайплайна озвучки) через параметр `seed`, т.к. для qwen3-tts декод
    /// talker'а greedy (seed бесполезен), а для cosyvoice3 RAS-семплер seed-зависим
    /// (см. retry-логику в dubbing пипeline). Runaway — это дивергенция
    /// кондиционирования модели, а не случайный шум: его причина в основном входе
    /// (слишком длинный/неподходящий референс-голос, неоднозначный текст), и
    /// единственный путь к успеху — изменить вход (короткий референс 3–10 с),
    /// а не повторять тот же.
    ///
    /// `seed` — опциональный seed генерации (см. `build_speech_body`); `None` —
    /// движок использует свой дефолт.
    pub async fn speak(
        &self,
        text: &str,
        voice: &str,
        instructions: &str,
        ref_text: &str,
        speed: f32,
        clone: bool,
        language: &str,
        source_lang: &str,
        seed: Option<u64>,
    ) -> Result<(Vec<u8>, Option<SynthTiming>), String> {
        let port = *self.port.lock().unwrap();
        let backend = self.loaded_backend.lock().unwrap().clone();
        if port == 0 {
            return Err("движок не запущен (вызовите ensure перед speak)".into());
        }

        // Замер принадлежит ТОЛЬКО этому запросу: сбрасываем до отправки, чтобы
        // не подхватить строку синтеза от предыдущего запроса, запоздавшую в stderr.
        *self.synth_timing.lock().unwrap() = None;

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
            source_lang,
            seed,
        );

        let start_runaways = self.runaways.load(Ordering::SeqCst);

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

        let runaways_now = self.runaways.load(Ordering::SeqCst);
        if runaways_now > start_runaways {
            // Движок сам пометил эту генерацию как runaway и просил отбросить.
            // Отдавать пользователю этот «аудио» нельзя — это тишина/шум.
            return Err(
                "TTS: движок сгенерировал выброс (runaway: talker не выдал EOS, результат отброшен). \
                 Чаще всего причину дивергенции создаёт сам вход — используйте короткий референс-голос \
                 (3–10 секунд) и короткую однозначную фразу, затем попробуйте ещё раз."
                    .to_string(),
            );
        }

        Ok((bytes.to_vec(), self.take_synth_timing().await))
    }

    /// Ждёт строку синтеза движка (обычно приходит одновременно с ответом;
    /// stderr-поток может запаздывать на несколько мс). Возвращает и забирает замер.
    async fn take_synth_timing(&self) -> Option<SynthTiming> {
        for _ in 0..25 {
            let got = self.synth_timing.lock().unwrap().take();
            if got.is_some() {
                return got;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        None
    }

    /// Регистрирует голос из хранилища (папка `<id>/voice.wav`) в запущенном
    /// сервере CrispASR и возвращает имя, под которым сервер его знает (ASCII).
    ///
    /// Референс ОБЯЗАТЕЛЬНО проходит `prepare_clone_reference` (канон до
    /// `voices::MAX_REF_SEC`), загрузка принудительная (`?force=true`), чтобы
    /// сервер не держал старую длинную копию под тем же именем.
    pub async fn ensure_voice<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        models_dir: &str,
        voice_id: &str,
        backend: &str,
    ) -> Result<(String, bool), String> {
        if voice_id.is_empty() {
            return Ok((String::new(), false));
        }
        let root = crate::voices::voices_root(models_dir);
        let wav = root.join(voice_id).join("voice.wav");
        if !wav.exists() {
            return Ok((voice_id.to_string(), false));
        }
        let backend = if backend.is_empty() { "qwen3-tts" } else { backend };
        let cr = crate::clone::prepare_clone_reference(
            models_dir,
            &wav.to_string_lossy(),
            voice_id,
            backend,
        )?;
        let server_name = ascii_voice_name(voice_id);
        let bytes = std::fs::read(&cr.voice_path)
            .map_err(|e| format!("не удалось прочитать файл голоса {}: {e}", cr.voice_path))?;
        self.upload_voice(app, &server_name, &bytes, cr.ref_text.trim()).await?;
        Ok((server_name, true))
    }

    /// Регистрирует произвольный WAV-файл как голос сервера под ASCII-именем
    /// (для бэкендов, требующих транскрипт референса: qwen3-tts/tada), без
    /// копирования файла в хранилище голосов. `force = true` — всегда заново
    /// загружать файл на сервер (даже если имя уже зарегистрировано), чтобы
    /// гарантировать, что сервер отдаёт текущий (обрезанный) референс.
    pub async fn register_voice_file<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        src_wav: &str,
        transcript: &str,
        force: bool,
    ) -> Result<String, String> {
        if src_wav.is_empty() || !std::path::Path::new(src_wav).exists() {
            return Err(format!("файл голоса не найден: {src_wav}"));
        }
        let name = ascii_voice_name(src_wav);
        if !force && self.voice_registered(&name).await {
            return Ok(name);
        }
        let (mono, rate) = crate::audio::decode_to_mono(src_wav)
            .map_err(|e| format!("не удалось декодировать голос «{src_wav}»: {e}"))?;
        if mono.is_empty() {
            return Err(format!("голос «{src_wav}» пустой (тишина?)"));
        }
        let (mono, rate) = crate::voices::to_24k_mono(mono, rate);
        let tmp = std::env::temp_dir().join(format!("crispasr_voice_{name}.wav"));
        crate::audio::wav::write_wav(&tmp.to_string_lossy(), &mono, rate)
            .map_err(|e| format!("не удалось записать 24кГц референс: {e}"))?;
        let bytes = std::fs::read(&tmp)
            .map_err(|e| format!("не удалось прочитать файл голоса: {e}"))?;
        let _ = std::fs::remove_file(&tmp);
        self.upload_voice(app, &name, &bytes, transcript).await?;
        Ok(name)
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
        *self.synth_timing.lock().unwrap() = None;
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
    fn launch_args_qwen3_tts_use_cpu_backend() {
        for backend in ["qwen3-tts", "qwen3-tts-customvoice", "qwen3-tts-1.7b-base"] {
            let args = engine_launch_args(backend, "m.gguf", "c.gguf", 18001, "C:\\v", "ref.wav");
            let i = args.iter().position(|a| a == "--gpu-backend").unwrap_or_else(|| {
                panic!("нет --gpu-backend для {backend}");
            });
            assert_eq!(args[i + 1], "cpu", "ожидал cpu для {backend}");
        }
        let args = engine_launch_args("confucius4-tts", "m.gguf", "c.gguf", 18001, "C:\\v", "ref.wav");
        assert!(
            !args.iter().any(|a| a == "--gpu-backend"),
            "--gpu-backend не должен попадать в не-qwen3-tts бэкенды"
        );
    }

    #[test]
    fn speech_body_has_no_consent_attestation() {
        let body = build_speech_body("cosyvoice3-tts", "привет", "voice1", "", "транскрипт", 1.0, false, "", "", None);
        let obj = body.as_object().unwrap();
        assert!(!obj.contains_key("consent_attestation"), "consent_attestation всё ещё в теле запроса — будет ватермарк");
        assert_eq!(obj.get("response_format").unwrap(), &serde_json::Value::String("wav".into()));
        assert_eq!(obj.get("input").unwrap(), &serde_json::Value::String("привет".into()));
        assert!(!obj.contains_key("speed"));
        assert!(obj.contains_key("voice"));

        let body2 = build_speech_body("qwen3-tts", "hi", "", "", "", 1.5, false, "", "", None);
        let obj2 = body2.as_object().unwrap();
        assert!(!obj2.contains_key("consent_attestation"));
        assert!(!obj2.contains_key("voice"));
        assert!(obj2.contains_key("speed"));
    }

    #[test]
    fn speech_body_clone_requests_opt_out_disclaimer() {
        let body = build_speech_body("qwen3-tts", "Привет", "v123abc", "", "transcript text", 1.0, true, "ru", "", None);
        let obj = body.as_object().unwrap();
        // Клон без consent_attestation движок отклоняет (400 consent_required),
        // поэтому поле обязано быть.
        assert!(obj.contains_key("consent_attestation"));
        // Слышимый AI-дисклеймер для клона выключаем явно + с аттестацией.
        assert_eq!(
            obj.get("spoken_disclaimer").unwrap(),
            &serde_json::Value::Bool(false),
            "clone request должен просить spoken_disclaimer:false"
        );
        let marking = obj
            .get("marking_attestation")
            .expect("marking_attestation обязателен для opt-out")
            .as_str()
            .unwrap();
        assert!(!marking.is_empty());
    }

    #[test]
    fn speech_body_non_clone_keeps_clean_payload() {
        let body = build_speech_body("cosyvoice3-tts", "привет", "voice1", "", "", 1.0, false, "", "", None);
        let obj = body.as_object().unwrap();
        assert!(!obj.contains_key("consent_attestation"));
        assert!(!obj.contains_key("spoken_disclaimer"));
        assert!(!obj.contains_key("marking_attestation"));
    }

    #[test]
    fn speech_body_source_lang_for_cross_lingual_clone() {
        // Cross-lingual clone: EN-референс → RU-озвучка. source_lang обязан попасть в тело.
        let body = build_speech_body("cosyvoice3-tts", "Привет", "v123abc", "", "the reference transcript", 1.0, true, "ru", "en", None);
        let obj = body.as_object().unwrap();
        assert_eq!(obj.get("source_lang").unwrap(), &serde_json::Value::String("en".into()));
        assert_eq!(obj.get("language").unwrap(), &serde_json::Value::String("ru".into()));

        // Пустой source_lang не загрязняет тело.
        let body2 = build_speech_body("cosyvoice3-tts", "hi", "voice1", "", "", 1.0, false, "", "", None);
        let obj2 = body2.as_object().unwrap();
        assert!(!obj2.contains_key("source_lang"));
    }

    #[test]
    fn runaway_line_detection() {
        assert!(is_runaway_line(
            "qwen3_tts: ERROR: talker hit the text-proportional frame cap (624 frames for 52 input codepoints, 49.9 s of audio) without emitting EOS. The output is a runaway and should be discarded."
        ));
        assert!(is_runaway_line(
            "qwen3_tts: ERROR: talker ran to the KV ceiling without emitting EOS — stopped at frame 4095 (n_past=4683, 327.6 s of audio). The output is a runaway and should be discarded."
        ));
        assert!(!is_runaway_line("qwen3_tts: produced 105 frames × 16 codebooks = 1680 codes"));
        assert!(!is_runaway_line("crispasr-server: listening on 127.0.0.1:21644"));
    }

    #[test]
    fn parses_synth_timing_line() {
        let t = parse_synth_timing(
            "crispasr-server: synthesized 3.6s audio in 0.56s (RTF=6.43) voice='vlad' speed=1.00 format=wav model='cosyvoice3-tts' chunks=1 sr=24000Hz",
        )
        .expect("строка синтеза не распознана");
        assert!((t.audio_secs - 3.6).abs() < 1e-9, "audio={}", t.audio_secs);
        assert!((t.gen_secs - 0.56).abs() < 1e-9, "gen={}", t.gen_secs);
        assert!((t.rtf() - 6.428_57).abs() < 1e-3);

        // Чужие строки (ASR-тайминг, ошибки, старт сервера) не парсим.
        assert!(parse_synth_timing("crispasr-server: transcribed 11.0s audio in 0.42s (26.2x realtime)").is_none());
        assert!(parse_synth_timing("crispasr-server: listening on 127.0.0.1:21644").is_none());
        assert!(parse_synth_timing("qwen3_tts: ERROR: runaway").is_none());
    }
}
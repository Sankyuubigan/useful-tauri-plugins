//! ImageEngine — генерация/редактирование через ОТДЕЛЬНЫЙ процесс `sd-server.exe`.
//!
//! Контракт (проверен по исходникам leejet/stable-diffusion.cpp):
//! - запуск: `sd-server --diffusion-model … --vae … --llm … [--llm_vision …]
//!   --listen-ip 127.0.0.1 --listen-port <rand> --fa --diffusion-fa --auto-fit on
//!   --max-vram -1 --model-args qwen_image_2_1_prefix_cache_type=q8_0`
//!   (БЕЗ `--offload-to-cpu` — он выключает auto-fit и роняет скорость
//!   деноизинга в разы, см. комментарий у spawn);
//!   (модель грузится ДО listen — готовность = успешный HTTP-ответ);
//! - генерация: `POST /sdcpp/v1/img_gen` (нативный async API) → 202 `{id, poll_url}` →
//!   poll `GET /sdcpp/v1/jobs/{id}` до `completed` → `result.images[0].b64_json`;
//! - референсы (`-r` в CLI) в HTTP — массив `ref_images[]` (base64/data-URL),
//!   **порядок массива = порядок conditioning** (маппинг аттачментов чата 1-в-1);
//! - api-key у sd-server НЕТ (httplib без auth) — изоляция только bind 127.0.0.1.
//!
//! Жизнь — per-request spawn: создал → сгенерировал → Drop убил дерево.
//! Зачистка: Drop → kill_process_tree + Windows Job Object KILL_ON_JOB_CLOSE.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engine::models_catalog::ImageBundleEntry;
use crate::engine::preflight::{preflight_check, PreflightVerdict};
use crate::engine::process_util::{
    assign_child_to_kill_job, kill_process_tree, register_engine_pid, unregister_engine_pid,
};
use crate::engine::{ref_size, sdcpp_installer};

/// Диапазон портов image-движка (не пересекается с llama 17800..19300).
const PORT_MIN: u16 = 19400;
const PORT_RANGE: u16 = 1500;
/// Ожидание готовности сервера (загрузка 8B-энкодера + diffusion в память).
const HEALTH_TIMEOUT: Duration = Duration::from_secs(300);
/// Общий таймаут одной генерации (долгая операция, минуты).
const GEN_TIMEOUT: Duration = Duration::from_secs(1800);
/// Интервал опроса job-статуса.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Serialize, Clone, Debug)]
pub struct ImageGenResult {
    /// Путь к сохранённому PNG.
    pub path: String,
    /// Seed (фактический; -1 = сервер выбрал случайный).
    pub seed: i64,
    /// Длительность генерации, сек.
    pub time_sec: f64,
}

/// Параметры img2img (native API `/sdcpp/v1/img_gen`, api.md 855-898:
/// top-level `init_image` = base64/data-URL строка, `strength` = число,
/// canvas по width/height запроса). Несовместимо с `ref_images` —
/// взаимоисключающие режимы (заполнено то или другое, а не оба).
#[derive(Clone, Debug)]
pub struct Img2Img {
    /// Исходное изображение base64 (без заголовка data-URL).
    pub init_b64: String,
    /// Denoise-strength (0.0..1.0): 0 = копия входа, 1 = полная регенерация.
    pub strength: f32,
}

#[derive(Deserialize, Debug)]
struct JobSubmit {
    id: String,
    #[allow(dead_code)]
    kind: Option<String>,
    #[allow(dead_code)]
    status: Option<String>,
    #[allow(dead_code)]
    poll_url: Option<String>,
}

#[derive(Deserialize, Debug)]
struct JobStatus {
    #[allow(dead_code)]
    id: Option<String>,
    status: String,
    result: Option<JobResult>,
    error: Option<JobError>,
}

#[derive(Deserialize, Debug)]
struct JobResult {
    images: Option<Vec<JobImage>>,
}

#[derive(Deserialize, Debug)]
struct JobImage {
    b64_json: String,
}

#[derive(Deserialize, Debug)]
struct JobError {
    #[allow(dead_code)]
    code: Option<String>,
    message: Option<String>,
}

/// Найти свободный порт bind-проверкой (не хардкодить — конфликты параллельных запусков).
fn pick_free_port() -> Option<u16> {
    // Простой детерминированный обход со случайным стартом (xorshift от времени+PID).
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        ^ (std::process::id() as u64).wrapping_mul(0xBF58476D1CE4E5B9);
    let mut xorshift = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let start = (xorshift() % PORT_RANGE as u64) as u16;
    for i in 0..5 {
        let candidate = PORT_MIN + (start + i * 137) % PORT_RANGE;
        if std::net::TcpListener::bind(("127.0.0.1", candidate)).is_ok() {
            return Some(candidate);
        }
    }
    None
}

pub struct ImageEngine {
    child: std::process::Child,
    base_url: String,
    client: reqwest::blocking::Client,
}

impl ImageEngine {
    /// Spawn sd-server с файлами бандла. Preflight — до spawn (понятная ошибка
    /// вместо падения сервера); вердикт Offload → стейджинг-флаги уже в cmd.
    pub fn new<L>(
        engine_dir: &Path,
        bundle_dir: &Path,
        entry: &ImageBundleEntry,
        variant_pref: Option<&str>,
        log_cb: L,
    ) -> Result<Self, String>
    where
        L: Fn(String),
    {
        let source = "leejet";
        let variant = sdcpp_installer::resolve_variant(variant_pref);
        let mut server_exe = sdcpp_installer::variant_dir(engine_dir, source, &variant)
            .join("sd-server.exe");
        if !server_exe.exists() {
            // legacy root fallback
            let legacy = engine_dir.join("sd-server.exe");
            if legacy.exists() {
                server_exe = legacy;
            } else {
                return Err(format!(
                    "Движок изображений не установлен (нет sd-server.exe).\n\
                     Откройте Настройки → «Движок изображений» и нажмите «Установить движок»."
                ));
            }
        }

        // ── Preflight ДО spawn ──
        let (verdict, est, msg) = preflight_check(entry);
        log_cb(format!(
            "💾 Бандл: ~{:.1} ГБ на диске; VRAM full ~{:.1} ГБ / минимум ~{:.1} ГБ; RAM нужно ~{:.1} ГБ.",
            est.disk_mb / 1024.0,
            est.full_mb / 1024.0,
            est.min_mb / 1024.0,
            est.ram_need_mb / 1024.0
        ));
        match verdict {
            PreflightVerdict::Fast => log_cb(format!("✅ {}", msg)),
            PreflightVerdict::Offload => log_cb(format!("⚙️ {}", msg)),
            PreflightVerdict::Insufficient => return Err(msg),
        }

        // ── Файлы бандла обязаны существовать ──
        let weights = crate::engine::models_catalog::bundle_weights_files(bundle_dir, entry);
        for role in ["diffusion", "vae", "llm"] {
            let p = weights.get(role).cloned().unwrap_or_default();
            if !Path::new(&p).exists() {
                return Err(format!(
                    "Файл бандла не найден ({}): {}\nСкачайте бандл: Настройки → «Движок изображений».",
                    role, p
                ));
            }
        }

        let port = pick_free_port()
            .ok_or_else(|| "Не найден свободный порт для sd-server (19400..20900).".to_string())?;

        let mut cmd = std::process::Command::new(&server_exe);
        cmd.args([
            "--diffusion-model",
            &weights["diffusion"],
            "--vae",
            &weights["vae"],
            "--llm",
            &weights["llm"],
        ]);
        if let Some(mmproj) = weights.get("mmproj") {
            if Path::new(mmproj).exists() {
                // Vision-веса для editing (флаг с подчёркиванием — как в docs/qwen_image_2.1.md).
                cmd.args(["--llm_vision", mmproj]);
            }
        }
        cmd.args(["--listen-ip", "127.0.0.1"]);
        cmd.args(["--listen-port", &port.to_string()]);
        // Память/скорость. КРИТИЧНО: НЕ передаём `--offload-to-cpu`.
        // Он подставляет `--params-backend '*=cpu'`, а ЛЮБОЙ явный `--params-backend`
        // ОТКЛЮЧАЕТ auto-fit (docs/backend.md: "Explicit --params-backend assignments
        // disable auto-fit"). Без auto-fit все веса (diffusion + text-encoder +
        // mmproj + VAE) живут в RAM, GPU-копии вытесняются на каждом шаге, и
        // деноизинг идёт через сегментированное исполнение со стримингом весов
        // по PCIe — на 4070 Ti SUPER это ~10 сек на шаг вместо ~1.
        // С auto-fit планировщик сам кладёт diffusion в VRAM (приоритет 1),
        // а TE/VAE уводит в RAM и освобождает их GPU-копии после своей фазы,
        // так что diffusion остаётся резидентным на всех шагах.
        cmd.args([
            "--fa",
            "--diffusion-fa",
            "--auto-fit",
            "on",
            // Зарезервировать ~1 ГБ от стартового свободного VRAM (документированная
            // рекомендация sd.cpp для «модели, которая влезает в карту»): авто-fit
            // не посчитает TE/VAE резидентными и оставит запас под prefix-кэш.
            "--max-vram",
            "-1",
            // VAE тайлами: без этого флага encode 2048² (hires-fix, Ланцош-второй
            // проход) требует ~15.7 ГБ и валит job («encode_first_stage failed
            // after hires Lanczos upscale»), а encode 1024²-референса на edit
            // (~4.4 ГБ) не влезает у Q6_K-бандла после резидентных весов —
            // замер 19.10.2026, bench_run.log. Тайлинг VAE делает оба режима
            // структурно проходимыми (sd-server --help: «--vae-tiling: process
            // vae in tiles to reduce memory usage»).
            "--vae-tiling",
            // Prefix KV-кэш (текст + vision-токены референсов) в 8 бит: вдвое меньше
            // VRAM, чем f16 (1.06 ГБ против 2 ГБ на префикс ~4096 токенов), при
            // сохранении ускорения «посчитать кондиционирование один раз».
            "--model-args",
            "qwen_image_2_1_prefix_cache_type=q8_0",
        ]);
        cmd.current_dir(engine_dir);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW — без мелькания консоли
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Не удалось запустить sd-server: {}", e))?;
        register_engine_pid(child.id());
        assign_child_to_kill_job(&child);

        // ОБА потока сервера — в лог хоста. sd.cpp пишет в stdout ВСЮ диагностику
        // (auto-fit plan, compute buffer / число сегментов, префикс-кэш, время
        // сэмплирования); stderr содержит только ggml/CUDA. Если слушать лишь
        // stderr, в логе остаются 4 строки и диагностировать пайплайн нечем.
        pipe_log(child.stdout.take(), "sd-server");
        pipe_log(child.stderr.take(), "sd-server");

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("Ошибка HTTP-клиента: {}", e))?;
        let engine = Self {
            child,
            base_url: format!("http://127.0.0.1:{}", port),
            client,
        };
        engine.wait_ready(&log_cb)?;
        Ok(engine)
    }

    /// Ожидание готовности: модель грузится ДО listen — первый успешный
    /// `/sdcpp/v1/capabilities` = сервер поднят с весами.
    fn wait_ready<L>(&self, log_cb: L) -> Result<(), String>
    where
        L: Fn(String),
    {
        log_cb("⏳ Загрузка бандла в sd-server…".to_string());
        let start = Instant::now();
        loop {
            match self.client.get(format!("{}/sdcpp/v1/capabilities", self.base_url)).send() {
                Ok(resp) if resp.status().is_success() => {
                    log_cb("✅ sd-server готов.".to_string());
                    return Ok(());
                }
                _ => {}
            }
            if start.elapsed() > HEALTH_TIMEOUT {
                return Err("sd-server не стал готов за 300 сек (см. лог [sd-server]).".to_string());
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    /// Генерация: submit → poll → PNG на диск. `ref_images_b64` — base64/data-URL
    /// в порядке conditioning (пусто = t2i). `custom_sigmas`/`scheduler` —
    /// необязательный сигма-профиль пресета (Turbo-семейство: custom_sigmas
    /// обязателен; контракт sd.cpp SamplePlan::resolve: steps = len(sigmas)-1,
    /// массив обязан кончаться терминальным 0.0). `hires` — объект hires-fix
    /// нативного API (`hires.enabled/upscaler/scale/steps/denoising_strength`),
    /// None = обычная однофазная генерация.
    pub fn generate<L>(
        &self,
        prompt: &str,
        width: u32,
        height: u32,
        steps: u32,
        cfg_scale: f32,
        seed: i64,
        ref_images_b64: &[String],
        out_path: &Path,
        cancel_flag: Option<Arc<AtomicBool>>,
        log_cb: L,
        custom_sigmas: Option<&[f32]>,
        scheduler: Option<&str>,
        hires: Option<&serde_json::Value>,
        img2img: Option<&Img2Img>,
    ) -> Result<ImageGenResult, String>
    where
        L: Fn(String),
    {
        let w = round32(width);
        let h = round32(height);
        if w != width || h != height {
            log_cb(format!("ℹ️ Размер скорректирован до кратного 32: {}x{} → {}x{}.", width, height, w, h));
        }
        // Сигма-профиль: контракт sd.cpp SamplePlan::resolve —
        // total_steps = custom_sigmas_count - 1, массив кончается 0.0.
        let effective_steps = match custom_sigmas {
            Some(s) if !s.is_empty() => s.len() as u32 - 1,
            _ => steps,
        };
        if custom_sigmas.is_some_and(|s| !s.is_empty()) && effective_steps != steps {
            log_cb(format!(
                "ℹ️ steps скорректирован сигма-профилем: {} → {} (len(custom_sigmas) − 1, sd.cpp).",
                steps, effective_steps
            ));
        }
        let mut sample_params = serde_json::json!({
            "sample_method": "euler",
            "sample_steps": effective_steps,
            "guidance": { "txt_cfg": cfg_scale },
        });
        // scheduler — ОТДЕЛЬНОЕ поле API (не sample_method): имя шедулера
        // из /sdcpp/v1/capabilities (напр. "discrete"). Актуальные пресеты
        // каталога его не задают: при наличии custom_sigmas шедулер
        // игнорируется (см. SamplePlan::resolve), иначе работает дефолт sd.cpp.
        if let Some(sched) = scheduler {
            if !sched.is_empty() {
                sample_params["scheduler"] = serde_json::json!(sched);
            }
        }
        if let Some(s) = custom_sigmas {
            if !s.is_empty() {
                sample_params["custom_sigmas"] = serde_json::json!(s);
            }
        }
        let mut body = serde_json::json!({
            "prompt": prompt,
            "width": w,
            "height": h,
            "seed": seed,
            "batch_count": 1,
            "ref_images": ref_images_b64,
            "sample_params": sample_params,
            "output_format": "png",
        });
        if let Some(h) = hires {
            if !h.is_null() {
                body["hires"] = h.clone();
            }
        }
        if let Some(i2i) = img2img {
            if !ref_images_b64.is_empty() {
                return Err(
                    "img2img и ref_images взаимоисключающие режимы — одновременно задавать нельзя.".to_string(),
                );
            }
            body["init_image"] = serde_json::json!(i2i.init_b64);
            body["strength"] = serde_json::json!(i2i.strength.clamp(0.0, 1.0));
        }
        log_cb(format!(
            "🎨 Генерация: {}x{}, steps={}, cfg={}, refs={}{}…",
            w,
            h,
            effective_steps,
            cfg_scale,
            ref_images_b64.len(),
            img2img.map(|i| format!(", init_image в {}x{}, strength {:.2}", w, h, i.strength)).unwrap_or_default()
        ));
        let t0 = Instant::now();

        let submit: JobSubmit = self
            .client
            .post(format!("{}/sdcpp/v1/img_gen", self.base_url))
            .json(&body)
            .send()
            .map_err(|e| format!("Ошибка submit img_gen: {}", e))?
            .error_for_status()
            .map_err(|e| format!("sd-server отклонил запрос: {}", e))?
            .json()
            .map_err(|e| format!("Ошибка парсинга ответа submit: {}", e))?;

        let b64 = self.poll_job(&submit.id, cancel_flag, &log_cb)?;
        let bytes = base64_decode(&b64)?;
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Не удалось создать папку {}: {}", parent.display(), e))?;
        }
        std::fs::write(out_path, &bytes)
            .map_err(|e| format!("Не удалось записать {}: {}", out_path.display(), e))?;
        let time_sec = t0.elapsed().as_secs_f64();
        log_cb(format!(
            "✅ Готово за {:.1} сек → {}",
            time_sec,
            out_path.display()
        ));
        Ok(ImageGenResult {
            path: out_path.to_string_lossy().to_string(),
            seed,
            time_sec,
        })
    }

    /// Poll job до completed/failed/cancelled. Отмена — через cancel_flag.
    fn poll_job<L>(
        &self,
        job_id: &str,
        cancel_flag: Option<Arc<AtomicBool>>,
        log_cb: L,
    ) -> Result<String, String>
    where
        L: Fn(String),
    {
        let start = Instant::now();
        loop {
            if let Some(flag) = &cancel_flag {
                if flag.load(Ordering::SeqCst) {
                    let _ = self.client.post(format!("{}/sdcpp/v1/jobs/{}/cancel", self.base_url, job_id)).send();
                    return Err("CANCELLED".to_string());
                }
            }
            let st: JobStatus = self
                .client
                .get(format!("{}/sdcpp/v1/jobs/{}", self.base_url, job_id))
                .send()
                .map_err(|e| format!("Ошибка poll job: {}", e))?
                .json()
                .map_err(|e| format!("Ошибка парсинга job: {}", e))?;
            match st.status.as_str() {
                "completed" => {
                    let b64 = st
                        .result
                        .and_then(|r| r.images)
                        .and_then(|mut v| v.pop())
                        .map(|img| img.b64_json)
                        .ok_or_else(|| "Job completed, но без изображений.".to_string())?;
                    return Ok(b64);
                }
                "failed" => {
                    let msg = st.error.map(|e| e.message.unwrap_or_default()).unwrap_or_default();
                    return Err(format!("Генерация не удалась: {}", msg));
                }
                "cancelled" => return Err("CANCELLED".to_string()),
                _ => {}
            }
            if start.elapsed() > GEN_TIMEOUT {
                let _ = self.client.post(format!("{}/sdcpp/v1/jobs/{}/cancel", self.base_url, job_id)).send();
                return Err("Таймаут генерации (30 мин) — job отменён.".to_string());
            }
            log_cb(format!("⏳ Генерация… {:.0} сек", start.elapsed().as_secs()));
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

/// Дефолтный denoise-strength апскейла продукта. Выбран юзером 19.10.2026
/// по A/B-сетке U*_S42 (`report.upscale_ab.json`, плитки d025/d035/d045):
/// d045 — максимум новодобавленной детализации; приёмка «рискует композиции»
/// не сработала: источник 1024² → 2048², композиция держится фактическим
/// латентом с strength-обрезкой сигм.
pub const UPSCALE_STRENGTH_DEFAULT: f32 = 0.45;

/// Целевые размеры апскейла ×2: каждая сторона удваивается, но не превышает
/// 2048 (потолок VRAM — hires-fix bench тоже останавливается на 2048²;
/// encode/decodebrask 4096² тайлам и вовсе не проверен). round32 сохраняется.
pub fn upscale_target_dims(width: u32, height: u32) -> (u32, u32) {
    const MAX_SIDE: u32 = 2048;
    (round32(((width * 2).min(MAX_SIDE)).max(32)), round32(((height * 2).min(MAX_SIDE)).max(32)))
}

impl Drop for ImageEngine {
    fn drop(&mut self) {
        let pid = self.child.id();
        kill_process_tree(&mut self.child);
        let _ = self.child.wait();
        unregister_engine_pid(pid);
    }
}

/// Размеры — кратно 32 (требование Qwen Image 2.1).
fn round32(v: u32) -> u32 {
    ((v + 31) / 32 * 32).max(32)
}

/// Перелить построчный лог процесса движка в лог хоста (`[sd-server] …`).
fn pipe_log<R>(stream: Option<R>, tag: &str)
where
    R: std::io::Read + Send + 'static,
{
    let Some(stream) = stream else { return };
    let tag = tag.to_string();
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = line.trim_end();
            if text.is_empty() {
                continue;
            }
            if text.contains("[ERROR]") || text.contains("error:") {
                log::error!("[{}] {}", tag, text);
            } else {
                log::info!("[{}] {}", tag, text);
            }
        }
    });
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    // Принимаем и чистый base64, и data-URL.
    let raw = s.split_once(',').map(|(_, b)| b).unwrap_or(s);
    base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| format!("Ошибка декодирования base64 картинки: {}", e))
}

/// One-shot t2i: spawn → generate → Drop. Точка входа per-request архитектуры.
pub fn generate_image_simple<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &ImageBundleEntry,
    variant_pref: Option<&str>,
    prompt: &str,
    width: u32,
    height: u32,
    steps: u32,
    cfg_scale: f32,
    seed: i64,
    out_path: &Path,
    cancel_flag: Option<Arc<AtomicBool>>,
    log_cb: L,
) -> Result<ImageGenResult, String>
where
    L: Fn(String) + Clone,
{
    let engine = ImageEngine::new(engine_dir, bundle_dir, entry, variant_pref, log_cb.clone())?;
    engine.generate(
        prompt,
        width,
        height,
        steps,
        cfg_scale,
        seed,
        &[],
        out_path,
        cancel_flag,
        log_cb,
        entry.preset.custom_sigmas.as_deref(),
        entry.preset.scheduler.as_deref(),
        None,
        None,
    )
}

/// One-shot edit: референсы читаются с диска В ПОРЯДКЕ массива → ref_images[].
///
/// Canvas (width/height) подстраивается под пропорции ПЕРВОГО референса
/// (`<image 1>` — тот, чья композиция должна выжить): та же площадь, что в
/// пресете каталога, но стороны кратны 32. Официальный ComfyUI-шаблон
/// Qwen-Image-2.1: «custom_size off: canvas comes from the encode latent
/// (image_1)» и «Keep it close to the resized image_1 size, or the edit can
/// shift». Пресет 1024x1024 у портретного референса тянул бы и искажение
/// композиции, и лишние вычисления.
pub fn edit_image_files<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &ImageBundleEntry,
    variant_pref: Option<&str>,
    prompt: &str,
    ref_paths: &[String],
    width: u32,
    height: u32,
    steps: u32,
    cfg_scale: f32,
    seed: i64,
    out_path: &Path,
    cancel_flag: Option<Arc<AtomicBool>>,
    log_cb: L,
) -> Result<ImageGenResult, String>
where
    L: Fn(String) + Clone,
{
    let mut refs_b64 = Vec::with_capacity(ref_paths.len());
    for p in ref_paths {
        let mut f = std::fs::File::open(p).map_err(|e| format!("Не найден референс {}: {}", p, e))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| format!("Ошибка чтения {}: {}", p, e))?;
        use base64::Engine;
        refs_b64.push(base64::engine::general_purpose::STANDARD.encode(&buf));
    }
    let (canvas_w, canvas_h) = ref_paths
        .first()
        .and_then(|first| ref_size::dimensions_from_path(Path::new(first)))
        .map(|dims| ref_size::canvas_for_ref((width, height), dims))
        .unwrap_or((width, height));
    if (canvas_w, canvas_h) != (width, height) {
        log_cb(format!(
            "ℹ️ Canvas по пропорциям <image 1>: {}x{} → {}x{}.",
            width, height, canvas_w, canvas_h
        ));
    }
    let engine = ImageEngine::new(engine_dir, bundle_dir, entry, variant_pref, log_cb.clone())?;
    engine.generate(
        prompt,
        canvas_w,
        canvas_h,
        steps,
        cfg_scale,
        seed,
        &refs_b64,
        out_path,
        cancel_flag,
        log_cb,
        entry.preset.custom_sigmas.as_deref(),
        entry.preset.scheduler.as_deref(),
        None,
        None,
    )
}

/// One-shot upscale: исходник с диска → img2img ×2 (init_image + strength).
/// Canvas = `upscale_target_dims` (стороны ×2, потолок 2048). Референсы не
/// задаются (img2img несовместим с ref_images). Сигма-профиль пресета —
/// как у обычной генерации: шаги обрезаются по `strength`.
pub fn upscale_image_files<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &ImageBundleEntry,
    variant_pref: Option<&str>,
    prompt: &str,
    strength: f32,
    source_path: &str,
    seed: i64,
    out_path: &Path,
    cancel_flag: Option<Arc<AtomicBool>>,
    log_cb: L,
) -> Result<ImageGenResult, String>
where
    L: Fn(String) + Clone,
{
    let mut f = std::fs::File::open(source_path)
        .map_err(|e| format!("Не найден исходник апскейла {}: {}", source_path, e))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| format!("Ошибка чтения {}: {}", source_path, e))?;
    use base64::Engine;
    let init_b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    let dims = ref_size::dimensions_from_path(Path::new(source_path))
        .ok_or_else(|| format!("Не определить размеры исходника {}", source_path))?;
    let (w, h) = upscale_target_dims(dims.0, dims.1);
    if (w, h) != (dims.0 * 2, dims.1 * 2) && (dims.0 * 2 > 2048 || dims.1 * 2 > 2048) {
        log_cb(format!(
            "ℹ️ Цель апскейла урезана потолком 2048 по стороне: {}x{} → {}x{}.",
            dims.0, dims.1, w, h
        ));
    }
    let engine = ImageEngine::new(engine_dir, bundle_dir, entry, variant_pref, log_cb.clone())?;
    engine.generate(
        prompt,
        w,
        h,
        entry.preset.steps,
        entry.preset.cfg_scale,
        seed,
        &[],
        out_path,
        cancel_flag,
        log_cb,
        entry.preset.custom_sigmas.as_deref(),
        entry.preset.scheduler.as_deref(),
        None,
        Some(&Img2Img { init_b64, strength }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round32_snaps_to_multiples() {
        assert_eq!(round32(1024), 1024);
        assert_eq!(round32(1000), 1024);
        assert_eq!(round32(1), 32);
    }

    #[test]
    fn upscale_target_doubles_and_caps_at_2048() {
        assert_eq!(upscale_target_dims(1024, 1024), (2048, 2048));
        assert_eq!(upscale_target_dims(512, 512), (1024, 1024));
        assert_eq!(upscale_target_dims(1600, 1024), (2048, 2048));
        assert_eq!(upscale_target_dims(300, 200), (608, 416));
        assert_eq!(upscale_target_dims(2048, 2048), (2048, 2048));
    }

    #[test]
    fn base64_decode_accepts_data_url() {
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3]);
        let url = format!("data:image/png;base64,{}", raw);
        assert_eq!(base64_decode(&url).unwrap(), vec![1, 2, 3]);
        assert_eq!(base64_decode(&raw).unwrap(), vec![1, 2, 3]);
    }
}

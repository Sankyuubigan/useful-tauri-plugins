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

use crate::engine::lora::{lora_spawn_args_for_base, LoraSpec, LORA_MULTIPLIER_MAX};
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

/// Политика preflight при spawn sd-server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreflightPolicy {
    /// Продуктовое поведение: при нехватке VRAM — понятная ошибка, движок не
    /// запускается (пользователю лучше отказ, чем OOM в середине генерации).
    Enforce,
    /// Замерный режим (bench): оценка preflight пишется в лог, но запуск не
    /// блокируется. Нужен ФИЗИЧЕСКИЙ ответ «влезает ли auto-fit в дефицит» —
    /// калькулятор preflight отвечает на другой вопрос («достаточно ли по
    /// оценке»).
    Measure,
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
        Self::new_with_policy(
            engine_dir,
            bundle_dir,
            entry,
            variant_pref,
            PreflightPolicy::Enforce,
            log_cb,
        )
    }

    /// То же, но с явной политикой preflight (см. `PreflightPolicy`).
    pub fn new_with_policy<L>(
        engine_dir: &Path,
        bundle_dir: &Path,
        entry: &ImageBundleEntry,
        variant_pref: Option<&str>,
        policy: PreflightPolicy,
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
            PreflightVerdict::Insufficient => match policy {
                PreflightPolicy::Enforce => return Err(msg),
                PreflightPolicy::Measure => log_cb(format!(
                    "⚠️ {} (замерный режим: запуск не блокируется — меряем физический результат).",
                    msg
                )),
            },
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
        ]);
        // Каталог LoRA сервера (PRP 29.10.26; F3 29.10.26 — папки по базам).
        // Передаётся ТОЛЬКО если папка существует — генерация без лоры
        // обязана работать как раньше, а заявка на несуществующую лору честно
        // падает в resolve_lora_names (тот же паттерн, что у видео-спавна с
        // ролью lora_draft). Каталог scoped на БАЗУ бандла
        // (`<image_loras>/<lora_base>`): лоры чужой базы физически не видны
        // серверу — кросс-базное применение невозможно по design.
        let lora_dir = crate::engine::config::image_lora_dir_early();
        let lora_args = lora_spawn_args_for_base(&lora_dir, entry.lora_base.as_deref());
        if !lora_args.is_empty() {
            match entry.lora_base.as_deref() {
                Some(base) => log_cb(format!(
                    "⚙ lora-dir: {} (база «{}»)",
                    lora_dir.join(base).display(),
                    base
                )),
                None => log_cb(format!("⚙ lora-dir: {}", lora_dir.display())),
            }
            cmd.args(&lora_args);
        } else {
            log_cb("⚙ lora-dir: (папки LoRA нет — LoRA-стек отключён)".to_string());
        }
        // Prefix KV-кэш (текст + vision-токены референсов) в 8 бит: вдвое меньше
        // VRAM, чем f16, при сохранении ускорения «посчитать кондиционирование
        // один раз». Ключ задаёт КАТАЛОГ бандла (per-bundle `model_args`): для
        // стеков Krea2 qwen-ключ не посылается — у krea2 нет собственного
        // парсера model-args, ключ молча игнорировался бы.
        match entry.model_args.as_deref().map(str::trim).filter(|arg| !arg.is_empty()) {
            Some(model_args) => {
                log_cb(format!("⚙ model-args: {}", model_args));
                cmd.args(["--model-args", model_args]);
            }
            None => log_cb("⚙ model-args: (бандл не задаёт)".to_string()),
        }
        cmd.current_dir(engine_dir);
        // CUDA-DLL лежат в общей папке backends/_cudart/cuda12 (sdcpp_installer::cudart_dir),
        // а не рядом с sd-server.exe — путь подставляется в PATH дочернего процесса.
        if let Some(p) = sdcpp_installer::child_path_with_cudart(engine_dir) {
            cmd.env("PATH", p);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW — без мелькания консоли | BELOW_NORMAL_PRIORITY_CLASS —
            // CPU-фазы движка не душат мышь/UI хоста (генерация — фон для юзера).
            cmd.creation_flags(0x0800_0000 | 0x0000_4000);
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
        pipe_log(child.stdout.take(), "sd-server", true);
        pipe_log(child.stderr.take(), "sd-server", false);

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
    /// None = обычная однофазная генерация. `ref_image_args` — строка
    /// ref-аргументов сервера (api.md: «ref_image_args», пары «key=value»);
    /// основной случай `preset=krea2_edit` для Krea2-edit-лоры (F4)-
    /// None — сервер сам выбирает вшитый preset модели (`krea2_ostris_edit`
    /// для Krea2, `qwen` для Qwen).
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
        loras: &[LoraSpec],
        ref_image_args: Option<&str>,
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
        // F4 (29.10.26): ref-аргументы сервера — HTTP-поле `ref_image_args`
        // (string, api.md). Основной случай `preset=krea2_edit`: переопределяет
        // вшитый `krea2_ostris_edit` на пресет, соответствующий обученной
        // edit-лоре. None/пусто — поле не отправляется вообще (родной preset
        // модели не сбивается, Qwen-стек живёт как раньше).
        if let Some(rargs) = ref_image_args {
            if !rargs.trim().is_empty() {
                body["ref_image_args"] = serde_json::json!(rargs.trim());
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
        // LoRA-стек (PRP 29.10.26): тело получает поле lora[] ТОЛЬКО при
        // непустом списке. path — имя файла относительно --lora-model-dir
        // (спавн); multiplier клампится на границе плагина (0..=2);
        // is_high_noise: false — та же сериализация, что у видео-ветки
        // (build_payload); высокошумная (Wan MoE) лора — вне этого плана.
        if !loras.is_empty() {
            body["lora"] = serde_json::json!(
                loras.iter()
                    .map(|l| serde_json::json!({
                        "path": l.path,
                        "multiplier": l.multiplier.clamp(0.0, LORA_MULTIPLIER_MAX),
                        "is_high_noise": false,
                    }))
                    .collect::<Vec<_>>()
            );
            let stack: Vec<String> = loras
                .iter()
                .map(|l| format!("{}×{:.2}", l.path, l.multiplier))
                .collect();
            log_cb(format!("🎛 LoRA stack: {}", stack.join(" + ")));
        }
        log_cb(format!(
            "🎨 Генерация: {}x{}, steps={}, cfg={}, refs={}{}{}…",
            w,
            h,
            effective_steps,
            cfg_scale,
            ref_images_b64.len(),
            img2img.map(|i| format!(", init_image в {}x{}, strength {:.2}", w, h, i.strength)).unwrap_or_default(),
            ref_image_args.map(|r| format!(", ref_image_args «{}»", r.trim())).unwrap_or_default()
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
/// `scan_lora`: stdout-канал дополнительно сканируется на отчёт применения
/// LoRA («X / N … tensors … applied»); X < N → предупреждение (симптом
/// несовместимости варианта модели, leejet issue #2083). Lora-скан включён
/// только для image-движка; video_engine имел свою копию pipe_log.
fn pipe_log<R>(stream: Option<R>, tag: &str, scan_lora: bool)
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
            if scan_lora {
                if let Some((applied, total)) = crate::engine::lora::scan_lora_applied_line(text) {
                    if applied < total {
                        log::warn!(
                            "[{}] ⚠️ Применены не все LoRA-тензоры: {}/{} — вероятно, вариант LoRA не совпадает с моделью (leejet issue #2083).",
                            tag, applied, total
                        );
                    }
                }
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
    loras: &[LoraSpec],
    cancel_flag: Option<Arc<AtomicBool>>,
    log_cb: L,
) -> Result<ImageGenResult, String>
where
    L: Fn(String) + Clone,
{
    // Единовременность media-генерации: второй параллельный spawn удвоил бы
    // пики RAM/VRAM (gen_guard.rs). Хостовые тула и GUI-команды проходят
    // через эту точку — оба канала защищены одним guard'ом.
    let _guard = crate::engine::gen_guard::MediaGenGuard::acquire()?;
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
        loras,
        None, // t2i — ref-аргументов нет
    )
}

/// План ref-edit для исполнителя (F4, 29.10.26): готовый стек LoRA + строка
/// ref_image_args сервера.
#[derive(Debug)]
pub enum RefEditPlan {
    /// Ref-edit доступен: обновлённый стек (edit-лора первой при
    /// edit_lora-контракте) и аргументы сервера (Some("preset=...") —
    /// только edit_lora; None у native).
    Ready {
        loras: Vec<LoraSpec>,
        ref_image_args: Option<String>,
    },
    /// Бандл не умеет ref-edit — fail-closed каталога; причина содержат
    /// имя бандла и путь решения (готовый текст отказа для тула/команды).
    Unsupported { reason: String },
}

/// F4 (29.10.26): SSOT-планировщик ref-edit — единственное место, где
/// «умеет ли бандл редактировать» превращается в исполняемый план.
///
/// Контракт каталога (`ref_edit_contract`) объявляет способность; здесь она
/// встречается с реальностью диска (резолв edit-лоры по базе → несуществующий
/// файл — честный Err, а не «checkerboard» на выходе, ostris issue #1).
/// Tauri-команда `edit_image` плагина и host-тул агентов (`media.rs
/// run_generate`) зовут ОДНУ ТОЧКУ — расхождения бой-путей невозможны.
pub fn plan_ref_edit(
    entry: &ImageBundleEntry,
    lora_dir: &Path,
    user_specs: Vec<LoraSpec>,
) -> Result<RefEditPlan, String> {
    let contract = crate::engine::models_catalog::ref_edit_contract(entry)?;
    let Some(contract) = contract else {
        return Ok(RefEditPlan::Unsupported {
            reason: format!(
                "Бандл «{}» не поддерживает редактирование по референсам: в каталоге нет ref_edit-контракта. Решения: переключитесь на Qwen-бандл (родной ref-conditioning) или используйте Krea2-бандл из актуального каталога (edit-лора krea2_identity_edit_v2_r128, папка {}).",
                entry.name,
                lora_dir.display()
            ),
        });
    };
    match contract.mode {
        crate::engine::models_catalog::RefEditMode::Native => Ok(RefEditPlan::Ready {
            loras: user_specs,
            ref_image_args: None,
        }),
        crate::engine::models_catalog::RefEditMode::EditLora => {
            let stem = contract.lora.clone().unwrap_or_default();
            let specs = crate::engine::lora::resolve_lora_specs_for_base(
                lora_dir,
                &[(stem.clone(), contract.lora_strength)],
                entry.lora_base.as_deref(),
            )?;
            let edit_spec = specs
                .first()
                .cloned()
                .ok_or_else(|| format!("edit-лора «{}» не зарезолвилась", stem))?;
            Ok(RefEditPlan::Ready {
                loras: crate::engine::lora::inject_edit_lora(
                    user_specs,
                    &edit_spec.path,
                    edit_spec.multiplier,
                ),
                ref_image_args: contract.ref_image_args.clone(),
            })
        }
    }
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
    loras: &[LoraSpec],
    cancel_flag: Option<Arc<AtomicBool>>,
    log_cb: L,
    ref_image_args: Option<&str>,
) -> Result<ImageGenResult, String>
where
    L: Fn(String) + Clone,
{
    // Единовременность media-генерации (gen_guard.rs) — как в generate_image_simple.
    let _guard = crate::engine::gen_guard::MediaGenGuard::acquire()?;
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
        loras,
        // F4: строка `ref_image_args` от контракта бандла (preset под
        // edit-лору). Edit-запрос всегда приходит через команду edit_image,
        // которая уже разбирала контракт — здесь только проводка.
        ref_image_args,
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
    loras: &[LoraSpec],
    cancel_flag: Option<Arc<AtomicBool>>,
    log_cb: L,
) -> Result<ImageGenResult, String>
where
    L: Fn(String) + Clone,
{
    // Единовременность media-генерации (gen_guard.rs) — как в generate_image_simple.
    let _guard = crate::engine::gen_guard::MediaGenGuard::acquire()?;
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
        loras,
        None, // img2img и ref_images взаимоисключающие — ref-аргументов нет
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

    // ───────────── SSOT-планировщик ref-edit (F4 29.10.26) ─────────────

    /// Пустой entry-test helper: главное в контракте — mode/лора/preset/база.
    fn synthetic_entry(mode: Option<crate::engine::models_catalog::RefEditMode>, base: Option<&str>) -> ImageBundleEntry {
        // mode=None — реалистичный «legacy-бандл»: ни mode, ни полей.
        let (lora, preset) = match mode {
            None => (None, None),
            Some(_) => (Some("synthetic_edit".to_string()), Some("synthetic_preset".to_string())),
        };
        ImageBundleEntry {
            name: "synthetic".to_string(),
            label: String::new(),
            note: String::new(),
            is_default: false,
            files: vec![crate::engine::models_catalog::BundleFile {
                role: "diffusion".to_string(),
                filename: "synthetic.gguf".to_string(),
                download_url: String::new(),
                size_bytes: None,
            }],
            model_args: None,
            compute_buffers_mb: None,
            lora_base: base.map(str::to_string),
            ref_edit_mode: mode,
            ref_edit_lora: lora,
            ref_edit_preset: preset,
            ref_edit_lora_strength: 1.0,
            preset: crate::engine::models_catalog::ImagePreset::default(),
            size_gb: None,
            vram_fast_gb: None,
            vram_min_gb: None,
            ram_min_gb: None,
        }
    }

    /// Без mode — Unsupported с понятной причиной (fail-closed каталога).
    #[test]
    fn plan_ref_edit_unsupported_without_mode() {
        let e = synthetic_entry(None, None);
        let plan = plan_ref_edit(&e, Path::new("whatever"), Vec::new()).expect("план");
        match plan {
            RefEditPlan::Unsupported { reason } => {
                assert!(reason.contains("synthetic"), "причина называет бандл: {}", reason);
                assert!(reason.contains("ref_edit-контракта"), "причина называет контракт: {}", reason);
            }
            RefEditPlan::Ready { .. } => panic!("без mode ref-edit обязан быть недоступен"),
        }
    }

    /// Native-стек: стек юзера проходит без изменений, ref_image_args нет
    /// (сервер сам выберет свой preset).
    #[test]
    fn plan_ref_edit_native_passes_user_stack() {
        let e = synthetic_entry(Some(crate::engine::models_catalog::RefEditMode::Native), None);
        let user = vec![LoraSpec { path: "style.safetensors".to_string(), multiplier: 0.7 }];
        match plan_ref_edit(&e, Path::new("whatever"), user).expect("план") {
            RefEditPlan::Ready { loras, ref_image_args } => {
                assert_eq!(loras.len(), 1, "native не добавляет лор");
                assert_eq!(loras[0].path, "style.safetensors");
                assert!(ref_image_args.is_none(), "native: preset выбирает сервер");
            }
            RefEditPlan::Unsupported { reason } => panic!("native обязан быть Ready: {}", reason),
        }
    }

    /// edit_lora-контракт: лора резолвится по базе бандла, идёт ПЕРВОЙ в стеке,
    /// ref_image_args содержит preset каталога юзерские стайл-лоры сохраняются.
    #[test]
    fn plan_ref_edit_edit_lora_resolves_and_prepends() {
        // Сэндбокс: <root>/krea2/synthetic_edit.safetensors (база бандла — krea2).
        let root = std::env::temp_dir().join("ko_plan_ref_edit_sandbox");
        let base_dir = root.join("krea2");
        std::fs::create_dir_all(&base_dir).expect("сэндбокс-папка");
        std::fs::write(base_dir.join("synthetic_edit.safetensors"), b"x").expect("стаб-файл");
        let e = synthetic_entry(
            Some(crate::engine::models_catalog::RefEditMode::EditLora),
            Some("krea2"),
        );
        let user = vec![LoraSpec { path: "style.safetensors".to_string(), multiplier: 0.5 }];
        match plan_ref_edit(&e, &root, user).expect("план") {
            RefEditPlan::Ready { loras, ref_image_args } => {
                assert_eq!(ref_image_args.as_deref(), Some("preset=synthetic_preset"));
                assert_eq!(loras.len(), 2);
                assert_eq!(loras[0].path, "synthetic_edit.safetensors", "edit-лора первая");
                assert_eq!(loras[0].multiplier, 1.0);
                assert_eq!(loras[1].path, "style.safetensors");
            }
            RefEditPlan::Unsupported { reason } => panic!("edit_lora обязан быть Ready: {}", reason),
        }
    }

    /// edit_lora-контракт, а файла в каталоге нет — честный Err до спавна
    /// (это тот самый класс «серых квадратов», который делает невозможным).
    #[test]
    fn plan_ref_edit_edit_lora_missing_file_is_honest_error() {
        // База krea2 существует, но пуста: резолвер честно говорит
        // «LoRA «synthetic_edit» не найдена» — белее не «checkerboard».
        let empty_root = std::env::temp_dir().join("ko_plan_ref_edit_empty");
        let empty_base = empty_root.join("krea2");
        std::fs::create_dir_all(&empty_base).expect("пустая база");
        let e = synthetic_entry(
            Some(crate::engine::models_catalog::RefEditMode::EditLora),
            Some("krea2"),
        );
        let err = plan_ref_edit(&e, &empty_root, Vec::new()).expect_err("нет файла — отказ");
        assert!(
            err.contains("synthetic_edit"),
            "ошибка называет стем edit-лоры: {}",
            err
        );
    }
}

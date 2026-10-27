//! VideoEngine — видео-генерация через ОТДЕЛЬНЫЙ процесс `sd-server.exe` (MiniMax-H3).
//!
//! Контракт выверен по первоисточникам leejet/stable-diffusion.cpp
//! (локальные копии: `king_orch_3/temp/minimax_h3_sdcpp.md`, `king_orch_3/temp/api.md`):
//! - spawn: `sd-server --diffusion-model … --vae … --audio-vae … --llm …
//!   --listen-ip 127.0.0.1 --listen-port <rand> <spawn_flags из каталога>`;
//!   флаги `--offload-to-cpu --rng cpu` — SSOT каталога (docs/minimax_h3.md).
//!   Image-флаги `--fa/--diffusion-fa/--auto-fit/--max-vram` НЕ копируются:
//!   docs/backend.md — explicit `--params-backend` от auto-fit отключает auto-fit.
//! - submit: `POST /sdcpp/v1/vid_gen` → 202 `{id, poll_url}`.
//! - poll: `GET /sdcpp/v1/jobs/{id}` → completed → `result` = ОДИН контейнер:
//!   `b64_json` + `mime_type` + `output_format` echo + `fps` + `frame_count`.
//! - `output_format: "webm"` требует билд движка с WebM — иначе submit 400;
//!   авто-фоллбэк на avi запрещён (честный отказ с подсказкой).
//! - сервер сам нормализует кадры (ядро: ближайшее 4n+1 ≤ запрошено; у H3 —
//!   сетка 17k+5) — клиент шлёт кадры как есть и честно отдаёт `frame_count`.
//! - `ref_images` валиден только для H3 Ref2VA и не смешивается с
//!   `init_image`/`end_image`; `control_frames` H3 не поддерживает — пусто.
//! - `lora[]` подтверждён бенчем 26.10.26 (600/600 тензоров DMAD applied,
//!   dmad4 130.8 с против ctrl20f 505.6 с): поле шлётся из сэмплинг-плана
//!   `sampling_plan_for_quality` (draft с файлом роли lora_draft); путь
//!   относительно `--lora-model-dir` (спавн, папка бандла).
//!
//! Жизнь — per-request spawn: создал → сгенерировал → Drop убил дерево.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engine::models_catalog::{bundle_files_weights, video_bundle_disk_bytes, BundleFile, VideoBundleEntry};
use crate::engine::process_util::{
    assign_child_to_kill_job, kill_process_tree, register_engine_pid, unregister_engine_pid,
};
use crate::engine::sdcpp_installer;

/// Диапазон портов, общий с image-движком (PRP Шаг 2: 19400..20900),
/// с bind-проверкой — два живых сервера не столкнутся.
const VIDEO_PORT_MIN: u16 = 19400;
const VIDEO_PORT_RANGE: u16 = 1500;
/// Ожидание готовности сервера — «те же», что у image (300 с): 28 ГБ весов H3
/// грузятся на SSD юзера за ~20–60 с.
const VIDEO_HEALTH_TIMEOUT: Duration = Duration::from_secs(300);
/// Таймаут видео-генерации (PRP: ≥ 3600 с): 15-сек final на 8 ГБ ≈ 25+ мин.
const VIDEO_GEN_TIMEOUT: Duration = Duration::from_secs(3600);
/// Интервал опроса job-статуса.
const VIDEO_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Режим генерации — выводится из заполненных полей VideoRequest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoMode {
    /// Text → video (+аудио).
    T2Va,
    /// Задан первый кадр.
    I2Va,
    /// Заданы первый + последний кадры.
    Fl2Va,
    /// Заданы референсы.
    Ref2Va,
}

impl VideoMode {
    pub fn as_str(self) -> &'static str {
        match self {
            VideoMode::T2Va => "t2va",
            VideoMode::I2Va => "i2va",
            VideoMode::Fl2Va => "fl2va",
            VideoMode::Ref2Va => "ref2va",
        }
    }
}

/// Режим по заполненным полям + честные отказы на запретные комбинации
/// (api.md: Ref2VA нельзя комбинировать с init/end; FL2VA требует оба кадра).
pub fn resolve_mode(init: Option<&str>, end: Option<&str>, refs: &[String]) -> Result<VideoMode, String> {
    let has_init = init.is_some();
    let has_end = end.is_some();
    let has_refs = !refs.is_empty();
    if has_refs && (has_init || has_end) {
        return Err(
            "ref2va и init/end-кадры взаимоисключающие (api.md: Ref2VA нельзя комбинировать \
             с init_image/end_image). Пришлите либо ref_images, либо кадры."
                .to_string(),
        );
    }
    if has_end && !has_init {
        return Err(
            "end_image без init_image — FL2VA требует ОБА кадра (первый + последний).".to_string(),
        );
    }
    if has_init && has_end {
        Ok(VideoMode::Fl2Va)
    } else if has_init {
        Ok(VideoMode::I2Va)
    } else if has_refs {
        Ok(VideoMode::Ref2Va)
    } else {
        Ok(VideoMode::T2Va)
    }
}

/// Одна LoRA vid_gen-запроса. `path` — ОТНОСИТЕЛЬНО каталога LoRA сервера
/// (`--lora-model-dir`; контракт temp/api.md:1201-1207).
#[derive(Clone, Debug)]
pub struct VideoLoraRef {
    pub path: String,
    pub multiplier: f32,
}

/// Сэмплинг-план vid_gen: что именно уйдёт в `sample_params` и `lora[]`
/// payload'а. Единственный источник шагов/сэмплера/шедулера/shift —
/// `sampling_plan_for_quality` (каталог, SSOT).
#[derive(Clone, Debug)]
pub struct VideoSamplingPlan {
    pub steps: u32,
    /// "euler" (final / draft-без-LoRA) | "lcm" (турбо-точка DMAD).
    pub sample_method: String,
    /// "discrete" (euler-путь) | "simple" (турбо-точка DMAD).
    pub scheduler: String,
    /// None = НЕ слать ключ (модельный дефолт). 0.0 у H3 вырождает сигма-сетку
    /// → CUDA crash — неположительный shift сюда не попадает (валидатор каталога).
    pub flow_shift: Option<f32>,
    /// Пусто = без LoRA.
    pub lora: Vec<VideoLoraRef>,
}

/// Параметры одного vid_gen-запроса (на один вызов `generate`).
#[derive(Clone, Debug)]
pub struct VideoRequest {
    pub prompt: String,
    pub negative_prompt: Option<String>,
    /// base64 первого кадра (i2va/fl2va).
    pub init_b64: Option<String>,
    /// base64 последнего кадра (только вместе с init_b64 — fl2va).
    pub end_b64: Option<String>,
    /// base64 референсов (ref2va; НЕ смешивается с кадрами).
    pub ref_images_b64: Vec<String>,
    pub width: u32,
    pub height: u32,
    /// Шаги (draft/final решает командный слой через sampling_plan_for_quality).
    pub steps: u32,
    pub cfg_scale: f32,
    pub seed: i64,
    /// Желаемые кадры; фактические вернёт сервер (`frame_count`).
    pub video_frames: u32,
    pub fps: u32,
    /// `vae_tiling_params.enabled` в payload.
    pub vae_tiling: bool,
    /// image-to-video сила (поле `strength`; дефолт примера 0.75).
    pub strength: f32,
    /// Сэмплер payload'а (sample_params.sample_method).
    pub sample_method: String,
    /// Шедулер payload'а (sample_params.scheduler).
    pub scheduler: String,
    /// flow_shift payload'а; None — ключ не шлётся (модельный дефолт).
    pub flow_shift: Option<f32>,
    /// LoRA vid_gen (пусто = без LoRA).
    pub lora: Vec<VideoLoraRef>,
}

impl VideoRequest {
    pub fn mode(&self) -> Result<VideoMode, String> {
        resolve_mode(
            self.init_b64.as_deref().filter(|s| !s.is_empty()),
            self.end_b64.as_deref().filter(|s| !s.is_empty()),
            &self.ref_images_b64,
        )
    }
}

/// Результат: сохранённый контейнер + факты сервера.
#[derive(Serialize, Clone, Debug)]
pub struct VideoGenResult {
    pub path: String,
    /// Echo сервера — эффективный playback fps.
    pub fps: u32,
    /// Фактическое число кадров после сетки сервера (4n+1 / 17k+5).
    pub frame_count: u64,
    /// `video/webm` / `image/webp` / `video/x-msvideo`.
    pub mime_type: String,
    pub time_sec: f64,
}

/// Расширение файла по mime контейнера (api.md: webm→video/webm,
/// webp→image/webp, avi→video/x-msvideo). Неизвестный mime → webm.
fn ext_for_mime(mime: &str) -> &'static str {
    match mime {
        "video/webm" => "webm",
        "image/webp" => "webp",
        "video/x-msvideo" => "avi",
        _ => "webm",
    }
}

/// Файл роли `lora_draft` записи каталога (None — роли нет).
fn lora_draft_file(entry: &VideoBundleEntry) -> Option<&BundleFile> {
    entry.files.iter().find(|f| f.role == "lora_draft")
}

/// Сэмплинг-план по качеству (SSOT шагов/сэмплера/шедулера/shift/LoRA — каталог):
/// - final → {steps_final, сэмплер каталога (euler), discrete, shift не шлётся, без LoRA};
/// - draft + файл роли `lora_draft` есть в bundle_dir → турбо-точка каталога
///   {steps_draft, turbo_sampler, turbo_scheduler, Some(turbo_flow_shift), lora_draft ×1.0};
/// - draft без LoRA (файла нет / роль не в каталоге) → {steps_draft_nolora,
///   сэмплер каталога, discrete, shift не шлётся, без LoRA}.
pub fn sampling_plan_for_quality(
    entry: &VideoBundleEntry,
    bundle_dir: &Path,
    final_quality: bool,
) -> VideoSamplingPlan {
    let p = &entry.preset;
    let base = VideoSamplingPlan {
        steps: if final_quality { p.steps_final } else { p.steps_draft_nolora },
        sample_method: p.sampler.clone(),
        scheduler: "discrete".to_string(),
        flow_shift: None,
        lora: Vec::new(),
    };
    if final_quality {
        return base;
    }
    let lora_ready = lora_draft_file(entry)
        .map(|f| bundle_dir.join(&f.filename).exists())
        .unwrap_or(false);
    if !lora_ready {
        return base;
    }
    let lora_path = lora_draft_file(entry)
        .map(|f| f.filename.clone())
        .unwrap_or_default();
    VideoSamplingPlan {
        steps: p.steps_draft,
        sample_method: p.turbo_sampler.clone(),
        scheduler: p.turbo_scheduler.clone(),
        flow_shift: Some(p.turbo_flow_shift),
        lora: vec![VideoLoraRef { path: lora_path, multiplier: 1.0 }],
    }
}

/// Секунды → желаемые кадры (5 c × 24 fps = 120). Клиент НЕ применяет сетку
/// 4n+1/17k+5 сам — её держит движок, фактическое число придёт в result.
pub fn seconds_to_frames(seconds: u32, fps: u32) -> u32 {
    seconds.saturating_mul(fps)
}

/// Сборка spawn-аргументов из бандла. Отказ называет ровно ту роль/ограничение,
/// которая не выполнена — без молчаливых подстановок.
pub fn build_spawn_args(
    entry: &VideoBundleEntry,
    weights: &std::collections::HashMap<String, String>,
    port: u16,
) -> Result<Vec<String>, String> {
    let need = [
        ("--diffusion-model", "diffusion"),
        ("--vae", "vae"),
        ("--audio-vae", "audio_vae"),
        ("--llm", "llm"),
    ];
    let mut args: Vec<String> = Vec::new();
    for (flag, role) in need {
        let p = weights
            .get(role)
            .ok_or_else(|| format!("нет файла роли «{}» (spawn видео-бандла «{}»)", role, entry.name))?;
        if !Path::new(p).exists() {
            return Err(format!(
                "Файл видео-бандла не найден ({}): {}\nСкачайте бандл: Настройки → «Видео».",
                role, p
            ));
        }
        args.push(flag.to_string());
        args.push(p.clone());
    }
    args.push("--listen-ip".to_string());
    args.push("127.0.0.1".to_string());
    args.push("--listen-port".to_string());
    args.push(port.to_string());
    // SSOT флагов — каталог. Пустые spawn_flags = дефект каталога.
    let flags = entry
        .spawn_flags
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if flags.is_empty() {
        return Err(format!(
            "spawn_flags бандла «{}» пуст — флаги видео-спавна обязаны идти из каталога (SSOT), не из кода.",
            entry.name
        ));
    }
    for f in flags.split_whitespace() {
        args.push(f.to_string());
    }
    if let Some(ma) = entry
        .model_args
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.push("--model-args".to_string());
        args.push(ma.to_string());
    }
    // Инвариант каталога: видео-спавн обязан вести offload (28 ГБ весов H3 не
    // влезают в карту целиком), а запретный image-тандем не посылается никогда.
    let joined = args.join(" ");
    if !joined.contains("--offload-to-cpu") {
        return Err(
            "spawn_flags бандла видео не содержит --offload-to-cpu — 28 ГБ весов H3 \
             не влезают в карту без offload. Исправьте каталог (SSOT)."
                .to_string(),
        );
    }
    if joined.contains("--auto-fit") || joined.contains("--max-vram") {
        return Err(
            "spawn_flags бандла видео содержит image-флаг (--auto-fit/--max-vram) — \
             с --offload-to-cpu они запрещены (docs/backend.md). Исправьте каталог."
                .to_string(),
        );
    }
    Ok(args)
}

/// Сборка payload `/sdcpp/v1/vid_gen`. Структура — 1:1 с официальным примером
/// api.md; значения — из параметров запроса. Отказы — по запретам api.md
/// (пустой промпт, нулевые размеры/кадры/качество, mix ref+кадры).
pub fn build_payload(params: &VideoRequest) -> Result<serde_json::Value, String> {
    if params.prompt.trim().is_empty() {
        return Err("Пустой промпт — vid_gen требует непустой prompt.".to_string());
    }
    if params.width == 0 || params.height == 0 {
        return Err(format!(
            "Нулевой размер {}x{} (width/height обязательны).",
            params.width, params.height
        ));
    }
    if params.video_frames == 0 {
        return Err("video_frames = 0 (кадры считаются как секунды × fps).".to_string());
    }
    if params.fps == 0 {
        return Err("fps = 0.".to_string());
    }
    if params.steps == 0 {
        return Err("steps = 0.".to_string());
    }
    if params.cfg_scale <= 0.0 {
        return Err("cfg_scale ≤ 0.".to_string());
    }
    // Режим — заодно честная проверка запретных комбинаций init/end/ref.
    params.mode()?;

    // sample_params собирается как Map (не json!-литерал): flow_shift вставляется
    // ТОЛЬКО при Some(v). Инвариант: flow_shift == 0.0 у H3 вырождает сигма-сетку
    // → CUDA illegal memory access (diag 26.10.26); ключ опускается при None —
    // работает модельный дефолт, «шлём ноль» невозможно конструктивно.
    let mut sample_params = serde_json::Map::new();
    sample_params.insert("scheduler".to_string(), serde_json::json!(params.scheduler));
    sample_params.insert("sample_method".to_string(), serde_json::json!(params.sample_method));
    sample_params.insert("sample_steps".to_string(), serde_json::json!(params.steps));
    sample_params.insert("eta".to_string(), serde_json::json!(1.0));
    sample_params.insert("shifted_timestep".to_string(), serde_json::json!(0));
    sample_params.insert("custom_sigmas".to_string(), serde_json::json!([]));
    if let Some(shift) = params.flow_shift {
        sample_params.insert("flow_shift".to_string(), serde_json::json!(shift));
    }
    sample_params.insert(
        "guidance".to_string(),
        serde_json::json!({
            "txt_cfg": params.cfg_scale,
            "img_cfg": params.cfg_scale,
            "distilled_guidance": 3.5,
            "slg": {
                "layers": [7, 8, 9],
                "layer_start": 0.01,
                "layer_end": 0.2,
                "scale": 0.0
            }
        }),
    );

    Ok(serde_json::json!({
        "prompt": params.prompt,
        "negative_prompt": params.negative_prompt.clone().unwrap_or_default(),
        "clip_skip": -1,
        "width": params.width,
        "height": params.height,
        "strength": params.strength,
        "seed": params.seed,
        "video_frames": params.video_frames,
        "fps": params.fps,
        // Wan MoE-автопереключение неприменимо к H3 — дефолт примера.
        "moe_boundary": 0.875,
        "vace_strength": 1.0,

        "init_image": params.init_b64.clone().filter(|s| !s.is_empty()),
        "end_image": params.end_b64.clone().filter(|s| !s.is_empty()),
        "ref_images": params.ref_images_b64,
        // H3 не поддерживает control_frames — «leave that array empty» (api.md).
        "control_frames": [],

        "sample_params": serde_json::Value::Object(sample_params),

        // Высокошумный первый проход отключён (пример допускает null/пропуск).
        "high_noise_sample_params": null,

        // LoRA из запроса (сэмплинг-план кладёт файл роли lora_draft ×1.0
        // для draft-турбо; путь относительно --lora-model-dir спавна).
        "lora": params.lora.iter().map(|l| serde_json::json!({
            "path": l.path,
            "multiplier": l.multiplier,
            "is_high_noise": false
        })).collect::<Vec<_>>(),

        "vae_tiling_params": {
            "enabled": params.vae_tiling,
            // H3 «keeps its model-specific fixed temporal windows» (api.md) —
            // темпоральные окна держит движок, клиент их не включает.
            "temporal_tiling": false,
            "tile_size_w": 0,
            "tile_size_h": 0,
            "target_overlap": 0.5,
            "rel_size_w": 0.0,
            "rel_size_h": 0.0,
            "extra_tiling_args": ""
        },

        "cache_mode": "disabled",
        "cache_option": "",
        "scm_mask": "",
        "scm_policy_dynamic": true,

        "preview": "none",
        "preview_interval": 1,

        // webm требует билд движка с WebM — иначе 400 с честной подсказкой юзеру.
        "output_format": "webm",
        "output_compression": 100
    }))
}

/// Сумма size_bytes файлов, которых НЕТ в bundle_dir — сколько реально нужно
/// освободить перед докачкой. Полностью скачанный бандл места не просит.
fn missing_bundle_bytes(entry: &VideoBundleEntry, bundle_dir: &Path) -> u64 {
    entry
        .files
        .iter()
        .filter(|f| !bundle_dir.join(&f.filename).exists())
        .map(|f| f.size_bytes.unwrap_or(0))
        .sum()
}

/// Preflight видео-бандла: пороги ИЗ каталога против фактов машины.
/// Ok = spawn допустим (возврат — человекочитаемое резюме);
/// Err — отказ с именем ровно того ресурса, которого мало.
pub fn preflight_video(entry: &VideoBundleEntry, bundle_dir: &Path) -> Result<String, String> {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let need_vram_gb = entry.vram_min_gb.unwrap_or(0.0) as f64;
    let need_ram_gb = entry.ram_min_gb.unwrap_or(0.0) as f64;
    let missing_bytes = missing_bundle_bytes(entry, bundle_dir) as f64;
    let full_bytes = video_bundle_disk_bytes(entry) as f64;

    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let ram_free_gb = sys.available_memory() as f64 / GIB;

    // Фактов VRAM может не быть (не-NVIDIA/драйвер) — тогда VRAM-порог
    // пропускается с честной пометкой в лог, а RAM/диск проверяются всегда.
    let (vram_total_gb, vram_ok) = match nvml_wrapper::Nvml::init() {
        Ok(nvml) => nvml
            .device_by_index(0)
            .and_then(|d| d.memory_info())
            .ok()
            .map(|m| (m.total as f64 / GIB, true))
            .unwrap_or((0.0, false)),
        Err(_) => (0.0, false),
    };
    if !vram_ok {
        log::warn!("[video-engine] NVML недоступен — preflight проверяет только RAM/диск.");
    } else if vram_total_gb < need_vram_gb {
        return Err(format!(
            "Не хватает VRAM: нужно ≥ {:.1} ГБ на карте, фактически {:.1} ГБ. \
             Возьмите бандл с меньшим квантом (Q3_K для карт ~8 ГБ).",
            need_vram_gb, vram_total_gb
        ));
    }
    if ram_free_gb < need_ram_gb {
        return Err(format!(
            "Не хватает RAM: нужно ≥ {:.1} ГБ свободной, фактически {:.1} ГБ. \
             Закройте другие приложения или возьмите бандл легче.",
            need_ram_gb, ram_free_gb
        ));
    }
    if missing_bytes > 0.0 {
        // Диск: том, на котором живёт/будет лежать папка бандла (длиннейший префикс).
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let mut best: Option<(usize, u64)> = None;
        for d in disks.list() {
            let mp = d.mount_point();
            if bundle_dir.starts_with(mp) {
                let len = mp.as_os_str().len();
                if best.map(|(l, _)| len > l).unwrap_or(true) {
                    best = Some((len, d.available_space()));
                }
            }
        }
        if let Some((_, free)) = best {
            if (free as f64) < missing_bytes {
                return Err(format!(
                    "Не хватает места на диске: для докачки нужно ≥ {:.1} ГБ на томе {}, \
                     свободно {:.1} ГБ. Освободите место или смените папку видео-бандла.",
                    missing_bytes / GIB,
                    bundle_dir.display(),
                    free as f64 / GIB
                ));
            }
        }
    }
    let vram_txt = if vram_ok {
        format!("{:.1} ГБ", vram_total_gb)
    } else {
        "N/A (NVML недоступен)".to_string()
    };
    let disk_txt = if missing_bytes > 0.0 {
        format!("нужно {:.1} ГБ на докачку", missing_bytes / GIB)
    } else {
        "всё уже скачано".to_string()
    };
    Ok(format!(
        "Preflight видео-бандла «{}»: карта — {}, порог VRAM {:.1} ГБ; RAM свободна {:.1} ГБ (порог {:.1}); \
         диск: {}; полный бандл {:.1} ГБ — годится.",
        entry.name,
        vram_txt,
        need_vram_gb,
        ram_free_gb,
        need_ram_gb,
        disk_txt,
        full_bytes / GIB
    ))
}

fn pick_free_port() -> Option<u16> {
    // Тот же аргумент, что у image_engine: случайный старт, bind-проверка.
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
    let start = (xorshift() % VIDEO_PORT_RANGE as u64) as u16;
    for i in 0..5u32 {
        let candidate = VIDEO_PORT_MIN + (start + (i * 137) as u16) % VIDEO_PORT_RANGE;
        if std::net::TcpListener::bind(("127.0.0.1", candidate)).is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn pipe_log<R>(stream: Option<R>, tag: &str)
where
    R: Read + Send + 'static,
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
    let raw = s.split_once(',').map(|(_, b)| b).unwrap_or(s);
    base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| format!("Ошибка декодирования base64 видео: {}", e))
}

#[derive(Deserialize, Debug)]
struct JobSubmit {
    id: String,
}

#[derive(Deserialize, Debug)]
struct JobStatus {
    status: String,
    result: Option<VidJobResult>,
    error: Option<VidJobError>,
}

#[derive(Deserialize, Debug)]
struct VidJobResult {
    b64_json: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    fps: Option<u32>,
    #[serde(default)]
    frame_count: Option<u64>,
}

#[derive(Deserialize, Debug)]
struct VidJobError {
    #[allow(dead_code)]
    code: Option<String>,
    message: Option<String>,
}

/// VideoEngine — живой процесс sd-server, обслуживающий vid_gen.
pub struct VideoEngine {
    child: std::process::Child,
    base_url: String,
    client: reqwest::blocking::Client,
    entry_name: String,
}

impl VideoEngine {
    /// Spawn sd-server с флагами ИЗ БАНДЛА (инвариант PRP: «флаги живут в каталоге, не в коде»).
    pub fn new<L>(
        engine_dir: &Path,
        bundle_dir: &Path,
        entry: &VideoBundleEntry,
        variant_pref: Option<&str>,
        log_cb: L,
    ) -> Result<Self, String>
    where
        L: Fn(String),
    {
        let source = "leejet";
        let variant = sdcpp_installer::resolve_variant(variant_pref);
        let mut server_exe = sdcpp_installer::variant_dir(engine_dir, source, &variant).join("sd-server.exe");
        if !server_exe.exists() {
            let legacy = engine_dir.join("sd-server.exe");
            if legacy.exists() {
                server_exe = legacy;
            } else {
                return Err(
                    "Движок видео не установлен (нет sd-server.exe).\n\
                     Откройте Настройки → «Движок изображений» и нажмите «Установить движок»."
                        .to_string(),
                );
            }
        }
        log_cb(format!(
            "🚀 spawn sd-server (видео): {} (variant {})",
            server_exe.display(),
            variant
        ));

        let msg = preflight_video(entry, bundle_dir)?;
        log_cb(format!("✅ {}", msg));

        let weights = bundle_files_weights(bundle_dir, &entry.files);
        let port = pick_free_port()
            .ok_or_else(|| "Не найден свободный порт для sd-server (19400..20900).".to_string())?;
        let mut args = build_spawn_args(entry, &weights, port)?;
        // LoRA-каталог сервера: файл роли lora_draft лежит плоско в bundle_dir
        // (SSOT каталога), lora[].path в payload — имя файла относительно него.
        if lora_draft_file(entry).is_some() {
            args.push("--lora-model-dir".to_string());
            args.push(bundle_dir.to_string_lossy().to_string());
        }
        log_cb(format!("🎬 spawn args: {}", args.join(" ")));

        let mut cmd = std::process::Command::new(&server_exe);
        cmd.args(&args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Не удалось запустить sd-server (видео): {}", e))?;
        register_engine_pid(child.id());
        assign_child_to_kill_job(&child);
        log_cb(format!("✅ sd-server видео запущен (PID {})", child.id()));

        pipe_log(child.stdout.take(), "sd-server-video");
        pipe_log(child.stderr.take(), "sd-server-video");

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("Ошибка HTTP-клиента: {}", e))?;

        let engine = Self {
            child,
            base_url: format!("http://127.0.0.1:{}", port),
            client,
            entry_name: entry.name.clone(),
        };
        engine.wait_ready(&log_cb)?;
        Ok(engine)
    }

    /// Модель грузится ДО listen — первый успешный `/sdcpp/v1/capabilities` =
    /// сервер с готовыми весами. 200 без vid_gen в supported_modes = старый
    /// релиз движка (MiniMax-H3 появился 2026-08-04) — честный отказ.
    fn wait_ready<L>(&self, log_cb: &L) -> Result<(), String>
    where
        L: Fn(String),
    {
        log_cb(format!("⏳ Загрузка видео-бандла «{}»…", self.entry_name));
        let start = Instant::now();
        loop {
            let resp = self
                .client
                .get(format!("{}/sdcpp/v1/capabilities", self.base_url))
                .send();
            match resp {
                Ok(r) if r.status().as_u16() == 404 => {
                    return Err(
                        "Установленный движок слишком старый: нет /sdcpp/v1/capabilities. \
                         Обновите движок через «Установить движок» (релиз с MiniMax-H3 ≥ 2026-08-04)."
                            .to_string(),
                    );
                }
                Ok(r) if r.status().is_success() => {
                    let modes: Vec<String> = r
                        .json::<serde_json::Value>()
                        .ok()
                        .and_then(|v| {
                            v.get("supported_modes")
                                .and_then(|m| m.as_array())
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|x| x.as_str().map(String::from))
                                        .collect()
                                })
                        })
                        .unwrap_or_default();
                    if !modes.is_empty() && !modes.iter().any(|m| m == "vid_gen") {
                        return Err(format!(
                            "Движок не заявляет режим vid_gen (supported_modes: {}). \
                             Обновите движок до релиза ≥ 2026-08-04.",
                            modes.join(", ")
                        ));
                    }
                    log_cb("✅ sd-server видео готов.".to_string());
                    return Ok(());
                }
                _ => {}
            }
            if start.elapsed() > VIDEO_HEALTH_TIMEOUT {
                return Err(
                    "sd-server видео не стал готов за 300 сек — веса H3 не загрузились. \
                     Диагностика в логе [sd-server-video].".to_string(),
                );
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    /// Один vid_gen-вызов: submit → poll → сохранение контейнера на диск.
    /// draft/final — разные ШАГИ одного forward; двухфазность (draft → затем
    /// fnial) собирает командный слой ДВУМЯ вызовами generate.
    pub fn generate<L>(
        &self,
        params: &VideoRequest,
        out_path: &Path,
        cancel_flag: Option<Arc<AtomicBool>>,
        log_cb: L,
    ) -> Result<VideoGenResult, String>
    where
        L: Fn(String),
    {
        let mode = params.mode()?;
        let body = build_payload(params)?;
        let t0 = Instant::now();
        log_cb(format!(
            "🎬 vid_gen {}: {}x{}, steps={}, frames={}, fps={}, refs={}{}",
            mode.as_str(),
            params.width,
            params.height,
            params.steps,
            params.video_frames,
            params.fps,
            params.ref_images_b64.len(),
            if params.init_b64.is_some() { ", init_image задан" } else { "" },
        ));

        let submit: JobSubmit = self
            .client
            .post(format!("{}/sdcpp/v1/vid_gen", self.base_url))
            .json(&body)
            .send()
            .map_err(|e| format!("Ошибка submit vid_gen: {}", e))?
            .error_for_status()
            .map_err(|e| {
                let code = e.status().map(|s| s.as_u16()).unwrap_or(0);
                format!(
                    "sd-server отклонил vid_gen ({}): частые причины — билд движка без WebM \
                     (нужен для output_format=webm), модель ещё не готова (503), невалидные \
                     параметры (400).",
                    code
                )
            })?
            .json()
            .map_err(|e| format!("Ошибка парсинга ответа submit: {}", e))?;
        log_cb(format!("🆔 job {} отправлен", submit.id));

        let (b64, mime, fps, frames) = self.poll_job(&submit.id, cancel_flag, &log_cb)?;
        let bytes = base64_decode(&b64)?;
        let ext = ext_for_mime(&mime);
        let out_path: PathBuf = if out_path.extension().is_none() {
            out_path.with_extension(ext)
        } else {
            out_path.to_path_buf()
        };
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    format!("Не удалось создать папку {}: {}", parent.display(), e)
                })?;
            }
        }
        std::fs::write(&out_path, &bytes)
            .map_err(|e| format!("Не удалось записать {}: {}", out_path.display(), e))?;
        let time_sec = t0.elapsed().as_secs_f64();
        log_cb(format!(
            "✅ Видео готово за {:.1} сек ({} кадр.) → {}",
            time_sec,
            frames,
            out_path.display()
        ));
        Ok(VideoGenResult {
            path: out_path.to_string_lossy().to_string(),
            fps,
            frame_count: frames,
            mime_type: mime,
            time_sec,
        })
    }

    fn poll_job<L>(
        &self,
        job_id: &str,
        cancel_flag: Option<Arc<AtomicBool>>,
        log_cb: &L,
    ) -> Result<(String, String, u32, u64), String>
    where
        L: Fn(String),
    {
        let start = Instant::now();
        loop {
            if let Some(flag) = &cancel_flag {
                if flag.load(Ordering::SeqCst) {
                    let _ = self
                        .client
                        .post(format!("{}/sdcpp/v1/jobs/{}/cancel", self.base_url, job_id))
                        .send();
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
                    let r = st.result.ok_or_else(|| {
                        "Job завершён «completed», но без result-контейнера (аварийный ответ сервера)."
                            .to_string()
                    })?;
                    let b64 = r
                        .b64_json
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| "result.b64_json пуст — сервер не вернул контейнер.".to_string())?;
                    let mime = r.mime_type.unwrap_or_else(|| "video/webm".to_string());
                    let fps = r.fps.unwrap_or(24);
                    let frames = r.frame_count.unwrap_or(0);
                    log_cb(format!("🎬 job {} готов: {} кадр. (fps {}).", job_id, frames, fps));
                    return Ok((b64, mime, fps, frames));
                }
                "failed" => {
                    let msg = st
                        .error
                        .and_then(|e| e.message)
                        .unwrap_or_else(|| "без сообщения сервера".to_string());
                    return Err(format!("Видео-генерация не удалась: {}", msg));
                }
                "cancelled" => return Err("CANCELLED".to_string()),
                _ => {}
            }
            if start.elapsed() > VIDEO_GEN_TIMEOUT {
                let _ = self
                    .client
                    .post(format!("{}/sdcpp/v1/jobs/{}/cancel", self.base_url, job_id))
                    .send();
                return Err("Таймаут видео-генерации (60 мин) — job отменён.".to_string());
            }
            log_cb(format!("⏳ Рендер видео… {:.0} сек", start.elapsed().as_secs()));
            std::thread::sleep(VIDEO_POLL_INTERVAL);
        }
    }
}

impl Drop for VideoEngine {
    fn drop(&mut self) {
        unregister_engine_pid(self.child.id());
        kill_process_tree(&mut self.child);
    }
}

/// One-shot видео (t2va): spawn → vid_gen → Drop. Точка входа host-tools:
/// дефолты ИЗ ПРЕСЕТА каталога (SSOT — tools-слой не дублирует константы),
/// seconds клампится по образцу GUI-команды `generate_video` (0 → 5, свыше
/// max_seconds → кламп с warn), сэмплинг решает `sampling_plan_for_quality`
/// (draft/final по наличию файла lora_draft; турбо-точка lcm/simple/shift —
/// из каталога). init/end/refs — не на этом уровне: все четыре режима
/// доступны через `VideoEngine::generate` напрямую.
pub fn generate_video_simple<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &VideoBundleEntry,
    prompt: &str,
    seconds: u32,
    final_quality: bool,
    out_path: &Path,
    cancel_flag: Option<Arc<AtomicBool>>,
    log_cb: L,
) -> Result<VideoGenResult, String>
where
    L: Fn(String) + Clone,
{
    let preset = &entry.preset;
    let mut secs = seconds;
    if secs == 0 {
        log_cb("⚠️ seconds=0 → дефолт 5 сек.".to_string());
        secs = 5;
    }
    if secs > preset.max_seconds {
        log_cb(format!(
            "⚠️ Клип {} сек выше лимита каталога «{}» ({} сек) — клампим.",
            secs, entry.name, preset.max_seconds
        ));
        secs = preset.max_seconds;
    }
    let plan = sampling_plan_for_quality(entry, bundle_dir, final_quality);
    let req = VideoRequest {
        prompt: prompt.to_string(),
        negative_prompt: None,
        init_b64: None,
        end_b64: None,
        ref_images_b64: Vec::new(),
        width: preset.width,
        height: preset.height,
        steps: plan.steps,
        cfg_scale: preset.cfg_scale,
        seed: preset.seed,
        video_frames: secs * preset.fps,
        fps: preset.fps,
        vae_tiling: true,
        strength: 0.75,
        sample_method: plan.sample_method,
        scheduler: plan.scheduler,
        flow_shift: plan.flow_shift,
        lora: plan.lora,
    };
    let engine = VideoEngine::new(engine_dir, bundle_dir, entry, None, log_cb.clone())?;
    engine.generate(&req, out_path, cancel_flag, log_cb)
}

// ───────────────────────────────── Тесты ─────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::models_catalog::{
        BundleFile, VideoPreset,
        {bundle_files_weights, default_video_bundle_entry},
    };
    use std::collections::HashMap;

    /// Синтетическая запись: полная конструкция для preflight/spawn тестов.
    fn synthetic_entry(
        files: Vec<(&str, &str, Option<u64>)>,
        spawn_flags: Option<&str>,
        vram_min: Option<f32>,
        ram_min: Option<f32>,
    ) -> VideoBundleEntry {
        VideoBundleEntry {
            name: "stub".to_string(),
            label: "stub".to_string(),
            note: String::new(),
            is_default: false,
            files: files
                .into_iter()
                .map(|(role, filename, size)| BundleFile {
                    role: role.to_string(),
                    filename: filename.to_string(),
                    download_url: String::new(),
                    size_bytes: size,
                })
                .collect(),
            spawn_flags: spawn_flags.map(String::from),
            model_args: None,
            preset: VideoPreset::default(),
            size_gb: None,
            vram_fast_gb: None,
            vram_min_gb: vram_min,
            ram_min_gb: ram_min,
        }
    }

    /// Каталожный бандл с реально созданными файлами в temp-папке
    /// (build_spawn_args проверяет существование — файлы должны быть на диске).
    /// Файлы роли lora_draft НЕ создаются: это дефолтное состояние продукта
    /// (роль в каталоге, файл ещё не скачан) — draft обязан уходить в fallback.
    fn catalog_bundle_with_files() -> (VideoBundleEntry, PathBuf) {
        let entry = default_video_bundle_entry().expect("дефолтный видео-бандл обязан существовать");
        let dir = std::env::temp_dir().join(format!("ko_video_engine_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp папка создалась");
        for f in &entry.files {
            if f.role == "lora_draft" {
                continue;
            }
            let p = dir.join(&f.filename);
            if !p.exists() {
                std::fs::write(&p, b"weights-placeholder").expect("temp файл создан");
            }
        }
        (entry, dir)
    }

    #[test]
    fn resolve_mode_covers_all_four_and_rejects_mixes() {
        assert_eq!(resolve_mode(None, None, &[]).unwrap(), VideoMode::T2Va);
        assert_eq!(
            resolve_mode(Some("data:image/png;base64,AA"), None, &[]).unwrap(),
            VideoMode::I2Va
        );
        assert_eq!(resolve_mode(Some("AA"), Some("BB"), &[]).unwrap(), VideoMode::Fl2Va);
        assert_eq!(resolve_mode(None, None, &[String::from("AA")]).unwrap(), VideoMode::Ref2Va);
        // ref2va + init — запрещено (api.md).
        assert!(resolve_mode(Some("AA"), None, &[String::from("X")]).is_err());
        // end без init — запрещено (FL2VA требует оба).
        assert!(resolve_mode(None, Some("BB"), &[]).is_err());
    }

    #[test]
    fn spawn_args_carry_bind_and_catalog_flags_but_no_image_flags() {
        let (entry, dir) = catalog_bundle_with_files();
        let weights = bundle_files_weights(&dir, &entry.files);
        let args = build_spawn_args(&entry, &weights, 20900).expect("spawn args собран");
        let j = args.join(" ");
        for flag in [
            "--diffusion-model",
            "--vae",
            "--audio-vae",
            "--llm",
            "--listen-ip",
            "127.0.0.1",
            "--listen-port",
            "20900",
            "--offload-to-cpu", // из каталожного spawn_flags
            "--rng",           // --rng cpu — тоже из каталога
        ] {
            assert!(j.contains(flag), "не хватает флага {}", flag);
        }
        assert!(!j.contains("--auto-fit"), "image-флаг запрещён видео");
        assert!(!j.contains("--max-vram"), "image-флаг запрещён видео");
        assert!(!j.contains("--fa"), "image-флаг запрещён видео");
    }

    #[test]
    fn spawn_args_fail_on_empty_weights_and_empty_flags() {
        let (entry, dir) = catalog_bundle_with_files();
        // 1) пустая карта ролей → отказ называет роль diffusion.
        let empty: HashMap<String, String> = HashMap::new();
        let err = build_spawn_args(&entry, &empty, 19900).unwrap_err();
        assert!(err.contains("diffusion"), "отказ называет роль: {}", err);
        // 2) пустые spawn_flags → дефект каталога, честный отказ.
        let mut noflag = synthetic_entry(
            vec![("diffusion", "w.gguf", Some(1))],
            None,
            None,
            None,
        );
        noflag.name = "noflag".to_string();
        let w = bundle_files_weights(&dir, &entry.files);
        let err2 = build_spawn_args(&noflag, &w, 19900).unwrap_err();
        assert!(err2.contains("пуст"), "отказ про пустые флаги: {}", err2);
    }

    #[test]
    fn spawn_args_fail_when_weight_file_missing() {
        // Пути на несуществующую папку → честный отказ «Файл не найден».
        let (entry, _) = catalog_bundle_with_files();
        let ghost_dir = std::env::temp_dir().join("ko_video_engine_ghost_missing_dir");
        let weights = bundle_files_weights(&ghost_dir, &entry.files);
        let err = build_spawn_args(&entry, &weights, 19900).unwrap_err();
        assert!(
            err.contains("Файл видео-бандла не найден"),
            "отказ называет путь: {}",
            err
        );
    }

    #[test]
    fn plan_follows_quality_ladder() {
        let (entry, dir) = catalog_bundle_with_files();
        // Роль lora_draft в каталоге есть, но файла в bundle_dir нет (хелпер
        // lora-файлы не создаёт) → draft уходит в fallback: nolora-шаги,
        // euler/discrete, shift не шлётся, без LoRA.
        let draft = sampling_plan_for_quality(&entry, &dir, false);
        assert_eq!(draft.steps, entry.preset.steps_draft_nolora);
        assert_eq!(draft.sample_method, "euler");
        assert_eq!(draft.scheduler, "discrete");
        assert!(draft.flow_shift.is_none());
        assert!(draft.lora.is_empty());
        // final — лестница качества: steps_final, euler/discrete, без LoRA.
        let fin = sampling_plan_for_quality(&entry, &dir, true);
        assert_eq!(fin.steps, entry.preset.steps_final);
        assert_eq!(fin.sample_method, "euler");
        assert_eq!(fin.scheduler, "discrete");
        assert!(fin.flow_shift.is_none());
        assert!(fin.lora.is_empty());
        // Синтетика с файлом lora_draft на диске → турбо-точка каталога:
        // steps_draft, lcm/simple/shift 12, одна LoRA ×1.0. Роль в каталоге уже
        // есть (DMAD) — переименовываем её файл на заглушку, а не дублируем роль
        // (find берёт первую lora_draft).
        let mut with_lora = entry.clone();
        let lora_ref = with_lora
            .files
            .iter_mut()
            .find(|f| f.role == "lora_draft")
            .expect("роль lora_draft в каталожной записи");
        lora_ref.filename = "fake_turbo_lora.safetensors".to_string();
        let p = dir.join("fake_turbo_lora.safetensors");
        std::fs::write(&p, b"lora-stub").expect("lora stub создан");
        let turbo = sampling_plan_for_quality(&with_lora, &dir, false);
        assert_eq!(turbo.steps, entry.preset.steps_draft);
        assert_eq!(turbo.sample_method, "lcm");
        assert_eq!(turbo.scheduler, "simple");
        assert_eq!(turbo.flow_shift, Some(12.0));
        assert_eq!(turbo.lora.len(), 1);
        assert_eq!(turbo.lora[0].path, "fake_turbo_lora.safetensors");
        assert!((turbo.lora[0].multiplier - 1.0).abs() < f32::EPSILON);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn payload_t2va_minimal_contract_fields() {
        let p = VideoRequest {
            prompt: "кот на волне".into(),
            negative_prompt: None,
            init_b64: None,
            end_b64: None,
            ref_images_b64: vec![],
            width: 864,
            height: 480,
            steps: 6,
            cfg_scale: 1.0,
            seed: -1,
            video_frames: seconds_to_frames(5, 24),
            fps: 24,
            vae_tiling: true,
            strength: 0.75,
            // euler-путь (final / draft-без-LoRA): shift не шлётся, без LoRA.
            sample_method: "euler".into(),
            scheduler: "discrete".into(),
            flow_shift: None,
            lora: vec![],
        };
        let body = build_payload(&p).expect("t2va payload собран");
        assert_eq!(body["prompt"], "кот на волне");
        assert_eq!(body["width"], 864);
        assert_eq!(body["height"], 480);
        assert_eq!(body["video_frames"], 120);
        assert_eq!(body["fps"], 24);
        assert_eq!(body["seed"], -1);
        assert_eq!(body["strength"], 0.75);
        assert_eq!(body["sample_params"]["sample_steps"], 6);
        assert_eq!(body["sample_params"]["sample_method"], "euler");
        assert_eq!(body["sample_params"]["scheduler"], "discrete");
        assert_eq!(body["sample_params"]["guidance"]["txt_cfg"], 1.0);
        assert_eq!(body["output_format"], "webm");
        assert_eq!(body["control_frames"], serde_json::json!([]));
        assert_eq!(body["lora"], serde_json::json!([]));
        assert_eq!(body["high_noise_sample_params"], serde_json::Value::Null);
        assert_eq!(body["vae_tiling_params"]["enabled"], true);
        assert_eq!(body["vae_tiling_params"]["temporal_tiling"], false);
        // t2va: init/end — null, ref_images — пуст.
        assert_eq!(body["init_image"], serde_json::Value::Null);
        assert_eq!(body["end_image"], serde_json::Value::Null);
        assert_eq!(body["ref_images"], serde_json::json!([]));
    }

    /// Контракт турбо-ветки (DMAD): lcm/simple/shift 12 уходят в payload,
    /// lora[] содержит путь + множитель + is_high_noise:false (формат бенча
    /// 26.10.26, подтверждён логом 600/600 applied).
    #[test]
    fn payload_lora_turbo_contract() {
        let p = VideoRequest {
            prompt: "танцор в неоне".into(),
            negative_prompt: None,
            init_b64: None,
            end_b64: None,
            ref_images_b64: vec![],
            width: 864,
            height: 480,
            steps: 4,
            cfg_scale: 1.0,
            seed: 42,
            video_frames: seconds_to_frames(5, 24),
            fps: 24,
            vae_tiling: true,
            strength: 0.75,
            sample_method: "lcm".into(),
            scheduler: "simple".into(),
            flow_shift: Some(12.0),
            lora: vec![VideoLoraRef {
                path: "dmad_minimax_h3_4step_full_critic.safetensors".into(),
                multiplier: 1.0,
            }],
        };
        let body = build_payload(&p).expect("turbo payload собран");
        assert_eq!(body["sample_params"]["flow_shift"], 12.0);
        assert_eq!(body["sample_params"]["sample_method"], "lcm");
        assert_eq!(body["sample_params"]["scheduler"], "simple");
        assert_eq!(body["sample_params"]["sample_steps"], 4);
        assert_eq!(
            body["lora"],
            serde_json::json!([{
                "path": "dmad_minimax_h3_4step_full_critic.safetensors",
                "multiplier": 1.0,
                "is_high_noise": false
            }])
        );
    }

    /// Краш-регрессия 26.10.26: flow_shift: 0.0 ронял движок (CUDA illegal
    /// memory access). При None ключ flow_shift ОБЯЗАН ОТСУТСТВОВАТЬ в
    /// sample_params (не null — отсутствие), работает модельный дефолт.
    #[test]
    fn payload_omits_flow_shift_when_none() {
        let p = VideoRequest {
            prompt: "сцена".into(),
            negative_prompt: None,
            init_b64: None,
            end_b64: None,
            ref_images_b64: vec![],
            width: 864,
            height: 480,
            steps: 20,
            cfg_scale: 1.0,
            seed: -1,
            video_frames: 120,
            fps: 24,
            vae_tiling: true,
            strength: 0.75,
            sample_method: "euler".into(),
            scheduler: "discrete".into(),
            flow_shift: None,
            lora: vec![],
        };
        let body = build_payload(&p).expect("payload собран");
        let sp = body["sample_params"].as_object().expect("sample_params — объект");
        assert!(
            !sp.contains_key("flow_shift"),
            "ключ flow_shift при None обязан отсутствовать (0.0 = CUDA crash у H3), got: {:?}",
            sp.get("flow_shift")
        );
    }

    #[test]
    fn payload_i2va_fl2va_ref2va_shapes() {
        let base = |init: Option<&str>, end: Option<&str>, refs: Vec<String>| VideoRequest {
            prompt: "сцена".into(),
            negative_prompt: None,
            init_b64: init.map(String::from),
            end_b64: end.map(String::from),
            ref_images_b64: refs,
            width: 864,
            height: 480,
            steps: 20,
            cfg_scale: 1.0,
            seed: 7,
            video_frames: 56,
            fps: 24,
            vae_tiling: true,
            strength: 0.75,
            sample_method: "euler".into(),
            scheduler: "discrete".into(),
            flow_shift: None,
            lora: vec![],
        };
        let i2va = build_payload(&base(Some("AA"), None, vec![])).expect("i2va");
        assert_eq!(i2va["init_image"], "AA");
        assert_eq!(i2va["end_image"], serde_json::Value::Null);
        let fl2va = build_payload(&base(Some("AA"), Some("BB"), vec![])).expect("fl2va");
        assert_eq!(fl2va["init_image"], "AA");
        assert_eq!(fl2va["end_image"], "BB");
        let ref2va = build_payload(&base(None, None, vec!["R1".into(), "R2".into()])).expect("ref2va");
        assert_eq!(ref2va["ref_images"], serde_json::json!(["R1", "R2"]));
        assert_eq!(ref2va["init_image"], serde_json::Value::Null);
        // Запретные комбинации → честный отказ, а не payload.
        assert!(build_payload(&base(Some("AA"), None, vec!["R".into()])).is_err());
        assert!(build_payload(&base(None, Some("BB"), vec![])).is_err());
    }

    #[test]
    fn payload_rejects_broken_params() {
        let mut p = VideoRequest {
            prompt: String::new(),
            negative_prompt: None,
            init_b64: None,
            end_b64: None,
            ref_images_b64: vec![],
            width: 864,
            height: 480,
            steps: 6,
            cfg_scale: 1.0,
            seed: -1,
            video_frames: 120,
            fps: 24,
            vae_tiling: true,
            strength: 0.75,
            sample_method: "euler".into(),
            scheduler: "discrete".into(),
            flow_shift: None,
            lora: vec![],
        };
        assert!(build_payload(&p).unwrap_err().contains("промпт"));
        p.prompt = "ок".into();
        p.steps = 0;
        assert!(build_payload(&p).unwrap_err().contains("steps"));
        p.steps = 6;
        p.fps = 0;
        assert!(build_payload(&p).unwrap_err().contains("fps"));
    }

    #[test]
    fn preflight_rejects_on_ram_threshold() {
        // RAM-порог детерминирован на любой машине: ни на одной не бывает
        // 16000 ГБ свободной RAM, поэтому отказ вернёт «RAM», а не успех.
        let dir = std::env::temp_dir().join("ko_video_engine_pf_ram");
        std::fs::create_dir_all(&dir).unwrap();
        let e = synthetic_entry(vec![("diffusion", "w.gguf", Some(1))], Some("--offload-to-cpu"), None, Some(16000.0));
        let err = preflight_video(&e, &dir).unwrap_err();
        assert!(err.contains("RAM"), "отказ называет RAM: {}", err);
    }

    #[test]
    fn preflight_rejects_on_disk_threshold() {
        // Свежая temp-папка: файл бандла отсутствует → докачке нужен весь
        // размер; u64::MAX не бывает свободным → честный отказ «места на диске».
        let dir = std::env::temp_dir().join(format!("ko_video_engine_pf_disk_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let e = synthetic_entry(
            vec![("diffusion", "huge.gguf", Some(u64::MAX))],
            Some("--offload-to-cpu"),
            None,
            None,
        );
        let err = preflight_video(&e, &dir).unwrap_err();
        assert!(err.contains("диске"), "отказ называет диск: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seconds_to_frames_is_plain_multiplication() {
        assert_eq!(seconds_to_frames(5, 24), 120);
        assert_eq!(seconds_to_frames(15, 24), 360);
        assert_eq!(seconds_to_frames(0, 24), 0);
    }

    #[test]
    fn ext_for_mime_maps_container_table() {
        assert_eq!(ext_for_mime("video/webm"), "webm");
        assert_eq!(ext_for_mime("image/webp"), "webp");
        assert_eq!(ext_for_mime("video/x-msvideo"), "avi");
        assert_eq!(ext_for_mime("что-то иное"), "webm");
    }
}

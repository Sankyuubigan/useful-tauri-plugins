//! A/B-сравнение апскейла: Krea-2-Turbo против наших Qwen-Image-2.1 Turbo.
//!
//! Пресет Krea2 НЕ подобран на глаз — он выведен из официального
//! первоисточника `krea-ai/krea-2` (README + `sampling.py`), см. константы
//! ниже. Фактические значения каждого джоба пишутся в отчёт: сравнение
//! визуальное, без «программного» вердикта.
//!
//! Каталог продукта (`image_models_catalog.json`) НЕ трогаем: стек Krea2
//! живёт только здесь, как bench-манифест.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::bench::VramSampler;
use crate::engine::bench_ext::read_ref_b64;
use crate::engine::bench_upscale::UPSCALE_PROMPT_DEFAULT;
use crate::engine::image_engine::{upscale_target_dims, Img2Img, ImageEngine};
use crate::engine::models_catalog::{bundle_by_name, BundleFile, ImageBundleEntry, ImagePreset};
use crate::engine::ref_size;

// ───────────────────────── Пресет Krea-2-Turbo ─────────────────────────
// Первоисточник: github.com/krea-ai/krea-2 (README.md + sampling.py).
// • «Krea 2 Turbo is an 8-step distilled checkpoint».
// • «run with 8 steps and CFG disabled».
// • «--mu ... Recommended 1.15 for oss_turbo»; sampling.py: «Pass an explicit
//   mu to pin a constant shift regardless of resolution (used by the distilled
//   checkpoint, which was trained at a fixed mu=1.15)».
pub const KREA2_TURBO_STEPS: u32 = 8;
pub const KREA2_TURBO_MU: f32 = 1.15;

/// sd.cpp-эквивалент «CFG disabled»: `txt_cfg = 1.0` → `use_uncond = false`
/// (один cond-проход, guidance не усиливается) — request.cpp:303-319.
/// НЕ 0.0: при cfg=0 sd.cpp включает «unconditioned mode» и отдаёт
/// БЕЗУСЛОВНЫЙ выход (картинка промпту не следует) — request.cpp:311-319.
pub const KREA2_TURBO_CFG: f32 = 1.0;

/// Потолок меньшей стороны «до»-картинки: иначе ×2 упирается в 2048 и
/// сравнение перестаёт быть апскейлом ×2 (canvas = upscale_target_dims).
pub const KREA2_MAX_PRE_SIDE: u32 = 1024;

/// Один файл стека Krea2 (sd.cpp docs/krea2.md): diffusion + VAE + TE.
#[derive(Clone, Copy, Debug)]
pub struct Krea2File {
    pub role: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
    pub size_bytes: u64,
}

/// Стек Krea-2-Turbo. Размеры — HF API, 20.10.2026 (сверены с манифестом).
pub const KREA2_STACK: &[Krea2File] = &[
    Krea2File {
        role: "diffusion",
        filename: "Krea-2-Turbo-Q4_K_M.gguf",
        url: "https://huggingface.co/realrebelai/KREA-2_GGUFs/resolve/main/TURBO/Krea-2-Turbo-Q4_K_M.gguf?download=true",
        size_bytes: 7_216_993_376,
    },
    Krea2File {
        role: "vae",
        filename: "wan_2.1_vae.safetensors",
        url: "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/vae/wan_2.1_vae.safetensors?download=true",
        size_bytes: 253_815_318,
    },
    Krea2File {
        role: "llm",
        filename: "Qwen3VL-4B-Instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/resolve/main/Qwen3VL-4B-Instruct-Q4_K_M.gguf?download=true",
        size_bytes: 2_497_281_664,
    },
];

/// Сигма-сетка официального Turbo = reference `timesteps()`:
/// `ts = linspace(1, 0, steps+1)`; `ts = e^mu / (e^mu + (1/ts - 1)^sigma)`, sigma=1.
/// Формула СОВПАДАЕТ с sd.cpp (`flux_time_shift`, denoiser.hpp:725), но сетка по
/// умолчанию для krea2 — DISCRETE_SCHEDULER (krea2 не в списке flux/qwen,
/// request.cpp:57-80) → `t = 1 - i/(n-1)`, последняя оценка модели при t≈0.001
/// вместо reference t=0.125. Поэтому шлём `custom_sigmas` явно (contract
/// sd.cpp: длина − 1 = steps, последний элемент 0.0).
pub fn krea2_turbo_sigmas(steps: u32) -> Vec<f32> {
    let n = steps as f64;
    let e = (KREA2_TURBO_MU as f64).exp();
    let mut sigmas = Vec::with_capacity(steps as usize + 1);
    for i in 0..=steps {
        let t = 1.0 - (i as f64) / n;
        let s = if t <= 0.0 {
            0.0
        } else {
            e / (e + (1.0 / t - 1.0))
        };
        sigmas.push(s as f32);
    }
    sigmas
}

/// Синтетический бандл Krea2 для bench (в каталог продукта не входит).
pub fn krea2_entry() -> ImageBundleEntry {
    ImageBundleEntry {
        name: "krea2-turbo-q4km".to_string(),
        label: "Krea-2-Turbo Q4_K_M (sd.cpp: Wan2.1 VAE + Qwen3-VL-4B)".to_string(),
        note: "Стек Krea2 для A/B-апскейла. Не в каталоге продукта.".to_string(),
        is_default: false,
        files: KREA2_STACK
            .iter()
            .map(|f| BundleFile {
                role: f.role.to_string(),
                filename: f.filename.to_string(),
                download_url: f.url.to_string(),
                size_bytes: Some(f.size_bytes),
                modes: Vec::new(),
                is_default: false,
            })
            .collect(),
        preset: ImagePreset {
            cfg_scale: KREA2_TURBO_CFG,
            sampler: "euler".to_string(),
            width: 1024,
            height: 1024,
            steps: KREA2_TURBO_STEPS,
            seed: -1,
            custom_sigmas: Some(krea2_turbo_sigmas(KREA2_TURBO_STEPS)),
            scheduler: None,
            native_sigma_preset: None,
        },
        model_args: None,
        compute_buffers_mb: None,
        // Синтетический бенч-бандл вне каталога продукта — легаси-плоский
        // каталог лор (None), чтобы не менять поведение бенчей.
        lora_base: None,
        // F4: бенч-стенд не в каталоге — контракт ref-edit не объявлен
        // (сравнение идёт по t2i/img2img, ref-edit бенчу не нужен).
        ref_edit_mode: None,
        ref_edit_lora: None,
        ref_edit_preset: None,
        ref_edit_lora_strength: 1.0,
        size_gb: None,
        vram_fast_gb: None,
        vram_min_gb: None,
        ram_min_gb: None,
    }
}

// ───────────────────────── Чужие картинки (манифест) ─────────────────────────

/// Одна чужая картинка: рецепт (из реестра Commons) + факты после скачивания.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KreaSource {
    pub id: String,
    pub commons_title: String,
    pub source_url: String,
    pub license: String,
    pub license_url: String,
    /// Автор/атрибуция (из лицензии Commons).
    #[serde(default)]
    pub author: String,
    /// Имя файла в каталоге `src/`.
    pub file: String,
    /// Размеры скачанного файла (факт, снят по заголовку).
    #[serde(default)]
    pub w: u32,
    #[serde(default)]
    pub h: u32,
    /// sha256 скачанного файла.
    #[serde(default)]
    pub sha256: String,
}

impl KreaSource {
    /// Меньшая сторона — «до»-размер источника.
    pub fn min_side(&self) -> u32 {
        self.w.min(self.h)
    }
}

/// Прочитать рецепт источников (JSON-массив).
pub fn load_sources(path: &Path) -> Result<Vec<KreaSource>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("Прочитать {}: {}", path.display(), e))?;
    serde_json::from_str(&raw).map_err(|e| format!("Разобрать {}: {}", path.display(), e))
}

/// Записать манифест (факты: размеры, sha256, лицензии).
pub fn save_manifest(path: &Path, sources: &[KreaSource]) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("Создать {}: {}", p.display(), e))?;
    }
    let json = serde_json::to_string_pretty(sources).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("Записать {}: {}", path.display(), e))
}

/// Скачать источники (skip-existing по размеру+sha256), проверить «до»-размер
/// (меньшая сторона ≤ KREA2_MAX_PRE_SIDE) и записать манифест.
pub fn download_sources<L>(
    recipe_path: &Path,
    src_dir: &Path,
    manifest_path: &Path,
    log_cb: L,
) -> Result<Vec<KreaSource>, String>
where
    L: Fn(String),
{
    let mut sources = load_sources(recipe_path)?;
    std::fs::create_dir_all(src_dir).map_err(|e| format!("Создать {}: {}", src_dir.display(), e))?;
    for src in sources.iter_mut() {
        let dest = src_dir.join(&src.file);
        let have = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        if have == 0 {
            log_cb(format!("⬇ Скачиваю {} → {}", src.commons_title, dest.display()));
            download_file(&src.source_url, &dest, 0)?;
        } else {
            log_cb(format!("⏭ Уже скачан: {}", dest.display()));
        }
        let (w, h) = ref_size::dimensions_from_path(&dest)
            .ok_or_else(|| format!("Не определить размеры {} (неизвестный формат?)", dest.display()))?;
        src.w = w;
        src.h = h;
        src.sha256 = sha256_file(&dest)?;
        if src.min_side() > KREA2_MAX_PRE_SIDE {
            return Err(format!(
                "Источник {}: меньшая сторона {} > {} — «до»-размер нечестный (×2 упирается в 2048).",
                src.id,
                src.min_side(),
                KREA2_MAX_PRE_SIDE
            ));
        }
    }
    save_manifest(manifest_path, &sources)?;
    log_cb(format!("📄 Манифест: {}", manifest_path.display()));
    Ok(sources)
}

// ───────────────────────── Ноги сравнения и прогон ─────────────────────────

/// Нога сравнения: id, подпись, бандл каталога (пусто = стек Krea2).
#[derive(Clone, Debug)]
pub struct KreaLeg {
    pub id: String,
    pub label: String,
    pub catalog_bundle: String,
}

/// Ноги сравнения по умолчанию: наш дефолт, наш эконом, challenger.
pub fn default_legs() -> Vec<KreaLeg> {
    vec![
        KreaLeg {
            id: "ours_unsloth_d045".to_string(),
            label: "OURS unsloth Q5_K_S".to_string(),
            catalog_bundle: "qwen-image-2.1-turbo-unsloth-q5_ks".to_string(),
        },
        KreaLeg {
            id: "ours_q4km_d045".to_string(),
            label: "OURS viggle Q4_K_M".to_string(),
            catalog_bundle: "qwen-image-2.1-turbo-viggle-q4km".to_string(),
        },
        KreaLeg {
            id: "krea2_turbo_d045".to_string(),
            label: "KREA2 Turbo Q4_K_M".to_string(),
            catalog_bundle: String::new(),
        },
    ]
}

/// Один результат: (картинка × нога).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KreaCell {
    pub source_id: String,
    pub leg: String,
    pub image_path: String,
    pub gen_time_s: f64,
    pub vram_peak_mb: Option<f64>,
    pub ok: bool,
    pub error: Option<String>,
}

/// Фактически использованный пресет Krea2 (для отчёта и манифеста).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Krea2PresetInfo {
    pub steps: u32,
    pub cfg_scale: f32,
    pub mu: f32,
    pub custom_sigmas: Vec<f32>,
    pub diffusion_file: String,
    pub diffusion_size_bytes: u64,
    pub vae_file: String,
    pub llm_file: String,
    pub source: String,
}

/// Отчёт сравнения → `report.krea_compare.json`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KreaCompareReport {
    pub prompt: String,
    pub strength: f32,
    pub seed: i64,
    pub canvas_rule: String,
    pub krea2_preset: Krea2PresetInfo,
    pub legs: Vec<KreaLegInfo>,
    pub cells: Vec<KreaCell>,
    pub fail_count: usize,
    pub notes: Vec<String>,
}

/// Описание ноги в отчёте.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KreaLegInfo {
    pub id: String,
    pub label: String,
    pub bundle: String,
    pub steps: u32,
    pub cfg_scale: f32,
    pub custom_sigmas: Option<Vec<f32>>,
}

/// Имя выхода: `K{id}_d045.png` (в папке ноги).
pub fn krea_out_name(source_id: &str, strength: f32) -> String {
    format!("K{}_{}.png", source_id, crate::engine::bench_upscale::denoise_tag(strength))
}

/// Прогон сравнения: спавн сервера на ногу, затем джобы по картинкам.
#[allow(clippy::too_many_arguments)]
pub fn run_krea_compare<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    krea_dir: &Path,
    out_dir: &Path,
    sources: &[KreaSource],
    legs: &[KreaLeg],
    strength: f32,
    seed: i64,
    log_cb: L,
) -> Result<KreaCompareReport, String>
where
    L: Fn(String) + Clone,
{
    std::fs::create_dir_all(out_dir).map_err(|e| format!("Создать {}: {}", out_dir.display(), e))?;
    let mut cells: Vec<KreaCell> = Vec::new();
    let mut leg_infos: Vec<KreaLegInfo> = Vec::new();

    for leg in legs {
        let (entry, weights_dir) = if leg.catalog_bundle.is_empty() {
            (krea2_entry(), krea_dir.to_path_buf())
        } else {
            let e = bundle_by_name(&leg.catalog_bundle)
                .ok_or_else(|| format!("Бандл {} не в каталоге продукта", leg.catalog_bundle))?;
            (e, bundle_dir.to_path_buf())
        };
        let leg_dir = out_dir.join(&leg.id);
        std::fs::create_dir_all(&leg_dir).map_err(|e| format!("Создать {}: {}", leg_dir.display(), e))?;

        let load_start = std::time::Instant::now();
        let engine = ImageEngine::new_with_policy(
            engine_dir,
            &weights_dir,
            &entry,
            None,
            crate::engine::image_engine::PreflightPolicy::Measure,
            log_cb.clone(),
        )?;
        log_cb(format!(
            "⏱ Нога {}: сервер поднят за {:.1} сек.",
            leg.id,
            load_start.elapsed().as_secs_f64()
        ));
        leg_infos.push(KreaLegInfo {
            id: leg.id.clone(),
            label: leg.label.clone(),
            bundle: if leg.catalog_bundle.is_empty() {
                "krea2-turbo-q4km (bench, вне каталога)".to_string()
            } else {
                leg.catalog_bundle.clone()
            },
            steps: entry.preset.steps,
            cfg_scale: entry.preset.cfg_scale,
            custom_sigmas: entry.preset.custom_sigmas.clone(),
        });

        for src in sources {
            let src_path = out_dir.join("src").join(&src.file);
            if !src_path.exists() {
                let msg = format!("Не найден источник {}", src_path.display());
                log_cb(format!("⚠️ {} / {}: {}", leg.id, src.id, msg));
                cells.push(KreaCell {
                    source_id: src.id.clone(),
                    leg: leg.id.clone(),
                    image_path: String::new(),
                    gen_time_s: 0.0,
                    vram_peak_mb: None,
                    ok: false,
                    error: Some(msg),
                });
                continue;
            }
            let (w, h) = upscale_target_dims(src.w, src.h);
            let out_path = leg_dir.join(krea_out_name(&src.id, strength));
            if out_path.exists() {
                log_cb(format!("⏭ Уже есть {} — пропуск.", out_path.display()));
                cells.push(KreaCell {
                    source_id: src.id.clone(),
                    leg: leg.id.clone(),
                    image_path: out_path.to_string_lossy().to_string(),
                    gen_time_s: 0.0,
                    vram_peak_mb: None,
                    ok: true,
                    error: None,
                });
                continue;
            }
            let (init_b64, _, _) = read_ref_b64(&src_path).map_err(|e| format!("{}: {}", src.id, e))?;
            let sampler = VramSampler::start();
            log_cb(format!(
                "🖼 {} / {}: {}x{} → {}x{}, strength {:.2}",
                leg.id, src.id, src.w, src.h, w, h, strength
            ));
            match engine.generate(
                UPSCALE_PROMPT_DEFAULT,
                w,
                h,
                entry.preset.steps,
                entry.preset.cfg_scale,
                seed,
                &[],
                &out_path,
                None,
                log_cb.clone(),
                entry.preset.custom_sigmas.as_deref(),
                entry.preset.scheduler.as_deref(),
                None,
                Some(&Img2Img {
                    init_b64,
                    strength,
                }),
                &[],  // loras: krea_compare — метрики без LoRA-стека
                None, // ref_image_args: img2img и ref_images взаимоисключающие
            ) {
                Ok(res) => cells.push(KreaCell {
                    source_id: src.id.clone(),
                    leg: leg.id.clone(),
                    image_path: res.path,
                    gen_time_s: res.time_sec,
                    vram_peak_mb: sampler.peak(),
                    ok: true,
                    error: None,
                }),
                Err(e) => {
                    log_cb(format!("⚠️ {} / {} упал: {}", leg.id, src.id, e));
                    cells.push(KreaCell {
                        source_id: src.id.clone(),
                        leg: leg.id.clone(),
                        image_path: out_path.to_string_lossy().to_string(),
                        gen_time_s: 0.0,
                        vram_peak_mb: sampler.peak(),
                        ok: false,
                        error: Some(e),
                    });
                }
            }
        }
        drop(engine); // сервер ноги закрыт до следующей ноги (VRAM освобождается)
    }

    let fail_count = cells.iter().filter(|c| !c.ok).count();
    let report = KreaCompareReport {
        prompt: UPSCALE_PROMPT_DEFAULT.to_string(),
        strength,
        seed,
        canvas_rule: "canvas = upscale_target_dims(«до»-размер): ×2, потолок 2048 по стороне".to_string(),
        krea2_preset: Krea2PresetInfo {
            steps: KREA2_TURBO_STEPS,
            cfg_scale: KREA2_TURBO_CFG,
            mu: KREA2_TURBO_MU,
            custom_sigmas: krea2_turbo_sigmas(KREA2_TURBO_STEPS),
            diffusion_file: KREA2_STACK[0].filename.to_string(),
            diffusion_size_bytes: KREA2_STACK[0].size_bytes,
            vae_file: KREA2_STACK[1].filename.to_string(),
            llm_file: KREA2_STACK[2].filename.to_string(),
            source: "github.com/krea-ai/krea-2 (README + sampling.py): 8 шагов, CFG выкл, mu=1.15".to_string(),
        },
        legs: leg_infos,
        cells,
        fail_count,
        notes: vec![
            "Сравнение визуальное: публичных метрик качества у Krea2 нет — вердикт за юзером по HTML.".to_string(),
            "Все ноги: один промпт, одна strength, один canvas, один seed — различаются только веса/движок.".to_string(),
            "Krea2-нога: custom_sigmas посчитаны по формуле reference (mu=1.15, 9 значений, терминальный 0.0).".to_string(),
            "fail-строки остаются в знаменателе (fail_count в отчёте).".to_string(),
        ],
    };
    let path = out_dir.join("report.krea_compare.json");
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Записать {}: {}", path.display(), e))?;
    log_cb(format!(
        "📄 Отчёт: {} (ок {} / упало {})",
        path.display(),
        report.cells.len() - report.fail_count,
        report.fail_count
    ));
    Ok(report)
}

// ───────────────────────── Скачивание (веса и картинки) ─────────────────────────

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(1800))
        // Wikimedia отклоняет запросы без User-Agent (403), поэтому он задан явно.
        .user_agent("king-orch-bench/1.0 (local image bench)")
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| format!("Ошибка HTTP-клиента: {}", e))
}

fn part_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    dest.with_file_name(format!("{}.part", name))
}

/// Скачать файл с докачкой (Range) в `.part` → переименование после сверки
/// размера. `expect = 0` — размер неизвестен (картинки): сверяется только факт
/// непустого файла.
pub fn download_file(url: &str, dest: &Path, expect: u64) -> Result<(), String> {
    let part = part_path(dest);
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("Создать {}: {}", p.display(), e))?;
    }
    let client = http_client()?;
    let mut last_err = String::new();
    for _ in 0..5 {
        let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if expect > 0 && have >= expect {
            break;
        }
        let mut req = client.get(url);
        if have > 0 {
            req = req.header("Range", format!("bytes={}-", have));
        }
        match req.send() {
            Ok(mut resp) => {
                let status = resp.status();
                if !status.is_success() {
                    last_err = format!("HTTP {}", status);
                    continue;
                }
                let ranged = have > 0 && status != reqwest::StatusCode::OK;
                let mut f = if ranged {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&part)
                        .map_err(|e| format!("Открыть {}: {}", part.display(), e))?
                } else {
                    std::fs::File::create(&part)
                        .map_err(|e| format!("Создать {}: {}", part.display(), e))?
                };
                use std::io::Read;
                let mut buf = [0u8; 262144];
                loop {
                    match resp.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            use std::io::Write;
                            f.write_all(&buf[..n])
                                .map_err(|e| format!("Запись {}: {}", part.display(), e))?;
                        }
                        Err(e) => {
                            last_err = e.to_string();
                            break;
                        }
                    }
                }
                if last_err.is_empty() {
                    break;
                }
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    let size = std::fs::metadata(&part)
        .map(|m| m.len())
        .map_err(|e| format!("Нет файла {}: {}", part.display(), e))?;
    if size == 0 {
        return Err(format!("Скачивание {} не удалось: {}", url, last_err));
    }
    if expect > 0 && size != expect {
        return Err(format!(
            "Скачано {} байт вместо {} ({}): {}",
            size, expect, url, last_err
        ));
    }
    std::fs::rename(&part, dest)
        .map_err(|e| format!("Переименовать {} → {}: {}", part.display(), dest.display(), e))?;
    Ok(())
}

/// Скачать веса стека Krea2, если их ещё нет (skip-existing по точному размеру).
pub fn ensure_krea2_weights<L>(dir: &Path, log_cb: L) -> Result<(), String>
where
    L: Fn(String),
{
    std::fs::create_dir_all(dir).map_err(|e| format!("Создать {}: {}", dir.display(), e))?;
    for f in KREA2_STACK {
        let dest = dir.join(f.filename);
        let have = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        if have == f.size_bytes {
            log_cb(format!("⏭ Уже скачан: {}", dest.display()));
            continue;
        }
        if have > 0 {
            log_cb(format!(
                "⚠️ {}: на диске {} б, ожидалось {} — докачиваю.",
                dest.display(),
                have,
                f.size_bytes
            ));
        }
        log_cb(format!("⬇ Скачиваю {} ({:.2} ГБ)…", f.filename, f.size_bytes as f64 / 1e9));
        download_file(f.url, &dest, f.size_bytes)?;
        log_cb(format!("✅ Скачан: {}", dest.display()));
    }
    Ok(())
}

/// sha256 файла (hex).
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| format!("Открыть {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Чтение {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Контракт sd.cpp для custom_sigmas: длина − 1 = steps, первый 1.0,
    /// терминальный 0.0, строго убывающий.
    #[test]
    fn krea2_sigmas_follow_sdpp_contract() {
        let sig = krea2_turbo_sigmas(KREA2_TURBO_STEPS);
        assert_eq!(sig.len(), KREA2_TURBO_STEPS as usize + 1);
        assert!((sig[0] - 1.0).abs() < 1e-6, "первый sigma = {}", sig[0]);
        assert_eq!(sig[sig.len() - 1], 0.0, "терминальный sigma не 0.0");
        for w in sig.windows(2) {
            assert!(w[0] > w[1], "сигмы не убывают: {:?}", w);
        }
    }

    /// Сдвиг reference-реализации: sigma(t=0.5) = e^mu / (e^mu + 1),
    /// mu = 1.15 (дистиллят закреплён на постоянном mu — krea-ai/krea-2).
    #[test]
    fn krea2_shift_matches_reference_formula() {
        let sig = krea2_turbo_sigmas(2); // t = 1.0, 0.5, 0.0
        let e = (KREA2_TURBO_MU as f64).exp();
        let expect = (e / (e + 1.0)) as f32;
        assert!(
            (sig[1] - expect).abs() < 1e-6,
            "sigma(0.5) = {} вместо {}",
            sig[1],
            expect
        );
    }

    /// Файлы стека: роли и размеры — как в HF API (20.10.2026).
    #[test]
    fn krea2_stack_matches_hf_api() {
        let roles: Vec<&str> = KREA2_STACK.iter().map(|f| f.role).collect();
        assert_eq!(roles, vec!["diffusion", "vae", "llm"]);
        assert_eq!(KREA2_STACK[0].size_bytes, 7_216_993_376);
        assert_eq!(KREA2_STACK[1].size_bytes, 253_815_318);
        assert_eq!(KREA2_STACK[2].size_bytes, 2_497_281_664);
        let names: std::collections::HashSet<&str> = KREA2_STACK.iter().map(|f| f.filename).collect();
        assert_eq!(names.len(), KREA2_STACK.len(), "имена файлов стека не уникальны");
    }

    #[test]
    fn krea2_entry_uses_turbo_preset() {
        let e = krea2_entry();
        assert_eq!(e.preset.steps, 8);
        assert_eq!(e.preset.cfg_scale, 1.0);
        assert_eq!(e.preset.custom_sigmas.as_ref().map(|s| s.len()), Some(9));
        assert_eq!(e.files.len(), 3);
    }

    #[test]
    fn out_name_follows_protocol() {
        assert_eq!(krea_out_name("1", 0.45), "K1_d045.png");
    }

    #[test]
    fn manifest_json_round_trip_in_memory() {
        let src = KreaSource {
            id: "1".into(),
            commons_title: "File:Example.png".into(),
            source_url: "https://example.org/1.png".into(),
            license: "CC0".into(),
            license_url: "https://example.org/lic".into(),
            author: "Author".into(),
            file: "K1.png".into(),
            w: 800,
            h: 600,
            sha256: "abc".into(),
        };
        let json = serde_json::to_string(&vec![src.clone()]).unwrap();
        let back: Vec<KreaSource> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, "1");
        assert_eq!(back[0].min_side(), 600);
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use crate::engine::bench_prompts::env_dir;
    use crate::engine::krea_view::build_krea_html;

    fn resolve() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let engine_dir = env_dir("IMG_BENCH_ENGINE_DIR")
            .unwrap_or_else(|| crate::engine::config::engine_dir_early());
        let bundle_dir = env_dir("IMG_BENCH_BUNDLE_DIR")
            .or_else(|| {
                let cfg = crate::engine::config::load_image_config_early();
                cfg.image_bundle_dir
                    .filter(|s| !s.trim().is_empty())
                    .map(PathBuf::from)
            })
            .expect("IMG_BENCH_BUNDLE_DIR не задан и в app_config.json нет image_bundle_dir");
        let out_root = env_dir("IMG_BENCH_OUT").unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../king_orch_3/test/bench")
        });
        let out_dir = env_dir("IMG_KREA_OUT").unwrap_or_else(|| out_root.join("krea2"));
        let krea_dir = env_dir("IMG_KREA_DIR").unwrap_or_else(|| {
            bundle_dir
                .parent()
                .map(|p| p.join("krea2"))
                .unwrap_or_else(|| out_dir.join("weights"))
        });
        std::fs::create_dir_all(&out_dir).expect("создать out_dir");
        (engine_dir, bundle_dir, krea_dir, out_dir)
    }

    #[test]
    #[ignore = "Krea2 A/B-апскейл: веса ~10 ГБ + 5 картинок × 3 ноги (~40-60 мин, sd-server) — запуск по явной просьбе юзера"]
    fn krea2_compare() {        let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
        let (engine_dir, bundle_dir, krea_dir, out_dir) = resolve();
        let log = |m: String| {
            println!("[krea] {}", m);
            log::info!("[krea] {}", m);
        };

        ensure_krea2_weights(&krea_dir, log.clone()).expect("веса Krea2");

        let recipe = out_dir.join("krea_sources.json");
        assert!(recipe.exists(), "нет рецепта источников: {}", recipe.display());
        let sources = download_sources(
            &recipe,
            &out_dir.join("src"),
            &out_dir.join("krea_manifest.json"),
            log.clone(),
        )
        .expect("источники");
        assert!(!sources.is_empty(), "пустой список источников");

        let report = run_krea_compare(
            &engine_dir,
            &bundle_dir,
            &krea_dir,
            &out_dir,
            &sources,
            &default_legs(),
            crate::engine::image_engine::UPSCALE_STRENGTH_DEFAULT,
            42,
            log.clone(),
        )
        .expect("сравнение");
        let html = build_krea_html(&out_dir, &sources, &report).expect("html");
        println!("[krea] отчёт: {}", html.display());
        println!(
            "[krea] строк: {}, упало: {}",
            report.cells.len(),
            report.fail_count
        );
    }

    /// Пересборка HTML по уже готовым артефактам (без GPU): правится вёрстка
    /// отчёта — пересобирать, а не гонять заново 15 генераций.
    #[test]
    fn rebuild_krea_html() {
        let (_, _, _, out_dir) = resolve();
        let manifest = out_dir.join("krea_manifest.json");
        let report_path = out_dir.join("report.krea_compare.json");
        if !manifest.exists() || !report_path.exists() {
            println!("[krea] нет артефактов — пересборка пропущена");
            return;
        }
        let raw = std::fs::read_to_string(&manifest).expect("манифест");
        let sources: Vec<KreaSource> = serde_json::from_str(&raw).expect("разбор манифеста");
        let raw = std::fs::read_to_string(&report_path).expect("отчёт");
        let report: KreaCompareReport = serde_json::from_str(&raw).expect("разбор отчёта");
        let html = build_krea_html(&out_dir, &sources, &report).expect("html");
        println!("[krea] HTML пересобран: {}", html.display());
    }
}

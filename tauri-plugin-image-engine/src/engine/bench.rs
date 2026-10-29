//! Бенч-харнесс времени и памяти для бандлов image-движка
//! (PRP «Сборки Qwen-Image-2.1 Turbo» Шаг 6 + Шаг 7.2 pHash).
//!
//! Контракт:
//! - `run_bench` — один бандл: spawn → warm-up → промпты × сиды → JSON-отчёт
//!   на диск (`<out>/<bundle>/report.json` + PNG рядом);
//! - пик VRAM — NVML-опрос каждые ~2 сек в фоне (`max(total − free)`);
//!   это пик ВСЕЙ карты (другие процессы входят в цифру) — заметка в отчёте;
//! - RAM-пик v1 не замеряется (оценка по размеру весов — замечание в отчёте);
//! - warm-up прогон помечен и в средние НЕ попадает (6.4: prefix-cache
//!   греется первым прогоном).
//!
//! pHash-хелперы и расширения протокола (edit/hires, PRP Шаг 8) — в
//! `bench_ext.rs`; интеграционные запуски тоже там (реальный spawn
//! sd-server — только по явной просьбе юзера).
//!
//! Запуск — ТОЛЬКО через `#[ignore]`-интеграцию по явной просьбе юзера
//! (реальный spawn sd-server, минуты на каждую картинку).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::engine::image_engine::ImageEngine;
use crate::engine::models_catalog::{bundle_disk_bytes, ImageBundleEntry};

pub(crate) const MIB: f64 = 1024.0 * 1024.0;
/// Интервал NVML-опроса при генерации (6.3).
const VRAM_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

/// Одна генерация замера.
#[derive(Serialize, Clone, Debug)]
pub struct BenchRun {
    pub prompt_idx: usize,
    pub seed: i64,
    pub image_path: String,
    pub gen_time_s: f64,
}

/// Отчёт одного бандла (пишется в report.json).
#[derive(Serialize, Clone, Debug)]
pub struct BenchReport {
    pub bundle: String,
    pub model: String,
    pub resolution: String,
    pub steps: u32,
    pub cfg: f32,
    pub custom_sigmas: bool,
    pub disk_gb: f64,
    pub load_time_s: f64,
    pub vram_total_mb: f64,
    pub vram_free_before_mb: f64,
    /// Пик занятой VRAM карты во время основных прогонов (NVML ~2 сек).
    pub vram_peak_used_mb: Option<f64>,
    pub vram_free_after_mb: f64,
    pub warmup: Vec<BenchRun>,
    pub runs: Vec<BenchRun>,
    pub failed: Vec<String>,
    /// Честные оговорки протокола замера.
    pub notes: Vec<String>,
}

impl BenchReport {
    /// Среднее время генерации по состоявшимся прогонам (для таблицы).
    pub fn mean_gen_time_s(&self) -> Option<f64> {
        if self.runs.is_empty() {
            return None;
        }
        Some(self.runs.iter().map(|r| r.gen_time_s).sum::<f64>() / self.runs.len() as f64)
    }
}

/// Имя PNG одного прогона: warm-up — `warmup_S<seed>`, замеры — `P<idx>_S<seed>`.
fn run_file_name(warmup: bool, prompt_idx: usize, seed: i64) -> String {
    if warmup {
        format!("warmup_S{}.png", seed)
    } else {
        format!("P{}_S{}.png", prompt_idx, seed)
    }
}

/// Фоновый NVML-сэмплер: каждые ~2 сек пишет `max(total − free)` в ячейку.
/// Nvml создаётся ВНУТРИ потока — кросс-поточная передача не требуется.
pub(crate) struct VramSampler {
    stop: Arc<AtomicBool>,
    peak_used: Arc<Mutex<f64>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl VramSampler {
    pub(crate) fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak_used = Arc::new(Mutex::new(0.0));
        let s_stop = stop.clone();
        let s_peak = peak_used.clone();
        let handle = std::thread::spawn(move || {
            let Ok(nvml) = nvml_wrapper::Nvml::init() else { return };
            let Ok(dev) = nvml.device_by_index(0) else { return };
            while !s_stop.load(Ordering::SeqCst) {
                if let Ok(mem) = dev.memory_info() {
                    let used = (mem.total as f64 - mem.free as f64) / MIB;
                    let mut p = s_peak.lock().unwrap();
                    if used > *p {
                        *p = used;
                    }
                }
                std::thread::sleep(VRAM_SAMPLE_INTERVAL);
            }
        });
        Self { stop, peak_used, handle: Some(handle) }
    }

    pub(crate) fn peak(&self) -> Option<f64> {
        let p = *self.peak_used.lock().unwrap();
        if p > 0.0 { Some(p) } else { None }
    }
}

impl Drop for VramSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

pub(crate) fn nvml_now() -> (f64, f64) {
    match nvml_wrapper::Nvml::init() {
        Ok(nvml) => match nvml.device_by_index(0).and_then(|d| d.memory_info()) {
            Ok(mem) => (mem.free as f64 / MIB, mem.total as f64 / MIB),
            Err(_) => (0.0, 0.0),
        },
        Err(_) => (0.0, 0.0),
    }
}

/// Одна генерация бенча: файл, метрики, честная ошибка при провале.
#[allow(clippy::too_many_arguments)]
fn bench_generate<L>(
    engine: &ImageEngine,
    entry: &ImageBundleEntry,
    run_dir: &Path,
    warmup: bool,
    prompt_idx: usize,
    prompt: &str,
    seed: i64,
    log_cb: L,
) -> Result<BenchRun, String>
where
    L: Fn(String),
{
    let out_path = run_dir.join(run_file_name(warmup, prompt_idx, seed));
    let res = engine.generate(
        prompt,
        entry.preset.width,
        entry.preset.height,
        entry.preset.steps,
        entry.preset.cfg_scale,
        seed,
        &[],
        &out_path,
        None,
        log_cb,
        entry.preset.custom_sigmas.as_deref(),
        entry.preset.scheduler.as_deref(),
        None, // hires: t2i-бенч однофазный
        None, // img2img: t2i-бенч не использует
        &[],  // loras: бенч — метрики без LoRA-стека
        None, // ref_image_args: t2i-бенч без референсов
    )?;
    Ok(BenchRun {
        prompt_idx,
        seed: res.seed,
        image_path: res.path,
        gen_time_s: res.time_sec,
    })
}

/// Прогон одного бандла: spawn → warm-up → промпты × сиды → отчёт на диск.
#[allow(clippy::too_many_arguments)]
pub fn run_bench<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &ImageBundleEntry,
    prompts: &[String],
    seeds: &[i64],
    out_dir: &Path,
    variant_pref: Option<&str>,
    log_cb: L,
) -> Result<BenchReport, String>
where
    L: Fn(String) + Clone,
{
    let dir = out_dir.join(&entry.name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Не создать {}: {}", dir.display(), e))?;

    let (free_before, total) = nvml_now();

    let load_start = Instant::now();
    // Замерный режим preflight: bench меряет ФИЗИЧЕСКУЮ возможность (влезает ли
    // auto-fit в дефицит VRAM), а не оценку калькулятора. Продуктовый путь
    // (`ImageEngine::new`) по-прежнему отказывает при нехватке.
    let engine = ImageEngine::new_with_policy(
        engine_dir,
        bundle_dir,
        entry,
        variant_pref,
        crate::engine::image_engine::PreflightPolicy::Measure,
        log_cb.clone(),
    )?;
    let load_time_s = load_start.elapsed().as_secs_f64();

    let mut failed = Vec::new();

    // 6.4: первый прогон греет prefix-cache — в средние не входит.
    let mut warmup = Vec::new();
    if let (Some(p0), Some(&s0)) = (prompts.first(), seeds.first()) {
        match bench_generate(&engine, entry, &dir, true, 0, p0, s0, log_cb.clone()) {
            Ok(run) => warmup.push(run),
            Err(e) => {
                failed.push(format!("warmup ({}): {}", entry.name, e));
                log_cb(format!("⚠️ Warm-up не удался ({}): {} — замеры продолжаются.", entry.name, e));
            }
        }
    }

    let sampler = VramSampler::start();
    let mut runs = Vec::new();
    'outer: for (idx, prompt) in prompts.iter().enumerate() {
        for &seed in seeds {
            match bench_generate(&engine, entry, &dir, false, idx, prompt, seed, log_cb.clone()) {
                Ok(run) => runs.push(run),
                Err(e) => {
                    failed.push(format!("P{}_S{} ({}): {}", idx, seed, entry.name, e));
                    if e == "CANCELLED" || e.contains("Таймаут генерации") {
                        // Движок, скорее всего, не отвечает — продолжать нет смысла.
                        break 'outer;
                    }
                }
            }
        }
    }
    let peak = sampler.peak();
    drop(sampler);

    let (free_after, _) = nvml_now();

    let diffusion = entry
        .files
        .iter()
        .find(|f| f.role == "diffusion")
        .map(|f| f.filename.clone())
        .unwrap_or_default();
    let report = BenchReport {
        bundle: entry.name.clone(),
        model: diffusion,
        resolution: format!("{}x{}", entry.preset.width, entry.preset.height),
        steps: entry.preset.steps,
        cfg: entry.preset.cfg_scale,
        custom_sigmas: entry.preset.custom_sigmas.is_some(),
        disk_gb: bundle_disk_bytes(entry) as f64 / MIB / 1024.0,
        load_time_s,
        vram_total_mb: total,
        vram_free_before_mb: free_before,
        vram_peak_used_mb: peak,
        vram_free_after_mb: free_after,
        warmup,
        runs,
        failed,
        notes: vec![
            "Пик VRAM — занятая память ВСЕЙ карты (NVML ~2 сек), чужие процессы включены.".to_string(),
            "RAM-пик не замерялся: оценка по размеру весов бандла.".to_string(),
            "Warm-up прогон (первый по времени) в средние не входит.".to_string(),
        ],
    };
    let path = dir.join("report.json");
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Не записать {}: {}", path.display(), e))?;
    log_cb(format!("📄 Отчёт: {}", path.display()));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_match_protocol() {
        assert_eq!(run_file_name(true, 0, 7), "warmup_S7.png");
        assert_eq!(run_file_name(false, 3, 42), "P3_S42.png");
    }
}

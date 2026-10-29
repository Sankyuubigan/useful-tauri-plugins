//! Расширения bench-протокола (PRP «Сборки Qwen-Image-2.1 Turbo», Шаг 8):
//! pHash-хелперы, edit- и hires-замеры по трём turbo-бандлам.
//!
//! Контекст 19.10.2026 (фиксируется ДО прогона — правила гейтов):
//! - base Q6_K удалён из каталога вердиктом юзера («ничем не лучше, а по
//!   скорости ужасен») — новым набором редактирование ОБЯЗАТЕЛЬНО
//!   (mmproj возвращён в каждый turbo), бенчи идут без базы;
//! - edit-замер повторяет протокол t2i (warm-up + 8×2) с ОДНИМ референсом
//!   из артефактов t2i-прогона; canvas по пропорциям ref
//!   (`ref_size::canvas_for_ref`, как в `edit_image_files`);
//! - hires-замер: native hires-fix sd.cpp («Lanczos», ×2 → 2048², denoise
//!   0.4, steps = пресетные); gen_time = полный двухфазный цикл;
//! - pHash для edit/hires НЕ считается (в папке миксуются имена разных
//!   фаз) — вердикт остаётся у глазного A/B юзера (как и для t2i).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use crate::engine::bench::{nvml_now, BenchReport, BenchRun, MIB, VramSampler};
use crate::engine::bench_prompts::{
    env_dir, hires_payload, load_edit_prompts, load_prompts, HIRES_DENOISE, HIRES_SCALE, HIRES_UPSCALER,
};
use crate::engine::image_engine::ImageEngine;
use crate::engine::models_catalog::{bundle_disk_bytes, ImageBundleEntry};
use crate::engine::ref_size;

/// Строка pHash-сравнения наборов.
#[derive(Serialize, Clone, Debug)]
pub struct PhashRow {
    pub image: String,
    pub distance: Option<u32>,
}

/// 7.2: pHash-расстояния PNG candidate-набора против другого набора.
/// Пары соединяются ПО ИМЕНИ ФАЙЛА; нет пары → distance=None.
/// Индикация «картинки по-разному меняются» — НЕ метрика качества.
pub fn phash_diff_vs(candidate_dir: &Path, base_dir: &Path) -> Result<Vec<PhashRow>, String> {
    let hasher = image_hasher::HasherConfig::new().to_hasher();
    let base = hash_dir(base_dir, &hasher)?;
    let mut rows = Vec::new();
    for (name, cand) in hash_dir(candidate_dir, &hasher)? {
        let distance = base
            .iter()
            .find(|(bn, _)| bn == &name)
            .map(|(_, bh)| cand.dist(bh));
        rows.push(PhashRow { image: name, distance });
    }
    Ok(rows)
}

/// Среднее расстояние по парам, у которых distance есть.
pub fn phash_mean(rows: &[PhashRow]) -> Option<f64> {
    let n = rows.iter().filter(|r| r.distance.is_some()).count();
    if n == 0 {
        return None;
    }
    let sum: f64 = rows.iter().filter_map(|r| r.distance).map(|d| d as f64).sum();
    Some(sum / n as f64)
}

/// Хэши всех PNG папки, отсортированные по имени.
fn hash_dir(dir: &Path, hasher: &image_hasher::Hasher) -> Result<Vec<(String, image_hasher::ImageHash)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("Не открыть {}: {}", dir.display(), e))?;
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        let is_png = path
            .extension()
            .and_then(|x| x.to_str())
            .map(|x| x.eq_ignore_ascii_case("png"))
            .unwrap_or(false);
        if !is_png {
            continue;
        }
        let img = image::open(&path).map_err(|e| format!("Не открыть {}: {}", path.display(), e))?;
        let short = path
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or_default()
            .to_string();
        out.push((short, hasher.hash_image(&img)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Имя PNG расширенного прогона: warm-up — `warmup<E|H>_S<seed>`,
/// замеры — `E<idx>_S<seed>` / `H<idx>_S<seed>`.
fn ext_file_name(warmup: bool, kind: &str, prompt_idx: usize, seed: i64) -> String {
    if warmup {
        format!("warmup{}_S{}.png", kind, seed)
    } else {
        format!("{}{}_S{}.png", kind, prompt_idx, seed)
    }
}

/// Референс edit-прогона: файл на диске → (base64, width, height).
pub(crate) fn read_ref_b64(path: &Path) -> Result<(String, u32, u32), String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("Не найден референс {}: {}", path.display(), e))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| format!("Ошибка чтения {}: {}", path.display(), e))?;
    let dims = ref_size::dimensions_from_path(path)
        .ok_or_else(|| format!("Не определить размеры референса {}", path.display()))?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    Ok((b64, dims.0, dims.1))
}

/// Одна генерация расширенного бенча (edit или hires).
#[allow(clippy::too_many_arguments)]
fn ext_generate<L>(
    engine: &ImageEngine,
    entry: &ImageBundleEntry,
    run_dir: &Path,
    warmup: bool,
    kind: &str,
    prompt_idx: usize,
    prompt: &str,
    seed: i64,
    width: u32,
    height: u32,
    refs_b64: &[String],
    hires: Option<&serde_json::Value>,
    log_cb: L,
) -> Result<BenchRun, String>
where
    L: Fn(String),
{
    let out_path = run_dir.join(ext_file_name(warmup, kind, prompt_idx, seed));
    let res = engine.generate(
        prompt,
        width,
        height,
        entry.preset.steps,
        entry.preset.cfg_scale,
        seed,
        refs_b64,
        &out_path,
        None,
        log_cb,
        entry.preset.custom_sigmas.as_deref(),
        entry.preset.scheduler.as_deref(),
        hires,
        None,
        &[], // loras: bench_ext не использует LoRA-стек
    )?;
    Ok(BenchRun {
        prompt_idx,
        seed: res.seed,
        image_path: res.path,
        gen_time_s: res.time_sec,
    })
}

/// Сборка отчёта с общими честными оговорками протокола.
#[allow(clippy::too_many_arguments)]
fn new_report(
    entry: &ImageBundleEntry,
    resolution: String,
    load_time_s: f64,
    total_mb: f64,
    free_before_mb: f64,
    peak: Option<f64>,
    free_after_mb: f64,
    warmup: Vec<BenchRun>,
    runs: Vec<BenchRun>,
    failed: Vec<String>,
    extra_notes: Vec<String>,
) -> BenchReport {
    let diffusion = entry
        .files
        .iter()
        .find(|f| f.role == "diffusion")
        .map(|f| f.filename.clone())
        .unwrap_or_default();
    let mut notes = vec![
        "Пик VRAM — занятая память ВСЕЙ карты (NVML ~2 сек), чужие процессы включены.".to_string(),
        "RAM-пик не замерялся: оценка по размеру весов бандла.".to_string(),
        "Warm-up прогон (первый по времени) в средние не входит.".to_string(),
    ];
    notes.extend(extra_notes);
    BenchReport {
        bundle: entry.name.clone(),
        model: diffusion,
        resolution,
        steps: entry.preset.steps,
        cfg: entry.preset.cfg_scale,
        custom_sigmas: entry.preset.custom_sigmas.is_some(),
        disk_gb: bundle_disk_bytes(entry) as f64 / MIB / 1024.0,
        load_time_s,
        vram_total_mb: total_mb,
        vram_free_before_mb: free_before_mb,
        vram_peak_used_mb: peak,
        vram_free_after_mb: free_after_mb,
        warmup,
        runs,
        failed,
        notes,
    }
}

/// Общий скелет расширенного прогона: spawn → warm-up → промпты × сиды
/// → отчёт `report.edits.json`/`report.hires.json` на диск.
#[allow(clippy::too_many_arguments)]
fn run_ext_bench<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &ImageBundleEntry,
    prompts: &[String],
    seeds: &[i64],
    out_dir: &Path,
    variant_pref: Option<&str>,
    kind: &str,
    report_file: &str,
    refs_b64: &[String],
    hires: Option<&serde_json::Value>,
    canvas: (u32, u32),
    resolution: String,
    extra_notes: Vec<String>,
    log_cb: L,
) -> Result<BenchReport, String>
where
    L: Fn(String) + Clone,
{
    let dir = out_dir.join(&entry.name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Не создать {}: {}", dir.display(), e))?;

    let (free_before, total) = nvml_now();

    let load_start = Instant::now();
    // Замерный режим preflight (см. PreflightPolicy): bench меряет ФИЗИЧЕЧЕСКУЮ
    // возможность, а не оценку калькулятора (в дефиците VRAM оценка блокирует
    // запуск, и замер невозможен).
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

    let mut warmup = Vec::new();
    if let (Some(p0), Some(&s0)) = (prompts.first(), seeds.first()) {
        match ext_generate(&engine, entry, &dir, true, kind, 0, p0, s0, canvas.0, canvas.1, refs_b64, hires, log_cb.clone()) {
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
            match ext_generate(&engine, entry, &dir, false, kind, idx, prompt, seed, canvas.0, canvas.1, refs_b64, hires, log_cb.clone()) {
                Ok(run) => runs.push(run),
                Err(e) => {
                    failed.push(format!("{}{}_S{} ({}): {}", kind, idx, seed, entry.name, e));
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

    let report = new_report(entry, resolution, load_time_s, total, free_before, peak, free_after, warmup, runs, failed, extra_notes);
    let path = dir.join(report_file);
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Не записать {}: {}", path.display(), e))?;
    log_cb(format!("📄 Отчёт: {}", path.display()));
    Ok(report)
}

/// Edit-замер одного бандла: протокол t2i (warm-up + 8×2) с ОДНИМ
/// референсом (условия агента: ref → base64 → ref_images[]; canvas по
/// пропорциям ref). Отчёт `<bundle>/report.edit.json`.
#[allow(clippy::too_many_arguments)]
pub fn run_edit_bench<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &ImageBundleEntry,
    prompts: &[String],
    seeds: &[i64],
    out_dir: &Path,
    ref_path: &Path,
    variant_pref: Option<&str>,
    log_cb: L,
) -> Result<BenchReport, String>
where
    L: Fn(String) + Clone,
{
    let (ref_b64, ref_w, ref_h) = read_ref_b64(ref_path)?;
    let (canvas_w, canvas_h) = ref_size::canvas_for_ref((entry.preset.width, entry.preset.height), (ref_w, ref_h));
    if (canvas_w, canvas_h) != (entry.preset.width, entry.preset.height) {
        log_cb(format!(
            "ℹ️ Canvas по пропорциям <image 1>: {}x{} → {}x{}.",
            entry.preset.width, entry.preset.height, canvas_w, canvas_h
        ));
    }
    let refs = vec![ref_b64];
    run_ext_bench(
        engine_dir,
        bundle_dir,
        entry,
        prompts,
        seeds,
        out_dir,
        variant_pref,
        "E",
        "report.edit.json",
        &refs,
        None,
        (canvas_w, canvas_h),
        format!("{}x{}", canvas_w, canvas_h),
        vec![
            format!(
                "Edit-протокол t2i (warm-up + {}×{}), один референс: {}.",
                prompts.len(),
                seeds.len(),
                ref_path.display()
            ),
            "Edit-выходы НЕ метрикуются pHash (в папке миксуются имена фаз) — вердикт у глазного A/B юзера.".to_string(),
        ],
        log_cb,
    )
}

/// Hires-замер одного бандла: native hires-fix sd.cpp — Джоб с Ланцош-
/// апскейлом ×2 (2048×2048 из 1024×1024) и denoise-проходом.
/// `gen_time` = полный двухфазный цикл. Отчёт `<bundle>/report.hires.json`.
pub fn run_hires_bench<L>(
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
    let hires = hires_payload(entry.preset.steps);
    let tw = (entry.preset.width as f64 * HIRES_SCALE) as u32;
    let th = (entry.preset.height as f64 * HIRES_SCALE) as u32;
    run_ext_bench(
        engine_dir,
        bundle_dir,
        entry,
        prompts,
        seeds,
        out_dir,
        variant_pref,
        "H",
        "report.hires.json",
        &[],
        Some(&hires),
        (entry.preset.width, entry.preset.height),
        format!("{}x{} → {}x{}", entry.preset.width, entry.preset.height, tw, th),
        vec![
            format!(
                "Hires-fix: второй проход \"{}\", scale {}, denoise {:.2}, steps = {} (пресетные).",
                HIRES_UPSCALER, HIRES_SCALE, HIRES_DENOISE, entry.preset.steps
            ),
            "gen_time = полный двухфазный цикл (база + hires-проход).".to_string(),
            "Пик VRAM покрывает и hires-фазу — данные для тестов 8 ГБ.".to_string(),
        ],
        log_cb,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_file_names_match_protocol() {
        assert_eq!(ext_file_name(true, "E", 0, 7), "warmupE_S7.png");
        assert_eq!(ext_file_name(false, "E", 3, 42), "E3_S42.png");
        assert_eq!(ext_file_name(false, "H", 5, 1337), "H5_S1337.png");
    }

    #[test]
    fn mean_skips_missing_pairs() {
        let rows = vec![
            PhashRow { image: "a".into(), distance: Some(10) },
            PhashRow { image: "b".into(), distance: None },
        ];
        assert_eq!(phash_mean(&rows), Some(10.0));
        assert_eq!(phash_mean(&[]), None);
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use crate::engine::bench::run_bench;

    fn resolve_common() -> (PathBuf, PathBuf, PathBuf) {
        let engine_dir = env_dir("IMG_BENCH_ENGINE_DIR")
            .unwrap_or_else(|| crate::engine::config::engine_dir_early());
        let bundle_dir = env_dir("IMG_BENCH_BUNDLE_DIR")
            .or_else(|| {
                let cfg = crate::engine::config::load_image_config_early();
                cfg.image_bundle_dir.filter(|s| !s.trim().is_empty()).map(PathBuf::from)
            })
            .expect(
                "IMG_BENCH_BUNDLE_DIR не задан и в app_config.json нет image_bundle_dir — \
                 скачайте бандл в приложении либо задайте переменную",
            );
        let out_root = env_dir("IMG_BENCH_OUT")
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../king_orch_3/test/bench"));
        std::fs::create_dir_all(&out_root).expect("создать test/bench");
        (engine_dir, bundle_dir, out_root)
    }

    #[test]
    #[ignore = "spawn sd-server + edit/hires 8×2 на КАЖДЫЙ бандл — запуск только по явной просьбе юзера (test_bench.bat)"]
    fn bench_modes_integration() {
        // Серверные строки sd-server (pipe_log → log::info!) — в bench_run.log.
        let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
        let (engine_dir, bundle_dir, out_root) = resolve_common();

        // IMG_BENCH_MODES — csv «edit,hires[,t2i]»; дефолт: edit+hires
        // (t2i замерен 19.10.2026, его артефакты переиспользуются как ref).
        let modes: Vec<String> = std::env::var("IMG_BENCH_MODES")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.split(',').map(|m| m.trim().to_ascii_lowercase()).collect())
            .unwrap_or_else(|| vec!["edit".to_string(), "hires".to_string()]);
        let only = std::env::var("IMG_BENCH_ONLY").ok().filter(|s| !s.trim().is_empty());
        println!("modes: {:?}; фильтр бандла (IMG_BENCH_ONLY): {:?}; сиды: [42, 1337]", modes, only);

        let (prompts, src) = load_prompts();
        assert!(prompts.len() >= 8, "hires-бенч требует >= 8 промптов (источник: {})", src);
        let (edit_prompts, esrc) = load_edit_prompts();
        assert!(edit_prompts.len() >= 8, "edit-бенч требует >= 8 промптов (источник: {})", esrc);
        println!("Промптов t2i: {} ({}), edit: {} ({})", prompts.len(), src, edit_prompts.len(), esrc);
        let seeds: Vec<i64> = vec![42, 1337];

        let log = |m: String| {
            println!("[bench] {}", m);
            log::info!("[bench] {}", m);
        };

        let catalog = crate::engine::models_catalog::load_image_catalog();
        println!("Бандлов в каталоге: {}", catalog.len());

        let mut ran_any = false;
        for entry in &catalog {
            if let Some(o) = &only {
                if o != &entry.name {
                    continue;
                }
            }
            let diff_exists = entry
                .files
                .iter()
                .find(|f| f.role == "diffusion")
                .map(|f| bundle_dir.join(&f.filename))
                .is_some_and(|p| p.exists());
            if !diff_exists {
                println!("⏭ Пропуск {}: diffusion не найден в {}", entry.name, bundle_dir.display());
                continue;
            }

            if modes.iter().any(|m| m == "edit") {
                let default_ref = out_root.join(&entry.name).join("P0_S42.png");
                let ref_path = env_dir("IMG_BENCH_EDIT_REF").unwrap_or(default_ref);
                if !ref_path.exists() {
                    println!(
                        "⏭ Edit {} пропущен: нет референса {} (нужен артефакт t2i-прогона или IMG_BENCH_EDIT_REF)",
                        entry.name,
                        ref_path.display()
                    );
                } else if let Err(e) =
                    run_edit_bench(&engine_dir, &bundle_dir, entry, &edit_prompts, &seeds, &out_root, &ref_path, None, log.clone())
                {
                    println!("⚠️ Edit {} упал целиком: {}", entry.name, e);
                } else {
                    ran_any = true;
                }
            }

            if modes.iter().any(|m| m == "hires") {
                if let Err(e) = run_hires_bench(&engine_dir, &bundle_dir, entry, &prompts, &seeds, &out_root, None, log.clone()) {
                    println!("⚠️ Hires {} упал целиком: {}", entry.name, e);
                } else {
                    ran_any = true;
                }
            }

            if modes.iter().any(|m| m == "t2i") {
                if let Err(e) = run_bench(&engine_dir, &bundle_dir, entry, &prompts, &seeds, &out_root, None, log.clone()) {
                    println!("⚠️ T2I {} упал целиком: {}", entry.name, e);
                } else {
                    ran_any = true;
                }
            }
        }
        assert!(
            ran_any,
            "ни одна нога не отработала — проверьте IMG_BENCH_MODES/IMG_BENCH_ONLY и наличие весов/артефактов"
        );
        println!("[bench] Это всё. Modes {:?} — завершены.", modes);
    }
}

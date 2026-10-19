//! A/B-замер denoise-степени апскейла (PRP «Сборки Qwen-Image-2.1 Turbo»,
//! Этап B, 19.10.2026). img2img через native `/sdcpp/v1/img_gen`:
//! `init_image` (base64) + `strength`; у Qwen несовместимость с `ref_images`
//! отсутствует (ограничение только у MiniMax-H3), но режим апскейла refs не
//! задаёт — композиция приходит из init.
//!
//! Протокол ГЛАЗНОГО A/B (до фиксации рабочего strength):
//! один spawn на бандл, источники — артефакты t2i (P{idx}_S{seed}.png),
//! цель = đối ×2 (2048²), три степени денойза 0.25/0.35/0.45. Warm-up нет:
//! это сет на просмотр, а не метрический bench (времена каждого job'а —
//! в log_cb и отчёте). Полный метрический протокол идёт ПОСЛЕ вердикта юзера.

use std::path::Path;

use serde::Serialize;

use crate::engine::bench_ext::read_ref_b64;
use crate::engine::image_engine::{upscale_target_dims, Img2Img, ImageEngine};
use crate::engine::models_catalog::ImageBundleEntry;

/// Грид денойза глазного A/B апскейла.
pub const UPSCALE_AB_STRENGTHS: &[f32] = &[0.25, 0.35, 0.45];

/// Тег denoise в имени файла: 0.25 → `d025` (проценты, 3 цифры).
pub fn denoise_tag(strength: f32) -> String {
    format!("d{:03}", (strength * 100.0).round() as u32)
}

/// Имя выходного файла апскейла: `U{idx}_S{seed}_{d025}.png`.
pub fn upscale_file_name(prompt_idx: usize, seed: i64, strength: f32) -> String {
    format!("U{}_S{}_{}.png", prompt_idx, seed, denoise_tag(strength))
}

/// Один job A/B-прогона.
#[derive(Serialize, Clone, Debug)]
pub struct UpscaleRun {
    pub prompt_idx: usize,
    pub seed: i64,
    pub denoise: f32,
    pub image_path: String,
    pub gen_time_s: f64,
}

/// Отчёт A/B-прогона апскейла одного бандла → `report.upscale_ab.json`.
#[derive(Serialize, Clone, Debug)]
pub struct UpscaleAbReport {
    pub bundle: String,
    pub sources: Vec<String>,
    pub strengths: Vec<f32>,
    pub runs: Vec<UpscaleRun>,
    pub failed: Vec<String>,
    pub notes: Vec<String>,
}

/// A/B денойза апскейла ×2 для одного бандла: spawn ОДИН раз, далее джобы
/// по (источник × strength). Промпт = исходный t2i-промпт источника.
#[allow(clippy::too_many_arguments)]
pub fn run_upscale_ab<L>(
    engine_dir: &Path,
    bundle_dir: &Path,
    entry: &ImageBundleEntry,
    out_dir: &Path,
    sources: &[(usize, i64)],
    strengths: &[f32],
    t2i_prompts: &[String],
    log_cb: L,
) -> Result<UpscaleAbReport, String>
where
    L: Fn(String) + Clone,
{
    let dir = out_dir.join(&entry.name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Не создать {}: {}", dir.display(), e))?;

    // Источники: только существующие P{idx}_S{seed}.png t2i-артефакты.
    let mut jobs = Vec::new();
    let mut used_sources = Vec::new();
    for &(idx, seed) in sources {
        let src = dir.join(format!("P{}_S{}.png", idx, seed));
        if !src.exists() {
            log_cb(format!("⏭ Источник P{}_S{}.png отсутствует — пропущен.", idx, seed));
            continue;
        }
        jobs.push((idx, seed, src));
        used_sources.push(format!("P{}_S{}.png", idx, seed));
    }
    if jobs.is_empty() {
        return Err(format!(
            "{}: нет источников апскейла (нужны P*_S*.png из t2i-прогона в {})",
            entry.name,
            dir.display()
        ));
    }

    let load_start = std::time::Instant::now();
    let engine = ImageEngine::new(engine_dir, bundle_dir, entry, None, log_cb.clone())?;
    log_cb(format!("⏱ Загрузка бандла: {:.1} сек.", load_start.elapsed().as_secs_f64()));

    let mut runs = Vec::new();
    let mut failed = Vec::new();
    let log = log_cb.clone();
    for &(idx, seed, ref src) in &jobs {
        let prompt = t2i_prompts
            .get(idx)
            .cloned()
            .unwrap_or_else(|| "Preserve the scene, increase resolution and fine detail".to_string());
        let dims = crate::engine::ref_size::dimensions_from_path(src)
            .ok_or_else(|| format!("{}: не определить размеры {}", entry.name, src.display()))?;
        let (w, h) = upscale_target_dims(dims.0, dims.1);
        for &st in strengths {
            let out_path = dir.join(upscale_file_name(idx, seed, st));
            let (init_b64, _, _) = read_ref_b64(src).map_err(|e| format!("{}: {}", entry.name, e))?;
            log_cb(format!(
                "🖼 Апскейл U{}_S{}_{} — источник {} ({}x{} → {}x{}).",
                idx,
                seed,
                denoise_tag(st),
                src.display(),
                dims.0,
                dims.1,
                w,
                h
            ));
            match engine.generate(
                &prompt,
                w,
                h,
                entry.preset.steps,
                entry.preset.cfg_scale,
                seed,
                &[],
                &out_path,
                None,
                &log,
                entry.preset.custom_sigmas.as_deref(),
                entry.preset.scheduler.as_deref(),
                None,
                Some(&Img2Img { init_b64, strength: st }),
            ) {
                Ok(res) => runs.push(UpscaleRun {
                    prompt_idx: idx,
                    seed,
                    denoise: st,
                    image_path: res.path,
                    gen_time_s: res.time_sec,
                }),
                Err(e) => {
                    failed.push(format!("U{}_S{}_{}: {}", idx, seed, denoise_tag(st), e));
                    if e == "CANCELLED" || e.contains("Таймаут генерации") {
                        log_cb(format!("⚠️ {} замолчал — прерываю A/B денойза.", entry.name));
                        break;
                    }
                }
            }
        }
    }

    let report = UpscaleAbReport {
        bundle: entry.name.clone(),
        sources: used_sources,
        strengths: strengths.to_vec(),
        runs,
        failed,
        notes: vec![
            "Протокол ГЛАЗНОГО A/B: warm-up нет — сет на просмотр (times — честные gen_time каждого джоба).".to_string(),
            "img2img native API: init_image (base64) + strength; canvas = источник ×2, потолок 2048 по стороне.".to_string(),
            "custom_sigmas пресета применён — шаги обрезаются по внутриигровому денойзу сервера.".to_string(),
            "VAE encode 2048² идёт тайлами (--vae-tiling в спавне) — отказ «encode_first_stage failed» невозможен по конфигурации.".to_string(),
        ],
    };
    let path = dir.join("report.upscale_ab.json");
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Не записать {}: {}", path.display(), e))?;
    log_cb(format!("📄 Отчёт: {} (ок {} / упало {})", path.display(), report.runs.len(), report.failed.len()));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denoise_tags_and_names_follow_convention() {
        assert_eq!(denoise_tag(0.25), "d025");
        assert_eq!(denoise_tag(0.35), "d035");
        assert_eq!(denoise_tag(0.45), "d045");
        assert_eq!(upscale_file_name(2, 1337, 0.35), "U2_S1337_d035.png");
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use crate::engine::bench_prompts::{env_dir, load_prompts};
    use crate::engine::ab_view::AB_DEFAULT_BUNDLES;

    fn resolve() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let engine_dir = env_dir("IMG_BENCH_ENGINE_DIR")
            .unwrap_or_else(|| crate::engine::config::engine_dir_early());
        let bundle_dir = env_dir("IMG_BENCH_BUNDLE_DIR")
            .or_else(|| {
                let cfg = crate::engine::config::load_image_config_early();
                cfg.image_bundle_dir.filter(|s| !s.trim().is_empty()).map(std::path::PathBuf::from)
            })
            .expect("IMG_BENCH_BUNDLE_DIR не задан и в app_config.json нет image_bundle_dir");
        let out_root = env_dir("IMG_BENCH_OUT")
            .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../king_orch_3/test/bench"));
        std::fs::create_dir_all(&out_root).expect("создать test/bench");
        (engine_dir, bundle_dir, out_root)
    }

    #[test]
    #[ignore = "img2img A/B денойза: 3 источника × 3 strengths × 2 бандла (~30 мин, sd-server) — запуск по явной просьбе юзера"]
    fn ab_upscale_strengths() {
        let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
        let (engine_dir, bundle_dir, out_root) = resolve();
        let bundles: Vec<String> = std::env::var("AB_BUNDLES")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.split(',').map(|b| b.trim().to_string()).collect())
            .unwrap_or_else(|| AB_DEFAULT_BUNDLES.iter().map(|s| s.to_string()).collect());

        let (t2i, tsrc) = load_prompts();
        assert!(t2i.len() >= 8, "источников-промптов < 8 (источник: {})", tsrc);

        let log = |m: String| {
            println!("[ab] {}", m);
            log::info!("[ab] {}", m);
        };
        let sources: Vec<(usize, i64)> = vec![(0, 42), (1, 42), (2, 42)];

        let mut ran_any = false;
        for name in &bundles {
            let Some(entry) = crate::engine::models_catalog::bundle_by_name(name) else {
                println!("⏭ Бандл {} не в каталоге", name);
                continue;
            };
            let entry = &entry;
            let diff_found = entry
                .files
                .iter()
                .find(|f| f.role == "diffusion")
                .map(|f| bundle_dir.join(&f.filename))
                .is_some_and(|p| p.exists());
            if !diff_found {
                println!("⏭ Пропуск {}: diffusion не найден в {}", entry.name, bundle_dir.display());
                continue;
            }
            match run_upscale_ab(&engine_dir, &bundle_dir, entry, &out_root, &sources, UPSCALE_AB_STRENGTHS, &t2i, log.clone()) {
                Ok(r) => {
                    println!("[ab] {}: ок {}, упало {}", entry.name, r.runs.len(), r.failed.len());
                    ran_any = true;
                }
                Err(e) => println!("⚠️ A/B апскейла {} целиком упал: {}", entry.name, e),
            }
        }
        assert!(ran_any, "ни один бандл не прогнался — проверьте AB_BUNDLES и наличие весов/артефактов");
    }
}

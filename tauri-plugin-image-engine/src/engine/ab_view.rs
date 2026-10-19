//! A/B-склейка глазного вердикта (PRP «Сборки Qwen-Image-2.1 Turbo»,
//! 19.10.2026). Юзер смотрит пары «до | после» В ОДНОМ месте:
//! PNG-коллажи `<bundle>/AB/` + страница `test/bench/ab.html` с текстами
//! промптов. LLM не вызывается — склеиваются готовые артефакты bench
//! (P*/E*/H*); отсутствующие пары молча пропускаются.
//!
//! Правило масштаба коллажа: если «после» крупнее «до» примерно в 2 раза
//! (hires-случай), «до» растягивается Lanczos до размера «после» — масштаб
//! совпадает, детализация сравнима; при прочих расхождениях приводится
//! «после» к размеру «до» (edit-канвас может отличаться от 1024²).

use std::path::{Path, PathBuf};

use crate::engine::bench_prompts::{env_dir, load_edit_prompts, load_prompts};
use crate::engine::bench_upscale::denoise_tag;

/// Бандлы глазного A/B по умолчанию (q6k снят вердиктом юзера — не показываем).
pub const AB_DEFAULT_BUNDLES: &[&str] =
    &["qwen-image-2.1-turbo-unsloth-q5_ks", "qwen-image-2.1-turbo-viggle-q4km"];

/// Степени денойза, показываемые в upscale-рядках (только существующие файлы).
pub const UPSCALE_TILE_DENOISES: &[f32] = &[0.25, 0.35, 0.45];

/// Описатель ОДНОГО ряда страницы: N картинок склеиваются бок-о-бок.
pub struct AbPair {
    /// Имя коллажа: `E0_S42_AB.png` / `H3_S1337_AB.png` / `U1_S42_AB.png`.
    pub out_name: String,
    /// Файлы-плитки по порядку (до, после[, варианты денойза]).
    pub files: Vec<String>,
    /// Текст промпта для подписи на странице.
    pub caption: String,
}

fn open_png(path: &Path) -> Result<image::DynamicImage, String> {
    image::open(path).map_err(|e| format!("Не открыть {}: {}", path.display(), e))
}

/// Коллаж из N PNG бок-о-бок (edit/hires — 2, upscale — 4-плитка
/// «до | d025 | d035 | d045»). Правило масштаба: все плитки приводятся
/// к размеру САМОЙ КРУПНОЙ (Lanczos) — масштаб по строке совпадает,
/// детализация сравнима.
pub fn make_ab_collage_multi(paths: &[&Path], out_path: &Path) -> Result<(), String> {
    if paths.len() < 2 {
        return Err("Коллаж требует минимум 2 картинки.".into());
    }
    let mut tiles = Vec::new();
    let mut max_w = 0u32;
    let mut max_h = 0u32;
    for p in paths {
        let img = open_png(p)?;
        max_w = max_w.max(img.width());
        max_h = max_h.max(img.height());
        tiles.push(img);
    }
    if max_w == 0 || max_h == 0 {
        return Err(format!("Пустые размеры в коллаже {:?}.", paths));
    }
    let total_w: u32 = paths.len() as u32 * max_w;
    let mut canvas = image::RgbImage::new(total_w, max_h);
    for (i, t) in tiles.iter().enumerate() {
        let tile = if (t.width(), t.height()) != (max_w, max_h) {
            t.resize_exact(max_w, max_h, image::imageops::FilterType::Lanczos3)
        } else {
            t.clone()
        };
        image::imageops::overlay(&mut canvas, &tile.to_rgb8(), (i as u32 * max_w) as i64, 0);
    }
    if let Some(p) = out_path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("Не создать {}: {}", p.display(), e))?;
    }
    canvas.save(out_path).map_err(|e| format!("Не записать {}: {}", out_path.display(), e))
}

/// Совместимость: два файла «до | после» в один коллаж.
pub fn make_ab_collage(before_path: &Path, after_path: &Path, out_path: &Path) -> Result<(), String> {
    make_ab_collage_multi(&[before_path, after_path], out_path)
}

/// Кроп-рядок для сравнения МЕЛКОЙ ТЕКСТУРЫ (зачем: коллаж 2048²-плиток в
/// браузере виден в ~12-25% ширины, разница денойза на таком масштабе не
/// читается — замер 19.10.2026). Каждый файл режется ЦЕНТРАЛЬНЫМ кропом
/// со стороной min(W,H)/2 (1024→512, 2048→1024), кропы приводятся к
/// размеру самого крупного (Lanczos) и склеиваются бок-о-бок «нативными
/// пикселями». Кнопка зума на странице открывает картинку 1:1.
pub fn make_ab_crop_row(paths: &[&Path], out_path: &Path) -> Result<(), String> {
    if paths.len() < 2 {
        return Err("Кроп-ряд требует минимум 2 картинки.".into());
    }
    let mut crops = Vec::new();
    let mut max_side = 0u32;
    for p in paths {
        let img = open_png(p)?;
        let side = img.width().min(img.height()) / 2;
        if side == 0 {
            return Err(format!("Слишком мала для кропа: {}", p.display()));
        }
        crops.push(img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side));
        max_side = max_side.max(side);
    }
    let total_w: u32 = paths.len() as u32 * max_side;
    let mut canvas = image::RgbImage::new(total_w, max_side);
    for (i, c) in crops.iter().enumerate() {
        let tile = if c.width() != max_side {
            c.resize_exact(max_side, max_side, image::imageops::FilterType::Lanczos3)
        } else {
            c.clone()
        };
        image::imageops::overlay(&mut canvas, &tile.to_rgb8(), (i as u32 * max_side) as i64, 0);
    }
    if let Some(p) = out_path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("Не создать {}: {}", p.display(), e))?;
    }
    canvas.save(out_path).map_err(|e| format!("Не записать {}: {}", out_path.display(), e))
}

/// Все ряды бандла с диска: edit (референс P0_S42 → 8×2), hires (P→H 8×2),
/// upscale (P → 3 варианта денойза × существующие сиды).
/// Отсутствующие файлы пропускаются — коллаж строится только по существующим.
pub fn collect_pairs(bundle_dir: &Path, t2i: &[String], edit: &[String]) -> Vec<AbPair> {
    let seeds = [42i64, 1337];
    let mut pairs = Vec::new();

    let ref_name = "P0_S42.png";
    for (idx, prompt) in edit.iter().take(8).enumerate() {
        for &seed in &seeds {
            let after = format!("E{}_S{}.png", idx, seed);
            if !bundle_dir.join(ref_name).exists() || !bundle_dir.join(&after).exists() {
                continue;
            }
            pairs.push(AbPair {
                out_name: format!("E{}_S{}_AB.png", idx, seed),
                files: vec![ref_name.to_string(), after],
                caption: format!("[{}] {}", idx, prompt),
            });
        }
    }
    for (idx, prompt) in t2i.iter().take(8).enumerate() {
        for &seed in &seeds {
            let before = format!("P{}_S{}.png", idx, seed);
            let after = format!("H{}_S{}.png", idx, seed);
            if !bundle_dir.join(&before).exists() || !bundle_dir.join(&after).exists() {
                continue;
            }
            pairs.push(AbPair {
                out_name: format!("H{}_S{}_AB.png", idx, seed),
                files: vec![before, after],
                caption: format!("[{}] {}", idx, prompt),
            });
        }
    }
    // Upscale-рядок: «до» + все существующие варианты денойза для (idx, seed).
    for (idx, prompt) in t2i.iter().take(8).enumerate() {
        for &seed in &seeds {
            let before = format!("P{}_S{}.png", idx, seed);
            if !bundle_dir.join(&before).exists() {
                continue;
            }
            let ups: Vec<String> = UPSCALE_TILE_DENOISES
                .iter()
                .map(|st| format!("U{}_S{}_{}.png", idx, seed, denoise_tag(*st)))
                .filter(|f| bundle_dir.join(f).exists())
                .collect();
            if ups.is_empty() {
                continue;
            }
            let mut files = vec![before];
            files.extend(ups);
            pairs.push(AbPair {
                out_name: format!("U{}_S{}_AB.png", idx, seed),
                files: files.clone(),
                caption: format!("[{}] {} (up ×2, денойз по плиткам)", idx, prompt),
            });
            // Кроп-рядок: тот же набор, разрезанный центральной областью —
            // текстура на нативных пикселях, разница d025/d035/d045 читается.
            pairs.push(AbPair {
                out_name: format!("U{}_S{}_ABcrop.png", idx, seed),
                files,
                caption: format!(
                    "[{}] {} — КРОП центра (до = 512² исходника ×2, варианты 1024² нативно; слева→справа: до, d025, d035, d045)",
                    idx, prompt
                ),
            });
        }
    }
    pairs
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Страница ab.html: на одну строку — текст промпта + коллаж(и) обоих сидов.
fn pair_rows(bundle: &str, pairs: &[AbPair]) -> String {
    let mut rows = Vec::new();
    for p in pairs {
        let src = format!("{}/AB/{}", html_escape(bundle), html_escape(&p.out_name));
        rows.push(format!(
            "<div class=\"row\"><div class=\"cap\">{}</div><img loading=\"lazy\" src=\"{}\" alt=\"{}\"></div>",
            html_escape(&p.caption),
            src,
            html_escape(&p.out_name)
        ));
    }
    rows.join("\n")
}

const AB_CSS: &str = r#"
body{background:#111;color:#ddd;font:14px/1.4 system-ui,sans-serif;margin:16px}
h1{font-size:20px}h2{font-size:16px;color:#8cf;margin:28px 0 8px}
.note{color:#999;max-width:1200px}
.row{margin-bottom:22px;background:#1a1a1a;padding:8px;border-radius:8px}
.cap{color:#ffd;font-family:monospace;margin-bottom:6px}
.row img{width:100%;height:auto;cursor:zoom-in;border-radius:4px;display:block}
#lb{position:fixed;inset:0;background:rgba(0,0,0,.95);display:none;overflow:auto}
#lb img{cursor:zoom-out;display:block;margin:0 auto}
"#;

/// Сборка артефактов глазного A/B: PNG-коллажи `<bundle>/AB/` + страница
/// `ab.html` в корне out_root. Возвращает (сколько коллажей, путь страницы).
pub fn build_ab_artifacts(
    out_root: &Path,
    bundles: &[String],
    t2i: &[String],
    edit: &[String],
) -> Result<(usize, PathBuf), String> {
    let mut sections = String::new();
    let mut made = 0usize;
    for bundle in bundles {
        let dir = out_root.join(bundle);
        if !dir.exists() {
            continue;
        }
        let pairs = collect_pairs(&dir, t2i, edit);
        if pairs.is_empty() {
            continue;
        }
        for p in &pairs {
            let out = dir.join("AB").join(&p.out_name);
            if out.exists() {
                made += 1;
                continue;
            }
            let tile_paths: Vec<PathBuf> = p.files.iter().map(|f| dir.join(f)).collect();
            let refs: Vec<&Path> = tile_paths.iter().map(|p| p.as_path()).collect();
            if out.to_string_lossy().contains("ABcrop") {
                make_ab_crop_row(&refs, &out)?;
            } else {
                make_ab_collage_multi(&refs, &out)?;
            }
            made += 1;
        }
        sections.push_str(&format!(
            "<h2>{}</h2>\n{}",
            html_escape(bundle),
            pair_rows(bundle, &pairs)
        ));
    }
    if sections.is_empty() {
        return Err("Ни для одного бандла не нашлось пар P*/E*/H* — сначала прогоните bench".into());
    }
    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Bench A/B</title><style>{}</style></head><body>\
<h1>Bench A/B — глазной вердикт</h1>\
<p class=\"note\">Edit: слева референс P0_S42.png (до), справа результат правки. \
Hires: слева t2i-кадр 1024² (до, растянут до 2048), справа hires 2048² (после) — РЕЖИМ ОТМЕНЁН вердиктом юзера 19.10.2026, показан только как архив замера. \
Upscale: слева до-кадр, дальше плитки d025 / d035 / d045 (img2img ×2). \
Кроп-рядки «КРОП центра»: фрагменты на нативных пикселях — там разница денойза читается. \
Клик по картинке — зум 1:1 с прокруткой. Подпись над рядом — промпт.<br>\
<b>Что такое d025/d035/d045:</b> сила перерисовки (denoise strength) при апскейле. \
d025 (25%) — минимальная правка: максимально близко к оригиналу, мало новых деталей; \
d035 (35%) — компромисс: утканность детализации без смены сюжета; \
d045 (45%) — модель перерисовывает почти половину: больше деталей, но сильнее отклонение от композиции и риск грязи.</p>\
{}\
<div id=\"lb\"><img id=\"lbimg\" src=\"\" alt=\"\"></div>\
<script>var lb=document.getElementById('lb'),li=document.getElementById('lbimg');\
document.addEventListener('click',function(e){{if(e.target.tagName==='IMG'){{li.src=e.target.src;lb.style.display='flex';}}else{{lb.style.display='none';}}}});</script>\
</body></html>",
        AB_CSS,
        sections
    );
    let path = out_root.join("ab.html");
    std::fs::write(&path, html).map_err(|e| format!("Не записать {}: {}", path.display(), e))?;
    Ok((made, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_needs_no_raw_tags() {
        assert_eq!(html_escape("<b>&\"x"), "&lt;b&gt;&amp;&quot;x");
    }

    #[test]
    fn pair_names_follow_bench_protocol() {
        assert_eq!("E0_S42_AB.png", format!("E{}_S{}_AB.png", 0, 42));
        assert_eq!("H7_S1337_AB.png", format!("H{}_S{}_AB.png", 7, 1337));
    }
}

#[cfg(test)]
mod integration {
    use super::*;

    #[test]
    #[ignore = "склейка ГОТОВЫХ PNG bench-артефактов (LLM не нужен) — запуск по явной просьбе юзера"]
    fn ab_collages() {
        let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
        let out_root = env_dir("IMG_BENCH_OUT").unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../king_orch_3/test/bench")
        });
        let bundles: Vec<String> = std::env::var("AB_BUNDLES")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.split(',').map(|b| b.trim().to_string()).collect())
            .unwrap_or_else(|| AB_DEFAULT_BUNDLES.iter().map(|s| s.to_string()).collect());

        let (t2i, tsrc) = load_prompts();
        let (edit, esrc) = load_edit_prompts();
        assert!(t2i.len() >= 8, "t2i-промптов < 8 (источник: {})", tsrc);
        assert!(edit.len() >= 8, "edit-промптов < 8 (источник: {})", esrc);

        let (made, html) = build_ab_artifacts(&out_root, &bundles, &t2i, &edit)
            .expect("сборка A/B артефактов не удалась");
        println!("Коллажей: {}; страница: {}", made, html.display());
        assert!(made >= 16, "ожидался как минимум edit-грид одного бандла (16 коллажей), сделано {}", made);
    }
}

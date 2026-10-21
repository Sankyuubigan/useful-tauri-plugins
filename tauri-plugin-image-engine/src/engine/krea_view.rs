//! HTML-отчёт сравнения апскейла Krea2 (`krea_compare.html`).
//!
//! Требование юзера дословно: «сначала целая картинка оригинал, потом
//! зумированные куски в двух вариантах апскейла». На каждую картинку:
//! 1) полный оригинал (full-width, клик — 1:1);
//! 2) синхронный зум-блок: 3 колонки (ноги), колесо — зум к точке курсора,
//!    перетаскивание — пан, двойной клик — сброс, 100% = пиксель-в-пиксель;
//!    кнопки-якоря «зоны сравнения» двигают все три колонки в одну зону;
//! 3) статические кропы по 3 зонам (оригинал ×2 + 3 ноги) — гарантированный
//!    fallback: разница читается даже без зума.
//!
//! Кропы режутся из PNG (в плагине включён только PNG-декод `image`), поэтому
//! для JPEG-источников кроп оригинала не строится — об этом честно пишет
//! лог (сама картинка в отчёте показывается браузером как есть).

use std::path::{Path, PathBuf};

use crate::engine::krea_compare::{KreaCompareReport, KreaSource};

/// Зоны сравнения: (id, подпись, x, y, w, h) — доли от размера картинки.
pub const KREA_ZONES: &[(&str, &str, f32, f32, f32, f32)] = &[
    ("center", "Центр", 0.25, 0.25, 0.50, 0.50),
    ("detail", "Деталь центра (×4)", 0.375, 0.375, 0.25, 0.25),
    ("edge", "Край (левый верхний угол)", 0.0, 0.0, 0.25, 0.25),
];

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Прямоугольник зоны в пикселях картинки.
fn zone_rect(w: u32, h: u32, zx: f32, zy: f32, zw: f32, zh: f32) -> (u32, u32, u32, u32) {
    let x = (w as f32 * zx).round() as u32;
    let y = (h as f32 * zy).round() as u32;
    let rw = ((w as f32 * zw).round() as u32).max(1);
    let rh = ((h as f32 * zh).round() as u32).max(1);
    (x.min(w.saturating_sub(1)), y.min(h.saturating_sub(1)), rw.min(w - x), rh.min(h - y))
}

/// Кроп зоны из PNG. `scale2` — во столько раз увеличить (для оригинала: 2,
/// чтобы пиксели совпали с масштабом ног = ×2).
fn crop_zone(src: &Path, out: &Path, zx: f32, zy: f32, zw: f32, zh: f32, scale2: bool) -> Result<(), String> {
    let img = image::open(src).map_err(|e| format!("Открыть {}: {}", src.display(), e))?;
    let (x, y, w, h) = zone_rect(img.width(), img.height(), zx, zy, zw, zh);
    let mut crop = img.crop_imm(x, y, w, h);
    if scale2 {
        crop = crop.resize_exact(w * 2, h * 2, image::imageops::FilterType::Nearest);
    }
    if let Some(p) = out.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("Создать {}: {}", p.display(), e))?;
    }
    crop.save(out).map_err(|e| format!("Записать {}: {}", out.display(), e))
}

/// Относительный путь от out_dir до абсолютного (для <img src>).
fn rel_path(out_dir: &Path, abs: &str) -> String {
    let p = PathBuf::from(abs);
    match p.strip_prefix(out_dir) {
        Ok(r) => r.to_string_lossy().replace('\\', "/"),
        Err(_) => abs.replace('\\', "/"),
    }
}

/// Построить отчёт → `krea_compare.html`. Возвращает путь к файлу.
pub fn build_krea_html(
    out_dir: &Path,
    sources: &[KreaSource],
    report: &KreaCompareReport,
) -> Result<PathBuf, String> {
    let crops_dir = out_dir.join("crops");
    std::fs::create_dir_all(&crops_dir).map_err(|e| format!("Создать {}: {}", crops_dir.display(), e))?;

    let mut sections = String::new();
    let mut sources_note = String::new();
    let mut crop_warnings: Vec<String> = Vec::new();

    for src in sources {
        let mut zoom_cols = String::new();
        let mut zone_rows = String::new();

        for leg in &report.legs {
            let cell = report
                .cells
                .iter()
                .find(|c| c.source_id == src.id && c.leg == leg.id && c.ok);
            let Some(cell) = cell else {
                zoom_cols.push_str(&format!(
                    "<div class=\"col\"><div class=\"vp\"><div class=\"miss\">нет результата</div></div><div class=\"cap\">{}</div></div>",
                    html_escape(&leg.label)
                ));
                continue;
            };
            let rel = rel_path(out_dir, &cell.image_path);
            zoom_cols.push_str(&format!(
                "<div class=\"col\"><div class=\"vp\"><img src=\"{0}\" alt=\"{1}\"></div><div class=\"cap\">{1} · {2:.1} с</div></div>",
                html_escape(&rel),
                html_escape(&leg.label),
                cell.gen_time_s
            ));

            // Кропы этой ноги по всем зонам.
            for (zid, _, zx, zy, zw, zh) in KREA_ZONES {
                let out = crops_dir.join(format!("K{}_zone{}_{}.png", src.id, zid, leg.id));
                if !out.exists() {
                    if let Err(e) = crop_zone(Path::new(&cell.image_path), &out, *zx, *zy, *zw, *zh, false) {
                        crop_warnings.push(format!("{} / {}: {}", src.id, leg.id, e));
                    }
                }
            }
        }

        // Кроп оригинала — средствами БРАУЗЕРА (CSS-crop): исходники Commons
        // приходят в JPEG, а в плагине включён только PNG-декод `image`
        // (ради веса бинаря хоста). Кроп оригинала в Rust потребовал бы
        // расширения бинаря. Плитка приводится к ТОЙ ЖЕ высоте, что у кропов ног
        // (TILE_H), иначе масштабы несопоставимы; зона — та же (по долям).
        const TILE_H: u32 = 220;
        let src_path = out_dir.join("src").join(&src.file);
        for (zid, ztitle, zx, zy, zw, zh) in KREA_ZONES {
            let (rx, ry, rw, rh) = zone_rect(src.w, src.h, *zx, *zy, *zw, *zh);
            let scale = TILE_H as f32 / rh.max(1) as f32;
            let mut tiles = String::new();
            tiles.push_str(&format!(
                "<div class=\"tilecrop\" style=\"width:{0}px;height:{1}px\" title=\"{2}\"><img src=\"{3}\" style=\"width:{4}px;height:{5}px;transform:translate({6}px,{7}px)\" alt=\"оригинал {2}\"></div>",
                ((rw as f32 * scale).round() as u32).max(1),
                TILE_H,
                html_escape(ztitle),
                html_escape(&rel_path(out_dir, &src_path.to_string_lossy())),
                (src.w as f32 * scale).round() as u32,
                (src.h as f32 * scale).round() as u32,
                -((rx as f32 * scale).round() as i32),
                -((ry as f32 * scale).round() as i32)
            ));
            for leg in &report.legs {
                let cell = report
                    .cells
                    .iter()
                    .find(|c| c.source_id == src.id && c.leg == leg.id && c.ok);
                let Some(_cell) = cell else { continue };
                let p = crops_dir.join(format!("K{}_zone{}_{}.png", src.id, zid, leg.id));
                if !p.exists() {
                    continue;
                }
                tiles.push_str(&format!(
                    "<img class=\"tile\" src=\"{}\" alt=\"{} {}\">",
                    html_escape(&rel_path(out_dir, &p.to_string_lossy())),
                    html_escape(&leg.label),
                    html_escape(ztitle)
                ));
            }
            zone_rows.push_str(&format!(
                "<div class=\"zone\"><div class=\"ztitle\">{}</div><div class=\"tiles\">{}</div></div>",
                html_escape(ztitle),
                tiles
            ));
        }

        let src_rel = rel_path(out_dir, &src_path.to_string_lossy());
        let lic_text = if src.author.trim().is_empty() {
            src.license.clone()
        } else {
            format!("{} · автор: {}", src.license, src.author)
        };
        sources_note.push_str(&format!(
            "<li><b>K{0}</b> — <a href=\"{1}\">{2}</a> · лицензия: <a href=\"{3}\">{4}</a> · автор: {5} · скачано {6}×{7} · sha256 {8}</li>",
            src.id,
            html_escape(&src.source_url),
            html_escape(&src.commons_title),
            html_escape(&src.license_url),
            html_escape(&src.license),
            html_escape(&src.author),
            src.w,
            src.h,
            html_escape(&src.sha256)
        ));
        sections.push_str(&format!(
            "<section>\n<h2>K{0} — {1}</h2>\n<div class=\"meta\">Источник: <a href=\"{2}\">{3}</a> · Лицензия: {4} · Оригинал {5}×{6} · canvas апскейла {7}×{8} · strength {9}</div>\n\
<div class=\"full\"><img src=\"{10}\" alt=\"оригинал K{0}\"></div>\n\
<div class=\"zoom\" data-block><div class=\"zoombar\"><button data-zone=\"0\">Центр</button><button data-zone=\"1\">Деталь</button><button data-zone=\"2\">Край</button><button data-reset>Сброс (100%)</button><span class=\"hint\">колесо — зум к курсору · тянуть — панорама · двойной клик — сброс</span></div><div class=\"cols\">{11}</div></div>\n\
<div class=\"zones\">{12}</div>\n</section>\n",
            src.id,
            html_escape(&src.commons_title),
            html_escape(&src.source_url),
            html_escape(&src.commons_title),
            html_escape(&lic_text),
            src.w,
            src.h,
            src.w * 2,
            src.h * 2,
            report.strength,
            html_escape(&src_rel),
            zoom_cols,
            zone_rows
        ));
    }

    let legs_note: String = report
        .legs
        .iter()
        .map(|l| {
            let sig = match &l.custom_sigmas {
                Some(s) => format!("custom_sigmas: {} значений, последнее {:.4}", s.len(), s.last().copied().unwrap_or(0.0)),
                None => "custom_sigmas: нет (дефолт сервера)".to_string(),
            };
            format!(
                "<li><b>{}</b> — {} · шагов {} · cfg {} · {}</li>",
                html_escape(&l.label),
                html_escape(&l.bundle),
                l.steps,
                l.cfg_scale,
                sig
            )
        })
        .collect();

    let warnings = if crop_warnings.is_empty() {
        String::new()
    } else {
        format!(
            "<p class=\"warn\">Кропы, которые не построились ({}): {}</p>",
            crop_warnings.len(),
            html_escape(&crop_warnings.join("; "))
        )
    };

    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Krea2 vs OURS — апскейл</title><style>{css}</style></head><body>\n\
<h1>Krea2-Turbo vs наши Qwen-Image-2.1 Turbo — апскейл одних и тех же чужих картинок</h1>\n\
<p class=\"note\">Промпт один на все ноги: <code>{prompt}</code> · strength {strength} · seed {seed} · canvas = «до»-размер ×2 (потолок 2048).<br>\
Правило честности: различаются ТОЛЬКО веса/движок. Krea2 — отдельный стек sd.cpp (Krea-2-Turbo GGUF + Wan2.1 VAE + Qwen3-VL-4B), пресет из официального первоисточника krea-ai/krea-2: 8 шагов, CFG выключен, mu=1.15.<br>\
Публичных метрик качества у Krea2 нет — вердикт «лучше/хуже» здесь только ваш, по картинкам.</p>\n\
<ul class=\"legs\">{legs_note}</ul>\n\
<ul class=\"sources\">{sources_note}</ul>\n\
{warnings}\n{sections}\n\
<div id=\"lb\"><img id=\"lbimg\" src=\"\" alt=\"\"></div>\n{script}\n</body></html>",
        css = KREA_CSS,
        prompt = html_escape(&report.prompt),
        strength = report.strength,
        seed = report.seed,
        legs_note = legs_note,
        sources_note = sources_note,
        warnings = warnings,
        sections = sections,
        script = KREA_JS,
    );

    let path = out_dir.join("krea_compare.html");
    std::fs::write(&path, html).map_err(|e| format!("Записать {}: {}", path.display(), e))?;
    Ok(path)
}

const KREA_CSS: &str = r#"
body{font-family:Segoe UI,system-ui,sans-serif;background:#101215;color:#e6e6e6;margin:0;padding:18px}
h1{font-size:22px}h2{font-size:17px;margin:26px 0 6px}
.note{background:#171a20;border:1px solid #262b33;border-radius:8px;padding:12px;line-height:1.5}
.note code{background:#0d0f13;padding:2px 5px;border-radius:4px}
.legs{background:#171a20;border:1px solid #262b33;border-radius:8px;padding:10px 10px 10px 30px;line-height:1.7}
.meta{color:#9fb0c0;font-size:13px;margin-bottom:8px}
.meta a{color:#6fb3ff}
.full{margin:10px 0}
.full img{width:100%;height:auto;display:block;border:1px solid #262b33;border-radius:6px;image-rendering:pixelated}
.zoom{border:1px solid #262b33;border-radius:8px;overflow:hidden;margin:10px 0}
.zoombar{display:flex;gap:8px;align-items:center;padding:8px;background:#171a20;flex-wrap:wrap}
.zoombar button{background:#222833;border:1px solid #39414d;color:#dfe6ee;border-radius:5px;padding:4px 10px;cursor:pointer}
.zoombar button:hover{background:#2b3240}
.hint{color:#7d8b99;font-size:12px}
.cols{display:grid;grid-template-columns:repeat(3,1fr);gap:8px;padding:8px}
.col{min-width:0}
.vp{position:relative;height:420px;overflow:hidden;background:#0b0d10;border:1px solid #262b33;border-radius:6px;cursor:grab}
.vp.drag{cursor:grabbing}
.vp img{position:absolute;top:0;left:0;transform-origin:0 0;image-rendering:pixelated;max-width:none}
.miss{display:flex;height:100%;align-items:center;justify-content:center;color:#7d8b99;font-size:13px}
.cap{padding:6px 2px;font-size:12px;color:#9fb0c0}
.zones{margin:10px 0 20px}
.zone{margin:8px 0}
.ztitle{font-size:13px;color:#9fb0c0;margin-bottom:4px}
.tiles{display:flex;gap:8px;flex-wrap:wrap}
.tile{height:220px;width:auto;image-rendering:pixelated;border:1px solid #262b33;border-radius:4px;background:#0b0d10}
.tilecrop{position:relative;overflow:hidden;border:1px solid #4c6b8a;border-radius:4px;background:#0b0d10;flex:none}
.tilecrop img{position:absolute;left:0;top:0;image-rendering:pixelated;max-width:none}
.sources{background:#171a20;border:1px solid #262b33;border-radius:8px;padding:10px 10px 10px 30px;line-height:1.6;font-size:13px}
.sources a{color:#6fb3ff}
.warn{color:#ffb86b;font-size:13px}
#lb{position:fixed;inset:0;background:rgba(0,0,0,.92);display:none;align-items:center;justify-content:center;z-index:50;cursor:zoom-out}
#lb img{max-width:96vw;max-height:96vh;image-rendering:pixelated}
"#;

const KREA_JS: &str = r#"
document.addEventListener('click',function(e){
  var lb=document.getElementById('lb'),li=document.getElementById('lbimg');
  if(e.target.tagName==='IMG'&&!e.target.closest('.vp')){li.src=e.target.src;lb.style.display='flex';}
  else if(e.target===lb){lb.style.display='none';}
});
document.querySelectorAll('.zoom').forEach(function(blk){
  var st={z:1,x:0,y:0};
  var vps=[].slice.call(blk.querySelectorAll('.vp'));
  var imgs=vps.map(function(vp){return vp.querySelector('img');}).filter(Boolean);
  var ZONES=[[0.25,0.25,0.50,0.50],[0.375,0.375,0.25,0.25],[0.0,0.0,0.25,0.25]];
  function apply(){
    imgs.forEach(function(im){
      im.style.transform='translate('+st.x+'px,'+st.y+'px) scale('+st.z+')';
    });
    blk.querySelectorAll('.zoombar').forEach(function(b){});
    var zl=blk.querySelector('.zoomval');
    if(zl){zl.textContent=Math.round(st.z*100)+'%';}
  }
  function vpOf(t){var v=t.closest? t.closest('.vp'):null;return v;}
  blk.addEventListener('wheel',function(e){
    e.preventDefault();
    var vp=vpOf(e.target); if(!vp) return;
    var r=vp.getBoundingClientRect();
    var cx=e.clientX-r.left, cy=e.clientY-r.top;
    var k=(e.deltaY<0)?1.25:1/1.25;
    var nz=Math.min(16,Math.max(1,st.z*k));
    var f=nz/st.z;
    st.x=cx-(cx-st.x)*f; st.y=cy-(cy-st.y)*f; st.z=nz;
    apply();
  },{passive:false});
  var drag=null;
  blk.addEventListener('mousedown',function(e){
    var vp=vpOf(e.target); if(!vp) return;
    drag={vp:vp,sx:e.clientX,sy:e.clientY,ox:st.x,oy:st.y}; vp.classList.add('drag'); e.preventDefault();
  });
  window.addEventListener('mousemove',function(e){
    if(!drag) return;
    st.x=drag.ox+(e.clientX-drag.sx); st.y=drag.oy+(e.clientY-drag.sy); apply();
  });
  window.addEventListener('mouseup',function(){ if(drag){drag.vp.classList.remove('drag'); drag=null;} });
  blk.addEventListener('dblclick',function(e){ if(vpOf(e.target)){st={z:1,x:0,y:0};apply();} });
  blk.querySelectorAll('.zoombar button[data-zone]').forEach(function(btn){
    btn.addEventListener('click',function(){
      var z=ZONES[parseInt(btn.getAttribute('data-zone'),10)]; if(!z) return;
      var im=imgs[0]; if(!im) return;
      var iw=im.naturalWidth||1, ih=im.naturalHeight||1;
      var vp=vps[0]; var r=vp.getBoundingClientRect();
      st.z=4;
      st.x=-(iw*z[0]*st.z)+(r.width-iw*z[2]*st.z)/2;
      st.y=-(ih*z[1]*st.z)+(r.height-ih*z[3]*st.z)/2;
      apply();
    });
  });
  blk.querySelectorAll('.zoombar button[data-reset]').forEach(function(btn){
    btn.addEventListener('click',function(){st={z:1,x:0,y:0};apply();});
  });
  apply();
});
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Зона всегда внутри картинки и не нулевая.
    #[test]
    fn zone_rect_stays_inside_image() {
        let (x, y, w, h) = zone_rect(1000, 800, 0.375, 0.375, 0.25, 0.25);
        assert_eq!((x, y, w, h), (375, 300, 250, 200));
        assert!(x + w <= 1000 && y + h <= 800);
    }

    #[test]
    fn zone_rect_never_exceeds_image() {
        let (x, y, w, h) = zone_rect(64, 64, 0.0, 0.0, 0.25, 0.25);
        assert_eq!((x, y, w, h), (0, 0, 16, 16));
        let (x, y, w, h) = zone_rect(10, 10, 0.9, 0.9, 0.5, 0.5);
        assert!(x + w <= 10 && y + h <= 10, "{} {} {} {}", x, y, w, h);
    }

    #[test]
    fn html_escape_blocks_tags() {
        let s = html_escape("<img src=x onerror=\"alert(1)\">");
        assert!(!s.contains('<'), "неэкранированный <: {}", s);
        assert!(s.contains("&lt;"));
        assert!(s.contains("&quot;"));
    }

    /// Подписи зон уникальны, id — латиница без пробелов (используются в именах файлов).
    #[test]
    fn zones_have_stable_ids() {
        let mut ids: Vec<&str> = KREA_ZONES.iter().map(|z| z.0).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), KREA_ZONES.len());
        for (id, _, _, _, _, _) in KREA_ZONES {
            assert!(id.chars().all(|c| c.is_ascii_alphanumeric()), "id {}", id);
        }
    }
}

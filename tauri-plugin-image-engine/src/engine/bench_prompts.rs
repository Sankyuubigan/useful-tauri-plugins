//! Наборы промптов и константы расширенного bench-протокола
//! (PRP «Сборки Qwen-Image-2.1 Turbo», Шаг 8, 19.10.2026).
//!
//! pHash-хелперы — см. `bench_ext.rs`.
//!
//! Сравнение между бандлами обязано идти на ОДНОМ наборе промптов:
//! обновлять наборы можно только через файлы хоста
//! (`test/bench/prompts.json` / `test/bench/edit_prompts.json`), а не
//! точечно править код.

use std::path::{Path, PathBuf};

/// 8 промптов бенч-грида (методика unsloth: 1024×1024, разные жанры).
pub const DEFAULT_PROMPTS: &[&str] = &[
    "Ultra-detailed photo of a misty alpine valley at sunrise, dramatic god rays through pine trees, 8k, photorealistic",
    "Studio portrait of a middle-aged fisherman with weathered skin and grey beard, rembrandt lighting, 85mm lens, photorealistic",
    "Modernist concrete house on a cliff above the ocean at dusk, architectural photography, soft warm interior lights",
    "Close-up of a bowl of handmade udon noodles with soft-boiled egg, steam rising, moody restaurant lighting, food photography",
    "Red fox curled up in deep snow, shallow depth of field, golden hour backlight, wildlife photography",
    "Abstract fluid painting with deep indigo and molten gold ink swirls on white silk, macro detail",
    "Retro-futuristic monorail gliding over a neon-lit megacity at night, rain streaks, cinematic wide shot",
    "Still life with brass telescope, antique maps and a pocket watch on a walnut desk, candlelight, Rembrandt tones",
];

/// 8 edit-промптов: однотипные инструкции-преобразования одного референса.
/// Сравнение бандлов идёт на одном референсе и одном наборе инструкций.
pub const EDIT_PROMPTS: &[&str] = &[
    "Repaint the sky into a deep purple twilight scattered with bright stars",
    "Make it late golden hour: warm amber sunlight and long soft shadows",
    "Add light snowfall: the ground and trees dusted with fresh snow, cold blue light",
    "Turn it into a misty autumn morning with orange and crimson foliage",
    "Turn the photo into a Japanese sumi-e ink wash painting",
    "Make it a rainy overcast day with wet surfaces reflecting the light",
    "Make it a moonlit night: silver light, glowing stars, thin mist",
    "Brighten it into a crisp midday summer look with a clear blue sky",
];

/// Протокол hires-бенча (native hires-fix sd.cpp, api.md 19.10.2026):
/// второй проход Lanczos, масштаб ×2 → 2048×2048, denoise-проход.
pub(crate) const HIRES_UPSCALER: &str = "Lanczos";
pub(crate) const HIRES_SCALE: f64 = 2.0;
pub(crate) const HIRES_DENOISE: f64 = 0.4;

/// Тело `hires`-объекта для `/sdcpp/v1/img_gen`. `custom_sigmas` в hires
/// сознательно НЕ задаётся — второй проход идёт с дефолтным планировщиком
/// сервера (честный протокол, одинаков для всех бандлов; расхождения
/// расписываются в notes отчёта).
pub(crate) fn hires_payload(steps: u32) -> serde_json::Value {
    serde_json::json!({
        "enabled": true,
        "upscaler": HIRES_UPSCALER,
        "scale": HIRES_SCALE,
        "steps": steps,
        "denoising_strength": HIRES_DENOISE,
    })
}

fn read_prompts_file(path: &Path, src_name: &str, fallback: &[&str], fallback_note: &str) -> (Vec<String>, String) {
    let mut from_file = Vec::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let arr = match v {
                serde_json::Value::Array(a) => Some(a),
                serde_json::Value::Object(m) => m.get("prompts").and_then(|p| p.as_array()).cloned(),
                _ => None,
            };
            if let Some(a) = arr {
                from_file = a
                    .into_iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .filter(|s| !s.trim().is_empty())
                    .collect();
            }
        }
    }
    if from_file.len() >= 8 {
        return (from_file, src_name.to_string());
    }
    (
        fallback.iter().map(|s| (*s).to_string()).collect(),
        format!("{} (в файле {} меньше 8 промптов)", fallback_note, path.display()),
    )
}

/// Промпты t2i-замера: из test/bench/prompts.json хоста, иначе встроенный дефолт.
pub(crate) fn load_prompts() -> (Vec<String>, String) {
    let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../king_orch_3/test/bench/prompts.json"));
    read_prompts_file(&path, "test/bench/prompts.json", DEFAULT_PROMPTS, "встроенный дефолт bench_prompts.rs")
}

/// Промпты edit-замера: из test/bench/edit_prompts.json, иначе встроенный дефолт.
pub(crate) fn load_edit_prompts() -> (Vec<String>, String) {
    let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../king_orch_3/test/bench/edit_prompts.json"));
    read_prompts_file(&path, "test/bench/edit_prompts.json", EDIT_PROMPTS, "встроенный дефолт EDIT_PROMPTS")
}

pub(crate) fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var(var).ok().filter(|s| !s.trim().is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hires_payload_has_contract_fields() {
        let v = hires_payload(8);
        assert_eq!(v["enabled"], true);
        assert_eq!(v["upscaler"], "Lanczos");
        assert_eq!(v["scale"], 2.0);
        assert_eq!(v["steps"], 8);
        assert_eq!(v["denoising_strength"], 0.4);
        assert!(v.get("custom_sigmas").is_none(), "hires custom_sigmas не задаётся — дефолт сервера");
    }

    #[test]
    fn prompt_sets_have_eight_unique_lines() {
        assert_eq!(DEFAULT_PROMPTS.len(), 8);
        assert_eq!(EDIT_PROMPTS.len(), 8);
    }
}

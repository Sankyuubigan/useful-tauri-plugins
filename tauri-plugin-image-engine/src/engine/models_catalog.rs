//! Каталог бандлов image-моделей.
//!
//! SSOT — встроенный JSON `image_models_catalog.json` (компилируется через
//! `include_str!`). Бандл = 4 файла (diffusion + vae + text encoder + mmproj),
//! качаются одной кнопкой в папку бандла. Дефолтные пресеты генерации —
//! ТОЛЬКО здесь (отдельного sampling_presets.json нет).

use serde::{Deserialize, Serialize};

/// Один файл бандла.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BundleFile {
    /// Роль: "diffusion" | "vae" | "llm" | "mmproj".
    pub role: String,
    /// Имя файла на диске (в папке бандла).
    pub filename: String,
    /// Прямая ссылка на скачивание (HF resolve).
    pub download_url: String,
    /// Точный размер в байтах (для прогресса и проверки места).
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

/// Дефолтный пресет генерации бандла (единственное место правды).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ImagePreset {
    #[serde(default = "default_cfg_scale")]
    pub cfg_scale: f32,
    #[serde(default = "default_sampler")]
    pub sampler: String,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_steps")]
    pub steps: u32,
    #[serde(default = "default_seed")]
    pub seed: i64,
    /// Явный сигма-профиль (Turbo/Viggle-семейство). Когда задан —
    /// steps должен равняться длине массива («sigma count determines steps»
    /// в sd.cpp), поле шлётся в sample_params.custom_sigmas.
    #[serde(default)]
    pub custom_sigmas: Option<Vec<f32>>,
    /// Имя шедулера для sample_params.scheduler (обычно не нужно — сервер
    /// подставляет дефолт; оставлено как явный задел).
    #[serde(default)]
    pub scheduler: Option<String>,
    /// Задел на будущее: именной пресет сервера (напр. "qwen-image-2.1-turbo")
    /// вместо массива. Сейчас не используется.
    #[serde(default)]
    pub native_sigma_preset: Option<String>,
}

fn default_cfg_scale() -> f32 { 6.0 }
fn default_sampler() -> String { "euler".to_string() }
fn default_width() -> u32 { 1024 }
fn default_height() -> u32 { 1024 }
fn default_steps() -> u32 { 28 }
fn default_seed() -> i64 { -1 }

impl Default for ImagePreset {
    fn default() -> Self {
        Self {
            cfg_scale: default_cfg_scale(),
            sampler: default_sampler(),
            width: default_width(),
            height: default_height(),
            steps: default_steps(),
            seed: default_seed(),
            custom_sigmas: None,
            scheduler: None,
            native_sigma_preset: None,
        }
    }
}

/// Один бандл каталога.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ImageBundleEntry {
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub files: Vec<BundleFile>,
    /// Доп. ключ `--model-args` сервера (строка «key=value»).
    ///
    /// None/пусто — флаг ВООБЩЕ не отправляется: у стеков без собственного
    /// парсера model-args (Krea2, sd.cpp `krea2.hpp`) qwen-ключ
    /// `qwen_image_2_1_prefix_cache_type` молча игнорируется, поэтому честнее
    /// не посылать его вовсе (tasks/21.10.26 …md §3.2).
    #[serde(default)]
    pub model_args: Option<String>,
    /// Compute-буфер (МБ) для ЭТОГО бандла — ЗАМЕРЕННОЕ значение
    /// («compute buffer» из лога сервера, методика §4 docs/BUNDLES.md).
    /// None — константа COMPUTE_BUFFERS_MB (preflight); поле заполняется
    /// замером, а не подбором (замер Krea2 — следующий этап плана §3.3).
    #[serde(default)]
    pub compute_buffers_mb: Option<f64>,
    #[serde(default)]
    pub preset: ImagePreset,
    /// Суммарный размер файлов на диске, ГБ (строка для UI).
    #[serde(default)]
    pub size_gb: Option<String>,
    /// VRAM для full-resident (макс. скорость), ГБ.
    #[serde(default)]
    pub vram_fast_gb: Option<f32>,
    /// VRAM минимум (offload/сегменты), ГБ.
    #[serde(default)]
    pub vram_min_gb: Option<f32>,
    /// RAM минимум (веса живут в RAM при offload), ГБ.
    #[serde(default)]
    pub ram_min_gb: Option<f32>,
}

const EMBEDDED_CATALOG: &str = include_str!("../../image_models_catalog.json");

/// Встроенный каталог бандлов.
pub fn load_image_catalog() -> Vec<ImageBundleEntry> {
    serde_json::from_str(EMBEDDED_CATALOG).unwrap_or_default()
}

/// Бандл по умолчанию (первый с is_default).
pub fn default_bundle_entry() -> Option<ImageBundleEntry> {
    load_image_catalog().into_iter().find(|e| e.is_default)
}

/// Бандл по имени каталога (для выбора юзером: selected_bundle).
pub fn bundle_by_name(name: &str) -> Option<ImageBundleEntry> {
    load_image_catalog().into_iter().find(|e| e.name == name)
}

/// Бандл по имени файла внутри него (stem без расширения, lower).
pub fn find_bundle_entry<'a>(
    catalog: &'a [ImageBundleEntry],
    file_path: &str,
) -> Option<&'a ImageBundleEntry> {
    let stem = std::path::Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())?;
    catalog.iter().find(|e| {
        e.files.iter().any(|f| {
            f.filename
                .trim_end_matches(".gguf")
                .trim_end_matches(".safetensors")
                .to_lowercase()
                == stem
        })
    })
}

/// Суммарный размер файлов бандла в байтах (по size_bytes, неизвестные = 0).
pub fn bundle_disk_bytes(entry: &ImageBundleEntry) -> u64 {
    entry.files.iter().map(|f| f.size_bytes.unwrap_or(0)).sum()
}

/// Пути файлов бандла по ролям внутри папки бандла.
pub fn bundle_weights_files(
    bundle_dir: &std::path::Path,
    entry: &ImageBundleEntry,
) -> std::collections::HashMap<String, String> {
    entry
        .files
        .iter()
        .map(|f| {
            (
                f.role.clone(),
                bundle_dir.join(&f.filename).to_string_lossy().to_string(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_and_has_default_bundle() {
        let catalog = load_image_catalog();
        assert_eq!(
            catalog.len(),
            4,
            "после добавления стека Krea2 в каталоге 4 бандла, got {}",
            catalog.len()
        );
        let def = default_bundle_entry().expect("нет бандла по умолчанию");
        // Дефолт после 19.10.2026 — официальный Turbo unsloth (LPIPS 0.103/0.083).
        assert_eq!(def.name, "qwen-image-2.1-turbo-unsloth-q5_ks");
        // Полный функционал (правило юзера): у КАЖДОГО набора редактирование —
        // значит в бандле роли mmproj и llm обязаны присутствовать.
        assert_eq!(def.files.len(), 4, "бандл = 4 файла, got {}", def.files.len());
        let roles: Vec<&str> = def.files.iter().map(|f| f.role.as_str()).collect();
        for r in ["diffusion", "vae", "llm", "mmproj"] {
            assert!(roles.contains(&r), "нет роли {}", r);
        }
        assert!((def.preset.cfg_scale - 1.0).abs() < f32::EPSILON, "Turbo: CFG 1.0");
        assert_eq!(def.preset.sampler, "euler");
        assert_eq!(def.preset.steps, 8, "официальный Turbo = 8 шагов");
        assert_eq!(def.preset.width % 32, 0);
        assert_eq!(def.preset.height % 32, 0);
    }

    #[test]
    fn bundle_disk_bytes_sums_sizes() {
        let def = default_bundle_entry().expect("бандл");
        let total = bundle_disk_bytes(&def);
        // diffusion 4.50 + llm 5.03 + mmproj 1.16 + vae 0.68 = 11.36 ГБ.
        assert!(total > 11_000_000_000, "ожидалось >11 ГБ, got {}", total);
    }

    #[test]
    fn turbo_bundles_parse_with_sigma_invariant() {
        let catalog = load_image_catalog();
        let find = |name: &str| {
            catalog
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("нет бандла {}", name))
                .clone()
        };
        for name in [
            "qwen-image-2.1-turbo-unsloth-q5_ks",
            "qwen-image-2.1-turbo-viggle-q4km",
            "krea2-turbo-q4km",
            "krea2-turbo-q3km",
        ] {
            let e = find(name);
            let sigmas = e
                .preset
                .custom_sigmas
                .clone()
                .expect("Turbo-бандл обязан иметь custom_sigmas");
            assert_eq!(
                sigmas.len() as u32,
                e.preset.steps + 1,
                "шаги == len(custom_sigmas) - 1 (sd.cpp SamplePlan::resolve, терминальный 0.0 в массиве) у {}",
                name
            );
            assert_eq!(e.preset.cfg_scale, 1.0, "Turbo: CFG 1.0 у {}", name);
            let roles: Vec<&str> = e.files.iter().map(|f| f.role.as_str()).collect();
            assert!(roles.contains(&"diffusion"));
            assert!(roles.contains(&"llm"));
            assert!(roles.contains(&"vae"));
            // Правило юзера 19.10.2026: НЕТ наборов без редактирования — mmproj у всех.
            assert!(roles.contains(&"mmproj"), "edit-функция обязательна: mmproj в наборе {}", name);
            assert_eq!(e.files.len(), 4, "полный набор = 4 файла у {}", name);
            for f in &e.files {
                // Роль-агностичный минимум: VAE ≥ 100 МБ, mmproj ≥ 300 МБ
                // (Qwen3VL-4B F16 mmproj = 836 МБ < старого порога 1 ГБ),
                // остальное ≥ 1 ГБ.
                let min = match f.role.as_str() {
                    "vae" => 100_000_000,
                    "mmproj" => 300_000_000,
                    _ => 1_000_000_000,
                };
                assert!(
                    f.size_bytes.unwrap_or(0) > min,
                    "странный размер {} файла {}",
                    f.size_bytes.unwrap_or(0),
                    f.filename
                );
            }
        }
        let unsloth = find("qwen-image-2.1-turbo-unsloth-q5_ks");
        let s = unsloth.preset.custom_sigmas.as_ref().unwrap();
        assert_eq!(s.len(), 9, "официальный Turbo = 9 сигм (8 шагов + 0.0)");
        assert!((s[s.len() - 1]).abs() < 1e-6, "последняя сигма обязана быть 0.0");
        // Дефолт после удаления base (вердикт юзера 19.10.2026): unsloth Turbo.
        let def = default_bundle_entry().expect("дефолт");
        assert_eq!(def.name, "qwen-image-2.1-turbo-unsloth-q5_ks", "дефолт — unsloth Turbo");
    }

    /// Контракт стеков Krea2: TE/Vae/mmproj из Qwen3-VL-4B, официал-сигма 1.0
    /// первой и 0.0 последней (Значения — krea_compare.rs, сверены с HF API).
    #[test]
    fn krea2_bundles_stack_and_model_args_contract() {
        let catalog = load_image_catalog();
        let find = |name: &str| {
            catalog
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("нет бандла {}", name))
                .clone()
        };
        let sigmas: [f32; 9] = [
            1.0, 0.95672375, 0.90453076, 0.8403488, 0.75951093, 0.6545668, 0.5128441,
            0.31090105, 0.0,
        ];
        for (name, diffusion_file) in [
            ("krea2-turbo-q4km", "Krea-2-Turbo-Q4_K_M.gguf"),
            ("krea2-turbo-q3km", "Krea-2-Turbo-Q3_K_M.gguf"),
        ] {
            let e = find(name);
            let file = |role: &str| {
                e.files
                    .iter()
                    .find(|f| f.role == role)
                    .unwrap_or_else(|| panic!("{}: нет роли {}", name, role))
                    .clone()
            };
            assert_eq!(file("diffusion").filename, diffusion_file);
            assert_eq!(file("llm").filename, "Qwen3VL-4B-Instruct-Q4_K_M.gguf");
            assert_eq!(file("mmproj").filename, "mmproj-Qwen3VL-4B-Instruct-F16.gguf");
            assert_eq!(file("vae").filename, "wan_2.1_vae.safetensors");
            // Компакт-контракт krea_compare.rs: mu=1.15, 9 сигм, last = 0.0.
            assert_eq!(e.preset.steps, 8);
            let preset_sigmas = e.preset.custom_sigmas.expect("Krea2 custom_sigmas");
            assert_eq!(preset_sigmas.len(), 9);
            assert!(
                (preset_sigmas[0] - sigmas[0]).abs() < 1e-6,
                "первая сигма Krea2 обязана быть 1.0"
            );
            for (preset_value, canon) in preset_sigmas.iter().zip(sigmas.iter()) {
                assert!(
                    (preset_value - canon).abs() < 1e-6,
                    "сигма {} ≠ официал {}: {} vs {}",
                    name,
                    canon,
                    preset_value,
                    canon
                );
            }
            // Krea2 НЕ получает qwen model-args (свой стек без парсера) —
            // и пока замер compute-буфера не сделан, значение — None.
            assert!(e.model_args.is_none(), "{}: qwen-ключ не посылается", name);
            assert!(
                e.compute_buffers_mb.is_none(),
                "{}: compute_buffers задаётся только замером",
                name
            );
            // Qwen-бандлы держат свой ключ в каталоге (SSOT спавна).
            let unsloth = find("qwen-image-2.1-turbo-unsloth-q5_ks");
            assert_eq!(
                unsloth.model_args.as_deref(),
                Some("qwen_image_2_1_prefix_cache_type=q8_0"),
                "qwen-бандл обязан сохранить прежний ключ"
            );
        }
    }
}

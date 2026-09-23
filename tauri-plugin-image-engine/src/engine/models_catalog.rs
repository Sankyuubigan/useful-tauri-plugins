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
        assert!(!catalog.is_empty(), "каталог бандлов пуст");
        let def = default_bundle_entry().expect("нет бандла по умолчанию");
        assert_eq!(def.files.len(), 4, "бандл = 4 файла, got {}", def.files.len());
        let roles: Vec<&str> = def.files.iter().map(|f| f.role.as_str()).collect();
        for r in ["diffusion", "vae", "llm", "mmproj"] {
            assert!(roles.contains(&r), "нет роли {}", r);
        }
        assert!((def.preset.cfg_scale - 6.0).abs() < f32::EPSILON);
        assert_eq!(def.preset.sampler, "euler");
        assert_eq!(def.preset.width % 32, 0);
        assert_eq!(def.preset.height % 32, 0);
    }

    #[test]
    fn bundle_disk_bytes_sums_sizes() {
        let def = default_bundle_entry().expect("бандл");
        let total = bundle_disk_bytes(&def);
        assert!(total > 12_000_000_000, "ожидалось >12 ГБ, got {}", total);
    }
}

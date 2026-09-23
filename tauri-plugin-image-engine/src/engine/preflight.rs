//! VRAM/RAM-preflight для бандла изображений.
//!
//! Три порога (считаются статически, без запуска):
//! - full-resident: всё в VRAM (макс. скорость) — сумма весов + буферы + запас;
//! - offload-минимум: diffusion + compute-буферы + запас (TE/VAE стейджатся из RAM);
//! - RAM-минимум: веса должны влезть в оперативку (params живут в RAM при offload).
//!
//! Вердикт: Fast → молча быстро; Offload → стейджинг + честная запись в лог;
//! Insufficient → понятная ошибка, а не чёрный квадрат.

use serde::Serialize;

use crate::engine::models_catalog::{bundle_disk_bytes, ImageBundleEntry};

/// Compute-буферы diffusion + запас (МБ). Уточняется замером на железе.
pub const COMPUTE_BUFFERS_MB: f64 = 2048.0;
/// Запас поверх расчёта (МБ).
pub const SAFETY_RESERVE_MB: f64 = 1024.0;

#[derive(Serialize, Clone, Debug)]
pub struct MemoryEstimate {
    /// Суммарный размер файлов бандла на диске, МБ.
    pub disk_mb: f64,
    /// Full-resident: всё + буферы + запас, МБ.
    pub full_mb: f64,
    /// Offload-минимум: diffusion + буферы + запас, МБ.
    pub min_mb: f64,
    /// RAM-минимум: веса + запас, МБ.
    pub ram_need_mb: f64,
    /// Свободно VRAM сейчас (NVML), МБ. 0 — NVML недоступен.
    pub vram_free_mb: f64,
    /// Всего VRAM (NVML), МБ. 0 — NVML недоступен.
    pub vram_total_mb: f64,
    /// Свободно RAM сейчас, МБ.
    pub ram_free_mb: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreflightVerdict {
    Fast,
    Offload,
    Insufficient,
}

/// Размер diffusion-файла бандла (для offload-минимума).
fn diffusion_bytes(entry: &ImageBundleEntry) -> u64 {
    entry
        .files
        .iter()
        .find(|f| f.role == "diffusion")
        .and_then(|f| f.size_bytes)
        .unwrap_or_else(|| bundle_disk_bytes(entry))
}

/// Статическая оценка по бандлу + факты NVML/sysinfo.
pub fn estimate_image_memory_mb(entry: &ImageBundleEntry) -> MemoryEstimate {
    const MIB: f64 = 1024.0 * 1024.0;
    let disk_mb = bundle_disk_bytes(entry) as f64 / MIB;
    let full_mb = disk_mb + COMPUTE_BUFFERS_MB + SAFETY_RESERVE_MB;
    let min_mb = diffusion_bytes(entry) as f64 / MIB + COMPUTE_BUFFERS_MB + SAFETY_RESERVE_MB;
    let ram_need_mb = disk_mb + SAFETY_RESERVE_MB;

    let (vram_free_mb, vram_total_mb) = match nvml_wrapper::Nvml::init() {
        Ok(nvml) => match nvml.device_by_index(0).and_then(|d| d.memory_info()) {
            Ok(mem) => (
                mem.free as f64 / MIB,
                mem.total as f64 / MIB,
            ),
            Err(_) => (0.0, 0.0),
        },
        Err(_) => (0.0, 0.0),
    };

    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let ram_free_mb = sys.available_memory() as f64 / MIB;

    MemoryEstimate {
        disk_mb,
        full_mb,
        min_mb,
        ram_need_mb,
        vram_free_mb,
        vram_total_mb,
        ram_free_mb,
    }
}

/// Вердикт preflight: влез full → Fast; влез минимум → Offload; иначе Insufficient.
/// Без NVML (CPU-машина / не-NVIDIA) — Offload, если хватает RAM.
pub fn preflight_check(entry: &ImageBundleEntry) -> (PreflightVerdict, MemoryEstimate, String) {
    let est = estimate_image_memory_mb(entry);
    if est.vram_total_mb <= 0.0 {
        // NVML недоступен: полагаемся на RAM + CPU-режим.
        if est.ram_free_mb >= est.ram_need_mb {
            return (
                PreflightVerdict::Offload,
                est,
                "NVML недоступен (нет NVIDIA GPU) — режим CPU/стейджинг из RAM.".to_string(),
            );
        }
        return (
            PreflightVerdict::Insufficient,
            est.clone(),
            format!(
                "Недостаточно RAM: нужно ~{:.1} ГБ для весов бандла, свободно {:.1} ГБ.",
                est.ram_need_mb / 1024.0,
                est.ram_free_mb / 1024.0
            ),
        );
    }
    if est.vram_free_mb >= est.full_mb {
        (
            PreflightVerdict::Fast,
            est,
            "Бандл влезает в VRAM целиком — максимальная скорость.".to_string(),
        )
    } else if est.vram_free_mb >= est.min_mb && est.ram_free_mb >= est.ram_need_mb {
        (
            PreflightVerdict::Offload,
            est.clone(),
            format!(
                "Full-resident не влез (~{:.1} ГБ) — включён стейджинг из RAM (offload).",
                est.full_mb / 1024.0
            ),
        )
    } else {
        (
            PreflightVerdict::Insufficient,
            est.clone(),
            format!(
                "Недостаточно памяти: минимум ~{:.1} ГБ VRAM + ~{:.1} ГБ RAM. Свободно: {:.1} ГБ VRAM, {:.1} ГБ RAM.",
                est.min_mb / 1024.0,
                est.ram_need_mb / 1024.0,
                est.vram_free_mb / 1024.0,
                est.ram_free_mb / 1024.0
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::models_catalog::load_image_catalog;

    #[test]
    fn estimate_matches_catalog_thresholds() {
        let catalog = load_image_catalog();
        let def = catalog.iter().find(|e| e.is_default).expect("бандл");
        let est = estimate_image_memory_mb(def);
        // Диск ~12.9 ГБ
        assert!(est.disk_mb > 12_000.0 && est.disk_mb < 14_000.0, "disk {}", est.disk_mb);
        // Full ~ диск + 3 ГБ; min ~ diffusion(5.6ГБ) + 3 ГБ
        assert!(est.full_mb > est.min_mb);
        assert!(est.min_mb > 7000.0 && est.min_mb < 11000.0, "min {}", est.min_mb);
        // Пороги каталога консистентны с расчётом: fast 16ГБ покрывает full, min 8ГБ ~ min_mb
        let fast_mb = def.vram_fast_gb.unwrap_or(0.0) as f64 * 1024.0;
        let min_cat_mb = def.vram_min_gb.unwrap_or(0.0) as f64 * 1024.0;
        assert!(fast_mb >= est.full_mb, "fast {} < full {}", fast_mb, est.full_mb);
        assert!((min_cat_mb - est.min_mb).abs() < 1500.0, "min {} vs {}", min_cat_mb, est.min_mb);
    }
}

//! Размеры референсного изображения БЕЗ внешних крейтов (PNG/JPEG/WebP).
//!
//! Зачем: при редактировании canvas должен повторять пропорции `<image 1>`
//! (первой референсной картинки). Официальный ComfyUI-шаблон Qwen-Image-2.1:
//! «custom_size off: canvas comes from the encode latent (image_1)», а
//! «Keep it close to the resized image_1 size, or the edit can shift».
//! Квадрат 1024x1024 у портретного/панорамного референса ломает композицию
//! и тратит вычисления впустую.

use std::path::Path;

/// Максимальная сторона (нативное 2K у Qwen-Image-2.1).
const MAX_SIDE: u32 = 2048;
/// Минимальная сторона — слишком мелко модель не тянет.
const MIN_SIDE: u32 = 512;
/// Все стороны кратны 32 (требование sd.cpp для Qwen Image 2.1).
const ALIGN: u32 = 32;

/// Размер (ширина, высота) изображения из первых байт файла.
/// `None` — формат не распознан (тогда callers берут пресет каталога).
pub fn dimensions_from_bytes(bytes: &[u8]) -> Option<(u32, u32)> {
    png(bytes).or_else(|| jpeg(bytes)).or_else(|| webp(bytes))
}

/// Размер изображения на диске.
pub fn dimensions_from_path(path: &Path) -> Option<(u32, u32)> {
    let bytes = std::fs::read(path).ok()?;
    dimensions_from_bytes(&bytes)
}

/// Canvas под референс: та же площадь, что в пресете каталога, но пропорции
/// референса и стороны кратны 32. Референс 16:9 на пресете 1024x1024 → 1820x1024.
pub fn canvas_for_ref(preset: (u32, u32), reference: (u32, u32)) -> (u32, u32) {
    let (preset_w, preset_h) = preset;
    let (ref_w, ref_h) = reference;
    if preset_w == 0 || preset_h == 0 || ref_w == 0 || ref_h == 0 {
        return (preset_w.max(ALIGN), preset_h.max(ALIGN));
    }
    let area = f64::from(preset_w) * f64::from(preset_h);
    let scale = (area / (f64::from(ref_w) * f64::from(ref_h))).sqrt();
    let snap = |value: f64| -> u32 {
        let rounded = (value.round() as u64 / u64::from(ALIGN) * u64::from(ALIGN)) as u32;
        rounded.clamp(MIN_SIDE, MAX_SIDE)
    };
    (
        snap(f64::from(ref_w) * scale),
        snap(f64::from(ref_h) * scale),
    )
}

fn png(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let read = |offset: usize| -> u32 {
        u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ])
    };
    Some((read(16), read(20)))
}

/// `true` для SOFn-маркеров, несущих размер кадра (DHT/DAC/RSTn/SOI/EOI — нет).
fn is_sof(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
}

fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[..2] != [0xFF, 0xD8] {
        return None;
    }
    let mut index = 2usize;
    while index + 4 <= bytes.len() {
        if bytes[index] != 0xFF {
            return None;
        }
        let marker = bytes[index + 1];
        // Заполняющие 0xFF и маркеры без длины (SOI/EOI/RSTn/TEM).
        if marker == 0xFF || marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            index += 2;
            continue;
        }
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;
        if is_sof(marker) && index + 9 <= bytes.len() {
            let height = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]);
            let width = u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]);
            return Some((u32::from(width), u32::from(height)));
        }
        index += 2 + length;
    }
    None
}

fn webp(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }
    let le = |offset: usize| -> u32 {
        u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ])
    };
    match &bytes[12..16] {
        b"VP8 " => {
            if bytes.len() < 30 {
                return None;
            }
            let width = (u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3FFF) as u32;
            let height = (u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3FFF) as u32;
            Some((width, height))
        }
        b"VP8L" => {
            if bytes.len() < 25 || bytes[20] != 0x2F {
                return None;
            }
            let bits = le(21);
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        b"VP8X" => {
            if bytes.len() < 30 {
                return None;
            }
            let read24 = |offset: usize| -> u32 {
                u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], 0])
            };
            Some((read24(24) + 1, read24(27) + 1))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_png_ihdr() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&800u32.to_be_bytes());
        bytes.extend_from_slice(&1200u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert_eq!(dimensions_from_bytes(&bytes), Some((800, 1200)));
    }

    #[test]
    fn reads_jpeg_sof0() {
        // SOI + APP0 (length 16 → сегмент занимает 18 байт) + SOF0: 600x400.
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        bytes.extend_from_slice(b"JFIF\0");
        bytes.extend_from_slice(&[0x01, 0x02, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&600u16.to_be_bytes());
        bytes.extend_from_slice(&400u16.to_be_bytes());
        bytes.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        assert_eq!(dimensions_from_bytes(&bytes), Some((400, 600)));
    }

    #[test]
    fn reads_webp_lossy() {
        // RIFF/WEBP/VP8 : chunk size, 3-байтовый frame tag, старт-код 9D 01 2A,
        // затем ширина и высота (14 бит + 2 бита масштаба).
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&22u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBPVP8 ");
        bytes.extend_from_slice(&10u32.to_le_bytes());
        bytes.extend_from_slice(&[0x30, 0x01, 0x00]);
        bytes.extend_from_slice(&[0x9D, 0x01, 0x2A]);
        bytes.extend_from_slice(&(190u16 | 0x8000).to_le_bytes());
        bytes.extend_from_slice(&(30u16 | 0x8000).to_le_bytes());
        assert_eq!(dimensions_from_bytes(&bytes), Some((190, 30)));
    }

    #[test]
    fn canvas_follows_reference_aspect_and_stays_multiple_of_32() {
        // Портрет 2:3 на пресете 1024x1024 → площадь та же, стороны кратны 32.
        let (w, h) = canvas_for_ref((1024, 1024), (800, 1200));
        assert_eq!(w % 32, 0);
        assert_eq!(h % 32, 0);
        assert!(h > w, "портретный референс должен дать портретный canvas");
        let preset_area = 1024.0 * 1024.0;
        let area = f64::from(w) * f64::from(h);
        assert!((area - preset_area).abs() / preset_area < 0.05, "площадь {}", area);
    }

    #[test]
    fn canvas_clamps_panorama_to_2k() {
        let (w, h) = canvas_for_ref((1024, 1024), (4000, 200));
        assert!(w <= MAX_SIDE && h <= MAX_SIDE);
        assert!(w >= MIN_SIDE && h >= MIN_SIDE);
    }

    #[test]
    fn canvas_falls_back_to_preset_without_reference() {
        assert_eq!(canvas_for_ref((1024, 1024), (0, 0)), (1024, 1024));
    }
}

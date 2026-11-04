//! 🧩 RefMod-карточки персонажей — SSOT формата (tasks/261104_0230-db2cb59c).
//!
//! Единая лёгкая архитектура БЕЗ лор (решение владельца 04.11.2026): персонаж —
//! и собранный из фото юзера, и взятый готовыми PNG из `malcolmrey/samples` —
//! хранится как **RefMod-карточка**: папка `meta.json` + эталонные кадры.
//! При генерации кадры уходят в штатные `ref_images` движка (Ref2VA), который
//! сам кодирует их H3 Video VAE; при рефах из карточки в payload добавляется
//! `ref_image_args="ref_image_size=max"` (рекомендация специалиста для identity).
//!
//! Габариты кадров жёстко нормируются ПРИ СОХРАНЕНИИ: длинная сторона ≤1024px,
//! обе стороны кратны 32px (сетка латентов H3). Без даунскейла 4K-фото с
//! `ref_image_size=max` (лимит движка до 2048px по короткой стороне) кладёт
//! DiT/VAE в OOM/своп. Файлы `*_refmod.safetensors` (латент) официальным
//! бинарником не потребляются.
//!
//! Этот модуль — единственная реализация формата: его используют и тулы хоста
//! (создание/редактирование карточек агентом), и команды плашки «Рефмодс»
//! (список карточек + миниатюры). Дублировать обход папок в другом крейте
//! запрещено (SSOT).
//!
//! Расположение (SSOT корня данных — `config::data_root`):
//! `<KingOrchData>/refmods/<slug>/`:
//! ```text
//! meta.json            {"_format_version":1,"name":...,"description":...}
//! frames/frame_NN.png  ... кадры, ≤8, стороны кратны 32px, длинная ≤1024px
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

/// Ошибка операций над RefMod-карточками. `Usage`/`NotFound` несут конкретную
/// честную причину в тексте (downstream не маскирует их «внутренней ошибкой»).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefModError {
    /// Некорректные аргументы или повреждённая карточка (вина вызывающего/данных).
    Usage(String),
    /// Карточки с таким slug нет.
    NotFound(String),
    /// Ошибка диска/декодера.
    Io(String),
}

impl fmt::Display for RefModError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefModError::Usage(msg) => write!(f, "{}", msg),
            RefModError::NotFound(msg) => write!(f, "{}", msg),
            RefModError::Io(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for RefModError {}

/// Кратность сторон кадра — сетка латентов H3 Video VAE.
pub const FRAME_SIZE_MULTIPLE: u32 = 32;
/// Максимум длинной стороны кадра при сохранении в карточку.
pub const FRAME_LONG_SIDE_MAX: u32 = 1024;
/// Максимум кадров в карточке (хранение).
pub const FRAMES_MAX: usize = 8;
/// Сколько кадров уходит в движок за один прогон (структурный лимит референсов
/// ref2va-тула; карточка может хранить больше — берутся первые).
pub const FRAMES_PER_RUN: usize = 3;
/// Длинная сторона миниатюры карточки для плашки «Рефмодс» (ограничена кодом,
/// чтобы не тянуть мегабайтные кадры в webview).
pub const THUMBNAIL_LONG_SIDE: u32 = 256;

const REFMODS_DIR_NAME: &str = "refmods";
const META_FILE: &str = "meta.json";
const FRAMES_DIR: &str = "frames";
/// Версия формата meta.json карточки.
const META_FORMAT_VERSION: u32 = 1;

/// `meta.json` RefMod-карточки.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RefModMeta {
    /// Версия формата (резерв эволюции).
    #[serde(default = "default_meta_format_version")]
    pub _format_version: u32,
    /// Человекочитаемое имя персонажа.
    pub name: String,
    /// Короткое описание внешности для каталога графа.
    #[serde(default)]
    pub description: String,
}

fn default_meta_format_version() -> u32 {
    META_FORMAT_VERSION
}

/// Загруженная RefMod-карточка.
#[derive(Clone, Debug, PartialEq)]
pub struct RefModBundle {
    pub slug: String,
    pub name: String,
    pub description: String,
    /// Абсолютные пути кадров (frames/frame_NN.png), отсортированы по номеру.
    pub frames: Vec<PathBuf>,
}

/// Папка карточек по действующему конфигу: `<KingOrchData>/refmods`.
pub fn refmods_dir() -> PathBuf {
    super::data_root().join(REFMODS_DIR_NAME)
}

/// Валидный slug папки карточки: 1–48 символов `[a-z0-9_-]`, начинается/кончается
/// цифрой или буквой. Одновременно защита от traversal (`..`, `/`, `\` отсечены
/// алфавитом).
pub fn validate_slug(slug: &str) -> Result<(), RefModError> {
    let slug = slug.trim();
    let ok_len = !slug.is_empty() && slug.len() <= 48;
    let ok_chars = slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    let ok_edges = slug
        .chars()
        .next()
        .map(|c| c.is_ascii_alphanumeric())
        .unwrap_or(false)
        && slug
            .chars()
            .next_back()
            .map(|c| c.is_ascii_alphanumeric())
            .unwrap_or(false);
    if ok_len && ok_chars && ok_edges {
        return Ok(());
    }
    Err(RefModError::Usage(format!(
        "refmod: некорректный slug карточки '{}'. Ожидается 1–48 символов a-z, 0-9, \
         '_' или '-', начинающийся и заканчивающийся буквой/цифрой.",
        slug
    )))
}

/// Округлить до ближайшего кратного `multiple` (минимум — само кратное).
fn round_to_multiple(value: f32, multiple: u32) -> u32 {
    ((value / multiple as f32).round().max(1.0) as u32) * multiple
}

/// Нормировать кадр под габариты карточки: длинная сторона ≤1024px,
/// обе стороны кратны 32px. Изображение меньше лимита не растягивается
/// по длинной стороне (scale = 1), но выравнивается кратно 32px.
/// Публичный SSOT нормализации: используется и при сохранении, и операциями
/// редактирования хоста (warp/splice возвращают кадр на сетку этим же кодом).
pub fn normalize_frame(img: image::DynamicImage) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    let long = w.max(h);
    let scale = if long > FRAME_LONG_SIDE_MAX {
        FRAME_LONG_SIDE_MAX as f32 / long as f32
    } else {
        1.0
    };
    let target_w = round_to_multiple(w as f32 * scale, FRAME_SIZE_MULTIPLE);
    let target_h = round_to_multiple(h as f32 * scale, FRAME_SIZE_MULTIPLE);
    if target_w == w && target_h == h {
        return img;
    }
    img.resize_exact(
        target_w,
        target_h,
        image::imageops::FilterType::Lanczos3,
    )
}

/// Габариты кадра валидны для карточки: обе стороны кратны 32px.
fn frame_size_valid(w: u32, h: u32) -> bool {
    w % FRAME_SIZE_MULTIPLE == 0 && h % FRAME_SIZE_MULTIPLE == 0 && w > 0 && h > 0
}

/// Сохранить RefMod-карточку из файлов-источников (перезаписывает существующую).
/// Каждый источник декодируется, нормируется (`normalize_frame`) и кладётся в
/// `frames/frame_NN.png`. Конвенция порядка кадров (владелец 04.11.26):
/// 1 — лицо крупно, 2 — ракурс 3/4 или профиль, 3 — фигура/одежда.
pub fn save_refmod_card(
    slug: &str,
    name: &str,
    description: &str,
    sources: &[PathBuf],
) -> Result<RefModBundle, RefModError> {
    save_refmod_card_in(&refmods_dir(), slug, name, description, sources)
}

pub fn save_refmod_card_in(
    refmods_root: &Path,
    slug: &str,
    name: &str,
    description: &str,
    sources: &[PathBuf],
) -> Result<RefModBundle, RefModError> {
    // Кадры декодируются ДО всякой записи: битый источник — честный Io-отказ,
    // при котором существующая карточка не тронута.
    let mut images = Vec::with_capacity(sources.len());
    for source in sources {
        let img = image::open(source).map_err(|e| {
            RefModError::Io(format!(
                "refmod: не удалось прочитать кадр-источник {}: {}",
                source.display(),
                e
            ))
        })?;
        images.push(img);
    }
    save_refmod_card_images_in(refmods_root, slug, name, description, &images)
}

/// Сохранить карточку из кадров в памяти (операции редактирования warp/hue/splice
/// трансформируют кадры в RAM и сохраняют результат без временных файлов).
pub fn save_refmod_card_images_in(
    refmods_root: &Path,
    slug: &str,
    name: &str,
    description: &str,
    images: &[image::DynamicImage],
) -> Result<RefModBundle, RefModError> {
    validate_slug(slug)?;
    let name = name.trim();
    if name.is_empty() {
        return Err(RefModError::Usage(
            "refmod: пустое имя карточки (name)".to_string(),
        ));
    }
    if images.is_empty() {
        return Err(RefModError::Usage(
            "refmod: карточке нужен хотя бы один кадр-источник".to_string(),
        ));
    }
    if images.len() > FRAMES_MAX {
        return Err(RefModError::Usage(format!(
            "refmod: максимум {} кадров в карточке, передано {}.",
            FRAMES_MAX,
            images.len()
        )));
    }

    let card_dir = refmods_root.join(slug.trim());
    let frames_dir = card_dir.join(FRAMES_DIR);
    // Перезапись чистая: старые кадры с большим номером не должны переживать
    // сохранение меньшего набора (stale frame_05 при новом наборе из 3 — мусор).
    if frames_dir.exists() {
        std::fs::remove_dir_all(&frames_dir)
            .map_err(|e| RefModError::Io(format!("{}: {}", frames_dir.display(), e)))?;
    }
    std::fs::create_dir_all(&frames_dir)
        .map_err(|e| RefModError::Io(format!("{}: {}", frames_dir.display(), e)))?;

    let mut frames = Vec::with_capacity(images.len());
    for (index, img) in images.iter().enumerate() {
        let normalized = normalize_frame(img.clone());
        let dst = frames_dir.join(format!("frame_{:02}.png", index + 1));
        normalized.save_with_format(&dst, image::ImageFormat::Png).map_err(|e| {
            RefModError::Io(format!("refmod: не удалось сохранить кадр {}: {}", dst.display(), e))
        })?;
        frames.push(dst);
    }

    let meta = RefModMeta {
        _format_version: META_FORMAT_VERSION,
        name: name.to_string(),
        description: description.trim().to_string(),
    };
    write_meta(&card_dir, &meta)?;

    Ok(RefModBundle {
        slug: slug.trim().to_string(),
        name: meta.name,
        description: meta.description,
        frames,
    })
}

fn write_meta(card_dir: &Path, meta: &RefModMeta) -> Result<(), RefModError> {
    std::fs::create_dir_all(card_dir)
        .map_err(|e| RefModError::Io(format!("{}: {}", card_dir.display(), e)))?;
    let text = serde_json::to_string_pretty(meta)
        .map_err(|e| RefModError::Io(format!("meta.json: {}", e)))?;
    std::fs::write(card_dir.join(META_FILE), text)
        .map_err(|e| RefModError::Io(format!("{}: {}", card_dir.display(), e)))
}

/// Загрузить карточку по slug (с валидацией габаритов кадров — карта могла
/// быть отредактирована вручную; проверка header-only, без полного декода).
pub fn load_refmod_card(slug: &str) -> Result<RefModBundle, RefModError> {
    load_refmod_card_in(&refmods_dir(), slug)
}

pub fn load_refmod_card_in(refmods_root: &Path, slug: &str) -> Result<RefModBundle, RefModError> {
    validate_slug(slug)?;
    let slug = slug.trim();
    let card_dir = refmods_root.join(slug);
    let meta_path = card_dir.join(META_FILE);
    if !meta_path.is_file() {
        return Err(RefModError::NotFound(format!(
            "RefMod-карточка '{}' не найдена (нет {}). Доступные карточки — в блоке \
             «Доступные RefMod-карточки» контекста.",
            slug,
            meta_path.display()
        )));
    }
    let text = std::fs::read_to_string(&meta_path)
        .map_err(|e| RefModError::Io(format!("{}: {}", meta_path.display(), e)))?;
    let meta: RefModMeta = serde_json::from_str(&text).map_err(|e| {
        RefModError::Usage(format!("{} повреждён: {}", meta_path.display(), e))
    })?;

    let frames_dir = card_dir.join(FRAMES_DIR);
    let mut frames: Vec<PathBuf> = Vec::new();
    let entries = std::fs::read_dir(&frames_dir)
        .map_err(|e| RefModError::Io(format!("{}: {}", frames_dir.display(), e)))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("png")) == Some(true)
        {
            frames.push(path);
        }
    }
    frames.sort();
    if frames.is_empty() {
        return Err(RefModError::Usage(format!(
            "RefMod-карточка '{}' пуста: в {} нет кадров.",
            slug,
            frames_dir.display()
        )));
    }
    if frames.len() > FRAMES_MAX {
        return Err(RefModError::Usage(format!(
            "RefMod-карточка '{}' повреждена: {} кадров, лимит {}.",
            slug,
            frames.len(),
            FRAMES_MAX
        )));
    }
    for path in &frames {
        let dims = image::image_dimensions(path).map_err(|e| {
            RefModError::Usage(format!(
                "RefMod-карточка '{}': кадр {} не читается как PNG: {}",
                slug,
                path.display(),
                e
            ))
        })?;
        if !frame_size_valid(dims.0, dims.1) {
            return Err(RefModError::Usage(format!(
                "RefMod-карточка '{}': кадр {} имеет размер {}x{} — обе стороны \
                 должны быть кратны {}px.",
                slug,
                path.display(),
                dims.0,
                dims.1,
                FRAME_SIZE_MULTIPLE
            )));
        }
    }

    Ok(RefModBundle {
        slug: slug.to_string(),
        name: meta.name,
        description: meta.description,
        frames,
    })
}

/// Список карточек (битые карточки пропускаем с warn-логом — одна повреждённая
/// папка не должна ломать каталог графа и плашку).
pub fn list_refmod_cards() -> Vec<RefModBundle> {
    list_refmod_cards_in(&refmods_dir())
}

pub fn list_refmod_cards_in(refmods_root: &Path) -> Vec<RefModBundle> {
    let mut cards = Vec::new();
    let Ok(entries) = std::fs::read_dir(refmods_root) else {
        return cards;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(slug) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        match load_refmod_card_in(refmods_root, slug) {
            Ok(bundle) => cards.push(bundle),
            Err(e) => log::warn!("[REFMOD] карточка {} пропущена: {}", slug, e),
        }
    }
    cards.sort_by(|a, b| a.slug.cmp(&b.slug));
    cards
}

/// Кадры карточки для одного прогона движка: первые `FRAMES_PER_RUN`
/// (владелец 04.11.26: карточка с 4–8 кадрами НЕ ошибка — берём первые три
/// в конвенции лицо → ракурс 3/4 → фигура).
pub fn engine_ref_paths(bundle: &RefModBundle) -> Vec<String> {
    bundle
        .frames
        .iter()
        .take(FRAMES_PER_RUN)
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

/// Блок «Доступные RefMod-карточки» для графа (`{{ video_refmods }}`).
pub fn refmods_prompt() -> String {
    refmods_prompt_in(&refmods_dir())
}

pub fn refmods_prompt_in(refmods_root: &Path) -> String {
    let cards = list_refmod_cards_in(refmods_root);
    if cards.is_empty() {
        return "(RefMod-карточек персонажей нет — аргумент refmod не передавай)".to_string();
    }
    let mut out = String::new();
    for card in &cards {
        out.push_str(&format!("- {}", card.slug));
        if !card.name.is_empty() {
            out.push_str(&format!(" — {}", card.name));
        }
        if !card.description.is_empty() {
            out.push_str(&format!(" — {}", card.description));
        }
        out.push_str(&format!(" (кадров: {})", card.frames.len()));
        out.push('\n');
    }
    out
}

/// Миниатюра карточки для плашки «Рефмодс»: первый кадр (конвенция — лицо),
/// декод, resize до `THUMBNAIL_LONG_SIDE` по длинной стороне, JPEG → data URL.
/// Возвращает `data:image/jpeg;base64,...` — компактнее PNG в десятки раз на фото.
pub fn thumbnail_data_url(slug: &str) -> Result<String, RefModError> {
    thumbnail_data_url_in(&refmods_dir(), slug)
}

pub fn thumbnail_data_url_in(refmods_root: &Path, slug: &str) -> Result<String, RefModError> {
    let bundle = load_refmod_card_in(refmods_root, slug)?;
    let frame = bundle
        .frames
        .first()
        .ok_or_else(|| RefModError::Usage(format!("RefMod-карточка '{}' пуста", slug)))?;
    let img = image::open(frame).map_err(|e| {
        RefModError::Io(format!(
            "refmod: не удалось декодировать миниатюру {}: {}",
            frame.display(),
            e
        ))
    })?;
    let thumb = if img.width().max(img.height()) > THUMBNAIL_LONG_SIDE {
        img.resize(
            THUMBNAIL_LONG_SIDE,
            THUMBNAIL_LONG_SIDE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };
    let mut jpeg: Vec<u8> = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85);
    encoder
        .encode_image(&thumb)
        .map_err(|e| RefModError::Io(format!("refmod: JPEG-кодирование миниатюры: {}", e)))?;
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(jpeg);
    Ok(format!("data:image/jpeg;base64,{}", b64))
}

#[cfg(test)]
#[path = "refmod_tests.rs"]
mod refmod_tests;

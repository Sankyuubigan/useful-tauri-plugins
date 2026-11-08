//! 🧩 RefMod-карточки персонажей — SSOT формата (tasks/261104_0230-db2cb59c).
//!
//! Единая лёгкая архитектура БЕЗ лор (решение владельца 04.11.2026): персонаж —
//! и собранный из фото юзера, и взятый готовыми PNG из `malcolmrey/samples` —
//! хранится как **RefMod-карточка**: папка `meta.json` + эталонные кадры.
//! При генерации кадры уходят в штатные `ref_images` движка (Ref2VA), который
//! сам кодирует их H3 Video VAE; при рефах из карточки в payload добавляется
//! `ref_image_args="ref_image_size=max"` (рекомендация специалиста для identity).
//!
//! Габариты кадров нормируются ПРИ СОХРАНЕНИИ по бюджету площади (08.11.26,
//! решение владельца — форма подачи как у блоггера): суммарно ≤4.0 Мпикс на
//! карточку (CARD_PX_BUDGET_TOTAL), главный кадр ≤2 Мпикс (CARD_PX_BUDGET_MAIN),
//! обе стороны кратны 32px (сетка латентов H3). Без бюджетной нормализации
//! 4K-фото с `ref_image_size=max` (лимит движка до 2048px по короткой стороне)
//! кладёт DiT/VAE в OOM/своп. Файлы `*_refmod.safetensors` (латент) официальным
//! бинарником не потребляются.
//!
//! Голос (Этап 3, 05.11.2026): карточка опционально несёт РОВНО ОДИН голос —
//! файл `voice.wav` (PCM16, исходные rate/channels; конверт в stereo 32 kHz
//! делает движок H3 сам) + человекочитаемое `voice_desc` в meta.json. Правки
//! голоса — IN-PLACE в карточке (смена не плодит дубли персонажа в каталоге);
//! транскод — `engine/refmod_voice.rs`. 2+ ref_audios H3 путает, поэтому
//! второй голос в карточку не попадает конструктивно.
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
//! frames/frame_NN.png  ... кадры, ≤8, стороны кратны 32px, Σ ≤4 Мпикс
//! voice.wav            голос (опционально, PCM16; ставится только voice-фн,
//!                      правки IN-PLACE; desc — voice_desc в meta.json)
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

use super::refmod_voice::{normalize_voice, VoiceWav, VOICE_WAV_FILE};

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
/// Бюджет формы подачи карточки (08.11.26, решение владельца: «бюджет должен
/// быть такой же» как у блога с двумя листами): суммарные пиксели всех кадров
/// = 4.0 Мпикс — потолок композера чужого пайплайна (2 листа × 2 Мпикс).
/// Прежнее равномерное ужатие каждого кадра до длинной ≤1024 снято: оно
/// одинаково резало все кадры, теряя детальность главного.
pub const CARD_PX_BUDGET_TOTAL: u64 = 4_000_000;
/// Потолок главного кадра (frame_01) — как главный лист блоггера (2 Мпикс).
pub const CARD_PX_BUDGET_MAIN: u64 = 2_000_000;
/// Максимум кадров в карточке (хранение).
pub const FRAMES_MAX: usize = 8;
/// Сколько кадров уходит в движок за один прогон. Равен потолку формата
/// карточки (05.11.26, решение владельца: ВСЕ кадры карточки — референсы,
/// «мёртвых» кадров не существует; раньше брали первые 3). Бюджет формы
/// подачи задаёт CARD_PX_BUDGET_TOTAL (08.11.26, решение владельца).
pub const FRAMES_PER_RUN: usize = FRAMES_MAX;
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
    /// Имя файла голоса внутри карточки (ровно один — `voice.wav`).
    /// None = карточка без голоса (старые карточки читаются как есть).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_file: Option<String>,
    /// Человекочитаемое описание тембра для промпта видео-агента.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_desc: Option<String>,
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
    /// Голос карточки (voice.wav), если задан. Правки — IN-PLACE (voice-фн).
    pub voice: Option<PathBuf>,
    /// Описание тембра (из meta.json; для промпта видео-агента).
    pub voice_desc: Option<String>,
}

/// Один кадр, чьи габариты изменились при СОХРАНЕНИИ карточки (было → стало).
#[derive(Debug, Clone, PartialEq)]
pub struct FrameResize {
    /// Имя файла кадра (frame_NN.png) — уже сохранённого на диске.
    pub frame_file: String,
    pub src_w: u32,
    pub src_h: u32,
    pub dst_w: u32,
    pub dst_h: u32,
}

/// Отчёт о нормализации кадров при сохранении (SSOT — точки
/// `normalize_frames_budget`/`normalize_frame`, где известны оба размера).
/// Пустой отчёт = ни один кадр не менял габариты (ни ужатия по бюджету
/// площади, ни выравнивания к сетке 32).
/// Загружаемые карточки (load/list) отчёта не имеют по определению.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NormalizeReport {
    pub resized: Vec<FrameResize>,
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

/// Привести кадр к сетке 32px с потолком площади `max_area` (форма подачи —
/// бюджет площади, не длинной стороны). Две ветки:
/// 1) площадь сверх потолка — ужатие scale = sqrt(max_area/area) с сеткой
///    ЧЕРЕЗ FLOOR: обе стороны не превышают масштабных, поэтому итоговая
///    площадь гарантированно ≤ max_area (апскейла нет);
/// 2) кадр в бюджете — выравнивание к БЛИЖАЙШЕЙ кратности 32 (round, прежнее
///    поведение: невыровненный кадр может растянуться на шаг сетки).
fn normalize_to_area_cap(img: image::DynamicImage, max_area: u64) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    let area = w as u64 * h as u64;
    let (target_w, target_h) = if area > max_area {
        let s = (max_area as f64 / area as f64).sqrt();
        let floor_grid = |v: f64| -> u32 {
            ((v / FRAME_SIZE_MULTIPLE as f64).floor().max(1.0) as u32) * FRAME_SIZE_MULTIPLE
        };
        (floor_grid(w as f64 * s), floor_grid(h as f64 * s))
    } else {
        (
            round_to_multiple(w as f32, FRAME_SIZE_MULTIPLE),
            round_to_multiple(h as f32, FRAME_SIZE_MULTIPLE),
        )
    };
    if target_w == w && target_h == h {
        return img;
    }
    img.resize_exact(
        target_w,
        target_h,
        image::imageops::FilterType::Lanczos3,
    )
}

/// Нормировать одиночный кадр под форму карточки: потолок площади — кап
/// главного кадра (CARD_PX_BUDGET_MAIN), сетка 32px. Публичный SSOT
/// нормализации: используется и при сохранении, и операциями редактирования
/// хоста (warp/splice возвращают кадр на сетку этим же кодом).
pub fn normalize_frame(img: image::DynamicImage) -> image::DynamicImage {
    normalize_to_area_cap(img, CARD_PX_BUDGET_MAIN)
}

/// Бюджетная форма подачи при сохранении карточки (08.11.26, решение
/// владельца: «сжимать не каждый кадр до 1024, а по бюджету методом
/// блоггера»). Главный кадр — первый (агент кладёт якорь-лицо первым,
/// конвенция карточки):
/// 1) суммарная площадь источников ≤ CARD_PX_BUDGET_TOTAL — никто не ужат
///    (только сетка 32);
/// 2) иначе главный сохраняет максимум детальности: кап CARD_PX_BUDGET_MAIN
///    (как главный лист блоггера), часто без ужатия вовсе;
/// 3) остальные делят остаток бюджета (CARD_PX_BUDGET_TOTAL − главный)
///    пропорционально исходным площадям; floor-сетка ветки ужатия даёт
///    гарантию «не больше своей доли».
/// Погрешность бюджета — до одного шага сетки 32px на кадр в меньшую сторону
/// (floor только вниз; перелив суммарного бюджета невозможен).
pub fn normalize_frames_budget(images: &[image::DynamicImage]) -> Vec<image::DynamicImage> {
    let areas: Vec<u64> = images
        .iter()
        .map(|i| i.width() as u64 * i.height() as u64)
        .collect();
    let total: u64 = areas.iter().sum();
    if total <= CARD_PX_BUDGET_TOTAL {
        return images
            .iter()
            .map(|i| normalize_to_area_cap(i.clone(), CARD_PX_BUDGET_MAIN))
            .collect();
    }
    let rest_budget = CARD_PX_BUDGET_TOTAL - areas[0].min(CARD_PX_BUDGET_MAIN);
    let rest_src: u64 = areas[1..].iter().sum();
    images
        .iter()
        .enumerate()
        .map(|(k, img)| {
            let cap = if k == 0 || rest_src == 0 {
                CARD_PX_BUDGET_MAIN
            } else {
                rest_budget * areas[k] / rest_src
            };
            normalize_to_area_cap(img.clone(), cap)
        })
        .collect()
}

/// Габариты кадра валидны для карточки: обе стороны кратны 32px.
fn frame_size_valid(w: u32, h: u32) -> bool {
    w % FRAME_SIZE_MULTIPLE == 0 && h % FRAME_SIZE_MULTIPLE == 0 && w > 0 && h > 0
}

/// Сохранить RefMod-карточку из файлов-источников (перезаписывает существующую).
/// Каждый источник декодируется, нормируется (`normalize_frame`) и кладётся в
/// `frames/frame_NN.png`. Порядок кадров = лексикографический порядок имён
/// файлов в `frames/` (движок видео получает их как Picture 1..N). Смысловая
/// расстановка («первым — лицо крупно») — мягкая рекомендация агенту через
/// описание тула, движок её не проверяет.
pub fn save_refmod_card(
    slug: &str,
    name: &str,
    description: &str,
    sources: &[PathBuf],
) -> Result<(RefModBundle, NormalizeReport), RefModError> {
    save_refmod_card_in(&refmods_dir(), slug, name, description, sources)
}

pub fn save_refmod_card_in(
    refmods_root: &Path,
    slug: &str,
    name: &str,
    description: &str,
    sources: &[PathBuf],
) -> Result<(RefModBundle, NormalizeReport), RefModError> {
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
) -> Result<(RefModBundle, NormalizeReport), RefModError> {
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
    // Наследование голоса при перезаписи (edit с ops + overwrite): перезапись
    // меняет кадры, но НЕ тембр — тот же персонаж того же slug. Старый voice.wav
    // не удаляется (wipe чистит только frames/) и остаётся объявленным в meta.
    let inherited_voice = read_meta(&card_dir).ok().and_then(|old| {
        let file = old.voice_file?;
        if !card_dir.join(&file).is_file() {
            return None;
        }
        Some((file, old.voice_desc))
    });
    // Перезапись чистая: старые кадры с большим номером не должны переживать
    // сохранение меньшего набора (stale frame_05 при новом наборе из 3 — мусор).
    if frames_dir.exists() {
        std::fs::remove_dir_all(&frames_dir)
            .map_err(|e| RefModError::Io(format!("{}: {}", frames_dir.display(), e)))?;
    }
    std::fs::create_dir_all(&frames_dir)
        .map_err(|e| RefModError::Io(format!("{}: {}", frames_dir.display(), e)))?;

    let mut frames = Vec::with_capacity(images.len());
    let mut report = NormalizeReport::default();
    let budgeted = normalize_frames_budget(images);
    for (index, (img, normalized)) in images.iter().zip(budgeted.iter()).enumerate() {
        let (src_w, src_h) = (img.width(), img.height());
        let dst = frames_dir.join(format!("frame_{:02}.png", index + 1));
        normalized.save_with_format(&dst, image::ImageFormat::Png).map_err(|e| {
            RefModError::Io(format!("refmod: не удалось сохранить кадр {}: {}", dst.display(), e))
        })?;
        if (normalized.width(), normalized.height()) != (src_w, src_h) {
            report.resized.push(FrameResize {
                frame_file: dst.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                src_w,
                src_h,
                dst_w: normalized.width(),
                dst_h: normalized.height(),
            });
            log::info!(
                "[REFMOD] кадр {} изменён нормализацией: {}x{} → {}x{}",
                dst.file_name().unwrap_or_default().to_string_lossy(),
                src_w,
                src_h,
                normalized.width(),
                normalized.height()
            );
        }
        frames.push(dst);
    }

    let meta = RefModMeta {
        _format_version: META_FORMAT_VERSION,
        name: name.to_string(),
        description: description.trim().to_string(),
        // Новая карточка — без голоса; перезапись существующей — голос
        // наследуется (voice_file/voice_desc старой meta). Новый голос ставится
        // ТОЛЬКО voice-фн (set_card_voice_in) — единая точка мутации.
        voice_file: inherited_voice.as_ref().map(|(f, _)| f.clone()),
        voice_desc: inherited_voice.as_ref().and_then(|(_, d)| d.clone()),
    };
    write_meta(&card_dir, &meta)?;

    Ok((
        RefModBundle {
            slug: slug.trim().to_string(),
            name: meta.name,
            description: meta.description,
            frames,
            // Голос: унаследованный при перезаписи, иначе нет. Новый голос —
            // только через voice-фн (set_card_voice_in и др.) — единая точка мутации.
            voice: inherited_voice
                .as_ref()
                .map(|(f, _)| card_dir.join(f)),
            voice_desc: inherited_voice.as_ref().and_then(|(_, d)| d.clone()),
        },
        report,
    ))
}

fn write_meta(card_dir: &Path, meta: &RefModMeta) -> Result<(), RefModError> {
    std::fs::create_dir_all(card_dir)
        .map_err(|e| RefModError::Io(format!("{}: {}", card_dir.display(), e)))?;
    let text = serde_json::to_string_pretty(meta)
        .map_err(|e| RefModError::Io(format!("meta.json: {}", e)))?;
    std::fs::write(card_dir.join(META_FILE), text)
        .map_err(|e| RefModError::Io(format!("{}: {}", card_dir.display(), e)))
}

// ─── Голос карточки (Этап 3, tasks/261104_0230-db2cb59c) ───────────────────
// Единая точка мутации голоса — voice-фн ниже. save-фн НОВЫЙ голос не ставят
// (перезапись существующей карточки НАСЛЕДУЕТ старый голос — тембр не зависит
// от кадров); ops-правка кадров в новую карточку наследует голос через
// copy_card_voice_in. Правки голоса — IN-PLACE: смена голоса не создаёт
// v2-дубль персонажа (дубли в каталоге = неоднозначность выбора версии —
// эпистемический отказ, §00.2 AGENTS.md).

/// Прочитать meta.json карточки (хелпер read-modify-write голосовых правок).
fn read_meta(card_dir: &Path) -> Result<RefModMeta, RefModError> {
    let meta_path = card_dir.join(META_FILE);
    let text = std::fs::read_to_string(&meta_path)
        .map_err(|e| RefModError::Io(format!("{}: {}", meta_path.display(), e)))?;
    serde_json::from_str(&text).map_err(|e| {
        RefModError::Usage(format!("{} повреждён: {}", meta_path.display(), e))
    })
}

/// Записать голос в карточку (voice.wav + meta read-modify-write) —
/// низкоуровневая точка записи. `desc = None` → описание сбрасывается
/// (описание тембра привязано к текущему голосу: сменили голос — старое
/// описание СТАРОГО тембра молча оставлять нельзя, это ложь в каталоге).
pub fn write_card_voice_in(
    refmods_root: &Path,
    slug: &str,
    voice: &VoiceWav,
    desc: Option<String>,
) -> Result<RefModBundle, RefModError> {
    validate_slug(slug)?;
    let slug = slug.trim();
    let card_dir = refmods_root.join(slug);
    if !card_dir.join(META_FILE).is_file() {
        return Err(RefModError::NotFound(format!(
            "RefMod-карточка '{}' не найдена — голос задавать некуда. Доступные \
             карточки — в блоке «Доступные RefMod-карточки» контекста.",
            slug
        )));
    }
    let wav_path = card_dir.join(VOICE_WAV_FILE);
    std::fs::write(&wav_path, voice.to_wav_bytes()).map_err(|e| {
        RefModError::Io(format!("{}: {}", wav_path.display(), e))
    })?;
    let mut meta = read_meta(&card_dir)?;
    meta.voice_file = Some(VOICE_WAV_FILE.to_string());
    meta.voice_desc = desc;
    write_meta(&card_dir, &meta)?;
    load_refmod_card_in(refmods_root, slug)
}

/// Задать/сменить голос карточки: декод аудио-байтов юзера (ДО всякой записи
/// на диск), затем запись. Промежуточная между `set_card_voice` и диском —
/// для тестов и хоста с нестандартным корнем.
pub fn set_card_voice_in(
    refmods_root: &Path,
    slug: &str,
    audio: &[u8],
    desc: Option<String>,
) -> Result<RefModBundle, RefModError> {
    let voice = normalize_voice(audio)?;
    load_refmod_card_in(refmods_root, slug)?;
    write_card_voice_in(refmods_root, slug, &voice, desc)
}

/// Задать/сменить голос карточки (корень по умолчанию).
pub fn set_card_voice(
    slug: &str,
    audio: &[u8],
    desc: Option<String>,
) -> Result<RefModBundle, RefModError> {
    set_card_voice_in(&refmods_dir(), slug, audio, desc)
}

/// Правка описания СУЩЕСТВУЮЩЕГО голоса (in-place). Голоса нет → Usage:
/// «описывать нечего — сначала задай голос».
pub fn update_voice_desc_in(
    refmods_root: &Path,
    slug: &str,
    desc: String,
) -> Result<RefModBundle, RefModError> {
    let bundle = load_refmod_card_in(refmods_root, slug)?;
    if bundle.voice.is_none() {
        return Err(RefModError::Usage(format!(
            "RefMod-карточка '{}': голоса нет — описывать нечего. Сначала задай \
             голос (refmod_edit с аудио-вложением и voice_desc).",
            slug
        )));
    }
    let card_dir = refmods_root.join(slug.trim());
    let mut meta = read_meta(&card_dir)?;
    meta.voice_desc = Some(desc);
    write_meta(&card_dir, &meta)?;
    load_refmod_card_in(refmods_root, slug)
}

/// Наследовать голос при ops-правке кадров (исходник → новая карточка v2).
/// Нет голоса — Ok(()) без действий (не голос → нечего копировать).
pub fn copy_card_voice_in(
    refmods_root: &Path,
    src_slug: &str,
    dst_slug: &str,
) -> Result<(), RefModError> {
    let src = load_refmod_card_in(refmods_root, src_slug)?;
    let Some(voice_path) = src.voice else {
        return Ok(());
    };
    let bytes = std::fs::read(&voice_path)
        .map_err(|e| RefModError::Io(format!("{}: {}", voice_path.display(), e)))?;
    let voice = normalize_voice(&bytes)?;
    write_card_voice_in(refmods_root, dst_slug, &voice, src.voice_desc.clone()).map(|_| ())
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

    // Голос: meta объявляет voice_file — файл обязан существовать. Отсутствие
    // файла (ручное удаление) НЕ бричит карточку: честный warn, голос None
    // (каталог не будет обещать голоса, которого нет).
    let voice = meta.voice_file.as_ref().map(|file| card_dir.join(file));
    let voice = match voice {
        Some(path) if path.is_file() => Some(path),
        Some(path) => {
            log::warn!(
                "[REFMOD] карточка '{}': voice объявлен в meta ({}), но файла нет — \
                 голос считается не заданным (можно вернуть через refmod_edit).",
                slug,
                path.display()
            );
            None
        }
        None => None,
    };

    Ok(RefModBundle {
        slug: slug.to_string(),
        name: meta.name,
        description: meta.description,
        frames,
        voice,
        voice_desc: meta.voice_desc,
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

/// Кадры карточки для одного прогона движка: ВСЕ кадры бандла (05.11.26,
/// решение владельца: «мёртвых» кадров не существует). Потолок — формат
/// карточки `FRAMES_MAX`; конвенция лицо → 3/4 → фигура задаёт порядок
/// первых трёх (приоритет референсов), кадры 4–8 — дополнительные ракурсы.
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
        if card.voice.is_some() {
            if let Some(desc) = &card.voice_desc {
                out.push_str(&format!(" 🎤 голос: {}", desc));
            } else {
                out.push_str(" 🎤 голос задан");
            }
        }
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

#[cfg(test)]
#[path = "refmod_voice_tests.rs"]
mod refmod_voice_tests;

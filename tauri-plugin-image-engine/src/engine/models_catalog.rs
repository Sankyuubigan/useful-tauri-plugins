//! Каталог бандлов image-моделей.
//!
//! SSOT — встроенный JSON `image_models_catalog.json` (компилируется через
//! `include_str!`). Бандл = 4 файла (diffusion + vae + text encoder + mmproj),
//! качаются одной кнопкой в папку бандла. Дефолтные пресеты генерации —
//! ТОЛЬКО здесь (отдельного sampling_presets.json нет).

use serde::{Deserialize, Serialize};

use crate::engine::lora::{clamp_multiplier, LORA_MULTIPLIER_MAX};

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
    /// База модели для LoRA-папки (F3, 29.10.26): лоры разложены по
    /// подпапкам каталога `--lora-model-dir` на базу (`qwen`, `krea2`...).
    /// Лора привязана к именам/размерностям слоёв конкретного бэкбона:
    /// кросс-базное применение sd.cpp распознаёт как частичное совпадение
    /// тензоров (тихий шум), поэтому резолв ищет ТОЛЬКО в подпапке базы.
    /// None — легаси-плоский каталог корня (совместимость).
    #[serde(default)]
    pub lora_base: Option<String>,
    /// Контракт ref-edit (F4, 29.10.26): декларирует, умеет ли бандл
    /// редактирование по референсам и каким стеком.
    /// - `native` — ref-conditioning обучен в самом стеке (Qwen-Image-2.1:
    ///   сервер сам применяет свой ref-конвейер, пресет вшит в модель);
    /// - `edit_lora` — ref-conditioning работает ТОЛЬКО в паре с обученной
    ///   edit-лорой (Krea2: серверный default `krea2_ostris_edit` без лоры
    ///   даёт артефакт «checkerboard», ostris/ComfyUI-Krea2-Ostris-Edit #1);
    /// - отсутствие поля — ref-edit НЕ поддержан: edit_image отказывает
    ///   честно (fail-closed), а не поставляет мусор из untrained-патча.
    #[serde(default)]
    pub ref_edit_mode: Option<RefEditMode>,
    /// Стем обученной edit-лоры в каталоге лор (обязателен при
    /// `edit_lora`; проверяет validate_image_entry). Плагин резолвит файл
    /// по базе бандла и подключает автоматически.
    #[serde(default)]
    pub ref_edit_lora: Option<String>,
    /// Значение `preset=` для `ref_image_args` сервера (обязательно при
    /// `edit_lora`): пресет патч-conditioning обязан соответствовать лоре
    /// (identity-lorа conradlocke ↔ `krea2_edit`, docs/edit.md).
    #[serde(default)]
    pub ref_edit_preset: Option<String>,
    /// Сила подключаемой edit-лоры (default 1.0; клампится на границе
    /// движка 0..=2 — та же вилка, что у юзерских Лор).
    #[serde(default = "default_ref_edit_strength")]
    pub ref_edit_lora_strength: f32,
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

/// Режим ref-edit бандла (F4, 29.10.26). Значения JSON: "native" | "edit_lora".
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefEditMode {
    /// Стек сам умеет ref-conditioning (Qwen-Image-2.1: ref-конвейер и пресет
    /// `qwen` вшиты в определении модели сервера, доп. лора не нужна).
    #[serde(rename = "native")]
    Native,
    /// Ref-conditioning работает ТОЛЬКО в паре с обученной edit-лорой
    /// (Krea2-стек: без неё патч-latents держат «checkerboard»-артефакт,
    /// ostris/ComfyUI-Krea2-Ostris-Edit issue #1 — идентичные серые квадраты).
    #[serde(rename = "edit_lora")]
    EditLora,
}

fn default_ref_edit_strength() -> f32 {
    1.0
}

/// Контракт ref-edit — то, что edit_image обязан выяснить ДО планирования.
#[derive(Clone, Debug)]
pub struct RefEditContract {
    pub mode: RefEditMode,
    /// Стем edit-лоры (None у Native).
    pub lora: Option<String>,
    /// Готовая строка `ref_image_args` для сервера (Some("preset=...") у
    /// EditLora, None у Native — сервер сам выберет preset модели).
    pub ref_image_args: Option<String>,
    /// Сила подключаемой edit-лоры (актуальна только при EditLora).
    pub lora_strength: f32,
}

/// F4 (29.10.26): декларативный контракт ref-edit бандла.
///
/// `None` — бандл не умеет ref-edit: вызывающий (edit_image) обязан отказать
/// с понятным текстом, а не сourses с грузом untrained-патча. `Err` —
/// внутренняя несостыковка режима (fields missing у EditLora / сила вне
/// вилки): её ловит `validate_image_entry` при загрузке каталога, но здесь
/// она повторно проверяется (defense-in-depth для ручных entries).
pub fn ref_edit_contract(
    entry: &ImageBundleEntry,
) -> Result<Option<RefEditContract>, String> {
    match entry.ref_edit_mode {
        // Нет поля — ref-edit не поддержан (fail-closed, осознанный дизайн).
        // Но сиротские поля (лора/пресет без mode) — дефект записи каталога:
        // молча проглотить их значит оставить «наполовину объявленный»
        // контракт — а половинный контракт хуже отсутствующего: читатель
        // записи честно поверит, что редактирование поддерживается.
        None => {
            if entry.ref_edit_lora.is_some() || entry.ref_edit_preset.is_some() {
                return Err(format!(
                    "Бандл «{}»: ref_edit_lora/ref_edit_preset заданы, но ref_edit_mode отсутствует — без mode контракт не активен",
                    entry.name
                ));
            }
            Ok(None)
        }
        Some(RefEditMode::Native) => Ok(Some(RefEditContract {
            mode: RefEditMode::Native,
            lora: None,
            ref_image_args: None,
            lora_strength: entry.ref_edit_lora_strength,
        })),
        Some(RefEditMode::EditLora) => {
            let lora = entry
                .ref_edit_lora
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let preset = entry
                .ref_edit_preset
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let (Some(lora), Some(preset)) = (lora, preset) else {
                return Err(format!(
                    "Бандл «{}»: ref_edit_mode = \"edit_lora\" требует непустых ref_edit_lora и ref_edit_preset",
                    entry.name
                ));
            };
            if !(entry.ref_edit_lora_strength.is_finite()
                && entry.ref_edit_lora_strength > 0.0
                && entry.ref_edit_lora_strength <= LORA_MULTIPLIER_MAX)
            {
                return Err(format!(
                    "Бандл «{}»: ref_edit_lora_strength = {} вне вилки (0, {}]",
                    entry.name, entry.ref_edit_lora_strength, LORA_MULTIPLIER_MAX
                ));
            }
            Ok(Some(RefEditContract {
                mode: RefEditMode::EditLora,
                lora: Some(lora),
                ref_image_args: Some(format!("preset={}", preset)),
                lora_strength: clamp_multiplier(entry.ref_edit_lora_strength),
            }))
        }
    }
}

const EMBEDDED_CATALOG: &str = include_str!("../../image_models_catalog.json");

/// Встроенный каталог бандлов. Битая запись НЕ молчит: пропускается с
/// log::error (тот же принцип, что у load_video_catalog — «сломанный бандл
/// не должен выглядеть рабочим»). Поверх по-бандловой валидации — инвариант
/// пула «одно имя файла = один контент» (`enforce_pool_filename_invariant`).
pub fn load_image_catalog() -> Vec<ImageBundleEntry> {
    let parsed: Vec<ImageBundleEntry> = serde_json::from_str(EMBEDDED_CATALOG).unwrap_or_default();
    let valid: Vec<ImageBundleEntry> = parsed
        .into_iter()
        .filter(|e| {
            if let Err(msg) = validate_image_entry(e) {
                log::error!(
                    "[IMG] Бандл «{}» в каталоге невалиден: {} — пропущен",
                    e.name,
                    msg
                );
                false
            } else {
                true
            }
        })
        .collect();
    enforce_pool_filename_invariant(valid)
}

/// Инвариант плоского пула: одно имя файла = один контент.
///
/// Пул резолвит веса ТОЛЬКО по имени файла (`<пул>/<filename>`), поэтому имя,
/// объявленное двумя бандлами с разным URL/размером, означало бы молчаливое
/// переиспользование чужого файла (совпал размер) или невидимую поломку
/// второго бандла после перезаписи общего файла при докачке (размер разный).
///
/// Нарушившая запись удаляется из каталога ЦЕЛИКОМ (fail-closed, «сломанный
/// бандл не должен выглядеть рабочим»): первый по порядку объявления владелец
/// имени остаётся, конфликтующий бандл пропускается с log::error. Боевой
/// каталог держит инвариант тестом `embedded_catalog_satisfies_pool_filename_invariant`.
pub fn enforce_pool_filename_invariant(entries: Vec<ImageBundleEntry>) -> Vec<ImageBundleEntry> {
    // имя файла → (download_url, size_bytes, бандл-владелец) первого объявления
    let mut seen: std::collections::HashMap<String, (String, Option<u64>, String)> =
        std::collections::HashMap::new();
    entries
        .into_iter()
        .filter(|e| {
            let mut conflict: Option<String> = None;
            for f in &e.files {
                if let Some((prev_url, prev_size, prev_owner)) = seen.get(&f.filename) {
                    if prev_url != &f.download_url || prev_size != &f.size_bytes {
                        conflict = Some(format!(
                            "файл «{}» уже объявлен бандлом «{}» с другим URL/размером ({} байт ≠ {} байт)",
                            f.filename,
                            prev_owner,
                            prev_size.map(|s| s.to_string()).unwrap_or_else(|| "?".into()),
                            f.size_bytes.map(|s| s.to_string()).unwrap_or_else(|| "?".into())
                        ));
                        break;
                    }
                }
            }
            match conflict {
                Some(msg) => {
                    log::error!(
                        "[IMG] Бандл «{}» пропущен: нарушен инвариант пула (одно имя = один контент) — {}",
                        e.name,
                        msg
                    );
                    false
                }
                None => {
                    for f in &e.files {
                        seen.entry(f.filename.clone()).or_insert_with(|| {
                            (f.download_url.clone(), f.size_bytes, e.name.clone())
                        });
                    }
                    true
                }
            }
        })
        .collect()
}

/// Валидация image-бандла (F4, 29.10.26; образец — validate_video_entry).
///
/// Способность генерировать обязана быть проверяемой из каталога: бандл без
/// diffusion-файла не может выдать НИЧЕГО — для него «правка каталога» — не
/// молчаливый допуск, а ошибка записи. Контракт ref-edit целиком прогоняется
/// через `ref_edit_contract` (значений mode и парности полей edit_lora).
pub fn validate_image_entry(entry: &ImageBundleEntry) -> Result<(), String> {
    if !entry.files.iter().any(|f| f.role == "diffusion") {
        return Err("нет роли «diffusion»".to_string());
    }
    ref_edit_contract(entry).map(|_| ())
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

// ─────────────────────────── ВИДЕО-КАТАЛОГ ───────────────────────────

/// Дефолтный пресет генерации видео-бандла (SSOT — тот же принцип, что у image).
///
/// steps_draft — шаги черновика при наличии Turbo-LoRA (операционная точка
/// DMAD 4-step, подтверждена бенчем 26.10.26: 130.8 с против 505.6 с у final,
/// 3.87×); если `lora_draft`-файл отсутствует в бандле — движок берёт
/// steps_draft_nolora (6) и логирует факт. steps_final — качество-режим
/// (20 шагов, замер ролика Ngc6vmyGOyU). Турбо-точка сэмплинга —
/// turbo_sampler/turbo_scheduler/turbo_flow_shift (lcm/simple/12.0); euler
/// остаётся путём final и draft-без-LoRA.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VideoPreset {
    /// Шаги черновика при наличии Turbo-LoRA (из каталога, не из кода).
    #[serde(default = "v_steps_draft")]
    pub steps_draft: u32,
    /// Шаги черновика БЕЗ Turbo-LoRA (fallback), тоже из каталога.
    #[serde(default = "v_steps_draft_nolora")]
    pub steps_draft_nolora: u32,
    /// Шаги финала (качество-режим).
    #[serde(default = "v_steps_final")]
    pub steps_final: u32,
    /// fps потока.
    #[serde(default = "v_fps")]
    pub fps: u32,
    /// Макс. длительность клипа (сек). Сетка кадров — ответственность движка.
    #[serde(default = "v_max_seconds")]
    pub max_seconds: u32,
    #[serde(default = "v_cfg_scale")]
    pub cfg_scale: f32,
    #[serde(default = "v_sampler")]
    pub sampler: String,
    #[serde(default = "v_seed")]
    pub seed: i64,
    #[serde(default = "v_width")]
    pub width: u32,
    #[serde(default = "v_height")]
    pub height: u32,
    /// Сэмплер турбо-точки (DMAD: lcm; euler — НЕ операционная точка DMAD).
    #[serde(default = "v_turbo_sampler")]
    pub turbo_sampler: String,
    /// Шедулер турбо-точки (DMAD: simple).
    #[serde(default = "v_turbo_scheduler")]
    pub turbo_scheduler: String,
    /// flow_shift турбо-точки (DMAD: 12.0). 0.0 у H3 вырождает сигма-сетку →
    /// CUDA crash — запрещён валидатором всегда (бенч 26.10.26, diag_report).
    #[serde(default = "v_turbo_flow_shift")]
    pub turbo_flow_shift: f32,
}

fn v_steps_draft() -> u32 { 3 }
fn v_steps_draft_nolora() -> u32 { 6 }
fn v_steps_final() -> u32 { 20 }
fn v_fps() -> u32 { 24 }
fn v_max_seconds() -> u32 { 15 }
fn v_cfg_scale() -> f32 { 1.0 }
fn v_sampler() -> String { "euler".to_string() }
fn v_seed() -> i64 { -1 }
fn v_width() -> u32 { 864 }
fn v_height() -> u32 { 480 }
fn v_turbo_sampler() -> String { "lcm".to_string() }
fn v_turbo_scheduler() -> String { "simple".to_string() }
fn v_turbo_flow_shift() -> f32 { 12.0 }

impl Default for VideoPreset {
    fn default() -> Self {
        Self {
            steps_draft: v_steps_draft(),
            steps_draft_nolora: v_steps_draft_nolora(),
            steps_final: v_steps_final(),
            fps: v_fps(),
            max_seconds: v_max_seconds(),
            cfg_scale: v_cfg_scale(),
            sampler: v_sampler(),
            seed: v_seed(),
            width: v_width(),
            height: v_height(),
            turbo_sampler: v_turbo_sampler(),
            turbo_scheduler: v_turbo_scheduler(),
            turbo_flow_shift: v_turbo_flow_shift(),
        }
    }
}

/// Один вариант размещения весов видео-бандла (политика placement).
///
/// Каталог держит ЛЕСТНИЦУ вариантов от самой быстрой (требует больше RAM)
/// к самой лёгкой; при спавне выбирается самый требовательный вариант,
/// который влезает в свободную ОЗУ машины — «жрёт сколько есть, остальное
/// стримится с диска». Пороги — ТОЛЬКО из замера (RSS-пик × 1.15, правило
/// docs/backend.md), не с потолка.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VideoPlacement {
    /// Идентификатор политики (лог выбора/UI): "autofit" | "te_disk" | "all_disk" | "offload".
    pub policy: String,
    /// Человекочитаемое описание варианта.
    #[serde(default)]
    pub label: String,
    /// CLI-флаги варианта (строка, сплит по пробелам) — SSOT каталога.
    pub spawn_flags: String,
    /// Свободная ОЗУ (ГБ), ниже которой вариант не выбирается. Из замера.
    pub ram_min_gb: f32,
}

/// Один бандл видео-каталога. Роли файлов: "diffusion" | "llm" | "vae" |
/// "audio_vae" | "lora_draft" (Turbo-LoRA DMAD 4-step, бенч 26.10.26).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VideoBundleEntry {
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub files: Vec<BundleFile>,
    /// CLI-флаги spawn (строкa, сплит по пробелам): `--offload-to-cpu --rng cpu`
    /// — из docs/minimax_h3.md. SSOT каталога, не кода; image-специфичное
    /// (`--auto-fit`/`--max-vram`/`--fa`) видео НЕ копирует.
    /// Легаси-режим: используется ТОЛЬКО когда `placement` пуст.
    #[serde(default)]
    pub spawn_flags: Option<String>,
    /// Лестница placement-вариантов по свободной ОЗУ (адаптивный выбор при
    /// спавне, `resolve_placement`). Пусто = легаси-режим: единственная
    /// политика из `spawn_flags`/`ram_min_gb`.
    #[serde(default)]
    pub placement: Vec<VideoPlacement>,
    /// Задел под `--model-args` (у H3 пока не нужен — ключей model-args нет).
    #[serde(default)]
    pub model_args: Option<String>,
    /// Режимы vid_gen, которые диффузион-файл бандла реально поддерживает
    /// (декларативный гейт тула: i2va-запрос на ref2va-бандле — честный отказ,
    /// не тихий t2va). Значения: "t2va" | "i2va" | "fl2va" | "ref2va"
    /// (VideoMode::as_str). "t2va" обязателен у каждого бандла.
    #[serde(default)]
    pub modes: Vec<String>,
    #[serde(default)]
    pub preset: VideoPreset,
    #[serde(default)]
    pub size_gb: Option<String>,
    #[serde(default)]
    pub vram_fast_gb: Option<f32>,
    #[serde(default)]
    pub vram_min_gb: Option<f32>,
    #[serde(default)]
    pub ram_min_gb: Option<f32>,
    /// База LoRA-каталога (подпапка `<video_loras>/<lora_base>`): лоры
    /// привязаны к диффузион-бэкбону, кросс-база отсекается структурно —
    /// зеркально `lora_base` у `ImageBundleEntry`. Все бандлы семейства
    /// MiniMax H3 — одна база `"minimax_h3"` (turbo-лора DMAD у них общая).
    /// None — легаси-плоский каталог сервера (bundle_dir).
    #[serde(default)]
    pub lora_base: Option<String>,
}

const EMBEDDED_VIDEO_CATALOG: &str = include_str!("../../video_models_catalog.json");

/// Роли, обязательные для КАЖДОГО видео-бандла (отказ-логика: бандл без
/// audio_vae = «немое видео» — это дефект каталога, а не молчаливый допуск).
const VIDEO_REQUIRED_ROLES: [&str; 4] = ["diffusion", "llm", "vae", "audio_vae"];

/// Валидные режимы vid_gen (VideoMode::as_str).
const VIDEO_VALID_MODES: [&str; 4] = ["t2va", "i2va", "fl2va", "ref2va"];

/// Валидация одного видео-бандла. Границы — защита от опечаток в JSON,
/// НЕ лимиты сервера: реальные ограничения проверяет preflight движка.
pub fn validate_video_entry(entry: &VideoBundleEntry) -> Result<(), String> {
    for role in VIDEO_REQUIRED_ROLES {
        if !entry.files.iter().any(|f| f.role == role) {
            return Err(format!("нет обязательной роли «{}»", role));
        }
    }
    for f in &entry.files {
        let count = entry.files.iter().filter(|g| g.role == f.role).count();
        if count > 1 {
            return Err(format!("роль «{}» задублирована ({} файла)", f.role, count));
        }
    }
    // modes — декларативный гейт режимов тула: пустой список ломает i2va-гейт
    // (запросы картинка→видео не смогут отличить поддержанный бандл от
    // неподдержанного), "t2va" обязателен — видео-бандл, не умеющий t2va,
    // не имеет смысла.
    if entry.modes.is_empty() {
        return Err("пустой modes — список поддерживаемых режимов vid_gen обязан быть объявлен".to_string());
    }
    if !entry.modes.iter().any(|m| m == "t2va") {
        return Err("modes не содержит «t2va» — видео-бандл обязан поддерживать text→video".to_string());
    }
    for m in &entry.modes {
        if !VIDEO_VALID_MODES.contains(&m.as_str()) {
            return Err(format!(
                "неизвестный режим «{}» в modes (валидные: {})",
                m,
                VIDEO_VALID_MODES.join(", ")
            ));
        }
    }
    if entry.preset.steps_draft == 0 {
        return Err("steps_draft = 0".to_string());
    }
    if entry.preset.steps_draft_nolora < entry.preset.steps_draft {
        return Err("steps_draft_nolora меньше steps_draft (fallback обязан быть не быстрее)".to_string());
    }
    if entry.preset.steps_final < entry.preset.steps_draft_nolora {
        return Err("steps_final меньше steps_draft_nolora".to_string());
    }
    if entry.preset.fps == 0 {
        return Err("fps = 0".to_string());
    }
    if entry.preset.max_seconds < 1 || entry.preset.max_seconds > 60 {
        return Err(format!("max_seconds = {} вне диапазона 1..=60", entry.preset.max_seconds));
    }
    if entry.preset.cfg_scale <= 0.0 {
        return Err("cfg_scale <= 0".to_string());
    }
    if entry.preset.width == 0 || entry.preset.height == 0 || entry.preset.width % 32 != 0 || entry.preset.height % 32 != 0 {
        return Err(format!("размер {}x{} не кратен 32", entry.preset.width, entry.preset.height));
    }
    if entry.preset.sampler.trim().is_empty() {
        return Err("пустой sampler".to_string());
    }
    // flow_shift 0.0 у H3 вырождает сигма-сетку → CUDA illegal memory access
    // (любой сэмплер; бенч 26.10.26, test/bench/video_h3_turbo/diag_report.json,
    // матрица 2×2). Делаем отказ структурно невозможным: неположительный shift
    // запрещён ВСЕГДА, не только в турбо-ветке.
    if entry.preset.turbo_flow_shift <= 0.0 {
        return Err(format!(
            "turbo_flow_shift = {} — 0.0 у H3 вырождает сигма-сетку → CUDA crash (бенч 26.10.26); \
             положительный shift обязателен всегда",
            entry.preset.turbo_flow_shift
        ));
    }
    // Бандл с ролью lora_draft обязан описать турбо-точку целиком: сэмплер и
    // шедулер непусты (shift уже проверен выше). Иначе draft-ветка ушла бы в
    // несуществующую операционную точку DMAD.
    if entry.files.iter().any(|f| f.role == "lora_draft")
        && (entry.preset.turbo_sampler.trim().is_empty() || entry.preset.turbo_scheduler.trim().is_empty())
    {
        return Err(
            "в бандле есть роль lora_draft, но turbo_sampler/turbo_scheduler пусты — \
             операционная точка Turbo-LoRA обязана быть описана в каталоге (SSOT)"
                .to_string(),
        );
    }
    // placement-лестница: каждый вариант обязан быть исполним (флаги непусты,
    // порог RAM осмыслен, имена политик уникальны — иначе выбор неоднозначен).
    let mut seen_policies: Vec<&str> = Vec::new();
    for v in &entry.placement {
        if v.policy.trim().is_empty() {
            return Err("в placement есть вариант с пустым policy".to_string());
        }
        if v.spawn_flags.trim().is_empty() {
            return Err(format!(
                "в placement варианта «{}» пустые spawn_flags — флаги обязаны идти из каталога (SSOT)",
                v.policy
            ));
        }
        if !(v.ram_min_gb.is_finite() && v.ram_min_gb > 0.0) {
            return Err(format!(
                "в placement варианта «{}» ram_min_gb = {} — порог обязан быть положительным числом из замера",
                v.policy, v.ram_min_gb
            ));
        }
        if seen_policies.contains(&v.policy.as_str()) {
            return Err(format!("в placement дублируется политика «{}»", v.policy));
        }
        seen_policies.push(&v.policy);
    }
    Ok(())
}

/// Адаптивный выбор placement-варианта по свободной ОЗУ: среди вариантов,
/// влезающих в `ram_free_gb`, берётся САМЫЙ ТРЕБОВАТЕЛЬНЫЙ (самый быстрый);
/// не влез никто — честный отказ с порогом самого лёгкого варианта.
/// `ram_free_gb = None` (фактов нет) → консервативно выбирается самый лёгкий.
///
/// Текст отказа БЕЗ канонического маркера — вызывающий (video_engine) префиксует
/// `preflight::resource_markers::RAM` (SSOT формулировок, инцидент 27.10.26).
pub fn resolve_placement<'a>(
    entry: &'a VideoBundleEntry,
    ram_free_gb: Option<f64>,
) -> Result<&'a VideoPlacement, String> {
    if entry.placement.is_empty() {
        return Err(
            "у бандла нет placement-вариантов (легаси-режим: единственная политика из spawn_flags)"
                .to_string(),
        );
    }
    // Фактов ОЗУ нет → консервативно самый лёгкий вариант (документированный
    // контракт); трактовать None как «0 ГБ» значило бы отвергать всё.
    let Some(free) = ram_free_gb else {
        let lightest = entry
            .placement
            .iter()
            .min_by(|a, b| a.ram_min_gb.total_cmp(&b.ram_min_gb))
            .expect("placement непуст (проверено выше)");
        return Ok(lightest);
    };
    let free = free as f32;
    let mut best: Option<&VideoPlacement> = None;
    for v in &entry.placement {
        if free >= v.ram_min_gb {
            best = match best {
                Some(b) if b.ram_min_gb >= v.ram_min_gb => Some(b),
                _ => Some(v),
            };
        }
    }
    if let Some(v) = best {
        return Ok(v);
    }
    // Ни один не влез: порог называет самый лёгкий вариант (минимальный ram_min_gb).
    let lightest = entry
        .placement
        .iter()
        .min_by(|a, b| a.ram_min_gb.total_cmp(&b.ram_min_gb))
        .expect("placement непуст (проверено выше)");
    Err(format!(
        "нужно ≥ {:.1} ГБ свободной ОЗУ даже для самой лёгкой политики «{}» ({:.1} ГБ), фактически {:.1} ГБ. \
         Закройте другие приложения или возьмите бандл легче.",
        lightest.ram_min_gb, lightest.policy, lightest.ram_min_gb, free
    ))
}

/// Встроенный каталог видео-бандлов. Битая запись НЕ молчит: пропускается
/// с log::error («сломанный граф не должен выглядеть рабочим»).
pub fn load_video_catalog() -> Vec<VideoBundleEntry> {
    let parsed: Vec<VideoBundleEntry> = serde_json::from_str(EMBEDDED_VIDEO_CATALOG).unwrap_or_default();
    parsed
        .into_iter()
        .filter(|e| {
            if let Err(msg) = validate_video_entry(e) {
                log::error!("[VIDEO] Бандл «{}» в каталоге невалиден: {} — пропущен", e.name, msg);
                false
            } else {
                true
            }
        })
        .collect()
}

/// Бандл по умолчанию (первый с is_default).
pub fn default_video_bundle_entry() -> Option<VideoBundleEntry> {
    load_video_catalog().into_iter().find(|e| e.is_default)
}

/// Бандл по имени (для set_selected_video_bundle).
pub fn video_bundle_by_name(name: &str) -> Option<VideoBundleEntry> {
    load_video_catalog().into_iter().find(|e| e.name == name)
}

/// Суммарный размер файлов видео-бандла в байтах.
pub fn video_bundle_disk_bytes(entry: &VideoBundleEntry) -> u64 {
    video_entry_files(entry).iter().map(|f| f.size_bytes.unwrap_or(0)).sum()
}

fn video_entry_files(entry: &VideoBundleEntry) -> &[BundleFile] {
    &entry.files
}

/// Пути файлов по ролям (общая версия — принимает срез файлов бандла).
pub fn bundle_files_weights(
    bundle_dir: &std::path::Path,
    files: &[BundleFile],
) -> std::collections::HashMap<String, String> {
    files
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

    /// F4 29.10.26: ref-edit контракт каталога — Qwen native (сервер сам умеет
    /// ref-conditioning), Krea2 = edit_lora (identity-лора + preset
    /// `krea2_edit`, docs/edit.md), контракт отдаёт готовый `preset=...`.
    #[test]
    fn ref_edit_contract_catalog_bundles() {
        let catalog = load_image_catalog();
        assert_eq!(catalog.len(), 4, "каталог должен пройти валидацию целым");
        let by = |name: &str| {
            catalog
                .iter()
                .find(|e| e.name == name)
                .cloned()
                .unwrap_or_else(|| panic!("нет бандла {}", name))
        };
        let unsloth = by("qwen-image-2.1-turbo-unsloth-q5_ks");
        assert_eq!(
            unsloth.ref_edit_mode,
            Some(RefEditMode::Native),
            "Qwen-2.1 сам умеет ref-conditioning — edit без лоры работает"
        );
        let c = ref_edit_contract(&unsloth).unwrap().unwrap();
        assert_eq!(c.mode, RefEditMode::Native);
        assert!(
            c.lora.is_none() && c.ref_image_args.is_none(),
            "native: лора и ref_image_args не нужны серверу"
        );
        assert_eq!(
            by("qwen-image-2.1-turbo-viggle-q4km").ref_edit_mode,
            Some(RefEditMode::Native),
            "viggle — тот же Qwen-стек, ref-edit нативный"
        );
        for name in ["krea2-turbo-q4km", "krea2-turbo-q3km"] {
            let e = by(name);
            assert_eq!(
                e.ref_edit_mode,
                Some(RefEditMode::EditLora),
                "Krea2 без обученной edit-лоры даёт «checkerboard» (ostris issue #1)"
            );
            assert_eq!(
                e.ref_edit_lora.as_deref(),
                Some("krea2_identity_edit_v2_r128"),
                "edit-лора каталога — identity edit V2 r128 (ранг-редуced, 914 МБ)"
            );
            assert_eq!(
                e.ref_edit_preset.as_deref(),
                Some("krea2_edit"),
                "пресет под identity-лору — krea2_edit (docs/edit.md)"
            );
            let c = ref_edit_contract(&e).unwrap().unwrap();
            assert_eq!(c.mode, RefEditMode::EditLora);
            assert_eq!(
                c.ref_image_args.as_deref(),
                Some("preset=krea2_edit"),
                "contract отдаёт готовую строку ref_image_args серверу"
            );
        }
    }

    /// Fail-closed по дизайну (§00.2): бандл без ref_edit_mode не редактирует
    /// вовсе (contract None), болтающиеся ref_edit_lora/preset без mode —
    /// ошибка каталога; edit_lora без лоры/пресета — отказ валидатора с именем
    /// поля; сила вне вилки — отказ.
    #[test]
    fn ref_edit_fail_closed_and_validator() {
        // Mode снят, поля от исходного Krea2-бандла остались — сироты:
        // валидатор отказывает и называет ref_edit_mode.
        let mut e = bundle_by_name("krea2-turbo-q4km").expect("бандл");
        e.ref_edit_mode = None;
        let msg = validate_image_entry(&e).unwrap_err();
        assert!(
            msg.contains("ref_edit_mode") && msg.contains("ref_edit_lora"),
            "валидатор называет несостыковку, got: {}",
            msg
        );
        // Чистый legacy: без mode и без полей — контракт None (fail-closed),
        // валидатор пропускает запись целиком.
        e.ref_edit_lora = None;
        e.ref_edit_preset = None;
        assert!(
            ref_edit_contract(&e).unwrap().is_none(),
            "нет mode — ref-edit недоступен (fail-closed, не «native по умолчанию»)"
        );
        assert!(
            validate_image_entry(&e).is_ok(),
            "без mode и без полей — легитимный legacy-бандл без ref-edit"
        );
        e.ref_edit_lora = Some("сиротская-лора".to_string());
        let msg = validate_image_entry(&e).unwrap_err();
        assert!(
            msg.contains("ref_edit_mode") && msg.contains("ref_edit_lora"),
            "валидатор называет несостыковку, got: {}",
            msg
        );
        e.ref_edit_lora = None;
        e.ref_edit_preset = Some("krea2_edit".to_string());
        assert!(validate_image_entry(&e).unwrap_err().contains("ref_edit_mode"));
        e.ref_edit_preset = None;
        assert!(validate_image_entry(&e).is_ok(), "без mode и без полей — легитимный legacy-бандл");

        // edit_lora без лоры / без пресета — отказ с именами полей.
        e.ref_edit_mode = Some(RefEditMode::EditLora);
        e.ref_edit_lora = None;
        assert!(validate_image_entry(&e).unwrap_err().contains("ref_edit_lora"));
        e.ref_edit_lora = Some("krea2_identity_edit_v2_r128".to_string());
        e.ref_edit_preset = None;
        assert!(validate_image_entry(&e).unwrap_err().contains("ref_edit_preset"));
        e.ref_edit_preset = Some("krea2_edit".to_string());
        assert!(validate_image_entry(&e).is_ok());

        // Сила вне вилки (0, 2] — отказ.
        let mut bad = e.clone();
        bad.ref_edit_lora_strength = 2.5;
        assert!(
            validate_image_entry(&bad).unwrap_err().contains("ref_edit_lora_strength"),
            "сила edit-лоры требует вилки"
        );
        let mut bad = e;
        bad.ref_edit_lora_strength = 0.0;
        assert!(validate_image_entry(&bad).unwrap_err().contains("ref_edit_lora_strength"));

        // Native-бандл с лорой-заделом юзера — валиден (юзер вправе дополнить стек).
        let mut native = bundle_by_name("qwen-image-2.1-turbo-unsloth-q5_ks").unwrap();
        native.ref_edit_lora = Some("что-то-личное".to_string());
        assert!(validate_image_entry(&native).is_ok());
    }
}

#[cfg(test)]
mod video_tests {
    use super::*;

    #[test]
    fn video_catalog_parses_with_default_q4km() {
        let catalog = load_video_catalog();
        assert_eq!(
            catalog.len(),
            3,
            "PRP 21.10.26: 3 стартовых видео-бандла (Q4_K + Q3_K + Ref2VA Q3_K), got {}",
            catalog.len()
        );
        let def = default_video_bundle_entry().expect("нет видео-бандла по умолчанию");
        assert_eq!(
            def.name, "minimax_h3_fl2va_pruned-q4km",
            "дефолт для VRAM ≥12 ГБ — Q4_K (юзер 16 ГБ), got {}",
            def.name
        );
        // Правило из PRP: 4 обязательные роли, включая audio_vae (рус. речь).
        for e in &catalog {
            for r in VIDEO_REQUIRED_ROLES {
                assert!(
                    e.files.iter().any(|f| f.role == r),
                    "бандл «{}» без роли «{}»",
                    e.name,
                    r
                );
            }
        }
    }

    #[test]
    fn video_buttons_follow_prp_numbers() {
        let catalog = load_video_catalog();
        for e in &catalog {
            assert_eq!(e.preset.steps_draft, 4, "draft с Turbo-LoRA = 4 шага (операционная точка DMAD, бенч 26.10.26) у {}", e.name);
            assert_eq!(e.preset.steps_draft_nolora, 6, "fallback без LoRA = 6 шагов у {}", e.name);
            assert_eq!(e.preset.steps_final, 20, "final = 20 шагов у {}", e.name);
            assert_eq!(e.preset.fps, 24);
            assert_eq!(e.preset.max_seconds, 15, "ролики рекомендуют ≤15 сек у {}", e.name);
            assert!((e.preset.cfg_scale - 1.0).abs() < f32::EPSILON, "H3 дефолт cfg 1.0 у {}", e.name);
            assert_eq!(e.preset.sampler, "euler");
            assert_eq!(e.preset.width % 32, 0);
            assert_eq!(e.preset.height % 32, 0);
            // SSOT: H3 без model-args (икс ключей нет) — и не должно быть.
            assert!(e.model_args.is_none(), "{}: model_args не должен быть", e.name);
        }
    }

    /// Turbo-точка DMAD во всех записях каталога: роль lora_draft +
    /// lcm/simple/shift12, записи проходят валидацию (бенч 26.10.26).
    #[test]
    fn turbo_fields_valid_for_all_video_entries() {
        let catalog = load_video_catalog();
        assert!(!catalog.is_empty(), "каталог не должен быть пуст");
        for e in &catalog {
            let roles: Vec<&str> = e.files.iter().map(|f| f.role.as_str()).collect();
            assert!(roles.contains(&"lora_draft"), "нет роли lora_draft у {}", e.name);
            assert_eq!(e.preset.turbo_sampler, "lcm", "операционная точка DMAD: lcm у {}", e.name);
            assert_eq!(e.preset.turbo_scheduler, "simple", "операционная точка DMAD: simple у {}", e.name);
            assert!((e.preset.turbo_flow_shift - 12.0).abs() < f32::EPSILON, "турбо shift 12.0 у {}", e.name);
            // Полный прогон валидатора — запись с lora_draft обязана проходить.
            validate_video_entry(e).unwrap_or_else(|err| panic!("{}: {}", e.name, err));
        }
    }

    /// Краш-регрессия 26.10.26: flow_shift 0.0 у H3 = CUDA crash. Валидатор
    /// обязан отказать записи с неположительным turbo_flow_shift (и с
    /// lora_draft, и без), упоминая flow_shift.
    #[test]
    fn video_entry_with_zero_turbo_flow_shift_is_rejected() {
        let base = video_bundle_by_name("minimax_h3_fl2va_pruned-q4km").expect("бандл");
        // С lora_draft: 0.0 → отказ с именем поля.
        let mut bad = base.clone();
        bad.preset.turbo_flow_shift = 0.0;
        let msg = validate_video_entry(&bad).unwrap_err();
        assert!(msg.contains("flow_shift"), "валидатор должен называть flow_shift, got: {}", msg);
        // Без lora_draft — тоже отказ: неположительный shift запрещён всегда.
        let mut bad_no_lora = base.clone();
        bad_no_lora.files.retain(|f| f.role != "lora_draft");
        bad_no_lora.preset.turbo_flow_shift = 0.0;
        let msg2 = validate_video_entry(&bad_no_lora).unwrap_err();
        assert!(msg2.contains("flow_shift"), "запрет 0.0 действует всегда, got: {}", msg2);
        // lora_draft без описания турбо-сэмплера — отказ (точка неполна).
        let mut bad_empty_turbo = base;
        bad_empty_turbo.preset.turbo_sampler = "  ".to_string();
        let msg3 = validate_video_entry(&bad_empty_turbo).unwrap_err();
        assert!(
            msg3.contains("turbo_sampler"),
            "lora_draft требует непустую турбо-точку, got: {}",
            msg3
        );
    }

    #[test]
    fn video_defaults_size_and_class_ordering() {
        let catalog = load_video_catalog();
        let by = |name: &str| {
            catalog
                .iter()
                .find(|e| e.name == name)
                .cloned()
                .unwrap_or_else(|| panic!("нет бандла {}", name))
        };
        let q4km = by("minimax_h3_fl2va_pruned-q4km");
        let q3k = by("minimax_h3_fl2va_pruned-q3k");
        let ref2va = by("minimax_h3_ref2va_pruned-q3k");
        // Классы памяти: порог Q4 К выше порога Q3_K (авто-выбор по vram_min_gb).
        assert!(
            q4km.vram_min_gb.unwrap_or(0.0) > q3k.vram_min_gb.unwrap_or(0.0),
            "Q4_K обязан требовать больше VRAM, чем Q3_K"
        );
        // Полный бандл — 4 файла + lora_draft (Turbo-LoRA DMAD, бенч 26.10.26).
        assert_eq!(q4km.files.len(), 5);
        let diff = |e: &VideoBundleEntry, role: &str| {
            e.files
                .iter()
                .find(|f| f.role == role)
                .cloned()
                .unwrap_or_else(|| panic!("нет роли {} у {}", role, e.name))
        };
        assert_eq!(diff(&q4km, "diffusion").size_bytes, Some(11420663904));
        assert_eq!(diff(&q3k, "diffusion").size_bytes, Some(8759328864));
        assert_eq!(diff(&ref2va, "diffusion").size_bytes, Some(8716105824));
        for e in [&q4km, &q3k, &ref2va] {
            assert_eq!(diff(e, "llm").size_bytes, Some(13102161024));
            assert_eq!(diff(e, "vae").size_bytes, Some(5207808496));
            assert_eq!(diff(e, "audio_vae").size_bytes, Some(605254808));
        }
        // Суммарный размер на диск — по тем же точным байтам.
        assert!(video_bundle_disk_bytes(&q4km) > 30_000_000_000, "q4km ≈ 30.3 ГБ");
    }

    #[test]
    fn video_invalid_entries_are_rejected_by_validator() {
        // Опечатка без diffusion — отказ с именем роли.
        let mut bad = video_bundle_by_name("minimax_h3_fl2va_pruned-q3k").expect("бандл");
        bad.files.retain(|f| f.role != "diffusion");
        let msg = validate_video_entry(&bad).unwrap_err().to_string();
        assert!(msg.contains("diffusion"), "валидатор должен называть роль, got: {}", msg);
        // steps_draft > steps_draft_nolora — логическое противоречие.
        let mut bad2 = bad;
        bad2.files.push(BundleFile {
            role: "diffusion".to_string(),
            filename: "minimax_h3_fl2va_pruned-Q3_K.gguf".to_string(),
            download_url: "https://huggingface.co/unsloth/MiniMax-H3-GGUF/resolve/main/minimax_h3_fl2va_pruned-Q3_K.gguf?download=true".to_string(),
            size_bytes: Some(8759328864),
        });
        bad2.preset.steps_draft = 10;
        bad2.preset.steps_draft_nolora = 6;
        let msg2 = validate_video_entry(&bad2).unwrap_err().to_string();
        assert!(msg2.contains("fallback"), "fallback-инвариант должен проверяться, got: {}", msg2);
    }

    /// modes — декларативный гейт режимов тула (Шаг 3, 27.10.26): пустой
    /// список, отсутствие t2va и неизвестный режим — отказ валидатора.
    #[test]
    fn video_modes_are_validated() {
        let catalog = load_video_catalog();
        assert!(!catalog.is_empty());
        for e in &catalog {
            assert!(
                !e.modes.is_empty() && e.modes.iter().any(|m| m == "t2va"),
                "{}: modes обязан быть непустым и содержать t2va, got {:?}",
                e.name,
                e.modes
            );
        }
        // fl2va-диффузион умеет кадры, ref2va — только референсы (BUNDLES.md §9).
        let by = |name: &str| {
            catalog
                .iter()
                .find(|e| e.name == name)
                .map(|e| e.modes.clone())
                .unwrap_or_default()
        };
        let q4km_modes = by("minimax_h3_fl2va_pruned-q4km");
        assert!(q4km_modes.contains(&"i2va".to_string()), "fl2va-бандл умеет i2va");
        assert!(q4km_modes.contains(&"fl2va".to_string()));
        let ref_modes = by("minimax_h3_ref2va_pruned-q3k");
        assert!(
            !ref_modes.contains(&"i2va".to_string()),
            "ref2va-диффузион не заявляет i2va до замера (§6 недоказанное)"
        );
        assert!(ref_modes.contains(&"ref2va".to_string()));

        // Битые modes — честный отказ валидатора.
        let mut bad = video_bundle_by_name("minimax_h3_fl2va_pruned-q3k").expect("бандл");
        bad.modes.clear();
        assert!(validate_video_entry(&bad).unwrap_err().contains("пустой modes"));
        bad.modes = vec!["i2va".to_string()];
        assert!(validate_video_entry(&bad).unwrap_err().contains("t2va"));
        bad.modes = vec!["t2va".to_string(), "hypervideo".to_string()];
        assert!(validate_video_entry(&bad).unwrap_err().contains("hypervideo"));
    }

    // ─────────── placement-лестница (адаптивное размещение, 29.10.26) ───────────

    #[test]
    fn placement_ladder_validation_rejects_broken_variants() {
        let base = video_bundle_by_name("minimax_h3_fl2va_pruned-q3k").expect("бандл");
        let mk = |policy: &str, flags: &str, ram: f32| VideoPlacement {
            policy: policy.to_string(),
            label: String::new(),
            spawn_flags: flags.to_string(),
            ram_min_gb: ram,
        };
        // Валидная лестница проходит.
        let mut ok = base.clone();
        ok.placement = vec![mk("all_disk", "--params-backend te=disk --rng cpu", 3.2)];
        assert!(validate_video_entry(&ok).is_ok());
        // Пустые флаги варианта — отказ.
        let mut bad = base.clone();
        bad.placement = vec![mk("all_disk", "   ", 3.2)];
        assert!(validate_video_entry(&bad).unwrap_err().contains("spawn_flags"));
        // Нулевой/отрицательный порог — отказ (порог только из замера).
        let mut bad2 = base.clone();
        bad2.placement = vec![mk("all_disk", "--params-backend te=disk", 0.0)];
        assert!(validate_video_entry(&bad2).unwrap_err().contains("ram_min_gb"));
        let mut bad3 = base.clone();
        bad3.placement = vec![mk("all_disk", "--params-backend te=disk", -1.0)];
        assert!(validate_video_entry(&bad3).unwrap_err().contains("ram_min_gb"));
        // Дубликат политики — отказ (выбор неоднозначен).
        let mut bad4 = base.clone();
        bad4.placement = vec![
            mk("all_disk", "--params-backend te=disk", 3.2),
            mk("all_disk", "--params-backend te=disk", 5.0),
        ];
        assert!(validate_video_entry(&bad4).unwrap_err().contains("дублируется"));
        // Пустой policy — отказ.
        let mut bad5 = base;
        bad5.placement = vec![mk("  ", "--params-backend te=disk", 3.2)];
        assert!(validate_video_entry(&bad5).unwrap_err().contains("policy"));
    }

    /// Гвардейский тест боевого каталога: q4km несёт measured-лестницу
    /// (бенч 29.10.26, test/bench/video_h3_disk/), Q3_K-бандлы остаются
    /// в легаси-режиме (offload) до собственного замера.
    #[test]
    fn embedded_catalog_q4km_has_measured_placement_ladder() {
        let catalog = load_video_catalog();
        let q4km = catalog
            .iter()
            .find(|e| e.name == "minimax_h3_fl2va_pruned-q4km")
            .expect("q4km в каталоге");
        // Лестница ровно из двух замеренных вариантов, пороги по правилу
        // «пик × 1.15, округление вверх»: te_disk 13.66→16.0, all_disk 2.72→3.2.
        assert_eq!(q4km.placement.len(), 2, "лестница q4km: te_disk + all_disk");
        let te = q4km
            .placement
            .iter()
            .find(|v| v.policy == "te_disk")
            .expect("te_disk в лестнице");
        let all = q4km
            .placement
            .iter()
            .find(|v| v.policy == "all_disk")
            .expect("all_disk в лестнице");
        assert!((te.ram_min_gb - 16.0).abs() < f32::EPSILON, "te_disk 16.0, got {}", te.ram_min_gb);
        assert!((all.ram_min_gb - 3.2).abs() < f32::EPSILON, "all_disk 3.2, got {}", all.ram_min_gb);
        for v in [te, all] {
            assert!(v.spawn_flags.contains("--params-backend"), "{}: disk-флаги", v.policy);
            assert!(v.spawn_flags.contains("--max-vram -2"), "{}: резерв VRAM -2", v.policy);
            assert!(v.spawn_flags.contains("--rng cpu"), "{}: rng cpu", v.policy);
        }
        assert!(te.spawn_flags.contains("diffusion=cpu"), "te_disk держит diffusion не на disk");
        assert!(all.spawn_flags.contains("diffusion=disk"), "all_disk стримит diffusion");
        // Легаси-поля q4km выведены из боя: политика теперь только в лестнице
        // (SSOT — один источник политики на бандл).
        assert!(
            q4km.spawn_flags.is_none(),
            "у q4km не должно быть легаси spawn_flags рядом с лестницей"
        );
        // Q3_K-бандлы: легаси-режим без лестницы (своего замера нет).
        for name in ["minimax_h3_fl2va_pruned-q3k", "minimax_h3_ref2va_pruned-q3k"] {
            let e = catalog.iter().find(|e| e.name == name).expect(name);
            assert!(e.placement.is_empty(), "{}: без своего замера лестницы нет", name);
            assert!(
                e.spawn_flags.as_deref().unwrap_or_default().contains("--offload-to-cpu"),
                "{}: легаси offload сохранён",
                name
            );
        }
    }

    // ─────────── Инвариант пула: одно имя файла = один контент ───────────

    fn entry_from_json(v: serde_json::Value) -> ImageBundleEntry {
        serde_json::from_value(v).expect("тестовый entry из JSON")
    }

    #[test]
    fn pool_filename_invariant_keeps_consistent_sharing() {
        // Два бандла делят vae: тот же URL и размер — оба остаются.
        let cat = enforce_pool_filename_invariant(vec![
            entry_from_json(serde_json::json!({
                "name": "a",
                "files": [
                    {"role": "vae", "filename": "v.safetensors", "download_url": "u1", "size_bytes": 10},
                    {"role": "diffusion", "filename": "a.gguf", "download_url": "u2", "size_bytes": 20}
                ]
            })),
            entry_from_json(serde_json::json!({
                "name": "b",
                "files": [
                    {"role": "vae", "filename": "v.safetensors", "download_url": "u1", "size_bytes": 10},
                    {"role": "diffusion", "filename": "b.gguf", "download_url": "u3", "size_bytes": 30}
                ]
            })),
        ]);
        assert_eq!(cat.len(), 2, "согласное разделение файла не выбрасывает бандлы");
        assert_eq!(cat[0].name, "a");
        assert_eq!(cat[1].name, "b");
    }

    #[test]
    fn pool_filename_invariant_drops_conflicting_entry() {
        // b переиспользует имя v.safetensors с другим размером — b целиком
        // пропускается, a (первый владелец имени) остаётся.
        let cat = enforce_pool_filename_invariant(vec![
            entry_from_json(serde_json::json!({
                "name": "a",
                "files": [
                    {"role": "vae", "filename": "v.safetensors", "download_url": "u1", "size_bytes": 10}
                ]
            })),
            entry_from_json(serde_json::json!({
                "name": "b",
                "files": [
                    {"role": "vae", "filename": "v.safetensors", "download_url": "u1", "size_bytes": 999}
                ]
            })),
        ]);
        assert_eq!(cat.len(), 1, "конфликтующее имя выбрасывает весь бандл");
        assert_eq!(cat[0].name, "a");
    }

    #[test]
    fn pool_filename_invariant_drops_conflicting_url_at_same_size() {
        // Одинаковый размер, другой URL — другой контент, тоже конфликт
        // (размер-сверка не ловит его, инвариант обязан).
        let cat = enforce_pool_filename_invariant(vec![
            entry_from_json(serde_json::json!({
                "name": "a",
                "files": [
                    {"role": "vae", "filename": "v.safetensors", "download_url": "u1", "size_bytes": 10}
                ]
            })),
            entry_from_json(serde_json::json!({
                "name": "b",
                "files": [
                    {"role": "vae", "filename": "v.safetensors", "download_url": "u1-reup", "size_bytes": 10}
                ]
            })),
        ]);
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].name, "a");
    }

    /// Гвардейский тест боевого каталога: инвариант пула выполняется, общие
    /// файлы реально делятся (иначе проверка была бы вырожденной).
    #[test]
    fn embedded_catalog_satisfies_pool_filename_invariant() {
        let cat = load_image_catalog();
        assert_eq!(cat.len(), 4, "все 4 бандла боевого каталога обязаны грузиться без пропусков");
        let names: std::collections::HashSet<&str> = cat.iter().map(|e| e.name.as_str()).collect();
        for expected in [
            "qwen-image-2.1-turbo-unsloth-q5_ks",
            "qwen-image-2.1-turbo-viggle-q4km",
            "krea2-turbo-q4km",
            "krea2-turbo-q3km",
        ] {
            assert!(names.contains(expected), "в каталоге нет бандла {}", expected);
        }
        // Общие файлы делятся честно: ровно два владельца на каждую общую роль.
        for shared in [
            "Qwen3VL-8B-Instruct-Q4_K_M.gguf",
            "mmproj-Qwen3VL-8B-Instruct-F16.gguf",
            "qwen_image_2.1_vae_bf16.safetensors",
            "Qwen3VL-4B-Instruct-Q4_K_M.gguf",
            "mmproj-Qwen3VL-4B-Instruct-F16.gguf",
            "wan_2.1_vae.safetensors",
        ] {
            let owners = cat
                .iter()
                .filter(|e| e.files.iter().any(|f| f.filename == shared))
                .count();
            assert_eq!(owners, 2, "файл {} ожиданно общий для двух бандлов", shared);
        }
    }
}

//! Конфиг image-движка: читается/пишется в общий `app_config.json` хоста
//! field-preserving-merge (хостовые ключи НЕ затираются — тот же паттерн,
//! что у llama-engine `save_engine_config_file`).
//!
//! Ключи: `sdcpp_dir` (переопределение папки движка),
//! `image_engine_variant` ("auto"/cuda12/cpu/vulkan/rocm),
//! `image_bundle_dir` (папка скачанного бандла весов),
//! `selected_bundle` (активный бандл каталога — выбор юзера в UI).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ImageEngineConfig {
    #[serde(default)]
    pub sdcpp_dir: Option<String>,
    #[serde(default)]
    pub image_engine_variant: Option<String>,
    #[serde(default)]
    pub image_bundle_dir: Option<String>,
    #[serde(default)]
    pub selected_bundle: Option<String>,
    /// Папка видео-бандлов (MiniMax H3). De factо жил РЯДОМ с image-бандлами.
    #[serde(default)]
    pub video_bundle_dir: Option<String>,
    /// Активный каталог-Bar video-бандл (выбор юзера в UI).
    #[serde(default)]
    pub selected_video_bundle: Option<String>,
    /// ЕДИНАЯ папка LoRA всех движков (решение юзера 30.10.26): подпапки по
    /// базе модели (`loras/minimax_h3`, `loras/qwen`, `loras/krea2`) — зеркало
    /// структуры HF-репо «King Orch Loras», чтобы скачивание набора = перенос
    /// файлов. Явная — из конфига, иначе дефолт `lora_dir_early()`
    /// (`<KingOrchData>/loras`). Старые ключи `image_lora_dir`/`video_lora_dir`
    /// удалены из схемы: несовместимые значения serde игнорирует, папки
    /// переносит `ensure_unified_lora_dir`.
    #[serde(default)]
    pub lora_dir: Option<String>,
    /// HF-репозиторий LoRA-каталога для плашки «скачать лоры»
    /// (видео- и image-лоры в одном репо, подпапки = базы). Явная — из
    /// конфига, иначе дефолт командного слоя ("nilden/king-orch-loras").
    #[serde(default)]
    pub loras_repo: Option<String>,
}

/// Путь к `app_config.json` хоста (runtime, через AppHandle).
pub fn get_config_path(_app: &AppHandle) -> PathBuf {
    // Конфиг image-engine лежит в каталоге хоста, но путь выбирает общая дверь
    // `ko-json-store`: прямой `app.path().app_data_dir()` приводил к тому, что
    // хост писал в один каталог, а плагин искал другой — и обе стороны
    // выглядели исправными, пока настройки не пропадали.
    ko_json_store::roots::app_dir(HOST_APP_IDENTIFIER, "").join("app_config.json")
}

/// Идентификатор приложения-хоста: имя папки данных King Orch.
const HOST_APP_IDENTIFIER: &str = "com.kingorch.app";

/// Имя папки общего хранилища данных хоста — зеркало
/// `infra::data_dir::DATA_DIR_NAME` хоста. Плагин не может импортировать хост,
/// поэтому приоритет пути повторён 1:1: явный `data_dir` из общего конфига →
/// диск с макс. свободным местом + `KingOrchData` → фоллбэк `<exe>/KingOrchData`.
pub const HOST_DATA_DIR_NAME: &str = "KingOrchData";

/// Нормализация пути хранилища (зеркало `data_dir::normalize` хоста): последний
/// компонент обязан быть KingOrchData (без учёта регистра), иначе приклеивается.
/// Чистая функция, без обращений к файловой системе.
fn normalize_data_root(raw: &str) -> PathBuf {
    let trimmed = raw.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return PathBuf::from(HOST_DATA_DIR_NAME);
    }
    let p = Path::new(trimmed);
    let needs_suffix = p
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| !n.eq_ignore_ascii_case(HOST_DATA_DIR_NAME))
        .unwrap_or(true);
    if needs_suffix {
        p.join(HOST_DATA_DIR_NAME)
    } else {
        p.to_path_buf()
    }
}

/// Диск с макс. свободным местом (зеркало `data_dir::best_drive_letter` хоста).
fn best_drive_letter() -> Option<String> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut best: Option<(String, u64)> = None;
    for disk in disks.list() {
        let mount = disk.mount_point().to_string_lossy().to_string();
        if mount.len() >= 2 && mount.as_bytes()[1] == b':' {
            let letter = mount[..1].to_uppercase();
            let available = disk.available_space();
            match &best {
                Some((_, best_avail)) if available <= *best_avail => {}
                _ => best = Some((letter, available)),
            }
        }
    }
    best.map(|(letter, _)| letter)
}

/// Корень общего хранилища данных хоста (`KingOrchData`) без AppHandle:
/// `data_dir` из общего app_config.json (тот же файл, что читают все методы
/// конфига) → диск с макс. свободным местом → фоллбэк `<exe>/KingOrchData`.
pub fn data_root() -> PathBuf {
    let path = ko_json_store::roots::app_dir(HOST_APP_IDENTIFIER, "").join("app_config.json");
    if let Ok(Some(text)) = ko_json_store::read_text(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(raw) = v.get("data_dir").and_then(|x| x.as_str()) {
                if !raw.trim().is_empty() {
                    return normalize_data_root(raw);
                }
            }
        }
    }
    match best_drive_letter() {
        Some(letter) => PathBuf::from(format!("{}:\\{}", letter, HOST_DATA_DIR_NAME)),
        None => std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(HOST_DATA_DIR_NAME),
    }
}

/// Дефолтная папка image-бандлов: `<хранилище>/image_models` (пул всех наборов).
pub fn default_image_bundle_dir() -> PathBuf {
    data_root().join("image_models")
}

/// Дефолтная папка видео-бандлов: `<хранилище>/video_models`.
pub fn default_video_bundle_dir() -> PathBuf {
    data_root().join("video_models")
}

/// ЕДИНАЯ дефолтная папка LoRA всех движков: `<хранилище>/loras`
/// (подпапки по базе: minimax_h3 / qwen / krea2 — зеркало HF-репо).
pub fn default_lora_dir() -> PathBuf {
    data_root().join("loras")
}

/// Миграция имён видео-бандлов (30.10.26): два q3k-бандла (fl2va/ref2va) слиты
/// в один uni-бандл с ДВУМЯ diffusion-файлами. Старые имена в
/// `selected_video_bundle` маппятся read-time в ОБОИХ загрузчиках (SSOT —
/// единая функция): иначе юзер остался бы с «мертвым» выбором, который дал бы
/// честный отказ «бандл отсутствует в каталоге» на каждый vid_gen.
fn normalize_selected_video_bundle(cfg: &mut ImageEngineConfig) {
    const LEGACY: [&str; 2] = [
        "minimax_h3_fl2va_pruned-q3k",
        "minimax_h3_ref2va_pruned-q3k",
    ];
    const MERGED: &str = "minimax_h3_uni-q3k";
    if let Some(sel) = cfg.selected_video_bundle.as_mut() {
        if LEGACY.contains(&sel.as_str()) {
            log::info!(
                "[image-engine] selected_video_bundle «{}» слит в «{}» — мигрирую",
                sel,
                MERGED
            );
            *sel = MERGED.to_string();
        }
    }
}

/// Читает image-ключи ДО создания Tauri-приложения (тулы оркестратора, рантайм
/// без AppHandle). Путь — тот же корень, что и у хоста.
pub fn load_image_config_early() -> ImageEngineConfig {
    let path = ko_json_store::roots::app_dir(HOST_APP_IDENTIFIER, "").join("app_config.json");
    let mut cfg = match ko_json_store::read_text(&path) {
        Ok(Some(data)) => serde_json::from_str(&data).unwrap_or_default(),
        Ok(None) => ImageEngineConfig::default(),
        Err(e) => {
            log::warn!("[image-engine] конфиг {} не прочитан ({}): беру значения по умолчанию", path.display(), e);
            ImageEngineConfig::default()
        }
    };
    normalize_selected_video_bundle(&mut cfg);
    cfg
}

/// Папка движка без AppHandle: явная из конфига, иначе `<exe>/sdcpp`.
pub fn engine_dir_early() -> PathBuf {
    let cfg = load_image_config_early();
    if let Some(p) = &cfg.sdcpp_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    crate::engine::sdcpp_installer::default_dir(&exe_dir)
}

/// Папка бандлов без AppHandle: явная из конфига, иначе
/// `<хранилище KingOrchData>/image_models` (пул всех наборов).
/// Одна папка-пул для ВСЕХ бандлов (имена общих ролей совпадают, downloader
/// пропускает существующее) — per-bundle подпапки убрали 25.10.2026: они
/// порождали перекачку общих файлов и «3 файла есть, 1 нет» при переключении
/// активного бандла.
pub fn bundle_dir_early() -> PathBuf {
    let cfg = load_image_config_early();
    if let Some(p) = &cfg.image_bundle_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    default_image_bundle_dir()
}

/// Папка видео-бандлов без AppHandle: явная из конфига, иначе
/// `<хранилище KingOrchData>/video_models`. Тот же пул-принцип, что у image:
/// diffusion-файлы разных бандлов имеют разные имена, общие роли
/// (llm/vae/audio_vae/lora_draft) — одинаковые.
pub fn video_bundle_dir_early() -> PathBuf {
    let cfg = load_image_config_early();
    if let Some(p) = &cfg.video_bundle_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    default_video_bundle_dir()
}

/// Однократный перенос лор в единую папку `loras/` (решение юзера 30.10.26).
fn ensure_unified_lora_dir(root: &Path) {
    static DONE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if DONE.get().is_some() {
        return;
    }
    unify_lora_folders(root);
    let _ = DONE.set(());
}

/// Слить содержимое `src` в `dst` rename'ом: нового — переехать, имя занято
/// у ФАЙЛА — пропустить (приоритет у единой папки), имя занято у ПАПКИ —
/// слить рекурсивно на уровень глубже (в легаси-базе могли появиться новые
/// файлы после первого переноса).
fn merge_dir_into(src: &Path, dst: &Path, moved: &mut bool) {
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let target = dst.join(entry.file_name());
        if target.exists() {
            if entry.path().is_dir() && target.is_dir() {
                merge_dir_into(&entry.path(), &target, moved);
            }
            continue;
        }
        if std::fs::rename(entry.path(), &target).is_ok() {
            *moved = true;
        }
    }
}

/// Ядро переноса (без guard'а — герметично для тестов): `image_loras/*` и
/// `video_loras/*` переезжают rename'ом в `loras/<имя>` — тот же том,
/// мгновенно, жёсткие ссылки turbo-лоры DMAD сохраняются. Коллизия папок
/// = слияние содержимого, коллизия файлов = приоритет у единой папки.
/// Опустевшие легаси-папки удаляются. Идемпотентно: нечего переносить — no-op.
fn unify_lora_folders(root: &Path) {
    let unified = root.join("loras");
    if std::fs::create_dir_all(&unified).is_err() {
        return; // честный отказ произойдёт позже в резолве (папки нет)
    }
    for legacy_name in ["image_loras", "video_loras"] {
        let legacy = root.join(legacy_name);
        if !legacy.is_dir() {
            continue;
        }
        let mut moved_any = false;
        merge_dir_into(&legacy, &unified, &mut moved_any);
        // Папка пуста (всё переехало) — убрать легаси-хвост.
        if std::fs::read_dir(&legacy)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
        {
            let _ = std::fs::remove_dir(&legacy);
        }
        if moved_any {
            log::info!(
                "[image-engine] LoRA перенесены: {} → {} (единая папка по базам)",
                legacy.display(),
                unified.display()
            );
        }
    }
}

/// ЕДИНАЯ папка LoRA без AppHandle: явная из конфига (`lora_dir`), иначе
/// `<хранилище KingOrchData>/loras` (с однократным переносом легаси-папок).
/// Оба потребителя — image- и video-ветки — зовут её через свои алиасы.
pub fn lora_dir_early() -> PathBuf {
    let cfg = load_image_config_early();
    if let Some(p) = &cfg.lora_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let root = data_root();
    ensure_unified_lora_dir(&root);
    root.join("loras")
}

/// Папка image-лор: алиас единой `lora_dir_early()` (подпапка базы
/// выбирается резолвером `resolve_lora_specs_for_base`).
pub fn image_lora_dir_early() -> PathBuf {
    lora_dir_early()
}

/// Папка видео-лор: алиас единой `lora_dir_early()` (подпапка базы —
/// `minimax_h3` — выбирается резолвером и спавном `--lora-model-dir`).
pub fn video_lora_dir_early() -> PathBuf {
    lora_dir_early()
}

/// Читает ТОЛЬКО image-ключи (остальное игнорируется serde по умолчанию).
pub fn load_image_config(app: &AppHandle) -> ImageEngineConfig {
    let mut cfg = if let Ok(data) = fs::read_to_string(get_config_path(app)) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        ImageEngineConfig::default()
    };
    normalize_selected_video_bundle(&mut cfg);
    cfg
}

/// Сохраняет image-ключи field-preserving-merge.
pub fn save_image_config(app: &AppHandle, config: &ImageEngineConfig) -> Result<(), String> {
    let path = get_config_path(app);
    let image_value = serde_json::to_value(config).map_err(|e| e.to_string())?;
    ko_json_store::update_json(&path, move |root: &mut serde_json::Value| {
        if !root.is_object() {
            *root = serde_json::json!({});
        }
        if let (serde_json::Value::Object(root_map), serde_json::Value::Object(image_map)) =
            (root, image_value)
        {
            for (k, v) in image_map {
                root_map.insert(k, v);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Новый ключ lora_dir — тот же контракт serde-дефолта; старые ключи
    /// image_lora_dir/video_lora_dir из схемы удалены (читатель их игнорирует).
    #[test]
    fn lora_dir_key_defaults_to_none() {
        let cfg: ImageEngineConfig = serde_json::from_str("{}").expect("пустой JSON");
        assert!(cfg.lora_dir.is_none());
        let legacy: ImageEngineConfig = serde_json::from_str(
            r#"{"image_lora_dir": "C:\\old", "video_lora_dir": "D:\\old2"}"#,
        )
        .expect("легаси-ключи не ломают чтение");
        assert!(legacy.lora_dir.is_none());
    }

    /// Миграция 30.10.26: старые имена q3k-бандлов (fl2va/ref2va) в
    /// selected_video_bundle маппятся на merged uni-бандл; прочие имена
    /// (q4km, отсутствующие) не трогаются.
    #[test]
    fn selected_video_bundle_legacy_names_map_to_uni() {
        for legacy in [
            "minimax_h3_fl2va_pruned-q3k",
            "minimax_h3_ref2va_pruned-q3k",
        ] {
            let mut cfg = ImageEngineConfig {
                selected_video_bundle: Some(legacy.to_string()),
                ..ImageEngineConfig::default()
            };
            super::normalize_selected_video_bundle(&mut cfg);
            assert_eq!(
                cfg.selected_video_bundle.as_deref(),
                Some("minimax_h3_uni-q3k"),
                "{} → uni",
                legacy
            );
        }
        // Не-легаси имена не трогаются.
        for keep in [None, Some("minimax_h3_fl2va_pruned-q4km".to_string()), Some("что-то-чужое".to_string())] {
            let mut cfg = ImageEngineConfig {
                selected_video_bundle: keep.clone(),
                ..ImageEngineConfig::default()
            };
            super::normalize_selected_video_bundle(&mut cfg);
            assert_eq!(cfg.selected_video_bundle, keep, "не-легаси имя сохранено");
        }
    }

    /// Миграция: image_loras/* и video_loras/* переезжают в loras/*,
    /// легаси-папки удаляются, уже существующее в loras не затирается,
    /// повторный запуск — no-op.
    #[test]
    fn unify_lora_folders_moves_legacy_bases_and_is_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "ko_unify_lora_{}_{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let img = root.join("image_loras");
        let vid = root.join("video_loras");
        std::fs::create_dir_all(img.join("qwen")).unwrap();
        std::fs::create_dir_all(vid.join("minimax_h3")).unwrap();
        std::fs::write(img.join("qwen").join("a.safetensors"), b"x").unwrap();
        std::fs::write(vid.join("minimax_h3").join("b.safetensors"), b"x").unwrap();
        // Конфликт: та же база уже есть в единой папке — не затирается.
        std::fs::create_dir_all(root.join("loras").join("krea2")).unwrap();
        std::fs::write(root.join("loras").join("krea2").join("c.safetensors"), b"x").unwrap();

        super::unify_lora_folders(&root);
        assert!(root.join("loras/qwen/a.safetensors").exists());
        assert!(root.join("loras/minimax_h3/b.safetensors").exists());
        assert!(root.join("loras/krea2/c.safetensors").exists(), "своё не тронуто");
        assert!(!img.exists(), "пустой легаси image_loras удалён");
        assert!(!vid.exists(), "пустой легаси video_loras удалён");

        // Повтор — no-op (папки уже нет; конфликтный кейс не переезжает).
        std::fs::create_dir_all(vid.join("minimax_h3")).unwrap();
        std::fs::write(vid.join("minimax_h3").join("d.safetensors"), b"x").unwrap();
        super::unify_lora_folders(&root);
        assert!(root.join("loras/minimax_h3/d.safetensors").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Зеркало тестов `data_dir::normalize` хоста: конвенция одна, ломаться
    /// должна одинаково.
    #[test]
    fn normalize_data_root_appends_suffix() {
        assert_eq!(normalize_data_root("D:\\foo"), PathBuf::from("D:\\foo\\KingOrchData"));
        assert_eq!(normalize_data_root("D:\\foo\\"), PathBuf::from("D:\\foo\\KingOrchData"));
        assert_eq!(normalize_data_root("D:\\foo\\bar"), PathBuf::from("D:\\foo\\bar\\KingOrchData"));
    }

    #[test]
    fn normalize_data_root_keeps_existing_suffix() {
        assert_eq!(
            normalize_data_root("D:\\foo\\KingOrchData"),
            PathBuf::from("D:\\foo\\KingOrchData")
        );
        assert_eq!(
            normalize_data_root("D:\\foo\\kingorchdata\\"),
            PathBuf::from("D:\\foo\\kingorchdata")
        );
    }

    #[test]
    fn normalize_data_root_no_partial_match_and_empty() {
        assert_eq!(
            normalize_data_root("D:\\MyKingOrchData"),
            PathBuf::from("D:\\MyKingOrchData\\KingOrchData")
        );
        assert_eq!(normalize_data_root(""), PathBuf::from("KingOrchData"));
        assert_eq!(normalize_data_root("   "), PathBuf::from("KingOrchData"));
    }
}

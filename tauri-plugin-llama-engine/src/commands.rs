//! Tauri-команды плагина: статус/установка/обновление движка llama.cpp,
//! каталог моделей, CRUD моделей, параметры сэмплинга, докачка mmproj.
//! Вся логика — в `crate::engine`; команды — тонкий слой (перенесён из
//! хоста King Orch `api/llamacpp.rs` + `api/models.rs` без изменения логики).

use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::engine::{self, gpu_detector, llamacpp_installer};

#[derive(Serialize, Clone)]
pub struct EngineStatus {
    pub installed: bool,
    pub tag: Option<String>,
    pub cuda: Option<String>,
    pub path: String,
    pub has_nvidia: bool,
    pub requires_driver_update: bool,
    pub cuda_major: u32,
    pub cuda_minor: u32,
    pub gpu_name: String,
    /// Compute capability вида "12.0" (пусто, если не определена)
    pub compute_cap: String,
    /// Какой вариант движка нужен этой машине по авто-подбору
    pub required_variant: String,
    /// Выбор юзера из конфига: "auto" или конкретный вариант
    pub selected_variant: String,
    /// Реально используемый вариант (auto → сработан по GPU)
    pub resolved_variant: String,
    /// Текущий источник бинарей ("ggml-org" / "beellama")
    pub selected_source: String,
    /// Все источники для дропдауна (с подписями и статусом установки)
    pub available_sources: Vec<crate::engine::sources::SourceInfo>,
    /// Установленные на диске варианты (для текущего источника)
    pub installed_variants: Vec<String>,
    /// Все варианты для дропдауна (с подписями и статусом установки)
    pub available_variants: Vec<llamacpp_installer::VariantInfo>,
    pub message: String,
}

fn engine_dir(app: &AppHandle) -> PathBuf {
    let cfg = engine::load_config(app);
    if let Some(p) = &cfg.llamacpp_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let exe_dir = match std::env::current_exe() {
        Ok(p) => p.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")),
        Err(_) => app
            .path()
            .executable_dir()
            .unwrap_or_else(|_| PathBuf::from(".")),
    };
    llamacpp_installer::default_dir(&exe_dir)
}

/// Хелпер для хоста (не команда): папка движка.
pub fn get_engine_dir(app: &AppHandle) -> PathBuf {
    engine_dir(app)
}

/// Выбор бекенда из конфига юзера ("auto" по умолчанию)
fn preferred_variant(app: &AppHandle) -> String {
    let cfg = engine::load_config(app);
    cfg.engine_variant
        .clone()
        .unwrap_or_else(|| llamacpp_installer::VARIANT_AUTO.to_string())
}

/// Выбор источника бинарей из конфига юзера (дефолт "ggml-org")
fn preferred_source(app: &AppHandle) -> String {
    let cfg = engine::load_config(app);
    engine::sources::resolve_source(cfg.engine_source.as_deref())
}

/// Плавная миграция старых форматов движка:
/// 1) корень → backends/ggml-org/<variant>/;
/// 2) плоский backends/<variant>/ → backends/ggml-org/<variant>/.
fn ensure_migrated(app: &AppHandle) {
    let dir = engine_dir(app);
    if let Ok(Some(variant)) = llamacpp_installer::migrate_legacy_layout(&dir) {
        log::info!("Миграция движка: корень → backends/ggml-org/{}", variant);
    }
    let moved = llamacpp_installer::migrate_sources_layout(&dir);
    if moved > 0 {
        log::info!("Миграция движка: плоский формат → source-level ({} вариантов)", moved);
    }
}

#[tauri::command]
pub fn list_engine_sources(app: AppHandle) -> Vec<engine::sources::SourceInfo> {
    let dir = engine_dir(&app);
    engine::sources::available_sources(&|source_id| {
        !llamacpp_installer::list_installed_variants(&dir, source_id).is_empty()
    })
}

#[tauri::command]
pub async fn set_engine_source(app: AppHandle, source: String) -> Result<EngineStatus, String> {
    if !engine::sources::is_known_source(&source) {
        return Err(format!("Неизвестный источник движка: {}", source));
    }
    let mut cfg = engine::load_config(&app);
    cfg.engine_source = Some(source.clone());
    engine::save_config(&app, &cfg);

    let dir = engine_dir(&app);
    let variant = llamacpp_installer::resolve_variant(Some(&preferred_variant(&app)));
    if !llamacpp_installer::is_installed(&dir, &source, &variant) {
        // Источник не установлен для выбранного варианта — предложить установку
        // через install_llamacpp (юзер жмёт «Установить» в UI).
        log::info!(
            "Источник «{}» выбран; вариант «{}» ещё не установлен.",
            source, variant
        );
    } else {
        log::info!("⚙️ Источник движка: {} (уже установлен — переключение мгновенное).", source);
    }
    Ok(get_engine_status(app))
}

#[tauri::command]
pub fn get_engine_status(app: AppHandle) -> EngineStatus {
    let dir = engine_dir(&app);
    ensure_migrated(&app);

    let gpu = gpu_detector::detect_gpu();
    let source = preferred_source(&app);
    let selected = preferred_variant(&app);
    let resolved = llamacpp_installer::resolve_variant(Some(&selected));
    let meta = llamacpp_installer::installed_meta(&dir, &source, &resolved);
    let installed_variants = llamacpp_installer::list_installed_variants(&dir, &source);
    let available = llamacpp_installer::available_variants(&dir, &source);
    let available_sources = engine::sources::available_sources(&|sid| {
        !llamacpp_installer::list_installed_variants(&dir, sid).is_empty()
    });

    let compute_cap = if gpu.compute_major > 0 {
        format!("{}.{}", gpu.compute_major, gpu.compute_minor)
    } else {
        String::new()
    };
    let required_variant = llamacpp_installer::select_variant();

    let source_label = engine::sources::source_spec(&source)
        .map(|s| s.label)
        .unwrap_or_else(|| source.clone());

    let message = if let Some(m) = &meta {
        format!(
            "Установлен: {} (источник: {}, вариант: {})",
            m.tag,
            source_label,
            llamacpp_installer::variant_label(&m.variant)
        )
    } else if installed_variants.is_empty() {
        if gpu.has_nvidia {
            format!(
                "Движок «{}» не установлен — инференс недоступен. Установите движок ниже.",
                source_label
            )
        } else {
            gpu_detector::describe_gpu(&gpu)
        }
    } else {
        format!(
            "Выбран вариант «{}», но он ещё не установлен (установлены: {}). Нажмите «Установить».",
            llamacpp_installer::variant_label(&resolved),
            installed_variants
                .iter()
                .map(|v| llamacpp_installer::variant_label(v))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    EngineStatus {
        installed: meta.is_some(),
        tag: meta.as_ref().map(|m| m.tag.clone()),
        cuda: meta.as_ref().map(|m| m.variant.clone()),
        path: dir.to_string_lossy().to_string(),
        has_nvidia: gpu.has_nvidia,
        requires_driver_update: gpu_detector::requires_driver_update(&gpu),
        cuda_major: gpu.cuda_major,
        cuda_minor: gpu.cuda_minor,
        gpu_name: gpu.gpu_name,
        compute_cap,
        required_variant,
        selected_variant: selected,
        resolved_variant: resolved,
        selected_source: source,
        available_sources,
        installed_variants,
        available_variants: available,
        message,
    }
}

#[tauri::command]
pub async fn install_llamacpp(app: AppHandle) -> Result<EngineStatus, String> {
    let log_cb = move |msg: String| {
        log::info!("[ENGINE] {}", msg);
    };

    let dir = engine_dir(&app);
    ensure_migrated(&app);

    let gpu = gpu_detector::detect_gpu();
    log_cb(gpu_detector::describe_gpu(&gpu));
    let source = preferred_source(&app);
    let selected = preferred_variant(&app);
    let variant = llamacpp_installer::resolve_variant(Some(&selected));
    log_cb(format!(
        "Источник: {}, вариант бекенда: {} ({})",
        source,
        variant,
        llamacpp_installer::variant_label(&variant)
    ));

    let _meta = llamacpp_installer::install(&dir, &source, &variant, &log_cb).await?;
    log_cb(format!("📂 Папка движка: {}", dir.display()));

    Ok(get_engine_status(app))
}

/// Смена бекенда: сохраняет выбор юзера в конфиг; если вариант ещё не установлен —
/// скачивает его.
#[tauri::command]
pub async fn set_engine_variant(app: AppHandle, variant: String) -> Result<EngineStatus, String> {
    let valid = variant == llamacpp_installer::VARIANT_AUTO
        || llamacpp_installer::is_known_variant(&variant);
    if !valid {
        return Err(format!("Неизвестный вариант бекенда: {}", variant));
    }

    let mut cfg = engine::load_config(&app);
    cfg.engine_variant = if variant == llamacpp_installer::VARIANT_AUTO {
        None
    } else {
        Some(variant.clone())
    };
    engine::save_config(&app, &cfg);

    let dir = engine_dir(&app);
    let source = preferred_source(&app);
    let resolved = llamacpp_installer::resolve_variant(Some(&variant));
    if !llamacpp_installer::is_installed(&dir, &source, &resolved) {
        install_llamacpp(app.clone()).await?;
    } else {
        let log_cb = move |msg: String| {
            log::info!("[ENGINE] {}", msg);
        };
        log_cb(format!(
            "⚙️ Выбран бекенд: {} (уже установлен — переключение мгновенное).",
            llamacpp_installer::variant_label(&resolved)
        ));
    }

    Ok(get_engine_status(app))
}

#[tauri::command]
pub async fn check_engine_update(app: AppHandle) -> Result<Option<String>, String> {
    let dir = engine_dir(&app);
    let source = preferred_source(&app);
    let variant = llamacpp_installer::resolve_variant(Some(&preferred_variant(&app)));
    let log_cb = move |msg: String| {
        log::info!("[ENGINE] {}", msg);
    };
    llamacpp_installer::check_update(&dir, &source, &variant, &log_cb).await
}

/// Обновление = переустановка выбранного варианта.
#[tauri::command]
pub async fn install_engine_update(app: AppHandle) -> Result<EngineStatus, String> {
    install_llamacpp(app).await
}

#[tauri::command]
pub fn remove_engine(app: AppHandle) -> Result<EngineStatus, String> {
    let dir = engine_dir(&app);
    let source = preferred_source(&app);
    let variant = llamacpp_installer::resolve_variant(Some(&preferred_variant(&app)));
    let log_cb = move |msg: String| {
        log::info!("[ENGINE] {}", msg);
    };
    llamacpp_installer::remove(&dir, &source, &variant, &log_cb)?;
    Ok(get_engine_status(app))
}

#[tauri::command]
pub fn set_engine_dir(app: AppHandle, path: String) -> Result<EngineStatus, String> {
    let mut cfg = engine::load_config(&app);
    cfg.llamacpp_dir = Some(path);
    engine::save_config(&app, &cfg);
    Ok(get_engine_status(app))
}

// ─────────────────────────── Модели и каталог ──────────────────────────

#[derive(Serialize)]
pub struct AutoDownloadInfo {
    pub model_name: String,
    pub model_url: String,
    pub size_gb: Option<String>,
    pub save_path: String,
    pub free_space_gb: u64,
    pub drive_letter: String,
}

#[tauri::command]
pub fn get_auto_download_info(app: AppHandle) -> Result<AutoDownloadInfo, String> {
    let catalog = engine::load_catalog(&app);
    let default_entry = catalog
        .iter()
        .find(|e| e.is_default)
        .ok_or_else(|| "В каталоге не найдена модель по умолчанию".to_string())?;

    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut best_drive: Option<(String, u64)> = None;

    for disk in &disks {
        let mount = disk.mount_point().to_string_lossy().to_string();
        let available = disk.available_space();
        if mount.len() >= 2 && mount.as_bytes()[1] == b':' {
            let drive_letter = mount[..1].to_uppercase();
            if let Some((_, best_avail)) = best_drive {
                if available > best_avail {
                    best_drive = Some((drive_letter, available));
                }
            } else {
                best_drive = Some((drive_letter, available));
            }
        }
    }

    let (drive_letter, free_space) =
        best_drive.ok_or_else(|| "Не найден ни один диск для сохранения модели".to_string())?;

    let save_dir = format!("{}:\\llm_local_ai_models", drive_letter);
    let filename = default_entry
        .download_url
        .split('/')
        .last()
        .and_then(|s| s.split('?').next())
        .unwrap_or(&format!("{}.gguf", default_entry.name))
        .to_string();
    let save_path = format!("{}\\{}", save_dir, filename);

    Ok(AutoDownloadInfo {
        model_name: default_entry.name.clone(),
        model_url: default_entry.download_url.clone(),
        size_gb: default_entry.size_gb.clone(),
        save_path,
        free_space_gb: free_space / (1024 * 1024 * 1024),
        drive_letter,
    })
}

#[tauri::command]
pub async fn auto_download_default_model(app: AppHandle, save_path: String) -> Result<(), String> {
    let catalog = engine::load_catalog(&app);
    let default_entry = catalog
        .iter()
        .find(|e| e.is_default)
        .ok_or_else(|| "В каталоге не найдена модель по умолчанию".to_string())?;

    let parent_dir = std::path::Path::new(&save_path)
        .parent()
        .ok_or_else(|| "Неверный путь сохранения".to_string())?;
    std::fs::create_dir_all(parent_dir)
        .map_err(|e| format!("Не удалось создать директорию: {}", e))?;

    engine::downloader::download_model(
        app.clone(),
        default_entry.download_url.clone(),
        save_path.clone(),
    )
    .await?;

    let mut mmproj_saved: Option<String> = None;
    if let Some(mmp_url) = &default_entry.mmproj_url {
        let mmp_name = mmp_url
            .split('/')
            .last()
            .and_then(|s| s.split('?').next())
            .unwrap_or("mmproj.gguf")
            .to_string();
        let mmp_path = format!(
            "{}{}{}",
            parent_dir.display(),
            std::path::MAIN_SEPARATOR,
            mmp_name
        );
        engine::downloader::download_model(app.clone(), mmp_url.clone(), mmp_path.clone()).await?;
        mmproj_saved = Some(mmp_path);
    }

    let models_dir = parent_dir.to_str().map(|d| d.to_string());

    let mut cfg = engine::load_config(&app);
    if !cfg.models.contains(&save_path) {
        cfg.models.push(save_path.clone());
    }
    cfg.last_model = Some(save_path.clone());
    cfg.models_dir = models_dir;
    if let Some(mp) = mmproj_saved {
        cfg.mmproj_files.insert(save_path.clone(), mp);
    }
    cfg.model_meta.insert(
        save_path.clone(),
        engine::ModelMeta {
            uncen: default_entry.uncen.unwrap_or(false),
            vision: default_entry.vision.unwrap_or(false),
            audio: default_entry.audio.unwrap_or(false),
        },
    );
    engine::save_config(&app, &cfg);

    Ok(())
}

#[tauri::command]
pub fn get_models_catalog(app: AppHandle) -> Vec<engine::CatalogEntry> {
    engine::load_catalog(&app)
}

/// Текущее движковое состояние конфига: список моделей, last_model, mmproj_files,
/// model_meta, движковые пути. Используется самодостаточными веб-компонентами
/// плагина (см. <llama-models-panel>) и хостами без собственного get_config.
#[tauri::command]
pub fn get_engine_config(app: AppHandle) -> engine::EngineConfig {
    engine::load_config(&app)
}

#[tauri::command]
pub fn get_model_params(app: AppHandle, model_path: String) -> engine::ModelParams {
    let mut cfg = engine::load_config(&app);
    if let Some(params) = cfg.model_params.get(&model_path) {
        return params.clone();
    }

    let mut params = engine::ModelParams::default();

    // УМНОЕ ЧТЕНИЕ (Ground Truth): перезаписываем настройки тем, что ВШИТО в .gguf.
    if let Some(temp) = engine::extract_f32_from_gguf(&model_path, "tokenizer.ggml.temp") {
        params.temperature = temp;
    }
    if let Some(top_k) = engine::extract_u32_from_gguf(&model_path, "tokenizer.ggml.top_k") {
        params.top_k = top_k;
    }
    if let Some(top_p) = engine::extract_f32_from_gguf(&model_path, "tokenizer.ggml.top_p") {
        params.top_p = top_p;
    }
    if let Some(min_p) = engine::extract_f32_from_gguf(&model_path, "tokenizer.ggml.min_p") {
        params.min_p = min_p;
    }
    if let Some(rep_pen) =
        engine::extract_f32_from_gguf(&model_path, "tokenizer.ggml.repetition_penalty")
    {
        params.repetition_penalty = rep_pen;
    }

    cfg.model_params.insert(model_path.clone(), params.clone());
    engine::save_config(&app, &cfg);

    params
}

#[tauri::command]
pub fn set_model_params(app: AppHandle, model_path: String, params: engine::ModelParams) {
    let mut cfg = engine::load_config(&app);
    cfg.model_params.insert(model_path, params);
    engine::save_config(&app, &cfg);
}

#[tauri::command]
pub fn reset_model_params(app: AppHandle, model_path: String) -> engine::ModelParams {
    let mut cfg = engine::load_config(&app);
    cfg.model_params.remove(&model_path);
    engine::save_config(&app, &cfg);
    get_model_params(app, model_path) // Пересчитает параметры из GGUF заново
}

/// Результат добавления модели: конфиг + опциональное предупреждение о том,
/// что GGUF-файл повреждён (модель всё равно добавляется, но не заработает).
#[derive(Serialize)]
pub struct AddModelOutcome {
    pub config: engine::EngineConfig,
    pub warning: Option<String>,
}

#[tauri::command]
pub fn add_model(
    app: AppHandle,
    path: String,
    flags: Option<engine::ModelMeta>,
) -> Result<AddModelOutcome, String> {
    let meta = std::fs::metadata(&path)
        .map_err(|e| format!("Файл модели не найден: {}", e))?;
    if meta.len() < 1024 * 1024 {
        return Err(format!(
            "Файл слишком маленький ({} байт) — это не GGUF-модель",
            meta.len()
        ));
    }

    let mut warning = None;
    if let Err(msg) = engine::llm_gguf::validate_gguf(&path) {
        let w = format!("Файл модели повреждён.\n{}", msg);
        log::warn!("add_model: {}", w);
        warning = Some(w);
    }

    if engine::is_mmproj_file(&path) {
        let catalog = engine::load_catalog(&app);
        if let Some(sibling) = engine::find_sibling_llm_for_mmproj(&path, &catalog) {
            let warning = format!(
                "Файл «{}» — это mmproj (проектор для изображений), он не является \
                 языковой моделью и не может работать как LLM.\n\nВместо него добавлена \
                 модель «{}». Если нужен именно проектор — он автоматически подхватится \
                 для моделей с поддержкой изображений из той же папки.",
                std::path::Path::new(&path)
                    .file_name()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_else(|| path.clone().into()),
                std::path::Path::new(&sibling)
                    .file_name()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default(),
            );
            return add_model_impl(app, sibling, flags, Some(warning));
        }
        return Err(format!(
            "Файл «{}» — это mmproj (мультимодальный проектор для изображений), \
             а не языковая модель. Запускать его как LLM нельзя: движок не сможет \
             обработать промпт.\n\nРядом не найдена основная LLM (нужен файл из \
             каталога или единственный другой .gguf в той же папке). Добавьте \
             языковую модель — проектор подхватится автоматически.",
            path
        ));
    }

    add_model_impl(app, path, flags, warning)
}

fn add_model_impl(
    app: AppHandle,
    path: String,
    flags: Option<engine::ModelMeta>,
    warning: Option<String>,
) -> Result<AddModelOutcome, String> {
    let mut cfg = engine::load_config(&app);
    if !cfg.models.contains(&path) {
        cfg.models.push(path.clone());
    }
    cfg.last_model = Some(path.clone());

    if let Some(mmp) = engine::auto_detect_mmproj(&path) {
        cfg.mmproj_files.insert(path.clone(), mmp);
    }

    if let Some(f) = flags {
        cfg.model_meta.insert(path.clone(), f);
    }

    engine::save_config(&app, &cfg);
    Ok(AddModelOutcome {
        config: cfg,
        warning,
    })
}

#[tauri::command]
pub fn remove_model(app: AppHandle, path: String) -> Result<engine::EngineConfig, String> {
    let mut cfg = engine::load_config(&app);
    cfg.models.retain(|m| m != &path);
    if cfg.last_model.as_deref() == Some(path.as_str()) {
        cfg.last_model = None;
    }
    cfg.model_params.remove(&path);
    cfg.mmproj_files.remove(&path);
    engine::save_config(&app, &cfg);
    Ok(cfg)
}

#[tauri::command]
pub fn delete_model_file(app: AppHandle, path: String) -> Result<engine::EngineConfig, String> {
    match std::fs::remove_file(&path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Не удалось удалить файл «{}»: {}", path, e)),
    }
    let mut cfg = engine::load_config(&app);
    cfg.models.retain(|m| m != &path);
    if cfg.last_model.as_deref() == Some(path.as_str()) {
        cfg.last_model = None;
    }
    cfg.model_params.remove(&path);
    cfg.mmproj_files.remove(&path);
    cfg.model_meta.remove(&path);
    engine::save_config(&app, &cfg);
    Ok(cfg)
}

#[tauri::command]
pub fn get_mmproj_path(app: AppHandle, model_path: String) -> Option<String> {
    let cfg = engine::load_config(&app);
    if let Some(path) = cfg.mmproj_files.get(&model_path) {
        if std::path::Path::new(path).exists() {
            return Some(path.clone());
        }
    }
    if let Some(mmp) = engine::auto_detect_mmproj(&model_path) {
        let mut cfg = engine::load_config(&app);
        cfg.mmproj_files.insert(model_path.clone(), mmp.clone());
        engine::save_config(&app, &cfg);
        return Some(mmp);
    }
    None
}

/// Возвращает актуальные возможности модели (vision/audio/uncen), читая их
/// напрямую из каталога (живьём) и наличия mmproj.
#[derive(Serialize)]
pub struct ModelCapabilities {
    pub vision: bool,
    pub audio: bool,
    pub uncen: bool,
}

#[tauri::command]
pub fn get_model_capabilities(app: AppHandle, model_path: String) -> ModelCapabilities {
    let catalog = engine::load_catalog(&app);
    let cfg = engine::load_config(&app);
    let mut caps = ModelCapabilities {
        vision: false,
        audio: false,
        uncen: false,
    };
    if let Some(entry) = engine::find_catalog_entry_for_model(&catalog, &model_path) {
        caps.vision = entry.vision.unwrap_or(false);
        caps.audio = entry.audio.unwrap_or(false);
        caps.uncen = entry.uncen.unwrap_or(false);
    }
    if cfg.mmproj_files.contains_key(&model_path) {
        caps.vision = true;
    }
    caps
}

/// Возвращает актуальные возможности (vision/audio/uncen) для ВСЕХ
/// установленных моделей из каталога и наличия mmproj.
#[tauri::command]
pub fn get_all_capabilities(app: AppHandle) -> std::collections::HashMap<String, ModelCapabilities> {
    use std::collections::HashMap;
    let catalog = engine::load_catalog(&app);
    let cfg = engine::load_config(&app);
    let mut map: HashMap<String, ModelCapabilities> = HashMap::new();
    for model_path in &cfg.models {
        let mut caps = ModelCapabilities {
            vision: false,
            audio: false,
            uncen: false,
        };
        if let Some(entry) = engine::find_catalog_entry_for_model(&catalog, model_path) {
            caps.vision = entry.vision.unwrap_or(false);
            caps.audio = entry.audio.unwrap_or(false);
            caps.uncen = entry.uncen.unwrap_or(false);
        }
        if cfg.mmproj_files.contains_key(model_path) {
            caps.vision = true;
        }
        map.insert(model_path.clone(), caps);
    }
    map
}

/// Возвращает путь к mmproj для модели, докачивая его по каталогу, если файл
/// не найден (нужно для кнопки «скрепка» у моделей, добавленных вручную).
#[tauri::command]
pub async fn ensure_mmproj(app: AppHandle, model_path: String) -> Result<Option<String>, String> {
    engine::ensure_mmproj_for_model(&app, &model_path).await
}

/// Для Live-превью: прогноз потребления VRAM (модель + KV-кэш) + факты текущей
/// занятости GPU через NVML. `vram_used_mb`/`vram_total_mb` равны 0, если NVML
/// недоступен (нет NVIDIA GPU/драйвера) — фронт покажет числитель без знаменателя.
#[derive(Serialize)]
pub struct PromptMemoryInfo {
    /// Оценка «модель + KV + буферы» на эффективный контекст, МБ.
    pub need_mb: f64,
    /// Занято VRAM сейчас (NVML, device 0, ВСЕ процессы), МБ.
    pub vram_used_mb: f64,
    /// Общий объём VRAM устройства (NVML, device 0), точные MiB.
    pub vram_total_mb: f64,
}

/// Live-превью счётчика токенов: прогноз VRAM на ЭФФЕКТИВНЫЙ контекст
/// (промпт + запас генерации + резерв, но не больше лимита) + факты NVML.
/// Единый источник оценки — `engine::vram_estimate` (то же, что в pre-flight
/// запуска и пик-линии после генерации). KV-spec резолвится по ТЕКУЩЕМУ
/// `engine_source` (BeeLlama → kvarn5/kvarn4+tail); входящие `kv_quant_*`
/// учитываются только для источников без своего runtime.
#[tauri::command]
pub fn estimate_prompt_memory(
    model_path: String,
    context_size: u32,
    kv_quant_keys: bool,
    kv_quant_values: bool,
    prompt_tokens: u32,
    max_gen: u32,
) -> Result<PromptMemoryInfo, String> {
    const CTX_RESERVE: u32 = 128;
    let effective_ctx = (prompt_tokens + max_gen + CTX_RESERVE).min(context_size);
    let spec = crate::engine::vram_estimate::current_kv_spec(kv_quant_keys, kv_quant_values);
    let est = crate::engine::vram_estimate::estimate_vram_with_spec(
        &model_path,
        effective_ctx,
        &spec,
    );
    let (used_mb, total_mb) = match nvml_wrapper::Nvml::init() {
        Ok(nvml) => match nvml.device_by_index(0).and_then(|d| d.memory_info()) {
            Ok(mem) => (
                mem.used as f64 / (1024.0 * 1024.0),
                mem.total as f64 / (1024.0 * 1024.0),
            ),
            Err(_) => (0.0, 0.0),
        },
        Err(_) => (0.0, 0.0),
    };
    Ok(PromptMemoryInfo {
        need_mb: est.total_mb,
        vram_used_mb: used_mb,
        vram_total_mb: total_mb,
    })
}
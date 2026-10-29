//! Tauri-команды плагина: статус/установка движка sd.cpp, каталог бандлов,
//! скачивание бандла, preflight, генерация/редактирование.
//! Команды — тонкий слой, вся логика в `crate::engine`.

use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

use crate::engine::{self, file_to_b64, sdcpp_installer};

#[derive(Serialize, Clone)]
pub struct ImageEngineStatus {
    pub installed: bool,
    pub tag: Option<String>,
    pub variant: Option<String>,
    pub path: String,
    pub has_nvidia: bool,
    pub gpu_name: String,
    pub required_variant: String,
    pub selected_variant: String,
    pub resolved_variant: String,
    pub installed_variants: Vec<String>,
    pub available_variants: Vec<sdcpp_installer::VariantInfo>,
    pub message: String,
}

fn exe_dir(app: &AppHandle) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| {
            app.path()
                .executable_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
        })
}

fn engine_dir(app: &AppHandle) -> PathBuf {
    let cfg = engine::load_image_config(app);
    if let Some(p) = &cfg.sdcpp_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    sdcpp_installer::default_dir(&exe_dir(app))
}

/// Хелпер для хоста (не команда): папка движка.
pub fn get_image_engine_dir(app: &AppHandle) -> PathBuf {
    engine_dir(app)
}

/// Папка бандлов: явная из конфига, иначе `<хранилище KingOrchData>/image_models`.
/// Одна папка-пул для ВСЕХ бандлов (имена общих ролей совпадают → downloader
/// пропускает существующее); per-bundle подпапки убраны 25.10.2026 —
/// они порождали перекачку общих файлов и «3 файла есть, 1 нет» при
/// переключении активного бандла.
fn bundle_dir(app: &AppHandle) -> PathBuf {
    let cfg = engine::load_image_config(app);
    if let Some(p) = &cfg.image_bundle_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    engine::default_image_bundle_dir()
}

fn preferred_variant(app: &AppHandle) -> Option<String> {
    engine::load_image_config(app).image_engine_variant
}

fn gpu_facts() -> (bool, String) {
    match nvml_wrapper::Nvml::init() {
        Ok(nvml) => match nvml.device_by_index(0) {
            Ok(d) => (true, d.name().unwrap_or_default()),
            Err(_) => (false, String::new()),
        },
        Err(_) => (false, String::new()),
    }
}

#[tauri::command]
pub fn get_image_engine_status(app: AppHandle) -> ImageEngineStatus {
    let dir = engine_dir(&app);
    let (has_nvidia, gpu_name) = gpu_facts();
    let source = "leejet";
    let selected = preferred_variant(&app).unwrap_or_else(|| sdcpp_installer::VARIANT_AUTO.to_string());
    let resolved = sdcpp_installer::resolve_variant(Some(&selected));
    let meta = sdcpp_installer::installed_meta(&dir, source, &resolved);
    let installed_variants = sdcpp_installer::list_installed_variants(&dir, source);
    let available = sdcpp_installer::available_variants(&dir, source);
    let required_variant = sdcpp_installer::select_variant();

    let message = if let Some(m) = &meta {
        format!("Установлен: {} (вариант: {})", m.tag, sdcpp_installer::variant_label(&m.variant))
    } else if installed_variants.is_empty() {
        "Движок изображений не установлен — генерация недоступна. Установите движок ниже.".to_string()
    } else {
        format!(
            "Выбран вариант «{}», но он ещё не установлен. Нажмите «Установить».",
            sdcpp_installer::variant_label(&resolved)
        )
    };

    ImageEngineStatus {
        installed: meta.is_some(),
        tag: meta.as_ref().map(|m| m.tag.clone()),
        variant: meta.as_ref().map(|m| m.variant.clone()),
        path: dir.to_string_lossy().to_string(),
        has_nvidia,
        gpu_name,
        required_variant,
        selected_variant: selected,
        resolved_variant: resolved,
        installed_variants,
        available_variants: available,
        message,
    }
}

#[tauri::command]
pub async fn install_image_engine(app: AppHandle) -> Result<ImageEngineStatus, String> {
    let on_log = |msg: String| log::info!("[IMG-ENGINE] {}", msg);
    let dir = engine_dir(&app);
    let selected = preferred_variant(&app).unwrap_or_else(|| sdcpp_installer::VARIANT_AUTO.to_string());
    let variant = sdcpp_installer::resolve_variant(Some(&selected));
    on_log(format!("Вариант бекенда: {} ({})", variant, sdcpp_installer::variant_label(&variant)));
    let _meta = sdcpp_installer::install(&dir, "leejet", &variant, &on_log).await?;
    on_log(format!("📂 Папка движка: {}", dir.display()));
    Ok(get_image_engine_status(app))
}

#[tauri::command]
pub async fn set_image_engine_variant(app: AppHandle, variant: String) -> Result<ImageEngineStatus, String> {
    if variant != sdcpp_installer::VARIANT_AUTO && !sdcpp_installer::is_known_variant(&variant) {
        return Err(format!("Неизвестный вариант бекенда: {}", variant));
    }
    let mut cfg = engine::load_image_config(&app);
    cfg.image_engine_variant = if variant == sdcpp_installer::VARIANT_AUTO { None } else { Some(variant.clone()) };
    engine::save_image_config(&app, &cfg)?;

    let dir = engine_dir(&app);
    let resolved = sdcpp_installer::resolve_variant(Some(&variant));
    if !sdcpp_installer::is_installed(&dir, "leejet", &resolved) {
        install_image_engine(app.clone()).await?;
    }
    Ok(get_image_engine_status(app))
}

#[tauri::command]
pub async fn check_image_engine_update(app: AppHandle) -> Result<Option<String>, String> {
    let dir = engine_dir(&app);
    let selected = preferred_variant(&app).unwrap_or_else(|| sdcpp_installer::VARIANT_AUTO.to_string());
    let variant = sdcpp_installer::resolve_variant(Some(&selected));
    let on_log = |msg: String| log::info!("[IMG-ENGINE] {}", msg);
    sdcpp_installer::check_update(&dir, "leejet", &variant, &on_log).await
}

#[tauri::command]
pub async fn install_image_engine_update(app: AppHandle) -> Result<ImageEngineStatus, String> {
    install_image_engine(app).await
}

#[tauri::command]
pub fn remove_image_engine(app: AppHandle) -> Result<ImageEngineStatus, String> {
    let dir = engine_dir(&app);
    let selected = preferred_variant(&app).unwrap_or_else(|| sdcpp_installer::VARIANT_AUTO.to_string());
    let variant = sdcpp_installer::resolve_variant(Some(&selected));
    let on_log = |msg: String| log::info!("[IMG-ENGINE] {}", msg);
    sdcpp_installer::remove(&dir, "leejet", &variant, &on_log)?;
    Ok(get_image_engine_status(app))
}

#[tauri::command]
pub fn set_image_engine_dir(app: AppHandle, path: String) -> Result<ImageEngineStatus, String> {
    let mut cfg = engine::load_image_config(&app);
    cfg.sdcpp_dir = Some(path);
    engine::save_image_config(&app, &cfg)?;
    Ok(get_image_engine_status(app))
}

#[tauri::command]
pub fn get_image_models_catalog() -> Vec<engine::ImageBundleEntry> {
    engine::load_image_catalog()
}

// ─────────────────────────── Каталог бандлов ──────────────────────────

#[derive(Serialize, Clone)]
pub struct BundleFileInfo {
    pub role: String,
    pub filename: String,
    pub size_bytes: Option<u64>,
    pub save_path: String,
    pub exists: bool,
}

#[derive(Serialize, Clone)]
pub struct ImageBundleInfo {
    pub bundle_name: String,
    pub label: String,
    pub note: String,
    pub files: Vec<BundleFileInfo>,
    pub total_bytes: u64,
    pub size_gb: Option<String>,
    pub vram_fast_gb: Option<f32>,
    pub vram_min_gb: Option<f32>,
    pub ram_min_gb: Option<f32>,
    pub save_dir: String,
    pub free_space_gb: u64,
    pub fully_downloaded: bool,
}

fn free_space_gb_for(path: &Path) -> u64 {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let ps = path.to_string_lossy().to_lowercase();
    let mut best: Option<u64> = None;
    for disk in &disks {
        let mount = disk.mount_point().to_string_lossy().to_lowercase();
        if !mount.is_empty() && ps.starts_with(mount.as_str()) {
            best = Some(disk.available_space());
            break;
        }
    }
    let avail = best.unwrap_or_else(|| {
        disks.iter().map(|d| d.available_space()).max().unwrap_or(0)
    });
    avail / (1024 * 1024 * 1024)
}

/// Entry бандла для операции: явное имя → из каталога; без имени →
/// активный (cfg.selected_bundle) → дефолт. Ошибка — честная, что где не так.
pub fn bundle_entry(
    app: &AppHandle,
    name: Option<&str>,
) -> Result<engine::ImageBundleEntry, String> {
    if let Some(n) = name.filter(|n| !n.is_empty()) {
        return engine::bundle_by_name(n)
            .ok_or_else(|| format!("Бандл «{}» отсутствует в каталоге плагина.", n));
    }
    if let Some(ref sel) = engine::load_image_config(app).selected_bundle {
        if !sel.is_empty() {
            return engine::bundle_by_name(sel).ok_or_else(|| {
                format!(
                    "Активный бандл «{}» отсутствует в каталоге — выберите другой в настройках image-engine.",
                    sel
                )
            });
        }
    }
    engine::default_bundle_entry()
        .ok_or_else(|| "В каталоге нет бандла по умолчанию".to_string())
}

#[tauri::command]
pub fn get_image_bundle_info(app: AppHandle, bundle_name: Option<String>) -> Result<ImageBundleInfo, String> {
    let entry = bundle_entry(&app, bundle_name.as_deref())?;
    let save_dir = bundle_dir(&app);
    let mut files = Vec::with_capacity(entry.files.len());
    let mut fully = true;
    for f in &entry.files {
        let save_path = save_dir.join(&f.filename);
        let exists = save_path.exists();
        if !exists {
            fully = false;
        }
        files.push(BundleFileInfo {
            role: f.role.clone(),
            filename: f.filename.clone(),
            size_bytes: f.size_bytes,
            save_path: save_path.to_string_lossy().to_string(),
            exists,
        });
    }
    let total_bytes: u64 = entry.files.iter().map(|f| f.size_bytes.unwrap_or(0)).sum();
    Ok(ImageBundleInfo {
        bundle_name: entry.name.clone(),
        label: entry.label.clone(),
        note: entry.note.clone(),
        files,
        total_bytes,
        size_gb: entry.size_gb.clone(),
        vram_fast_gb: entry.vram_fast_gb,
        vram_min_gb: entry.vram_min_gb,
        ram_min_gb: entry.ram_min_gb,
        save_dir: save_dir.to_string_lossy().to_string(),
        free_space_gb: free_space_gb_for(&save_dir),
        fully_downloaded: fully,
    })
}

#[derive(Serialize, Clone)]
pub struct ImageBundleValidate {
    pub valid: bool,
    pub dir: String,
    pub present: Vec<String>,
    pub missing: Vec<String>,
}

fn validate_bundle_dir(entry: &engine::ImageBundleEntry, dir: &Path) -> ImageBundleValidate {
    let mut present = Vec::new();
    let mut missing = Vec::new();
    for f in &entry.files {
        if dir.join(&f.filename).exists() {
            present.push(format!("{} ({})", f.filename, f.role));
        } else {
            missing.push(format!("{} ({})", f.filename, f.role));
        }
    }
    ImageBundleValidate {
        valid: missing.is_empty(),
        dir: dir.to_string_lossy().to_string(),
        present,
        missing,
    }
}

#[tauri::command]
pub fn validate_image_bundle_dir(
    path: String,
    bundle_name: Option<String>,
) -> Result<ImageBundleValidate, String> {
    let entry_opt = match bundle_name.as_deref() {
        Some(n) if !n.is_empty() => engine::bundle_by_name(n),
        _ => engine::default_bundle_entry(),
    };
    let entry = entry_opt.ok_or_else(|| match bundle_name.as_deref() {
        Some(n) => format!("Бандл «{}» отсутствует в каталоге плагина.", n),
        None => "В каталоге нет бандла по умолчанию".to_string(),
    })?;
    Ok(validate_bundle_dir(&entry, Path::new(&path)))
}

#[tauri::command]
pub fn set_selected_image_bundle(
    app: AppHandle,
    bundle_name: String,
) -> Result<ImageBundleInfo, String> {
    if bundle_name.trim().is_empty() {
        let mut cfg = engine::load_image_config(&app);
        cfg.selected_bundle = None;
        engine::save_image_config(&app, &cfg)?;
        return get_image_bundle_info(app, None);
    }
    let entry = engine::bundle_by_name(&bundle_name).ok_or_else(|| {
        format!("Бандл «{}» отсутствует в каталоге плагина.", bundle_name)
    })?;
    // Гейт активации (решение юзера 25.10.2026): неполный бандл нельзя
    // сделать активным — иначе generate_image честно упадёт с «скачан
    // не полностью». Ошибка называет недостающие файлы.
    let dir = bundle_dir(&app);
    let missing: Vec<&str> = entry
        .files
        .iter()
        .filter(|f| !dir.join(&f.filename).exists())
        .map(|f| f.filename.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "Бандл «{}» скачан не полностью: нет файлов {}. Сначала нажмите «Скачать набор» / «Докачать файлы» — активация недокачанного набора запрещена.",
            entry.name,
            missing.join(", ")
        ));
    }
    let mut cfg = engine::load_image_config(&app);
    cfg.selected_bundle = Some(bundle_name.clone());
    engine::save_image_config(&app, &cfg)?;
    log::info!("[IMG] Активный бандл: {}", entry.name);
    image_bundle_info_for(&app, &entry)
}

/// Сборка ImageBundleInfo для конкретного entry (общая для info/set_selected).
fn image_bundle_info_for(app: &AppHandle, entry: &engine::ImageBundleEntry) -> Result<ImageBundleInfo, String> {
    let save_dir = bundle_dir(app);
    let mut files = Vec::with_capacity(entry.files.len());
    let mut fully = true;
    for f in &entry.files {
        let save_path = save_dir.join(&f.filename);
        let exists = save_path.exists();
        if !exists {
            fully = false;
        }
        files.push(BundleFileInfo {
            role: f.role.clone(),
            filename: f.filename.clone(),
            size_bytes: f.size_bytes,
            save_path: save_path.to_string_lossy().to_string(),
            exists,
        });
    }
    let total_bytes: u64 = entry.files.iter().map(|f| f.size_bytes.unwrap_or(0)).sum();
    Ok(ImageBundleInfo {
        bundle_name: entry.name.clone(),
        label: entry.label.clone(),
        note: entry.note.clone(),
        files,
        total_bytes,
        size_gb: entry.size_gb.clone(),
        vram_fast_gb: entry.vram_fast_gb,
        vram_min_gb: entry.vram_min_gb,
        ram_min_gb: entry.ram_min_gb,
        save_dir: save_dir.to_string_lossy().to_string(),
        free_space_gb: free_space_gb_for(&save_dir),
        fully_downloaded: fully,
    })
}

#[tauri::command]
pub fn set_image_bundle_dir(app: AppHandle, path: String) -> Result<ImageBundleInfo, String> {
    let p = Path::new(&path);
    if !p.exists() {
        std::fs::create_dir_all(p)
            .map_err(|e| format!("Не удалось создать папку {}: {}", p.display(), e))?;
    }
    let mut cfg = engine::load_image_config(&app);
    cfg.image_bundle_dir = Some(path);
    engine::save_image_config(&app, &cfg)?;
    get_image_bundle_info(app, None)
}

#[tauri::command]
pub async fn download_image_bundle(
    app: AppHandle,
    save_dir: String,
    bundle_name: Option<String>,
) -> Result<ImageBundleInfo, String> {
    let entry = bundle_entry(&app, bundle_name.as_deref())?;
    let dir = PathBuf::from(&save_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Не удалось создать папку {}: {}", dir.display(), e))?;
    let on_log = |msg: String| log::info!("[IMG-DL] {}", msg);
    for f in &entry.files {
        let dest = dir.join(&f.filename);
        if dest.exists() {
            if let Some(expected) = f.size_bytes {
                if let Ok(meta) = std::fs::metadata(&dest) {
                    if meta.len() == expected {
                        on_log(format!("Уже скачан, пропуск: {}", f.filename));
                        continue;
                    }
                }
            } else {
                on_log(format!("Уже скачан, пропуск: {}", f.filename));
                continue;
            }
        }
        // Adoption: файл из легаси per-bundle подпапки пула (установки вида
        // «каждый бандл — своя папка») переносится в пул вместо перекачки.
        if let Some(src) =
            crate::pool::find_adoptable(&dir, &entry.name, &f.filename, f.size_bytes)
        {
            match crate::pool::adopt_into_pool(&src, &dest) {
                Ok(how) => {
                    on_log(format!(
                        "Найден существующий файл {} ({}), {} в пул без перекачки",
                        f.filename,
                        src.display(),
                        match how {
                            crate::pool::AdoptHow::Moved => "перемещён",
                            crate::pool::AdoptHow::Linked => "жёсткая ссылка",
                        }
                    ));
                    continue;
                }
                Err(e) => {
                    on_log(format!(
                        "Adoption {} из {} не удался ({}): качаю заново",
                        f.filename,
                        src.display(),
                        e
                    ));
                }
            }
        }
        tauri_plugin_downloader::download(
            &f.download_url,
            &dest,
            tauri_plugin_downloader::DownloadOptions {
                label: format!("{} ({})", f.filename, f.role),
                kind: "image-model".into(),
                expected_size: f.size_bytes,
                ..Default::default()
            },
            Some(&on_log),
        )
        .await
        .map_err(|e| format!("Не удалось скачать {}: {}", f.filename, e))?;
    }
    // Верификация ПОСЛЕ цикла (идея юзера): «скачан целиком» — только когда
    // каждый файл каталога на диске и совпадает размером.
    let refs: Vec<(&str, Option<u64>)> = entry
        .files
        .iter()
        .map(|f| (f.filename.as_str(), f.size_bytes))
        .collect();
    crate::pool::verify_files(&refs, &dir)?;
    let mut cfg = engine::load_image_config(&app);
    cfg.image_bundle_dir = Some(save_dir);
    engine::save_image_config(&app, &cfg)?;
    on_log("Бандл скачан целиком.".to_string());
    get_image_bundle_info(app, bundle_name)
}

#[tauri::command]
pub fn remove_image_bundle(app: AppHandle, bundle_name: Option<String>) -> Result<ImageBundleInfo, String> {
    let entry = bundle_entry(&app, bundle_name.as_deref())?;
    let dir = bundle_dir(&app);
    for f in &entry.files {
        let p = dir.join(&f.filename);
        if p.exists() {
            std::fs::remove_file(&p)
                .map_err(|e| format!("Не удалось удалить {}: {}", p.display(), e))?;
        }
    }
    // Если удаляли активный бандл — снимаем выбор (иначе все операции будут
    // падать с «активный бандл отсутствует»).
    if engine::load_image_config(&app).selected_bundle.as_deref() == Some(entry.name.as_str()) {
        let mut cfg = engine::load_image_config(&app);
        cfg.selected_bundle = None;
        engine::save_image_config(&app, &cfg)?;
    }
    Ok(get_image_bundle_info(app, None)?)
}

#[tauri::command]
pub fn remove_image_bundle_from_list(app: AppHandle) -> Result<ImageBundleInfo, String> {
    let mut cfg = engine::load_image_config(&app);
    cfg.image_bundle_dir = None;
    engine::save_image_config(&app, &cfg)?;
    Ok(get_image_bundle_info(app, None)?)
}

#[derive(Serialize, Clone)]
pub struct ImageMemoryInfo {
    pub estimate: engine::preflight::MemoryEstimate,
    pub verdict: engine::preflight::PreflightVerdict,
    pub message: String,
}

#[tauri::command]
pub fn estimate_image_memory(bundle_name: Option<String>) -> Result<ImageMemoryInfo, String> {
    let entry_opt = match bundle_name.as_deref() {
        Some(n) if !n.is_empty() => engine::bundle_by_name(n),
        _ => engine::default_bundle_entry(),
    };
    let entry = entry_opt.ok_or_else(|| {
        match bundle_name.as_deref() {
            Some(n) => format!("Бандл «{}» отсутствует в каталоге плагина.", n),
            None => "В каталоге нет бандла по умолчанию".to_string(),
        }
    })?;
    let (verdict, estimate, message) = engine::preflight_check(&entry);
    Ok(ImageMemoryInfo { estimate, verdict, message })
}

// ─────────────────────────── Генерация ──────────────────────────

fn resolve_gen_params(
    entry: &engine::ImageBundleEntry,
    width: Option<u32>,
    height: Option<u32>,
    steps: Option<u32>,
    cfg_scale: Option<f32>,
    seed: Option<i64>,
) -> (u32, u32, u32, f32, i64) {
    (
        width.unwrap_or(entry.preset.width),
        height.unwrap_or(entry.preset.height),
        steps.unwrap_or(entry.preset.steps),
        cfg_scale.unwrap_or(entry.preset.cfg_scale),
        seed.unwrap_or(entry.preset.seed),
    )
}

fn default_out_path(bundle_dir: &Path) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    bundle_dir.join("outputs").join(format!("img_{}.png", ts))
}

#[tauri::command]
pub async fn generate_image(
    app: AppHandle,
    prompt: String,
    width: Option<u32>,
    height: Option<u32>,
    steps: Option<u32>,
    cfg_scale: Option<f32>,
    seed: Option<i64>,
    out_path: Option<String>,
    bundle_name: Option<String>,
    lora_names: Option<Vec<String>>,
    lora_strength: Option<f32>,
) -> Result<engine::ImageGenResult, String> {
    let entry = bundle_entry(&app, bundle_name.as_deref())?;
    let (w, h, st, cfg, sd) = resolve_gen_params(&entry, width, height, steps, cfg_scale, seed);
    // LoRA-резолв по SSOT-плагиновому модулю (PRP 29.10.26): несуществующее
    // имя — честный отказ команды (текст включает папку и доступные стемы).
    // F3 29.10.26: ресолв scoped на базу бандла — лоры чужой базы не найдутся.
    let loras = engine::resolve_lora_names_for_base(
        &engine::image_lora_dir_early(),
        lora_names.as_deref().unwrap_or(&[]),
        lora_strength.unwrap_or(1.0),
        entry.lora_base.as_deref(),
    )?;
    let edir = engine_dir(&app);
    let bdir = bundle_dir(&app);
    let variant = preferred_variant(&app);
    let out = out_path.map(PathBuf::from).unwrap_or_else(|| default_out_path(&bdir));
    let log_cb = |msg: String| log::info!("[IMG] {}", msg);
    tokio::task::spawn_blocking(move || {
        engine::generate_image_simple(&edir, &bdir, &entry, variant.as_deref(), &prompt, w, h, st, cfg, sd, &out, &loras, None, log_cb)
    })
    .await
    .map_err(|e| format!("Задача генерации прервана: {}", e))?
}

#[tauri::command]
pub async fn edit_image(
    app: AppHandle,
    prompt: String,
    ref_paths: Vec<String>,
    width: Option<u32>,
    height: Option<u32>,
    steps: Option<u32>,
    cfg_scale: Option<f32>,
    seed: Option<i64>,
    out_path: Option<String>,
    bundle_name: Option<String>,
    lora_names: Option<Vec<String>>,
    lora_strength: Option<f32>,
) -> Result<engine::ImageGenResult, String> {
    if ref_paths.is_empty() {
        return Err("Для редактирования нужен хотя бы один референс (ref_paths пуст).".to_string());
    }
    let entry = bundle_entry(&app, bundle_name.as_deref())?;
    if !entry.files.iter().any(|f| f.role == "mmproj") {
        return Err(format!(
            "Edit требует mmproj (vision-энкодер). Бандл «{}» его не содержит — выберите полный набор в настройках image-engine.",
            entry.name
        ));
    }
    let (w, h, st, cfg, sd) = resolve_gen_params(&entry, width, height, steps, cfg_scale, seed);
    // LoRA-резолв по SSOT-плагиновому модулю (PRP 29.10.26): несуществующее
    // имя — честный отказ команды (текст включает папку и доступные стемы).
    // F3 29.10.26: ресолв scoped на базу бандла — лоры чужой базы не найдутся.
    let user_loras = engine::resolve_lora_names_for_base(
        &engine::image_lora_dir_early(),
        lora_names.as_deref().unwrap_or(&[]),
        lora_strength.unwrap_or(1.0),
        entry.lora_base.as_deref(),
    )?;
    // F4 (29.10.26): SSOT-планировщик ref-edit (image_engine::plan_ref_edit).
    // Гейт ДО планирования: бандл без контракта — отказ, Krea2 — автолора
    // krea2_identity_edit_v2_r128 + preset krea2_edit, Qwen — нативный стек.
    // [IMG] origin: хост-тул агентов идёт через ту же точку.
    let (loras, ref_image_args) = match engine::plan_ref_edit(&entry, &engine::image_lora_dir_early(), user_loras)? {
        engine::RefEditPlan::Unsupported { reason } => {
            log::warn!("[IMG] ref-edit отказ (fail-closed): бандл «{}»", entry.name);
            return Err(reason);
        }
        engine::RefEditPlan::Ready { loras, ref_image_args } => (loras, ref_image_args),
    };
    let edir = engine_dir(&app);
    let bdir = bundle_dir(&app);
    let variant = preferred_variant(&app);
    let out = out_path.map(PathBuf::from).unwrap_or_else(|| default_out_path(&bdir));
    let log_cb = |msg: String| log::info!("[IMG] {}", msg);
    tokio::task::spawn_blocking(move || {
        engine::edit_image_files(&edir, &bdir, &entry, variant.as_deref(), &prompt, &ref_paths, w, h, st, cfg, sd, &out, &loras, None, log_cb, ref_image_args.as_deref())
    })
    .await
    .map_err(|e| format!("Задача редактирования прервана: {}", e))?
}

// ─────────────────────────── Видео (Шаг 2: MiniMax H3) ──────────────────────────

#[derive(Serialize, Clone)]
pub struct VideoBundleInfo {
    pub bundle_name: String,
    pub label: String,
    pub note: String,
    /// Роли видео-бандла: diffusion / llm / vae / audio_vae (+lora_draft после бенча).
    pub files: Vec<BundleFileInfo>,
    pub total_bytes: u64,
    pub size_gb: Option<String>,
    pub vram_fast_gb: Option<f32>,
    pub vram_min_gb: Option<f32>,
    pub ram_min_gb: Option<f32>,
    pub save_dir: String,
    pub free_space_gb: u64,
    pub fully_downloaded: bool,
}

#[derive(Serialize, Clone)]
pub struct VideoBundleValidate {
    pub valid: bool,
    pub dir: String,
    pub present: Vec<String>,
    pub missing: Vec<String>,
}

/// Папка видео-бандлов: явная из конфига, иначе
/// `<хранилище KingOrchData>/video_models`. Тот же пул-принцип, что у image
/// (25.10.2026): diffusion-файлы разных бандлов имеют разные имена, общие
/// роли (llm/vae/audio_vae/lora_draft) — одинаковые, downloader пропускает
/// существующее. Per-bundle подпапки порождали перекачку.
fn video_bundle_dir(app: &AppHandle) -> PathBuf {
    let cfg = engine::load_image_config(app);
    if let Some(p) = &cfg.video_bundle_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    engine::default_video_bundle_dir()
}

/// Взять запись: имя → каталог; без имени → selected_video_bundle → дефолт.
fn video_entry(app: &AppHandle, name: Option<&str>) -> Result<engine::VideoBundleEntry, String> {
    if let Some(n) = name.filter(|n| !n.is_empty()) {
        return engine::video_bundle_by_name(n)
            .ok_or_else(|| format!("Видео-бандл «{}» отсутствует в каталоге плагина.", n));
    }
    if let Some(ref sel) = engine::load_image_config(app).selected_video_bundle {
        if !sel.is_empty() {
            return engine::video_bundle_by_name(sel).ok_or_else(|| {
                format!(
                    "Выбранный видео-бандл «{}» отсутствует в каталоге — смените выбор в интерфейсе «Видео».",
                    sel
                )
            });
        }
    }
    engine::default_video_bundle_entry()
        .ok_or_else(|| "В каталоге нет видео-бандла по умолчанию".to_string())
}

/// Бандл без AppHandle (валидация папки): имя → каталог, иначе дефолт.
fn video_entry_from_name(name: Option<&str>) -> Result<engine::VideoBundleEntry, String> {
    match name.filter(|n| !n.is_empty()) {
        Some(n) => engine::video_bundle_by_name(n)
            .ok_or_else(|| format!("Видео-бандл «{}» отсутствует в каталоге плагина.", n)),
        None => engine::default_video_bundle_entry()
            .ok_or_else(|| "В каталоге нет видео-бандла по умолчанию".to_string()),
    }
}

fn video_bundle_info_for(app: &AppHandle, entry: &engine::VideoBundleEntry) -> VideoBundleInfo {
    let save_dir = video_bundle_dir(app);
    let mut files = Vec::with_capacity(entry.files.len());
    let mut fully = true;
    for f in &entry.files {
        let save_path = save_dir.join(&f.filename);
        let exists = save_path.exists();
        if !exists {
            fully = false;
        }
        files.push(BundleFileInfo {
            role: f.role.clone(),
            filename: f.filename.clone(),
            size_bytes: f.size_bytes,
            save_path: save_path.to_string_lossy().to_string(),
            exists,
        });
    }
    let total_bytes: u64 = entry.files.iter().map(|f| f.size_bytes.unwrap_or(0)).sum();
    VideoBundleInfo {
        bundle_name: entry.name.clone(),
        label: entry.label.clone(),
        note: entry.note.clone(),
        files,
        total_bytes,
        size_gb: entry.size_gb.clone(),
        vram_fast_gb: entry.vram_fast_gb,
        vram_min_gb: entry.vram_min_gb,
        ram_min_gb: entry.ram_min_gb,
        save_dir: save_dir.to_string_lossy().to_string(),
        free_space_gb: free_space_gb_for(&save_dir),
        fully_downloaded: fully,
    }
}

fn validate_video_dir(entry: &engine::VideoBundleEntry, dir: &Path) -> VideoBundleValidate {
    let mut present = Vec::new();
    let mut missing = Vec::new();
    for f in &entry.files {
        if dir.join(&f.filename).exists() {
            present.push(f.filename.clone());
        } else {
            missing.push(f.filename.clone());
        }
    }
    VideoBundleValidate {
        valid: missing.is_empty(),
        dir: dir.to_string_lossy().to_string(),
        present,
        missing,
    }
}

#[tauri::command]
pub fn get_video_bundles() -> Result<Vec<engine::VideoBundleEntry>, String> {
    Ok(engine::load_video_catalog())
}

#[tauri::command]
pub fn get_video_bundle_info(
    app: AppHandle,
    bundle_name: Option<String>,
) -> Result<VideoBundleInfo, String> {
    let entry = video_entry(&app, bundle_name.as_deref())?;
    Ok(video_bundle_info_for(&app, &entry))
}

#[tauri::command]
pub fn set_selected_video_bundle(
    app: AppHandle,
    bundle_name: String,
) -> Result<VideoBundleInfo, String> {
    if bundle_name.trim().is_empty() {
        let mut cfg = engine::load_image_config(&app);
        cfg.selected_video_bundle = None;
        engine::save_image_config(&app, &cfg)?;
        return get_video_bundle_info(app, None);
    }
    let entry = engine::video_bundle_by_name(&bundle_name).ok_or_else(|| {
        format!("Видео-бандл «{}» отсутствует в каталоге плагина.", bundle_name)
    })?;
    // Гейт активации (зеркало image): неполный видео-бандл не активируется.
    let dir = video_bundle_dir(&app);
    let missing: Vec<&str> = entry
        .files
        .iter()
        .filter(|f| !dir.join(&f.filename).exists())
        .map(|f| f.filename.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "Видео-бандл «{}» скачан не полностью: нет файлов {}. Сначала нажмите «Скачать набор» / «Докачать файлы» — активация недокачанного набора запрещена.",
            entry.name,
            missing.join(", ")
        ));
    }
    let mut cfg = engine::load_image_config(&app);
    cfg.selected_video_bundle = Some(bundle_name.clone());
    engine::save_image_config(&app, &cfg)?;
    log::info!("[VIDEO] Активный видео-бандл: {}", entry.name);
    Ok(video_bundle_info_for(&app, &entry))
}

#[tauri::command]
pub fn validate_video_bundle_dir(
    path: String,
    bundle_name: Option<String>,
) -> Result<VideoBundleValidate, String> {
    let entry = video_entry_from_name(bundle_name.as_deref())?;
    Ok(validate_video_dir(&entry, Path::new(&path)))
}

/// Custom-папка видео-бандла (зеркало `set_image_bundle_dir`): назначить папку
/// текущего видео-бандла (add) или сбросить на дефолт (пустая строкa → None).
#[tauri::command]
pub fn set_video_bundle_dir(app: AppHandle, path: Option<String>) -> Result<VideoBundleInfo, String> {
    let mut cfg = engine::load_image_config(&app);
    match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => {
            if !Path::new(&p).exists() {
                return Err(format!("Папка «{}» не существует.", p));
            }
            cfg.video_bundle_dir = Some(p);
        }
        None => cfg.video_bundle_dir = None,
    }
    engine::save_image_config(&app, &cfg)?;
    video_entry(&app, None).map(|e| video_bundle_info_for(&app, &e))
}

/// Удалить файлы видео-бандла на диске (зеркало `remove_image_bundle`):
/// решает бандл как `video_entry` (selected → дефолт), удаляет файлы, снимает
/// выбор, если удаляли активный; custom-папку НЕ трогает (путь остаётся).
#[tauri::command]
pub fn remove_video_bundle_files(app: AppHandle, bundle_name: Option<String>) -> Result<VideoBundleInfo, String> {
    let entry = video_entry(&app, bundle_name.as_deref())?;
    let dir = video_bundle_dir(&app);
    for f in &entry.files {
        let p = dir.join(&f.filename);
        if p.exists() {
            std::fs::remove_file(&p)
                .map_err(|e| format!("Не удалось удалить {}: {}", p.display(), e))?;
        }
    }
    if engine::load_image_config(&app)
        .selected_video_bundle
        .as_deref()
        == Some(entry.name.as_str())
    {
        let mut cfg = engine::load_image_config(&app);
        cfg.selected_video_bundle = None;
        engine::save_image_config(&app, &cfg)?;
    }
    let fresh = video_entry(&app, None)?;
    Ok(video_bundle_info_for(&app, &fresh))
}

/// Скачивание видео-бандла — переиспользование загрузчика image-бандлов
/// (per-file по каталогу: `download_url` + `size_bytes` + progress-метка
/// с ролью; загрузчик агностичен к ролям, обязательность `audio_vae`
/// держит `validate_video_entry`/`VIDEO_REQUIRED_ROLES`).
#[tauri::command]
pub async fn download_video_bundle(
    app: AppHandle,
    save_dir: String,
    bundle_name: Option<String>,
) -> Result<VideoBundleInfo, String> {
    let entry = video_entry(&app, bundle_name.as_deref())?;
    let dir = PathBuf::from(&save_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Не удалось создать папку {}: {}", dir.display(), e))?;
    let on_log = |msg: String| log::info!("[VIDEO-DL] {}", msg);
    for f in &entry.files {
        let dest = dir.join(&f.filename);
        if dest.exists() {
            if let Some(expected) = f.size_bytes {
                if let Ok(meta) = std::fs::metadata(&dest) {
                    if meta.len() == expected {
                        on_log(format!("Уже скачан, пропуск: {}", f.filename));
                        continue;
                    }
                }
            } else {
                on_log(format!("Уже скачан, пропуск: {}", f.filename));
                continue;
            }
        }
        // Adoption легаси per-bundle подпапок — как у image-бандлов.
        if let Some(src) =
            crate::pool::find_adoptable(&dir, &entry.name, &f.filename, f.size_bytes)
        {
            match crate::pool::adopt_into_pool(&src, &dest) {
                Ok(how) => {
                    on_log(format!(
                        "Найден существующий файл {} ({}), {} в пул без перекачки",
                        f.filename,
                        src.display(),
                        match how {
                            crate::pool::AdoptHow::Moved => "перемещён",
                            crate::pool::AdoptHow::Linked => "жёсткая ссылка",
                        }
                    ));
                    continue;
                }
                Err(e) => {
                    on_log(format!(
                        "Adoption {} из {} не удался ({}): качаю заново",
                        f.filename,
                        src.display(),
                        e
                    ));
                }
            }
        }
        tauri_plugin_downloader::download(
            &f.download_url,
            &dest,
            tauri_plugin_downloader::DownloadOptions {
                label: format!("{} ({})", f.filename, f.role),
                kind: "video-model".into(),
                expected_size: f.size_bytes,
                ..Default::default()
            },
            Some(&on_log),
        )
        .await
        .map_err(|e| format!("Не удалось скачать {}: {}", f.filename, e))?;
    }
    // Верификация после цикла — зеркало image-ветки.
    let refs: Vec<(&str, Option<u64>)> = entry
        .files
        .iter()
        .map(|f| (f.filename.as_str(), f.size_bytes))
        .collect();
    crate::pool::verify_files(&refs, &dir)?;
    let mut cfg = engine::load_image_config(&app);
    cfg.video_bundle_dir = Some(save_dir);
    engine::save_image_config(&app, &cfg)?;
    on_log("Видео-бандл скачан целиком.".to_string());
    get_video_bundle_info(app, bundle_name)
}

// ─────────────────────────── Генерация видео ──────────────────────────

// file_to_b64 живёт в движке (engine::video_engine::file_to_b64) — единая
// точка чтения кадра для GUI-команды и хостовой generate_video_simple (SSOT).

fn default_video_out_path(bundle_dir: &Path) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    bundle_dir.join("outputs").join(format!("vid_{}.webm", ts))
}

/// vid_gen-запрос из GUI: промпт + опциональные кадры (локальные пути или
/// data-URL') + параметры. Дефолты размеров/качества/seed/fps — ИЗ ПРЕСЕТА
/// каталога; draft/final решает sampling_plan_for_quality по наличию файла
/// lora_draft (турбо-точка lcm/simple/shift из каталога).
#[tauri::command]
pub async fn generate_video(
    app: AppHandle,
    prompt: String,
    negative_prompt: Option<String>,
    init_path: Option<String>,
    end_path: Option<String>,
    ref_paths: Vec<String>,
    // seconds: длительность клипа (кадры = seconds × fps; сетку держит сервер).
    seconds: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    // steps: явные шаги — приоритетнее final_quality.
    steps: Option<u32>,
    cfg_scale: Option<f32>,
    seed: Option<i64>,
    // final_quality: false/None = draft (steps_draft/_nolora), true = steps_final.
    final_quality: Option<bool>,
    // strength: image-to-video сила (init/end кадры); дефолт 0.75.
    strength: Option<f32>,
    vae_tiling: Option<bool>,
    out_path: Option<String>,
    bundle_name: Option<String>,
) -> Result<engine::VideoGenResult, String> {
    let entry = video_entry(&app, bundle_name.as_deref())?;
    let bdir = video_bundle_dir(&app);
    let edir = engine_dir(&app);
    let variant = preferred_variant(&app);

    // Дефолты ИЗ ПРЕСЕТА КАТАЛОГА; явные аргументы переопределяют.
    let preset = &entry.preset;
    let fps = preset.fps;
    let w = width.unwrap_or(preset.width);
    let h = height.unwrap_or(preset.height);
    let cfgs = cfg_scale.unwrap_or(preset.cfg_scale);
    let sd = seed.unwrap_or(preset.seed);
    // Сэмплинг-план (шаги/сэмплер/шедулер/shift/LoRA) — из каталога; явные
    // шаги переопределяют ТОЛЬКО шаги, остальная турбо-точка не трогается.
    let mut plan = engine::sampling_plan_for_quality(&entry, &bdir, final_quality.unwrap_or(false));
    if let Some(s) = steps {
        plan.steps = s;
    }
    let st = plan.steps;
    // Лимит каталога — честный clamp с warn (сетку кадров всё равно держит сервер).
    let mut secs = seconds.unwrap_or(5);
    if secs == 0 {
        return Err("seconds = 0 — нужен хотя бы 1 секунда клипа.".to_string());
    }
    if secs > preset.max_seconds {
        log::warn!(
            "[VID] seconds={} выше лимита каталога {} — клампим.",
            secs, preset.max_seconds
        );
        secs = preset.max_seconds;
    }

    // Кадры по локальным путям (или data-URL) — зашиваем в запрос.
    let init_b64 = match init_path.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(p) => Some(file_to_b64(p)?),
        None => None,
    };
    let end_b64 = match end_path.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(p) => Some(file_to_b64(p)?),
        None => None,
    };
    if ref_paths.len() > 8 {
        return Err(format!(
            "ref_paths = {} — H3 Ref2VA ограничен разумным числом референсов (≤8).",
            ref_paths.len()
        ));
    }
    let mut ref_b64 = Vec::with_capacity(ref_paths.len());
    for p in &ref_paths {
        ref_b64.push(file_to_b64(p)?);
    }

    let req = engine::VideoRequest {
        prompt: prompt.clone(),
        negative_prompt: negative_prompt.clone(),
        init_b64,
        end_b64,
        ref_images_b64: ref_b64,
        width: w,
        height: h,
        steps: st,
        cfg_scale: cfgs,
        seed: sd,
        video_frames: engine::seconds_to_frames(secs, fps),
        fps,
        vae_tiling: vae_tiling.unwrap_or(true),
        strength: strength.unwrap_or(0.75),
        sample_method: plan.sample_method,
        scheduler: plan.scheduler,
        flow_shift: plan.flow_shift,
        lora: plan.lora,
    };

    let out = out_path.map(PathBuf::from).unwrap_or_else(|| default_video_out_path(&bdir));
    let log_cb = |msg: String| log::info!("[VID] {}", msg);
    log::info!(
        "[VID] generate_video «{}»: {}x{}, steps {}, {} сек @ {} fps (final={})",
        prompt.chars().take(60).collect::<String>(),
        w,
        h,
        st,
        secs,
        fps,
        final_quality.unwrap_or(false)
    );
    tokio::task::spawn_blocking(move || {
        let eng = engine::VideoEngine::new(&edir, &bdir, &entry, variant.as_deref(), log_cb)?;
        eng.generate(&req, &out, None, log_cb)
    })
    .await
    .map_err(|e| format!("Задача видео-генерации прервана: {}", e))?
}

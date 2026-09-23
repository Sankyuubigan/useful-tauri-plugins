//! Tauri-команды плагина: статус/установка движка sd.cpp, каталог бандлов,
//! скачивание бандла, preflight, генерация/редактирование.
//! Команды — тонкий слой, вся логика в `crate::engine`.

use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

use crate::engine::{self, sdcpp_installer};

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

/// Папка бандла: явная из конфига, иначе `<exe>/image_models/<bundle_name>`.
fn bundle_dir(app: &AppHandle) -> PathBuf {
    let cfg = engine::load_image_config(app);
    if let Some(p) = &cfg.image_bundle_dir {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let name = engine::default_bundle_entry()
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "qwen-image-2.1".to_string());
    exe_dir(app).join("image_models").join(name)
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
    engine::save_image_config(&app, &cfg);

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
    engine::save_image_config(&app, &cfg);
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

#[tauri::command]
pub fn get_image_bundle_info(app: AppHandle) -> Result<ImageBundleInfo, String> {
    let entry = engine::default_bundle_entry()
        .ok_or_else(|| "В каталоге нет бандла по умолчанию".to_string())?;
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
pub fn validate_image_bundle_dir(path: String) -> Result<ImageBundleValidate, String> {
    let entry = engine::default_bundle_entry()
        .ok_or_else(|| "В каталоге нет бандла по умолчанию".to_string())?;
    Ok(validate_bundle_dir(&entry, Path::new(&path)))
}

#[tauri::command]
pub fn set_image_bundle_dir(app: AppHandle, path: String) -> Result<ImageBundleInfo, String> {
    let entry = engine::default_bundle_entry()
        .ok_or_else(|| "В каталоге нет бандла по умолчанию".to_string())?;
    let v = validate_bundle_dir(&entry, Path::new(&path));
    if !v.valid {
        return Err(format!("Набор не валиден, не хватает: {}", v.missing.join(", ")));
    }
    let mut cfg = engine::load_image_config(&app);
    cfg.image_bundle_dir = Some(path);
    engine::save_image_config(&app, &cfg);
    get_image_bundle_info(app)
}

#[tauri::command]
pub async fn download_image_bundle(app: AppHandle, save_dir: String) -> Result<ImageBundleInfo, String> {
    let entry = engine::default_bundle_entry()
        .ok_or_else(|| "В каталоге нет бандла по умолчанию".to_string())?;
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
    let mut cfg = engine::load_image_config(&app);
    cfg.image_bundle_dir = Some(save_dir);
    engine::save_image_config(&app, &cfg);
    on_log("✅ Бандл скачан целиком.".to_string());
    get_image_bundle_info(app)
}

#[tauri::command]
pub fn remove_image_bundle(app: AppHandle) -> Result<ImageBundleInfo, String> {
    let entry = engine::default_bundle_entry()
        .ok_or_else(|| "В каталоге нет бандла по умолчанию".to_string())?;
    let dir = bundle_dir(&app);
    for f in &entry.files {
        let p = dir.join(&f.filename);
        if p.exists() {
            std::fs::remove_file(&p)
                .map_err(|e| format!("Не удалось удалить {}: {}", p.display(), e))?;
        }
    }
    Ok(get_image_bundle_info(app)?)
}

#[derive(Serialize, Clone)]
pub struct ImageMemoryInfo {
    pub estimate: engine::preflight::MemoryEstimate,
    pub verdict: engine::preflight::PreflightVerdict,
    pub message: String,
}

#[tauri::command]
pub fn estimate_image_memory() -> Result<ImageMemoryInfo, String> {
    let entry = engine::default_bundle_entry()
        .ok_or_else(|| "В каталоге нет бандла по умолчанию".to_string())?;
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
) -> Result<engine::ImageGenResult, String> {
    let entry = engine::default_bundle_entry()
        .ok_or_else(|| "В каталоге нет бандла по умолчанию".to_string())?;
    let (w, h, st, cfg, sd) = resolve_gen_params(&entry, width, height, steps, cfg_scale, seed);
    let edir = engine_dir(&app);
    let bdir = bundle_dir(&app);
    let variant = preferred_variant(&app);
    let out = out_path.map(PathBuf::from).unwrap_or_else(|| default_out_path(&bdir));
    let log_cb = |msg: String| log::info!("[IMG] {}", msg);
    tokio::task::spawn_blocking(move || {
        engine::generate_image_simple(&edir, &bdir, &entry, variant.as_deref(), &prompt, w, h, st, cfg, sd, &out, None, log_cb)
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
) -> Result<engine::ImageGenResult, String> {
    if ref_paths.is_empty() {
        return Err("Для редактирования нужен хотя бы один референс (ref_paths пуст).".to_string());
    }
    let entry = engine::default_bundle_entry()
        .ok_or_else(|| "В каталоге нет бандла по умолчанию".to_string())?;
    let (w, h, st, cfg, sd) = resolve_gen_params(&entry, width, height, steps, cfg_scale, seed);
    let edir = engine_dir(&app);
    let bdir = bundle_dir(&app);
    let variant = preferred_variant(&app);
    let out = out_path.map(PathBuf::from).unwrap_or_else(|| default_out_path(&bdir));
    let log_cb = |msg: String| log::info!("[IMG] {}", msg);
    tokio::task::spawn_blocking(move || {
        engine::edit_image_files(&edir, &bdir, &entry, variant.as_deref(), &prompt, &ref_paths, w, h, st, cfg, sd, &out, None, log_cb)
    })
    .await
    .map_err(|e| format!("Задача редактирования прервана: {}", e))?
}

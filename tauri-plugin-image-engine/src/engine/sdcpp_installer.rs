//! Установка движка sdcpp: скачивание полного релиза из GitHub-источника.
//!
//! Движок — это ОТДЕЛЬНЫЙ ПРОЦЕСС `sd-server.exe`, который приложение запускает
//! по HTTP. Приложение НЕ линкует stable-diffusion.cpp (нет PE-импортов, нет DLL
//! рядом с exe) — поэтому нужен полный архив движка, а не только CUDA runtime.
//!
//! Несколько вариантов могут быть установлены ОДНОВРЕМЕННО:
//! `backends/<source>/<variant>/` — side-by-side без перекачивания. Источник
//! сейчас один (`leejet`), уровень source оставлен под будущие форки.

use crate::engine::sources::{self, SourceSpec};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

/// Актуальная CUDA-сборка sd.cpp (win-cuda12).
pub const VARIANT_CUDA12: &str = "cuda12";
pub const VARIANT_CPU: &str = "cpu";
pub const VARIANT_VULKAN: &str = "vulkan";
/// ROCm для Windows — только современные AMD. Старые AMD — Vulkan.
pub const VARIANT_ROCM: &str = "rocm";
/// Значение конфига «подобрать автоматически по видеокарте».
pub const VARIANT_AUTO: &str = "auto";

const METADATA_FILE: &str = "engine_meta.json";

/// Семейство варианта движка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineFamily {
    Cpu,
    Cuda12,
    Vulkan,
    Rocm,
}

impl EngineFamily {
    pub fn from_variant(variant: &str) -> EngineFamily {
        let v = variant.to_lowercase();
        if v.contains("cuda") {
            EngineFamily::Cuda12
        } else if v.starts_with("vulkan") {
            EngineFamily::Vulkan
        } else if v.starts_with("rocm") || v.starts_with("hip") {
            EngineFamily::Rocm
        } else {
            EngineFamily::Cpu
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EngineFamily::Cpu => "CPU",
            EngineFamily::Cuda12 => "CUDA 12.x",
            EngineFamily::Vulkan => "Vulkan",
            EngineFamily::Rocm => "ROCm",
        }
    }

    /// GPU-семейство (модель оффлоудится в VRAM при запуске)
    pub fn is_gpu(self) -> bool {
        !matches!(self, EngineFamily::Cpu)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EngineMeta {
    pub tag: String,
    /// Вариант движка (фактический, из имени ассета)
    #[serde(default)]
    pub variant: String,
    /// Источник бинарей ("leejet"); пусто у старых meta → leejet.
    #[serde(default)]
    pub source: String,
    pub installed_at: String,
}

#[derive(Deserialize, Clone)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize, Clone)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

/// Папка движка по умолчанию — рядом с exe (в папке установки программы)
pub fn default_dir(exe_dir: &Path) -> PathBuf {
    exe_dir.join("sdcpp")
}

/// Папка со всеми установленными бекендами: `<sdcpp_dir>/backends`
pub fn backends_dir(dir: &Path) -> PathBuf {
    dir.join("backends")
}

/// Папка источника: `<sdcpp_dir>/backends/<source>`
pub fn source_dir(dir: &Path, source: &str) -> PathBuf {
    backends_dir(dir).join(source)
}

/// Папка конкретного варианта бекенда: `<sdcpp_dir>/backends/<source>/<variant>`
pub fn variant_dir(dir: &Path, source: &str, variant: &str) -> PathBuf {
    source_dir(dir, source).join(variant)
}

/// Путь к метаданным варианта.
pub fn meta_path(dir: &Path, source: &str, variant: &str) -> PathBuf {
    variant_dir(dir, source, variant).join(METADATA_FILE)
}

/// Все известные варианты бекенда (для дропдауна в настройках)
pub fn all_variants() -> Vec<String> {
    vec![
        VARIANT_CPU.to_string(),
        VARIANT_CUDA12.to_string(),
        VARIANT_VULKAN.to_string(),
        VARIANT_ROCM.to_string(),
    ]
}

/// Известен ли вариант (не "auto" и есть в списке)
pub fn is_known_variant(variant: &str) -> bool {
    all_variants().iter().any(|v| v == variant)
}

/// Автоопределение варианта по GPU: NVIDIA с драйвером CUDA 12+ → cuda12
/// (у sd.cpp одна CUDA-сборка), иначе CPU. AMD/Intel юзер выбирает вручную
/// (vulkan/rocm) — авто их не трогает, чтобы не угадать мимо.
pub fn select_variant() -> String {
    match detect_cuda_major() {
        Some(major) if major >= 12 => VARIANT_CUDA12.to_string(),
        _ => VARIANT_CPU.to_string(),
    }
}

/// Мажорная версия CUDA-драйвера через NVML (None — нет NVIDIA/драйвера).
fn detect_cuda_major() -> Option<u32> {
    let nvml = nvml_wrapper::Nvml::init().ok()?;
    let ver: u32 = nvml.sys_cuda_driver_version().ok()?.try_into().ok()?;
    if ver < 1000 {
        return None;
    }
    Some(ver / 1000)
}

/// Итоговый вариант по предпочтению юзера: "auto"/None → подбор по GPU,
/// явный известный вариант → как есть, неизвестный → авто.
pub fn resolve_variant(pref: Option<&str>) -> String {
    match pref {
        Some(p) if !p.is_empty() && p != VARIANT_AUTO && is_known_variant(p) => p.to_string(),
        _ => select_variant(),
    }
}

/// Человекочитаемое описание варианта для UI
pub fn variant_note(variant: &str) -> &'static str {
    match variant {
        VARIANT_CPU => "Работает на любом компьютере, без видеокарты (медленно)",
        VARIANT_CUDA12 => "NVIDIA GTX 10xx и новее (драйвер CUDA 12+)",
        VARIANT_VULKAN => "Любые видеокарты: AMD, Intel, NVIDIA (через Vulkan)",
        VARIANT_ROCM => "Только современные AMD (ROCm). На старых AMD не работает — выберите Vulkan",
        _ => "",
    }
}

/// Описание варианта для дропдауна в настройках
#[derive(Serialize, Clone)]
pub struct VariantInfo {
    pub id: String,
    pub label: String,
    pub note: String,
    /// Этот вариант подобрал бы авто-режим на текущей машине
    pub recommended: bool,
    pub installed: bool,
}

/// Список вариантов для дропдауна + установлен ли каждый на диске.
pub fn available_variants(dir: &Path, source: &str) -> Vec<VariantInfo> {
    let auto = select_variant();
    let installed = list_installed_variants(dir, source);
    all_variants()
        .into_iter()
        .map(|id| VariantInfo {
            label: variant_label(&id).to_string(),
            note: variant_note(&id).to_string(),
            recommended: id == auto,
            installed: installed.iter().any(|v| v == &id),
            id,
        })
        .collect()
}

pub fn variant_label(variant: &str) -> &'static str {
    match variant {
        VARIANT_CPU => "CPU (процессор)",
        VARIANT_CUDA12 => "CUDA 12.x (NVIDIA)",
        VARIANT_VULKAN => "Vulkan (любая видеокарта)",
        VARIANT_ROCM => "ROCm (современные AMD)",
        VARIANT_AUTO => "Авто (рекомендуется)",
        _ => "Вариант (неизвестный)",
    }
}

/// Установлен ли вариант (есть sd-server.exe + метаданные).
pub fn is_installed(dir: &Path, source: &str, variant: &str) -> bool {
    variant_dir(dir, source, variant).join("sd-server.exe").exists()
        && meta_path(dir, source, variant).exists()
}

/// Метаданные установленного варианта.
pub fn installed_meta(dir: &Path, source: &str, variant: &str) -> Option<EngineMeta> {
    let data = fs::read_to_string(meta_path(dir, source, variant)).ok()?;
    serde_json::from_str(&data).ok()
}

/// Список установленных вариантов источника.
pub fn list_installed_variants(dir: &Path, source: &str) -> Vec<String> {
    let src = source_dir(dir, source);
    let Ok(entries) = fs::read_dir(&src) else { return vec![] };
    entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|v| is_installed(dir, source, v))
        .collect()
}

/// Хоть что-то установлено (для гейта перед генерацией).
pub fn has_any_installed(dir: &Path) -> bool {
    let backends = backends_dir(dir);
    let Ok(src_entries) = fs::read_dir(&backends) else { return false };
    for src in src_entries.flatten() {
        if !src.path().is_dir() {
            continue;
        }
        let source = src.file_name().to_string_lossy().to_string();
        if !list_installed_variants(dir, &source).is_empty() {
            return true;
        }
    }
    false
}

const VC_REDIST_DLLS: &[&str] = &["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"];

/// Рантайм VC++ рядом с движком (чтобы работало без ручной установки
/// Visual C++ Redistributable — иначе sd-server падает с 0xC0000135).
pub fn ensure_vc_redist(engine_exe_dir: &Path, on_log: &dyn Fn(String)) {
    let app_exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();

    let mut sources: Vec<PathBuf> = vec![app_exe_dir.join("redist"), app_exe_dir];
    if let Some(sysroot) = std::env::var_os("SystemRoot") {
        sources.push(PathBuf::from(sysroot).join("System32"));
    }

    for dll in VC_REDIST_DLLS {
        let dst = engine_exe_dir.join(dll);
        if dst.exists() {
            continue;
        }
        let mut copied = false;
        for src_dir in &sources {
            let src = src_dir.join(dll);
            if src.exists() {
                if fs::copy(&src, &dst).is_ok() {
                    copied = true;
                    break;
                }
            }
        }
        if copied {
            on_log(format!("  ✓ Рантайм VC++ добавлен рядом с движком: {}", dll));
        } else {
            on_log(format!(
                "  ⚠️ Рантайм VC++ {} не найден рядом с приложением — если его нет в System32, движок не запустится.",
                dll
            ));
        }
    }
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("king-orch-app/1.0")
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| format!("Ошибка создания HTTP-клиента: {}", e))
}

/// Чистая (без сети) логика выбора релиза: из списка релизов (от новых к старым)
/// возвращает первый, в котором есть готовый бинарник движка для варианта.
/// Если source.prefer_stable — сначала ищем среди stable, затем fallback.
fn first_release_with_engine(
    releases: &[GitHubRelease],
    spec: &SourceSpec,
    variant: &str,
) -> Option<GitHubRelease> {
    if spec.prefer_stable {
        if let Some(rel) = releases.iter().find(|r| {
            !r.prerelease
                && !r.tag_name.to_lowercase().contains("preview")
                && find_engine_asset(r, spec, variant).is_some()
        }) {
            return Some(rel.clone());
        }
    }
    releases
        .iter()
        .find(|r| find_engine_asset(r, spec, variant).is_some())
        .cloned()
}

/// Получить с GitHub самый свежий релиз источника, содержащий готовый бинарник
/// движка для запрошенного варианта (платформа Windows x64).
async fn fetch_release_with_engine(
    client: &reqwest::Client,
    spec: &SourceSpec,
    variant: &str,
) -> Result<GitHubRelease, String> {
    let releases_url = sources::releases_url(spec);
    let resp = client
        .get(&releases_url)
        .send()
        .await
        .map_err(|e| format!("Ошибка запроса GitHub API: {}", e))?;
    let status = resp.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(
            "Превышен лимит запросов к GitHub API. Подождите несколько минут и повторите установку."
                .to_string(),
        );
    }
    if !status.is_success() {
        return Err(format!("GitHub API вернул HTTP {}", status));
    }
    let releases: Vec<GitHubRelease> = resp
        .json()
        .await
        .map_err(|e| format!("Ошибка парсинга ответа GitHub: {}", e))?;

    first_release_with_engine(&releases, spec, variant).ok_or_else(|| {
        format!(
            "Не найден релиз «{}» с готовым движком для варианта «{}». Проверьте доступ к GitHub (api.github.com) и повторите позже.",
            spec.id, variant
        )
    })
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path).map_err(|e| format!("Ошибка чтения файла: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("Ошибка чтения: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hex: String = hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect();
    Ok(hex)
}

/// Извлечение ВСЕГО содержимого архива в dest_dir (защита от path traversal).
fn extract_all<L: Fn(String)>(zip_path: &Path, dest_dir: &Path, on_log: &L) -> Result<u32, String> {
    on_log("📦 Распаковка движка sd.cpp...".to_string());
    let data = fs::read(zip_path).map_err(|e| format!("Ошибка чтения zip: {}", e))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|e| format!("Ошибка чтения zip: {}", e))?;

    let mut count = 0u32;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Ошибка чтения zip-записи: {}", e))?;
        let name = entry.name().replace('\\', "/");
        let clean = name.trim_start_matches('/');
        if clean.is_empty() || clean.ends_with('/') {
            continue;
        }
        let out_path = dest_dir.join(clean);
        // Защита от path traversal: файл обязан остаться внутри dest_dir
        if !out_path.starts_with(dest_dir) {
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Ошибка создания папки {}: {}", parent.display(), e))?;
        }
        let mut out_file = fs::File::create(&out_path)
            .map_err(|e| format!("Ошибка создания {}: {}", out_path.display(), e))?;
        std::io::copy(&mut entry, &mut out_file)
            .map_err(|e| format!("Ошибка распаковки {}: {}", clean, e))?;
        count += 1;
    }
    Ok(count)
}

/// Удаление содержимого папки (перед распаковкой новой версии)
fn clear_dir(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = fs::remove_dir_all(&path);
            } else {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// Поиск `sd-server.exe` под папкой (архив может иметь вложенную структуру).
fn find_server_dir(root: &Path) -> Option<PathBuf> {
    let mut queue: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(cur) = queue.pop() {
        let Ok(entries) = fs::read_dir(&cur) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                queue.push(path);
            } else if path.file_name().map(|n| n == "sd-server.exe").unwrap_or(false) {
                return Some(cur);
            }
        }
    }
    None
}

/// Подъём содержимого вложенной папки с sd-server.exe в корень папки варианта.
fn lift_server_files(variant_root: &Path, on_log: &dyn Fn(String)) -> Result<(), String> {
    if variant_root.join("sd-server.exe").exists() {
        return Ok(());
    }
    let Some(src) = find_server_dir(variant_root) else {
        return Err("После распаковки sd-server.exe не найден — архив повреждён или изменил структуру.".to_string());
    };
    if src == variant_root {
        return Ok(());
    }
    on_log(format!("📁 Архив имел вложенную структуру — поднимаю файлы из {}", src.display()));
    let entries = fs::read_dir(&src).map_err(|e| format!("Ошибка чтения {}: {}", src.display(), e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let dest = variant_root.join(&name);
        if dest.exists() {
            continue;
        }
        fs::rename(&path, &dest).map_err(|e| format!("Ошибка переноса {}: {}", name, e))?;
    }
    let mut cur = src.clone();
    while cur != *variant_root {
        let _ = fs::remove_dir(&cur);
        cur = match cur.parent() {
            Some(p) => p.to_path_buf(),
            None => break,
        };
    }
    Ok(())
}

/// Имена ассетов движка: `sd-{tag}-bin-win-{variant}-x64.zip`
/// (напр. `sd-master-28b454b-bin-win-cuda12-x64.zip`).
/// cudart-архив (`cudart-sd-bin-...`) — НЕ кандидат в движок.
fn asset_name_candidates(spec: &SourceSpec, tag: &str, variant: &str) -> Vec<String> {
    vec![format!(
        "{}{}-bin-win-{}-x64.zip",
        spec.asset_prefix, tag, variant
    )]
}

/// Фактический вариант из имени ассета:
/// "sd-master-28b454b-bin-win-cuda12-x64.zip" → "cuda12".
fn variant_from_asset_name(name: &str) -> Option<String> {
    let stem = name.strip_suffix("-x64.zip")?;
    let idx = stem.rfind("-win-")?;
    let v = &stem[idx + 5..];
    if v.is_empty() {
        return None;
    }
    // Нормализация: rocm-7.14.0 → rocm (папка зависит от выбора юзера).
    if v.starts_with("rocm") {
        return Some(VARIANT_ROCM.to_string());
    }
    Some(v.to_string())
}

/// Поиск ассета по семейству (смена минорной версии в имени не ломает установку).
fn find_asset_by_family<'a>(
    release: &'a GitHubRelease,
    family: EngineFamily,
) -> Option<(&'a GitHubAsset, String)> {
    let needles: &[&str] = match family {
        EngineFamily::Cuda12 => &["-win-cuda12", "-win-cuda-12"],
        EngineFamily::Vulkan => &["-win-vulkan"],
        EngineFamily::Rocm => &["-win-rocm", "-win-hip"],
        EngineFamily::Cpu => &["-win-cpu"],
    };
    for asset in &release.assets {
        if asset.name.contains("cudart") {
            continue;
        }
        if !asset.name.ends_with("-x64.zip") {
            continue;
        }
        if needles.iter().any(|n| asset.name.contains(n)) {
            let actual = variant_from_asset_name(&asset.name)
                .unwrap_or_else(|| match family {
                    EngineFamily::Cuda12 => VARIANT_CUDA12.to_string(),
                    EngineFamily::Vulkan => VARIANT_VULKAN.to_string(),
                    EngineFamily::Rocm => VARIANT_ROCM.to_string(),
                    EngineFamily::Cpu => VARIANT_CPU.to_string(),
                });
            return Some((asset, actual));
        }
    }
    None
}

fn find_engine_asset<'a>(
    release: &'a GitHubRelease,
    spec: &SourceSpec,
    variant: &str,
) -> Option<(&'a GitHubAsset, String)> {
    let tag = &release.tag_name;
    let candidates = asset_name_candidates(spec, tag, variant);
    for name in &candidates {
        if let Some(asset) = release.assets.iter().find(|a| a.name == *name) {
            return Some((asset, variant.to_string()));
        }
    }
    // Фолбэк по маске: могло измениться форматирование имени
    let prefix = &spec.asset_prefix;
    if let Some(asset) = release.assets.iter().find(|a| {
        a.name.starts_with(&format!("{}{}-bin-win-{}", prefix, tag, variant))
            && a.name.ends_with("-x64.zip")
            && !a.name.contains("cudart")
    }) {
        return Some((asset, variant.to_string()));
    }
    // Фолбэк по семейству
    if let Some(hit) = find_asset_by_family(release, EngineFamily::from_variant(variant)) {
        return Some(hit);
    }
    None
}

/// Ищет отдельный архив CUDA-рантайма (паттерн из SourceSpec.cudart_pattern).
/// Без cublas DLL ggml-бэкенд не грузится и движок тихо уходит в CPU.
/// ВАЖНО: НИКОГДА не возвращать main-bin ассет (`-bin-`) — это движок, не cudart.
fn find_cudart_asset<'a>(
    release: &'a GitHubRelease,
    spec: &SourceSpec,
    variant: &str,
) -> Option<(&'a GitHubAsset, String)> {
    release.assets.iter().find_map(|a| {
        if !a.name.contains(&spec.cudart_pattern) {
            return None;
        }
        if !a.name.ends_with("-x64.zip") {
            return None;
        }
        // cudart-архив должен относиться к тому же семейству (cu12)
        let actual = variant_from_asset_name(&a.name).unwrap_or_else(|| variant.to_string());
        Some((a, actual))
    })
}

/// Установка (или обновление) варианта бекенда из указанного источника.
/// Ставится в `backends/<source>/<variant>/`, остальные не трогаются.
/// Прогресс — через единый tauri-plugin-downloader (`downloader:progress`).
pub async fn install<L: Fn(String) + Send + Sync>(
    dir: &Path,
    source: &str,
    variant: &str,
    on_log: L,
) -> Result<EngineMeta, String> {
    let spec = sources::source_spec(source)
        .ok_or_else(|| format!("Неизвестный источник движка: {}", source))?;
    let target = variant_dir(dir, source, variant);
    fs::create_dir_all(&target).map_err(|e| format!("Не удалось создать папку {}: {}", target.display(), e))?;

    // Удаляем старые файлы варианта ДО скачивания.
    clear_dir(&target);

    on_log(format!(
        "🔄 Источник «{}», вариант «{}»: поиск актуального релиза с готовым движком…",
        spec.label, variant
    ));
    let client = http_client()?;
    let release = fetch_release_with_engine(&client, &spec, variant).await?;

    let asset = find_engine_asset(&release, &spec, variant).ok_or_else(|| {
        format!(
            "В релизе {} не найден ассет движка для варианта «{}». Возможно, формат релизов изменился — сообщите разработчику.",
            release.tag_name, variant
        )
    })?;
    let actual_variant = asset.1.clone();
    if actual_variant != variant {
        on_log(format!(
            "ℹ️ Точный вариант {} в релизе не найден — используется {} (совместим).",
            variant, actual_variant
        ));
    }

    let zip_path = target.join("engine.zip");
    tauri_plugin_downloader::download(
        &asset.0.browser_download_url,
        &zip_path,
        tauri_plugin_downloader::DownloadOptions {
            label: format!("Движок {}", actual_variant),
            kind: "engine".into(),
            expected_size: Some(asset.0.size),
            ..Default::default()
        },
        Some(&on_log),
    )
    .await?;

    if let Some(digest) = &asset.0.digest {
        let expected = digest.strip_prefix("sha256:").unwrap_or(digest);
        let actual = sha256_file(&zip_path)?;
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(&zip_path);
            return Err(format!(
                "Контрольная сумма не совпала! Ожидалось {}, получено {}. Загрузка повреждена.",
                expected, actual
            ));
        }
        on_log("✅ Контрольная сумма SHA-256 подтверждена".to_string());
    }

    let file_count = extract_all(&zip_path, &target, &on_log)?;
    let _ = fs::remove_file(&zip_path);

    // Архивы с вложенной структурой — поднимаем бинарь наверх
    lift_server_files(&target, &on_log)?;

    // Рантайм VC++ рядом с движком
    ensure_vc_redist(&target, &on_log);

    // ── CUDA-рантайм (дополнение) ──
    let main_asset_is_cudart = asset.0.name.contains("cudart");
    let family = EngineFamily::from_variant(&actual_variant);
    if !main_asset_is_cudart && matches!(family, EngineFamily::Cuda12) {
        if let Some(cudart) = find_cudart_asset(&release, &spec, &actual_variant) {
            on_log(format!("⬇️ Дополнение CUDA-рантайма: {}", cudart.0.name));
            let cudart_zip = target.join("cudart.zip");
            tauri_plugin_downloader::download(
                &cudart.0.browser_download_url,
                &cudart_zip,
                tauri_plugin_downloader::DownloadOptions {
                    label: format!("CUDA runtime {}", actual_variant),
                    kind: "engine".into(),
                    expected_size: Some(cudart.0.size),
                    ..Default::default()
                },
                Some(&on_log),
            )
            .await?;
            if let Some(digest) = &cudart.0.digest {
                let expected = digest.strip_prefix("sha256:").unwrap_or(digest);
                let actual = sha256_file(&cudart_zip)?;
                if !actual.eq_ignore_ascii_case(expected) {
                    let _ = fs::remove_file(&cudart_zip);
                    return Err(format!(
                        "Контрольная сумма CUDA-рантайма не совпала! Ожидалось {}, получено {}. Загрузка повреждена.",
                        expected, actual
                    ));
                }
            }
            let cudart_count = extract_all(&cudart_zip, &target, &on_log)?;
            let _ = fs::remove_file(&cudart_zip);
            on_log(format!("✅ CUDA-рантайм распакован: {} файлов", cudart_count));
        } else {
            on_log(format!(
                "⚠️ В релизе {} не найден архив CUDA-рантайма — GPU-режим может не работать.",
                release.tag_name
            ));
        }
        // ── Guard: cublas DLL ОБЯЗАН быть после установки CUDA-варианта ──
        let required_dll = "cublas64_12.dll";
        if !target.join(required_dll).exists() {
            let _ = clear_dir(&target);
            let _ = fs::remove_dir_all(&target);
            return Err(format!(
                "После установки CUDA-рантайма не найден {} в {}.\n\
                 Без него GPU-режим не работает.\n\
                 Возможно, формат релизов источника «{}» изменился — сообщите разработчику.",
                required_dll, target.display(), source
            ));
        }
    }

    let meta = EngineMeta {
        tag: release.tag_name.clone(),
        variant: actual_variant.clone(),
        source: source.to_string(),
        installed_at: chrono_now(),
    };
    let data = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    fs::write(meta_path(dir, source, variant), data)
        .map_err(|e| format!("Ошибка записи метаданных: {}", e))?;

    on_log(format!(
        "✅ Бекенд установлен: {} (источник {}, вариант {}). Распаковано файлов: {}.",
        release.tag_name, source, actual_variant, file_count
    ));
    Ok(meta)
}

/// Проверка наличия обновления конкретного варианта (только проверка, не установка)
pub async fn check_update<L: Fn(String)>(
    dir: &Path,
    source: &str,
    variant: &str,
    on_log: L,
) -> Result<Option<String>, String> {
    let meta = match installed_meta(dir, source, variant) {
        Some(m) => m,
        None => return Ok(None),
    };
    let spec = sources::source_spec(source)
        .ok_or_else(|| format!("Неизвестный источник движка: {}", source))?;
    let client = http_client()?;
    let release = fetch_release_with_engine(&client, &spec, variant).await?;
    if release.tag_name != meta.tag {
        on_log(format!(
            "🔄 Доступно обновление бекенда ({}): {} → {}",
            variant, meta.tag, release.tag_name
        ));
        Ok(Some(release.tag_name))
    } else {
        on_log(format!("Бекенд sd.cpp актуален ({}): {}", variant, meta.tag));
        Ok(None)
    }
}

/// Удаление конкретного варианта бекенда.
pub fn remove<L: Fn(String)>(dir: &Path, source: &str, variant: &str, on_log: &L) -> Result<(), String> {
    let target = variant_dir(dir, source, variant);
    if !target.exists() {
        return Ok(());
    }
    clear_dir(&target);
    let _ = fs::remove_dir(&target);
    on_log(format!(
        "🗑️ Бекенд «{}» удалён. Установите его заново, чтобы пользоваться этим режимом.",
        variant_label(variant)
    ));
    Ok(())
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_from_asset_name_parses_sd_format() {
        assert_eq!(
            variant_from_asset_name("sd-master-28b454b-bin-win-cuda12-x64.zip").unwrap(),
            "cuda12"
        );
        assert_eq!(
            variant_from_asset_name("sd-master-28b454b-bin-win-cpu-x64.zip").unwrap(),
            "cpu"
        );
        assert_eq!(
            variant_from_asset_name("sd-master-28b454b-bin-win-vulkan-x64.zip").unwrap(),
            "vulkan"
        );
        assert_eq!(
            variant_from_asset_name("sd-master-28b454b-bin-win-rocm-7.14.0-x64.zip").unwrap(),
            "rocm"
        );
    }

    #[test]
    fn family_mapping_and_labels() {
        assert_eq!(EngineFamily::from_variant("cuda12"), EngineFamily::Cuda12);
        assert_eq!(EngineFamily::from_variant("rocm"), EngineFamily::Rocm);
        assert!(EngineFamily::Cuda12.is_gpu());
        assert!(!EngineFamily::Cpu.is_gpu());
        assert!(is_known_variant("cuda12"));
        assert!(!is_known_variant("cuda-13.3"));
    }

    #[test]
    fn resolve_variant_prefers_user_choice() {
        assert_eq!(resolve_variant(Some("cpu")), "cpu");
        assert_eq!(resolve_variant(Some("unknown-variant")).len() > 0, true);
    }

    #[test]
    fn asset_candidates_use_sd_prefix() {
        let spec = sources::source_spec("leejet").expect("leejet");
        let c = asset_name_candidates(&spec, "master-899-28b454b", "cuda12");
        assert_eq!(c, vec!["sd-master-899-28b454b-bin-win-cuda12-x64.zip"]);
    }

    #[test]
    fn find_engine_asset_skips_cudart() {
        let spec = sources::source_spec("leejet").expect("leejet");
        let rel = GitHubRelease {
            tag_name: "master-899-28b454b".to_string(),
            prerelease: false,
            assets: vec![
                GitHubAsset {
                    name: "cudart-sd-bin-win-cu12-x64.zip".to_string(),
                    browser_download_url: "https://example.com/c".into(),
                    size: 1,
                    digest: None,
                },
                GitHubAsset {
                    name: "sd-master-899-28b454b-bin-win-cuda12-x64.zip".to_string(),
                    browser_download_url: "https://example.com/e".into(),
                    size: 2,
                    digest: None,
                },
            ],
        };
        let (asset, actual) = find_engine_asset(&rel, &spec, "cuda12").expect("engine");
        assert!(asset.name.starts_with("sd-master"));
        assert_eq!(actual, "cuda12");
        let (cudart, _) = find_cudart_asset(&rel, &spec, "cuda12").expect("cudart");
        assert!(cudart.name.contains("cudart-sd-bin"));
    }
}

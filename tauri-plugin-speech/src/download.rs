use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::io::AsyncWriteExt;

/// Адрес GitHub API последнего релиза CrispASR (для списка бинарей движка и проверки обновлений).
pub const RELEASE_API: &str = "https://api.github.com/repos/CrispStrobe/CrispASR/releases/latest";

/// Описание пресета TTS-движка CrispASR.
///
/// Пресеты грузятся из `tts_models.json` (встроенного в плагин + рядом с exe хоста).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TtsPreset {
    pub id: String,
    pub label: String,
    pub backend: String,
    pub model_file: String,
    pub model_url: String,
    pub codec: Option<(String, String)>,
    pub voice: Option<(String, String)>,
    pub extras: Vec<(String, String)>,
    pub voice_type: String,
    pub builtin_voices: Vec<String>,
    pub supports_instruct: bool,
    /// Поддерживает ли модель русский язык на синтез.
    pub supports_russian: bool,
    /// Примерный вес скачиваемых GGUF (для отображения в GUI).
    pub size: String,
}

static PRESETS_CACHE: OnceLock<Vec<TtsPreset>> = OnceLock::new();

/// Загружает пресеты из `tts_models.json`.
///
/// Порядок резолва файла:
/// 1) `tts_models.json` рядом с текущим exe;
/// 2) подъём вверх по каталогам от exe (ловит корень проекта в dev-сборке);
/// 3) встроенная копия (`include_str!`), если файл не найден или невалиден.
pub fn presets() -> &'static [TtsPreset] {
    PRESETS_CACHE.get_or_init(|| {
        if let Some(path) = find_tts_models_json() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(v) = serde_json::from_str::<Vec<TtsPreset>>(&text) {
                    if !v.is_empty() {
                        return v;
                    }
                }
            }
        }
        serde_json::from_str(EMBEDDED_TTS_MODELS_JSON)
            .expect("встроенный tts_models.json невалиден")
    })
}

/// Встроенная копия `tts_models.json` из корня плагина (на момент компиляции).
const EMBEDDED_TTS_MODELS_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tts_models.json"));

/// Ищет `tts_models.json`, начиная от каталога текущего exe и поднимаясь вверх.
fn find_tts_models_json() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?;
    for _ in 0..12 {
        let cand = dir.join("tts_models.json");
        if cand.is_file() {
            return Some(cand);
        }
        dir = dir.parent()?;
    }
    None
}

pub fn preset_by_id(id: &str) -> Option<&'static TtsPreset> {
    let id_low = id.to_lowercase();
    presets().iter().find(|p| p.id.to_lowercase() == id_low)
}

pub fn preset_backend(id: &str) -> Option<&'static str> {
    preset_by_id(id).map(|p| p.backend.as_str())
}

pub fn list_presets() -> Vec<serde_json::Value> {
    presets()
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "label": p.label,
                "backend": p.backend,
                "has_codec": p.codec.is_some(),
                "has_voice": p.voice.is_some(),
                "voice_type": p.voice_type,
                "builtin_voices": p.builtin_voices,
                "supports_instruct": p.supports_instruct,
                "supports_russian": p.supports_russian,
                "size": p.size,
            })
        })
        .collect()
}

/// Информация о доступном бинаре движка (один вариант сборки под Windows).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineBackendInfo {
    pub id: String,
    pub label: String,
    pub asset_name: String,
    pub url: String,
    pub tag: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// Превращает имя ассета (`crispasr-windows-x86_64-cuda-non-cuda.zip`) в уникальный
/// `slug` (`cuda-non-cuda`), отрезая префикс `crispasr[-windows-x86_64]-` и суффикс `.zip`.
fn asset_slug(name: &str) -> Option<String> {
    let n = name.to_ascii_lowercase();
    if !n.contains("windows") || !n.ends_with(".zip") {
        return None;
    }
    let stripped = n
        .strip_prefix("crispasr-windows-x86_64-")
        .or_else(|| n.strip_prefix("crispasr-"))
        .unwrap_or(&n)
        .strip_suffix(".zip")
        .unwrap_or(&n);
    let slug = stripped.trim_matches('-').to_string();
    if slug.is_empty() {
        None
    } else {
        Some(slug)
    }
}

/// Классифицирует Windows-ассет движка. Возвращает `(id, label)`.
fn classify_backend(name: &str) -> Option<(String, String)> {
    let slug = asset_slug(name)?;

    if slug.contains("non-cuda") {
        return None;
    }

    if slug.starts_with("cuda13") {
        return Some((
            slug,
            "NVIDIA CUDA (GPU) — CUDA 13, для новых видеокарт (RTX 20+)".to_string(),
        ));
    }
    if slug.starts_with("cuda") {
        return Some((
            slug,
            "NVIDIA CUDA (GPU) — CUDA 12, для старых видеокарт".to_string(),
        ));
    }

    let (cat, detail) = if slug.starts_with("cpu-legacy") {
        ("CPU (legacy SSE2)", slug.strip_prefix("cpu-legacy").unwrap_or(""))
    } else if slug.starts_with("cpu") {
        ("CPU (AVX2)", slug.strip_prefix("cpu").unwrap_or(""))
    } else if slug.contains("cuda") {
        ("NVIDIA CUDA (GPU)", slug.strip_prefix("cuda").unwrap_or(""))
    } else if slug.contains("rocm") || slug.contains("hip") {
        (
            "AMD ROCm (GPU)",
            slug.strip_prefix("rocm").or_else(|| slug.strip_prefix("hip")).unwrap_or(""),
        )
    } else if slug.contains("vulkan") {
        ("Vulkan (GPU)", slug.strip_prefix("vulkan").unwrap_or(""))
    } else {
        return None;
    };

    let detail = detail.trim_matches('-');
    let label = if detail.is_empty() {
        cat.to_string()
    } else {
        format!("{cat} [{detail}]")
    };
    Some((slug, label))
}

/// Запрашивает GitHub и возвращает список доступных Windows-бинарей движка.
pub async fn engine_backends() -> Result<Vec<EngineBackendInfo>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(RELEASE_API)
        .header("User-Agent", "SpeechLab")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("ошибка запроса релиза CrispASR: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub вернул {} для релиза", resp.status()));
    }
    let release: GhRelease = resp
        .json()
        .await
        .map_err(|e| format!("не удалось разобрать релиз: {e}"))?;
    let tag = release.tag_name.clone();
    let mut out = Vec::new();
    for a in release.assets {
        if let Some((id, label)) = classify_backend(&a.name) {
            out.push(EngineBackendInfo {
                id,
                label,
                asset_name: a.name,
                url: a.browser_download_url,
                tag: tag.clone(),
            });
        }
    }
    if out.is_empty() {
        return Err("в релизе не найдено Windows-бинарей движка".into());
    }
    Ok(out)
}

/// Папка движка относительно exe: `<exe_dir>/crispasr`.
pub fn default_engine_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    exe.parent()
        .map(|p| p.join("crispasr"))
        .unwrap_or_else(|| PathBuf::from("crispasr"))
}

/// Папка моделей относительно exe: `<exe_dir>/tts_models`.
pub fn default_models_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    exe.parent()
        .map(|p| p.join("tts_models"))
        .unwrap_or_else(|| PathBuf::from("tts_models"))
}

/// Резолвит путь к exe движка для выбранного бэкенда.
pub fn resolve_engine_exe(engine_dir: &str, backend_id: &str) -> PathBuf {
    let base: PathBuf = if engine_dir.is_empty() {
        default_engine_dir()
    } else {
        PathBuf::from(engine_dir)
    };
    let folder = base.join(backend_id);
    let direct = folder.join("crispasr.exe");
    if direct.exists() {
        return direct;
    }
    if let Some(found) = find_exe(&folder) {
        return found;
    }
    direct
}

/// Возвращает сохранённую версию установленного бэкенда (из `version.txt`), если есть.
pub fn installed_engine_version(engine_dir: &str, backend_id: &str) -> Option<String> {
    let base: PathBuf = if engine_dir.is_empty() {
        default_engine_dir()
    } else {
        PathBuf::from(engine_dir)
    };
    let vf = base.join(backend_id).join("version.txt");
    std::fs::read_to_string(&vf).ok().map(|s| s.trim().to_string())
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    kind: &str,
    name: &str,
    downloaded: u64,
    total: u64,
) {
    let _ = app.emit(
        "tts-download",
        json!({ "kind": kind, "name": name, "downloaded": downloaded, "total": total }),
    );
}

async fn download_to<R: Runtime>(
    app: &AppHandle<R>,
    url: &str,
    dest: &Path,
    kind: &str,
) -> Result<(), String> {
    let name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    emit_progress(app, kind, name, 0, 0);

    let resp = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("ошибка запроса {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("сервер вернул {} для {url}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("не удалось создать файл {}: {e}", dest.display()))?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("ошибка скачивания: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("ошибка записи: {e}"))?;
        downloaded += chunk.len() as u64;
        emit_progress(app, kind, name, downloaded, total);
    }
    file.flush().await.ok();
    Ok(())
}

/// Скачивает prebuilt `crispasr.exe` (Windows, выбранный вариант) в
/// `dest_dir/<backend_id>/`, распаковывает и сохраняет `version.txt` (тег релиза).
pub async fn download_engine<R: Runtime>(
    app: &AppHandle<R>,
    dest_dir: &str,
    backend_id: &str,
    url: &str,
    tag: &str,
) -> Result<String, String> {
    let dir: PathBuf = if dest_dir.is_empty() {
        default_engine_dir()
    } else {
        PathBuf::from(dest_dir)
    };
    let target = dir.join(backend_id);
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("не удалось создать папку {}: {e}", target.display()))?;

    let zip_path = target.join("crispasr.zip");
    download_to(app, url, &zip_path, "engine").await?;

    extract_zip(&zip_path, &target)?;
    let _ = std::fs::remove_file(&zip_path);

    // Сохраняем тег релиза для последующей проверки обновлений.
    let _ = std::fs::write(target.join("version.txt"), tag);

    let exe = find_exe(&target).ok_or_else(|| {
        format!("после распаковки не найден crispasr.exe в {}", target.display())
    })?;
    Ok(exe.to_string_lossy().to_string())
}

/// Скачивает все GGUF выбранного пресета в папку `dest_dir/<preset_id>/` и возвращает
/// пути к модели / codec / voice (codec и voice — пустые строки, если не нужны).
pub async fn download_model<R: Runtime>(
    app: &AppHandle<R>,
    preset_id: &str,
    dest_dir: &str,
) -> Result<serde_json::Value, String> {
    let preset = preset_by_id(preset_id)
        .ok_or_else(|| format!("неизвестный пресет TTS: {preset_id}"))?;

    let dest = PathBuf::from(dest_dir).join(&preset.id);
    std::fs::create_dir_all(&dest)
        .map_err(|e| format!("не удалось создать папку {}: {e}", dest.display()))?;

    let model = dest.join(&preset.model_file);
    download_to(app, &preset.model_url, &model, "model").await?;

    let mut codec_path = String::new();
    if let Some((cf, cu)) = &preset.codec {
        let p = dest.join(cf);
        download_to(app, cu, &p, "model").await?;
        codec_path = p.to_string_lossy().to_string();
    }

    let mut voice_path = String::new();
    if let Some((vf, vu)) = &preset.voice {
        let p = dest.join(vf);
        download_to(app, vu, &p, "model").await?;
        voice_path = p.to_string_lossy().to_string();
    }

    for (ef, eu) in &preset.extras {
        let p = dest.join(ef);
        download_to(app, eu, &p, "model").await?;
    }

    Ok(json!({
        "model": model.to_string_lossy().to_string(),
        "codec": codec_path,
        "voice": voice_path,
    }))
}

/// Возвращает для каждого пресета статус установки в `models_dir`.
pub fn list_installed_models(models_dir: &str) -> Vec<serde_json::Value> {
    let base: PathBuf = if models_dir.is_empty() {
        default_models_dir()
    } else {
        PathBuf::from(models_dir)
    };
    presets()
        .iter()
        .map(|p| {
            let folder = base.join(&p.id);
            let has_model = folder.join(&p.model_file).exists();
            let has_codec = match &p.codec {
                Some((cf, _)) => folder.join(cf).exists(),
                None => true,
            };
            let has_voice = match &p.voice {
                Some((vf, _)) => folder.join(vf).exists(),
                None => true,
            };
            let installed = has_model && has_codec && has_voice;
            json!({
                "id": p.id,
                "label": p.label,
                "installed": installed,
                "has_model": has_model,
                "has_codec": has_codec,
                "has_voice": has_voice,
                "voice_type": p.voice_type,
                "size": p.size,
                "supports_russian": p.supports_russian,
            })
        })
        .collect()
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("не удалось открыть zip {}: {e}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("не удалось прочитать zip: {e}"))?;
    archive
        .extract(dest.to_path_buf())
        .map_err(|e| format!("ошибка распаковки: {e}"))?;
    Ok(())
}

/// Рекурсивно ищет `crispasr.exe` (zip может класть в подпапку).
fn find_exe(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(found) = find_exe(&p) {
                return Some(found);
            }
        } else if p
            .file_name()
            .map(|n| n == "crispasr.exe")
            .unwrap_or(false)
        {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_cuda_variants_are_filtered_out() {
        assert!(classify_backend("crispasr-windows-x86_64-cuda-non-cuda.zip").is_none());
        assert!(classify_backend("crispasr-windows-x86_64-cuda13-non-cuda.zip").is_none());
        assert!(classify_backend("crispasr-windows-x86_64-cuda13-non-cuda-more.zip").is_none());
    }

    #[test]
    fn self_contained_backends_are_kept() {
        let (id, label) = classify_backend("crispasr-windows-x86_64-cuda.zip").unwrap();
        assert_eq!(id, "cuda");
        assert_eq!(label, "NVIDIA CUDA (GPU) — CUDA 12, для старых видеокарт");

        let (id, label) = classify_backend("crispasr-windows-x86_64-cuda13.zip").unwrap();
        assert_eq!(id, "cuda13");
        assert_eq!(label, "NVIDIA CUDA (GPU) — CUDA 13, для новых видеокарт (RTX 20+)");

        let (id, label) = classify_backend("crispasr-windows-x86_64-vulkan.zip").unwrap();
        assert_eq!(id, "vulkan");
        assert_eq!(label, "Vulkan (GPU)");
    }
}
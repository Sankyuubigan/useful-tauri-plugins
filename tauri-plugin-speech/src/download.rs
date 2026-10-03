use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Адрес GitHub API последнего релиза CrispASR (для списка бинарей движка и проверки обновлений).
pub const RELEASE_API: &str = "https://api.github.com/repos/CrispStrobe/CrispASR/releases/latest";

/// Описание пресета модели (TTS или STT) движка CrispASR.
///
/// Пресеты грузятся из `speech_models.json` (встроенного в плагин + рядом с exe хоста).
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

/// Срезает UTF-8 BOM, который `serde_json::from_str` не переваривает.
fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// Загружает пресеты из `speech_models.json`.
///
/// Порядок резолва файла:
/// 1) `speech_models.json` рядом с текущим exe;
/// 2) подъём вверх по каталогам от exe (ловит корень проекта в dev-сборке);
/// 3) встроенная копия (`include_str!`), если файл не найден или невалиден.
pub fn presets() -> &'static [TtsPreset] {
    PRESETS_CACHE.get_or_init(|| {
        if let Some(path) = find_speech_models_json() {
            match std::fs::read_to_string(&path) {
                Ok(text) => match serde_json::from_str::<Vec<TtsPreset>>(strip_bom(&text)) {
                    Ok(v) if !v.is_empty() => return v,
                    Ok(v) => {
                        log::error!(
                            "[tts] {} найден, но содержит {} пресет(ов) — пустой список, беру встроенную копию",
                            path.display(),
                            v.len()
                        );
                    }
                    Err(e) => {
                        log::error!(
                            "[tts] парсинг {} не удался: {e} — беру встроенную копию",
                            path.display()
                        );
                    }
                },
                Err(e) => {
                    log::error!(
                        "[tts] не удалось прочитать {}: {e} — беру встроенную копию",
                        path.display()
                    );
                }
            }
        } else {
            log::warn!("[tts] speech_models.json не найден рядом с exe — беру встроенную копию");
        }
        match serde_json::from_str::<Vec<TtsPreset>>(strip_bom(EMBEDDED_SPEECH_MODELS_JSON)) {
            Ok(v) if !v.is_empty() => v,
            Ok(v) => {
                log::error!(
                    "[tts] встроенная копия speech_models.json содержит {} пресетов — пусто!",
                    v.len()
                );
                v
            }
            Err(e) => {
                log::error!("[tts] встроенная копия speech_models.json битая: {e}");
                Vec::new()
            }
        }
    })
}

/// Встроенная копия `speech_models.json` из корня плагина (на момент компиляции).
const EMBEDDED_SPEECH_MODELS_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/speech_models.json"));

/// Ищет `speech_models.json`, начиная от каталога текущего exe и поднимаясь вверх.
fn find_speech_models_json() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?;
    for _ in 0..12 {
        let cand = dir.join("speech_models.json");
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

/// Папка движка: значение `plugins.speech.default_engine_dir` из конфига хоста,
/// иначе `<exe_dir>/crispasr`.
pub fn default_engine_dir() -> PathBuf {
    if let Some(v) = crate::config().default_engine_dir.as_ref() {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    exe.parent()
        .map(|p| p.join("crispasr"))
        .unwrap_or_else(|| PathBuf::from("crispasr"))
}

/// Папка моделей: значение `plugins.speech.default_models_dir` из конфига хоста,
/// иначе `<exe_dir>/tts_models`.
pub fn default_models_dir() -> PathBuf {
    if let Some(v) = crate::config().default_models_dir.as_ref() {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    exe.parent()
        .map(|p| p.join("tts_models"))
        .unwrap_or_else(|| PathBuf::from("tts_models"))
}

/// Базовая папка движков: явный `engine_dir` из настроек, иначе папка по умолчанию.
///
/// Единая точка вычисления корня (core §2.1 SSOT) — её используют все функции,
/// которые ходят по диску движка, и команда `tts_get_engine_status`.
pub fn engine_base(engine_dir: &str) -> PathBuf {
    if engine_dir.trim().is_empty() {
        default_engine_dir()
    } else {
        PathBuf::from(engine_dir)
    }
}

/// Резолвит путь к exe движка для выбранного бэкенда.
pub fn resolve_engine_exe(engine_dir: &str, backend_id: &str) -> PathBuf {
    let base = engine_base(engine_dir);
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
    let vf = engine_base(engine_dir).join(backend_id).join("version.txt");
    std::fs::read_to_string(&vf).ok().map(|s| s.trim().to_string())
}

/// Локально установленный бэкенд движка, найденный сканированием папки.
#[derive(Debug, Clone, Serialize)]
pub struct InstalledEngineBackend {
    /// Имя папки бэкенда (= `backend_id`, как его создаёт `download_engine`).
    pub id: String,
    /// Тег релиза из `version.txt`, если папка его содержит.
    pub installed_version: Option<String>,
}

/// Бэкенды, реально лежащие на диске: верхнеуровневые подпапки `engine_dir`,
/// внутри которых рекурсивно находится `crispasr.exe`.
///
/// Локальный источник правды для UI: работает без сети и находит папки, которых
/// уже нет в свежем релизе GitHub (layout менялся — старая раскладка остаётся
/// рабочей, но в `engine_backends()` её больше нет).
pub fn list_installed_engine_backends(engine_dir: &str) -> Vec<InstalledEngineBackend> {
    let base = engine_base(engine_dir);
    let Ok(entries) = std::fs::read_dir(&base) else {
        // Папки движка нет — это «ничего не установлено», а не ошибка:
        // вызывающий код сам сообщает юзеру, что движок не установлен.
        return Vec::new();
    };
    let mut out: Vec<InstalledEngineBackend> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let folder = e.path();
            let id = folder.file_name()?.to_str()?.to_string();
            find_exe(&folder).map(|_| InstalledEngineBackend {
                installed_version: installed_engine_version(engine_dir, &id),
                id,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Отвергает `backend_id`, который мог бы выйти за пределы папки движка
/// (пустой, `.`, `..`, с разделителями пути или с буквой диска вида `C:`).
fn validate_backend_id(backend_id: &str) -> Result<(), String> {
    let id = backend_id.trim();
    let bad = id.is_empty()
        || id == "."
        || id == ".."
        || id.contains('/')
        || id.contains('\\')
        || id.contains(':');
    if bad {
        return Err(format!("недопустимый идентификатор бэкенда: {backend_id:?}"));
    }
    Ok(())
}

/// Рекурсивный размер папки в байтах. `None` — корень не читается
/// (тогда UI не показывает размер, но удаление всё равно можно подтвердить).
fn dir_size(path: &Path) -> Option<u64> {
    let entries = std::fs::read_dir(path).ok()?;
    let mut total = 0u64;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            total += dir_size(&p).unwrap_or(0);
        } else {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    Some(total)
}

/// Удаляет папку установленного движка `<engine_dir>/<backend_id>` целиком.
///
/// Папка моделей (`models_dir`) не затрагивается — это соседняя, независимая папка.
/// Возвращает освобождённый размер (если удалось посчитать) и удалённый путь.
pub fn delete_engine(engine_dir: &str, backend_id: &str) -> Result<(Option<u64>, String), String> {
    validate_backend_id(backend_id)?;
    let target = engine_base(engine_dir).join(backend_id.trim());
    if !target.exists() {
        return Err(format!(
            "движок «{backend_id}» не установлен: папки {} нет",
            target.display()
        ));
    }
    if !target.is_dir() {
        return Err(format!("{} — это не папка движка", target.display()));
    }
    let size = dir_size(&target);
    std::fs::remove_dir_all(&target).map_err(|e| {
        format!(
            "не удалось удалить папку движка {}: {e} (возможно, движок ещё запущен)",
            target.display()
        )
    })?;
    Ok((size, target.to_string_lossy().to_string()))
}

/// Скачивает файл через ЕДИНЫЙ движок проекта (`tauri-plugin-downloader`).
///
/// Раньше здесь была своя копия на голом reqwest: без stall-детекта, без
/// таймаутов, без фолбэков. Зависший ответ CDN (0 байт бесконечно) навечно
/// блокировал команду: прогресс молчал, логов не было, кнопки не отвечали.
/// Теперь stall 60с → ошибка → следующий из 6 уровней (curl/PowerShell/…),
/// каждый шаг пишется в лог хоста, прогресс идёт событием `downloader:progress`.
///
/// `kind` попадает в событие прогресса ("engine" | "model") — по нему панели
/// отделяют движок от моделей.
async fn download_to(url: &str, dest: &Path, kind: &str) -> Result<(), String> {
    let label = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Загрузка".to_string());
    let opts = tauri_plugin_downloader::DownloadOptions {
        label,
        kind: kind.to_string(),
        ..Default::default()
    };
    tauri_plugin_downloader::download(url, dest, opts, None).await
}

/// Скачивает prebuilt `crispasr.exe` (Windows, выбранный вариант) в
/// `dest_dir/<backend_id>/`, распаковывает и сохраняет `version.txt` (тег релиза).
pub async fn download_engine(
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
    download_to(url, &zip_path, "engine").await?;

    extract_zip(&zip_path, &target)?;
    let _ = std::fs::remove_file(&zip_path);

    // Сохраняем тег релиза для последующей проверки обновлений. Ошибку НЕ
    // глотаем: молчаливо потерянный version.txt превращает установленный движок
    // в «версия неизвестна», и апдейты для него больше не проверяются.
    std::fs::write(target.join("version.txt"), tag).map_err(|e| {
        format!(
            "движок распакован, но не удалось записать version.txt в {}: {e}",
            target.display()
        )
    })?;

    let exe = find_exe(&target).ok_or_else(|| {
        format!("после распаковки не найден crispasr.exe в {}", target.display())
    })?;
    Ok(exe.to_string_lossy().to_string())
}

/// Скачивает все GGUF выбранного пресета в папку `dest_dir/<preset_id>/` и возвращает
/// пути к модели / codec / voice (codec и voice — пустые строки, если не нужны).
pub async fn download_model(
    preset_id: &str,
    dest_dir: &str,
) -> Result<serde_json::Value, String> {
    let preset = preset_by_id(preset_id)
        .ok_or_else(|| format!("неизвестный пресет TTS: {preset_id}"))?;

    let dest = PathBuf::from(dest_dir).join(&preset.id);
    std::fs::create_dir_all(&dest)
        .map_err(|e| format!("не удалось создать папку {}: {e}", dest.display()))?;

    let model = dest.join(&preset.model_file);
    download_to(&preset.model_url, &model, "model").await?;

    let mut codec_path = String::new();
    if let Some((cf, cu)) = &preset.codec {
        let p = dest.join(cf);
        download_to(cu, &p, "model").await?;
        codec_path = p.to_string_lossy().to_string();
    }

    let mut voice_path = String::new();
    if let Some((vf, vu)) = &preset.voice {
        let p = dest.join(vf);
        download_to(vu, &p, "model").await?;
        voice_path = p.to_string_lossy().to_string();
    }

    for (ef, eu) in &preset.extras {
        let p = dest.join(ef);
        download_to(eu, &p, "model").await?;
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

    #[test]
    fn backend_id_rejects_path_escapes() {
        // Защита `delete_engine` от выхода за пределы папки движка.
        for bad in [
            "",
            "   ",
            ".",
            "..",
            "../cuda",
            "..\\cuda",
            "a/b",
            "a\\b",
            "C:",
            "C:\\x",
        ] {
            assert!(
                validate_backend_id(bad).is_err(),
                "ожидался отказ для {bad:?}"
            );
        }
        for ok in ["cuda", "cuda13", "cpu", "cpu-legacy"] {
            assert!(validate_backend_id(ok).is_ok(), "ожидался пропуск для {ok:?}");
        }
    }

    #[test]
    fn engine_base_falls_back_to_default_when_dir_empty() {
        // Пустая строка и пробелы = «папка из настроек не задана» → дефолт.
        assert_eq!(engine_base(""), default_engine_dir());
        assert_eq!(engine_base("   "), default_engine_dir());
        assert_eq!(
            engine_base("D:\\nn\\crispasr"),
            PathBuf::from("D:\\nn\\crispasr")
        );
    }

    /// Временная папка теста с гарантированной уборкой (RAII).
    ///
    /// Класс, а не функция: `Drop` вызывается и при раскрутке стека, поэтому
    /// упавший ассерт больше не оставляет мусор в репозитории (core §1.2).
    /// Путь — воркспейс `target/test-scratch` (тот, что в `.gitignore` как
    /// `/target`), а не `target` внутри крейта: `/target` в игноре заякорен на
    /// корень монорепо, и вложенный `tauri-plugin-speech/target/` всплывал
    /// в `git status` как untracked.
    struct Scratch {
        path: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
            let workspace_target = manifest.parent().unwrap_or(manifest).join("target");
            let path = workspace_target
                .join("test-scratch")
                .join(format!("{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("не удалось создать временную папку теста");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn installed_backends_scan_reads_disk_without_network() {
        // Скан диска: папка с crispasr.exe (в т.ч. в подпапке) = установленный бэкенд,
        // пустая папка и файл — нет.
        let scratch = Scratch::new("eng-scan");
        let root = scratch.path();

        let nested = root.join("cuda13").join("crispasr-windows-x86_64-cuda13");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("crispasr.exe"), b"MZ").unwrap();
        std::fs::write(root.join("cuda13").join("version.txt"), "v1.2.3").unwrap();

        std::fs::create_dir_all(root.join("empty")).unwrap();
        std::fs::write(root.join("not-a-dir.txt"), b"x").unwrap();

        let found = list_installed_engine_backends(&root.to_string_lossy());

        assert_eq!(found.len(), 1, "ожидался только cuda13: {found:?}");
        assert_eq!(found[0].id, "cuda13");
        assert_eq!(found[0].installed_version.as_deref(), Some("v1.2.3"));

        // Несуществующая папка = «ничего не установлено», НЕ паника.
        assert!(list_installed_engine_backends("Z:\\нет\\такой\\папки").is_empty());
    }

    #[test]
    fn dir_size_sums_nested_files_or_reports_unreadable() {
        let scratch = Scratch::new("eng-size");
        let root = scratch.path();
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("a.bin"), vec![0u8; 100]).unwrap();
        std::fs::write(root.join("nested").join("b.bin"), vec![0u8; 23]).unwrap();

        assert_eq!(dir_size(root), Some(123));
        assert_eq!(dir_size(Path::new("Z:\\нет\\такой\\папки")), None);
    }

    #[test]
    fn delete_engine_removes_only_backend_folder() {
        // Ключевая гарантия: models_dir (соседняя папка) не затрагивается.
        let scratch = Scratch::new("eng-del");
        let root = scratch.path();

        let backend_dir = root.join("crispasr").join("cuda13");
        std::fs::create_dir_all(&backend_dir).unwrap();
        std::fs::write(backend_dir.join("crispasr.exe"), b"MZ").unwrap();
        std::fs::write(backend_dir.join("version.txt"), "v1.2.3").unwrap();

        let models_dir = root.join("tts_models");
        std::fs::create_dir_all(models_dir.join("cosyvoice3")).unwrap();
        std::fs::write(models_dir.join("cosyvoice3").join("model.gguf"), b"GGUF").unwrap();

        let base = root.join("crispasr").to_string_lossy().to_string();
        let (freed, path) = delete_engine(&base, "cuda13").unwrap();
        assert_eq!(freed, Some(2 + 6), "размер = exe (2) + version.txt (6)");
        assert!(path.ends_with("cuda13"));

        assert!(!backend_dir.exists(), "папка движка должна быть удалена");
        assert!(
            models_dir.join("cosyvoice3").join("model.gguf").exists(),
            "модели не должны пострадать при удалении движка"
        );

        // Повторное удаление — честная ошибка, а не «успех».
        assert!(delete_engine(&base, "cuda13").is_err());
    }
}
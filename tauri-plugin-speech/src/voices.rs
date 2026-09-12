use std::path::PathBuf;

use serde::Serialize;

use crate::audio::denoise::{denoise_mono, DenoiseOpts};
use crate::audio::wav;

/// Информация о сохранённом голосе из хранилища `<models_dir>/voices/<id>/`.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceInfo {
    /// Уникальный id (санированное имя папки).
    pub id: String,
    /// Отображаемое имя (то, что дал пользователь).
    pub name: String,
    /// Абсолютный путь к `voice.wav` (для передачи в `--voice`/тело запроса).
    pub path: String,
    /// Референсный текст (если указан).
    pub ref_text: String,
    /// Есть ли `avatar.jpg` у голоса.
    pub has_avatar: bool,
    /// ISO-время создания (RFC3339).
    pub created_at: String,
}

/// ЕДИНЫЙ стандарт длительности голосового референса (секунды): хранимые голоса
/// (`<id>/voice.wav`), клон-кэш (`.clone_cache/<id>.r2.wav`) и всё, что подаётся
/// на модель, обязаны быть ДО 10 секунд. Менять только здесь.
pub const MAX_REF_SEC: f32 = 10.0;

/// Обрезает моно-сэмплы до первых `MAX_REF_SEC` секунд (с начала).
fn trim_to_max_sec(mono: &[f32], rate: u32) -> Vec<f32> {
    let max_samples = (MAX_REF_SEC * rate as f32) as usize;
    if mono.len() > max_samples {
        mono[..max_samples].to_vec()
    } else {
        mono.to_vec()
    }
}

/// Приводит моно-сэмплы к каноническим 24 кГц mono (zonos/chatterbox клонируют
/// только из 16/24 кГц mono PCM, см. логи CrispASR).
pub fn to_24k_mono(mono: Vec<f32>, rate: u32) -> (Vec<f32>, u32) {
    const TARGET: u32 = 24000;
    if rate == TARGET {
        (mono, rate)
    } else {
        match crate::audio::denoise::resample(&mono, rate as usize, TARGET as usize) {
            Ok(r) => (r, TARGET),
            Err(_) => (mono, rate),
        }
    }
}

/// Корень хранилища голосов: `<models_dir>/voices`.
pub fn voices_root(models_dir: &str) -> PathBuf {
    let base: PathBuf = if models_dir.is_empty() {
        crate::download::default_models_dir()
    } else {
        PathBuf::from(models_dir)
    };
    base.join("voices")
}

/// Папка конкретного голоса: `<voices_root>/<id>/`.
fn voice_folder(root: &std::path::Path, id: &str) -> PathBuf {
    root.join(id)
}

/// Санирует имя пользователя в безопасный id папки:
/// не-алфавитно-цифровые символы → `_`, схлопывание повторов, обрезка краёв.
fn sanitize_id(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_underscore = true; // не начинаем с _
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "voice".to_string()
    } else {
        trimmed
    }
}

/// Читает манифест/файлы голоса `<id>/` либо собирает минимальный из наличия файлов.
pub(crate) fn read_voice(root: &std::path::Path, id: &str) -> Option<VoiceInfo> {
    let folder = voice_folder(root, id);
    let wav = folder.join("voice.wav");
    if !wav.exists() {
        return None;
    }
    let manifest = folder.join("voice.json");
    let (name, ref_text, has_avatar, created_at) = if let Ok(text) =
        std::fs::read_to_string(&manifest)
    {
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        (
            v["name"].as_str().unwrap_or(id).to_string(),
            v["ref_text"].as_str().unwrap_or("").to_string(),
            v["has_avatar"].as_bool().unwrap_or(false),
            v["created_at"].as_str().unwrap_or("").to_string(),
        )
    } else {
        (
            id.to_string(),
            String::new(),
            folder.join("avatar.jpg").exists(),
            String::new(),
        )
    };
    Some(VoiceInfo {
        id: id.to_string(),
        name,
        path: wav.to_string_lossy().to_string(),
        ref_text,
        has_avatar,
        created_at,
    })
}

/// Сканирует хранилище и возвращает список сохранённых голосов.
pub fn list_voices(models_dir: &str) -> Vec<VoiceInfo> {
    let root = voices_root(models_dir);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let fname = entry.file_name();
            let fname_str = fname.to_string_lossy();
            if fname_str.starts_with('.') {
                continue;
            }
            if let Some(info) = read_voice(&root, &fname_str) {
                out.push(info);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Резолвит значение поля `voice` для HTTP-запроса к CrispASR.
pub fn resolve_voice_for_server(models_dir: &str, voice: &str) -> Result<String, String> {
    let v = voice.trim();
    if v.is_empty() {
        return Ok(String::new());
    }
    let looks_like_path = v.contains('/')
        || v.contains('\\')
        || std::path::Path::new(v).is_absolute()
        || v.to_ascii_lowercase().ends_with(".wav")
        || v.to_ascii_lowercase().ends_with(".gguf");
    if looks_like_path && !std::path::Path::new(v).exists() {
        return Err(format!("файл голоса не найден: {v}"));
    }
    let _ = models_dir;
    Ok(v.to_string())
}

/// Добавляет голос в хранилище (отдельная папка `<id>/`).
pub fn add_voice<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    models_dir: &str,
    name: &str,
    src_audio: &str,
    ref_text: &str,
    avatar: &str,
    denoise: bool,
    denoise_strength: f32,
) -> Result<VoiceInfo, String> {
    if name.trim().is_empty() {
        return Err("укажите имя голоса".into());
    }
    if !std::path::Path::new(src_audio).exists() {
        return Err(format!("файл аудио не найден: {src_audio}"));
    }

    let root = voices_root(models_dir);
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("не удалось создать папку голосов {}: {e}", root.display()))?;

    let base_id = sanitize_id(name);
    let mut id = base_id.clone();
    let mut suffix = 2u32;
    while voice_folder(&root, &id).join("voice.wav").exists() {
        id = format!("{base_id}_{suffix}");
        suffix += 1;
    }
    let folder = voice_folder(&root, &id);
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("не удалось создать папку голоса {}: {e}", folder.display()))?;

    crate::log::app_log(app, &format!("[voices] конвертирую «{name}» в WAV..."));
    let (mono, rate) = crate::audio::decode_to_mono(src_audio)
        .map_err(|e| format!("не удалось декодировать аудио «{src_audio}»: {e}"))?;
    if mono.is_empty() {
        return Err("декодированное аудио пустое (тишина?)".into());
    }
    let mono = if denoise {
        denoise_mono(
            &mono,
            rate,
            &DenoiseOpts {
                enabled: true,
                strength: denoise_strength,
            },
        )
        .map_err(|e| format!("ошибка шумоподавления: {e}"))?
    } else {
        mono
    };
    let mono = trim_to_max_sec(&mono, rate);
    let (mono, rate) = to_24k_mono(mono, rate);
    let wav_path = folder.join("voice.wav");
    wav::write_wav(&wav_path.to_string_lossy(), &mono, rate)
        .map_err(|e| format!("не удалось записать WAV: {e}"))?;

    let _ = std::fs::write(folder.join("ref_text.txt"), ref_text.trim());

    let mut has_avatar = false;
    if !avatar.is_empty() && std::path::Path::new(avatar).exists() {
        let ext = std::path::Path::new(avatar)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp") {
            let _ = std::fs::copy(avatar, folder.join("avatar.jpg"));
            has_avatar = true;
        }
    }

    let created_at = chrono_now();
    let manifest = serde_json::json!({
        "id": id,
        "name": name,
        "ref_text": ref_text.trim(),
        "has_avatar": has_avatar,
        "created_at": created_at,
    });
    let _ = std::fs::write(
        folder.join("voice.json"),
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    );

    crate::log::app_log(app, &format!("[voices] голос «{name}» сохранён ({})", folder.display()));
    Ok(VoiceInfo {
        id,
        name: name.to_string(),
        path: wav_path.to_string_lossy().to_string(),
        ref_text: ref_text.trim().to_string(),
        has_avatar,
        created_at,
    })
}

/// Удаляет голос (папку `<id>/` и, для совместимости, старые плоские файлы) из хранилища.
pub fn delete_voice(models_dir: &str, id: &str) -> Result<(), String> {
    let root = voices_root(models_dir);
    let folder = voice_folder(&root, id);
    let mut removed = false;
    if folder.exists() {
        let _ = std::fs::remove_dir_all(&folder);
        removed = true;
    }
    for ext in ["wav", "txt", "json", "avatar.jpg"] {
        let f = root.join(format!("{id}.{ext}"));
        if f.exists() {
            let _ = std::fs::remove_file(f);
            removed = true;
        }
    }
    if !removed {
        return Err(format!("голос не найден: {id}"));
    }
    Ok(())
}

/// Обновляет метаданные и (опц.) референсное аудио/аватар существующего голоса.
pub fn update_voice<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    models_dir: &str,
    id: &str,
    name: &str,
    ref_text: &str,
    avatar: &str,
    src_audio: &str,
    denoise: bool,
    denoise_strength: f32,
) -> Result<VoiceInfo, String> {
    let root = voices_root(models_dir);
    let folder = voice_folder(&root, id);
    let wav = folder.join("voice.wav");
    if !wav.exists() {
        return Err(format!("голос не найден: {id}"));
    }

    if !src_audio.trim().is_empty() {
        if !std::path::Path::new(src_audio).exists() {
            return Err(format!("файл аудио не найден: {src_audio}"));
        }
        crate::log::app_log(app, &format!("[voices] перекодирую «{name}» в WAV..."));
        let (mono, rate) = crate::audio::decode_to_mono(src_audio)
            .map_err(|e| format!("не удалось декодировать аудио «{src_audio}»: {e}"))?;
        if mono.is_empty() {
            return Err("декодированное аудио пустое (тишина?)".into());
        }
        let mono = if denoise {
            denoise_mono(
                &mono,
                rate,
                &DenoiseOpts {
                    enabled: true,
                    strength: denoise_strength,
                },
            )
            .map_err(|e| format!("ошибка шумоподавления: {e}"))?
        } else {
            mono
        };
        let mono = trim_to_max_sec(&mono, rate);
        let (mono, rate) = to_24k_mono(mono, rate);
        wav::write_wav(&wav.to_string_lossy(), &mono, rate)
            .map_err(|e| format!("не удалось записать WAV: {e}"))?;
        let _ = std::fs::write(folder.join("ref_text.txt"), ref_text.trim());
    }

    let mut has_avatar = folder.join("avatar.jpg").exists();
    if avatar == "__REMOVE__" {
        let _ = std::fs::remove_file(folder.join("avatar.jpg"));
        has_avatar = false;
    } else if !avatar.trim().is_empty() && std::path::Path::new(avatar).exists() {
        let ext = std::path::Path::new(avatar)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp") {
            let _ = std::fs::copy(avatar, folder.join("avatar.jpg"));
            has_avatar = true;
        }
    }

    let prev_created = folder
        .join("voice.json")
        .as_path()
        .exists()
        .then(|| {
            std::fs::read_to_string(folder.join("voice.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .and_then(|v| v["created_at"].as_str().map(|s| s.to_string()))
        })
        .flatten()
        .unwrap_or_else(chrono_now);

    let manifest = serde_json::json!({
        "id": id,
        "name": name,
        "ref_text": ref_text.trim(),
        "has_avatar": has_avatar,
        "created_at": prev_created,
    });
    let _ = std::fs::write(
        folder.join("voice.json"),
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    );

    crate::log::app_log(app, &format!("[voices] голос «{name}» обновлён ({id})"));
    read_voice(&root, id).ok_or_else(|| format!("не удалось прочитать обновлённый голос: {id}"))
}

/// Возвращает байты аватара `<id>/avatar.jpg` (JPEG) либо `None`, если его нет.
pub fn voice_avatar(models_dir: &str, id: &str) -> Option<Vec<u8>> {
    let p = voice_folder(&voices_root(models_dir), id).join("avatar.jpg");
    std::fs::read(p).ok()
}

/// Возвращает байты референсного WAV `<id>/voice.wav` (полный файл) для проигрывания.
pub fn voice_audio(models_dir: &str, id: &str) -> Result<Vec<u8>, String> {
    let p = voice_folder(&voices_root(models_dir), id).join("voice.wav");
    std::fs::read(&p).map_err(|e| format!("не удалось прочитать аудио голоса {id}: {e}"))
}

/// RFC3339-подобное UTC-время (без внешних зависимостей — через std::time).
fn chrono_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    const MONTH_DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let days = secs / 86400;
    let mut y = 1970u32;
    let mut rem = days;
    loop {
        let leap = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
            366
        } else {
            365
        };
        if rem < leap {
            break;
        }
        rem -= leap;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mut m = 0usize;
    let mut d = rem;
    loop {
        let md = (MONTH_DAYS[m] + if m == 1 && leap { 1 } else { 0 }) as u64;
        if d < md {
            break;
        }
        d -= md;
        m += 1;
    }
    let day = d + 1;
    let month = m + 1;
    let hour = (secs % 86400) / 3600;
    let min = (secs % 3600) / 60;
    let sec = secs % 60;
    format!("{y}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_models_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("speechlab_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn sanitize_id_basic() {
        assert_eq!(sanitize_id("Morgan Freeman"), "morgan_freeman");
        assert_eq!(sanitize_id("голос-1!"), "голос_1");
        assert_eq!(sanitize_id("  "), "voice");
        assert!(!sanitize_id("Влад").is_empty());
    }

    #[test]
    fn resolve_does_not_pollute_store() {
        let models = tmp_models_dir().join("pollute_test");
        let _ = std::fs::create_dir_all(&models);
        let outside = std::env::temp_dir().join(format!("outside_{}.wav", std::process::id()));
        {
            let mut f = std::fs::File::create(&outside).unwrap();
            f.write_all(b"RIFF....WAVE").unwrap();
        }
        let res = resolve_voice_for_server(&models.to_string_lossy(), &outside.to_string_lossy());
        assert!(res.is_ok(), "resolve не должен падать на существующем файле");
        let vr = voices_root(&models.to_string_lossy());
        let entries: Vec<_> = std::fs::read_dir(&vr).map(|r| r.flatten().collect()).unwrap_or_default();
        assert!(entries.is_empty(), "resolve_voice_for_server не должен копировать файлы в хранилище");
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&models);
    }

    #[test]
    fn list_voices_excludes_clone_cache() {
        let models = tmp_models_dir();
        let root = voices_root(&models.to_string_lossy());
        std::fs::create_dir_all(&root).unwrap();
        let vfolder = root.join("my_voice");
        std::fs::create_dir_all(&vfolder).unwrap();
        std::fs::write(vfolder.join("voice.wav"), b"RIFF....WAVE").unwrap();
        std::fs::write(vfolder.join("voice.json"), r#"{"name":"My Voice"}"#).unwrap();
        let cache = root.join(".clone_cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("my_voice.wav"), b"RIFF....WAVE").unwrap();
        let hidden = root.join(".hidden");
        std::fs::create_dir_all(&hidden).unwrap();
        std::fs::write(hidden.join("voice.wav"), b"RIFF....WAVE").unwrap();

        let voices = list_voices(&models.to_string_lossy());
        assert_eq!(voices.len(), 1, "в списке только сохранённый голос");
        assert_eq!(voices[0].id, "my_voice");
        assert_eq!(voices[0].name, "My Voice");

        let _ = std::fs::remove_dir_all(&models);
    }
}
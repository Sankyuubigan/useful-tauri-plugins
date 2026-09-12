use crate::voices;

/// Лимиты длительности референса (в секундах) для моделей TTS, клонирующих голос
/// из произвольного WAV (zero-shot cloning). Возвращает `(min_seconds, max_seconds)`.
///
/// MAX для ВСЕХ clone-бэкендов — единый канонический `voices::MAX_REF_SEC` (10 с):
/// хранимый голос, клон-кэш и то, что подаётся на модель, совпадают.
/// `None` — бэкенд не клонирует из WAV в рантайме через `--voice`.
pub fn clone_reference_limits(backend: &str) -> Option<(f32, f32)> {
    const MIN_CLONE_REF: f32 = 3.0;
    let supported = matches!(
        backend,
        "cosyvoice3-tts"
            | "cosyvoice3-tts-rl"
            | "f5-tts"
            | "zonos"
            | "voxcpm2-tts"
            | "dots-tts"
            | "irodori-tts"
            | "indextts"
            | "omnivoice"
            | "moss-tts"
            | "moss-tts-local"
            | "confucius4-tts"
            | "chatterbox"
            | "chatbox"
            | "pocket-tts"
            | "vibevoice-1.5b"
            | "tada"
            | "tada-1b"
            | "tada-3b-ml"
            | "qwen3-tts"
            | "qwen3-tts-1.7b-base"
            | "qwen3-tts-1.7b-voicedesign"
    );
    if supported {
        Some((MIN_CLONE_REF, crate::voices::MAX_REF_SEC))
    } else {
        None
    }
}

/// Бэкенды, которым для WAV-клонирования обязателен `ref_text` (транскрипт
/// образца), и которые читают его **только** через канал загрузки голоса
/// (`POST /v1/voices`, поле `transcript`), игнорируя поле `ref_text` в теле
/// `POST /v1/audio/speech`.
pub fn backend_needs_ref_text(backend: &str) -> bool {
    matches!(
        backend,
        "qwen3-tts" | "qwen3-tts-1.7b-base" | "qwen3-tts-1.7b-voicedesign" | "tada" | "tada-1b"
            | "tada-3b-ml"
    )
}

/// Подготовленный референс для клонирования.
pub struct CloneRef {
    /// Абсолютный путь к (возможно обрезанному) WAV-референсу.
    pub voice_path: String,
    /// Референсный текст (транскрипт) из `<id>.txt`, если есть; иначе пусто.
    pub ref_text: String,
}

/// Готовит WAV-референс для клонирования: при наличии лимита длительности
/// обрезает исходник до первых `voices::MAX_REF_SEC` секунд (кэш
/// `.clone_cache/<ascii_id>.r2.wav` + маркер версии `.r2.meta`), исходник НЕ
/// трогает. Кэш пересоздаётся при изменении исходника ИЛИ при смене лимита
/// (маркер хранит max_sec + mtime источника). Возвращает путь + ref_text.
pub fn prepare_clone_reference(
    models_dir: &str,
    src_wav: &str,
    id: &str,
    backend: &str,
) -> Result<CloneRef, String> {
    let src = std::path::Path::new(src_wav);
    if !src.exists() {
        return Err(format!(
            "файл голоса не найден: {src_wav} (сначала добавьте голос или выберите WAV)"
        ));
    }

    let root = voices::voices_root(models_dir);
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("не удалось создать папку голосов {}: {e}", root.display()))?;
    let cache_id = crate::tts::ascii_voice_name(id);

    let (voice_path, ref_text) = match clone_reference_limits(backend) {
        Some((_min, max)) => {
            let cache_dir = root.join(".clone_cache");
            let _ = std::fs::create_dir_all(&cache_dir);
            let trimmed = cache_dir.join(format!("{cache_id}.r2.wav"));
            let meta_path = trimmed.with_extension("meta");
            let src_mtime_ns = file_mtime_ns(src);
            let need_rebuild = !trimmed.exists()
                || !meta_path.exists()
                || read_meta(&meta_path) != Some((crate::voices::MAX_REF_SEC as u32, src_mtime_ns));
            if need_rebuild {
                let (mono, rate) = crate::audio::decode_to_mono(src_wav)
                    .map_err(|e| format!("не удалось декодировать референс «{id}»: {e}"))?;
                if mono.is_empty() {
                    return Err(format!("референс «{id}» пустой (тишина?)"));
                }
                let max_samples = (max * rate as f32) as usize;
                let taken = if mono.len() > max_samples {
                    &mono[..max_samples]
                } else {
                    &mono[..]
                };
                const TARGET_RATE: u32 = 24000;
                let resampled = if rate == TARGET_RATE {
                    taken.to_vec()
                } else {
                    crate::audio::denoise::resample(
                        taken,
                        rate as usize,
                        TARGET_RATE as usize,
                    )
                    .map_err(|e| format!("не удалось ресемплировать референс «{id}»: {e}"))?
                };
                crate::audio::wav::write_wav(
                    &trimmed.to_string_lossy(),
                    &resampled,
                    TARGET_RATE,
                )
                .map_err(|e| format!("не удалось записать обрезанный референс: {e}"))?;
                write_meta(&meta_path, crate::voices::MAX_REF_SEC as u32, src_mtime_ns);
            }
            let ref_text = read_ref_text(&root, id);
            if !ref_text.trim().is_empty() {
                let _ = std::fs::write(trimmed.with_extension("txt"), ref_text.trim());
            }
            (trimmed.to_string_lossy().to_string(), ref_text)
        }
        None => (src.to_string_lossy().to_string(), read_ref_text(&root, id)),
    };

    Ok(CloneRef { voice_path, ref_text })
}

fn read_ref_text(root: &std::path::Path, voice_id: &str) -> String {
    let txt = root.join(voice_id).join("ref_text.txt");
    if txt.exists() {
        std::fs::read_to_string(&txt).unwrap_or_default()
    } else {
        String::new()
    }
}

fn file_mtime_ns(path: &std::path::Path) -> u64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn write_meta(path: &std::path::Path, max_sec: u32, mtime_ns: u64) {
    let _ = std::fs::write(path, format!("{max_sec}\n{mtime_ns}\n"));
}

fn read_meta(path: &std::path::Path) -> Option<(u32, u64)> {
    let s = std::fs::read_to_string(path).ok()?;
    let mut it = s.lines();
    let max: u32 = it.next()?.trim().parse().ok()?;
    let mtime: u64 = it.next()?.trim().parse().ok()?;
    Some((max, mtime))
}

/// Возвращает байты **обрезанного** референса голоса (такой же, какой пойдёт в
/// клонирование, до `voices::MAX_REF_SEC`). Бэкенд по умолчанию —
/// `cosyvoice3-tts` (тот же канонический лимит).
pub fn voice_trimmed_audio(
    models_dir: &str,
    id: &str,
    backend: &str,
) -> Result<Vec<u8>, String> {
    let root = voices::voices_root(models_dir);
    let wav = root.join(id).join("voice.wav");
    if !wav.exists() {
        return Err(format!("файл голоса не найден: {id}"));
    }
    let backend = if backend.is_empty() {
        "cosyvoice3-tts"
    } else {
        backend
    };
    let cr = prepare_clone_reference(models_dir, &wav.to_string_lossy(), id, backend)?;
    std::fs::read(&cr.voice_path)
        .map_err(|e| format!("не удалось прочитать обрезанный референс {}: {e}", cr.voice_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_known_backends() {
        assert_eq!(clone_reference_limits("cosyvoice3-tts"), Some((3.0, 10.0)));
        assert_eq!(clone_reference_limits("cosyvoice3-tts-rl"), Some((3.0, 10.0)));
        assert_eq!(clone_reference_limits("f5-tts"), Some((3.0, 10.0)));
        assert_eq!(clone_reference_limits("zonos"), Some((3.0, 10.0)));
        assert_eq!(clone_reference_limits("voxcpm2-tts"), Some((3.0, 10.0)));
        assert_eq!(clone_reference_limits("chatterbox"), Some((3.0, 10.0)));
        assert_eq!(clone_reference_limits("pocket-tts"), Some((3.0, 10.0)));
        assert_eq!(clone_reference_limits("tada"), Some((3.0, 10.0)));
        assert_eq!(clone_reference_limits("kokoro"), None);
        assert_eq!(clone_reference_limits("qwen3-tts"), Some((3.0, 10.0)));
        assert_eq!(clone_reference_limits("qwen3-tts-1.7b-base"), Some((3.0, 10.0)));
        assert_eq!(clone_reference_limits("unknown"), None);
    }

    #[test]
    fn all_clone_backends_share_10s_max() {
        let clone = [
            "cosyvoice3-tts", "cosyvoice3-tts-rl", "f5-tts", "zonos", "voxcpm2-tts", "dots-tts",
            "irodori-tts", "indextts", "omnivoice", "moss-tts", "moss-tts-local",
            "confucius4-tts", "chatterbox", "chatbox", "pocket-tts", "vibevoice-1.5b", "tada",
            "tada-1b", "tada-3b-ml", "qwen3-tts", "qwen3-tts-1.7b-base", "qwen3-tts-1.7b-voicedesign",
        ];
        for b in clone {
            assert_eq!(
                clone_reference_limits(b),
                Some((3.0, crate::voices::MAX_REF_SEC)),
                "бэкенд {b} должен использовать канон voices::MAX_REF_SEC"
            );
        }
    }

    #[test]
    fn cache_meta_invalidates_on_limits_change() {
        let models = std::env::temp_dir().join(format!("clone_meta_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&models);
        let root = models.join("voices");
        let _ = std::fs::create_dir_all(&root);
        let src = root.join("voice.wav");
        fs_write_wav(&src, &vec![0.5f32; 8000], 8000);
        let cache_dir = root.join(".clone_cache");
        let _ = std::fs::create_dir_all(&cache_dir);
        let cache_id = crate::tts::ascii_voice_name("тест");
        let trimmed = cache_dir.join(format!("{cache_id}.r2.wav"));
        let meta = trimmed.with_extension("meta");
        fs_write_wav(&trimmed, &[0.0f32; 100], 100);
        write_meta(&meta, 8, file_mtime_ns(&src));
        let cr = prepare_clone_reference(&models.to_string_lossy(), &src.to_string_lossy(), "тест", "qwen3-tts")
            .expect("prepare ok");
        assert_eq!(
            read_meta(&meta).map(|m| m.0),
            Some(crate::voices::MAX_REF_SEC as u32),
            "старый кэш (лимит 8 с) должен пересоздаться под канон voices::MAX_REF_SEC"
        );
        assert!(cr.voice_path.ends_with(".r2.wav"));
        let _ = std::fs::remove_dir_all(&models);
    }

    fn fs_write_wav(path: &std::path::Path, mono: &[f32], rate: u32) {
        crate::audio::wav::write_wav(&path.to_string_lossy(), mono, rate)
            .expect("write wav");
    }

    #[test]
    fn clone_cache_name_is_ascii() {
        for id in ["Влад_без_текста", "Морган Фримен", "голос-1", "voice"] {
            let cache = format!(
                ".clone_cache/{}.r2.wav",
                crate::tts::ascii_voice_name(id)
            );
            assert!(cache.chars().all(|c| c.is_ascii()), "не-ASCII в кэше: {cache}");
            assert!(cache.ends_with(".r2.wav"));
        }
    }
}
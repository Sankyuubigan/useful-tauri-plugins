//! Тесты голоса RefMod-карточек (Этап 3, tasks/261104_0230-db2cb59c): транскод
//! symphonia, set/change/desc/copy, деградация «meta объявляет голос, файла
//! нет», 🎤 в каталоге. Герметично: временные корни, синтетический WAV;
//! реальный `<KingOrchData>/refmods` не трогается (все проверки через `_in`).

use super::*;
// VOICE_WAV_FILE приходит через `use super::*` (импортирован в refmod.rs).

fn voice_root(tag: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "king_orch_voice_{}_{}_{}",
        tag,
        std::process::id(),
        stamp
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png_source(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let path = dir.join(name);
    image::DynamicImage::new_rgb8(w, h)
        .save_with_format(&path, image::ImageFormat::Png)
        .unwrap();
    path
}

/// Синтетический голос-синусоида → канонический WAV-контейнер плагина.
/// (Косвенно проверяет и `VoiceWav::to_wav_bytes` — symphonia его декодирует.)
fn sine_wav_bytes(rate: u32, channels: u16, secs: f32) -> Vec<u8> {
    let frames = (rate as f32 * secs) as usize;
    let total = frames * channels as usize;
    let samples = (0..total)
        .map(|i| {
            let t = (i as f64 / rate as f64) * 2.0 * std::f64::consts::PI;
            (t.sin() * 8000.0) as i16
        })
        .collect();
    VoiceWav {
        samples,
        sample_rate: rate,
        channels,
    }
    .to_wav_bytes()
}

// ── Транскод (normalize_voice / to_wav_bytes) ──

#[test]
fn normalize_voice_roundtrips_wav() {
    let voice = normalize_voice(&sine_wav_bytes(44_100, 2, 0.05)).expect("wav декодируется");
    assert_eq!(voice.sample_rate, 44_100);
    assert_eq!(voice.channels, 2);
    assert!(voice.duration_secs() > 0.0);
    // Roundtrip контейнера: перечитанный voice.wav даёт те же параметры.
    let again = normalize_voice(&voice.to_wav_bytes()).expect("канонический WAV читается");
    assert_eq!(again.sample_rate, voice.sample_rate);
    assert_eq!(again.channels, voice.channels);
    assert_eq!(again.samples, voice.samples);

    // Честные отказы ДО всякой записи на диск.
    assert!(normalize_voice(&[]).is_err(), "пустой ввод");
    let err = normalize_voice(b"not audio at all").unwrap_err();
    assert!(
        matches!(err, RefModError::Usage(ref m) if m.contains("не распознано")),
        "{:?}",
        err
    );
}

#[test]
fn wav_container_header_is_riff_pcm16() {
    let wav = VoiceWav {
        samples: vec![100, -100, 200, -200],
        sample_rate: 22_050,
        channels: 1,
    }
    .to_wav_bytes();
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(wav.len(), 44 + 4 * 2);
    // fmt: PCM=1, channels, rate.
    let fmt_tag = u16::from_le_bytes([wav[20], wav[21]]);
    assert_eq!(fmt_tag, 1, "PCM");
    let ch = u16::from_le_bytes([wav[22], wav[23]]);
    assert_eq!(ch, 1);
    let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
    assert_eq!(rate, 22_050);
}

// ── Set / change (IN-PLACE) ──

#[test]
fn voice_set_and_change_in_place() {
    let root = voice_root("set_change");
    let src = png_source(&root, "s.png", 64, 64);
    save_refmod_card_in(&root, "test_char", "Тест", "", &[src]).unwrap();
    let clean = load_refmod_card_in(&root, "test_char").unwrap();
    assert!(clean.voice.is_none() && clean.voice_desc.is_none(), "save-фн голос не ставит");

    let card = set_card_voice_in(&root, "test_char", &sine_wav_bytes(44_100, 2, 0.05),
        Some("низкий бас".to_string()))
    .expect("голос задан");
    assert!(card.voice.as_ref().unwrap().ends_with(VOICE_WAV_FILE));
    assert_eq!(card.voice_desc.as_deref(), Some("низкий бас"));
    // Файл на диске — валидный WAV (перечитывается транскодом).
    let on_disk = std::fs::read(card.voice.as_ref().unwrap()).unwrap();
    let reread = normalize_voice(&on_disk).expect("voice.wav валиден");
    assert_eq!((reread.sample_rate, reread.channels), (44_100, 2));

    // Смена голоса — та же карточка (slug не растёт), desc сброшен (None),
    // новый файл записан поверх.
    let changed = set_card_voice_in(&root, "test_char", &sine_wav_bytes(22_050, 1, 0.05), None)
        .expect("голос сменён");
    assert_eq!(changed.slug, "test_char");
    assert!(changed.voice.is_some());
    assert_eq!(changed.voice_desc, None, "старое описание сменившегося тембра не живёт");
    let on_disk = std::fs::read(changed.voice.as_ref().unwrap()).unwrap();
    let reread = normalize_voice(&on_disk).unwrap();
    assert_eq!((reread.sample_rate, reread.channels), (22_050, 1), "файл перезаписан");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn save_overwrite_inherits_voice() {
    let root = voice_root("overwrite_voice");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let s1 = png_source(&src_dir, "a.png", 64, 64);
    let s2 = png_source(&src_dir, "b.png", 64, 64);
    save_refmod_card_in(&root, "test_char", "Первый", "", &[s1]).unwrap();
    set_card_voice_in(&root, "test_char", &sine_wav_bytes(44_100, 1, 0.02),
        Some("тенор".to_string()))
    .unwrap();

    // Перезапись кадров того же slug (edit с ops + overwrite): тембр не
    // зависит от кадров — голос НАСЛЕДУЕТСЯ, а не теряется.
    let card = save_refmod_card_in(&root, "test_char", "Второй", "", &[s2])
        .expect("перезапись")
        .0;
    assert!(card.voice.is_some(), "перезапись наследует голос");
    assert_eq!(card.voice_desc.as_deref(), Some("тенор"));
    let loaded = load_refmod_card_in(&root, "test_char").unwrap();
    let on_disk = std::fs::read(loaded.voice.expect("голос жив после перезаписи")).unwrap();
    assert!(normalize_voice(&on_disk).is_ok(), "унаследованный voice.wav валиден");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn voice_set_requires_existing_card() {    let root = voice_root("voice_missing");
    let err = set_card_voice_in(&root, "nobody", &sine_wav_bytes(44_100, 1, 0.02), None).unwrap_err();
    assert!(matches!(err, RefModError::NotFound(ref m) if m.contains("nobody")), "{:?}", err);
    // Slug-гейт раньше диска.
    let err = set_card_voice_in(&root, "../evil", &sine_wav_bytes(44_100, 1, 0.02), None).unwrap_err();
    assert!(matches!(err, RefModError::Usage(_)), "{:?}", err);
    // Нет голоса и нет карточки → NotFound (не Usage декода).
    let wav = sine_wav_bytes(44_100, 1, 0.02);
    let err = write_card_voice_in(&root, "nobody", &VoiceWav { samples: vec![0; 4410], sample_rate: 44_100, channels: 1 }, None).unwrap_err();
    assert!(matches!(err, RefModError::NotFound(_)), "{:?}", err);
    let _ = wav; // keep alive for readability of assertions above
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn voice_desc_edit_requires_existing_voice() {
    let root = voice_root("desc_edit");
    let src = png_source(&root, "s.png", 64, 64);
    save_refmod_card_in(&root, "test_char", "Тест", "", &[src]).unwrap();

    // Голоса нет — описывать нечего (честный Usage, не тихий no-op).
    let err = update_voice_desc_in(&root, "test_char", "хриплый".to_string()).unwrap_err();
    assert!(
        matches!(err, RefModError::Usage(ref m) if m.contains("голоса нет")),
        "{:?}",
        err
    );

    set_card_voice_in(&root, "test_char", &sine_wav_bytes(16_000, 1, 0.02),
        Some("звонкий".to_string()))
    .unwrap();
    let card = update_voice_desc_in(&root, "test_char", "хриплый".to_string()).expect("desc правится");
    assert_eq!(card.voice_desc.as_deref(), Some("хриплый"));
    assert!(card.voice.is_some(), "правка desc не трогает голос-файл");

    // Несуществующая карточка → NotFound.
    let err = update_voice_desc_in(&root, "nobody", "x".to_string()).unwrap_err();
    assert!(matches!(err, RefModError::NotFound(_)), "{:?}", err);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn copy_voice_inherits_and_skips_cleanly() {
    let root = voice_root("copy");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let s1 = png_source(&src_dir, "a.png", 64, 64);
    let s2 = png_source(&src_dir, "b.png", 64, 64);
    save_refmod_card_in(&root, "v1_char", "Версия 1", "", &[s1.clone()]).unwrap();
    set_card_voice_in(&root, "v1_char", &sine_wav_bytes(32_000, 2, 0.02),
        Some("мягкий тембр".to_string()))
    .unwrap();
    // «v2» из ops-правки: новая карточка (slug другой) — save-фн голос не ставит.
    save_refmod_card_in(&root, "v1_char_v2", "Версия 2", "", &[s2]).unwrap();

    copy_card_voice_in(&root, "v1_char", "v1_char_v2").expect("голос наследуется");
    let dst = load_refmod_card_in(&root, "v1_char_v2").unwrap();
    assert!(dst.voice.is_some());
    assert_eq!(dst.voice_desc.as_deref(), Some("мягкий тембр"));
    // Файл идентичен исходному (тот же нормализованный WAV).
    let a = std::fs::read(load_refmod_card_in(&root, "v1_char").unwrap().voice.unwrap()).unwrap();
    let b = std::fs::read(dst.voice.unwrap()).unwrap();
    assert_eq!(a, b, "наследование — байт-в-байт тот же voice.wav");

    // src без голоса: Ok без действий, dst не меняется.
    save_refmod_card_in(&root, "mute_char", "Без голоса", "", &[s1]).unwrap();
    copy_card_voice_in(&root, "mute_char", "v1_char_v2").unwrap();
    let dst = load_refmod_card_in(&root, "v1_char_v2").unwrap();
    assert!(dst.voice.is_some(), "нет голоса у источника — копирование не стирает");

    // Несуществующий src → NotFound.
    let err = copy_card_voice_in(&root, "nobody", "v1_char_v2").unwrap_err();
    assert!(matches!(err, RefModError::NotFound(_)), "{:?}", err);
    let _ = std::fs::remove_dir_all(root);
}

// ── Устойчивость: meta объявляет голос, файла нет (ручное удаление) ──

#[test]
fn declared_voice_without_file_degrades_not_breaks() {
    let root = voice_root("degrade");
    let src = png_source(&root, "s.png", 64, 64);
    let card = save_refmod_card_in(&root, "test_char", "Тест", "", &[src]).unwrap();
    set_card_voice_in(&root, "test_char", &sine_wav_bytes(44_100, 1, 0.02),
        Some("пропавший".to_string()))
    .unwrap();
    // Ручное удаление voice.wav: meta объявляет голос — файла нет.
    std::fs::remove_file(card_dir_of(&root, "test_char").join(VOICE_WAV_FILE)).unwrap();

    let loaded = load_refmod_card_in(&root, "test_char").expect("карточка НЕ бричится");
    assert!(loaded.voice.is_none(), "файла нет — голос None");
    assert!(loaded.voice_desc.is_some(), "desc в meta остаётся как записан");

    // Каталог НЕ обещает голос: prompt молчит, пока voice фактически None.
    let prompt = refmods_prompt_in(&root);
    assert!(!prompt.contains("🎤"), "{}", prompt);
    let _ = std::fs::remove_dir_all(root);
}

fn card_dir_of(root: &Path, slug: &str) -> PathBuf {
    root.join(slug)
}

// ── Каталог 🎤 ──

#[test]
fn prompt_marks_voice_cards() {
    let root = voice_root("prompt");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let s1 = png_source(&src_dir, "a.png", 64, 64);
    let s2 = png_source(&src_dir, "b.png", 64, 64);
    save_refmod_card_in(&root, "voiced_char", "С голосом", "описание", &[s1]).unwrap();
    set_card_voice_in(&root, "voiced_char", &sine_wav_bytes(44_100, 1, 0.02),
        Some("низкий бас".to_string()))
    .unwrap();
    save_refmod_card_in(&root, "mute_char", "Без голоса", "", &[s2]).unwrap();

    let prompt = refmods_prompt_in(&root);
    assert!(prompt.contains("🎤 голос: низкий бас"), "{}", prompt);
    let voiced_line = prompt.lines().find(|l| l.contains("voiced_char")).unwrap();
    assert!(voiced_line.contains("🎤 голос: низкий бас"), "{}", voiced_line);
    let mute_line = prompt.lines().find(|l| l.contains("mute_char")).unwrap();
    assert!(!mute_line.contains("🎤"), "{}", mute_line);

    // Голос без desc — «задан» вместо описания.
    set_card_voice_in(&root, "mute_char", &sine_wav_bytes(44_100, 1, 0.02), None).unwrap();
    let prompt = refmods_prompt_in(&root);
    let line = prompt.lines().find(|l| l.contains("mute_char")).unwrap();
    assert!(line.contains("🎤 голос задан"), "{}", line);
    let _ = std::fs::remove_dir_all(root);
}

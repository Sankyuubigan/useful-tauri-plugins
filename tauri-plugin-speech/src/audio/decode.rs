use anyhow::{Context, Result};
use symphonia::core::audio::conv::IntoSample;
use symphonia::core::audio::sample::Sample;
use symphonia::core::audio::Audio;
use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::default::{get_codecs, get_probe};

/// Декодирует файл в Vec<f32> (планы по каналам, перемежённые: [L,R,L,R,...]).
/// Возвращает (кол-во каналов, sample_rate, samples).
pub fn decode_file(path: &str) -> Result<(usize, u32, Vec<f32>)> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("не удалось открыть файл: {path}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        hint.with_extension(ext);
    }

    match decode_stream(mss, hint) {
        Ok(decoded) => Ok(decoded),
        Err(e) => {
            // symphonia 0.6 не поддерживает opus — пробуем отдельный модуль.
            let is_ogg = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("ogg"))
                .unwrap_or(false);
            if is_ogg {
                return crate::audio::opus_decode::decode_opus(path);
            }
            Err(e)
        }
    }
}

/// Декодирует аудио из памяти в Vec<f32> (перемежённые планы).
/// Формат определяется пробой содержимого (для MP3 без расширения достаточно).
pub fn decode_bytes(bytes: &[u8]) -> Result<(usize, u32, Vec<f32>)> {
    let mss = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    decode_stream(mss, Hint::new())
}

/// Общий декод потока: probe -> аудиодорожка -> декодер -> сэмплы.
fn decode_stream(mss: MediaSourceStream, hint: Hint) -> Result<(usize, u32, Vec<f32>)> {
    let mut format = get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .context("не удалось определить формат аудио (поддерживается ogg/vorbis, wav, flac, mp3)")?;

    let track = format
        .tracks()
        .iter()
        .find(|t| matches!(t.codec_params, Some(symphonia::core::codecs::CodecParameters::Audio(_))))
        .context("в файле не найдено аудиодорожек")?;

    let params = match track.codec_params {
        Some(symphonia::core::codecs::CodecParameters::Audio(ref p)) => p,
        _ => anyhow::bail!("дорожка не содержит аудио-параметров"),
    };

    let sample_rate = params.sample_rate.unwrap_or(16000);
    let channels = params.channels.clone().map(|c| c.count()).unwrap_or(1);

    let audio_params = params.clone();

    let mut decoder = get_codecs()
        .make_audio_decoder(&audio_params, &Default::default())
        .context("не удалось создать декодер для этого кодека")?;

    let mut samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(symphonia::core::errors::Error::ResetRequired) => continue,
            Err(_) => break,
        };

        let decoded = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(_) => continue,
        };

        // читаем перемежённые f32-сэмплы из планарного буфера
        use symphonia::core::audio::{AudioBuffer, GenericAudioBufferRef};
        let spec = decoded.spec();
        let n_ch = spec.channels().count();
        fn read_planes<S: Sample + IntoSample<f32>>(
            buf: &AudioBuffer<S>,
            n_ch: usize,
            samples: &mut Vec<f32>,
        ) {
            let mut chans: Vec<&[S]> = Vec::with_capacity(n_ch);
            for c in 0..n_ch {
                chans.push(buf.plane(c).unwrap_or(&[]));
            }
            let frames = chans[0].len();
            for i in 0..frames {
                for c in 0..n_ch {
                    samples.push(chans[c][i].into_sample());
                }
            }
        }
        match decoded {
            GenericAudioBufferRef::U8(b) => read_planes(&b, n_ch, &mut samples),
            GenericAudioBufferRef::U16(b) => read_planes(&b, n_ch, &mut samples),
            GenericAudioBufferRef::U24(b) => read_planes(&b, n_ch, &mut samples),
            GenericAudioBufferRef::U32(b) => read_planes(&b, n_ch, &mut samples),
            GenericAudioBufferRef::S8(b) => read_planes(&b, n_ch, &mut samples),
            GenericAudioBufferRef::S16(b) => read_planes(&b, n_ch, &mut samples),
            GenericAudioBufferRef::S24(b) => read_planes(&b, n_ch, &mut samples),
            GenericAudioBufferRef::S32(b) => read_planes(&b, n_ch, &mut samples),
            GenericAudioBufferRef::F32(b) => read_planes(&b, n_ch, &mut samples),
            GenericAudioBufferRef::F64(b) => read_planes(&b, n_ch, &mut samples),
        }
    }

    Ok((channels, sample_rate, samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Каталог для временных файлов тестов: `target/tmp` рядом с бинарником
    /// теста (`target/<profile>/deps/test.exe` -> `target/tmp`).
    /// Не системный temp (core rules §1.2) и не корень репозитория.
    fn test_tmp_dir() -> std::path::PathBuf {
        let mut p = std::env::current_exe().expect("current_exe");
        p.pop(); // deps
        p.pop(); // <profile>
        p.push("tmp");
        std::fs::create_dir_all(&p).expect("создать target/tmp");
        p
    }

    /// Самодостаточная проверка ветки «файл на диске» (File + подсказка по
    /// расширению). Хардкодить путь пользователя нельзя (§1.4) — файл
    /// синтезируется тут же.
    #[test]
    fn decode_file_reads_wav_from_disk() {
        let rate = 24_000u32;
        let n = 4_800usize; // 0.2 с
        let path = test_tmp_dir().join("decode_file_test.wav");

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut w = hound::WavWriter::create(&path, spec).expect("writer");
            for i in 0..n {
                let s = 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin();
                w.write_sample((s * i16::MAX as f32) as i16).expect("sample");
            }
            w.finalize().expect("finalize");
        }

        let (channels, decoded_rate, samples) = decode_file(&path.to_string_lossy())
            .expect("wav с диска должен декодироваться");
        assert_eq!(channels, 1);
        assert_eq!(decoded_rate, rate);
        assert!(samples.len() >= n, "сэмплов меньше, чем записано: {}", samples.len());
        let energy: f32 = samples.iter().map(|s| s * s).sum();
        assert!(energy > 0.0, "в аудио должна быть энергия");

        let _ = std::fs::remove_file(&path);
    }

    /// `decode_bytes` (используется для проверки свежесозданного MP3) отдаёт
    /// тот же результат, что и файловый путь для того же содержимого.
    #[test]
    fn decode_bytes_matches_decode_file_for_same_wav() {
        let rate = 24_000u32;
        let n = 2_400usize;
        let mut bytes = Vec::new();
        {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut bytes), spec)
                .expect("writer");
            for i in 0..n {
                let s = 0.25 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / rate as f32).sin();
                w.write_sample((s * i16::MAX as f32) as i16).expect("sample");
            }
            w.finalize().expect("finalize");
        }

        let (c1, r1, s1) = decode_bytes(&bytes).expect("decode_bytes");
        let path = test_tmp_dir().join("decode_bytes_test.wav");
        std::fs::write(&path, &bytes).expect("write");
        let (c2, r2, s2) = decode_file(&path.to_string_lossy()).expect("decode_file");
        let _ = std::fs::remove_file(&path);

        assert_eq!((c1, r1, s1.len()), (c2, r2, s2.len()));
    }

    /// Декод реального ogg с внешнего диска. Путь берётся из переменной
    /// окружения `SPEECHLAB_TEST_OGG`; тест `#[ignore]`, потому что требует
    /// данных пользователя (core rules §2.9) и не должен молча «проходить»
    /// в его отсутствие (core rules §2.2).
    ///
    /// Запуск: `cargo test -p tauri-plugin-speech -- --ignored wav_to_mp3`
    /// c заданной `SPEECHLAB_TEST_OGG`, либо из `run_asr_test.bat`.
    #[test]
    #[ignore = "нужен реальный ogg: задай SPEECHLAB_TEST_OGG"]
    fn ogg_decode() {
        let path = match std::env::var("SPEECHLAB_TEST_OGG") {
            Ok(p) => p,
            Err(_) => panic!("SPEECHLAB_TEST_OGG не задан — этот тест нельзя молча пропускать"),
        };
        if !std::path::Path::new(&path).exists() {
            panic!("тестовый ogg не найден по пути из SPEECHLAB_TEST_OGG: {path}");
        }
        let (channels, rate, samples) = decode_file(&path).expect("декод должен успешно пройти");
        println!("decoded: channels={channels}, rate={rate}, samples={}", samples.len());
        assert!(channels >= 1, "каналов должно быть >= 1");
        assert!(rate > 0, "sample_rate должен быть > 0");
        assert!(!samples.is_empty(), "сэмплы не должны быть пустыми");
        let energy: f32 = samples.iter().map(|s| s * s).sum();
        assert!(energy > 0.0, "в аудио должна быть энергия");
        println!("✅ ogg декодирован успешно, энергия={energy:.4}");
    }
}
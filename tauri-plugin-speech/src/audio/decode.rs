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

    let mut decoder = match get_codecs().make_audio_decoder(&audio_params, &Default::default()) {
        Ok(d) => d,
        Err(_) => {
            // symphonia 0.6 не поддерживает opus — пробуем отдельный модуль.
            let is_ogg = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("ogg"))
                .unwrap_or(false);
            if is_ogg {
                return crate::audio::opus_decode::decode_opus(path);
            }
            anyhow::bail!("не удалось создать декодер для этого кодека");
        }
    };

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

    #[test]
    fn ogg_decode() {
        let path = "E:\\Downloads\\audio_2026-07-18_23-59-01.ogg";
        if !std::path::Path::new(path).exists() {
            eprintln!("⚠️ тестовый файл не найден: {path} — пропускаем");
            return;
        }
        let (channels, rate, samples) = decode_file(path).expect("декод должен успешно пройти");
        println!("decoded: channels={channels}, rate={rate}, samples={}", samples.len());
        assert!(channels >= 1, "каналов должно быть >= 1");
        assert!(rate > 0, "sample_rate должен быть > 0");
        assert!(!samples.is_empty(), "сэмплы не должны быть пустыми");
        let energy: f32 = samples.iter().map(|s| s * s).sum();
        assert!(energy > 0.0, "в аудио должна быть энергия");
        println!("✅ ogg декодирован успешно, энергия={energy:.4}");
    }
}
//! Локальное сжатие озвучки WAV -> MP3.
//!
//! Чистый Rust-кодек `rusty_mp3`: ноль зависимостей, без build.rs, поэтому
//! хост не компилирует нативный C++ ради одной кнопки «Сохранить MP3».
//!
//! Контекст: движок CrispASR отдаёт озвучку как WAV (RIFF, 16-bit PCM int16,
//! 24 кГц моно) — см. `global_ai_docs/desktop_rust_tauri/crispasr_engine.md`.
//! Синтез при сжатии НЕ повторяется: кодируются те же байты, что уже прислал
//! `tts_speak`.

use anyhow::{bail, Context, Result};
use hound::{SampleFormat, WavSpec};

/// Битрейт по умолчанию. Валиден для любой частоты движка: MPEG-2 (16/22.05/24
/// кГц) потолок — 160 кбит/с, MPEG-1 — 320; крейт сам приводит значение к
/// ближайшей допустимой табличной ступени для своей версии MPEG.
pub const DEFAULT_BITRATE_KBPS: u32 = 128;

/// Кодирует WAV-байты (16-bit PCM int16) в MP3.
pub fn encode_wav_to_mp3(wav: &[u8], bitrate_kbps: u32) -> Result<Vec<u8>> {
    let (pcm, channels, sample_rate) = decode_wav_i16(wav)?;
    encode_pcm_to_mp3(&pcm, channels, sample_rate, bitrate_kbps)
}

/// Кодирует WAV-байты в MP3 с битрейтом по умолчанию.
pub fn encode_wav_to_mp3_default(wav: &[u8]) -> Result<Vec<u8>> {
    encode_wav_to_mp3(wav, DEFAULT_BITRATE_KBPS)
}

/// Разбирает WAV -> (интерливированные f32 в [-1,1], каналы, частота дискретизации).
///
/// Только 16-bit PCM int16 — формат, который отдаёт движок. На любой другой
/// (float, 24/32-bit) возвращаем явную ошибку, а не молча перекодируем
/// (core rules §2.2: правда вместо тихой подмены).
fn decode_wav_i16(wav: &[u8]) -> Result<(Vec<f32>, u16, u32)> {
    let mut reader = hound::WavReader::new(std::io::Cursor::new(wav))
        .context("не удалось разобрать WAV озвучки (ожидается RIFF/PCM от движка)")?;
    let spec: WavSpec = reader.spec();

    match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 16) => {}
        (fmt, bits) => bail!(
            "WAV озвучки имеет неподдерживаемый формат: {fmt:?}, {bits} бит. \
             Движок отдаёт 16-bit PCM int16 — сначала проверь response_format."
        ),
    }

    let channels = spec.channels.max(1);
    let sample_rate = spec.sample_rate;
    let pcm: Vec<f32> = reader
        .samples::<i16>()
        .collect::<std::result::Result<Vec<i16>, _>>()
        .context("не удалось прочитать PCM-сэмплы WAV")?
        .into_iter()
        .map(|s| s as f32 / 32768.0)
        .collect();

    if pcm.is_empty() {
        bail!("WAV озвучки пустой — нечего кодировать");
    }

    Ok((pcm, channels, sample_rate))
}

/// Кодирует интерливированные f32 в MP3.
fn encode_pcm_to_mp3(
    interleaved: &[f32],
    channels: u16,
    sample_rate: u32,
    bitrate_kbps: u32,
) -> Result<Vec<u8>> {
    let mut enc = rusty_mp3::Mp3Encoder::new(rusty_mp3::Mp3EncoderConfig {
        bitrate_kbps,
        vbr_quality: None, // CBR — предсказуемый размер и битрейт
    });

    enc.push_pcm_f32(interleaved, channels, sample_rate)
        .context("MP3-энкодер отверг PCM (проверь частоту дискретизации и число каналов)")?;

    // finish() дописывает хвост до целого фрейма и собирает резервуар + заголовок
    // Xing/Info. До него next_packet() отдаёт Again, поэтому порядок важен.
    enc.finish();

    let mut mp3 = Vec::new();
    loop {
        match enc.next_packet() {
            Ok(pkt) => mp3.extend_from_slice(&pkt),
            Err(rusty_mp3::Error::Eof) => break,
            Err(rusty_mp3::Error::Again) => {
                bail!("MP3-энкодер не выдал ни одного фрейма — озвучка слишком короткая")
            }
            Err(e) => bail!("ошибка MP3-энкодера: {e}"),
        }
    }

    if mp3.is_empty() {
        bail!("MP3-энкодер вернул пустой поток");
    }

    Ok(mp3)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Синтетическая озвучка (тон 440 Гц, 24 кГц моно — формат движка)
    /// -> WAV -> MP3 -> обратное декодирование symphonia. Проверяем, что поток
    /// валиден (symphonia его читает) и длительность сохранилась.
    #[test]
    fn wav_to_mp3_roundtrips_through_symphonia() {
        let rate = 24_000u32;
        let n = rate as usize / 2; // ~0.5 с
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin()
            })
            .collect();

        let mut wav_bytes = Vec::new();
        {
            let spec = WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            };
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut wav_bytes), spec)
                .expect("writer");
            for s in &samples {
                w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                    .expect("sample");
            }
            w.finalize().expect("finalize");
        }

        let mp3 = encode_wav_to_mp3_default(&wav_bytes).expect("mp3 закодировался");
        assert!(mp3.len() > 1024, "mp3 подозрительно маленький: {} байт", mp3.len());

        // Обратный декод: валидный MP3 читается symphonia без ошибок.
        let (channels, decoded_rate, decoded) =
            crate::audio::decode::decode_bytes(&mp3).expect("mp3 декодировался обратно");
        assert_eq!(channels, 1, "каналов должно быть 1");
        assert_eq!(decoded_rate, rate, "частота дискретизации не изменилась");

        let dur = decoded.len() as f64 / decoded_rate as f64;
        assert!(
            (0.45..0.60).contains(&dur),
            "длительность после mp3 вне допуска: {dur:.3} с (ждали ~0.5)"
        );
    }

    /// Не-WAV должен честно падать с внятной причиной, а не писать мусор в файл.
    #[test]
    fn rejects_non_wav_input() {
        let err = encode_wav_to_mp3_default(b"not a wav file at all").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("WAV"),
            "ошибка должна называть формат, получено: {msg}"
        );
    }

    /// 24-битный WAV отвергаем явно, а не перекодируем молча.
    #[test]
    fn rejects_unsupported_bit_depth() {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 24_000,
            bits_per_sample: 24,
            sample_format: SampleFormat::Int,
        };
        let mut wav_bytes = Vec::new();
        {
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut wav_bytes), spec)
                .expect("writer");
            w.write_sample(0i32).expect("sample");
            w.finalize().expect("finalize");
        }
        let err = encode_wav_to_mp3_default(&wav_bytes).unwrap_err();
        assert!(
            format!("{err}").contains("бит"),
            "ошибка должна называть разрядность, получено: {err}"
        );
    }
}
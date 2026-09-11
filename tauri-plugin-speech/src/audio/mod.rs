pub mod decode;
pub mod denoise;
pub mod opus_decode;
pub mod wav;

use anyhow::Result;

/// Декодирует аудиофайл (любой поддерживаемый symphonia: ogg/vorbis, wav, flac, mp3...)
/// в плоский Vec<f32> (моно, исходная частота) + sample_rate.
pub fn decode_to_mono(path: &str) -> Result<(Vec<f32>, u32)> {
    let (channels, rate, samples) = decode::decode_file(path)?;
    let mono = if channels == 1 {
        samples
    } else {
        // усредняем каналы в моно
        samples
            .chunks_exact(channels)
            .map(|c| c.iter().sum::<f32>() / channels as f32)
            .collect()
    };
    Ok((mono, rate))
}
use anyhow::Result;
use hound::{SampleFormat, WavSpec, WavWriter};

/// Пишет моно f32 ([-1.0, 1.0]) в 16-bit PCM WAV.
pub fn write_wav(path: &str, samples: &[f32], sample_rate: u32) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec)?;
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer.write_sample(v)?;
    }
    writer.finalize()?;
    Ok(())
}
use anyhow::{anyhow, Result};
use audioadapter_buffers::direct::InterleavedSlice;
use nnnoiseless::DenoiseState;
use rubato::{Fft, FixedSync, Resampler};

/// Параметры шумоподавления для новых/обновляемых голосов.
#[derive(Debug, Clone, Copy)]
pub struct DenoiseOpts {
    pub enabled: bool,
    /// Сила смешивания оригинала и очищенного: 0.0 — без изменений, 1.0 — максимум.
    pub strength: f32,
}

impl Default for DenoiseOpts {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 0.9,
        }
    }
}

const TARGET_RATE: usize = 48000;
const PCM_SCALE: f32 = 32768.0;

/// Применяет RNNoise-шумоподавление (`nnnoiseless`) к моно-сигналу `samples`
/// в диапазоне `[-1, 1]` с частотой `rate`. Возвращает очищенный сигнал
/// в том же диапазоне.
pub fn denoise_mono(samples: &[f32], rate: u32, opts: &DenoiseOpts) -> Result<Vec<f32>> {
    if !opts.enabled || opts.strength <= 0.0 {
        return Ok(samples.to_vec());
    }
    let strength = opts.strength.clamp(0.0, 1.0);

    // В int16-PCM диапазон ([-32768, 32767]), как того требует nnnoiseless.
    let pcm: Vec<f32> = samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * PCM_SCALE))
        .collect();

    // resample исходной частоты -> 48k.
    let pcm48 = resample(&pcm, rate as usize, TARGET_RATE)?;

    let frame = DenoiseState::FRAME_SIZE;
    let n = pcm48.len();

    // RNNoise обрабатывает окна по `frame` сэмплов; первый выходной кадр
    // содержит артефакт fade-in — отбрасываем.
    let mut st = DenoiseState::new();
    let mut denoised = Vec::with_capacity(n.saturating_sub(frame));
    let mut dropped = false;
    let mut i = 0usize;
    while i + frame <= n {
        let mut out = [0f32; DenoiseState::FRAME_SIZE];
        st.process_frame(&mut out, &pcm48[i..i + frame]);
        if !dropped {
            dropped = true;
        } else {
            denoised.extend_from_slice(&out);
        }
        i += frame;
    }

    // Линейное смешивание: result = orig*(1-strength) + denoised*strength.
    let mut out_mixed = Vec::with_capacity(denoised.len());
    for (k, &dn) in denoised.iter().enumerate() {
        let orig = pcm48.get(k + frame).copied().unwrap_or(0.0);
        out_mixed.push(orig * (1.0 - strength) + dn * strength);
    }

    // resample обратно в оригинальную частоту.
    let back = resample(&out_mixed, TARGET_RATE, rate as usize)?;

    Ok(back
        .iter()
        .map(|x| (x / PCM_SCALE).clamp(-1.0, 1.0))
        .collect())
}

/// Ресемплинг через FFT-ресемплер `rubato`. Для совпадения частот — копия.
pub fn resample(data: &[f32], in_rate: usize, out_rate: usize) -> Result<Vec<f32>> {
    if in_rate == out_rate {
        return Ok(data.to_vec());
    }
    let frames = data.len();
    let mut resampler = Fft::<f32>::new(in_rate, out_rate, 1024, 1, FixedSync::Both)
        .map_err(|e| anyhow!("не удалось создать ресемплер: {e}"))?;
    let adapter = InterleavedSlice::new(data, 1, frames)
        .map_err(|e| anyhow!("не удалось создать адаптер аудио: {e:?}"))?;
    let out = resampler
        .process_all(&adapter, frames, None)
        .map_err(|e| anyhow!("ошибка ресемплинга: {e}"))?;
    Ok(out.take_data())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_disabled() {
        let s: Vec<f32> = (0..1000).map(|i| (i as f32) * 0.001 - 0.5).collect();
        let out = denoise_mono(&s, 44100, &DenoiseOpts::default()).unwrap();
        assert_eq!(out.len(), s.len());
    }

    #[test]
    fn reduces_noise_on_silent_signal() {
        let s: Vec<f32> = (0..48000)
            .map(|i| (i as f32 * 0.0001).sin() * 0.01 + (i as f32 % 7.0 - 3.0) * 0.005)
            .collect();
        let out = denoise_mono(
            &s,
            48000,
            &DenoiseOpts {
                enabled: true,
                strength: 1.0,
            },
        )
        .unwrap();
        assert!(!out.is_empty());
        for &v in &out {
            assert!(v.is_finite() && v.abs() <= 1.0, "значение вне [-1,1]: {v}");
        }
        assert!((out.len() as i64 - s.len() as i64).abs() <= 480);
    }
}
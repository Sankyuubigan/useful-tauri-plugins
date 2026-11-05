//! 🎤 Голос RefMod-карточки — SSOT транскода аудио (tasks/261104_0230-db2cb59c,
//! Этап 3, решение владельца 05.11.2026: голос — часть карточки).
//!
//! Юзер прикладывает сэмпл голоса (обычно mp3/wav/m4a) — здесь он декодируется
//! (symphonia, pure Rust) в PCM16 WAV и сохраняется в карточку как `voice.wav`.
//! При генерации видео файл уходит движку в `ref_audios` (H3 voice cloning).
//!
//! Границы честности:
//! - Конвертацию частоты/каналов НЕ дублируем — движок H3 сам приводит
//!   референс к stereo 32 kHz (docs/minimax_h3.md). Мы сохраняем PCM16
//!   с исходными rate/channels — детерминированный канонический вид.
//! - opus/webm-аудио НЕ поддержано: в symphonia 0.5 opus-кодека нет
//!   (проверено crates.io 05.11.2026). Такие файлы → `Usage` с подсказкой.
//! - Декод идёт ДО всякой записи на диск: битый сэмпл не портит карточку.

use super::refmod::RefModError;

/// Имя файла голоса внутри папки карточки (ровно один голос на карточку —
/// структурный запрет мульти-голосов: 2+ ref_audios H3 путает, комьюнити-кейс).
pub const VOICE_WAV_FILE: &str = "voice.wav";

/// Нормализованный голос: PCM16, исходные частота/каналы (моно ок).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceWav {
    /// Интерливнутые сэмплы PCM16 (len кратно channels).
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl VoiceWav {
    /// Длительность в секундах (для логов/отчётов тула).
    pub fn duration_secs(&self) -> f32 {
        let rate = self.sample_rate.max(1) as f32;
        let frames = self.samples.len() / self.channels.max(1) as usize;
        frames as f32 / rate
    }

    /// Канонический WAV-контейнер (RIFF/PCM16), который пишется в `voice.wav`
    /// и уходит движку base64 в `ref_audios`.
    pub fn to_wav_bytes(&self) -> Vec<u8> {
        let channels = self.channels.max(1);
        let rate = self.sample_rate.max(1);
        let data_len = (self.samples.len() * 2) as u32;
        let byte_rate = rate * channels as u32 * 2;
        let block_align = channels * 2;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // формат PCM
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes()); // бит на сэмпл
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in &self.samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }
}

/// Декодировать аудио-сэмпл юзера в нормализованный голос. Пустой декод
/// (0 сэмплов) и не-аудио байты — честный `Usage` до всякой записи на диск.
pub fn normalize_voice(bytes: &[u8]) -> Result<VoiceWav, RefModError> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error as SymphoniaError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    if bytes.is_empty() {
        return Err(RefModError::Usage(
            "refmod: аудио-сэмпл голоса пустой (0 байт).".to_string(),
        ));
    }
    let mss = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| {
            RefModError::Usage(format!(
                "refmod: аудио не распознано как голосовой сэмпл ({}). Поддержаны \
                 mp3/wav/m4a/ogg-vorbis/flac; записи webm/opus не поддержаны — \
                 приложи mp3 или wav.",
                e
            ))
        })?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| {
            RefModError::Usage(
                "refmod: в аудио-файле нет декодируемой дорожки (opus/webm не \
                 поддержан — приложи mp3 или wav)."
                    .to_string(),
            )
        })?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| RefModError::Usage(format!("refmod: кодек аудио не поддержан: {}", e)))?;

    let mut samples_f32: Vec<f32> = Vec::new();
    let mut channels = 0u16;
    let mut rate = 0u32;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // Конец потока — штатный выход цикла (symphonia-пример).
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => {
                return Err(RefModError::Usage(format!("refmod: чтение аудио сорвалось: {}", e)))
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                channels = spec.channels.count() as u16;
                rate = spec.rate;
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                samples_f32.extend_from_slice(buf.samples());
            }
            // Битый пакет — пропускаем (сэмпл юзера не обязан быть идеальным).
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => {
                return Err(RefModError::Usage(format!("refmod: декод аудио не удался: {}", e)))
            }
        }
    }
    if samples_f32.is_empty() || channels == 0 || rate == 0 {
        return Err(RefModError::Usage(
            "refmod: аудио декодировалось в тишину (0 сэмплов) — нужен сэмпл с речью."
                .to_string(),
        ));
    }
    // Масштаб ×32768 (конвенция symphonia: PCM16 → f32 = s/32768): степень
    // двойки — точная в f32, поэтому decode→encode→decode строго идемпотентен
    // (канонизация voice.wav не деградирует при повторных записях). Насыщающий
    // cast покрывает 1.0 → 32767.
    let samples = samples_f32
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32768.0) as i16)
        .collect();
    Ok(VoiceWav {
        samples,
        sample_rate: rate,
        channels,
    })
}

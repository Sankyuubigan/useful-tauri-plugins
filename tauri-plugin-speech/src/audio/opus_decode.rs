//! Декод ogg/opus через `ogg` (demux) + `opus` (libopus).
//! symphonia 0.6 не поддерживает opus, поэтому opus-дорожки декодируем отдельно.

use anyhow::{Context, Result};
use opus::Channels;
use std::fs::File;
use std::io::BufReader;

/// Декодирует ogg/opus файл в Vec<f32> (планы по каналам, перемежённые).
/// Возвращает (каналы, sample_rate, samples).
pub fn decode_opus(path: &str) -> Result<(usize, u32, Vec<f32>)> {
    let file = File::open(path).with_context(|| format!("не удалось открыть файл: {path}"))?;
    let mut reader = ogg::PacketReader::new(BufReader::new(file));

    // Считываем OpusHead, чтобы узнать каналы и preskip.
    let first = reader
        .read_packet()
        .context("не удалось прочитать opus head")?
        .context("пустой ogg-файл")?;
    let (channels, preskip) = parse_opus_head(&first.data)
        .context("некорректный opus head (это не ogg/opus?)")?;
    let sample_rate = 48000u32; // opus всегда декодируется в 48k

    let ch = match channels {
        1 => Channels::Mono,
        2 => Channels::Stereo,
        _ => anyhow::bail!("неподдержимое число каналов opus: {channels}"),
    };

    let mut decoder = opus::Decoder::new(sample_rate, ch)
        .map_err(|e| anyhow::anyhow!("не удалось создать opus-декодер: {e}"))?;

    let mut samples: Vec<f32> = Vec::new();
    // первый считанный пакет — head; второй (tags) тоже пропускаем как данные
    let mut packets_seen = 0u32;

    loop {
        let packet = match reader.read_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e) => {
                eprintln!("[warn] ogg read error: {e}");
                break;
            }
        };
        packets_seen += 1;
        if packets_seen <= 1 {
            continue;
        }

        // Максимально возможный пакет Opus — 120 мс, что при 48kHz составляет 5760 семплов на канал.
        let mut pcm = vec![0.0f32; 5760 * channels as usize];
        let n = decoder
            .decode_float(&packet.data, &mut pcm, false)
            .map_err(|e| anyhow::anyhow!("ошибка декода opus: {e}"))?;

        let total = n * channels as usize;
        for i in 0..total {
            samples.push(pcm[i]);
        }
    }

    // Opus-спецификация: в начале декодированного потока preskip семплов тишины
    // (из-за lookahead кодека) — их обязательно отбрасываем.
    let drop = (preskip as usize).min(samples.len());
    let samples = samples[drop..].to_vec();

    Ok((channels as usize, sample_rate, samples))
}

/// Парсит OpusHead (первый пакет ogg/opus страницы 0).
/// Возвращает (channels, preskip).
fn parse_opus_head(data: &[u8]) -> Option<(u8, u16)> {
    // "OpusHead" + version(1) + channels(1) + preskip(2 LE) + ...
    if data.len() < 8 || &data[0..8] != b"OpusHead" {
        return None;
    }
    let channels = data[9];
    let preskip = u16::from_le_bytes([data[10], data[11]]);
    Some((channels, preskip))
}
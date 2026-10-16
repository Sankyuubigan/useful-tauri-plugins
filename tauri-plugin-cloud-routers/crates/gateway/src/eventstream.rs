//! Декодер бинарного AWS EventStream (ответ Kiro-инференса).
//!
//! Формат фрейма (подтверждено `open-sse/utils/awsEventStream.js` 9Router):
//! ```text
//! [total_len: u32 BE][headers_len: u32 BE][prelude CRC: u32 BE]
//! [headers: headers_len байт][payload]
//! [message CRC: u32 BE]
//! ```
//! Заголовки: последовательности `{name_len: u8, name, type: u8, value}`.
//! Типы значений: 0=true, 1=false, 2=byte, 3=short, 4=int, 5=long, 6=bytes,
//! 7=string, 8=timestamp, 9=uuid. Нам нужны только строковые (`:message-type`,
//! `:event-type`, `:content-type`), остальные пропускаются по длине.
//!
//! Строгость — архитектурная: CRC/длины проверяются всегда. Битый фрейм —
//! ошибка с байт-смещением, а не молчаливая склейка мусора в поток клиенту.
//! Лимиты — как у 9Router (`maxMessageBytes` 24 МБ, `maxHeadersBytes` 128 КБ).

/// Лимит размера одного фрейма (защита от раздувания памяти).
pub const MAX_MESSAGE_BYTES: usize = 24 * 1024 * 1024;
/// Лимит секции заголовков фрейма.
pub const MAX_HEADERS_BYTES: usize = 128 * 1024;

/// Разобранный фрейм: тип сообщения, тип события, JSON-payload.
#[derive(Debug, Clone, PartialEq)]
pub struct EventFrame {
    /// `:message-type` (`event` | `error` | `exception`).
    pub message_type: String,
    /// `:event-type` (`assistantResponseEvent`, `toolUseEvent`, …).
    pub event_type: String,
    /// Тело события (JSON).
    pub payload: serde_json::Value,
}

/// Ошибка декодера с позицией: где в потоке сломался разбор.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeError {
    /// Смещение в байтах от начала буфера.
    pub offset: usize,
    /// Что именно не сошлось.
    pub reason: String,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EventStream: {} (смещение {})", self.reason, self.offset)
    }
}

impl std::error::Error for DecodeError {}

/// Потоковый декодер: кормим чанками байт, забираем готовые фреймы.
///
/// Неполный фрейм в конце буфера — не ошибка: ждём следующий чанк.
pub struct EventStreamDecoder {
    buf: Vec<u8>,
    offset: usize,
}

impl EventStreamDecoder {
    pub fn new() -> Self {
        Self { buf: Vec::new(), offset: 0 }
    }

    /// Скормить чанк байт, вернуть все полные фреймы из буфера.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<EventFrame>, DecodeError> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            match try_parse_frame(&self.buf[self.offset..])? {
                Some((frame, consumed)) => {
                    self.offset += consumed;
                    out.push(frame);
                }
                None => break,
            }
        }
        // Чистим съеденное, чтобы буфер не рос бесконечно на длинном стриме.
        if self.offset > 0 {
            self.buf.drain(..self.offset);
            self.offset = 0;
        }
        Ok(out)
    }
}

impl Default for EventStreamDecoder {
    fn default() -> Self {
        Self::new()
    }
}

fn try_parse_frame(data: &[u8]) -> Result<Option<(EventFrame, usize)>, DecodeError> {
    if data.len() < 12 {
        return Ok(None);
    }
    let total = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let headers_len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if total < 12 + 4 || total > MAX_MESSAGE_BYTES {
        return Err(DecodeError {
            offset: 0,
            reason: format!("некорректная длина фрейма: {total}"),
        });
    }
    if headers_len > MAX_HEADERS_BYTES || headers_len + 12 + 4 > total {
        return Err(DecodeError {
            offset: 0,
            reason: format!("некорректная длина заголовков: {headers_len}"),
        });
    }
    if data.len() < total {
        return Ok(None); // ждём остаток фрейма
    }
    let frame = &data[..total];
    let prelude_crc = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    if crc32(&frame[..8]) != prelude_crc {
        return Err(DecodeError { offset: 0, reason: "не сошёлся prelude CRC".into() });
    }
    let message_crc =
        u32::from_be_bytes([frame[total - 4], frame[total - 3], frame[total - 2], frame[total - 1]]);
    if crc32(&frame[..total - 4]) != message_crc {
        return Err(DecodeError { offset: 0, reason: "не сошёлся message CRC".into() });
    }
    let (headers, headers_end) = parse_headers(&frame[12..12 + headers_len], 12)?;
    let _ = headers_end;
    let payload_bytes = &frame[12 + headers_len..total - 4];
    let payload: serde_json::Value = serde_json::from_slice(payload_bytes).map_err(|e| {
        DecodeError { offset: 12 + headers_len, reason: format!("payload не JSON: {e}") }
    })?;
    let message_type = headers.get(":message-type").cloned().unwrap_or_default();
    let event_type = headers.get(":event-type").cloned().unwrap_or_default();
    Ok(Some((EventFrame { message_type, event_type, payload }, total)))
}

fn parse_headers(
    data: &[u8],
    base: usize,
) -> Result<(std::collections::HashMap<String, String>, usize), DecodeError> {
    let mut map = std::collections::HashMap::new();
    let mut i = 0;
    while i < data.len() {
        let name_len = *data.get(i).ok_or_else(|| DecodeError {
            offset: base + i,
            reason: "обрезан заголовок: нет длины имени".into(),
        })? as usize;
        i += 1;
        if i + name_len > data.len() {
            return Err(DecodeError {
                offset: base + i,
                reason: "обрезан заголовок: нет имени".into(),
            });
        }
        let name = std::str::from_utf8(&data[i..i + name_len]).map_err(|_| DecodeError {
            offset: base + i,
            reason: "имя заголовка не UTF-8".into(),
        })?;
        i += name_len;
        let typ = *data.get(i).ok_or_else(|| DecodeError {
            offset: base + i,
            reason: format!("обрезан заголовок «{name}»: нет типа значения"),
        })?;
        i += 1;
        let value = match typ {
            0 => "true".to_string(),
            1 => "false".to_string(),
            2 => {
                i += 1;
                String::new()
            }
            3 => {
                i += 2;
                String::new()
            }
            4 => {
                i += 4;
                String::new()
            }
            5 => {
                i += 8;
                String::new()
            }
            6 => {
                let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
                i += 2 + len;
                String::new()
            }
            7 => {
                let len = u16::from_be_bytes([
                    *data.get(i).ok_or_else(|| DecodeError {
                        offset: base + i,
                        reason: format!("обрезан заголовок «{name}»: нет длины строки"),
                    })?,
                    *data.get(i + 1).ok_or_else(|| DecodeError {
                        offset: base + i + 1,
                        reason: format!("обрезан заголовок «{name}»: нет длины строки"),
                    })?,
                ]) as usize;
                i += 2;
                let s = std::str::from_utf8(data.get(i..i + len).ok_or_else(|| {
                    DecodeError {
                        offset: base + i,
                        reason: format!("обрезан заголовок «{name}»: нет значения"),
                    }
                })?)
                .map_err(|_| DecodeError {
                    offset: base + i,
                    reason: format!("значение заголовка «{name}» не UTF-8"),
                })?;
                i += len;
                s.to_string()
            }
            8 => {
                i += 8;
                String::new()
            }
            9 => {
                i += 16;
                String::new()
            }
            other => {
                return Err(DecodeError {
                    offset: base + i - 1,
                    reason: format!("неизвестный тип значения заголовка «{name}»: {other}"),
                })
            }
        };
        if i > data.len() {
            return Err(DecodeError {
                offset: base + i,
                reason: format!("заголовок «{name}» выходит за границу секции"),
            });
        }
        map.insert(name.to_string(), value);
    }
    Ok((map, i))
}

/// CRC-32 (IEEE, полином `0xEDB88320`) — как в 9Router.
pub fn crc32(data: &[u8]) -> u32 {
    // Таблица генерируется при первом вызове: 256 u32 — дешевле, чем
    // зависимость ради одной функции, и честнее, чем магия констант.
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Собрать фрейм (только для тестов): обратный путь к парсеру.
#[cfg(test)]
pub fn build_frame(message_type: &str, event_type: &str, payload: &[u8]) -> Vec<u8> {
    fn header(out: &mut Vec<u8>, name: &str, value: &str) {
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
        out.push(7u8); // string
        out.extend_from_slice(&(value.len() as u16).to_be_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    let mut frame: Vec<u8> = Vec::new();
    frame.extend_from_slice(&[0u8; 12]); // место под prelude
    let headers_start = 12;
    header(&mut frame, ":message-type", message_type);
    header(&mut frame, ":event-type", event_type);
    let headers_len = frame.len() - headers_start;
    frame.extend_from_slice(payload);
    let total = frame.len() + 4;
    frame[0..4].copy_from_slice(&(total as u32).to_be_bytes());
    frame[4..8].copy_from_slice(&(headers_len as u32).to_be_bytes());
    let pre = crc32(&frame[..8]);
    frame[8..12].copy_from_slice(&pre.to_be_bytes());
    let msg = crc32(&frame[..]);
    frame.extend_from_slice(&msg.to_be_bytes());
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(s: &str) -> Vec<u8> {
        serde_json::json!({ "content": s }).to_string().into_bytes()
    }

    #[test]
    fn roundtrip_single_frame() {
        let raw = build_frame("event", "assistantResponseEvent", &payload("hi"));
        let mut d = EventStreamDecoder::new();
        let frames = d.feed(&raw).expect("decode");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].message_type, "event");
        assert_eq!(frames[0].event_type, "assistantResponseEvent");
        assert_eq!(frames[0].payload["content"], "hi");
    }

    #[test]
    fn split_chunk_waits_for_rest() {
        let raw = build_frame("event", "assistantResponseEvent", &payload("hi"));
        let mut d = EventStreamDecoder::new();
        let cut = raw.len() / 2;
        assert!(d.feed(&raw[..cut]).expect("part").is_empty(), "полфрейма — не фрейм");
        let frames = d.feed(&raw[cut..]).expect("rest");
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn broken_prelude_crc_is_error_with_offset() {
        let mut raw = build_frame("event", "assistantResponseEvent", &payload("x"));
        raw[8] ^= 0xFF;
        let mut d = EventStreamDecoder::new();
        let err = d.feed(&raw).expect_err("CRC обязан не сойтись");
        assert!(err.to_string().contains("prelude CRC"), "{err}");
    }

    #[test]
    fn broken_message_crc_is_error() {
        let mut raw = build_frame("event", "assistantResponseEvent", &payload("x"));
        let last = raw.len() - 1;
        raw[last] ^= 0xFF;
        let mut d = EventStreamDecoder::new();
        let err = d.feed(&raw).expect_err("CRC обязан не сойтись");
        assert!(err.to_string().contains("message CRC"), "{err}");
    }

    #[test]
    fn garbage_length_is_rejected() {
        let mut d = EventStreamDecoder::new();
        let err = d.feed(&[0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0])
            .expect_err("гигантская длина обязана отвергаться");
        assert!(err.to_string().contains("длина фрейма"), "{err}");
    }

    #[test]
    fn error_frames_are_visible_not_silent() {
        let raw = build_frame(
            "exception",
            "com.amazon.codewhisperer#ValidationException",
            &serde_json::json!({ "message": "bad request" }).to_string().into_bytes(),
        );
        let mut d = EventStreamDecoder::new();
        let frames = d.feed(&raw).expect("decode");
        assert_eq!(frames[0].message_type, "exception");
        assert_eq!(frames[0].payload["message"], "bad request");
    }
}

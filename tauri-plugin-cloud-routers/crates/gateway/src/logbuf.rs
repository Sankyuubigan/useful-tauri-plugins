//! Кольцевой буфер событий для дашборда.
//!
//! ## Зачем
//!
//! Проксирование LLM — это в первую очередь отладка. «Шлюз вернул 429» или
//! «ключ `k1` ушёл в кулдаун, запрос ушёл на `k2`» — информация, которой нет
//! ни в ответе IDE, ни в состоянии конфига. Писать её в файл лога и читать
//! `tail` вручную можно, но дашборд открыт всегда, когда пользователь
//! разбирается с провайдером.
//!
//! ## Почему в памяти, а не в файле
//!
//! Кольцевой буфер **не является** журналом аудита: у него нет требований
//! «пережить перезапуск». Полный лог шлюза пишется в `gateway.log` (stdout
//! приложения перенаправляется туда плагином). Дублировать то же самое в файл
//! означало бы две правды об одном событии.
//!
//! ## Ключевые секреты сюда не попадают
//!
//! Событие — это строка, формируемая вызывающим кодом. Правило: пишем id
//! ключа и префикс секрета, но не полный токен. Иначе дашборд, доступный
//! любому локальному процессу без авторизации, стал бы файлом с ключами.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;

/// Максимальное число событий в буфере.
const CAPACITY: usize = 500;

/// Одно событие для дашборда.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    /// Монотонный номер — дашборд догружает только новое с последнего.
    pub seq: u64,
    /// Unix-время в миллисекундах.
    pub ts_ms: u64,
    /// Уровень: `info` | `warn` | `error`.
    pub level: &'static str,
    /// Текст сообщения.
    pub message: String,
}

/// Кольцевой буфер последних событий.
pub struct EventLog {
    events: Mutex<Vec<Event>>,
    next_seq: AtomicU64,
    capacity: usize,
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLog {
    pub fn new() -> Self {
        Self { events: Mutex::new(Vec::new()), next_seq: AtomicU64::new(1), capacity: CAPACITY }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Event>> {
        self.events.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Записать событие. Отдаёт его монотонный номер.
    pub fn push(&self, level: &'static str, message: impl Into<String>) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let ev = Event {
            seq,
            ts_ms: now_ms(),
            level,
            message: message.into(),
        };
        // Тот же уровень дублируется в stdout: полный лог остаётся в gateway.log.
        match level {
            "error" => log::error!("cloud-routers-gateway: {}", ev.message),
            "warn" => log::warn!("cloud-routers-gateway: {}", ev.message),
            _ => log::info!("cloud-routers-gateway: {}", ev.message),
        }
        let mut buf = self.lock();
        buf.push(ev);
        if buf.len() > self.capacity {
            // Разница положительная, поэтому drop без переноса памяти.
            let excess = buf.len() - self.capacity;
            buf.drain(0..excess);
        }
        seq
    }

    pub fn info(&self, message: impl Into<String>) -> u64 {
        self.push("info", message)
    }

    pub fn warn(&self, message: impl Into<String>) -> u64 {
        self.push("warn", message)
    }

    pub fn error(&self, message: impl Into<String>) -> u64 {
        self.push("error", message)
    }

    /// Последние события, у которых `seq > since`.
    pub fn since(&self, since: u64) -> Vec<Event> {
        self.lock().iter().filter(|e| e.seq > since).cloned().collect()
    }

    /// Последние `limit` событий (для первичной загрузки дашборда).
    pub fn tail(&self, limit: usize) -> Vec<Event> {
        let buf = self.lock();
        let limit = limit.clamp(1, self.capacity);
        buf.iter().rev().take(limit).cloned().collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Номер последнего выданного события.
    pub fn last_seq(&self) -> u64 {
        self.next_seq.load(Ordering::Relaxed).saturating_sub(1)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Замаскировать секрет для показа в UI и журнале.
///
/// Показывается начало и длина — этого достаточно, чтобы человек узнал свой
/// ключ в списке, но недостаточно, чтобы использовать его. Токены короче 12
/// символов показываются целиком-звёздочками: частичное раскрытие короткой
/// строки опаснее её отсутствия.
pub fn mask_secret(secret: &str) -> String {
    let s = secret.trim();
    if s.len() < 12 {
        return "*".repeat(s.len());
    }
    let head: String = s.chars().take(6).collect();
    format!("{head}{}{}", "*".repeat(8), s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_hides_middle_of_long_secret() {
        let secret = "gsk_1234567890abcdef";
        let masked = mask_secret(secret);
        // Префикс помогает человеку узнать свой ключ в списке.
        assert!(masked.starts_with("gsk_12"), "{}", masked);
        assert!(masked.contains("********"), "{}", masked);
        // Длина помогает отличить два ключа одного провайдера.
        assert!(
            masked.ends_with(&secret.len().to_string()),
            "в маске должна быть длина секрета: {} для {}",
            masked,
            secret.len()
        );
        // Хвост секрета не показывается НИКОГДА: по нему ключ можно подобрать.
        assert!(!masked.contains("7890abcdef"), "хвост секрета утёк: {}", masked);
        assert!(!masked.contains("abcdef"), "хвост секрета утёк: {}", masked);
    }

    #[test]
    fn mask_hides_short_secret_entirely() {
        // Короткая строка: частичное раскрытие опаснее, чем полное скрытие.
        assert_eq!(mask_secret("short"), "*****".to_string());
        assert_eq!(mask_secret(""), "");
    }

    #[test]
    fn push_assigns_monotonic_sequence() {
        let log = EventLog::new();
        let a = log.info("first");
        let b = log.warn("second");
        let c = log.error("third");
        assert!(a < b && b < c, "{a} {b} {c}");
        assert_eq!(log.last_seq(), c);
    }

    #[test]
    fn since_returns_only_newer_events() {
        let log = EventLog::new();
        log.info("one");
        let mark = log.info("two");
        log.info("three");
        let got = log.since(mark);
        let msgs: Vec<&str> = got.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(msgs, vec!["three"]);
    }

    #[test]
    fn tail_returns_most_recent_in_chronological_order() {
        let log = EventLog::new();
        for i in 0..5 {
            log.info(format!("e{i}"));
        }
        let got = log.tail(3);
        let msgs: Vec<&str> = got.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(msgs, vec!["e2", "e3", "e4"]);
    }

    #[test]
    fn tail_clamps_limit() {
        let log = EventLog::new();
        log.info("only");
        assert_eq!(log.tail(0).len(), 1, "limit=0 не должен отдавать пустоту");
        assert_eq!(log.tail(usize::MAX).len(), 1);
    }

    #[test]
    fn buffer_is_bounded() {
        let log = EventLog::new();
        for i in 0..(CAPACITY + 200) {
            log.info(format!("e{i}"));
        }
        let buf = log.lock();
        assert_eq!(buf.len(), CAPACITY, "буфер обязан быть ограничен");
        // Самые старые вытеснены, самые свежие на месте.
        assert_eq!(buf.last().expect("last").message, format!("e{}", CAPACITY + 199));
        assert!(buf.first().expect("first").message.contains("e200"));
    }
}
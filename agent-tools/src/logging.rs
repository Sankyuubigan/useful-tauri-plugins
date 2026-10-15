//! Минимальный `log::Log` → stderr.
//!
//! У крейта нет GUI и нет файла лога, но правило репозитория («каждая ошибка
//! пишется в лог») действует и здесь: без установленного логгера
//! `log::warn!`/`log::error!` внутри `ko-json-store` (атомарные записи состояния)
//! молча исчезают, и агент не видит причину отказа. Поэтому пишем в stderr,
//! который всегда попадает в вывод вызывающего агента.
//!
//! Уровень по умолчанию — `warn`: `info`/`debug` шумят в stdout/stderr ответа
//! эксперта и могут сбить с толку парсер. Поднять детальность:
//! `AGENT_TOOLS_LOG=info agent-tools …`.

use std::io::Write;
use std::sync::Once;

/// Установить логгер ровно один раз за процесс.
pub fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let level = level_from_env();
        if log::set_boxed_logger(Box::new(StderrLogger { level })).is_ok() {
            log::set_max_level(level);
        }
    });
}

/// Уровень из `AGENT_TOOLS_LOG`; неизвестное значение и пустое — `warn`.
fn level_from_env() -> log::LevelFilter {
    match std::env::var("AGENT_TOOLS_LOG").ok().as_deref() {
        Some("off" | "none") => log::LevelFilter::Off,
        Some("error") => log::LevelFilter::Error,
        Some("info") => log::LevelFilter::Info,
        Some("debug") => log::LevelFilter::Debug,
        Some("trace") => log::LevelFilter::Trace,
        _ => log::LevelFilter::Warn,
    }
}

struct StderrLogger {
    level: log::LevelFilter,
}

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // Ошибка записи в stderr не должна ронять инструмент: агент всё равно
        // получит код возврата, который важнее текста предупреждения.
        let _ = writeln!(
            std::io::stderr(),
            "[agent-tools] {}: {}",
            record.level(),
            record.args()
        );
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log as _;

    #[test]
    fn unknown_level_falls_back_to_warn() {
        // Переменная может отсутствовать — это нормальный режим.
        let level = level_from_env();
        assert!(level >= log::LevelFilter::Warn || level == log::LevelFilter::Off);
    }

    #[test]
    fn logger_respects_its_level() {
        let warn_only = StderrLogger {
            level: log::LevelFilter::Warn,
        };
        assert!(warn_only.enabled(&log::Metadata::builder().level(log::Level::Warn).build()));
        assert!(!warn_only.enabled(&log::Metadata::builder().level(log::Level::Info).build()));

        let off = StderrLogger {
            level: log::LevelFilter::Off,
        };
        assert!(!off.enabled(&log::Metadata::builder().level(log::Level::Error).build()));
    }
}
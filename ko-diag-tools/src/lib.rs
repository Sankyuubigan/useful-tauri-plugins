//! # ko-diag-tools — SSOT-ядро диагностических тулов
//!
//! Ядро для диагностики Windows-систем: что «болит» и почему. Три слоя:
//!
//! 1. **Статический аудит** ([`snapshot::run_snapshot`]): железо и память,
//!    настройки ядра (HVCI, TDR), файл подкачки, драйверы/процессы.
//! 2. **Криминалистика прошлых сбоев**: журнал Windows (WHEA, нехватка памяти,
//!    отвалы диска/видеодрайвера), скрытые дампы (`LiveKernelReports`,
//!    `Minidump`), краши приложений.
//! 3. **«Чёрный ящик»** ([`recorder`]): фоновый мониторинг с принудительным
//!    сбросом каждой строки на физический носитель (`sync_all`) — последняя
//!    строчка лога выживает даже при намертво зависшей системе.
//!
//! ## Границы
//! - **Без Tauri и без C-компиляции**: `sysinfo` (чистый Rust) + `winreg` +
//!    PowerShell (внешняя программа, не зависимость — см. [`sys_helpers`]).
//! - **Про Windows only — но честно**: вызовы работают на любой ОС сборки
//!    Rust, но целевая платформа крейта — Windows. Реестр и журнал Windows
//!    на других ОС отсутствуют: соответствующие секции возвращают статус
//!    `Failed` с причиной, а не паникуют.
//! - **Проверки независимы**: каждая секция снапшота собирается отдельно;
//!    отказ одной не роняет отчёт и не пропускается молча — пишет свой
//!    статус `Failed` с причиной (правило «программа не врёт»).
//!
//! ## Хост-контракт
//! Хост (King Orch / CLI agent-tools) сам решает, откуда файлы чёрного ящика:
//! передаёт каталог ([`recorder::start_recording`]). Ядро не знает про
//! exe-пути и структуру конфига хоста — только аргументы.

pub mod dumps;
pub mod eventlog;
pub mod identity;
pub mod kernel;
pub mod processes;
pub mod recorder;
pub mod snapshot;
pub mod sys_helpers;

/// Версия ядра (для логов и отчётов host-адаптеров).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Итог проверки секции снапшота.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    /// Проверка прошла, проблем не найдено.
    Ok,
    /// Проверка нашла наблюдения, на которые стоит обратить внимание.
    Findings,
    /// Проверка не смогла выполниться — причина в теле секции.
    Failed,
}

/// Одна секция отчёта диагностики.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Заголовок вида `"[3] ЯДРО WINDOWS: HVCI / TDR"`.
    pub title: String,
    pub status: CheckStatus,
    /// Тело секции: строки без \n в начале/конце; причина отказа — здесь.
    pub body: String,
}

impl Section {
    pub fn new_ok(title: impl Into<String>, body: impl Into<String>) -> Self {
        Section { title: title.into(), status: CheckStatus::Ok, body: body.into() }
    }

    pub fn new_findings(title: impl Into<String>, body: impl Into<String>) -> Self {
        Section { title: title.into(), status: CheckStatus::Findings, body: body.into() }
    }

    /// Секция с честным отказом: к телу автоматически приписывается причина.
    pub fn new_failed(title: impl Into<String>, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Section { title: title.into(), status: CheckStatus::Failed, body: reason }
    }

    /// Статус человекочитаемой строкой (для рендера и тестов).
    pub fn status_str(&self) -> &'static str {
        match self.status {
            CheckStatus::Ok => "OK",
            CheckStatus::Findings => "НАЙДЕНО",
            CheckStatus::Failed => "НЕ УДАЛОСЬ",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_statuses_are_honest() {
        let ok = Section::new_ok("t", "clean");
        assert_eq!(ok.status_str(), "OK");
        let failed = Section::new_failed("t", "нет прав администратора");
        assert_eq!(failed.status_str(), "НЕ УДАЛОСЬ");
        // Причина отказа обязана попасть в тело (§2.2: молчаливых отказов нет).
        assert!(failed.body.contains("нет прав"),
            "причина отказа должна быть в теле секции: {}", failed.body);
    }
}

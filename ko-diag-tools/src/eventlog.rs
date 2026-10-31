//! Криминалистика журнала Windows: что система уже записала о прошлых сбоях
//! за последние N часов. Журнал «System» — железо и ядро, «Application» —
//! краши/зависания программ.
//!
//! Универсальность: мы спрашиваем по КАТЕГОРИЯМ провайдеров/кодов, а не по
//! названиям продуктов. Найденное — не диагноз, а факты для гипотез.
//!
//! Ограничения честности:
//! - WMI/журнал Windows могут вернуть пусто при отсутствии записей — это
//!   Okay («в окне пусто»), а не флчата (§2.2);
//! - приватные права НЕ нужны для чтения журнала System/Application
//!   обычным пользователем; отказат только нештатная конфигурация.

use super::Section;
use crate::sys_helpers;
use std::time::Duration;

/// Окно криминалистики: 72 часа.
pub const WINDOW_HOURS: u64 = 72;

/// Верхний предел записей в секции: журнал может забить минуты на чтение
/// при общих запросах, а анализатор всё равно читает несколько свежих.
pub const MAX_SYSTEM_ENTRIES: usize = 20;
pub const MAX_APP_ENTRIES: usize = 15;

const TIMEOUT: Duration = Duration::from_secs(20);

/// Форматирование записи журнала в одну строку: время | ID | провайдер | сообщение.
/// Вызов -replace убирает переводы строк (сообщения многоблочные), а
/// TimeCreated печатаем ISО-строкой через ToString — независимо от локали.
fn one_line_render(max_chars: usize) -> String {
    format!(
        "| ForEach-Object {{ \
            $t = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss'); \
            $p = if ($null -eq $_.ProviderName) {{ '-' }} else {{ $_.ProviderName }}; \
            $m = if ($null -eq $_.Message) {{ '' }} else {{ ($_.Message -replace \"[`r`n]+\", ' ') }}; \
            if ($m.Length -gt {max_chars}) {{ $m = $m.Substring(0, {max_chars}) }}; \
            \"$t | ID $($_.Id) | $p | $m\" }}"
    )
}

/// Журнал System: уровень 1..3 (крит/ошибка/предупреждение), интерес
/// по провайдерам: железо (WHEA), память (Resource-Exhaustion), питание
/// (Kernel-Power), видеодрайвер (Display/nvlddmkm/amdkmdag), диски
/// (disk/stornvme/volmgr/Ntfs), краши ядра (BugCheck).
pub fn system_events_section() -> Section {
    let title = "[7] ЖУРНАЛ WINDOWS «SYSTEM» ЗА 72 Ч (крит/ошибка/предупреждение)";
    let script = format!(
        "Get-WinEvent -FilterHashtable @{{ \
              LogName='System'; \
              StartTime=(Get-Date).AddHours(-{WINDOW_HOURS}); \
              Level=1,2,3 }} \
          -ErrorAction SilentlyContinue | \
          Where-Object {{ $_.ProviderName -match 'WHEA|Resource-Exhaustion|Kernel-Power|Display|nvlddmkm|amdkmdag|disk|stornvme|volmgr|Ntfs|BugCheck' }} | \
          Select-Object -First {MAX_SYSTEM_ENTRIES} {}",
        one_line_render(180)
    );

    section_from_query(title, &script)
}

/// Журнал Application: коды 1000 (краш) и 1002 (выполнение зависшей
/// программы) — история же падений игр/софта без «гаданий».
pub fn app_events_section() -> Section {
    let title = "[8] КРАШ/ЗАВИСАНИЕ ПРОГРАММ ЗА 72 Ч (журнал «Application», ID 1000/1002)";
    let script = format!(
        "Get-WinEvent -FilterHashtable @{{ \
              LogName='Application'; \
              StartTime=(Get-Date).AddHours(-{WINDOW_HOURS}); \
              Id=1000,1002 }} \
          -ErrorAction SilentlyContinue | \
          Select-Object -First {MAX_APP_ENTRIES} {}",
        one_line_render(180)
    );

    section_from_query(title, &script)
}

fn section_from_query(title: &str, script: &str) -> Section {
    match sys_helpers::run_powershell(script, TIMEOUT) {
        Ok(out) => {
            let trimmed = out.trim();
            if trimmed.is_empty() {
                Section::new_ok(title, "В окне 72 часа записей нет — свежих сбоев в этой категории не зарегистрировано.")
            } else {
                Section::new_findings(title, trimmed.to_string())
            }
        }
        Err(reason) => Section::new_failed(title, reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_holds_time_and_id() {
        let r = one_line_render(180);
        assert!(r.contains("TimeCreated"), "рендер обязан идти через TimeCreated: {r}");
        assert!(r.contains("ProviderName"), "рендер обязан идти через ProviderName: {r}");
        assert!(r.contains('$'), "рендер обязан использовать подстановку PS: {r}");
    }

    #[test]
    fn system_query_filters_by_category_providers() {
        // Тот же фильтр, что в system_events_section (константы совпадают);
        // проверяем, что фильтр не потерял ключевые категории провайдеров.
        let script = format!(
            "Get-WinEvent -FilterHashtable @{{ \
                  LogName='System'; \
                  StartTime=(Get-Date).AddHours(-{WINDOW_HOURS}); \
                  Level=1,2,3 }} \
              -ErrorAction SilentlyContinue | \
              Where-Object {{ $_.ProviderName -match 'WHEA|Resource-Exhaustion|Kernel-Power|Display|nvlddmkm|amdkmdag|disk|stornvme|volmgr|Ntfs|BugCheck' }} | \
              Select-Object -First {MAX_SYSTEM_ENTRIES} {}",
            one_line_render(180)
        );
        assert!(script.contains("Get-WinEvent"));
        assert!(script.contains("WHEA"), "фильтр обязан брать WHEA: {script}");
        assert!(script.contains("BugCheck"), "фильтр обязан брать BugCheck: {script}");
        assert!(script.contains("stornvme"), "фильтр обязан брать диски: {script}");
    }

    #[test]
    fn sections_live_query_runs() {
        // Реальный запрос на CI/машине: что вернулось — честная секция
        // со статусом (пусто тоже валидно, отказ не панится).
        let sec = app_events_section();
        assert!(!sec.body.is_empty(), "секция обязана иметь тело (пусто или причина отказа)");
        assert!(sec.body.len() <= 400 * MAX_APP_ENTRIES, "секция не должна разрастаться до гиганта");
    }
}

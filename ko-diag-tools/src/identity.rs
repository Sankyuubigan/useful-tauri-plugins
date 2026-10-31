//! Идентификация системы: ОС, процессор, ОЗУ + файл подкачки, видеокарта.
//!
//! Источники:
//! - `System` (sysinfo) — ОС/ЦП/ОЗУ/своп, готовый вид;
//! - реестр — `PagingFiles` (конфигурация файла подкачки);
//! - PowerShell `Win32_VideoController` — видеокарта (в sysinfo 0.30 её нет).

use super::Section;
use crate::sys_helpers;
use sysinfo::System;
use winreg::enums::*;
use winreg::RegKey;

/// Порог «файл подкачки подозрительно мал»: ниже система выживает, но при
/// исчерпании ОЗУ держится на ничтожной виртуальной памяти и способна
/// намертво зависнуть. Универсальный признак, не привязан к софту.
const MIN_SWAP_REPORTED_GB: f64 = 8.0;

/// Признаки десктопных Intel 13/14-поколения: серия известна нестабильностью
/// на деградировавших экземплярах под пиковыми нагрузками (архитектурное
/// свойство линейки Raptor/Raptor Refresh: микрокод + дефолтные профили).
/// Это общее инженерное знание о железе, а не кейс конкретного юзера.
const INTEL_1314_TOKENS: [&str; 6] = ["13700", "13900", "14700", "14900", "13600", "14600"];

fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1_073_741_824.0
}

fn fmt_gb(bytes: u64) -> String {
    format!("{:.2} ГБ", gb(bytes))
}

/// Секция [1]: ОС и аптайм (все значения — статические методы sysinfo).
pub fn os_section() -> Section {
    let host = System::host_name().unwrap_or_else(|| "<нет>".into());
    let os = System::long_os_version().unwrap_or_else(|| "<не определена>".into());
    let kernel = System::kernel_version().unwrap_or_else(|| "<не определена>".into());
    let uptime_min = System::uptime() / 60;
    Section::new_ok(
        "[1] ОС И АПТАЙМ",
        format!("Хост: {host}\nОС: {os}\nЯдро: {kernel}\nАптайм: {uptime_min} мин"),
    )
}

/// Секция [2]: процессор.
pub fn cpu_section(sys: &System) -> Section {
    let cpus = sys.cpus();
    let mut lines = Vec::new();
    if let Some(cpu) = cpus.first() {
        let brand = cpu.brand().trim();
        let brand_out = if brand.is_empty() { "<не определена>" } else { brand };
        lines.push(format!(
            "Модель: {brand_out} | лог. ядер: {} | частота: {} МГц",
            cpus.len(),
            cpu.frequency()
        ));
        for token in INTEL_1314_TOKENS {
            if brand_out.contains(token) {
                lines.push(
                    "[ЗАМЕТКА] Процессор Intel 13/14 поколения: серия известна нестабильностью \
                     (Vmin Shift) на деградировавших экземплярах под пиковыми нагрузками. \
                     Мероприятия: свежий BIOS/микрокод, дефолтные профили питания и ОЗУ.".to_string(),
                );
            }
        }
    } else {
        lines.push("<процессоры не определены>".to_string());
    }
    Section::new_ok("[2] ПРОЦЕССОР", lines.join("\n"))
}

/// Секция [3]: ОЗУ + файл подкачки. sysinfo = состояние «сейчас»,
/// реестр PagingFiles = конфигурация (вступает после перезагрузки).
pub fn memory_section(sys: &System) -> Section {
    let mut lines = vec![
        format!(
            "ОЗУ всего: {} (доступно сейчас: {})",
            fmt_gb(sys.total_memory()),
            fmt_gb(sys.available_memory())
        ),
        format!(
            "Своп — видно сейчас: всего {}, занято {}",
            fmt_gb(sys.total_swap()),
            fmt_gb(sys.used_swap())
        ),
        format!("Процессов запущено: {}", sys.processes().len()),
    ];

    let mut findings = false;
    if gb(sys.total_swap()) < MIN_SWAP_REPORTED_GB {
        findings = true;
        lines.push(
            "[!] КРИТИЧНО: файл подкачки отключён или слишком мал (< 8 ГБ) — \
             при исчерпании ОЗУ система может зависнуть наглухо."
                .to_string(),
        );
    }

    match read_paging_files_config() {
        Ok(config_lines) => lines.extend(config_lines),
        Err(reason) => {
            // Отказ чтения реестра не глотается: причина видна в отчёте.
            lines.push(format!("Конфигурация подкачки (реестр): не удалось: {reason}"));
        }
    }

    if findings {
        Section::new_findings("[3] ПАМЯТЬ И ФАЙЛ ПОДКАЧКИ", lines.join("\n"))
    } else {
        Section::new_ok("[3] ПАМЯТЬ И ФАЙЛ ПОДКАЧКИ", lines.join("\n"))
    }
}

/// REG_MULTI_SZ `PagingFiles` из Memory Management:
/// `?:\pagefile.sys` — системноуправляемый; `C:\pagefile.sys 2048 4096` —
/// явные min/max (МБ). Пустой список = подкачка отключена вручную.
fn read_paging_files_config() -> Result<Vec<String>, String> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Memory Management")
        .map_err(|e| format!("нет доступа к ключу Memory Management: {e}"))?;
    let entries: Vec<String> = key
        .get_value("PagingFiles")
        .map_err(|e| format!("значение PagingFiles не читается: {e}"))?;

    if entries.is_empty() {
        return Ok(vec![
            "[!] Конфигурация подкачки: PagingFiles ПУСТ — отключён вручную.".to_string()
        ]);
    }
    Ok(entries
        .iter()
        .map(|raw| {
            if raw.trim() == r"?:\pagefile.sys" {
                "Конфигурация подкачки: системноуправляемый (все диски)".to_string()
            } else {
                format!("Конфигурация подкачки: {raw}")
            }
        })
        .collect())
}

/// Секция [4]: видеокарта (Win32_VideoController).
///
/// Известное WMI-ограничение: `AdapterRAM` — uint32, у карт > 4 ГБ значение
/// неверно (обрезано) — указываем это прямо, чтобы анализатор не делал
/// выводов о «малом VRAM».
pub fn video_section() -> Section {
    let script = r#"Get-CimInstance Win32_VideoController | ForEach-Object { $d = $_.DriverDate; $dd = if ($null -ne $d) { $d.ToString('yyyy-MM-dd') } else { '-' }; '{0} | драйвер {1} (от {2}) | статус {3}' -f $_.Name, $_.DriverVersion, $dd, $_.Status }"#;

    match sys_helpers::run_powershell(script, sys_helpers::DEFAULT_POWERSHELL_TIMEOUT) {
        Ok(out) if out.trim().is_empty() => Section::new_failed(
            "[4] ВИДЕОКАРТА",
            "WMI не вернул ни одного видеоадаптера (Win32_VideoController пуст)",
        ),
        Ok(out) => Section::new_ok(
            "[4] ВИДЕОКАРТА",
            format!(
                "Ограничение WMI: AdapterRAM не отражает объём > 4 ГБ — не делай выводов о VRAM.\n{}",
                out.trim()
            ),
        ),
        Err(reason) => Section::new_failed("[4] ВИДЕОКАРТА", reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CheckStatus;

    #[test]
    fn gb_formats_are_readable() {
        assert_eq!(fmt_gb(1_073_741_824), "1.00 ГБ");
        assert_eq!(fmt_gb(0), "0.00 ГБ");
    }

    #[test]
    fn section_status_helper_maps_findings() {
        // Инвариант секции памяти: статус Findings возможен (своп < порога
        // выражается через этот статус), но проверяется не на числах машины,
        // а на пересборке секции вручную — Findings всегда рендерится статусом.
        let s = Section::new_findings("t", "наблюдение");
        assert_eq!(s.status_str(), "НАЙДЕНО");
        assert_eq!(s.status, CheckStatus::Findings);
    }

    #[test]
    fn memory_section_names_ram_and_swap() {
        // На CI/машине значения разные; проверяем СТРУКТУРУ отчёта:
        // обязательные строки обязаны присутствовать при любой машине.
        let sys = System::new_all();
        let section = memory_section(&sys);
        assert!(section.body.contains("ОЗУ всего"), "нет ОЗУ:\n{}", section.body);
        assert!(section.body.contains("Своп"), "нет свопа:\n{}", section.body);
        assert!(section.body.contains("Процессов запущено"), "нет числа процессов:\n{}", section.body);
    }

    #[test]
    fn cpu_section_names_the_model_line() {
        let sys = System::new_all();
        let section = cpu_section(&sys);
        assert!(
            section.body.contains("Модель:"),
            "секция ЦП обязана начинаться с модели:\n{}", section.body
        );
    }
}

//! Параметры ядра Windows из реестра: изоляция ядра (HVCI) и TDR
//! (наблюдение за «зависанием» видеодрайвера). Проверки чисто-реестровые:
//! ни PowerShell, ни админ-права не нужны.
//!
//! Что меряем и почему (общее инженерное знание, не кейс юзера):
//! - **HVCI** («Целостность памяти») — гипервизорный фильтр кода ядра:
//!   сторонние драйверы, не переписанные под него, сбоят чаще;
//! - **TDR** — тайминги перезапуска зависшего видеодрайвера: их меняют
//!   «лечилом», превращая обычный сброс драйвера в намертво висящую систему.

use std::time::Duration;

use winreg::enums::*;

use super::Section;
use crate::sys_helpers;

const HVCI_KEY: &str =
    "SYSTEM\\CurrentControlSet\\Control\\DeviceGuard\\Scenarios\\HypervisorEnforcedCodeIntegrity";
const GRAPHICS_KEY: &str = "SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers";

fn read_u32(subkey: &str, value: &str) -> Result<u32, String> {
    let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey(subkey)
        .map_err(|e| format!("ключ {subkey} не открыт: {e}"))?;
    key.get_value(value)
        .map_err(|e| format!("значение {value} у ключа {subkey}: {e}"))
}

/// Секция [5]: HVCI. Enabled=1 → Findings: сторонние непосланные
/// драйверы (античиты, старые антивирусы) под гипервизором сбоили чаще.
pub fn hvci_section() -> Section {
    match read_u32(HVCI_KEY, "Enabled") {
        Ok(1) => Section::new_findings(
            "[5] ИЗОЛЯЦИЯ ЯДРА (HVCI)",
            "Открыт Enabled = 1: «Целостность памяти» ВКЛЮЧЕНА. \
             Сторонние драйверы Ring-0, несовместимые с гипервизором, под ним \
             сбоят значительно чаще. Отключение — Windows Security \
             → Device Security → Core isolation → Memory integrity.",
        ),
        Ok(_) => Section::new_ok(
            "[5] ИЗОЛЯЦИЯ ЯДРА (HVCI)",
            "Целостность памяти НЕ активна (Enabled != 1) — гипервизорный фильтр ядра отсутствует.",
        ),
        Err(reason) if reason.contains("не открыт") => Section::new_ok(
            "[5] ИЗОЛЯЦИЯ ЯДРА (HVCI)",
            "Ключ HVCI отсутствует: изоляция ядра не активировалась ни разу (штатно для большинства систем).",
        ),
        Err(reason) => Section::new_failed("[5] ИЗОЛЯЦИЯ ЯДРА (HVCI)", reason),
    }
}

/// Секция [6]: TDR — тайминги перезапуска видеодрайвера.
/// Дефолты: TdrLevel=3 (recover), TdrDelay=2 c, TdrDdiDelay=5 c.
/// TdrLevel=0 (отключён вообще) и завышенные TdrDelay означают, что кто-то
/// уже «лечил» зависания — классический источник «намертво зависших» систем
/// вместо обычного сброса драйвера.
pub fn tdr_section() -> Section {
    let mut lines = Vec::new();
    let mut findings = false;

    match read_u32(GRAPHICS_KEY, "TdrLevel") {
        Ok(0) => {
            findings = true;
            lines.push("[!] TdrLevel = 0: перезапуск видеодрайвера ОТКЛЮЧЁН реестром. \
                        Отвал драйвера теперь зависает всю систему наглухо \
                        вместо честного чёрного экрана и восстановления."
                .to_string());
        }
        Ok(level) => lines.push(format!("TdrLevel = {level} (штат: 3 — проба «recover»)")),
        Err(reason) => {
            lines.push(format!("TdrLevel: не задан (дефолт 3). Детали: {reason}"))
        }
    }

    match read_u32(GRAPHICS_KEY, "TdrDelay") {
        Ok(delay) if delay > 2 => {
            findings = true;
            lines.push(format!(
                "[ЗАМЕТКА] TdrDelay = {delay} с (штат: 2). Кто-то увеличивал тайминги — \
                 вместо сброса драйвера система дольше висит."
            ));
        }
        Ok(delay) => lines.push(format!("TdrDelay = {delay} с (штат: 2)")),
        Err(reason) => {
            lines.push(format!("TdrDelay: не задан (дефолт 2). Детали: {reason}"))
        }
    }

    if findings {
        Section::new_findings("[6] VIDEO TDR (перезапуск видеодрайвера)", lines.join("\n"))
    } else {
        Section::new_ok("[6] VIDEO TDR (перезапуск видеодрайвера)", lines.join("\n"))
    }
}

/// Потолок строк секции сторонних драйверов (первые 50).
const MAX_THIRD_PARTY_DRIVERS: usize = 50;

/// Таймаут: перечисление всех работающих kernel-драйверов + чтение
/// VersionInfo каждого .sys-файла тяжелее обычного WMI-запроса.
const DRIVERS_TIMEOUT: Duration = Duration::from_secs(30);

/// Секция [9]: сторонние (не Microsoft) драйверы, работающие в Ring 0.
///
/// Метод: Win32_SystemDriver (State=Running) → CompanyName из VersionInfo
/// каждого .sys → только НЕ Microsoft. Сторонний драйвер — не «виноват»:
/// это факты для гипотез (античиты, антивирусы, утилиты сидят ближе всех
/// к железу и чаще прочего фигурируют в зависаниях и чёрных экранах).
pub fn third_party_drivers_section() -> Section {
    let title = "[9] СТОРОННИЕ ДРАЙВЕРЫ ЯДРА (RING 0, НЕ MICROSOFT)";
    let script = format!(
        r#"$rows = Get-CimInstance Win32_SystemDriver | Where-Object {{ $_.State -eq 'Running' -and $_.PathName }} | ForEach-Object {{ $p = $_.PathName -replace '^\\\?\?\\', ''; if ($p -and (Test-Path -LiteralPath $p)) {{ $v = (Get-Item -LiteralPath $p).VersionInfo; if ($v.CompanyName -and ($v.CompanyName -notmatch 'Microsoft')) {{ '{{0}} | {{1}} | {{2}} | {{3}} | {{4}}' -f $_.Name, $_.DisplayName, $v.CompanyName, $v.FileVersion, $p }} }} }}; if ($rows) {{ $rows | Select-Object -First {MAX_THIRD_PARTY_DRIVERS} }}"#
    );

    match sys_helpers::run_powershell(&script, DRIVERS_TIMEOUT) {
        Ok(out) => {
            let trimmed = out.trim();
            if trimmed.is_empty() {
                Section::new_ok(
                    title,
                    "Среди работающих kernel-драйверов сторонних (не Microsoft) не найдено \
                     (либо у всех файлов не читается VersionInfo).",
                )
            } else {
                Section::new_findings(
                    title,
                    format!(
                        "Сторонние драйверы в ядре — факты для гипотез, не приговор \
                         (античиты/антивирусы/утилиты сидят ближе всех к железу):\n{trimmed}"
                    ),
                )
            }
        }
        Err(reason) => Section::new_failed(title, reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_paths_are_canonical() {
        // Замаранные пути — корень иных ошибок. Проверяем их единственный
        // SSOT-вид (никаких дубликатов в других модулях этого проекта нет).
        assert!(HVCI_KEY.contains("DeviceGuard"));
        assert!(HVCI_KEY.contains("HypervisorEnforcedCodeIntegrity"));
        assert!(GRAPHICS_KEY.contains("GraphicsDrivers"));
    }

    #[test]
    fn hvci_and_tdr_sections_render_some_verdict() {
        // На CI/машине значения разные; проверяем, что секция живая:
        // тело непусто при любом раскладе, статусы — валидными строками.
        let hvci = hvci_section();
        assert!(!hvci.body.is_empty(), "HVCI секция пустая");
        let tdr = tdr_section();
        assert!(!tdr.body.is_empty(), "TDR секция пустая");
        assert!(tdr.body.contains("TdrLevel"), "TDR обязан говорить TdrLevel:\n{}", tdr.body);
    }

    #[test]
    fn third_party_drivers_section_renders_honest_status() {
        // Живой запуск (PowerShell до 30 с): статус валиден при любом
        // раскладе, тело непусто (§2.2: пустых секций нет).
        let sec = third_party_drivers_section();
        assert!(!sec.body.is_empty(), "секция обязана иметь тело:\n{}", sec.body);
        assert!(
            matches!(sec.status_str(), "OK" | "НАЙДЕНО" | "НЕ УДАЛОСЬ"),
            "статус обязан быть валидным: {}",
            sec.status_str()
        );
    }
}

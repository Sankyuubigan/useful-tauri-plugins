//! Параметры ядра Windows из реестра: изоляция ядра (HVCI) и TDR
//! (наблюдение за «зависанием» видеодрайвера). Проверки чисто-реестровые:
//! ни PowerShell, ни админ-права не нужны.
//!
//! Что меряем и почему (общее инженерное знание, не кейс юзера):
//! - **HVCI** («Целостность памяти») — гипервизорный фильтр кода ядра:
//!   сторонние драйверы, не переписанные под него, сбоят чаще;
//! - **TDR** — тайминги перезапуска зависшего видеодрайвера: их меняют
//!   «лечилом», превращая обычный сброс драйвера в намертво висящую систему.

use winreg::enums::*;

use super::Section;

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
}

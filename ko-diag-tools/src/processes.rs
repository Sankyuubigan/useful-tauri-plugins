//! Топ-процессы прямо сейчас: кто сильнее всех грузит ЦП и кто больше всех
//! ест ОЗУ. Универсальный детектор «виновника» — без имён конкретного софта
//! (антиневеровский принцип: наблюдаешь, а не угадываешь).
//!
//! Нюанс sysinfo 0.30: `Process::cpu_usage()` — процент ОДНОГО логического
//! ядра, и у многопоточного процесса может быть больше 100. Для «процент от
//! всей машины» делим на число ядер — так же, как показывает Монитор
//! ресурсов. Формула написана прямо в строке секции, чтобы анализатор
//! не путал однопроцентные шкалы.

use super::Section;
use sysinfo::{Process, System};

/// Топ процессов в каждой категории.
pub const TOP_N: usize = 10;

/// Приватная замена: fmt-проценты с разделением точки (LLM-анализатор).
fn usage_share(usage: f32, n_cpus: usize) -> f32 {
    if n_cpus == 0 {
        usage
    } else {
        usage / n_cpus as f32
    }
}

struct Row {
    name: String,
    pid: u32,
    value: f64,
}

fn collect_top(
    sys: &System,
    pick: impl Fn(&Process) -> f64,
    fmt_value: impl Fn(&Row) -> String,
) -> Vec<String> {
    let mut rows: Vec<Row> = sys
        .processes()
        .values()
        .filter(|p| !p.name().trim().is_empty())
        .map(|p| Row {
            name: p.name().trim().to_string(),
            pid: p.pid().as_u32(),
            value: pick(p),
        })
        .collect();
    rows.sort_by(|a, b| b.value.total_cmp(&a.value));
    rows.into_iter()
        .take(TOP_N)
        .map(|r| fmt_value(&r))
        .collect()
}

/// Секция [12]: лидеры по ЦП и по ОЗУ.
pub fn processes_section(sys: &System) -> Section {
    let n_cpus = sys.cpus().len().max(1);

    let top_cpu = collect_top(
        sys,
        |p| p.cpu_usage() as f64,
        |r| {
            format!(
                "{} (PID {}) — {:.1}% от всей машины",
                r.name,
                r.pid,
                usage_share(r.value as f32, n_cpus)
            )
        },
    );

    let top_ram = collect_top(
        sys,
        |p| p.memory() as f64,
        |r| format!("{} (PID {}) — {:.0} МБ", r.name, r.pid, r.value),
    );

    let body = format!(
        "Формула CPU: usage узла = share процента одного ядра / число лог. ядер.\n\
         Лидеры по ЦП ({TOP_N}):\n{}\n\n\
         Лидеры по ОЗУ ({TOP_N}):\n{}",
        top_cpu.join("\n"),
        top_ram.join("\n")
    );

    Section::new_ok("[12] ЛИДЕРЫ НАГРУЗКИ ПРЯМО СЕЙЧАС", body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_share_survives_zero_cpus() {
        assert_eq!(usage_share(50.0, 0), 50.0, "делить на 0 нельзя — 0 ядер = сырой usage");
        assert_eq!(usage_share(200.0, 4), 50.0, "200% на 4 ядрах = 50% машины");
    }

    #[test]
    fn top_n_is_ten() {
        assert_eq!(TOP_N, 10);
    }

    #[test]
    fn real_system_yields_dark_rows() {
        // На CI/машине значения разные; инвариант: непустые строки в обеих
        // категориях и каждый ряд называет процесс и PID.
        let sys = sysinfo::System::new_all();
        let section = processes_section(&sys);
        assert!(section.body.contains("Лидеры по ЦП"), "нет заголовка ЦП:\n{}", section.body);
        assert!(section.body.contains("Лидеры по ОЗУ"), "нет заголовка ОЗУ:\n{}", section.body);
        // Хотя бы одна строка с PID (при реальной системе процессы есть всегда).
        assert!(section.body.contains("(PID "), "нет ни одного процесса:\n{}", section.body);
        assert!(
            section.body.len() < 4_096,
            "секция обязана быть компактной ({} байт)",
            section.body.len()
        );
    }
}

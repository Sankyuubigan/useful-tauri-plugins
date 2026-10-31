//! Сборка полного снапшота: независимые секции + рендер отчёта.
//!
//! Контракт честности: каждая секция собирается отдельно, отказ одной
//! не роняет остальные и не проглатывается молча — свой статус Failed
//! с причиной в теле (§2.2). Рендер выводит статус ЛЮБОЙ секции, поэтому
//! анализатор (LLM) видит, какие проверки не состоялись и почему.

use super::Section;
use crate::{dumps, eventlog, identity, kernel, processes, recorder, sys_helpers};
use std::path::Path;

/// Полный снапшот: список независимых секций.
#[derive(Debug, Clone)]
pub struct Diagnosis {
    pub sections: Vec<Section>,
}

/// Запустить полный снапшот.
///
/// `diag_dir` — необязательный каталог чёрного ящика (хост передаёт
/// свой `<...>/diag_outputs/<session>/`). Если передан, добавляется
/// секция свежих логов рекордера — способ агента найти лог прошлого
/// прогона ПОСЛЕ перезагрузки машины (рекордер умер вместе с системой,
/// файл остался на диске).
pub fn run_snapshot(diag_dir: Option<&Path>) -> Diagnosis {
    let sys = sysinfo::System::new_all();

    let mut sections = vec![
        identity::os_section(),
        identity::cpu_section(&sys),
        identity::memory_section(&sys),
        identity::video_section(),
        kernel::hvci_section(),
        kernel::tdr_section(),
    ];
    sections.extend(dumps::dumps_sections());
    sections.push(eventlog::system_events_section());
    sections.push(eventlog::app_events_section());
    sections.push(processes::processes_section(&sys));
    if let Some(dir) = diag_dir {
        sections.push(recorder_logs_section(dir));
    }

    Diagnosis { sections }
}

/// Секция свежих логов цикличного рекордера (последние 24 ч).
///
/// Она НЕ возвращает хвосты (хвосты отдаёт stop-тула); здесь — карта
/// «что вообще есть на диске», чтобы после перезагрузки агент знал,
/// какой файл читать.
pub fn recorder_logs_section(dir: &Path) -> Section {
    let title = "[13] ЛОГИ ЧЁРНОГО ЯЩИКА ЭТОЙ СЕССИИ (24 ч)";
    // Полный список файлов рекордера с путями и давностью.
    let mut rows: Vec<(std::time::SystemTime, String)> = Vec::new();
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            return Section::new_failed(
                title,
                format!("каталог рекордера {} не читается: {e}", dir.display()),
            );
        }
    };
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(recorder::FILE_PREFIX) || !name.ends_with(".log") {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if let Ok(mtime) = meta.modified() {
                let fresh = local_now_unix() - mtime_unix(&mtime) < 86_400;
                if fresh {
                    rows.push((mtime, entry.path().display().to_string()));
                }
            }
        }
    }
    if rows.is_empty() {
        return Section::new_ok(
            title,
            format!("В {} свежих логов (<24 ч) нет.", dir.display()),
        );
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let body = rows
        .into_iter()
        .map(|(_, p)| p)
        .collect::<Vec<_>>()
        .join("\n");
    Section::new_findings(title, body)
}

fn mtime_unix(t: &std::time::SystemTime) -> u64 {
    t.duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn local_now_unix() -> u64 {
    chrono::Local::now().timestamp() as u64
}

impl Diagnosis {
    /// Рендер в текст для тул-вывода (LLM-анализатор читает этот текст).
    pub fn render(&self) -> String {
        let elevated = match sys_helpers::is_elevated() {
            Some(true) => "администратор",
            Some(false) => "обычный пользователь (часть проверок будет недоступна — причины в секциях)",
            None => "не удалось определить (нет net.exe?)",
        };
        let mut out = format!(
            "=== ДИАГНОСТИКА ПК | {} | права: {elevated} | ko-diag-tools v{} ===",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            super::VERSION,
        );
        for s in &self.sections {
            out.push_str(&format!("\n\n### {} [{}]\n{}", s.title, s.status_str(), s.body));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::RecorderConfig;
    use std::time::SystemTime;

    /// Тестовый каталог (§1.2: sandbox `ko_diag_tools_*`).
    fn sandbox(tag: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        std::env::temp_dir().join(format!("ko_diag_tools_{tag}_{nanos}"))
    }

    #[test]
    fn snapshot_has_all_sections_and_render_names_statuses() {
        // Живой запуск: PowerShell ×3 + реестр. Медленно, но это CORE-путь.
        let d = run_snapshot(None);
        assert!(d.sections.len() >= 9, "секций обязано быть ≥ 9: {}", d.sections.len());
        for s in &d.sections {
            assert!(!s.body.is_empty(), "секция «{}» пустая", s.title);
        }
        let rendered = d.render();
        assert!(rendered.contains("ДИАГНОСТИКА ПК"));
        assert!(rendered.contains("[OK]") || rendered.contains("[НАЙДЕНО]") || rendered.contains("[НЕ УДАЛОСЬ]"),
            "статусы обязаны быть в рендере:\n{rendered:.200}");
        assert!(rendered.len() < 60_000, "рендер обязан быть компактным: {} байт", rendered.len());
    }

    #[test]
    fn recorder_logs_section_lists_fresh_log() {
        let dir = sandbox("logs");
        std::fs::create_dir_all(&dir).unwrap();
        // Пустой каталог → честное «нет логов».
        let empty = recorder_logs_section(&dir);
        assert_eq!(empty.status_str(), "OK", "пусто = OK, не Failed:\n{}", empty.body);

        // Файл рекордера в каталоге → секция НАЙДЕНО с путём.
        let log = dir.join(format!("{}1.log", recorder::FILE_PREFIX));
        std::fs::write(&log, "tick").unwrap();
        let sec = recorder_logs_section(&dir);
        assert_eq!(sec.status_str(), "НАЙДЕНО", "лог должен найтись:\n{}", sec.body);
        assert!(sec.body.contains("diag_recorder_1.log"), "путь в теле:\n{}", sec.body);
    }

    #[test]
    fn recorder_logs_section_failed_on_unreadable_dir() {
        let dir = sandbox("unreadable");
        // Каталог не создаём: read_dir даст NotFound → честный Failed.
        let sec = recorder_logs_section(&dir);
        assert_eq!(sec.status_str(), "НЕ УДАЛОСЬ");
        assert!(sec.body.contains("не читается"), "причина в теле:\n{}", sec.body);
    }

    #[test]
    fn snapshot_with_diag_dir_mentions_recorder_section() {
        let dir = sandbox("with_logs");
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join(format!("{}1.log", recorder::FILE_PREFIX));
        std::fs::write(&log, "tick").unwrap();
        let d = run_snapshot(Some(&dir));
        assert!(
            d.sections.iter().any(|s| s.title.contains("ЛОГИ ЧЁРНОГО ЯЩИКА")),
            "секция рекордера обязана быть при переданном каталоге"
        );
    }

    /// Молчаливое дымовое: `RecorderConfig::default().interval` не ниже
    /// минимума sysinfo (защита от часовых редакций Default).
    #[test]
    fn default_config_is_legal() {
        assert!(RecorderConfig::default().interval >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    }
}

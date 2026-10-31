//! Скрытые дампы ядра: `C:\Windows\LiveKernelReports` (живые дампы драйверов
//! без BSOD — видеодрайвер часто не даёт полноценный синий экран),
//! `C:\Windows\Minidump` (дампы BSOD) и `C:\Windows\MEMORY.DMP` (полный).
//!
//! Права: системные каталоги доступны только администратору. Обычному
//! процессу `ReadDir` вернёт PermissionDenied — это ЧЕСТНЫЙ статус секции
//! «нет прав», а не «дампов нет» (§2.2).

use super::Section;
use chrono::Local;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

/// Свежесть дампа: старше 5 дней не считается наблюдением (система живёт).
const FRESHNESS_DAYS: u64 = 5;

/// Потолок строк в секции (сортировка по новизне, лишнее срезается).
const MAX_DUMP_ENTRIES: usize = 20;

/// Описание одного файла дампа для строки секции.
struct DumpEntry {
    path: String,
    size_kb: u64,
    modified_unix_secs: u64,
}

/// Свеж ли файл: новее порога FRESHNESS_DAYS.
fn fresh_enough(modified: SystemTime) -> bool {
    let now = Local::now().timestamp() as u64;
    let secs = modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(secs) < FRESHNESS_DAYS * 86_400
}

/// Рекурсивный список свежих файлов с потолком MAX_DUMP_ENTRIES.
fn collect_recent_files(
    dir: &Path,
    depth: u8,
    out: &mut Vec<DumpEntry>,
) -> Result<(), std::io::Error> {
    // Живые дампы лежат в подкаталогах (WATCHDOG, ...) — глубина ограничена.
    if depth > 3 {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            collect_recent_files(&path, depth + 1, out)?;
        } else if let Ok(mtime) = meta.modified() {
            if fresh_enough(mtime) {
                out.push(DumpEntry {
                    path: path.display().to_string(),
                    size_kb: meta.len() / 1024,
                    modified_unix_secs: mtime
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                });
            }
        }
    }
    Ok(())
}

/// Строки секции из списка: новые вверху, время в ISO (локальная зона).
///
/// `DateTime::<Utc>::from(SystemTime)` — стабильно живёт в chrono 0.4
/// (старейшая конверсия); `.with_timezone(&Local)` — локальная зона.
fn render_entries(mut entries: Vec<DumpEntry>) -> Vec<String> {
    entries.sort_by(|a, b| b.modified_unix_secs.cmp(&a.modified_unix_secs));
    entries
        .into_iter()
        .take(MAX_DUMP_ENTRIES)
        .map(|e| {
            let mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(e.modified_unix_secs);
            let time = chrono::DateTime::<chrono::Utc>::from(mtime)
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string();
            format!("{time} | {} КБ | {}", e.size_kb, e.path)
        })
        .collect()
}

/// Секции дампов: LiveKernelReports + Minidump + MEMORY.DMP. Отказ каждой
/// точки — свой текст, остальные точки продолжают работать.
pub fn dumps_sections() -> Vec<Section> {
    let mut out = Vec::new();

    for (dir, title) in [
        (
            Path::new(r"C:\Windows\LiveKernelReports"),
            "[9] ЖИВЫЕ ДАМПЫ ЯДРА ЗА 5 ДНЕЙ (LiveKernelReports)",
        ),
        (
            Path::new(r"C:\Windows\Minidump"),
            "[10] ДАМПЫ BSOD ЗА 5 ДНЕЙ (Minidump)",
        ),
    ] {
        let title = title.to_string();
        if !dir.exists() {
            out.push(Section::new_ok(
                title,
                "Каталог отсутствует — систему дампом этого типа пока не счётывало (штатно при отсутствии сбоев).".to_string(),
            ));
            continue;
        }
        let mut entries = Vec::new();
        match collect_recent_files(dir, 0, &mut entries) {
            Ok(()) if entries.is_empty() => {
                out.push(Section::new_ok(
                    title,
                    "Каталог существует, свежих (<5 дней) дампов нет.".to_string(),
                ));
            }
            Ok(()) => {
                let rendered = render_entries(entries);
                out.push(Section::new_findings(
                    title,
                    format!(
                        "Свежие дампы (<5 дней). Указанный путь отдаётся разбору:\n{}",
                        rendered.join("\n")
                    ),
                ));
            }
            Err(e) => {
                let reason = if e.kind() == std::io::ErrorKind::PermissionDenied {
                    format!("нет прав (нужен запуск King Orch от администратора): {e}")
                } else {
                    format!("каталог {} не читается: {e}", dir.display())
                };
                out.push(Section::new_failed(title, reason));
            }
        }
    }

    // Полный дамп памяти: один файл — достаточно метаданных.
    let full = Path::new(r"C:\Windows\MEMORY.DMP");
    let title = "[11] ПОЛНЫЙ ДАМП ПАМЯТИ (MEMORY.DMP)".to_string();
    match full.metadata() {
        Ok(meta) => {
            let age = match meta.modified() {
                Ok(mtime) if fresh_enough(mtime) => "свежий (<5 дней)".to_string(),
                Ok(_) => "старый (>5 дней)".to_string(),
                Err(_) => "время изменения неизвестно".to_string(),
            };
            out.push(Section::new_findings(
                title,
                format!(
                    "Есть полный дамп ({age}) размером {} МБ: {}.\nПри разборе сопоставляй с WHEA/BugCheck из журнала System.",
                    meta.len() / 1024 / 1024,
                    full.display()
                ),
            ));
        }
        Err(_) => {
            out.push(Section::new_ok(
                title,
                "Полного дампа нет (система настроена на Minidump или сбоев ядра не было).".to_string(),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_enough_accepts_now_and_rejects_epoch() {
        let now = SystemTime::now();
        assert!(fresh_enough(now), "сейчас = свежий");
        assert!(
            !fresh_enough(SystemTime::UNIX_EPOCH),
            "эпоха (1970) обязана считаться старой"
        );
    }

    #[test]
    fn freshness_threshold_is_5_days() {
        assert_eq!(FRESHNESS_DAYS, 5);
        assert_eq!(MAX_DUMP_ENTRIES, 20);
    }

    #[test]
    fn render_entries_sorts_newest_first() {
        let entries = vec![
            DumpEntry { path: "a".into(), size_kb: 1, modified_unix_secs: 1_000 },
            DumpEntry { path: "b".into(), size_kb: 2, modified_unix_secs: 2_000 },
        ];
        let rendered = render_entries(entries);
        assert_eq!(rendered.len(), 2, "оба выше потолка 20");
        assert!(rendered[0].contains("КБ | b"), "новый первым:\n{:?}", rendered);
        assert!(rendered[1].contains("КБ | a"), "старый вторым:\n{:?}", rendered);
        assert!(!rendered[0].starts_with(' '), "время обязано начинать строку:\n{:?}", rendered);
    }
}

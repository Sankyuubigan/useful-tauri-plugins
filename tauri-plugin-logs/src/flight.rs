//! Flight Recorder: кольцевой буфер последних записей + сброс в постоянный
//! файл `crash_dump.log` при ошибке/панике.
//!
//! В отличие от `king_orch.log` (truncate на старте сессии), crash-дамп живёт
//! МЕЖДУ запусками: если приложение упало и перезапустилось, логи сессии уже
//! обнулены, а дамп с контекстом сбоя остаётся (append, ротация ~1MB).
//!
//! Буфер наполняется из единого конвейера (`logger::write_regular`/`write_files`)
//! и из команд фронта (`add_breadcrumb`). Сброс происходит на `ERROR`, панике
//! и unhandledrejection — с полной хроникой того, что было перед сбоем.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

/// Ёмкость кольцевого буфера (последние записи сессии).
const BUFFER_CAPACITY: usize = 200;
/// Предел размера `crash_dump.log` до ротации.
const DUMP_MAX_BYTES: u64 = 1 * 1024 * 1024;
/// Не чаще одного дампа ошибки в этот интервал (дамп на панику — всегда).
const ERROR_DUMP_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

static BUFFER: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static DUMP_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static LAST_ERROR_DUMP: Mutex<Option<Instant>> = Mutex::new(None);

fn lock<T>(guard: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    guard.lock().unwrap_or_else(|p| p.into_inner())
}

/// Путь к файлу crash-дампа: рядом с exe (правило cwd), первый доступный каталог.
fn resolve_dump_path() -> PathBuf {
    crate::path::resolve_log_file("crash_dump.log")
}

/// Задать путь дампа (только для тестов; в рантайме резолвится автоматически).
#[cfg(test)]
pub fn set_dump_path(path: PathBuf) {
    *lock(&DUMP_PATH) = Some(path);
}

fn dump_path() -> PathBuf {
    lock(&DUMP_PATH)
        .as_ref()
        .cloned()
        .unwrap_or_else(resolve_dump_path)
}

/// Положить строку в кольцевой буфер (старьё вытесняется).
pub fn push(line: &str) {
    let mut buf = lock(&BUFFER);
    if buf.len() >= BUFFER_CAPACITY {
        buf.pop_front();
    }
    buf.push_back(line.to_string());
}

/// Снимок буфера (порядок: от самой старой записи к свежей).
pub fn snapshot() -> Vec<String> {
    lock(&BUFFER).iter().cloned().collect()
}

/// Очистить буфер (старт новой сессии flight-данных).
pub fn clear() {
    lock(&BUFFER).clear();
}

fn append_file(path: &PathBuf, content: &str) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(content.as_bytes());
    }
}

/// Ротация: если файл превысил лимит — переименовать в `<name>.old`,
/// текущий начинает с чистого состояния (append переживает рестарты).
fn rotate_if_needed(path: &PathBuf) {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size < DUMP_MAX_BYTES {
        return;
    }
    let old = path.with_extension("old");
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::rename(path, &old);
}

fn header(trigger: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "\n============== CRASH / ERROR DUMP ==============\n\
         Timestamp: {}\n\
         Trigger: {}\n\
         App version: {}\n\
         OS: {} | arch: {}\n",
        crate::logger::timestamp(),
        trigger,
        version,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Дамп в постоянный файл.
///
/// `detail` — строка-описание самого сбоя (сообщение паники/ошибки);
/// `extra_lines` — доп. хроника (например, буфер breadcrumbs фронта),
/// вставляется ПОСЛЕ общего буфера, непосредственно перед ошибкой.
/// Функция безопасна для вызова из panic-hook: только файловый I/O.
pub fn dump(trigger: &str, detail: Option<&str>, extra_lines: &[String]) {
    let path = dump_path();
    rotate_if_needed(&path);
    let buf = snapshot();
    let detail = detail.unwrap_or("");

    let mut body = header(trigger);
    body.push_str(&format!("Detail: {}\n", detail));
    body.push_str("-- кольцевой буфер (последние записи сессии) --\n");
    if buf.is_empty() {
        body.push_str("(пусто)\n");
    }
    for line in &buf {
        body.push_str(line);
        body.push('\n');
    }
    if !extra_lines.is_empty() {
        body.push_str("-- breadcrumbs фронта --\n");
        for line in extra_lines {
            body.push_str(line);
            body.push('\n');
        }
    }
    body.push_str("==============================================\n");
    append_file(&path, &body);
}

/// Дамп на ERROR (rate-limited) — подробный cтек/локация прилагается.
pub fn dump_error(location: &str, bt: &str) {
    let detail = if bt.is_empty() {
        location.to_string()
    } else {
        format!("{}\nBacktrace:\n{}", location, bt)
    };
    let now = Instant::now();
    {
        let mut last = lock(&LAST_ERROR_DUMP);
        if let Some(t) = last.as_ref() {
            if now.duration_since(*t) < ERROR_DUMP_MIN_INTERVAL {
                return;
            }
        }
        *last = Some(now);
    }
    dump("ERROR", Some(&detail), &[]);
}

/// Дамп на панику — без rate-limit (терминальное событие).
pub fn dump_panic(detail: &str) {
    dump("PANIC", Some(detail), &[]);
}

/// Дамп по отчёту с фронта (unhandledrejection / window.onerror).
/// В `extra_lines` фронт присылает свой буфер breadcrumbs.
pub fn dump_frontend(kind: &str, detail: &str, extra_lines: &[String]) {
    dump(kind, Some(detail), extra_lines);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Все тесты ниже работают с глобальным буфером/путём — сериализуем их.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn buffer_evicts_oldest() {
        let _g = lock(&TEST_LOCK);
        clear();
        for i in 0..(BUFFER_CAPACITY + 50) {
            push(&format!("line {}", i));
        }
        let snap = snapshot();
        assert_eq!(snap.len(), BUFFER_CAPACITY);
        assert_eq!(snap.first().map(|s| s.as_str()), Some("line 50"));
        assert_eq!(snap.last().map(|s| s.as_str()), Some(format!("line {}", BUFFER_CAPACITY + 49).as_str()));
    }

    #[test]
    fn rotation_moves_old_file_to_old_extension() {
        let _g = lock(&TEST_LOCK);
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("crash_dump.log");
        // «прошлый дамп» занимает больше лимита
        std::fs::write(&path, vec![b'x'; (DUMP_MAX_BYTES + 10) as usize]).unwrap();
        set_dump_path(path.clone());
        dump("TEST", Some("большой прошлый файл"), &[]);
        assert!(path.with_extension("old").exists(), "старый файл должен уехать в .old");
        assert!(path.exists(), "текущий файл создан заново");
    }

    #[test]
    fn dump_contains_trigger_detail_and_buffer() {
        let _g = lock(&TEST_LOCK);
        clear();
        push("до ошибки: работало");
        push("до ошибки: ещё работало");
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("crash_dump.log");
        set_dump_path(path.clone());
        dump("ERROR", Some("что-то сломалось"), &["[UI ACTION] клик".to_string()]);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Trigger: ERROR"));
        assert!(content.contains("Detail: что-то сломалось"));
        assert!(content.contains("до ошибки: работало"));
        assert!(content.contains("[UI ACTION] клик"));
    }
}
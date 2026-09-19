//! Единый кастомный `log::Log` (core rules §2.5), общий для всех проектов.
//!
//! Каждая запись:
//! 1. дублируется в stderr;
//! 2. дописывается в exe-файл лога (`king_orch.log`, если настроен; truncate на
//!    старте сессии) и в dev-зеркало `test/last_logs.txt` (если есть каталог `test/`);
//! 3. кладётся в кольцевой буфер flight-recorder (`crash_dump.log`);
//! 4. эмитится фронту как событие `logs:message` (вкладка «Логи»).
//!
//! Ранний pre-Tauri период пишется через `early_init` / `early_log` — краш-лог
//! живёт с первой миллисекунды, даже если приложение падает до запуска GUI.
//!
//! На `ERROR`/панике/необработанной ошибке фронта буфер сбрасывается в
//! постоянный `crash_dump.log` (см. `crate::flight`).

use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, Once, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};
use tauri::{AppHandle, Emitter};

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static FILE_LOCK: Mutex<()> = Mutex::new(());
static LOG_FILE: OnceLock<std::path::PathBuf> = OnceLock::new();
static LAST_LOGS: OnceLock<std::path::PathBuf> = OnceLock::new();
static PANIC_HOOK_INSTALLED: Once = Once::new();

/// Читаемый локальный таймстамп `YYYY-MM-DD HH:MM:SS`.
pub fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn lock<T>(guard: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    guard.lock().unwrap_or_else(|p| p.into_inner())
}

fn append_file(file: &Path, line: &str) {
    if let Some(parent) = file.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Открыть файл начисто (create + truncate). Каждый файл лога начинается с
/// чистого состояния на старте сессии (core rules §2.5.1).
fn fresh_file(path: &Path) {
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path);
}

/// Запись только в файлы (без stderr и события) — для panic-hook, чтобы не
/// рисковать вторичной паникой внутри уже паникующего потока.
fn write_files(level: &str, msg: &str) {
    let line = format!("[{}] [{}] {}\n", timestamp(), level, msg);
    let _guard = lock(&FILE_LOCK);
    if let Some(path) = LAST_LOGS.get() {
        append_file(path, &line);
    }
    if let Some(path) = LOG_FILE.get() {
        append_file(path, &line);
    }
    crate::flight::push(&line);
}

/// Полная запись: stderr + файлы + кольцевой буфер + событие `logs:message`.
pub fn write_regular(level: &str, msg: &str) {
    let line = format!("[{}] [{}] {}", timestamp(), level, msg);
    eprintln!("{line}");
    {
        let _guard = lock(&FILE_LOCK);
        if let Some(path) = LAST_LOGS.get() {
            append_file(path, &format!("{line}\n"));
        }
        if let Some(path) = LOG_FILE.get() {
            append_file(path, &format!("{line}\n"));
        }
        crate::flight::push(&format!("{line}\n"));
    }
    if let Some(app) = APP_HANDLE.get() {
        let _ = app.emit("logs:message", line);
    }
}

pub fn log_file_path() -> Option<std::path::PathBuf> {
    LOG_FILE.get().cloned()
}

pub fn last_logs_path() -> Option<std::path::PathBuf> {
    LAST_LOGS.get().cloned()
}

/// Exe-файл лога (`king_orch.log`, если настроен): настраиваем ОДИН раз. Каждый
/// запуск начинается с чистого файла (truncate), как и `test/last_logs.txt`
/// (core rules §2.5.1) — исключается неконтролируемый рост между сессиями.
fn ensure_log_file(name: &str) {
    if LOG_FILE.get().is_none() {
        let path = crate::path::resolve_log_file(name);
        if LOG_FILE.set(path.clone()).is_ok() {
            fresh_file(&path);
        }
    }
}

/// Dev-зеркало `test/last_logs.txt`: настраиваем ОДИН раз (truncate в начале
/// сессии, дальше только append). Каталог `test/` обязан уже существовать.
fn ensure_last_logs() {
    if LAST_LOGS.get().is_some() {
        return;
    }
    if let Some(path) = crate::path::resolve_last_logs_file() {
        if LAST_LOGS.set(path.clone()).is_ok() {
            fresh_file(&path);
        }
    }
}

pub struct AppLogger;

static LOGGER: AppLogger = AppLogger;

impl Log for AppLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let msg = record.args().to_string();
        write_regular(record.level().as_str(), &msg);
        if record.level() == Level::Error {
            // Дамп контекста в постоянной crash_dump.log (переживает рестарт)
            // + отправка в облако (если включено). Стек извлекаем на месте —
            // единственная здоровая точка захвата (ошибки редки, ловим все).
            let location = record
                .file()
                .map(|f| {
                    record
                        .line()
                        .map(|l| format!("{}:{}", f, l))
                        .unwrap_or_else(|| f.to_string())
                })
                .unwrap_or_else(|| "<нет файла:строка>".to_string());
            let bt = capture_backtrace();
            crate::flight::dump_error(&format!("{} ({})", location, record.target()), &bt);
            crate::error_reporting::report_handled(record.target(), &msg);
        }
    }

    fn flush(&self) {}
}

/// Захват стектрейса ошибки. Возвращает пустую строку, если символизация
/// недоступна (например, RUST_BACKTRACE не дал адресов) — дамп всё равно пишется.
fn capture_backtrace() -> String {
    if std::env::var("RUST_BACKTRACE").map(|v| v == "0").unwrap_or(true) {
        return String::new();
    }
    let bt = std::backtrace::Backtrace::capture();
    if bt.status() != std::backtrace::BacktraceStatus::Captured {
        return String::new();
    }
    // Не раздуваем дамп: первые 12 фреймов достаточно, чтобы найти виновника.
    let text = format!("{bt}");
    let lines: Vec<&str> = text.lines().take(12).collect();
    lines.join("\n")
}

fn install_logger() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}

/// Ранняя (pre-Tauri) инициализация — вызывается в самом начале `main()` хоста,
/// чтобы краш-лог существовал до запуска Tauri:
/// ```no_run
/// tauri_plugin_logs::early_init("king_orch.log");
/// tauri_plugin_logs::early_log("INFO", "=== запуск ===");
/// ```
pub fn early_init(log_file_name: &str) {
    // Стектрейсы на ERROR-путях — с первой секунды (см. AppLogger::log).
    std::env::set_var("RUST_BACKTRACE", "1");
    ensure_log_file(log_file_name);
    ensure_last_logs();
    crate::flight::clear();
    install_panic_hook();
    write_files("INFO", &format!("=== старт процесса (pre-Tauri) ==="));
}

/// Ранняя запись строки в файлы (только files + stderr, без события — GUI ещё нет).
pub fn early_log(level: &str, msg: &str) {
    let line = format!("[{}] [{}] {}", timestamp(), level, msg);
    eprintln!("{line}");
    write_files(level, msg);
}

/// Установка логгера и путей в setup плагина. Повторные вызовы безвредны.
pub fn install(app: &AppHandle, last_logs: bool, log_file_name: Option<&str>) {
    let _ = APP_HANDLE.set(app.clone());
    if last_logs {
        ensure_last_logs();
    }
    if let Some(name) = log_file_name {
        ensure_log_file(name);
    }
    install_panic_hook();
    install_logger();
}

pub fn install_panic_hook() {
    PANIC_HOOK_INSTALLED.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let msg = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "неизвестная паника".to_string());
            let loc = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_default();
            write_files("PANIC", &format!("{} | {}", msg, loc));
            crate::flight::dump_panic(&format!("{} | {}", msg, loc));
            crate::error_reporting::report_fatal("Backend Panic", &msg, &loc);
            default_hook(info);
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_format_is_local_yyyymmdd_hhmmss() {
        let ts = timestamp();
        assert!(ts.len() >= 19, "формат YYYY-MM-DD HH:MM:SS, получено: {}", ts);
        assert!(ts.chars().nth(4) == Some('-') && ts.chars().nth(7) == Some('-'), "дата YYYY-MM-DD");
        assert!(ts.chars().nth(10) == Some(' '), "пробел между датой и временем");
        assert!(ts.chars().nth(13) == Some(':') && ts.chars().nth(16) == Some(':'), "время HH:MM:SS");
        let secs = chrono::Local::now();
        assert_eq!(
            format!("{}", secs.format("%Y-%m-%d %H:%M:%S")),
            ts,
            "совпадает с chrono-форматом локального времени"
        );
    }

    #[test]
    fn append_line_writes_timestamped_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("log.txt");
        append_file(&file, "some line\n");
        append_file(&file, "another line\n");
        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "some line\nanother line\n");
    }

    #[test]
    fn fresh_file_starts_session_with_empty_log() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("king_orch.log");
        // «прошлая сессия» уже что-то записала в файл
        append_file(&file, "old session line\n");
        append_file(&file, "another old line\n");
        assert!(std::fs::metadata(&file).unwrap().len() > 0);

        // новый запуск — файл обнуляется (create+truncate), дальше append
        fresh_file(&file);
        assert_eq!(std::fs::metadata(&file).unwrap().len(), 0, "старт сессии обязан обнулить файл");

        append_file(&file, "current session\n");
        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "current session\n");
    }
}
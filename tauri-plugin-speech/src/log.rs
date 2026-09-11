use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Runtime};

/// Сериализует запись в `last_logs`: в файл пишут два потока одновременно
/// (stderr-поток движка и основной поток ошибок), и без блокировки их записи
/// перемешивались, терялся `\n` и строки склеивались.
static LOG_MUTEX: Mutex<()> = Mutex::new(());

/// Вычисляет путь к файлу `test/last_logs`.
pub fn last_logs_path() -> PathBuf {
    let start = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut dir = start;
    loop {
        if dir.join("test").is_dir() {
            return dir.join("test").join("last_logs.txt");
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => break,
        }
    }

    let fallback = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("test");
    let _ = std::fs::create_dir_all(&fallback);
    fallback.join("last_logs.txt")
}

/// Очищает (truncate) файл last_logs при старте сессии.
pub fn truncate_last_logs() {
    let path = last_logs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, "");
}

/// Метка времени события для каждой строки лога.
fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Единая точка записи логов: метка времени + stderr + UI-событие `app-log` + файл.
pub fn app_log<R: Runtime>(app: &AppHandle<R>, msg: &str) {
    let line = format!("[{}] {}", timestamp(), msg);
    eprintln!("{line}");
    let _ = app.emit("app-log", line.clone());
    if let Err(e) = write_to_file(&line) {
        eprintln!("[logger] не удалось записать last_logs: {e}");
    }
}

fn write_to_file(msg: &str) -> std::io::Result<()> {
    let _guard = LOG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path = last_logs_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{msg}")?;
    Ok(())
}
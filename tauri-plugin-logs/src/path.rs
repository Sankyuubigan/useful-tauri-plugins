//! Резолв путей плагина.
//!
//! - Лог-файл рядом с exe (`king_orch.log`): fallback CWD → temp (чтобы юзер мог
//!   скинуть файл, даже если exe не имеет прав на запись — Program Files).
//! - Dev-зеркало `test/last_logs.txt`: пишется ТОЛЬКО если существует каталог
//!   `test/` (dev-комплект). Ищем в CWD, затем от каталога exe вверх.

use std::path::PathBuf;

pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Резолв файла лога рядом с exe. Первый путь, который удалось открыть на
/// дозапись (создавая файл при необходимости), становится итоговым.
pub fn resolve_log_file(name: &str) -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = exe_dir() {
        candidates.push(dir.join(name));
    }
    if let Ok(cwd) = std::env::current_dir() {
        let p = cwd.join(name);
        if !candidates.contains(&p) {
            candidates.push(p);
        }
    }
    let temp = std::env::temp_dir().join(name);
    if !candidates.contains(&temp) {
        candidates.push(temp);
    }

    candidates
        .into_iter()
        .find(|p| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .is_ok()
        })
        .unwrap_or_else(|| std::env::temp_dir().join(name))
}

/// Dev-зеркало: `test/last_logs.txt` ТОЛЬКО если существует каталог `test/`
/// (CWD первым, затем подъём от каталога exe). Иначе `None`.
pub fn resolve_last_logs_file() -> Option<PathBuf> {
    if let Ok(cwd) = std::env::current_dir() {
        let dir = cwd.join("test");
        if dir.is_dir() {
            return Some(dir.join("last_logs.txt"));
        }
    }
    let mut dir = exe_dir()?;
    loop {
        let candidate = dir.join("test");
        if candidate.is_dir() {
            return Some(candidate.join("last_logs.txt"));
        }
        if !dir.pop() {
            break;
        }
    }
    None
}


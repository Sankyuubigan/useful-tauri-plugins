//! Уровень 2: curl (libcurl crate) — запасной надёжный бэкенд.
//! low-speed abort (stall), resume, follow redirects, прогресс.
//! Нет системных зависимостей: libcurl вендорится крейтом.

use super::progress::TaskHandle;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

/// Если скорость < MIN_SPEED_BPS дольше LOW_SPEED_SECS — libcurl оборвёт сам.
const MIN_SPEED_BPS: u32 = 1024;
const LOW_SPEED_SECS: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT_SECS: u64 = 15;
const MAX_SECONDS: u64 = 2 * 3600;

pub fn download(task: &TaskHandle, url: &str, dest: &Path) -> Result<(u64, u64), String> {
    let part = super::reqwest_dl::part_path(dest);
    let resume_from: u64 = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    if resume_from == 0 {
        if let Ok(meta) = std::fs::metadata(dest) {
            if meta.len() > 0 {
                return Ok((meta.len(), meta.len()));
            }
        }
    }

    let mut easy = curl::easy::Easy::new();
    easy.url(url).map_err(|e| format!("curl url: {}", e))?;
    easy.follow_location(true)
        .map_err(|e| format!("curl follow: {}", e))?;
    easy.connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .map_err(|e| format!("curl connect_timeout: {}", e))?;
    easy.low_speed_limit(MIN_SPEED_BPS)
        .map_err(|e| format!("curl low_speed_limit: {}", e))?;
    easy.low_speed_time(LOW_SPEED_SECS)
        .map_err(|e| format!("curl low_speed_time: {}", e))?;
    easy.timeout(Duration::from_secs(MAX_SECONDS))
        .map_err(|e| format!("curl timeout: {}", e))?;
    easy.useragent("king-orch-app/1.0")
        .map_err(|e| format!("curl ua: {}", e))?;
    easy.progress(true)
        .map_err(|e| format!("curl progress: {}", e))?;
    if resume_from > 0 {
        easy.resume_from(resume_from)
            .map_err(|e| format!("curl resume: {}", e))?;
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resume_from > 0)
        .truncate(resume_from == 0)
        .open(&part)
        .map_err(|e| format!("Ошибка файла: {}", e))?;

    let mut state = State {
        total_est: 0,
        last_bytes: resume_from,
        last_t: Instant::now(),
    };

    let mut transfer = easy.transfer();

    transfer
        .write_function(|data| match file.write_all(data) {
            Ok(()) => Ok(data.len()),
            Err(_) => Ok(0),
        })
        .map_err(|e| format!("curl write_fn: {}", e))?;

    transfer
        .progress_function(|dltotal, dlnow, _utotal, _ulnow| {
            if task.is_cancelled() {
                return false;
            }
            let now = Instant::now();
            if dltotal > 0.0 && state.total_est == 0 {
                state.total_est = dltotal as u64;
            }
            let dt = now.duration_since(state.last_t).as_secs_f64();
            if dt >= 0.2 {
                let cur = dlnow.max(0.0) as u64;
                let speed = (cur.saturating_sub(state.last_bytes)) as f64 / dt;
                let total = if state.total_est > 0 {
                    state.total_est
                } else if dltotal > 0.0 {
                    dltotal as u64
                } else {
                    0
                };
                task.emit_progress(cur, total, speed, "curl", false);
                state.last_t = now;
                state.last_bytes = cur;
            }
            true
        })
        .map_err(|e| format!("curl progress_fn: {}", e))?;

    transfer
        .perform()
        .map_err(|e| format!("curl perform: {}", e))?;
    drop(transfer);

    let code = easy
        .response_code()
        .map_err(|e| format!("curl code: {}", e))?;
    if code >= 400 {
        let _ = std::fs::remove_file(&part);
        return Err(format!("curl HTTP {}", code));
    }

    if task.is_cancelled() {
        let _ = std::fs::remove_file(&part);
        return Err("Отменено".into());
    }

    drop(file);
    let meta = std::fs::metadata(&part).map_err(|e| format!("Нет скачанного файла: {}", e))?;
    if meta.len() == 0 {
        let _ = std::fs::remove_file(&part);
        return Err("curl скачал 0 байт".into());
    }
    std::fs::rename(&part, dest).map_err(|e| format!("Ошибка переименования: {}", e))?;
    let len = meta.len();
    task.emit_progress(len, len, 0.0, "curl done", true);
    Ok((len, len))
}

struct State {
    total_est: u64,
    last_bytes: u64,
    last_t: Instant,
}

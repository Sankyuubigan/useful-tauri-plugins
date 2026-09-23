//! Реестр активных загрузок: task_id, отмена, эмит событий прогресса.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

/// Имя единого события прогресса (слушают все UI-компоненты).
pub const PROGRESS_EVENT: &str = "downloader:progress";

#[derive(Clone, Debug, Serialize)]
pub struct ProgressPayload {
    pub task_id: String,
    /// Человекочитаемая метка: «Движок llama.cpp», «Модель», …
    pub label: String,
    /// Логическая категория: "engine" | "model" | "mmproj" | "app" | "bin" | "9router" | "file"
    pub kind: String,
    pub url: String,
    pub dest: String,
    pub downloaded: u64,
    pub total: u64,
    pub speed_bps: f64,
    /// Оценка оставшегося времени, сек; < 0 — неизвестно.
    pub eta_s: f64,
    /// Текущий уровень цепочки (1..=6).
    pub level: u32,
    pub level_name: String,
    pub status: String, // "running" | "done" | "error" | "cancelled"
    pub message: String,
}

struct TaskEntry {
    cancel: Arc<AtomicBool>,
    /// PID внешнего процесса (уровни 3..=6), если запущен.
    child_pid: Mutex<Option<u32>>,
    started: Instant,
    label: String,
    kind: String,
    url: String,
    dest: String,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static TASKS: OnceLock<Mutex<HashMap<String, TaskEntry>>> = OnceLock::new();

fn tasks() -> &'static Mutex<HashMap<String, TaskEntry>> {
    TASKS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn set_app(app: AppHandle) {
    let _ = APP.set(app);
}

fn app() -> Option<&'static AppHandle> {
    APP.get()
}

/// Доступ к AppHandle для эмита из других модулей (reqwest-репортер и т.п.).
pub fn app_handle() -> Option<&'static AppHandle> {
    APP.get()
}

/// Глобальный счётчик task_id.
static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct TaskHandle {
    pub id: String,
    pub cancel: Arc<AtomicBool>,
    pub label: String,
    pub kind: String,
    pub url: String,
    pub dest: String,
    pub started: Instant,
    level: std::sync::atomic::AtomicU32,
    level_name: Mutex<String>,
    last_bytes: Mutex<u64>,
    last_emit: Mutex<Instant>,
}

impl TaskHandle {
    pub fn new(label: &str, kind: &str, url: &str, dest: &str) -> Self {
        let id = format!("dl_{}", COUNTER.fetch_add(1, Ordering::Relaxed));
        let cancel = Arc::new(AtomicBool::new(false));
        let entry = TaskEntry {
            cancel: cancel.clone(),
            child_pid: Mutex::new(None),
            started: Instant::now(),
            label: label.to_string(),
            kind: kind.to_string(),
            url: url.to_string(),
            dest: dest.to_string(),
        };
        tasks().lock().unwrap().insert(id.clone(), entry);
        Self {
            id,
            cancel,
            label: label.to_string(),
            kind: kind.to_string(),
            url: url.to_string(),
            dest: dest.to_string(),
            started: Instant::now(),
            level: std::sync::atomic::AtomicU32::new(0),
            level_name: Mutex::new(String::new()),
            last_bytes: Mutex::new(0),
            last_emit: Mutex::new(Instant::now() - std::time::Duration::from_secs(1)),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn check_cancel(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err("Отменено".to_string())
        } else {
            Ok(())
        }
    }

    pub fn set_level(&self, level: u32, name: &str) {
        self.level.store(level, Ordering::Relaxed);
        *self.level_name.lock().unwrap() = name.to_string();
    }

    pub fn level(&self) -> u32 {
        self.level.load(Ordering::Relaxed)
    }

    pub fn set_child_pid(&self, pid: u32) {
        if let Some(entry) = tasks().lock().unwrap().get(&self.id) {
            *entry.child_pid.lock().unwrap() = Some(pid);
        }
    }

    /// Эмит прогресса. `force=false` — троттлинг до ~5/сек, `force=true` — всегда
    /// (для terminal-статусов done/error/cancelled).
    pub fn emit_progress(&self, downloaded: u64, total: u64, speed_bps: f64, message: &str, force: bool) {
        if !force {
            let mut last = self.last_emit.lock().unwrap();
            if last.elapsed() < std::time::Duration::from_millis(200) {
                return;
            }
            *last = Instant::now();
        }
        *self.last_bytes.lock().unwrap() = downloaded;
        let eta = if speed_bps > 1.0 && total > downloaded {
            ((total - downloaded) as f64 / speed_bps).round()
        } else {
            -1.0
        };
        let payload = ProgressPayload {
            task_id: self.id.clone(),
            label: self.label.clone(),
            kind: self.kind.clone(),
            url: self.url.clone(),
            dest: self.dest.clone(),
            downloaded,
            total,
            speed_bps,
            eta_s: eta,
            level: self.level(),
            level_name: self.level_name.lock().unwrap().clone(),
            status: "running".into(),
            message: message.to_string(),
        };
        if let Some(app) = app() {
            let _ = app.emit(PROGRESS_EVENT, payload);
        }
    }

    pub fn emit_terminal(&self, status: &str, message: &str, downloaded: u64, total: u64) {
        let payload = ProgressPayload {
            task_id: self.id.clone(),
            label: self.label.clone(),
            kind: self.kind.clone(),
            url: self.url.clone(),
            dest: self.dest.clone(),
            downloaded,
            total,
            speed_bps: 0.0,
            eta_s: -1.0,
            level: self.level(),
            level_name: self.level_name.lock().unwrap().clone(),
            status: status.into(),
            message: message.to_string(),
        };
        if let Some(app) = app() {
            let _ = app.emit(PROGRESS_EVENT, payload);
        }
    }
}

impl Drop for TaskHandle {
    fn drop(&mut self) {
        tasks().lock().unwrap().remove(&self.id);
    }
}

/// Отменить конкретную задачу.
pub fn cancel(task_id: &str) -> bool {
    let map = tasks().lock().unwrap();
    if let Some(entry) = map.get(task_id) {
        entry.cancel.store(true, Ordering::SeqCst);
        if let Some(pid) = *entry.child_pid.lock().unwrap() {
            kill_pid(pid);
        }
        true
    } else {
        false
    }
}

/// Отменить все активные (вызывается при ExitRequested).
pub fn cancel_all() {
    let map = tasks().lock().unwrap();
    for entry in map.values() {
        entry.cancel.store(true, Ordering::SeqCst);
        if let Some(pid) = *entry.child_pid.lock().unwrap() {
            kill_pid(pid);
        }
    }
}

/// Снимок активных задач для команды list_active.
pub fn list_active() -> Vec<serde_json::Value> {
    let map = tasks().lock().unwrap();
    map.iter()
        .map(|(id, e)| {
            serde_json::json!({
                "task_id": id,
                "label": e.label,
                "kind": e.kind,
                "url": e.url,
                "dest": e.dest,
                "elapsed_ms": e.started.elapsed().as_millis() as u64,
                "cancelled": e.cancel.load(Ordering::SeqCst),
            })
        })
        .collect()
}

#[cfg(windows)]
fn kill_pid(pid: u32) {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let _ = cmd.output();
}

#[cfg(not(windows))]
fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).output();
}

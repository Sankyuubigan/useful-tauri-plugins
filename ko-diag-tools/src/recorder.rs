//! «Чёрный ящик» (flight recorder): фоновый мониторинг с принудительным
//! сбросом каждой строки на физический носитель.
//!
//! Зачем это существует: при намертво зависшей системе обычные логи
//! умирают вместе с кэшем записи ОС. Каждая строка здесь пишется
//! через `sync_all()` — минуя кэш, сразу на носитель. После перезагрузки
//! остаётся последняя строка за долю секунды до смерти системы.
//!
//! Что следит (универсально, без имён конкретного софта):
//! - любой НОВЫЙ процесс (детектор спавна: игры, античиты, фоновые
//!   службы — хоть скрытый майнер);
//! - средняя загрузка ЦП, ОЗУ, своп;
//! - топ-1 по ЦП и топ-1 по ОЗУ.
//!
//! Хост-контракт: хост передаёт только каталог. Файл —
//! `diag_recorder_<N>.log` (N = 1..∞, вычисляется по содержимому каталога:
//! скрытого счётчика нет, SSOT — файлы на диске).
//!
//! Лимиты: авто-останов по времени (`max_duration`) или по размеру файла
//! (`max_file_bytes`) — рекордер не может остаться «забытым навечно»;
//! причина авто-остановки возвращается в [`StopSummary`].

use chrono::Local;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use sysinfo::System;

/// Префикс файлов рекордера (SSOT: имя образует файл на диске).
pub const FILE_PREFIX: &str = "diag_recorder_";

/// Параметры рекордера (Default — боевые значения).
#[derive(Debug, Clone)]
pub struct RecorderConfig {
    /// Интервал наблюдения. Простой отказ при < sysinfo::MINIMUM_CPU_UPDATE_INTERVAL:
    /// ниже минимума статистика ЦП перестаёт обновляться и честно давала бы
    /// нули, если бы sysinfo молчал — молчание запрещено, поэтому отказ.
    pub interval: Duration,
    /// Максимальное время записи (авто-остановка).
    pub max_duration: Duration,
    /// Максимальный размер файла (авто-остановка). 10 МБ текста —
    /// несколько часов честных тиков.
    pub max_file_bytes: u64,
    /// Хвост для StopSummary (последние N строк — почти момент смерти).
    pub tail_lines: usize,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        RecorderConfig {
            interval: Duration::from_millis(400),
            max_duration: Duration::from_secs(3600), // 60 мин
            max_file_bytes: 10 * 1024 * 1024,        // 10 МБ
            tail_lines: 40,
        }
    }
}

/// Управление живущим рекордером: остановка + чтение состояния.
pub struct RecorderControl {
    stopped: Arc<AtomicBool>,
    ticks: Arc<AtomicU64>,
    write_error: Arc<Mutex<Option<String>>>,
    auto_stopped: Arc<Mutex<Option<String>>>,
    path: PathBuf,
    started_at: chrono::DateTime<Local>,
    join: Option<JoinHandle<()>>,
}

impl RecorderControl {
    /// Путь лог-файла (путь остаётся у юзера даже после зависания машины).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Начало записи (тайминг для отчёта).
    pub fn started_at(&self) -> chrono::DateTime<Local> {
        self.started_at
    }

    /// Жив ли рекордер (иначе — умер сам: авто-лимит/ошибка записи).
    pub fn is_running(&self) -> bool {
        !self.stopped.load(Ordering::Relaxed)
    }
}

/// Итог остановки.
#[derive(Debug)]
pub struct StopSummary {
    /// Путь к полному логу (остаётся у юзера после зависания машины).
    pub path: PathBuf,
    /// Начало записи.
    pub started_at: chrono::DateTime<Local>,
    /// Сколько тиков записано (маловато = рекордер умер responsable).
    pub ticks: u64,
    /// Последние N строк (самое интересное перед обрывом).
    pub tail: Vec<String>,
    /// Ошибка записи (если файл перестал писаться — причина здесь).
    pub write_error: Option<String>,
    /// Причина авто-остановки (время/размер), если сработала.
    pub auto_stopped: Option<String>,
}

/// Записать строку и сбросить её на физический носитель.
/// `sync_all` — причина существования рекордера: без него последняя строка
/// при зависании остаётся в кэше ОС и умирает вместе с машиной.
fn write_synced(file: &mut File, line: &str) -> Result<(), std::io::Error> {
    writeln!(file, "{line}")?;
    file.sync_all()
}

/// Следующий файл: `diag_recorder_<N+1>.log`, N+1 = макс+1 по каталогу.
///
/// Никаких скрытых счётчиков: SSOT — содержимое каталога. Паранойя против
/// потерь имени при перезапуске хоста.
fn next_recorder_file(dir: &Path) -> Result<PathBuf, String> {
    let mut max = 0u64;
    let dir_read =
        std::fs::read_dir(dir).map_err(|e| format!("каталог {} не читается: {e}", dir.display()))?;
    for entry in dir_read {
        let entry = entry.map_err(|e| format!("запись каталога не читается: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(rest) = name.strip_prefix(FILE_PREFIX) {
            if let Some(num) = rest.strip_suffix(".log") {
                if let Ok(n) = num.parse::<u64>() {
                    if n > max {
                        max = n;
                    }
                }
            }
        }
    }
    Ok(dir.join(format!("{FILE_PREFIX}{}.log", max + 1)))
}

/// Новый рекордер: создаёт каталог, выбирает файл, запускает поток.
///
/// «Двойной запуск» одной сессии — забота хост-адаптера: ядро не ведёт
/// реестра (host-side state), а хост может перезапускать рекордер,
/// останавливая старый поток ДО этого вызова.
pub fn start_recording(dir: &Path, cfg: RecorderConfig) -> Result<RecorderControl, String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("создание каталога рекордера {}: {e}", dir.display()))?;
    if cfg.interval < sysinfo::MINIMUM_CPU_UPDATE_INTERVAL {
        return Err(format!(
            "интервал рекордера {} мс ниже минимума sysinfo {} мс — \
             статистика ЦП перестанет обновляться; поднимай интервал до 200 мс",
            cfg.interval.as_millis(),
            sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.as_millis()
        ));
    }
    let path = next_recorder_file(dir)?;

    let stopped = Arc::new(AtomicBool::new(false));
    let ticks = Arc::new(AtomicU64::new(0));
    let write_error = Arc::new(Mutex::new(None));
    let auto_stopped = Arc::new(Mutex::new(None));
    let started_at = Local::now();

    let th_stopped = stopped.clone();
    let th_ticks = ticks.clone();
    let th_write_error = write_error.clone();
    let th_auto_stopped = auto_stopped.clone();
    let th_path = path.clone();
    let th_cfg = cfg.clone();
    let join = thread::spawn(move || {
        record_loop(th_path, th_cfg, th_stopped, th_ticks, th_write_error, th_auto_stopped)
    });

    log::info!(
        "diag: рекордер запущен, файл {}; интервал {} мс",
        path.display(),
        cfg.interval.as_millis()
    );

    Ok(RecorderControl {
        stopped,
        ticks,
        write_error,
        auto_stopped,
        path,
        started_at,
        join: Some(join),
    })
}

/// Остановить рекордер и получить сводку (флаг → join → чтение хвоста).
pub fn stop_recording(mut control: RecorderControl, cfg: &RecorderConfig) -> StopSummary {
    control.stopped.store(true, Ordering::Relaxed);
    // Цикл добирает текущую итерацию и выходит; join здесь конечен.
    if let Some(handle) = control.join.take() {
        if handle.join().is_err() {
            // Паника потока: не молчим (§2.2), но сводку всё равно собираем.
            log::error!("diag: поток рекордера panicked");
        }
    }
    let ticks = control.ticks.load(Ordering::Relaxed);
    let write_error = control.write_error.lock().unwrap().take();
    let auto_stopped = control.auto_stopped.lock().unwrap().take();
    let tail = read_tail(&control.path, cfg.tail_lines);
    log::info!(
        "diag: рекордер остановлен; тиков: {}; файл {}",
        ticks,
        control.path.display()
    );
    StopSummary {
        path: control.path.clone(),
        started_at: control.started_at,
        ticks,
        tail,
        write_error,
        auto_stopped,
    }
}

/// Последние N строк файла (заполнение края кольца хвоста).
fn read_tail(path: &Path, tail_lines: usize) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return vec![];
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(tail_lines);
    lines[start..].iter().map(|l| l.to_string()).collect()
}

/// Публичное чтение хвоста произвольного лог-файла:StoppedSummary отдается
/// хвост рекордера этой жизни; при «после перезагрузки» хост читает хвост
/// файла прошлой жизни через этот хелпер (latest_log → read_log_tail).
/// Файл не читается → пустой хвост (хост сообщит причину сам по fs-ошибке).
pub fn read_log_tail(path: &Path, tail_lines: usize) -> Vec<String> {
    read_tail(path, tail_lines)
}

/// Свежий лог рекординга каталога (для режима «после зависания» — когда
/// хост перезапущен и не знает рекордера): макс. время изменения среди
/// `diag_recorder_*.log`. Ничего нет → `None`.
pub fn latest_log(dir: &Path) -> Option<PathBuf> {
    let dir_read = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for entry in dir_read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(FILE_PREFIX) || !name.ends_with(".log") {
            continue;
        }
        let mtime = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue, // узкая запись без меты — пропускаем честно
        };
        if best.as_ref().map_or(true, |(_, t)| mtime > *t) {
            best = Some((entry.path(), mtime));
        }
    }
    best.map(|(p, _)| p)
}

// ─────────────────────────── Тело рекордера ───────────────────────────

fn record_loop(
    path: PathBuf,
    cfg: RecorderConfig,
    stopped_flag: Arc<AtomicBool>,
    ticks: Arc<AtomicU64>,
    write_error_slot: Arc<Mutex<Option<String>>>,
    auto_stopped_slot: Arc<Mutex<Option<String>>>,
) {
    let mut file = match OpenOptions::new().create(true).write(true).truncate(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            *write_error_slot.lock().unwrap() = Some(format!("файл рекордера не открыт: {e}"));
            stopped_flag.store(true, Ordering::Relaxed);
            return;
        }
    };
    // Локальный хелпер: ошибка записи → слот + флаг остановки (секция §2.2).
    let fail_write = |msg: String| {
        *write_error_slot.lock().unwrap() = Some(msg);
        stopped_flag.store(true, Ordering::Relaxed);
    };

    // Шапка: интервал и формула ЦП (анализатор обязан знать шкалу).
    let header = format!(
        "=== ДИАГНОСТИЧЕСКИЙ РЕКОРДЕР, интервал {} мс ===\n\
         Шкала ЦП: «% машины» = sysinfo usage (процент одного ядра) / число лог. ядер.\n\
         ===",
        cfg.interval.as_millis()
    );
    if write_synced(&mut file, &header).is_err() {
        fail_write("шапка лога не записана".to_string());
        return;
    }

    let mut sys = System::new_all();
    let n_cpus = sys.cpus().len().max(1);
    let mut known_pids: HashSet<u32> = sys.processes().keys().map(|p| p.as_u32()).collect();
    let started = Instant::now();

    loop {
        if stopped_flag.load(Ordering::Relaxed) {
            break;
        }
        thread::sleep(cfg.interval);

        sys.refresh_memory();
        sys.refresh_cpu();
        sys.refresh_processes();

        let now = Local::now().format("%H:%M:%S%.3f");
        let avg_cpu: f32 = sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / n_cpus as f32;
        let ram = sys.used_memory() as f64 / 1_073_741_824.0;
        let ram_total = sys.total_memory() as f64 / 1_073_741_824.0;
        let swap = sys.used_swap() as f64 / 1_073_741_824.0;
        let swap_total = sys.total_swap() as f64 / 1_073_741_824.0;

        // Детектор спавна: новый PID → отдельная строка. Последняя строка
        // перед «смертью машины» очень часто оказывается именно она.
        let mut new_lines: Vec<String> = Vec::new();
        let mut current: HashSet<u32> = HashSet::with_capacity(known_pids.len());
        for p in sys.processes().values() {
            let pid = p.pid().as_u32();
            current.insert(pid);
            if !known_pids.contains(&pid) {
                new_lines.push(format!(
                    "[{now}] + НОВЫЙ ПРОЦЕСС: {} (PID {pid}, {} МБ)",
                    p.name().trim(),
                    p.memory() / 1_048_576
                ));
            }
        }
        known_pids = current;

        // Топ-1 по ЦП (в долях от всей машины) и по ОЗУ — имена виновников.
        let top_cpu = sys
            .processes()
            .values()
            .max_by(|a, b| a.cpu_usage().total_cmp(&b.cpu_usage()))
            .map_or("<нет>".to_string(), |p| {
                format!(
                    "{} ({:.1}% машины / {:.0}% одного ядра)",
                    p.name().trim(),
                    p.cpu_usage() / n_cpus as f32,
                    p.cpu_usage()
                )
            });
        let top_ram = sys
            .processes()
            .values()
            .max_by(|a, b| a.memory().cmp(&b.memory()))
            .map_or("<нет>".to_string(), |p| {
                format!("{} ({} МБ)", p.name().trim(), p.memory() / 1_048_576)
            });

        let tick_line = format!(
            "[{now}] ЦП {avg_cpu:5.1}% | ОЗУ {ram:.2}/{ram_total:.2} ГБ | своп {swap:.2}/{swap_total:.2} ГБ | топ-ЦП: {top_cpu} | топ-ОЗУ: {top_ram}"
        );

        if write_synced(&mut file, &tick_line).is_err() {
            fail_write("тик не записан (носитель)".to_string());
            break;
        }
        for line in &new_lines {
            if write_synced(&mut file, line).is_err() {
                fail_write("строка нового процесса не записана".to_string());
                break;
            }
        }
        ticks.fetch_add(1, Ordering::Relaxed);

        // Лимиты: рекордер не может остаться забытым навечно.
        if started.elapsed() >= cfg.max_duration {
            let msg = format!("авто-остановка: лимит ВРЕМЕНИ {} сек", cfg.max_duration.as_secs());
            let _ = write_synced(&mut file, &format!("[{now}] === {msg} ==="));
            *auto_stopped_slot.lock().unwrap() = Some(msg);
            stopped_flag.store(true, Ordering::Relaxed);
            break;
        }
        if let Ok(meta) = file.metadata() {
            if meta.len() >= cfg.max_file_bytes {
                let msg = format!("авто-остановка: лимит РАЗМЕРА {} КБ", cfg.max_file_bytes / 1024);
                let _ = write_synced(&mut file, &format!("[{now}] === {msg} ==="));
                *auto_stopped_slot.lock().unwrap() = Some(msg);
                stopped_flag.store(true, Ordering::Relaxed);
                break;
            }
        }
    }

    // Финальная строка — при остановке по запросу и после ошибок записи.
    let now = Local::now().format("%H:%M:%S%.3f");
    let _ = write_synced(&mut file, &format!("[{now}] === РЕКОРДЕР ОСТАНОВЛЕН ==="));
}

// Тест-модуль вынесен в отдельный файл (гейт «файлы ≤ 500 строк» §8 PRP):
// подключается через #[path], чтобы unit-инварианты жили рядом с кодом.
#[cfg(test)]
#[path = "recorder_tests.rs"]
mod tests;

//! Управление процессом сервера 9router.
//!
//! - **Ленивый запуск по требованию**: порт пингуется TCP-коннектом; если сервер
//!   уже живёт — ничего не делаем, иначе стартуем `node.exe <dist>/custom-server.js`.
//! - **Без утечек**: процесс привязывается к Windows Job Object с флагом
//!   `KILL_ON_JOB_CLOSE` (ОС убивает на выходе приложения САМ, даже при
//!   насильственном закрытии) + реестр живых PID для планового килла в
//!   `RunEvent::ExitRequested`.
//! - **Headless**: сервер запускается напрямую (без интерактивного меню/трея CLI),
//!   по аналогии с `llama-server.exe`.

use std::io::Write;
use std::net::TcpStream;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::router::config::{node_exe, server_script, NineRouterConfig};

/// Живые PID запущенных серверов 9router. Нужен, чтобы при выходе из приложения
/// (`RunEvent::ExitRequested`) докилять сервер, если Job Object вдруг не сработал.
static ACTIVE_SERVER_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

pub fn register_server_pid(pid: u32) {
    ACTIVE_SERVER_PIDS.lock().unwrap().push(pid);
}

pub fn unregister_server_pid(pid: u32) {
    let mut g = ACTIVE_SERVER_PIDS.lock().unwrap();
    if let Some(pos) = g.iter().position(|&p| p == pid) {
        g.remove(pos);
    }
}

/// Докилять все живые серверы (вызывается при выходе из приложения).
pub fn kill_active_servers() {
    let pids: Vec<u32> = ACTIVE_SERVER_PIDS.lock().unwrap().clone();
    for pid in pids {
        log::info!("🔻 Принудительная остановка 9router (pid {}) при выходе из приложения", pid);
        kill_pid_tree(pid);
    }
    ACTIVE_SERVER_PIDS.lock().unwrap().clear();
}

/// Убить процесс по PID вместе со всем деревом потомков (taskkill /F /T).
#[cfg(windows)]
pub fn kill_pid_tree(pid: u32) {
    let mut kill = std::process::Command::new("taskkill");
    kill.args(["/F", "/T", "/PID", &pid.to_string()]);
    { use std::os::windows::process::CommandExt; kill.creation_flags(0x08000000); }
    let _ = kill.output();
}

/// Убить процесс по PID вместе со всем деревом потомков.
#[cfg(not(windows))]
pub fn kill_pid_tree(pid: u32) {
    let _ = std::process::Command::new("pkill").args(["-P", &pid.to_string()]).output();
    let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).output();
}

/// Достижим ли порт (health-check готовности сервера).
pub fn port_open(port: u16, timeout: Duration) -> bool {
    let addr: std::net::SocketAddr = match format!("127.0.0.1:{}", port).parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}

/// Дождаться готовности порта (фоновый сервер стартует ~1-2 сек).
pub fn wait_port(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if port_open(port, Duration::from_millis(300)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

/// Запустить сервер 9router (лениво, если ещё не запущен).
/// Возвращает PID запущенного процесса или 0, если сервер уже был жив.
pub fn start_server(app: &AppHandle, cfg: &NineRouterConfig) -> Result<u32, String> {
    let port = cfg.port_or_default();

    // Уже запущен — ничего не делаем.
    if port_open(port, Duration::from_millis(400)) {
        log::info!("🟢 9router уже запущен на порту {}", port);
        return Ok(0);
    }

    let dir = crate::router::config::router_dir(app);
    let node = node_exe(&dir);
    if !node.exists() {
        return Err("Установите 9router (портативный Node.js отсутствует).".to_string());
    }

    let dist = crate::router::config::dist_dir(&dir);
    let server = server_script(&dist);
    if !server.exists() {
        return Err(format!(
            "Установка 9router повреждена: не найден {}",
            server.display()
        ));
    }

    // ── Команда запуска (headless, без меню/трея CLI) ──
    // 9router CLI стартует standalone через: node --dns-result-order=ipv4first
    // --max-old-space-size=6144 <dist>/app/custom-server.js, cwd=dist/app, env PORT/HOSTNAME.
    // Повторяем в точности, чтобы сервер видел bundled node_modules (sql.js) через NODE_PATH.
    let mut cmd = std::process::Command::new(&node);
    cmd.args(["--dns-result-order=ipv4first", "--max-old-space-size=6144"])
        .arg(&server)
        .arg("--port")
        .arg(port.to_string())
        .current_dir(dist.join("app"))
        .env("PORT", port.to_string())
        // Бинд ТОЛЬКО на loopback — дашборд не должен торчать в сеть.
        .env("HOSTNAME", "127.0.0.1")
        .env("NODE_PATH", bundled_node_modules(&dist));

    // На Windows прячем консольное окно node.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // Логи сервера пишем в файл рядом с установкой (отладка без терминала).
    let log_path = dir.join("server.log");
    if let Ok(f) = std::fs::File::create(&log_path) {
        cmd.stdout(std::process::Stdio::from(f.try_clone().unwrap()));
        cmd.stderr(std::process::Stdio::from(f));
    } else {
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
    }

    log::info!("🚀 Запуск 9router: {} {}", node.display(), server.display());
    let child = cmd.spawn().map_err(|e| format!("Не удалось запустить 9router: {}", e))?;
    let pid = child.id();
    register_server_pid(pid);
    assign_child_to_kill_job(&child);

    // Готовность порта: без слепых ожиданий.
    if !wait_port(port, Duration::from_secs(20)) {
        // Не поднялся — подождём чуть-чуть и глянем log-хвост для диагностики.
        log::warn!("⚠️ 9router не ответил на порту {} за 20 сек (pid {})", port, pid);
        if let Ok(meta) = std::fs::metadata(&log_path) {
            if meta.len() > 0 {
                if let Ok(data) = std::fs::read(&log_path) {
                    let tail = String::from_utf8_lossy(&data[data.len().saturating_sub(2000)..]);
                    log::warn!("9router server.log хвост:\n{}", tail);
                }
            }
        }
        // Не убиваем сразу: сервер может дочитывать лонг-старт; вернём PID, хост
        // покажет статус, а провал ответит на /v1/models. Статус перепроверит UI.
    }

    Ok(pid)
}

/// NODE_PATH для дочернего node: bundled node_modules standalone (sql.js) +
/// runtime-каталог %APPDATA%/9router/runtime/node_modules (better-sqlite3, если есть).
fn bundled_node_modules(dist: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    let bundled = dist.join("app").join("node_modules");
    if bundled.exists() {
        parts.push(bundled.to_string_lossy().to_string());
    }
    if let Some(apd) = std::env::var_os("APPDATA") {
        let runtime = std::path::PathBuf::from(apd).join("9router").join("runtime").join("node_modules");
        if runtime.exists() {
            parts.push(runtime.to_string_lossy().to_string());
        }
    }
    parts.join(";")
}

/// Остановить сервер 9router (плавно: по PID-дереву).
pub fn stop_server() {
    let pids: Vec<u32> = ACTIVE_SERVER_PIDS.lock().unwrap().clone();
    for pid in pids {
        log::info!("🛑 Остановка 9router (pid {})", pid);
        kill_pid_tree(pid);
    }
    ACTIVE_SERVER_PIDS.lock().unwrap().clear();
}

/// Прибить все процессы node.exe, чей исполняемый файл — `target`.
///
/// Нужен для обновления: `install_or_update` перезаписывает `node.exe`, а
/// Windows блокирует занятый файл («os error 32»). Помимо зарегистрированных
/// PID (`stop_server`) здесь ловим и «осиротевшие» node.exe из прошлых сессий
/// приложения, которые не попали в реестр `ACTIVE_SERVER_PIDS`.
pub fn kill_node_processes(target: &Path) {
    let target_str = target.to_string_lossy().replace('\'', "''");
    #[cfg(windows)]
    {
        let ps = format!(
            "Get-CimInstance Win32_Process | Where-Object {{ $_.Name -eq 'node.exe' -and $_.ExecutablePath -eq '{t}' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}",
            t = target_str
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &ps])
            .output();
        if let Ok(out) = out {
            if !out.status.success() {
                log::warn!(
                    "kill_node_processes: PowerShell: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-f", &target.to_string_lossy()])
            .output();
    }
}

/// Тихая остановка по RunEvent — требует AppHandle, но у нас достаточно реестра.
///
/// Здесь мы НЕ убиваем поверх уже живого (job-объект сам займётся при выходе),
/// дублирующий килл — на `kill_active_servers`.

// ═══════════════════════ Windows Job Object (KILL_ON_JOB_CLOSE) ═══════════════════════

#[cfg(windows)]
mod kill_job {
    //! Windows Job Object с флагом KILL_ON_JOB_CLOSE: ОС сама убивает все
    //! назначенные процессы, когда приложение завершается — даже при
    //! насильственном закрытии («End Process» в Диспетчере), когда `Drop`
    //! и обработчик ExitRequested уже не успевают отработать.
    #![allow(non_camel_case_types, dead_code)]

    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::ptr;
    use std::sync::Mutex;

    type HANDLE = *mut c_void;
    type BOOL = i32;
    type DWORD = u32;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct JobObjectBasicLimitInformation {
        limit_flags: DWORD,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: DWORD,
        affinity: usize,
        priority_class: DWORD,
        scheduling_class: DWORD,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct IoCounters {
        read: u64, write: u64, other: u64, read_ops: u64, write_ops: u64, other_ops: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct JobObjectExtendedLimitInformation {
        basic_limit_information: JobObjectBasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: DWORD = 0x0000_2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;

    extern "system" {
        fn CreateJobObjectW(lp_attributes: *mut c_void, lp_name: *const u16) -> HANDLE;
        fn AssignProcessToJobObject(h_job: HANDLE, h_process: HANDLE) -> BOOL;
        fn SetInformationJobObject(
            h_job: HANDLE,
            info_class: u32,
            lp_info: *mut c_void,
            cb: DWORD,
        ) -> BOOL;
        fn CloseHandle(h_object: HANDLE) -> BOOL;
    }

    // Хендл job-объекта живёт всё время работы приложения (в static). Когда
    // процесс приложения завершается, ОС закрывает этот хендл → срабатывает
    // KILL_ON_JOB_CLOSE → все назначенные серверы убиваются.
    static KILL_JOB: Mutex<Option<isize>> = Mutex::new(None);

    /// Назначить дочерний процесс в общий kill-job. Best-effort: если ОС не
    /// разрешает (процесс уже в системном job), просто игнорируем — тогда
    /// зачистка ложится на `kill_pid_tree` по ExitRequested.
    pub fn assign_child_to_kill_job(child: &std::process::Child) {
        let mut guard = KILL_JOB.lock().unwrap();
        let job = match *guard {
            Some(h) => h as HANDLE,
            None => {
                let h = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
                if h.is_null() {
                    return;
                }
                let mut info = JobObjectExtendedLimitInformation {
                    basic_limit_information: JobObjectBasicLimitInformation {
                        limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                        ..unsafe { std::mem::zeroed() }
                    },
                    ..unsafe { std::mem::zeroed() }
                };
                let ok = unsafe {
                    SetInformationJobObject(
                        h,
                        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                        &mut info as *mut _ as *mut c_void,
                        std::mem::size_of::<JobObjectExtendedLimitInformation>() as DWORD,
                    )
                };
                if ok == 0 {
                    unsafe { CloseHandle(h); }
                    return;
                }
                *guard = Some(h as isize);
                h
            }
        };
        drop(guard);
        let _ = unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) };
    }
}

#[cfg(windows)]
pub use kill_job::assign_child_to_kill_job;

/// Не-windows: назначение в kill-job — no-op (на Linux/macOS дерево гасится
/// kill_pid_tree по ExitRequested).
#[cfg(not(windows))]
pub fn assign_child_to_kill_job(_child: &std::process::Child) {}

/// Дописать строку в лог (используется installer-ом для прогресса).
pub fn append_log(dir: &Path, line: &str) {
    let log_path = dir.join("server.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path) {
        let _ = writeln!(f, "{}", line);
    }
}
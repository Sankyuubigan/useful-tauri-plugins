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

// ───────────────────────── Владелец порта (правда о статусе) ─────────────────────────

/// Состояние порта 9router с точки зрения НАШЕЙ установки.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatewayState {
    /// Порт закрыт — сервера нет.
    NotListening,
    /// Порт держит наш портативный node.exe — наш шлюз жив.
    OursRunning,
    /// Порт держит чужой процесс (внешний 9Router из npm и т.п.), pid владельца.
    ForeignOccupant(u32),
}

/// Фактическое состояние шлюза. Чужой процесс на порту НЕ выдаётся за наш
/// запущенный сервер: иначе панель и логи врут (инцидент: внешний npm 9router
/// на порту 20128 выглядел как «работающий» шлюз при ненастроенном бандле).
pub fn gateway_state(port: u16, router_dir: &Path) -> GatewayState {
    if !port_open(port, Duration::from_millis(400)) {
        return GatewayState::NotListening;
    }

    // 1. Если эндпоинт отвечает на 9Router health-check — шлюз активен и готов к работе
    if crate::router::client::is_healthy(&format!("http://127.0.0.1:{}", port)) {
        return GatewayState::OursRunning;
    }

    // 2. Проверяем владеющий процессу сокет
    if let Some(pid) = listener_pid(port) {
        if pid != 0 {
            // Запущен нами в этой сессии
            if ACTIVE_SERVER_PIDS.lock().unwrap().contains(&pid) {
                return GatewayState::OursRunning;
            }
            // Совпадает путь исполняемого файла с нашими настройками
            if process_exe_matches(pid, &node_exe(router_dir)) {
                return GatewayState::OursRunning;
            }
            // Процесс на порту — node.exe (наш портативный или прошлый запуск)
            if let Some(exe) = port_owner_exe_path(pid) {
                let exe_lower = exe.to_lowercase();
                if exe_lower.ends_with("node.exe") || exe_lower.ends_with("node") {
                    return GatewayState::OursRunning;
                }
            }
            return GatewayState::ForeignOccupant(pid);
        }
    }

    // 3. Фолбэк (не удалось получить PID)
    if !ACTIVE_SERVER_PIDS.lock().unwrap().is_empty() {
        GatewayState::OursRunning
    } else {
        GatewayState::ForeignOccupant(0)
    }
}

#[cfg(windows)]
fn listener_pid(port: u16) -> Option<u32> {
    port_owner::listening_pid(port)
}

#[cfg(not(windows))]
fn listener_pid(_port: u16) -> Option<u32> {
    None
}

#[cfg(windows)]
fn port_owner_exe_path(pid: u32) -> Option<String> {
    port_owner::exe_path(pid)
}

#[cfg(not(windows))]
fn port_owner_exe_path(_pid: u32) -> Option<String> {
    None
}

#[cfg(windows)]
fn process_exe_matches(pid: u32, ours: &Path) -> bool {
    let Some(p) = port_owner::exe_path(pid) else {
        return false;
    };
    let ours_norm = port_owner::normalize(ours.to_string_lossy().as_ref());
    port_owner::normalize(&p).eq_ignore_ascii_case(&ours_norm)
}

#[cfg(not(windows))]
fn process_exe_matches(_pid: u32, _ours: &Path) -> bool {
    // Best-effort: на не-Windows владельца порта не опросить — полагаемся на
    // реестр поднятых PID (fallback в gateway_state).
    false
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

    // Уже жив НАШ сервер — ничего не делаем. Чужой процесс на порту (внешний
    // 9Router из npm и т.п.) за «запущенный» не выдаём: приложение свой сервер
    // не подняло, а писать «работает» про чужой — ложь (инцидент на порту 20128).
    let dir_early = crate::router::config::router_dir(app);
    match gateway_state(port, &dir_early) {
        GatewayState::OursRunning => {
            log::info!("🟢 9router уже запущен на порту {} (наш сервер)", port);
            return Ok(0);
        }
        GatewayState::ForeignOccupant(pid) => {
            let who = if pid != 0 {
                format!("чужим процессом (pid {})", pid)
            } else {
                "чужим процессом".to_string()
            };
            return Err(format!(
                "Порт {} занят {}, не нашим 9router. Остановите внешний 9Router или смените порт (nine_router.port), затем повторите.",
                port, who
            ));
        }
        GatewayState::NotListening => {}
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

    // Дополнительно останавливаем сиротские процессы на порту 20128
    if let Some(pid) = listener_pid(20128) {
        if pid != 0 {
            log::info!("🛑 Остановка процесса на порту 20128 (pid {})", pid);
            kill_pid_tree(pid);
        }
    }
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
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &ps]);
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let out = cmd.output();
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

// ══════════════════ Windows: владелец LISTEN-порта (правда о статусе) ══════════════════

#[cfg(windows)]
mod port_owner {
    //! Определение реального владельца порта: `GetExtendedTcpTable` (LISTEN →
    //! owning PID) + `QueryFullProcessImageNameW` (путь процесса). Без новых
    //! крейтов, raw FFI как в `kill_job`.
    #![allow(non_camel_case_types, dead_code)]

    use std::ffi::c_void;

    type DWORD = u32;
    type HANDLE = *mut c_void;

    const AF_INET: u32 = 2;
    const TCP_TABLE_OWNER_PID_LISTENER: u32 = 4;
    const NO_ERROR: u32 = 0;
    const MIB_TCP_STATE_LISTEN: u32 = 2;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MibTcpRowOwnerPid {
        state: u32,
        local_addr: u32,
        local_port: u32, // network byte order
        remote_addr: u32,
        remote_port: u32,
        owning_pid: u32,
    }

    #[repr(C)]
    struct MibTcpTableOwnerPid {
        num_entries: u32,
        table: [MibTcpRowOwnerPid; 1],
    }

    extern "system" {
        fn GetExtendedTcpTable(
            p_tcp_table: *mut c_void,
            pdw_size: *mut DWORD,
            b_order: i32,
            ul_af: u32,
            table_class: u32,
            reserved: u32,
        ) -> u32;
        fn OpenProcess(dw_desired_access: u32, b_inherit_handle: i32, dw_process_id: u32) -> HANDLE;
        fn QueryFullProcessImageNameW(
            h_process: HANDLE,
            dw_flags: u32,
            lp_exe_name: *mut u16,
            lpdw_size: *mut u32,
        ) -> i32;
        fn CloseHandle(h_object: HANDLE) -> i32;
    }

    /// PID процесса, владеющего LISTEN-сокетом на заданном порту (или None).
    pub fn listening_pid(port: u16) -> Option<u32> {
        let mut size: u32 = 0;
        let _ = unsafe {
            GetExtendedTcpTable(
                std::ptr::null_mut(),
                &mut size,
                0,
                AF_INET,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let rc = unsafe {
            GetExtendedTcpTable(
                buf.as_mut_ptr() as *mut c_void,
                &mut size,
                0,
                AF_INET,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if rc != NO_ERROR {
            return None;
        }
        let table = unsafe { &*(buf.as_ptr() as *const MibTcpTableOwnerPid) };
        if table.num_entries == 0 {
            return None;
        }
        let rows = unsafe {
            std::slice::from_raw_parts(
                table.table.as_ptr(),
                table.num_entries as usize,
            )
        };
        for row in rows {
            if row.state == MIB_TCP_STATE_LISTEN
                && u16::from_be((row.local_port & 0xffff) as u16) == port
            {
                return Some(row.owning_pid);
            }
        }
        None
    }

    /// Полный путь к исполняемому файлу процесса.
    pub fn exe_path(pid: u32) -> Option<String> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 32768];
        let mut size = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) };
        unsafe { CloseHandle(handle) };
        if ok == 0 || size == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..size as usize]))
    }

    /// Нормализация пути для сравнения (разделители, хвостовой NUL).
    pub fn normalize(p: &str) -> String {
        p.replace('/', "\\").trim_end_matches('\0').to_string()
    }
}

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
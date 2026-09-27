//! Управление процессом сервера 9router.
//!
//! - **Ленивый запуск по требованию**: порт пингуется TCP-коннектом; если сервер
//!   уже живёт — ничего не делаем, иначе стартуем `node.exe <dist>/custom-server.js`.
//! - **Долгоживущий сервер**: процесс переживает закрытие приложения и работает
//!   в фоновом режиме, обслуживая и dev, и релизные сборки.
//! - **Headless**: сервер запускается напрямую (без интерактивного меню/трея CLI),
//!   по аналогии с `llama-server.exe`.

use std::io::Write;
use std::net::TcpStream;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::router::config::{
    clear_server_record, node_exe, read_server_record, router_data_dir, server_script,
    write_server_record, NineRouterConfig,
};

/// Живые PID запущенных серверов 9router в текущей сессии.
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

/// Жив ли процесс по PID.
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        port_owner::exe_path(pid).is_some()
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

/// Достижим ли порт (health-check готовности сервера).
pub fn port_open(port: u16, timeout: Duration) -> bool {
    let addr: std::net::SocketAddr = match format!("127.0.0.1:{}", port).parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}

fn decode_local_port(value: u32) -> u16 {
    // `dwLocalPort` из GetExtendedTcpTable приходит в network byte order
    // (big-endian), а не в little-endian хоста — иначе порт 20128 (0x4EA0)
    // декодируется как 0xA04E и поиск PID по порту никогда не находит цель.
    u16::from_be(value as u16)
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

/// Фактическое состояние шлюза с учётом `server.json` и активных PID.
pub fn gateway_state(port: u16, router_dir: &Path, data_dir: &Path) -> GatewayState {
    if !port_open(port, Duration::from_millis(400)) {
        return GatewayState::NotListening;
    }

    // 1. Проверяем server.json запись
    if let Some(rec) = read_server_record(data_dir) {
        if pid_alive(rec.pid) {
            return GatewayState::OursRunning;
        }
    }

    // 2. Проверяем активные PID в текущей сессии
    if let Some(pid) = listener_pid(port) {
        if pid != 0 {
            if ACTIVE_SERVER_PIDS.lock().unwrap().contains(&pid) {
                return GatewayState::OursRunning;
            }
            if process_exe_matches(pid, &node_exe(router_dir)) {
                return GatewayState::OursRunning;
            }
            if let Some(exe) = port_owner_exe_path(pid) {
                let exe_lower = exe.to_lowercase();
                if exe_lower.ends_with("node.exe") || exe_lower.ends_with("node") {
                    return GatewayState::OursRunning;
                }
            }
            return GatewayState::ForeignOccupant(pid);
        }
    }

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
pub fn process_exe_matches(pid: u32, ours: &Path) -> bool {
    let Some(p) = port_owner::exe_path(pid) else {
        return false;
    };
    let ours_norm = port_owner::normalize(ours.to_string_lossy().as_ref());
    port_owner::normalize(&p).eq_ignore_ascii_case(&ours_norm)
}

#[cfg(not(windows))]
pub fn process_exe_matches(_pid: u32, _ours: &Path) -> bool {
    false
}

/// Дождаться готовности порта.
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

/// Дождаться закрытия порта.
pub fn wait_port_closed(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !port_open(port, Duration::from_millis(200)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

/// Запустить сервер 9router (лениво, если ещё не запущен).
pub fn start_server(app: &AppHandle, cfg: &NineRouterConfig) -> Result<u32, String> {
    let port = cfg.port_or_default();
    let dir = crate::router::config::router_dir(app);
    let data_dir = router_data_dir(app);

    match gateway_state(port, &dir, &data_dir) {
        GatewayState::OursRunning => {
            log::info!("🟢 9router уже запущен на порту {} (наш сервер)", port);
            if read_server_record(&data_dir).is_none() {
                if let Some(pid) = listener_pid(port) {
                    write_server_record(&data_dir, pid, port);
                }
            }
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

    let mut cmd = std::process::Command::new(&node);
    cmd.args(["--dns-result-order=ipv4first", "--max-old-space-size=6144"])
        .arg(&server)
        .arg("--port")
        .arg(port.to_string())
        .current_dir(dist.join("app"))
        .env("PORT", port.to_string())
        .env("HOSTNAME", "127.0.0.1")
        .env("DATA_DIR", &data_dir)
        .env("NODE_PATH", bundled_node_modules(&dist, &data_dir));

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let log_path = dir.join("server.log");
    if let Ok(f) = std::fs::File::create(&log_path) {
        cmd.stdout(std::process::Stdio::from(f.try_clone().unwrap()));
        cmd.stderr(std::process::Stdio::from(f));
    } else {
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
    }

    log::info!("🚀 Запуск 9router: {} {} (DATA_DIR={})", node.display(), server.display(), data_dir.display());
    let child = cmd.spawn().map_err(|e| format!("Не удалось запустить 9router: {}", e))?;
    let pid = child.id();
    register_server_pid(pid);
    write_server_record(&data_dir, pid, port);

    if !wait_port(port, Duration::from_secs(20)) {
        log::warn!("⚠️ 9router не ответил на порту {} за 20 сек (pid {})", port, pid);
        if let Ok(meta) = std::fs::metadata(&log_path) {
            if meta.len() > 0 {
                if let Ok(data) = std::fs::read(&log_path) {
                    let tail = String::from_utf8_lossy(&data[data.len().saturating_sub(2000)..]);
                    log::warn!("9router server.log хвост:\n{}", tail);
                }
            }
        }
    }

    Ok(pid)
}

/// NODE_PATH для дочернего node: bundled node_modules standalone + data_dir/runtime/node_modules.
fn bundled_node_modules(dist: &Path, data_dir: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    let bundled = dist.join("app").join("node_modules");
    if bundled.exists() {
        parts.push(bundled.to_string_lossy().to_string());
    }
    let runtime = data_dir.join("runtime").join("node_modules");
    if runtime.exists() {
        parts.push(runtime.to_string_lossy().to_string());
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    parts.join(sep)
}

/// Остановить сервер 9router (активные PID + server.json + порт).
pub fn stop_server(port: u16, data_dir: &Path) {
    let mut pids: Vec<u32> = ACTIVE_SERVER_PIDS.lock().unwrap().clone();
    if let Some(rec) = read_server_record(data_dir) {
        if !pids.contains(&rec.pid) {
            pids.push(rec.pid);
        }
    }
    if let Some(pid) = listener_pid(port) {
        if pid != 0 && !pids.contains(&pid) {
            if let Some(exe) = port_owner_exe_path(pid) {
                let lower = exe.to_lowercase();
                if lower.ends_with("node.exe") || lower.ends_with("node") {
                    pids.push(pid);
                }
            }
        }
    }

    if pids.is_empty() {
        log::warn!(
            "⚠️ 9router: stop_server не нашёл процессов для остановки (порт {})",
            port
        );
    }

    for pid in pids {
        log::info!("🛑 Остановка 9router (pid {})", pid);
        kill_pid_tree(pid);
    }
    ACTIVE_SERVER_PIDS.lock().unwrap().clear();
    clear_server_record(data_dir);
}

/// Самолечение машин при старте нового бинаря: убить старый бесхозный инстанс node.exe на порту.
pub fn reconcile_server_state(app: &AppHandle) {
    let cfg = crate::router::config::load_config(app);
    let port = cfg.port_or_default();
    let dir = crate::router::config::router_dir(app);
    let data_dir = router_data_dir(app);

    if let Some(rec) = read_server_record(&data_dir) {
        if pid_alive(rec.pid) {
            log::info!("9router: обнаружен работающий наш сервер (pid {})", rec.pid);
            return;
        } else {
            clear_server_record(&data_dir);
        }
    }

    match gateway_state(port, &dir, &data_dir) {
        GatewayState::ForeignOccupant(pid) if pid != 0 => {
            if process_exe_matches(pid, &node_exe(&dir)) {
                log::warn!("9router: обнаружен старый бесхозный инстанс node.exe на порту {} (pid {}), останавливаем...", port, pid);
                stop_server(port, &data_dir);
                let _ = wait_port_closed(port, Duration::from_secs(5));
            }
        }
        _ => {}
    }
}

/// Прибить все процессы node.exe, чей исполняемый файл — `target`.
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
        let _ = cmd.output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-f", &target.to_string_lossy()])
            .output();
    }
}

pub fn append_log(dir: &Path, line: &str) {
    let log_path = dir.join("server.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path) {
        let _ = writeln!(f, "{}", line);
    }
}

#[cfg(windows)]
mod port_owner {
    #![allow(non_camel_case_types, dead_code)]

    use super::decode_local_port;
    use std::ffi::c_void;

    type DWORD = u32;
    type HANDLE = *mut c_void;

    const AF_INET: u32 = 2;
    const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;
    const NO_ERROR: u32 = 0;
    const MIB_TCP_STATE_LISTEN: u32 = 2;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MibTcpRowOwnerPid {
        state: u32,
        local_addr: u32,
        local_port: u32,
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
                && decode_local_port(row.local_port) == port
            {
                return Some(row.owning_pid);
            }
        }
        None
    }

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

    pub fn normalize(p: &str) -> String {
        p.replace('/', "\\").trim_end_matches('\0').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::decode_local_port;

    #[test]
    fn windows_local_port_decodes_network_byte_order() {
        // Порт 20128 = 0x4EA0 в network byte order (big-endian): 0xA04E в LE.
        // Раньше здесь стоял from_le — порт не совпадал и listener_pid не находил.
        assert_eq!(decode_local_port(20_128), 20_128);
        assert_ne!(decode_local_port(20_128), 0xA04E);
    }
}

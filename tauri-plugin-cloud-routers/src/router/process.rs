//! Управление процессом сервера облачных роутеров.
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
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::commands::RouterId;
use crate::router::config::{
    clear_server_record, node_exe, read_server_record, router_data_dir, server_script,
    write_server_record, RouterConfig,
};

/// Бюджет верификации остановки: тот же, что был у сильного пути (`stop_and_wait`
/// в `installer.rs`). Единый для кнопки и установки — два разных таймаута на одно
/// действие были нарушением SSOT.
pub const STOP_WAIT: Duration = Duration::from_secs(5);

/// Итог попытки убить процесс.
///
/// Раньше вывод `taskkill` выбрасывался (`let _ = kill.output()`), и причина отказа
/// вида «kill вызван, порт не закрылся» была неустановима в принципе. Теперь
/// результат возвращается и логируется — молчаливый отказ невозможен конструктивно.
#[derive(Debug, Clone)]
pub struct KillReport {
    pub pid: u32,
    pub succeeded: bool,
    pub detail: String,
}

/// Живые пиды серверов — отдельно на каждый роутер.
///
/// Общий список с глобальным `clear()` приводил к тому, что `stop(9router)` стирал
/// учёт extremerouter/omniroute. Индексация — по `RouterId::index()`, как мьютексы
/// установки в `installer.rs`: размер берётся из таблицы роутеров, а не из литерала.
static ACTIVE_SERVER_PIDS: OnceLock<Vec<Mutex<Vec<u32>>>> = OnceLock::new();

fn active_pids() -> &'static Vec<Mutex<Vec<u32>>> {
    ACTIVE_SERVER_PIDS.get_or_init(|| {
        (0..RouterId::ALL.len())
            .map(|_| Mutex::new(Vec::new()))
            .collect()
    })
}

pub fn register_server_pid(router_id: RouterId, pid: u32) {
    active_pids()[router_id.index()].lock().unwrap().push(pid);
}

pub fn unregister_server_pid(router_id: RouterId, pid: u32) {
    let mut g = active_pids()[router_id.index()].lock().unwrap();
    if let Some(pos) = g.iter().position(|&p| p == pid) {
        g.remove(pos);
    }
}

#[cfg(test)]
pub fn test_active_snapshot(router_id: RouterId) -> Vec<u32> {
    active_pids()[router_id.index()].lock().unwrap().clone()
}

fn truncate_detail(s: &str, max_chars: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max_chars {
        return t.to_string();
    }
    let skip = t.chars().count() - max_chars;
    format!("…{}", t.chars().skip(skip).collect::<String>())
}

/// Декодировать stdout/stderr **внешней консольной программы** (SSOT для всех
/// мест вставки такого вывода в журналы).
///
/// Windows-консольные утилиты (`taskkill`, `powershell`) пишут локализованные
/// сообщения в OEM-кодировке (для ru-RU — CP866). Прежний
/// `String::from_utf8_lossy` оставлял CP866-байты как есть: лог (UTF-8)
/// превращался в кракозябры, и «причина отказа» становилась нечитаемой
/// (proof 22.10.26: строки «taskkill pid …: отказ …: �訡�� …» в last_logs.txt).
/// Для не-Windows обычный UTF-8-lossy остаётся.
fn console_lossy(bytes: &[u8]) -> String {
    #[cfg(windows)]
    {
        encoding_rs::IBM866.decode(bytes).0.into_owned()
    }
    #[cfg(not(windows))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(windows)]
pub fn kill_pid_tree(pid: u32) -> KillReport {
    let mut kill = std::process::Command::new("taskkill");
    kill.args(["/F", "/T", "/PID", &pid.to_string()]);
    { use std::os::windows::process::CommandExt; kill.creation_flags(0x08000000); }
    match kill.output() {
        Ok(out) => {
            let rep = KillReport {
                pid,
                succeeded: out.status.success(),
                detail: truncate_detail(
                    &format!(
                        "{} {}",
                        console_lossy(&out.stdout),
                        console_lossy(&out.stderr)
                    ),
                    500,
                ),
            };
            if rep.succeeded {
                log::info!("taskkill pid {}: успех {}", pid, rep.detail);
            } else {
                log::warn!("taskkill pid {}: отказ ({}): {}", pid, out.status, rep.detail);
            }
            rep
        }
        Err(e) => {
            log::warn!("taskkill pid {}: не запустился: {}", pid, e);
            KillReport { pid, succeeded: false, detail: e.to_string() }
        }
    }
}

#[cfg(not(windows))]
pub fn kill_pid_tree(pid: u32) -> KillReport {
    let _ = std::process::Command::new("pkill").args(["-P", &pid.to_string()]).output();
    match std::process::Command::new("kill").args(["-9", &pid.to_string()]).output() {
        Ok(out) => {
            let rep = KillReport {
                pid,
                succeeded: out.status.success(),
                detail: truncate_detail(
                    &format!(
                        "{} {}",
                        console_lossy(&out.stdout),
                        console_lossy(&out.stderr)
                    ),
                    500,
                ),
            };
            if !rep.succeeded {
                log::warn!("kill -9 pid {}: отказ: {}", pid, rep.detail);
            }
            rep
        }
        Err(e) => KillReport { pid, succeeded: false, detail: e.to_string() },
    }
}

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

pub fn port_open(port: u16, timeout: Duration) -> bool {
    let addr: std::net::SocketAddr = match format!("127.0.0.1:{}", port).parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}

fn decode_local_port(value: u32) -> u16 {
    u16::from_be(value as u16)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatewayState {
    NotListening,
    OursRunning,
    ForeignOccupant(u32),
}

pub fn gateway_state(router_id: RouterId, port: u16, router_dir: &Path, data_dir: &Path) -> GatewayState {
    if !port_open(port, Duration::from_millis(400)) {
        return GatewayState::NotListening;
    }

    if let Some(rec) = read_server_record(data_dir) {
        if pid_alive(rec.pid) {
            return GatewayState::OursRunning;
        }
    }

    if let Some(pid) = listener_pid(port) {
        if pid != 0 {
            if active_pids()[router_id.index()].lock().unwrap().contains(&pid) {
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

    if !active_pids()[router_id.index()].lock().unwrap().is_empty() {
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

/// Дождаться, пока процессы с данным node.exe перестанут существовать.
///
/// `kill_node_processes` и `taskkill` возвращаются сразу, а Windows ещё
/// несколько миллисекунд держит открытые файлы. Вызывающий код сразу после
/// этого удаляет папку с этими файлами — и получает отказ «занято», который
/// выглядит как ошибка удаления, хотя это ошибка слишком раннего удаления.
pub fn wait_node_gone(target: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if node_pids(target).is_empty() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// PID процессов node.exe по точному пути исполняемого файла.
///
/// Единственный источник правды о «кто ещё держит наш node»: тот же запрос,
/// что и в `kill_node_processes`, поэтому «убили» и «действительно умерли»
/// всегда про одно и то же множество процессов.
fn node_pids(target: &Path) -> Vec<u32> {
    #[cfg(windows)]
    {
        let target_str = target.to_string_lossy().replace('\'', "''");
        let ps = format!(
            "Get-CimInstance Win32_Process | Where-Object {{ $_.Name -eq 'node.exe' -and $_.ExecutablePath -eq '{t}' }} | ForEach-Object {{ $_.ProcessId }}",
            t = target_str
        );
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &ps]);
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        match cmd.output() {
            Ok(out) => console_lossy(&out.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
                .filter_map(|s| s.parse::<u32>().ok())
                .collect(),
            Err(_) => Vec::new(),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Vec::new()
    }
}

/// Хвост `server.log` для сообщения об ошибке запуска.
pub fn server_log_tail(dir: &Path, max_chars: usize) -> String {
    let path = dir.join("server.log");
    let Ok(data) = std::fs::read(&path) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&data);
    if text.chars().count() <= max_chars {
        return text.trim().to_string();
    }
    let skip = text.chars().count() - max_chars;
    format!(
        "…{}",
        text.chars().skip(skip).collect::<String>().trim_start()
    )
}

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

pub fn start_server(app: &AppHandle, router_id: RouterId, cfg: &RouterConfig) -> Result<u32, String> {
    let port = cfg.port_or_default(router_id);
    let dir = crate::router::config::router_dir(app, router_id);
    let data_dir = router_data_dir(app, router_id);

    match gateway_state(router_id, port, &dir, &data_dir) {
        GatewayState::OursRunning => {
            log::info!("🟢 {} уже запущен на порту {} (наш сервер)", router_id, port);
            if read_server_record(&data_dir).is_none() {
                if let Some(pid) = listener_pid(port) {
                    write_server_record(&data_dir, pid, port, None);
                }
            }
            // Проверка экрана логина идёт по состоянию живого сервера, а не по
            // конфиг-флагу: ensure_started при работающем сервере больше ничего
            // не делает, и без этой точки проверка замёрзла бы до следующего
            // рестарта вручную. Дедуп по pid сервера — внутри no_login.rs.
            let record_pid = read_server_record(&data_dir).map(|r| r.pid).unwrap_or(0);
            crate::router::no_login::enforce_login_state(app, router_id, port, record_pid);
            return Ok(0);
        }
        GatewayState::ForeignOccupant(pid) => {
            let who = if pid != 0 {
                format!("чужим процессом (pid {})", pid)
            } else {
                "чужим процессом".to_string()
            };
            return Err(format!(
                "Порт {} занят {}, не нашим {}. Остановите внешний роутер или смените порт.",
                port, who, router_id
            ));
        }
        GatewayState::NotListening => {}
    }

    let node = node_exe(&dir);
    if !node.exists() {
        return Err(format!("Установите {} (портативный Node.js отсутствует).", router_id));
    }

    let dist = crate::router::config::dist_dir(&dir);
    let server = server_script(&dist, router_id);
    if !server.exists() {
        return Err(format!(
            "Установка {} повреждена: не найден {}",
            router_id,
            server.display()
        ));
    }

    let server_parent = server.parent().unwrap_or(&dist).to_path_buf();
    let mut cmd = std::process::Command::new(&node);
    cmd.args(["--dns-result-order=ipv4first", "--max-old-space-size=6144"])
        .arg(&server)
        .arg("--port")
        .arg(port.to_string())
        .current_dir(&server_parent)
        .env("PORT", port.to_string())
        .env("HOSTNAME", "127.0.0.1")
        .env("DATA_DIR", &data_dir)
        .env("NODE_PATH", bundled_node_modules(&server_parent, &data_dir));

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let log_path = dir.join("server.log");
    if let Ok(f) = std::fs::File::create(&log_path) {
        cmd.stdout(std::process::Stdio::from(f.try_clone().unwrap()));
        cmd.stderr(std::process::Stdio::from(f));
    } else {
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
    }

    log::info!("🚀 Запуск {}: {} {} (DATA_DIR={})", router_id, node.display(), server.display(), data_dir.display());
    let child = cmd.spawn().map_err(|e| format!("Не удалось запустить {}: {}", router_id, e))?;
    let pid = child.id();
    register_server_pid(router_id, pid);
    write_server_record(&data_dir, pid, port, None);

    if !wait_port(port, Duration::from_secs(20)) {
        // Раньше здесь стоял только warning, а функция возвращала Ok(pid).
        // Для вызывающих (ensure_started / get_combos / chat_completion /
        // open_dashboard) это был ложный успех: UI показывал «запущено»,
        // а порт не отвечал, и следующий запрос падал уже как чужую ошибку.
        // Теперь это честный отказ, причина — из server.log, а не «где-то в логе».
        let tail = server_log_tail(&dir, 1200);
        log::error!(
            "❌ {} не ответил на порту {} за 20 сек (pid {})\n--- server.log ---\n{}",
            router_id,
            port,
            pid,
            tail
        );
        kill_pid_tree(pid);
        unregister_server_pid(router_id, pid);
        clear_server_record(&data_dir);
        return Err(match tail.is_empty() {
            false => format!(
                "{} не ответил на порту {} за 20 сек и был остановлен.\nПоследние строки server.log:\n{}",
                router_id, port, tail
            ),
            true => format!(
                "{} не ответил на порту {} за 20 сек и был остановлен. \
                 server.log пуст — сервер умер до первой строки вывода (проверьте node.exe и порт).",
                router_id, port
            ),
        });
    }

    // Проверка экрана логина — state-check (no_login.rs): публичный статус и
    // однократная попытка на процесс сервера. Порт уже реально слушает, поэтому
    // запросы не уходят в недоступность. Отказ шага ошибкой запуска не
    // считается: только warn в лог.
    crate::router::no_login::enforce_login_state(app, router_id, port, pid);

    Ok(pid)
}

fn bundled_node_modules(server_parent: &Path, data_dir: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    let bundled = server_parent.join("node_modules");
    if bundled.exists() {
        parts.push(bundled.to_string_lossy().to_string());
    }
    if let Some(parent) = server_parent.parent() {
        let parent_modules = parent.join("node_modules");
        if parent_modules.exists() {
            parts.push(parent_modules.to_string_lossy().to_string());
        }
    }
    let runtime = data_dir.join("runtime").join("node_modules");
    if runtime.exists() {
        parts.push(runtime.to_string_lossy().to_string());
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    parts.join(sep)
}

/// Остановить node-роутер с проверкой результата.
///
/// Контракт (единый для кнопки «Остановить» и установки):
/// - убиваются ТОЛЬКО свои процессы (сверка exe с нашим `node.exe`, как у шлюза);
///   чужой `node.exe` на порту не трогается — возвращается `Err`;
/// - успех подтверждается закрытием порта и уходом процессов (`STOP_WAIT`);
/// - при неудаче запись `server.json` НЕ стирается и возвращается `Err` с причиной:
///   ложный успех («остановлен», хотя порт открыт) невозможен конструктивно;
/// - учёт `ACTIVE` ведётся только своего роутера — соседние роутеры не затрагиваются.
///
/// Sweep `kill_node_processes` входит сюда же: раньше он был только у пути установки
/// (`stop_and_wait`), и кнопка убивала слабее, чем установка, для того же действия.
pub fn stop_server(
    router_id: RouterId,
    router_dir: &Path,
    port: u16,
    data_dir: &Path,
) -> Result<(), String> {
    let node = node_exe(router_dir);
    let mut targets: Vec<u32> = Vec::new();
    let mut foreign: Option<u32> = None;
    let mut record_pid: Option<u32> = None;

    // 1. Запись о процессе: живой свой pid — кандидат; живой чужой — отказ с
    // сохранением записи (как у шлюза); мёртвый — мусор, убираем сразу.
    if let Some(rec) = read_server_record(data_dir) {
        if rec.pid == 0 || !pid_alive(rec.pid) {
            clear_server_record(data_dir);
        } else if process_exe_matches(rec.pid, &node) {
            record_pid = Some(rec.pid);
            targets.push(rec.pid);
        } else {
            log::warn!(
                "🚫 {}: pid {} из записи — не наш node.exe ({}). Остановка пропущена, запись оставлена.",
                router_id,
                rec.pid,
                node.display()
            );
            return Err(format!(
                "Порт {} держит чужой процесс (pid {}), не {}. Остановите его или смените порт.",
                port, rec.pid, router_id
            ));
        }
    }

    // 2. Учёт текущей сессии: мёртвые чистим, живых чужих (переиспользованный ОС
    // pid) вычёркиваем без kill, своих — в кандидаты.
    {
        let mut active = active_pids()[router_id.index()].lock().unwrap();
        active.retain(|pid| {
            if record_pid == Some(*pid) || targets.contains(pid) {
                return true;
            }
            if !pid_alive(*pid) {
                return false;
            }
            if process_exe_matches(*pid, &node) {
                targets.push(*pid);
                true
            } else {
                if foreign.is_none() {
                    foreign = Some(*pid);
                }
                false
            }
        });
    }

    // 3. Владелец порта: свой node.exe — кандидат; чужой процесс — не убивать.
    if let Some(pid) = listener_pid(port) {
        if pid != 0 && !targets.contains(&pid) {
            if process_exe_matches(pid, &node) {
                targets.push(pid);
            } else if foreign.is_none() {
                foreign = Some(pid);
            }
        }
    }

    if targets.is_empty() {
        // Протухший учёт (чужой живой pid в ACTIVE) к этому месту уже вычищен.
        // Вердикт — по порту, а не по учёту: порт закрыт — останавливать нечего
        // (это Ok, а не «чужой держит порт»); порт открыт — он чужой, не убиваем.
        if port_open(port, Duration::from_millis(400)) {
            if let Some(pid) = foreign {
                if pid != 0 {
                    return Err(format!(
                        "Порт {} держит чужой процесс (pid {}), не {}. Остановите его или смените порт.",
                        port, pid, router_id
                    ));
                }
            }
            return Err(format!(
                "Порт {} открыт, но {} на нём не отвечает. Возможно, порт занят другим сервисом — смените порт в настройках.",
                port, router_id
            ));
        }
        log::info!("{}: нечего останавливать (порт {} закрыт)", router_id, port);
        return Ok(());
    }

    // 4. Kill своих + sweep остатков того же node.exe, затем верификация.
    let mut reports: Vec<KillReport> = Vec::new();
    for pid in &targets {
        log::info!("🛑 {}: остановка роутера (pid {})", router_id, pid);
        reports.push(kill_pid_tree(*pid));
    }
    kill_node_processes(&node);

    let port_closed = wait_port_closed(port, STOP_WAIT);
    let node_gone = wait_node_gone(&node, STOP_WAIT);

    // 5. Учёт: мёртвых вычёркиваем, живых оставляем на повтор.
    {
        let mut active = active_pids()[router_id.index()].lock().unwrap();
        active.retain(|pid| pid_alive(*pid));
    }

    if port_closed && node_gone {
        // Запись снимаем только если её pid мёртв: чужую живую запись мы выше
        // уже отвергли отказом и сюда не доходим.
        if let Some(rec) = read_server_record(data_dir) {
            if !pid_alive(rec.pid) {
                clear_server_record(&data_dir);
            }
        }
        log::info!("✅ {} остановлен (порт {} закрыт)", router_id, port);
        Ok(())
    } else {
        let alive: Vec<u32> = targets.iter().copied().filter(|p| pid_alive(*p)).collect();
        let rep = reports
            .iter()
            .map(|r| format!("pid {}: {} ({})", r.pid, if r.succeeded { "ok" } else { "отказ" }, r.detail))
            .collect::<Vec<_>>()
            .join("; ");
        log::warn!(
            "⚠️ {}: порт {} не закрылся за {} сек (порт закрыт: {}, node ушёл: {}). Живы: {:?}. taskkill: {}",
            router_id,
            port,
            STOP_WAIT.as_secs(),
            port_closed,
            node_gone,
            alive,
            rep
        );
        Err(format!(
            "{} не остановился за {} сек: порт {} {}. Живые pid: {:?}. Причина kill: {}. Повторите остановку или убейте процесс вручную.",
            router_id,
            STOP_WAIT.as_secs(),
            port,
            if port_closed { "закрыт, но процессы node.exe живы" } else { "остался открыт" },
            alive,
            if rep.is_empty() { "—".to_string() } else { rep }
        ))
    }
}

pub fn reconcile_server_state(app: &AppHandle, router_id: RouterId) {
    let cfg = crate::router::config::load_config(app, router_id);
    let port = cfg.port_or_default(router_id);
    let dir = crate::router::config::router_dir(app, router_id);
    let data_dir = router_data_dir(app, router_id);

    if let Some(rec) = read_server_record(&data_dir) {
        if pid_alive(rec.pid) {
            log::info!("cloud-routers: обнаружен работающий наш сервер (pid {})", rec.pid);
            return;
        } else {
            clear_server_record(&data_dir);
        }
    }

    match gateway_state(router_id, port, &dir, &data_dir) {
        GatewayState::ForeignOccupant(pid) if pid != 0 => {
            if process_exe_matches(pid, &node_exe(&dir)) {
                log::warn!("cloud-routers: обнаружен старый бесхозный инстанс node.exe на порту {} (pid {}), останавливаем...", port, pid);
                if let Err(e) = stop_server(router_id, &dir, port, &data_dir) {
                    log::warn!("cloud-routers: бесхозный инстанс не остановился: {}", e);
                }
            }
        }
        _ => {}
    }
}

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
            cmd.creation_flags(0x08000000);
        }
        match cmd.output() {
            Ok(out) if !out.status.success() => log::warn!(
                "sweep node.exe ({}): powershell вернул {}: {}",
                target.display(),
                out.status,
                truncate_detail(&console_lossy(&out.stderr), 300)
            ),
            Err(e) => log::warn!("sweep node.exe ({}): не запустился powershell: {}", target.display(), e),
            _ => {}
        }
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
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
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
    #[derive(Clone, Copy)]
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
    use super::*;
    use crate::commands::RouterId;

    #[test]
    fn windows_local_port_decodes_network_byte_order() {
        assert_eq!(decode_local_port(20_128), 0xA04E);
        assert_ne!(decode_local_port(20_128), 20_128);
    }

    /// CP866-байты «Ошибка: Отказано в доступе.» (вывод taskkill по-русски)
    /// декодируются читаемо, а не остаются кракозябрами UTF-8-lossy.
    /// Proof 22.10.26: старый `from_utf8_lossy` давал «�訡��: ...» в last_logs.txt.
    #[test]
    fn console_lossy_decodes_cp866_taskkill_error_readably() {
        // «Ошибка: Не удалось завершить процесс ... Причина: Отказано в доступе.»
        // «Ошибка: » (О=0x8E, ш=0xE8, и=0xA8, б=0xA1, к=0xAA, а=0xA0)
        let cp866: &[u8] = &[
            0x8E, 0xE8, 0xA8, 0xA1, 0xAA, 0xA0, 0x3A, 0x20,
        ];
        let decoded = console_lossy(cp866);
        assert!(decoded.starts_with("Ошибка:"), "получено: {:?}", decoded);
        // На Windows байты всегда трактуются как CP866 (консольная OEM-страница),
        // поэтому сквозной UTF-8 там не ожидается; проверяем его только для
        // не-Windows ветки (там функция — from_utf8_lossy).
        if !cfg!(windows) {
            let utf8 = "шлюз остановлен ✔";
            assert_eq!(console_lossy(utf8.as_bytes()), utf8);
        }
    }

    /// Свободный порт для тестов: занимаем эфемерный, отпускаем, проверяем что закрыт.
    fn free_test_port() -> u16 {
        for _ in 0..20 {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = listener.local_addr().expect("addr").port();
            drop(listener);
            if !port_open(port, Duration::from_millis(200)) {
                return port;
            }
        }
        panic!("не нашлось свободного порта для теста");
    }

    fn unique_scratch(tag: &str) -> std::path::PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        crate::router::test_scratch::scratch_dir(&format!("cr_proc_{}_{}", tag, seq))
    }

    #[test]
    fn kill_pid_tree_reports_failure_for_bogus_pid() {
        // Заведомо мёртвый pid: убивать нечего, отчёт обязан честно сказать «отказ».
        // Именно потеря этого отчёта (`let _ =`) делала причину инцидента 16.10.26
        // неустановимой. Текст не проверяем — он зависит от локали ОС.
        let rep = kill_pid_tree(u32::MAX - 7);
        assert_eq!(rep.pid, u32::MAX - 7);
        assert!(!rep.succeeded, "kill несуществующего pid обязан вернуть отказ");
        assert!(!rep.detail.is_empty(), "у отказа обязана быть причина");
    }

    #[test]
    fn stop_without_record_is_ok_on_closed_port() {
        let data_dir = unique_scratch("norecord_data");
        let router_dir = unique_scratch("norecord_inst");
        let port = free_test_port();
        stop_server(RouterId::NineRouter, &router_dir, port, &data_dir)
            .expect("нечего останавливать — Ok");
        assert!(
            !crate::router::config::server_record_path(&data_dir).exists(),
            "записи не было — появляться ей неоткуда"
        );
    }

    #[test]
    fn stop_clears_stale_record() {
        let data_dir = unique_scratch("stale_data");
        let router_dir = unique_scratch("stale_inst");
        std::fs::create_dir_all(&data_dir).expect("mkdir");
        crate::router::config::write_server_record(&data_dir, 0, 59997, None);
        let port = free_test_port();
        stop_server(RouterId::NineRouter, &router_dir, port, &data_dir)
            .expect("протухшая запись — Ok");
        assert!(
            !crate::router::config::server_record_path(&data_dir).exists(),
            "запись о мёртвом процессе обязана быть убрана"
        );
    }

    #[test]
    fn stop_refuses_foreign_exe_and_keeps_record() {
        // Ключевая гарантия безопасности (симметрично шлюзу): pid из записи,
        // чей exe — не наш node.exe, убивать нельзя. Здесь pid — сам тестовый
        // процесс, то есть заведомо чужой: остановка обязана отказать Err,
        // запись — пережить отказ, а процесс — остаться живым (иначе прогон бы оборвался).
        let data_dir = unique_scratch("foreign_data");
        let router_dir = unique_scratch("foreign_inst");
        std::fs::create_dir_all(&data_dir).expect("mkdir");
        let my_pid = std::process::id();
        crate::router::config::write_server_record(&data_dir, my_pid, 59996, None);
        let port = free_test_port();
        let err = stop_server(RouterId::NineRouter, &router_dir, port, &data_dir)
            .expect_err("чужой pid убивать нельзя");
        assert!(err.contains("чужой"), "ошибка обязана называть причину: {}", err);
        let rec = crate::router::config::read_server_record(&data_dir).expect("запись цела");
        assert_eq!(rec.pid, my_pid, "запись о живом процессе обязана пережить отказ");
        assert!(pid_alive(my_pid), "тестовый процесс обязан быть жив — его не убили");
    }

    #[test]
    fn stop_one_router_keeps_other_router_pids() {
        // Глобальный clear() стирал учёт соседних роутеров. Теперь учёт
        // per-router: stop(NineRouter) не трогает ACTIVE[OmniRoute].
        let bogus = u32::MAX - 13;
        register_server_pid(RouterId::OmniRoute, bogus);
        let data_dir = unique_scratch("isol_data");
        let router_dir = unique_scratch("isol_inst");
        let port = free_test_port();
        stop_server(RouterId::NineRouter, &router_dir, port, &data_dir)
            .expect("нечего останавливать — Ok");
        assert!(
            test_active_snapshot(RouterId::OmniRoute).contains(&bogus),
            "учёт соседнего роутера обязан пережить чужую остановку"
        );
        unregister_server_pid(RouterId::OmniRoute, bogus);
    }
}

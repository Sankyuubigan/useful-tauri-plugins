//! Уровни 3–6: внешние процессы (PowerShell, bitsadmin, MCP/Deno, Chrome CDP).
//! Общие правила: CREATE_NO_WINDOW, общий deadline + stall-поллинг размера файла,
//! прогресс через поллинг (process-фоллбэки не дают потокового прогресса).

use super::progress::TaskHandle;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Нет роста файла дольше — убиваем процесс и отдаём ошибку уровню.
const STALL_KILL: Duration = Duration::from_secs(60);
/// Абсолютный потолок на один процесс-уровень (страховка).
const LEVEL_DEADLINE: Duration = Duration::from_secs(1800);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

fn spawn_hidden(program: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.kill_on_drop(true);
    cmd
}

/// Поллинг размера dest, пока процесс не завершится. Возвращает итоговый размер.
/// При stall/deadline/cancel — kill и Err.
async fn wait_with_progress(
    task: &TaskHandle,
    mut child: tokio::process::Child,
    dest: &Path,
    level_name: &str,
) -> Result<u64, String> {
    let pid = child.id();
    if let Some(pid) = pid {
        task.set_child_pid(pid);
    }
    let deadline = Instant::now() + LEVEL_DEADLINE;
    let mut last_size: u64 = 0;
    let mut last_growth = Instant::now();
    let mut last_bytes_emitted: u64 = 0;
    let mut last_emit = Instant::now();

    loop {
        if task.is_cancelled() {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err("Отменено".into());
        }
        if Instant::now() > deadline {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(format!("{}: превышен лимит времени", level_name));
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                // Процесс завершился — финальный поллинг размера.
                let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
                task.emit_progress(size, size, 0.0, level_name, true);
                if status.success() {
                    if size == 0 {
                        return Err(format!("{}: файл пуст после завершения", level_name));
                    }
                    return Ok(size);
                }
                return Err(format!("{}: код возврата {:?}", level_name, status.code()));
            }
            Ok(None) => {
                // Ещё работает — поллим размер.
                let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
                if size > last_size {
                    last_size = size;
                    last_growth = Instant::now();
                }
                if last_growth.elapsed() > STALL_KILL {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(format!(
                        "{}: зависание (нет роста файла {} с)",
                        level_name,
                        STALL_KILL.as_secs()
                    ));
                }
                if last_emit.elapsed() >= Duration::from_millis(200) {
                    let dt = last_emit.elapsed().as_secs_f64();
                    let speed = (size.saturating_sub(last_bytes_emitted)) as f64 / dt;
                    task.emit_progress(size, 0, speed, level_name, false);
                    last_bytes_emitted = size;
                    last_emit = Instant::now();
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            Err(e) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(format!("{}: ошибка ожидания процесса: {}", level_name, e));
            }
        }
    }
}

// ────────────────────────────── Level 3: PowerShell ──────────────────────────────

pub async fn powershell(task: &TaskHandle, url: &str, dest: &Path) -> Result<u64, String> {
    let dest_str = dest.display().to_string();
    let url_escaped = url.replace('\'', "''");
    let dest_escaped = dest_str.replace('\'', "''");
    let ps_script = format!(
        "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
         $ProgressPreference = 'SilentlyContinue'; \
         Invoke-WebRequest -Uri '{url}' -OutFile '{dest}' -UseBasicParsing; \
         (Get-Item '{dest}').Length",
        url = url_escaped,
        dest = dest_escaped,
    );
    // Частичный файл от уровня 1 мешает — убираем.
    let _ = std::fs::remove_file(super::reqwest_dl::part_path(dest));
    let _ = std::fs::remove_file(dest);

    let child = spawn_hidden("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &ps_script,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("PowerShell не найден: {}", e))?;

    wait_with_progress(task, child, dest, "PowerShell").await
}

// ────────────────────────────── Level 4: bitsadmin ──────────────────────────────

pub async fn bitsadmin(task: &TaskHandle, url: &str, dest: &Path) -> Result<u64, String> {
    let dest_str = dest.display().to_string();
    let job_name = format!(
        "king_orch_dl_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let _ = std::fs::remove_file(super::reqwest_dl::part_path(dest));
    let _ = std::fs::remove_file(dest);

    let child = spawn_hidden("bitsadmin")
        .args(["/transfer", &job_name, "/download", "/priority", "high", url, &dest_str])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("bitsadmin не найден: {}", e))?;

    wait_with_progress(task, child, dest, "bitsadmin").await
}

// ────────────────────────────── Level 5: MCP/Deno ──────────────────────────────

pub async fn mcp_deno(task: &TaskHandle, url: &str, dest: &Path) -> Result<u64, String> {
    let script = find_mcp_script()?;
    let deno = find_deno()?;
    let dest_str = dest.display().to_string();
    let _ = std::fs::remove_file(super::reqwest_dl::part_path(dest));
    let _ = std::fs::remove_file(dest);

    let child = spawn_hidden(&deno.to_string_lossy())
        .args([
            "run",
            "--allow-all",
            "--no-lock",
            script.to_str().unwrap_or(""),
            "--download-cli",
            url,
            &dest_str,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Не удалось запустить deno: {}", e))?;

    wait_with_progress(task, child, dest, "MCP/Deno").await
}

fn find_mcp_script() -> Result<PathBuf, String> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| "Не удалось определить папку приложения".to_string())?;
    let script = exe_dir.join("mcp_servers").join("downloader.ts");
    if script.exists() {
        return Ok(script);
    }
    let dev = exe_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("mcp_servers").join("downloader.ts"))
        .filter(|p| p.exists())
        .ok_or_else(|| "MCP-сервер downloader.ts не найден".to_string())?;
    Ok(dev)
}

fn find_deno() -> Result<PathBuf, String> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| "Не удалось определить папку приложения".to_string())?;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    for rel in [format!("bins/deno{}", suffix), format!("llamacpp/deno{}", suffix)] {
        let p = exe_dir.join(&rel);
        if p.exists() {
            return Ok(p);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-Command",
            "(Get-Command deno -ErrorAction SilentlyContinue).Source",
        ]);
        cmd.creation_flags(0x08000000);
        if let Ok(out) = cmd.output() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                let p = PathBuf::from(&path);
                if p.exists() {
                    return Ok(p);
                }
            }
        }
    }
    Err("Deno не найден (нужен для MCP-фоллбэка)".into())
}

// ────────────────────────────── Level 6: Chrome CDP (делегируется) ──────────────────────────────

pub async fn chrome_cdp(task: &TaskHandle, url: &str, dest: &Path) -> Result<u64, String> {
    super::chrome_cdp::chrome_cdp(task, url, dest).await
}

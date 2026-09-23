//! Уровень 6: Chrome CDP (headless Chrome, BoringSSL — обход DPI).
//! CREATE_NO_WINDOW, таймауты, поллинг размера для прогресса.

use super::progress::TaskHandle;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::AsyncBufReadExt;

const STALL_KILL: Duration = Duration::from_secs(60);
const LEVEL_DEADLINE: Duration = Duration::from_secs(600);

fn spawn_hidden(program: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }
    cmd.kill_on_drop(true);
    cmd
}

pub async fn chrome_cdp(task: &TaskHandle, url: &str, dest: &Path) -> Result<u64, String> {
    let chrome_exe = find_chrome()?;
    let temp_dir = std::env::temp_dir().join(format!(
        "king_orch_chrome_dl_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Ошибка temp-директории: {}", e))?;

    let _ = std::fs::remove_file(super::reqwest_dl::part_path(dest));
    let _ = std::fs::remove_file(dest);

    let mut child = spawn_hidden(&chrome_exe.to_string_lossy())
        .args([
            "--headless=new",
            "--no-sandbox",
            "--disable-gpu",
            "--disable-extensions",
            "--remote-debugging-port=0",
            &format!("--user-data-dir={}", temp_dir.display()),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Не удалось запустить Chrome: {}", e))?;

    if let Some(pid) = child.id() {
        task.set_child_pid(pid);
    }

    let stdout = child.stdout.take().ok_or("Нет stdout у Chrome")?;
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let ws_url = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(line) = reader
            .next_line()
            .await
            .map_err(|e| format!("Ошибка чтения stdout: {}", e))?
        {
            if line.contains("DevTools listening on") {
                return Ok::<String, String>(line.replace("DevTools listening on ", ""));
            }
        }
        Err("Chrome не вывел DevTools listening".to_string())
    })
    .await
    .map_err(|_| "Таймаут ожидания Chrome DevTools".to_string())??;

    let result = cdp_download_file(task, &ws_url, url, dest).await;

    let _ = child.kill().await;
    let _ = child.wait().await;
    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

async fn cdp_download_file(
    task: &TaskHandle,
    ws_url: &str,
    download_url: &str,
    dest: &Path,
) -> Result<u64, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let ws_addr = ws_url
        .strip_prefix("ws://")
        .or_else(|| ws_url.strip_prefix("wss://"))
        .ok_or_else(|| format!("Некорректный WebSocket URL: {}", ws_url))?;

    let stream = tokio::net::TcpStream::connect(ws_addr)
        .await
        .map_err(|e| format!("TCP connect к Chrome failed: {}", e))?;

    let key = base64_encode(&rand_bytes(16));
    let request = format!(
        "GET / HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\n\r\n",
        ws_addr, key
    );

    let (mut reader, mut writer) = tokio::io::split(stream);
    writer
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("WebSocket handshake write: {}", e))?;

    let mut buf_reader = tokio::io::BufReader::new(&mut reader);
    let mut headers_done = false;
    while !headers_done {
        let mut line = String::new();
        buf_reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("WebSocket handshake read: {}", e))?;
        if line.trim().is_empty() || line == "\r\n" {
            headers_done = true;
        }
    }

    let set_download_cmd = serde_json::json!({
        "id": 1,
        "method": "Browser.setDownloadBehavior",
        "params": {
            "behavior": "allow",
            "downloadPath": dest.parent().unwrap_or(Path::new(".")).to_string_lossy(),
            "eventsEnabled": true
        }
    });
    send_cdp_message(&mut writer, &set_download_cmd.to_string()).await?;

    let navigate_cmd = serde_json::json!({
        "id": 2,
        "method": "Page.navigate",
        "params": { "url": download_url }
    });
    send_cdp_message(&mut writer, &navigate_cmd.to_string()).await?;

    let deadline = Instant::now() + LEVEL_DEADLINE;
    let mut last_size: u64 = 0;
    let mut last_growth = Instant::now();
    let mut last_emit = Instant::now();
    let mut last_bytes_emitted: u64 = 0;
    // Имя файла Chrome определяет сам — поллим каталог dest.parent() по URL-хвосту.
    let url_tail = download_url
        .rsplit('/')
        .next()
        .and_then(|s| s.split('?').next())
        .unwrap_or("download.bin")
        .to_string();
    let parent = dest.parent().unwrap_or(Path::new("."));
    let candidates = [dest.to_path_buf(), parent.join(&url_tail)];

    loop {
        if task.is_cancelled() {
            return Err("Отменено".into());
        }
        if Instant::now() > deadline {
            return Err("Chrome CDP: превышено время скачивания".into());
        }

        let mut size: u64 = 0;
        for c in &candidates {
            if let Ok(m) = std::fs::metadata(c) {
                if m.len() > size {
                    size = m.len();
                }
            }
        }
        if size > last_size {
            last_size = size;
            last_growth = Instant::now();
            // Если файл появился не по dest — перемещаем.
            for c in &candidates {
                if *c != dest && c.exists() {
                    if let Ok(m) = std::fs::metadata(c) {
                        if m.len() == size {
                            let _ = std::fs::rename(c, dest);
                            break;
                        }
                    }
                }
            }
        }
        if last_growth.elapsed() > STALL_KILL && size > 0 {
            return Err(format!(
                "Chrome CDP: зависание (нет роста {} с)",
                STALL_KILL.as_secs()
            ));
        }
        if last_emit.elapsed() >= Duration::from_millis(200) {
            let dt = last_emit.elapsed().as_secs_f64();
            let speed = (size.saturating_sub(last_bytes_emitted)) as f64 / dt;
            task.emit_progress(size, 0, speed, "Chrome CDP", false);
            last_bytes_emitted = size;
            last_emit = Instant::now();
        }

        // Ждём WS-события с коротким таймаутом, чтобы не занимать поток.
        let _ = tokio::time::timeout(
            Duration::from_millis(500),
            buf_reader.read_line(&mut String::new()),
        )
        .await;

        if dest.exists() {
            if let Ok(m) = std::fs::metadata(dest) {
                if m.len() > 0 && last_growth.elapsed() > Duration::from_secs(3) {
                    // Файл стабилен 3 сек — считаем завершённым.
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let final_size =
                        std::fs::metadata(dest).map(|m| m.len()).unwrap_or(m.len());
                    if final_size > 0 {
                        task.emit_progress(final_size, final_size, 0.0, "Chrome CDP done", true);
                        return Ok(final_size);
                    }
                }
            }
        }
    }
}

async fn send_cdp_message(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    msg: &str,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    let payload = msg.as_bytes();
    let len = payload.len();
    let mut frame = vec![0x81u8];
    let mask = rand_bytes(4);

    if len < 126 {
        frame.push(0x80 | (len as u8));
    } else if len < 65536 {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    let masked: Vec<u8> = payload.iter().enumerate().map(|(i, &b)| b ^ mask[i % 4]).collect();
    frame.extend_from_slice(&masked);

    writer
        .write_all(&frame)
        .await
        .map_err(|e| format!("CDP send: {}", e))
}

fn find_chrome() -> Result<PathBuf, String> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| "Не удалось определить папку приложения".to_string())?;
    let bins = exe_dir.join("bins");
    for sub in ["cloak", "chrome"] {
        if let Some(exe) = find_chrome_in_dir(&bins.join(sub)) {
            return Ok(exe);
        }
    }
    for path in [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ] {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }
    Err("Chrome не найден. Скачайте CloakBrowser или Chrome-for-Testing.".into())
}

fn find_chrome_in_dir(dir: &Path) -> Option<PathBuf> {
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).ok()?;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase());
                if name.as_deref() == Some("chrome.exe") || name.as_deref() == Some("chromium.exe")
                {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn rand_bytes(n: usize) -> Vec<u8> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut hasher = s.build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );
    let seed = hasher.finish();
    (0..n).map(|i| ((seed >> (i * 8)) & 0xFF) as u8).collect()
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

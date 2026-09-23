//! Оркестрация уровней: reqwest → curl → PowerShell → bitsadmin → MCP → Chrome.
//! Каждый уровень получает task для прогресса/отмены; при ошибке — лог и следующий.

use super::progress::TaskHandle;
use std::path::Path;

pub struct ChainOutcome {
    pub bytes: u64,
    pub total: u64,
    pub level: u32,
    pub level_name: &'static str,
}

/// Скачать файл, перебирая уровни. `expected_size` — если известен (для прогресса).
pub async fn download_with_fallback(
    task: &TaskHandle,
    url: &str,
    dest: &Path,
    expected_size: Option<u64>,
    on_log: &(dyn Fn(String) + Send + Sync),
) -> Result<ChainOutcome, String> {
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut errors: Vec<String> = Vec::new();

    // ── 1/6 reqwest ──
    task.set_level(1, "reqwest");
    on_log("🔄 [1/6] Скачивание через HTTP (reqwest multi-chunk)...".into());
    task.emit_progress(0, expected_size.unwrap_or(0), 0.0, "reqwest", true);
    match super::reqwest_dl::download(task, url, dest).await {
        Ok((bytes, total)) => {
            on_log(format!("✅ [1/6] reqwest: {} МБ", bytes / 1024 / 1024));
            return Ok(ChainOutcome {
                bytes,
                total: total.max(bytes),
                level: 1,
                level_name: "reqwest",
            });
        }
        Err(e) if e == "Отменено" => return Err(e),
        Err(e) => {
            on_log(format!("⚠️ [1/6] reqwest не смог: {}", e));
            errors.push(format!("reqwest: {}", e));
        }
    }
    task.check_cancel()?;

    // ── 2/6 curl (синхронный libcurl; block_in_place, чтобы не блокировать рантайм) ──
    task.set_level(2, "curl");
    on_log("🔄 [2/6] Скачивание через curl (libcurl)...".into());
    task.emit_progress(0, expected_size.unwrap_or(0), 0.0, "curl", true);
    let curl_res: Result<(u64, u64), String> =
        tokio::task::block_in_place(|| super::curl_dl::download(task, url, dest));
    match curl_res {
        Ok((bytes, total)) => {
            on_log(format!("✅ [2/6] curl: {} МБ", bytes / 1024 / 1024));
            return Ok(ChainOutcome {
                bytes,
                total: total.max(bytes),
                level: 2,
                level_name: "curl",
            });
        }
        Err(e) if e == "Отменено" => return Err(e),
        Err(e) => {
            on_log(format!("⚠️ [2/6] curl не смог: {}", e));
            errors.push(format!("curl: {}", e));
        }
    }
    task.check_cancel()?;

    // ── 3/6 PowerShell ──
    task.set_level(3, "PowerShell");
    on_log("🔄 [3/6] Скачивание через PowerShell...".into());
    task.emit_progress(0, expected_size.unwrap_or(0), 0.0, "PowerShell", true);
    match super::process_dl::powershell(task, url, dest).await {
        Ok(bytes) => {
            on_log(format!("✅ [3/6] PowerShell: {} МБ", bytes / 1024 / 1024));
            return Ok(ChainOutcome {
                bytes,
                total: expected_size.unwrap_or(bytes),
                level: 3,
                level_name: "PowerShell",
            });
        }
        Err(e) if e == "Отменено" => return Err(e),
        Err(e) => {
            on_log(format!("⚠️ [3/6] PowerShell не смог: {}", e));
            errors.push(format!("PowerShell: {}", e));
        }
    }
    task.check_cancel()?;

    // ── 4/6 bitsadmin ──
    task.set_level(4, "bitsadmin");
    on_log("🔄 [4/6] Скачивание через bitsadmin (WinINet)...".into());
    task.emit_progress(0, expected_size.unwrap_or(0), 0.0, "bitsadmin", true);
    match super::process_dl::bitsadmin(task, url, dest).await {
        Ok(bytes) => {
            on_log(format!("✅ [4/6] bitsadmin: {} МБ", bytes / 1024 / 1024));
            return Ok(ChainOutcome {
                bytes,
                total: expected_size.unwrap_or(bytes),
                level: 4,
                level_name: "bitsadmin",
            });
        }
        Err(e) if e == "Отменено" => return Err(e),
        Err(e) => {
            on_log(format!("⚠️ [4/6] bitsadmin не смог: {}", e));
            errors.push(format!("bitsadmin: {}", e));
        }
    }
    task.check_cancel()?;

    // ── 5/6 MCP/Deno ──
    task.set_level(5, "MCP/Deno");
    on_log("🔄 [5/6] Скачивание через MCP (Deno)...".into());
    task.emit_progress(0, expected_size.unwrap_or(0), 0.0, "MCP/Deno", true);
    match super::process_dl::mcp_deno(task, url, dest).await {
        Ok(bytes) => {
            on_log(format!("✅ [5/6] MCP: {} МБ", bytes / 1024 / 1024));
            return Ok(ChainOutcome {
                bytes,
                total: expected_size.unwrap_or(bytes),
                level: 5,
                level_name: "MCP/Deno",
            });
        }
        Err(e) if e == "Отменено" => return Err(e),
        Err(e) => {
            on_log(format!("⚠️ [5/6] MCP не смог: {}", e));
            errors.push(format!("MCP: {}", e));
        }
    }
    task.check_cancel()?;

    // ── 6/6 Chrome CDP ──
    task.set_level(6, "Chrome CDP");
    on_log("🔄 [6/6] Скачивание через Chrome (CDP)...".into());
    task.emit_progress(0, expected_size.unwrap_or(0), 0.0, "Chrome CDP", true);
    match super::process_dl::chrome_cdp(task, url, dest).await {
        Ok(bytes) => {
            on_log(format!("✅ [6/6] Chrome CDP: {} МБ", bytes / 1024 / 1024));
            return Ok(ChainOutcome {
                bytes,
                total: expected_size.unwrap_or(bytes),
                level: 6,
                level_name: "Chrome CDP",
            });
        }
        Err(e) if e == "Отменено" => return Err(e),
        Err(e) => {
            on_log(format!("⚠️ [6/6] Chrome CDP не смог: {}", e));
            errors.push(format!("Chrome CDP: {}", e));
        }
    }

    Err(format!(
        "Все 6 методов скачивания не сработали. Проверьте интернет/прокси.\n{}",
        errors.join("; ")
    ))
}

//! Управление yt-dlp: установка, обновление, проверка статуса, Deno, FFmpeg.
//!
//! Аналогично tauri-plugin-llama-engine: бинарник не бандлится в приложение,
//! скачивается из GitHub Releases при установке.

use crate::commands::YtdlpStatus;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::AppHandle;

const MIN_STANDALONE_SIZE: u64 = 1_000_000;
const GITHUB_RELEASES_API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const GITHUB_DOWNLOAD_BASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";

pub fn default_dir(exe_dir: &Path) -> PathBuf {
    exe_dir.join("ytdlp")
}

pub fn get_exe_path(dir: &Path) -> PathBuf {
    let exe_name = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };
    dir.join("bin").join(exe_name)
}

pub fn get_deno_path(dir: &Path) -> PathBuf {
    let exe_name = if cfg!(windows) { "deno.exe" } else { "deno" };
    dir.join("deno").join(exe_name)
}

pub fn get_ffmpeg_path() -> Option<PathBuf> {
    if cfg!(windows) {
        let ffmpeg = PathBuf::from("ffmpeg.exe");
        if ffmpeg.exists() {
            return Some(ffmpeg);
        }
        if let Ok(paths) = std::env::var("PATH") {
            for p in paths.split(';') {
                let candidate = Path::new(p).join("ffmpeg.exe");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    } else {
        let ffmpeg = PathBuf::from("ffmpeg");
        if ffmpeg.exists() {
            return Some(ffmpeg);
        }
        if let Ok(paths) = std::env::var("PATH") {
            for p in paths.split(':') {
                let candidate = Path::new(p).join("ffmpeg");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn no_window_command(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
}

pub fn is_installed(dir: &Path) -> bool {
    let exe = get_exe_path(dir);
    if !exe.exists() {
        return false;
    }
    match fs::metadata(&exe) {
        Ok(m) => m.len() > MIN_STANDALONE_SIZE,
        Err(_) => false,
    }
}

pub fn get_version(dir: &Path) -> Option<String> {
    let exe = get_exe_path(dir);
    if !exe.exists() {
        return None;
    }
    let mut cmd = Command::new(&exe);
    cmd.arg("--version");
    no_window_command(&mut cmd);
    match cmd.output() {
        Ok(out) if out.status.success() => {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        }
        _ => None,
    }
}

fn get_download_url() -> String {
    if cfg!(windows) {
        format!("{GITHUB_DOWNLOAD_BASE}/yt-dlp.exe")
    } else if cfg!(target_os = "macos") {
        format!("{GITHUB_DOWNLOAD_BASE}/yt-dlp_macos")
    } else {
        format!("{GITHUB_DOWNLOAD_BASE}/yt-dlp_linux")
    }
}

pub fn get_status(dir: &Path, _app: &AppHandle) -> Result<YtdlpStatus, String> {
    let exe = get_exe_path(dir);
    let installed = is_installed(dir);
    let version = if installed { get_version(dir) } else { None };

    let ffmpeg_path = get_ffmpeg_path();
    let ffmpeg_installed = ffmpeg_path.is_some();

    let deno_exe = get_deno_path(dir);
    let deno_installed = deno_exe.exists();

    let message = if installed {
        format!(
            "yt-dlp установлен (версия {})",
            version.as_deref().unwrap_or("неизвестна")
        )
    } else {
        "yt-dlp не установлен. Нажмите «Установить».".to_string()
    };

    Ok(YtdlpStatus {
        installed,
        version,
        exe_path: exe.to_string_lossy().to_string(),
        dir: dir.to_string_lossy().to_string(),
        ffmpeg_installed,
        ffmpeg_path: ffmpeg_path.map(|p| p.to_string_lossy().to_string()),
        deno_installed,
        deno_path: if deno_installed {
            Some(deno_exe.to_string_lossy().to_string())
        } else {
            None
        },
        message,
    })
}

pub fn install_or_update(dir: &Path, callback: &dyn Fn(String)) -> Result<(), String> {
    callback("Скачивание yt-dlp с GitHub...".to_string());

    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("Не удалось создать директорию: {e}"))?;

    let exe = get_exe_path(dir);
    let url = get_download_url();
    callback(format!("URL: {url}"));

    let temp_file = exe.with_extension("exe.part");
    let max_retries = 3;

    for attempt in 1..=max_retries {
        if max_retries > 1 {
            callback(format!("Скачивание yt-dlp... (попытка {attempt}/{max_retries})"));
        }

        match download_file(&url, &temp_file, callback) {
            Ok(_) => {
                let size = fs::metadata(&temp_file)
                    .map(|m| m.len())
                    .unwrap_or(0);
                if size < MIN_STANDALONE_SIZE {
                    let msg = format!(
                        "Скачанный файл слишком маленький ({size} байт). Возможно, неверная ссылка."
                    );
                    callback(format!("Ошибка: {msg}"));
                    let _ = fs::remove_file(&temp_file);
                    return Err(msg);
                }

                if exe.exists() {
                    let _ = fs::remove_file(&exe);
                }
                fs::rename(&temp_file, &exe)
                    .map_err(|e| format!("Ошибка переименования: {e}"))?;

                if !cfg!(windows) {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = fs::set_permissions(&exe, fs::Permissions::from_mode(0o755));
                    }
                }

                let version = get_version(dir);
                callback(format!(
                    "yt-dlp установлен! Версия: {}",
                    version.as_deref().unwrap_or("неизвестна")
                ));
                return Ok(());
            }
            Err(e) => {
                let _ = fs::remove_file(&temp_file);
                if attempt < max_retries {
                    callback(format!("Попытка {attempt} не удалась: {e}. Повтор..."));
                    std::thread::sleep(std::time::Duration::from_secs(2 * attempt as u64));
                    continue;
                }
                return Err(format!("Ошибка скачивания: {e}"));
            }
        }
    }

    Err("Не удалось скачать yt-dlp".to_string())
}

fn download_file(url: &str, dest: &Path, callback: &dyn Fn(String)) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Ошибка HTTP-клиента: {e}"))?;

    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| format!("Ошибка запроса: {e}"))?;

    let total = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut file = fs::File::create(dest)
        .map_err(|e| format!("Ошибка создания файла: {e}"))?;

    let mut buf = [0u8; 256 * 1024];
    let mut last_pct: Option<u32> = None;
    loop {
        let n = resp.read(&mut buf).map_err(|e| format!("Ошибка чтения: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("Ошибка записи: {e}"))?;
        downloaded += n as u64;
        if total > 0 {
            let pct = (downloaded * 100 / total) as u32;
            if last_pct != Some(pct) {
                last_pct = Some(pct);
                callback(format!("Скачивание yt-dlp... {pct}%"));
            }
        }
    }

    Ok(())
}

pub fn check_update(dir: &Path, callback: &dyn Fn(String)) -> Result<Option<String>, String> {
    let current = get_version(dir);
    if current.is_none() {
        callback("yt-dlp не установлен — обновление невозможно".to_string());
        return Ok(None);
    }

    callback("Проверка обновлений yt-dlp...".to_string());

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Ошибка HTTP-клиента: {e}"))?;

    let resp = client
        .get(GITHUB_RELEASES_API)
        .header("User-Agent", "tauri-plugin-ytdlp")
        .send()
        .map_err(|e| format!("Ошибка запроса: {e}"))?;

    let json: serde_json::Value = resp
        .json()
        .map_err(|e| format!("Ошибка парсинга JSON: {e}"))?;

    let latest = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Не удалось определить последнюю версию".to_string())?;

    let current = current.unwrap();
    if latest == current {
        callback("yt-dlp актуален".to_string());
        Ok(None)
    } else {
        callback(format!("Доступно обновление: {current} → {latest}"));
        Ok(Some(latest))
    }
}

pub fn install_deno(dir: &Path, callback: &dyn Fn(String)) -> Result<(), String> {
    callback("Скачивание Deno...".to_string());

    let deno_dir = dir.join("deno");
    fs::create_dir_all(&deno_dir)
        .map_err(|e| format!("Не удалось создать директорию: {e}"))?;

    let url = if cfg!(windows) {
        "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip"
    } else if cfg!(target_os = "macos") {
        "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-apple-darwin.zip"
    } else {
        "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip"
    };

    let temp_zip = deno_dir.join("deno.zip");
    download_file(url, &temp_zip, callback)?;

    callback("Распаковка Deno...".to_string());

    let file = fs::File::open(&temp_zip)
        .map_err(|e| format!("Ошибка открытия zip: {e}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Ошибка чтения zip: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| format!("Ошибка чтения архива: {e}"))?;
        let name = entry.name().to_string();
        let out_path = deno_dir.join(&name);

        if name.ends_with('/') {
            fs::create_dir_all(&out_path)
                .map_err(|e| format!("Ошибка создания директории: {e}"))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Ошибка создания директории: {e}"))?;
            }
            let mut out = fs::File::create(&out_path)
                .map_err(|e| format!("Ошибка создания файла: {e}"))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("Ошибка записи: {e}"))?;
        }
    }

    let _ = fs::remove_file(&temp_zip);

    if !cfg!(windows) {
        let deno_exe = get_deno_path(dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&deno_exe, fs::Permissions::from_mode(0o755));
        }
        let _ = deno_exe;
    }

    callback("Deno установлен!".to_string());
    Ok(())
}

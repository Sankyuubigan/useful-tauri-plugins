//! Скачивание YouTube-видео через yt-dlp subprocess с прогрессом.
//!
//! Логика перенесена из старого Python-модуля (youtube_worker.py):
//! 1. Анализ видео через --dump-json
//! 2. Скачивание с парсингом прогресса из stdout
//! 3. Поддержка отмены через kill_process_tree

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use regex::Regex;
use tauri::{AppHandle, Emitter};

use super::ytdlp_manager;

lazy_static::lazy_static! {
    /// Строка прогресса yt-dlp с `--newline`. Разбираем ТОЛЬКО процент:
    /// реальные строки бывают `of ~ 434.16MiB at 7.26MiB/s` (фрагменты HLS),
    /// `of 21.55MiB at Unknown B/s` (аудио) и `100% of ~ 21.55MiB in 01:32`
    /// (без `at`) — общий хвост у них разный, процент единственный стабильный.
    static ref PROGRESS_RE: Regex = Regex::new(r"^\[download\]\s+([\d.]+)%\s+of\b").unwrap();
    /// Размер: опциональная группа для текста статуса.
    static ref SIZE_RE: Regex = Regex::new(r"\bof\s+~?\s*([\d.]+)\s*(\w+)").unwrap();
    /// Скорость: опциональная группа (может быть `Unknown`).
    static ref SPEED_RE: Regex = Regex::new(r"\bat\s+([\d.]+)\s*(\w+)").unwrap();
    static ref DEST_RE: Regex = Regex::new(r"^\[download\]\s+Destination:\s+(.+)").unwrap();
}

static ACTIVE_CANCEL: AtomicBool = AtomicBool::new(false);

pub fn cancel_active() {
    ACTIVE_CANCEL.store(true, Ordering::SeqCst);
}

pub fn kill_active_downloads() {
    ACTIVE_CANCEL.store(true, Ordering::SeqCst);
}

fn no_window_command(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
}

/// Чтение потока построчно БЕЗ жёсткой UTF-8 валидации.
///
/// `BufRead::lines()` возвращает `Err(InvalidData)` на невалидном UTF-8, а
/// вызывающий на такой ошибке рвал цикл и ронял `ChildStdout` — pipe закрывался
/// раньше времени, и yt-dlp получал EINVAL при записи в stdout. Кодировка
/// stdout у yt-dlp.exe на Windows — cp1251, поэтому кириллическое название
/// видео ломало разбор. Здесь байты декодируются через `from_utf8_lossy`
/// (эквивалент `errors="replace"`), и цикл прерывается ТОЛЬКО на реальной
/// I/O-ошибке.
fn read_stream_lossy(reader: &mut impl BufRead, on_line: &mut dyn FnMut(&str)) {
    let mut raw: Vec<u8> = Vec::new();
    loop {
        raw.clear();
        match reader.read_until(b'\n', &mut raw) {
            Ok(0) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&raw);
                let line = line.trim_end_matches(['\r', '\n']);
                if !line.trim().is_empty() {
                    on_line(line);
                }
            }
            Err(e) => {
                log::error!("[YTDLP] Ошибка чтения потока: {e}");
                break;
            }
        }
    }
}

/// JS-рантаймы для n-sig / player challenge. Deno ставится плагином в
/// `<ytdlp>/deno/`, чего нет в PATH, поэтому путь передаётся явно.
fn js_runtimes(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    let deno = ytdlp_manager::get_deno_path(dir);
    if deno.exists() {
        out.push(format!("deno:{}", deno.display()));
    }

    let (node_name, sep) = if cfg!(windows) {
        ("node.exe", ';')
    } else {
        ("node", ':')
    };
    if let Ok(paths) = std::env::var("PATH") {
        for p in paths.split(sep) {
            let candidate = Path::new(p).join(node_name);
            if candidate.exists() {
                out.push(format!("node:{}", candidate.display()));
                break;
            }
        }
    }

    out
}

/// Текст строки статуса: «Скачивание: 46.3% · 21.55 MiB · 4.37 MiB/s».
/// Размер и скорость — опциональные группы: у аудио yt-dlp пишет
/// `at  Unknown B/s`, а финальная строка вообще обходится без `at`.
fn progress_status(line: &str, pct: f64) -> String {
    let mut parts = vec![format!("Скачивание: {pct:.1}%")];
    if let Some(m) = SIZE_RE.captures(line) {
        parts.push(format!("{} {}", &m[1], &m[2]));
    }
    if let Some(m) = SPEED_RE.captures(line) {
        parts.push(format!("{} {}/s", &m[1], &m[2]));
    }
    parts.join(" · ")
}

pub fn fetch_info(dir: &Path, url: &str) -> Result<serde_json::Value, String> {    let exe = ytdlp_manager::get_exe_path(dir);
    if !exe.exists() {
        return Err(format!(
            "yt-dlp не найден: {}\nУстановите yt-dlp в настройках.",
            exe.display()
        ));
    }

    let mut cmd = Command::new(&exe);
    cmd.args(["--dump-json", "--no-download", url]);
    no_window_command(&mut cmd);

    let output = cmd.output().map_err(|e| format!("Ошибка запуска yt-dlp: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp ошибка: {err}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("Ошибка парсинга JSON: {e}"))
}

#[allow(clippy::too_many_arguments)]
pub async fn start_download(
    app: AppHandle,
    dir: PathBuf,
    url: String,
    format: String,
    quality: String,
    output_dir: String,
    auth_mode: String,
    browser: Option<String>,
    cookies_file: Option<String>,
) -> Result<(), String> {
    ACTIVE_CANCEL.store(false, Ordering::SeqCst);

    let exe = ytdlp_manager::get_exe_path(&dir);
    if !exe.exists() {
        return Err(format!(
            "yt-dlp не найден: {}\nУстановите yt-dlp в настройках.",
            exe.display()
        ));
    }

    let res_str = if quality == "max" {
        String::new()
    } else {
        format!("[height<={}]", quality.replace('p', ""))
    };

    let mp4_fmt = format!(
        "bestvideo[ext=mp4]{}+bestaudio[ext=m4a]/best[ext=mp4]{}/best{}/best",
        res_str, res_str, res_str
    );
    let best_fmt = format!("bestvideo{}+bestaudio/best{}/best", res_str, res_str);

    let (fmt_args, merge_fmt): (Vec<&str>, Option<&str>) = match format.as_str() {
        "mp4" => (vec!["--format", &mp4_fmt], Some("mp4")),
        "mp3" => (
            vec![
                "--format",
                "bestaudio/best",
                "--extract-audio",
                "--audio-format",
                "mp3",
                "--audio-quality",
                "192",
            ],
            None,
        ),
        _ => (vec!["--format", &best_fmt], Some("mkv")),
    };

    let outtmpl = format!(
        "{}/%(title)s.%(ext)s",
        output_dir.replace('\\', "/")
    );

    let mut args: Vec<String> = Vec::new();
    args.extend(fmt_args.iter().map(|s| s.to_string()));
    if let Some(mf) = merge_fmt {
        args.push("--merge-output-format".to_string());
        args.push(mf.to_string());
    }
    args.push("-o".to_string());
    args.push(outtmpl);

    if let Some(ffmpeg_path) = ytdlp_manager::get_ffmpeg_path() {
        if let Some(parent) = ffmpeg_path.parent() {
            args.push("--ffmpeg-location".to_string());
            args.push(parent.to_string_lossy().to_string());
        }
    }

    args.push("--no-playlist".to_string());
    args.push("--socket-timeout".to_string());
    args.push("60".to_string());
    args.push("--retries".to_string());
    args.push("10".to_string());
    args.push("--newline".to_string());
    args.push("--no-warnings".to_string());

    let runtimes = js_runtimes(&dir);
    if runtimes.is_empty() {
        log::warn!(
            "[YTDLP] JS-рантайм не найден: подпись потоков (n-sig/player challenge) может не пройти. \
             Установите Deno в настройках загрузчика."
        );
    } else {
        log::info!("[YTDLP] JS-рантаймы: {}", runtimes.join(", "));
        args.push("--js-runtimes".to_string());
        args.push(runtimes.join(","));
    }

    match auth_mode.as_str() {
        "browser" => {
            if let Some(b) = &browser {
                args.push("--cookies-from-browser".to_string());
                args.push(b.clone());
            }
        }
        "file" => {
            if let Some(cf) = &cookies_file {
                args.push("--cookies".to_string());
                args.push(cf.clone());
            }
        }
        _ => {}
    }

    args.push(url.clone());

    log::info!("[YTDLP] Запуск: {} {}", exe.display(), args.join(" "));

    let mut cmd = Command::new(&exe);
    cmd.args(&args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    no_window_command(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| format!("Ошибка spawn: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "у yt-dlp отсутствует stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "у yt-dlp отсутствует stderr".to_string())?;

    let app_clone = app.clone();

    // stderr нужно читать непрерывно: буфер stderr на Windows — 64 КБ, и если
    // процесс пишет больше, он зависает НАВСЕГДА (таймаута нет). Раньше stderr
    // читался только после child.wait() — это гарантированный дедлок.
    let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let stderr_lines_sink = stderr_lines.clone();
    let stderr_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut sink = |line: &str| {
            log::info!("[YTDLP][stderr] {line}");
            match stderr_lines_sink.lock() {
                Ok(mut v) => v.push(line.to_string()),
                Err(poisoned) => poisoned.into_inner().push(line.to_string()),
            }
        };
        read_stream_lossy(&mut reader, &mut sink);
    });

    let mut reader = BufReader::new(stdout);
    let mut last_percent = -1;
    let mut merge_announced = false;

    let mut on_stdout_line = |line: &str| {
        if let Some(m) = PROGRESS_RE.captures(line) {
            // Процент дробный ("0.2", "10.0") — f64, потом округляем. События
            // шлём только при смене целого процента: иначе тысячи IPC-вызовов.
            let pct: f64 = m[1].parse().unwrap_or(0.0);
            let pct_int = pct.round() as i32;
            if pct_int != last_percent {
                last_percent = pct_int;
                let _ = app_clone.emit("ytdlp:download-progress", pct_int);
                let _ = app_clone.emit("ytdlp:download-status", progress_status(line, pct));
            }
            if pct >= 100.0 && !merge_announced {
                merge_announced = true;
                let _ = app_clone.emit("ytdlp:download-progress", 100);
                let _ = app_clone.emit("ytdlp:download-status", "Обработка (ffmpeg)...");
                log::info!("[YTDLP] Загрузка завершена. Склейка (если нужно)...");
            }
            // Прогресс — телеметрия для UI, а не запись лога: в лог он не пишется
            // (на 400 МБ это ~3000 строк, «срач» в test/last_logs.txt).
            return;
        }

        if DEST_RE.is_match(line) {
            // Новый файл (поток): у него свой прогресс с нуля.
            last_percent = -1;
        }

        log::info!("[YTDLP] {line}");
    };

    let mut cancelled = false;
    let mut read_error: Option<String> = None;
    loop {
        let mut raw: Vec<u8> = Vec::new();
        let n = match reader.read_until(b'\n', &mut raw) {
            Ok(n) => n,
            Err(e) => {
                log::error!("[YTDLP] Ошибка чтения stdout: {e}");
                read_error = Some(e.to_string());
                break;
            }
        };
        if n == 0 {
            break;
        }

        if ACTIVE_CANCEL.load(Ordering::SeqCst) {
            let _ = child.kill();
            cancelled = true;
            break;
        }

        let line = String::from_utf8_lossy(&raw);
        on_stdout_line(line.trim_end_matches(['\r', '\n']));
    }

    let status = child.wait().map_err(|e| format!("Ошибка ожидания: {e}"))?;

    if stderr_thread.join().is_err() {
        log::error!("[YTDLP] Поток чтения stderr завершился аварийно");
    }

    if cancelled {
        let _ = app_clone.emit("ytdlp:download-cancelled", ());
        return Err("Скачивание отменено".to_string());
    }

    if !status.success() {
        let collected = match stderr_lines.lock() {
            Ok(v) => v.join("\n"),
            Err(poisoned) => poisoned.into_inner().join("\n"),
        };
        let msg = if collected.trim().is_empty() {
            format!("yt-dlp завершился с кодом {}", status.code().unwrap_or(-1))
        } else {
            collected.trim().to_string()
        };
        log::error!("[YTDLP] Ошибка загрузки: код выхода {}", status.code().unwrap_or(-1));
        let _ = app.emit("ytdlp:download-error", &msg);
        return Err(msg);
    }

    if let Some(e) = read_error {
        log::error!("[YTDLP] Поток stdout прерван, но процесс завершился успешно: {e}");
    }

    log::info!("[YTDLP] Загрузка завершена успешно");
    let _ = app.emit("ytdlp:download-progress", 100);
    let _ = app.emit("ytdlp:download-status", "Готово");
    let _ = app.emit("ytdlp:download-finished", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Регрессия на корневую причину падения `[Errno 22] Invalid argument`:
    /// stdout yt-dlp.exe на Windows в cp1251, строгий UTF-8-разбор обрывал
    /// цикл и закрывал pipe. Поток с невалидным UTF-8 обязан быть прочитан
    /// ЦЕЛИКОМ, а не потерян после первой битой строки.
    #[test]
    fn lossy_reader_keeps_stream_with_invalid_utf8() {
        // Синтетические строки: cp1251-байты кириллицы невалидны как UTF-8.
        let mut stream: Vec<u8> = Vec::new();
        stream.extend_from_slice(b"[download] Destination: ");
        stream.extend_from_slice(&[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]); // "Тест"
        stream.extend_from_slice(b".mp4\n");
        stream.extend_from_slice(b"[download] 100% of 10.00MiB at 1.00MiB/s\n");
        stream.extend_from_slice(b"[Merger] Merging formats into \"out.mkv\"\n");

        let mut reader = BufReader::new(std::io::Cursor::new(stream));
        let mut lines: Vec<String> = Vec::new();
        read_stream_lossy(&mut reader, &mut |line| lines.push(line.to_string()));

        assert_eq!(lines.len(), 3, "ни одна строка не должна теряться: {lines:?}");
        assert!(lines[0].starts_with("[download] Destination: "));
        assert!(PROGRESS_RE.is_match(&lines[1]), "строка прогресса: {}", lines[1]);
        assert!(lines[2].starts_with("[Merger] Merging formats into "));
    }

    /// Регрессия на пустой прогрессбар: старый регекс требовал `at <число>`
    /// сразу после размера, а реальные строки yt-dlp имеют `~ 434.16MiB`
    /// (пробел после `~`), `at  Unknown B/s` и финальную форму без `at`.
    /// Старый регекс не совпал НИ С ОДНОЙ строкой — полоса была всегда пуста.
    #[test]
    fn progress_line_matches_every_real_yt_dlp_shape() {
        let shapes = [
            "[download]   0.2% of ~ 347.66KiB at    682.43B/s ETA Unknown (frag 1/250)",
            "[download]  14.8% of ~ 435.48MiB at    4.33MiB/s ETA 01:17 (frag 36/250)",
            "[download]   0.0% of   21.55MiB at  Unknown B/s ETA Unknown",
            "[download] 100% of ~ 434.16MiB in 01:32",
            "[download] 100.0% of ~   21.55MiB at    2.10MiB/s ETA 00:10",
        ];
        for line in shapes {
            let m = PROGRESS_RE
                .captures(line)
                .unwrap_or_else(|| panic!("не распознана прогресс-строка: {line}"));
            let pct: f64 = m[1].parse().unwrap_or(-1.0);
            assert!((0.0..=100.0).contains(&pct), "процент вне диапазона: {pct} ({line})");
        }
    }

    /// Не-прогрессные строки `[download]` обязаны остаться не-прогрессными,
    /// иначе `Destination:`-веха уедет в телеметрию.
    #[test]
    fn destination_line_is_not_progress() {
        let line = "[download] Destination: E:\\Downloads\\sample.f616.mp4";
        assert!(!PROGRESS_RE.is_match(line));
        assert!(DEST_RE.is_match(line));
    }

    #[test]
    fn status_text_has_percent_and_optional_parts() {
        assert_eq!(
            progress_status("[download]  46.3% of   21.55MiB at  Unknown B/s ETA Unknown", 46.3),
            "Скачивание: 46.3% · 21.55 MiB"
        );
        assert_eq!(
            progress_status("[download]  14.8% of ~ 435.48MiB at    4.33MiB/s ETA 01:17", 14.8),
            "Скачивание: 14.8% · 435.48 MiB · 4.33 MiB/s"
        );
        assert_eq!(
            progress_status("[download] 100% of ~ 434.16MiB in 01:32", 100.0),
            "Скачивание: 100.0% · 434.16 MiB"
        );
    }

    #[test]
    fn lossy_reader_stops_on_real_io_error() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "сломанный pipe"))
            }
        }
        let mut reader = BufReader::new(Failing);
        let mut count = 0;
        read_stream_lossy(&mut reader, &mut |_| count += 1);
        assert_eq!(count, 0);
    }
}

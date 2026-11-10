//! Запуск ffmpeg/ffprobe как subprocess БЕЗ shell: аргументы передаются
//! массивом (никаких кавычек/инъекций), stdin закрыт (ffmpeg не зависнет
//! в ожидании ответа на «File exists, overwrite?»), таймаут с kill.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Дефолтный таймаут запуска: перекодирование длинных видео может идти минуты.
pub const DEFAULT_RUN_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Хвост stderr, попадающий в отчёт об ошибке (ffmpeg пишет причину в конец).
const TAIL_CHARS: usize = 4000;

/// Результат запуска тулa.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr_tail: String,
}

/// Запустить исполняемый файл с аргументами, вернуть stdout + хвост stderr.
/// Ошибкой (Err) считаются только: не удалось запустить / превышен таймаут.
/// Ненулевой код выхода — это валидный RunOutput с `success: false`.
pub fn run_tool(exe: &Path, args: &[String], timeout: Duration) -> Result<RunOutput, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window_command(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Не удалось запустить {}: {e}", exe.display()))?;

    // Пайпы читаем в отдельных потоках: без этого заполненный буфер пайпа
    // блокирует ребёнка, а мы его — deadlock до таймаута.
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let out_thread = std::thread::spawn(move || read_all(&mut stdout_pipe));
    let err_thread = std::thread::spawn(move || read_all(&mut stderr_pipe));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None; // таймаут: пайпы закроются смертью ребёнка
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("Ожидание {}: {e}", exe.display())),
        }
    };

    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();

    match status {
        Some(st) => Ok(RunOutput {
            success: st.success(),
            exit_code: st.code(),
            stdout,
            stderr_tail: tail(&stderr, TAIL_CHARS),
        }),
        None => Err(format!(
            "Превышен таймаут {} с: {} (аргументы: {})",
            timeout.as_secs(),
            exe.display(),
            args.join(" ")
        )),
    }
}

fn read_all<R: Read>(pipe: &mut Option<R>) -> String {
    let mut buf = Vec::new();
    if let Some(p) = pipe {
        let _ = p.read_to_end(&mut buf); // ошибка чтения = пустой вывод, не падение
    }
    String::from_utf8_lossy(&buf).to_string()
}

/// Последние `max` символов (граница символа UTF-8 не ломается).
pub fn tail(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().skip(s.chars().count() - max).collect()
}

fn no_window_command(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: не мигаем консолью из GUI-хоста.
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn tail_keeps_utf8_boundaries() {
        assert_eq!(tail("abc", 10), "abc");
        assert_eq!(tail("привет", 3), "вет");
        assert_eq!(tail("", 5), "");
    }

    #[test]
    #[cfg(windows)]
    fn run_tool_captures_stdout_and_success() {
        // Windows PowerShell есть всегда; спавн без shell — args массивом.
        let out = run_tool(
            Path::new("powershell"),
            &[
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                "Write-Output marker_12345".into(),
            ],
            Duration::from_secs(60),
        )
        .expect("powershell должен запуститься");
        assert!(out.success, "exit_code={:?}", out.exit_code);
        assert!(out.stdout.contains("marker_12345"), "stdout={}", out.stdout);
    }

    #[test]
    #[cfg(windows)]
    fn run_tool_kills_on_timeout_and_reports_honestly() {
        let res = run_tool(
            Path::new("powershell"),
            &[
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                "Start-Sleep -Seconds 60".into(),
            ],
            Duration::from_secs(2),
        );
        match res {
            Err(msg) => assert!(msg.contains("таймаут"), "сообщение: {msg}"),
            Ok(out) => panic!("ожидали таймаут, получили успех: {:?}", out.exit_code),
        }
    }
}

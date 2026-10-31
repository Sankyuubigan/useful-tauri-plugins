//! Внешние процессы: PowerShell-запросы с таймаутом и честной кодировкой,
//! детект прав администратора.
//!
//! Две задачи, ради которых существует модуль:
//!
//! 1. [`run_powershell`] — WMI/журнал Windows читаются только через
//!    PowerShell (5.1 у всех Windows 10/11). Вердикты:
//!    - `[Console]::OutputEncoding = UTF8` в начале каждого скрипта — иначе
//!      русские сообщения журнала приходят в CP866/CP1251 и портятся.
//!    - чтение stdout/stderr в потоках-читателях: длинный вывод не должен
//!      забивать пайп и зависать, а таймаут обязан убивать процесс.
//! 2. [`is_elevated`] — часть проверок (дампы ядра, LiveKernelReports)
//!    доступна только под правами администратора. Проба — `net session`
//!    (код 0 = админ; работает на любом языке системы, не зависит от
//!    локали). Отсутствие net.exe → честное `None`.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Префикс, форсирующий UTF-8 в выводе PowerShell 5.1.
///
/// Без него Windows-консоль отдаёт текст в кодовой странице OEM (CP866 на
/// русской системе) — российские сообщения журнала читаются как мусор, и
/// даже символов ASCII-диапазона касаться не приходится: поток байт не UTF-8.
const PS_PREFIX: &str = "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; ";

/// Таймаут по умолчанию для журнальных/WMi-запросов (Б Black ящик не ждёт).
pub const DEFAULT_POWERSHELL_TIMEOUT: Duration = Duration::from_secs(20);

/// Прочитать пайп дочернего процесса до EOF в отдельном потоке.
fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        buf
    })
}

/// Запустить PowerShell и собрать stdout. Ошибка — строка с причиной
/// (код выхода + stderr), готовая класть в тело секции отчёта.
///
/// Честность (core §2.2): ни один отказ не глотается — ретрансу политику
/// ведёт вызывающий код, здесь только факт.
pub fn run_powershell(script: &str, timeout: Duration) -> Result<String, String> {
    log::debug!("diag: powershell: {} символов скрипта", script.chars().count());

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &format!("{}{}", PS_PREFIX, script),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("powershell не запущен: {e}"))?;

    // Читатели пайпов уходят в треды: главному потоку остаётся только
    // следить за таймаутом (объяснение — в доке модуля).
    let stdout_pipe = child.stdout.take().expect("stdout piped выше");
    let stderr_pipe = child.stderr.take().expect("stderr piped выше");
    let stdout_reader = drain(stdout_pipe);
    let stderr_reader = drain(stderr_pipe);

    let started = Instant::now();
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if started.elapsed() > timeout {
                    kill_child(&mut child);
                    break None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                kill_child(&mut child);
                // Превращаем дорогой формат состояние->строка честно.
                return Err(format!("powershell: внутренняя ошибка ожидания: {e}"));
            }
        }
    };

    // После kill виноват таймаут — пайпы закрываются, читатели уходят.
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    match exit_status {
        Some(status) if status.success() => Ok(String::from_utf8_lossy(&stdout)
            .trim_end()
            .to_string()),
        Some(status) => {
            let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "?".into());
            let reason = String::from_utf8_lossy(&stderr).trim().to_string();
            Err(format!("powershell завершился с кодом {code}: {reason}"))
        }
        None => Err(format!(
            "powershell убит по таймауту {} с (запрос слишком долгий — сокращай окно/лимит)",
            timeout.as_secs()
        )),
    }
}

fn kill_child(child: &mut Child) {
    // Ошибка kill не скрывается: без неё поток-висулька остаётся навсегда.
    if let Err(e) = child.kill() {
        log::warn!("diag: powershell не убит после таймаута: {e}");
    }
}

/// Есть ли у текущего процесса права администратора.
///
/// `Ok(true/false) = ответ`; `None` — пробу выполнить не удалось (в этом
/// случае секции обязаны сами сообщать СВОИ ограничения, а не молчать).
/// Идея пробы: `net session` доступен только админу, код 0 — успех.
/// Язык системы не важен: решение по коду процесса, а не по тексту (§2.2).
pub fn is_elevated() -> Option<bool> {
    let status = Command::new("net")
        .arg("session")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    Some(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_runs_simple_command() {
        let out = run_powershell("Write-Output ok", Duration::from_secs(15))
            .expect("powershell должен работать на Windows");
        assert_eq!(out, "ok");
    }

    #[test]
    fn powershell_failure_is_honest_not_empty() {
        // Команды с таким именем не существует: код 1 + текст ошибки.
        let err = run_powershell("this-command-must-not-exist-4a12", Duration::from_secs(15));
        match err {
            Ok(_) => panic!("несуществующая команда обязана дать ошибку, а не пустой успех"),
            Err(msg) => {
                assert!(msg.contains("powershell"), "причина обязана называть источник: {msg}");
            }
        }
    }

    #[test]
    fn elevation_probe_returns_a_verdict_or_none() {
        // На CI/на машине юзера оба варианта честны: ответ или «пробу не удалось».
        let verdict = is_elevated();
        if verdict.is_none() {
            // Возврат None допустим только при недоступной команде net.
            log::warn!("diag: пробу админа выполнить не удалось (net.exe недоступен?)");
        }
    }
}

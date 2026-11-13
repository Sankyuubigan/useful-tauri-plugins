//! Потоковый FFmpeg-процесс с живыми пайпами stdin/stdout (live-режим хоста:
//! медиа-сегменты скармливаются в stdin порциями, декодированный вывод читается
//! из stdout, пока процесс жив). В отличие от `runner::run_tool` — это
//! долгоживущий процесс без таймаута: остановка управляется хостом явно.
//!
//! Правила безопасности совпадают с runner: без shell, аргументы массивом,
//! CREATE_NO_WINDOW, stderr собирается в кольцо-хвост для отчёта об ошибке.
//!
//! Разбор пайпа (`split`) обязателен, когда вход и выход обслуживаются
//! конкурентно из разных потоков: иначе заполненный выходной пайп останавливает
//! чтение входа у ffmpeg, а писец, заблокированный в write, не даёт читать
//! выход — взаимная блокировка. Поля структуры — Option: split забирает их
//! через take(), после чего Drop (гасящий процесс) становится no-op —
//! ответственность за жизненный цикл переходит к PipeGuard.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Сколько последних байт stderr держать в памяти (ffmpeg пишет причину в конец).
const STDERR_TAIL_BYTES: usize = 8000;

pub struct FfmpegPipe {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl FfmpegPipe {
    /// Спавн `exe args…`: stdin/stdout — пайпы, stderr — поток-коллектор.
    pub fn spawn(exe: &Path, args: &[String]) -> Result<Self, String> {
        let mut cmd = Command::new(exe);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        super::runner::no_window_command(&mut cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Не удалось запустить {}: {e}", exe.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "ffmpeg: не открыт stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "ffmpeg: не открыт stdout".to_string())?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| "ffmpeg: не открыт stderr".to_string())?;

        let stderr_tail: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let tail_clone = Arc::clone(&stderr_tail);
        let stderr_thread = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let sink = tail_clone;
            // Читаем до EOF; держим только хвост STDERR_TAIL_BYTES.
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if let Ok(mut v) = sink.lock() {
                    v.extend_from_slice(&buf[..n]);
                    let len = v.len();
                    if len > STDERR_TAIL_BYTES {
                        v.drain(..len - STDERR_TAIL_BYTES);
                    }
                }
            }
        });

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr_tail,
            stderr_thread: Some(stderr_thread),
        })
    }

    /// Разобрать пайп на части для работы из разных потоков: писец пишет в
    /// stdin, читатель — блокирует stdout, страж гасит процесс. Поля забраны
    /// take() → Drop данного экземпляра ничего не гасит; за процесс отвечает
    /// PipeGuard (гасит при drop), stdin закрывает писец после EOF очереди.
    pub fn split(mut self) -> (PipeStdin, PipeStdout, PipeGuard) {
        let child = self.child.take();
        let stdin = self.stdin.take();
        let stdout = self.stdout.take();
        let stderr_tail = Arc::clone(&self.stderr_tail);
        let stderr_thread = self.stderr_thread.take();
        (
            PipeStdin { inner: stdin },
            PipeStdout { inner: stdout },
            PipeGuard {
                child,
                stderr_tail,
                stderr_thread,
            },
        )
    }

    /// Записать порцию входных данных в stdin процесса.
    /// Блокирующий вызов: допустимо из выделенного потока-писца, не из async.
    pub fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.stdin_mut()?.write_all(data).map_err(|e| format!("ffmpeg: запись в stdin: {e}"))
    }

    /// Блокирующее чтение порции вывода (до buf.len()); Ok(0) = EOF.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let out = self
            .stdout
            .as_mut()
            .ok_or_else(|| "ffmpeg: stdout уже разобран".to_string())?;
        out.read(buf).map_err(|e| format!("ffmpeg: чтение stdout: {e}"))
    }

    /// Завершён ли процесс (неблокирующе).
    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.child.as_mut().and_then(|c| c.try_wait().ok().flatten())
    }

    /// Хвост stderr как строка (для сообщения об ошибке).
    pub fn stderr_tail(&self) -> String {
        stderr_tail_string(&self.stderr_tail)
    }

    fn stdin_mut(&mut self) -> Result<&mut ChildStdin, String> {
        self.stdin
            .as_mut()
            .ok_or_else(|| "ffmpeg: stdin уже закрыт".to_string())
    }
}

/// До split: закрыть stdin (ffmpeg допишет хвост и завершится сам), гасить
/// процесс. После split все поля None — здесь уже нечего делать.
impl Drop for FfmpegPipe {
    fn drop(&mut self) {
        if let Some(mut s) = self.stdin.take() {
            let _ = s.flush();
        }
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Половина пайпа для записи (поток-писец).
pub struct PipeStdin {
    inner: Option<ChildStdin>,
}

impl PipeStdin {
    /// Записать порцию входных данных (блокирующе).
    pub fn write(&mut self, data: &[u8]) -> Result<(), String> {
        let stdin = self
            .inner
            .as_mut()
            .ok_or_else(|| "ffmpeg: stdin уже закрыт".to_string())?;
        stdin
            .write_all(data)
            .map_err(|e| format!("ffmpeg: запись в stdin: {e}"))
    }

    /// Закрыть stdin: ffmpeg дописывает хвост вывода и завершается сам (EOF).
    pub fn close(&mut self) {
        if let Some(mut s) = self.inner.take() {
            let _ = s.flush();
        }
    }

    pub fn is_closed(&self) -> bool {
        self.inner.is_none()
    }
}

/// Половина пайпа для чтения вывода (поток-читатель).
pub struct PipeStdout {
    inner: Option<ChildStdout>,
}

impl PipeStdout {
    /// Блокирующее чтение до buf.len(); Ok(0) = EOF (процесс закончил вывод).
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let out = self
            .inner
            .as_mut()
            .ok_or_else(|| "ffmpeg: stdout уже разобран".to_string())?;
        out.read(buf).map_err(|e| format!("ffmpeg: чтение stdout: {e}"))
    }
}

/// Страж процесса: держит child живым, гасит при Drop.
pub struct PipeGuard {
    child: Option<Child>,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl PipeGuard {
    /// Завершён ли процесс (неблокирующе).
    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.child.as_mut().and_then(|c| c.try_wait().ok().flatten())
    }

    /// Жёсткое гашение: kill + wait + join потока stderr.
    pub fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Хвост stderr как строка (для сообщения об ошибке).
    pub fn stderr_tail(&self) -> String {
        stderr_tail_string(&self.stderr_tail)
    }
}

impl Drop for PipeGuard {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        // Процесс дожит (wait выше) → пайп stderr закрыт → поток-коллектор
        // увидит EOF; join чистит ресурс, а не виснет.
        if let Some(h) = self.stderr_thread.take() {
            let _ = h.join();
        }
    }
}

fn stderr_tail_string(tail: &Arc<Mutex<Vec<u8>>>) -> String {
    let bytes = tail.lock().map(|v| v.clone()).unwrap_or_default();
    String::from_utf8_lossy(&bytes).to_string()
}

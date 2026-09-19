//! Утилиты управления внешними prebuilt-процессами движков (CrispASR и т.п.).
//!
//! 1. [`kill_process_tree`] — атомарный убой ВСЕГО дерева (`taskkill /F /T` до
//!    репарентинга, затем `child.kill()` как fallback).
//! 2. [`JobGuard`] — Windows Job Object с `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
//!    гарантирующий, что ОС сама убьёт дерево при насильственном закрытии/краше.
//! 3. [`kill_active_engines`] — глобальная зачистка зомби при старте и выходе.

use std::process::Child;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Принудительно убивает ВСЁ дерево процессов (родитель + все потомки).
pub fn kill_process_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let pid = child.id();
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Проверяет, запущен ли в системе хотя бы один процесс движка (по имени образа).
/// Нужна для внятной ошибки при установке поверх работающего движка.
pub fn engine_running() -> bool {
    #[cfg(windows)]
    {
        match std::process::Command::new("tasklist")
            .args(["/NH", "/FI", "IMAGENAME eq crispasr.exe"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
        {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stdout);
                // В строке «INFO: No tasks...» тоже есть «crispasr.exe» (условие фильтра),
                // поэтому проверяем именно имя образа в начале строки процесса.
                text.lines().any(|line| {
                    line.split_whitespace()
                        .next()
                        .map(|w| w.eq_ignore_ascii_case("crispasr.exe"))
                        .unwrap_or(false)
                })
            }
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    {
        matches!(
            std::process::Command::new("pgrep")
                .args(["-f", "crispasr"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status(),
            Ok(s) if s.success()
        )
    }
}

/// Глобальная зачистка всех запущенных движков (по имени образа).
pub fn kill_active_engines() {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/F", "/IM", "crispasr.exe"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.status();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-f", "crispasr"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// RAII-гард Job Object'а. При выходе из области видимости закрывает хендл, что
/// (благодаря `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) убивает всё дерево процессов.
#[cfg(windows)]
pub struct JobGuard {
    handle: *mut std::ffi::c_void,
}

#[cfg(not(windows))]
pub struct JobGuard;

#[cfg(windows)]
impl JobGuard {
    /// Назначает дочерний процесс в Job Object с `KILL_ON_JOB_CLOSE`.
    pub fn assign(child: &Child) -> Option<JobGuard> {
        use std::os::windows::io::AsRawHandle;

        const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
        const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;

        unsafe {
            let handle = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if handle.is_null() {
                return None;
            }

            let info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                    PerProcessUserTimeLimit: 0,
                    PerJobUserTimeLimit: 0,
                    LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    MinimumWorkingSetSize: 0,
                    MaximumWorkingSetSize: 0,
                    ActiveProcessLimit: 0,
                    Affinity: 0,
                    PriorityClass: 0,
                    SchedulingClass: 0,
                },
                IoInfo: std::mem::zeroed(),
                ProcessMemoryLimit: 0,
                JobMemoryLimit: 0,
                PeakProcessMemoryUsed: 0,
                PeakJobMemoryUsed: 0,
            };

            let ok = SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                let _ = CloseHandle(handle);
                return None;
            }

            let process = child.as_raw_handle() as *mut std::ffi::c_void;
            let ok = AssignProcessToJobObject(handle, process);
            if ok == 0 {
                let _ = CloseHandle(handle);
                return None;
            }

            Some(JobGuard { handle })
        }
    }

    /// Немедленно убивает всё дерево процессов в job.
    pub fn terminate(&self) {
        unsafe {
            let _ = TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(not(windows))]
impl JobGuard {
    /// На non-Windows Job Object не применяется — зачистка только через `kill_process_tree`.
    pub fn assign(_child: &Child) -> Option<JobGuard> {
        None
    }

    /// No-op на non-Windows.
    pub fn terminate(&self) {}
}

#[cfg(windows)]
impl Drop for JobGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

// Job-хендл — это описатель ОС (указатель) без разделяемого состояния. Доступ к
// нему всегда сериализован через `Mutex<Option<JobGuard>>` и перед использованием
// он извлекается (`take`), поэтому `Send`/`Sync` безопасны.
#[cfg(windows)]
unsafe impl Send for JobGuard {}
#[cfg(windows)]
unsafe impl Sync for JobGuard {}

#[cfg(windows)]
unsafe extern "system" {
    fn CreateJobObjectW(
        job_attributes: *mut std::ffi::c_void,
        name: *const u16,
    ) -> *mut std::ffi::c_void;

    fn AssignProcessToJobObject(
        job: *mut std::ffi::c_void,
        process: *mut std::ffi::c_void,
    ) -> i32;

    fn SetInformationJobObject(
        job: *mut std::ffi::c_void,
        job_object_information_class: u32,
        job_object_information: *const std::ffi::c_void,
        length: u32,
    ) -> i32;

    fn TerminateJobObject(job: *mut std::ffi::c_void, exit_code: u32) -> i32;

    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

#[cfg(windows)]
#[repr(C)]
struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
    PerProcessUserTimeLimit: i64,
    PerJobUserTimeLimit: i64,
    LimitFlags: u32,
    MinimumWorkingSetSize: usize,
    MaximumWorkingSetSize: usize,
    ActiveProcessLimit: u32,
    Affinity: usize,
    PriorityClass: u32,
    SchedulingClass: u32,
}

#[cfg(windows)]
#[repr(C)]
struct IO_COUNTERS {
    ReadOperationCount: u64,
    WriteOperationCount: u64,
    OtherOperationCount: u64,
    ReadTransferCount: u64,
    WriteTransferCount: u64,
    OtherTransferCount: u64,
}

#[cfg(windows)]
#[repr(C)]
struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
    BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION,
    IoInfo: IO_COUNTERS,
    ProcessMemoryLimit: usize,
    JobMemoryLimit: usize,
    PeakProcessMemoryUsed: usize,
    PeakJobMemoryUsed: usize,
}
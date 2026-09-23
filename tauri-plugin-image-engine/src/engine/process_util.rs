//! Утилиты управления дочерними процессами (порт паттерна llama-engine).
//!
//! `Child::kill()` на Windows завершает только прямого ребёнка — `taskkill
//! /F /T /PID` убивает всё дерево. Плюс Windows Job Object KILL_ON_JOB_CLOSE
//! как сейфет при насильственном закрытии приложения.

use std::sync::Mutex;

/// Живые PID запущенных sd-server. Докиляются при выходе из приложения.
static ACTIVE_ENGINE_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Зарегистрировать PID запущенного движка.
pub fn register_engine_pid(pid: u32) {
    ACTIVE_ENGINE_PIDS.lock().unwrap().push(pid);
}

/// Снять PID завершённого движка из реестра.
pub fn unregister_engine_pid(pid: u32) {
    let mut g = ACTIVE_ENGINE_PIDS.lock().unwrap();
    if let Some(pos) = g.iter().position(|&p| p == pid) {
        g.remove(pos);
    }
}

/// Докилять все живые движки (вызывается при выходе из приложения).
pub fn kill_active_engines() {
    let pids: Vec<u32> = ACTIVE_ENGINE_PIDS.lock().unwrap().clone();
    for pid in pids {
        log::info!("Принудительная остановка sd-server (pid {}) при выходе из приложения", pid);
        kill_pid_tree(pid);
    }
    ACTIVE_ENGINE_PIDS.lock().unwrap().clear();
}

/// Убить процесс по PID вместе со всем деревом потомков.
#[cfg(windows)]
pub fn kill_pid_tree(pid: u32) {
    let mut kill = std::process::Command::new("taskkill");
    kill.args(["/F", "/T", "/PID", &pid.to_string()]);
    // Без CREATE_NO_WINDOW taskkill мелькает чёрным консольным окном.
    { use std::os::windows::process::CommandExt; kill.creation_flags(0x08000000); }
    let _ = kill.output();
}

/// Убить процесс по PID вместе со всем деревом потомков.
#[cfg(not(windows))]
pub fn kill_pid_tree(pid: u32) {
    let _ = std::process::Command::new("pkill").args(["-P", &pid.to_string()]).output();
    let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).output();
}

/// Убить непосредственного ребёнка вместе со всем его деревом.
pub fn kill_process_tree(child: &mut std::process::Child) {
    let pid = child.id();
    kill_pid_tree(pid);
    // Fallback, если taskkill/pkill недоступны.
    let _ = child.kill();
}

#[cfg(windows)]
mod kill_job {
    //! Windows Job Object с флагом KILL_ON_JOB_CLOSE: ОС сама убивает все
    //! назначенные процессы, когда приложение завершается.

    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::ptr;
    use std::sync::Mutex;

    type HANDLE = *mut c_void;
    type BOOL = i32;
    type DWORD = u32;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct JobObjectBasicLimitInformation {
        limit_flags: DWORD,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: DWORD,
        affinity: usize,
        priority_class: DWORD,
        scheduling_class: DWORD,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct IoCounters {
        read: u64, write: u64, other: u64, read_ops: u64, write_ops: u64, other_ops: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct JobObjectExtendedLimitInformation {
        basic_limit_information: JobObjectBasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: DWORD = 0x0000_2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;

    extern "system" {
        fn CreateJobObjectW(lp_attributes: *mut c_void, lp_name: *const u16) -> HANDLE;
        fn AssignProcessToJobObject(h_job: HANDLE, h_process: HANDLE) -> BOOL;
        fn SetInformationJobObject(
            h_job: HANDLE,
            info_class: u32,
            lp_info: *mut c_void,
            cb: DWORD,
        ) -> BOOL;
        fn CloseHandle(h_object: HANDLE) -> BOOL;
    }

    static KILL_JOB: Mutex<Option<isize>> = Mutex::new(None);

    /// Назначить дочерний процесс в общий kill-job. Best-effort.
    pub fn assign_child_to_kill_job(child: &std::process::Child) {
        let mut guard = KILL_JOB.lock().unwrap();
        let job = match *guard {
            Some(h) => h as HANDLE,
            None => {
                let h = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
                if h.is_null() {
                    return;
                }
                let mut info: JobObjectExtendedLimitInformation = unsafe { std::mem::zeroed() };
                info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = unsafe {
                    SetInformationJobObject(
                        h,
                        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                        &mut info as *mut _ as *mut c_void,
                        std::mem::size_of::<JobObjectExtendedLimitInformation>() as DWORD,
                    )
                };
                if ok == 0 {
                    unsafe { CloseHandle(h) };
                    return;
                }
                *guard = Some(h as isize);
                h
            }
        };
        let handle = child.as_raw_handle() as HANDLE;
        unsafe { AssignProcessToJobObject(job, handle) };
    }
}

#[cfg(windows)]
pub use kill_job::assign_child_to_kill_job;

#[cfg(not(windows))]
pub fn assign_child_to_kill_job(_child: &std::process::Child) {}

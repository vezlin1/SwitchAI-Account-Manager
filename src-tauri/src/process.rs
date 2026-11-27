use crate::errors::{AppError, AppResult};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

fn poll_until<T>(
    timeout: Duration,
    interval: Duration,
    mut poll: impl FnMut() -> AppResult<Option<T>>,
) -> AppResult<Option<T>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = poll()? {
            return Ok(Some(value));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        std::thread::sleep(interval.min(remaining));
    }
}

/// Every external stop helper gets its own deadline, including taskkill itself.
/// Output is discarded; no pipes can fill and stop process termination.
pub fn command_status_with_timeout(
    command: &mut Command,
    timeout: Duration,
    context: &'static str,
) -> AppResult<ExitStatus> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| AppError::Io { context, source })?;
    let status = poll_until(timeout, Duration::from_millis(25), || {
        child
            .try_wait()
            .map_err(|source| AppError::Io { context, source })
    })?;
    if let Some(status) = status {
        return Ok(status);
    }
    let _ = child.kill();
    // Reap after cancellation, also with a bounded wait.
    let _ = poll_until(Duration::from_secs(1), Duration::from_millis(25), || {
        child
            .try_wait()
            .map_err(|source| AppError::Io { context, source })
    });
    Err(AppError::msg(format!(
        "{context}: command timed out after {} seconds",
        timeout.as_secs()
    )))
}

#[cfg(target_os = "windows")]
pub fn is_named_process_running(exe_names: &[&str]) -> Vec<bool> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let mut found = vec![false; exe_names.len()];
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return found;
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe_str = String::from_utf16_lossy(&entry.szExeFile[..len]);
                for (i, &name) in exe_names.iter().enumerate() {
                    if !found[i] && exe_str.eq_ignore_ascii_case(name) {
                        found[i] = true;
                    }
                }
                if found.iter().all(|&f| f) {
                    break;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    found
}

#[cfg(target_os = "macos")]
pub fn is_named_process_running(names: &[&str]) -> Vec<bool> {
    use std::os::raw::{c_char, c_int, c_void};

    const PROC_ALL_PIDS: u32 = 1;
    const PROC_PIDTBSDINFO: c_int = 3;

    #[repr(C)]
    struct ProcBsdInfo {
        pbi_flags: u32,
        pbi_status: u32,
        pbi_xstatus: u32,
        pbi_pid: u32,
        pbi_ppid: u32,
        pbi_uid: u32,
        pbi_gid: u32,
        pbi_ruid: u32,
        pbi_rgid: u32,
        pbi_svuid: u32,
        pbi_svgid: u32,
        rfu_1: u32,
        pbi_comm: [c_char; 16],
        pbi_name: [c_char; 32],
        pbi_nfiles: u32,
        pbi_pgid: u32,
        pbi_pjobc: u32,
        e_tdev: u32,
        e_tpgid: u32,
        pbi_nice: i32,
        pbi_start_tvsec: u64,
        pbi_start_tvusec: u64,
    }

    unsafe extern "C" {
        fn proc_listpids(
            proc_type: u32,
            proc_info: u32,
            buffer: *mut c_void,
            buffersize: c_int,
        ) -> c_int;
        fn proc_pidinfo(
            pid: c_int,
            flavor: c_int,
            arg: u64,
            buffer: *mut c_void,
            buffersize: c_int,
        ) -> c_int;
    }

    let mut found = vec![false; names.len()];
    unsafe {
        let count = proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0);
        if count <= 0 {
            return found;
        }
        let mut pids = vec![0 as c_int; (count as usize) / std::mem::size_of::<c_int>() + 32];
        let bytes_written = proc_listpids(
            PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr() as *mut c_void,
            (pids.len() * std::mem::size_of::<c_int>()) as c_int,
        );
        if bytes_written <= 0 {
            return found;
        }
        let num_pids = (bytes_written as usize) / std::mem::size_of::<c_int>();
        let mut bsd_info = std::mem::zeroed::<ProcBsdInfo>();
        let bsd_size = std::mem::size_of::<ProcBsdInfo>() as c_int;
        for &pid in &pids[..num_pids] {
            if pid <= 0 {
                continue;
            }
            let ret = proc_pidinfo(
                pid,
                PROC_PIDTBSDINFO,
                0,
                &mut bsd_info as *mut _ as *mut c_void,
                bsd_size,
            );
            if ret == bsd_size {
                let c_chars_to_str = |buf: &[c_char]| -> &str {
                    let bytes = std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len());
                    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                    std::str::from_utf8(&bytes[..len]).unwrap_or("")
                };
                let name = c_chars_to_str(&bsd_info.pbi_name);
                let comm = c_chars_to_str(&bsd_info.pbi_comm);
                for (i, &target) in names.iter().enumerate() {
                    if !found[i]
                        && (name.eq_ignore_ascii_case(target) || comm.eq_ignore_ascii_case(target))
                    {
                        found[i] = true;
                    }
                }
                if found.iter().all(|&f| f) {
                    break;
                }
            }
        }
    }
    found
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn is_named_process_running(names: &[&str]) -> Vec<bool> {
    vec![false; names.len()]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEntry {
    pub pid: u32,
    pub parent_pid: u32,
    pub name: String,
}

#[cfg(target_os = "windows")]
pub fn list_processes() -> Vec<ProcessEntry> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let mut processes = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return processes;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                processes.push(ProcessEntry {
                    pid: entry.th32ProcessID,
                    parent_pid: entry.th32ParentProcessID,
                    name: String::from_utf16_lossy(&entry.szExeFile[..len]),
                });
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    processes
}

#[cfg(target_os = "macos")]
pub fn list_processes() -> Vec<ProcessEntry> {
    use std::os::raw::{c_int, c_void};

    const PROC_ALL_PIDS: u32 = 1;
    const PROC_PIDT_SHORTBSDINFO: c_int = 13;
    const MAXCOMLEN: usize = 16;

    // struct proc_bsdshortinfo from <sys/proc_info.h>.
    #[repr(C)]
    struct ProcBsdShortInfo {
        pbsi_pid: u32,
        pbsi_ppid: u32,
        pbsi_pgid: u32,
        pbsi_status: u32,
        pbsi_comm: [u8; MAXCOMLEN],
        pbsi_flags: u32,
        pbsi_uid: u32,
        pbsi_gid: u32,
        pbsi_ruid: u32,
        pbsi_rgid: u32,
        pbsi_svuid: u32,
        pbsi_svgid: u32,
        pbsi_rfu: u32,
    }

    unsafe extern "C" {
        fn proc_listpids(
            proc_type: u32,
            proc_info: u32,
            buffer: *mut c_void,
            buffersize: c_int,
        ) -> c_int;
        fn proc_pidinfo(
            pid: c_int,
            flavor: c_int,
            arg: u64,
            buffer: *mut c_void,
            buffersize: c_int,
        ) -> c_int;
    }

    let mut processes = Vec::new();
    unsafe {
        let count = proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0);
        if count <= 0 {
            return processes;
        }
        let mut pids = vec![0 as c_int; (count as usize) / std::mem::size_of::<c_int>() + 32];
        let bytes_written = proc_listpids(
            PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr() as *mut c_void,
            (pids.len() * std::mem::size_of::<c_int>()) as c_int,
        );
        if bytes_written <= 0 {
            return processes;
        }
        let num_pids = (bytes_written as usize) / std::mem::size_of::<c_int>();
        let mut info = std::mem::zeroed::<ProcBsdShortInfo>();
        let size = std::mem::size_of::<ProcBsdShortInfo>() as c_int;
        for &pid in &pids[..num_pids] {
            if pid <= 0 {
                continue;
            }
            let ret = proc_pidinfo(
                pid,
                PROC_PIDT_SHORTBSDINFO,
                0,
                &mut info as *mut _ as *mut c_void,
                size,
            );
            if ret != size {
                continue;
            }
            let len = info
                .pbsi_comm
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(MAXCOMLEN);
            processes.push(ProcessEntry {
                pid: info.pbsi_pid,
                parent_pid: info.pbsi_ppid,
                name: String::from_utf8_lossy(&info.pbsi_comm[..len]).into_owned(),
            });
        }
    }
    processes
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn list_processes() -> Vec<ProcessEntry> {
    Vec::new()
}

/// Counts processes called `name` that are not descendants of a process
/// called one of `excluded_ancestors` (case-insensitive).
pub fn count_processes_outside(
    processes: &[ProcessEntry],
    name: &str,
    excluded_ancestors: &[&str],
) -> usize {
    let by_pid: std::collections::HashMap<u32, &ProcessEntry> = processes
        .iter()
        .map(|process| (process.pid, process))
        .collect();
    processes
        .iter()
        .filter(|process| process.name.eq_ignore_ascii_case(name))
        .filter(|process| {
            let mut parent_pid = process.parent_pid;
            // Bounded walk: PIDs can be reused, so a cycle must not hang us.
            for _ in 0..16 {
                let Some(parent) = by_pid.get(&parent_pid) else {
                    return true;
                };
                if parent.pid == parent.parent_pid {
                    return true;
                }
                if excluded_ancestors
                    .iter()
                    .any(|excluded| parent.name.eq_ignore_ascii_case(excluded))
                {
                    return false;
                }
                parent_pid = parent.parent_pid;
            }
            true
        })
        .count()
}

pub fn is_process_running(name: &str) -> AppResult<bool> {
    Ok(is_named_process_running(&[name])[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn stalled_child_command_returns_a_timeout_instead_of_waiting_for_exit() {
        use std::os::windows::process::CommandExt;

        let started = Instant::now();
        let result = command_status_with_timeout(
            Command::new("powershell.exe")
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Start-Sleep -Seconds 20",
                ])
                .creation_flags(0x0800_0000),
            Duration::from_millis(100),
            "Test child command",
        );
        assert!(
            result
                .unwrap_err()
                .user_message()
                .contains("command timed out")
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn bounded_poll_times_out_without_waiting_for_a_child() {
        let mut calls = 0;
        let result = poll_until::<()>(Duration::ZERO, Duration::from_secs(60), || {
            calls += 1;
            Ok(None)
        })
        .expect("poll");
        assert_eq!(result, None);
        assert_eq!(calls, 1);
    }

    #[test]
    fn bounded_poll_returns_ready_result_and_propagates_errors() {
        assert_eq!(
            poll_until(Duration::ZERO, Duration::ZERO, || Ok(Some(42))).unwrap(),
            Some(42)
        );
        let result = poll_until::<()>(Duration::ZERO, Duration::ZERO, || {
            Err(AppError::msg("poll failed"))
        });
        assert_eq!(result.unwrap_err().user_message(), "poll failed");
    }

    #[test]
    fn process_running_does_not_crash_on_nonexistent() {
        let running = is_process_running("nonexistent_process_definitely_not_running_12345.exe")
            .expect("detection should not fail");
        assert!(!running);
    }

    #[test]
    fn counts_only_processes_outside_the_excluded_process_tree() {
        let entry = |pid, parent_pid, name: &str| ProcessEntry {
            pid,
            parent_pid,
            name: name.to_string(),
        };
        let processes = vec![
            entry(1, 0, "explorer.exe"),
            entry(10, 1, "ChatGPT.exe"),
            entry(11, 10, "helper.exe"),
            entry(12, 11, "codex.exe"),
            entry(20, 1, "Code.exe"),
            entry(21, 20, "CODEX.EXE"),
            entry(30, 99, "codex.exe"),
            entry(40, 40, "codex.exe"),
        ];
        assert_eq!(
            count_processes_outside(&processes, "codex.exe", &["ChatGPT.exe"]),
            3
        );
        assert_eq!(count_processes_outside(&processes, "codex.exe", &[]), 4);
        let cycle = vec![entry(1, 2, "a"), entry(2, 1, "b"), entry(3, 1, "codex")];
        assert_eq!(count_processes_outside(&cycle, "codex", &["x"]), 1);
    }

    #[test]
    fn batch_process_running_handles_empty_slice() {
        let results = is_named_process_running(&[]);
        assert!(results.is_empty());
    }

    #[test]
    fn safe_c_chars_decoding_handles_non_null_terminated() {
        let buf = ['a' as std::os::raw::c_char; 16];
        let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len()) };
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let decoded = std::str::from_utf8(&bytes[..len]).unwrap_or("");
        assert_eq!(decoded, "aaaaaaaaaaaaaaaa");
    }
}

use crate::errors::AppResult;

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

pub fn is_process_running(name: &str) -> AppResult<bool> {
    Ok(is_named_process_running(&[name])[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_running_does_not_crash_on_nonexistent() {
        let running = is_process_running("nonexistent_process_definitely_not_running_12345.exe")
            .expect("detection should not fail");
        assert!(!running);
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

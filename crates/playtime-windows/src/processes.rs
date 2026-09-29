//! Running processes with their full executable paths (the C# `ProcessScanner`). Paths come from
//! `QueryFullProcessImageNameW` with `PROCESS_QUERY_LIMITED_INFORMATION`, which works for elevated games too.

use playtime_core::engine::ProcessInfo;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Closes a handle on drop.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: we own the handle and close it once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Every process with its image name; `path` is filled in by [`exe_path`] only when needed.
pub fn list() -> Vec<ProcessInfo> {
    // SAFETY: plain snapshot call; the handle is owned below.
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };
    let snapshot = OwnedHandle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut processes = Vec::new();
    // SAFETY: `entry` is a correctly sized PROCESSENTRY32W for each call.
    let mut ok = unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_ok();
    while ok {
        let name_len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        processes.push(ProcessInfo {
            pid: entry.th32ProcessID,
            name: String::from_utf16_lossy(&entry.szExeFile[..name_len]),
            path: None,
        });
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snapshot.0, &mut entry) }.is_ok();
    }
    processes
}

/// The full path of a process's executable, if it can be read.
pub fn exe_path(pid: u32) -> Option<String> {
    // SAFETY: opening with the least access that allows reading the image name; the handle is owned below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let process = OwnedHandle(process);
    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    // SAFETY: `buffer` holds `size` characters; the API writes at most that and updates `size`.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    }
    .ok()?;
    String::from_utf16(&buffer[..size as usize]).ok()
}

/// Lists processes and reads exe paths, reusing paths already known for the same (pid, name) from `previous`.
pub fn snapshot(previous: &[ProcessInfo]) -> Vec<ProcessInfo> {
    let known: std::collections::HashMap<(u32, &str), &Option<String>> = previous
        .iter()
        .map(|p| ((p.pid, p.name.as_str()), &p.path))
        .collect();
    list()
        .into_iter()
        .map(|mut p| {
            if p.pid > 4 {
                p.path = match known.get(&(p.pid, p.name.as_str())) {
                    Some(path) => (*path).clone(),
                    None => exe_path(p.pid),
                };
            }
            p
        })
        .collect()
}

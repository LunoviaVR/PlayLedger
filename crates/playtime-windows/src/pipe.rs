//! The tracker's named pipe, `\\.\pipe\PlaytimeTracker.<user SID>`: its name, and a client connection that is only
//! accepted if it really leads to the current user's own `playtime-tracker.exe`.
//!
//! The tracker creates the pipe with a DACL granting only the user's SID and rejects remote clients (see the
//! tracker's `pipe.rs`). The client checks the other side too: the pipe object must be owned by the current user, and
//! the process serving it must be `playtime-tracker.exe`, so another program that grabbed the name first can't pose
//! as the tracker.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// The tracker's executable name; only a pipe served by it is trusted.
pub const TRACKER_EXE: &str = "playtime-tracker.exe";

/// A SID as a string (`S-1-5-21-…`).
fn sid_string(sid: PSID) -> Option<String> {
    let mut text = PWSTR::null();
    // SAFETY: valid SID; the string is allocated by the system and freed with LocalFree.
    unsafe { ConvertSidToStringSidW(sid, &mut text) }.ok()?;
    // SAFETY: `text` is a valid NUL-terminated string until freed.
    let result = unsafe { text.to_string() }.ok();
    // SAFETY: freeing the string ConvertSidToStringSidW allocated.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
    }
    result
}

/// The current user's SID as a string (e.g. `S-1-5-21-…`).
pub fn current_user_sid() -> Option<String> {
    let mut token = HANDLE::default();
    // SAFETY: opens our own process token for querying; closed below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.ok()?;
    let mut size = 0u32;
    // SAFETY: size query; expected to fail with "insufficient buffer" and report the size.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut size) };
    // u64 elements keep the buffer suitably aligned for TOKEN_USER.
    let mut buffer = vec![0u64; (size as usize).div_ceil(8).max(1)];
    // SAFETY: `buffer` has at least `size` bytes.
    let got = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    };
    // SAFETY: we own the token handle.
    unsafe {
        let _ = CloseHandle(token);
    }
    got.ok()?;
    // SAFETY: the buffer now holds a TOKEN_USER whose SID points inside it.
    let sid = unsafe { (*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid };
    sid_string(sid)
}

/// The owner of a kernel object (here, the pipe) as a SID string.
fn owner_of(handle: HANDLE) -> Option<String> {
    let mut owner = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `handle` is open; the descriptor is allocated by the system and freed with LocalFree, and `owner`
    // points inside it.
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            Some(&mut descriptor),
        )
    };
    let result = if status.is_ok() {
        sid_string(owner)
    } else {
        None
    };
    if !descriptor.0.is_null() {
        // SAFETY: freeing the descriptor GetSecurityInfo allocated.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
    }
    result
}

/// The full path of the process serving the pipe.
fn server_image(handle: HANDLE) -> Option<PathBuf> {
    let mut pid = 0u32;
    // SAFETY: `handle` is an open client end of a named pipe.
    unsafe { GetNamedPipeServerProcessId(handle, &mut pid) }.ok()?;
    // SAFETY: query-only access; closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    // SAFETY: `buffer` holds `size` UTF-16 units.
    let got = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    };
    // SAFETY: we own the process handle.
    unsafe {
        let _ = CloseHandle(process);
    }
    got.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(
        &buffer[..size as usize],
    )))
}

/// Connects to the current user's tracker. Fails (as `NotFound`) if it isn't running, and (as
/// `PermissionDenied`) if the pipe isn't the current user's or isn't served by `playtime-tracker.exe`.
pub fn connect() -> io::Result<File> {
    let sid = current_user_sid()
        .ok_or_else(|| io::Error::other("couldn't read the current user's SID"))?;
    let pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(playtime_core::ipc::pipe_name(&sid))?;
    let handle = HANDLE(pipe.as_raw_handle());
    if owner_of(handle).as_deref() != Some(sid.as_str()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the tracker's pipe doesn't belong to this user",
        ));
    }
    let served_by_tracker = server_image(handle)
        .and_then(|path| path.file_name().map(|n| n.to_string_lossy().to_lowercase()))
        .is_some_and(|name| name == TRACKER_EXE);
    if !served_by_tracker {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the pipe isn't served by PlayLedger",
        ));
    }
    Ok(pipe)
}

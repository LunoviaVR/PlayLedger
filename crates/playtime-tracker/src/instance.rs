//! One tracker per Windows session, with the same kernel object names as earlier versions so an old and a new copy
//! detect each other: a second launch asks the running copy to show the dashboard, and `--exit` (used by the
//! installer) asks it to save and quit.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, OpenMutexW, SetEvent, WaitForSingleObject,
    EVENT_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE,
};

pub const MUTEX_NAME: &str = r"Local\GameSessionTracker.SingleInstance";
pub const SHOW_EVENT: &str = r"Local\GameSessionTracker.Show";
pub const EXIT_EVENT: &str = r"Local\GameSessionTracker.Exit";

/// An owned kernel handle.
pub struct Handle(pub HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: we own the handle and close it once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

// SAFETY: kernel handles may be waited on and closed from any thread.
unsafe impl Send for Handle {}
// SAFETY: as above; only waited on, never mutated.
unsafe impl Sync for Handle {}

/// The single-instance mutex, held for the tracker's lifetime.
pub struct Instance {
    _mutex: Handle,
    pub show: Handle,
    pub exit: Handle,
}

/// Becomes the running instance, or returns `None` if another copy already is.
pub fn acquire() -> Option<Instance> {
    let name = HSTRING::from(MUTEX_NAME);
    // SAFETY: creates or opens a named mutex; we own the returned handle.
    let mutex = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) }.ok()?;
    // SAFETY: reads the calling thread's last error right after CreateMutexW.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        drop(Handle(mutex));
        return None;
    }
    let event = |name: &str, manual_reset: bool| -> Option<Handle> {
        let name = HSTRING::from(name);
        // SAFETY: creates a named auto/manual-reset event owned by the returned handle.
        unsafe { CreateEventW(None, manual_reset, false, PCWSTR(name.as_ptr())) }
            .ok()
            .map(Handle)
    };
    Some(Instance {
        _mutex: Handle(mutex),
        show: event(SHOW_EVENT, false)?,
        exit: event(EXIT_EVENT, true)?,
    })
}

/// Asks the running copy to show its dashboard.
pub fn signal_show() -> bool {
    signal(SHOW_EVENT)
}

/// Asks the running copy to exit and waits up to `timeout_ms` for it to release the mutex.
pub fn request_exit(timeout_ms: u32) -> bool {
    if !signal(EXIT_EVENT) {
        return true; // nothing running
    }
    let name = HSTRING::from(MUTEX_NAME);
    // SAFETY: opens the mutex only to wait for its release; the handle is owned below.
    let Ok(mutex) =
        (unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, PCWSTR(name.as_ptr())) })
    else {
        return true;
    };
    let mutex = Handle(mutex);
    // SAFETY: waiting on a valid handle. WAIT_ABANDONED also means the owner is gone.
    let result = unsafe { WaitForSingleObject(mutex.0, timeout_ms) };
    result == WAIT_OBJECT_0 || result.0 == 0x80
}

fn signal(name: &str) -> bool {
    let name = HSTRING::from(name);
    // SAFETY: opens an existing event with modify rights only; owned below.
    let Ok(event) = (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) })
    else {
        return false;
    };
    let event = Handle(event);
    // SAFETY: valid event handle.
    unsafe { SetEvent(event.0) }.is_ok()
}

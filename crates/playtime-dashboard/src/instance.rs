//! One dashboard at a time, with the same names the previous dashboard used: a second launch (the tray's *Open
//! dashboard* or *Settings*) brings the open window forward, on the Settings page if asked, and exits.

/// What another launch asked for (only Windows has other launches to hear from).
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Wake {
    Show,
    ShowSettings,
}

#[cfg(windows)]
mod imp {
    use super::Wake;
    use windows::core::w;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, ReleaseMutex, SetEvent, WaitForMultipleObjects,
        EVENT_MODIFY_STATE, INFINITE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

    /// The mutex this process holds while it's the dashboard (as an integer so it can live in a static).
    static HELD: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

    /// Becomes the dashboard, or hands over to the one already open (returning `false`).
    pub fn claim(settings: bool) -> bool {
        // A dashboard that is closing still holds the name but no longer listens: try again for a moment, then take
        // over, so a launch never does nothing.
        for _ in 0..10 {
            match try_claim(settings) {
                Some(claimed) => return claimed,
                None => std::thread::sleep(std::time::Duration::from_millis(300)),
            }
        }
        true
    }

    /// `Some(true)`: this is the dashboard now. `Some(false)`: the open one was asked to come forward. `None`: the
    /// open one isn't listening (it's closing).
    fn try_claim(settings: bool) -> Option<bool> {
        // SAFETY: plain calls with constant names; the handle is kept for the life of the process (or released by
        // `release`).
        unsafe {
            let Ok(mutex) = CreateMutexW(None, true, w!("Local\\PlaytimeTracker.Dashboard")) else {
                // Can't tell: better two windows than none.
                return Some(true);
            };
            if GetLastError() != ERROR_ALREADY_EXISTS {
                HELD.store(mutex.0 as isize, std::sync::atomic::Ordering::Relaxed);
                return Some(true);
            }
            let _ = CloseHandle(mutex);
            // Let the open dashboard come to the front.
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let name = if settings {
                w!("Local\\PlaytimeTracker.Dashboard.ShowSettings")
            } else {
                w!("Local\\PlaytimeTracker.Dashboard.Show")
            };
            let event = OpenEventW(EVENT_MODIFY_STATE, false, name).ok()?;
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
            Some(false)
        }
    }

    /// Lets go of the dashboard's name, so a new one (Reopen now) can take over.
    pub fn release() {
        let held = HELD.swap(0, std::sync::atomic::Ordering::Relaxed);
        if held != 0 {
            let mutex = HANDLE(held as *mut core::ffi::c_void);
            // SAFETY: this process created the mutex and owns it; it's released and closed once.
            unsafe {
                let _ = ReleaseMutex(mutex);
                let _ = CloseHandle(mutex);
            }
        }
    }

    /// Calls `wake` (on a background thread) whenever another launch asks for the window.
    pub fn listen(wake: impl Fn(Wake) + Send + 'static) {
        // SAFETY: plain calls with constant names; the events live as long as the listening thread (the process).
        let events = unsafe {
            (
                CreateEventW(
                    None,
                    false,
                    false,
                    w!("Local\\PlaytimeTracker.Dashboard.Show"),
                ),
                CreateEventW(
                    None,
                    false,
                    false,
                    w!("Local\\PlaytimeTracker.Dashboard.ShowSettings"),
                ),
            )
        };
        let (Ok(show), Ok(settings)) = events else {
            return;
        };
        let handles = [show.0 as isize, settings.0 as isize];
        let _ = std::thread::Builder::new()
            .name("dashboard-instance".into())
            .spawn(move || {
                let handles = handles.map(|h| HANDLE(h as *mut core::ffi::c_void));
                loop {
                    // SAFETY: both handles stay open for the life of the process.
                    let result = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                    match result.0.wrapping_sub(WAIT_OBJECT_0.0) {
                        0 => wake(Wake::Show),
                        1 => wake(Wake::ShowSettings),
                        _ => return,
                    }
                }
            });
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Wake;

    pub fn claim(_settings: bool) -> bool {
        true
    }

    pub fn release() {}

    pub fn listen(_wake: impl Fn(Wake) + Send + 'static) {}
}

pub use imp::{claim, listen, release};

/// Brings the window to the front (restoring it if minimised).
#[cfg(windows)]
pub fn bring_to_front(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    if hwnd == 0 {
        return;
    }
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    // SAFETY: the handle belongs to this process's live window.
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(not(windows))]
pub fn bring_to_front(_hwnd: isize) {}

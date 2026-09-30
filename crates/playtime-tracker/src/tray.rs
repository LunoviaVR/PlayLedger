//! The tray icon and the hidden window that drives everything on the main thread: the poll timer, power and
//! shutdown messages, the tray menu, notifications, and "show"/"exit" requests from other copies.

use crate::instance::Instance;
use crate::service::{Notification, Service};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{WaitForMultipleObjects, INFINITE};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO,
    NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK,
    NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics,
    KillTimer, LoadIconW, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetMenuDefaultItem, SetTimer, TrackPopupMenu, TranslateMessage, HICON,
    IDI_APPLICATION, LR_DEFAULTCOLOR, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, PBT_APMSUSPEND,
    SM_CXSMICON, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_COMMAND,
    WM_CONTEXTMENU, WM_DESTROY, WM_ENDSESSION, WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_TIMER,
    WNDCLASSW, WS_OVERLAPPED,
};

const WM_TRAY: u32 = WM_APP + 1;
const WM_SHOW_REQUEST: u32 = WM_APP + 2;
const WM_EXIT_REQUEST: u32 = WM_APP + 3;
const POLL_TIMER: usize = 1;
const CMD_OPEN: usize = 1;
const CMD_SETTINGS: usize = 2;
const CMD_EXIT: usize = 3;
const CMD_UPDATE: usize = 4;
const ICON_ID: u32 = 1;

/// The dashboard: installed in a `Dashboard` folder next to the tracker (or right next to it in a dev build).
const DASHBOARD_EXE: &str = "PlaytimeTracker.Dashboard.exe";
const DASHBOARD_FOLDER: &str = "Dashboard";

static APP_ICON: &[u8] = include_bytes!("../../../assets/app.ico");

struct Tray {
    hwnd: HWND,
    service: Arc<Mutex<Service>>,
    icon: HICON,
    taskbar_created: u32,
    poll_ms: Cell<u32>,
}

thread_local! {
    static TRAY: RefCell<Option<Rc<Tray>>> = const { RefCell::new(None) };
}

/// Runs `f` with the tray state. The borrow is released before `f` runs, because menus and `DestroyWindow`
/// dispatch messages back into the window procedure while they run.
fn with_tray<R>(f: impl FnOnce(&Tray) -> R) -> Option<R> {
    let tray = TRAY.with(|tray| tray.borrow().clone())?;
    Some(f(&tray))
}

fn wide<const N: usize>(text: &str) -> [u16; N] {
    let mut out = [0u16; N];
    for (slot, unit) in out.iter_mut().take(N - 1).zip(text.encode_utf16()) {
        *slot = unit;
    }
    out
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        out.push_str("...");
        out
    }
}

/// The tray icon from the app's .ico, at the small-icon size; Windows' default icon if that fails.
fn load_icon() -> HICON {
    // SAFETY: plain metric query.
    let want = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let read_u16 = |at: usize| {
        APP_ICON
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let read_u32 = |at: usize| {
        APP_ICON
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let count = read_u16(4).unwrap_or(0) as usize;
    // (size, offset, length) of each image in the .ico directory.
    let mut images: Vec<(i32, usize, usize)> = (0..count)
        .filter_map(|i| {
            let entry = 6 + i * 16;
            let width = i32::from(*APP_ICON.get(entry)?);
            let size = if width == 0 { 256 } else { width };
            Some((
                size,
                read_u32(entry + 12)? as usize,
                read_u32(entry + 8)? as usize,
            ))
        })
        .collect();
    // Smallest image at least as big as wanted, else the biggest.
    images.sort_by_key(|(size, _, _)| (*size < want, if *size >= want { *size } else { -*size }));
    if let Some(bytes) = images
        .first()
        .and_then(|(_, offset, length)| APP_ICON.get(*offset..offset.checked_add(*length)?))
    {
        // SAFETY: `bytes` is one complete icon image from our embedded .ico.
        if let Ok(icon) = unsafe {
            CreateIconFromResourceEx(bytes, true, 0x0003_0000, want, want, LR_DEFAULTCOLOR)
        } {
            return icon;
        }
    }
    // SAFETY: loading a stock system icon.
    unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default()
}

fn notify_data(tray: &Tray) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: tray.hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

fn add_icon(tray: &Tray, tooltip: &str) {
    let mut data = notify_data(tray);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = tray.icon;
    data.szTip = wide(&truncate(tooltip, 127));
    // SAFETY: `data` is fully initialised for these calls.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
    }
}

fn set_tooltip(tray: &Tray, tooltip: &str) {
    let mut data = notify_data(tray);
    data.uFlags = NIF_TIP | NIF_SHOWTIP;
    data.szTip = wide(&truncate(tooltip, 127));
    // SAFETY: `data` is initialised for NIM_MODIFY.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

fn show_notification(tray: &Tray, notification: &Notification) {
    let mut data = notify_data(tray);
    data.uFlags = NIF_INFO;
    data.szInfoTitle = wide(&truncate(&notification.title, 63));
    data.szInfo = wide(&truncate(&notification.body, 255));
    data.dwInfoFlags = if notification.warning {
        NIIF_WARNING
    } else {
        NIIF_INFO
    };
    // SAFETY: `data` is initialised for NIM_MODIFY.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

fn remove_icon(tray: &Tray) {
    let data = notify_data(tray);
    // SAFETY: removing our own icon.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

fn status_tooltip(service: &Service) -> String {
    format!("Playtime Tracker\n{}", service.status())
}

/// The dashboard executable next to the tracker, if it's installed.
pub fn dashboard_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [
        dir.join(DASHBOARD_FOLDER).join(DASHBOARD_EXE),
        dir.join(DASHBOARD_EXE),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

/// Opens the dashboard (optionally on a page), or the data folder if the dashboard isn't installed.
pub fn open_dashboard(page: Option<&str>, data_folder: &Path) {
    match dashboard_path() {
        Some(path) => {
            let mut command = std::process::Command::new(path);
            if let Some(page) = page {
                command.args(["--page", page]);
            }
            let _ = command.spawn();
        }
        None => {
            let _ = std::process::Command::new("explorer.exe")
                .arg(data_folder)
                .spawn();
        }
    }
}

fn open(tray: &Tray, page: Option<&str>) {
    let folder = tray
        .service
        .lock()
        .map(|s| s.data_folder().clone())
        .unwrap_or_default();
    open_dashboard(page, &folder);
}

fn show_menu(tray: &Tray) {
    let (status, update) = tray
        .service
        .lock()
        .map(|s| {
            let update = s
                .update
                .available
                .as_ref()
                .filter(|u| s.is_installed() && u.can_install() && !s.update.busy)
                .map(|u| format!("Install update {}", u.version));
            (s.status(), update)
        })
        .unwrap_or_default();
    // SAFETY: builds, shows and destroys our own popup menu; strings outlive the calls.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let status = HSTRING::from(truncate(&status, 100));
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(status.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN, w!("Open dashboard"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_SETTINGS, w!("Settings"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let update = update.map(HSTRING::from);
        if let Some(update) = &update {
            let _ = AppendMenuW(menu, MF_STRING, CMD_UPDATE, PCWSTR(update.as_ptr()));
        }
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("Exit"));
        let _ = SetMenuDefaultItem(menu, CMD_OPEN as u32, 0);
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(tray.hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            point.x,
            point.y,
            None,
            tray.hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
    }
}

fn poll(tray: &Tray) {
    let Ok(mut service) = tray.service.lock() else {
        return;
    };
    let mut notifications = service.tick();
    notifications.extend(service.take_notifications());
    let tooltip = status_tooltip(&service);
    let poll_ms = service.poll_interval_ms();
    drop(service);
    set_tooltip(tray, &tooltip);
    for notification in &notifications {
        show_notification(tray, notification);
    }
    if poll_ms != tray.poll_ms.get() {
        tray.poll_ms.set(poll_ms);
        // SAFETY: replaces our own timer.
        unsafe { SetTimer(Some(tray.hwnd), POLL_TIMER, poll_ms, None) };
    }
}

fn exit(tray: &Tray) {
    if let Ok(mut service) = tray.service.lock() {
        service.shutdown();
    }
    // SAFETY: destroying our own window; WM_DESTROY removes the icon and ends the loop.
    unsafe {
        let _ = DestroyWindow(tray.hwnd);
    }
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let handled = with_tray(|tray| {
        match message {
            WM_TIMER if wparam.0 == POLL_TIMER => poll(tray),
            WM_TRAY => match (lparam.0 & 0xFFFF) as u32 {
                NIN_SELECT | NIN_BALLOONUSERCLICK => open(tray, None),
                WM_CONTEXTMENU => show_menu(tray),
                _ => {}
            },
            WM_COMMAND => match wparam.0 & 0xFFFF {
                CMD_OPEN => open(tray, None),
                CMD_SETTINGS => open(tray, Some("settings")),
                CMD_EXIT => exit(tray),
                CMD_UPDATE => {
                    if let Ok(service) = tray.service.lock() {
                        service.request_update(crate::service::UpdateCommand::InstallNow);
                    }
                }
                _ => {}
            },
            WM_POWERBROADCAST if wparam.0 as u32 == PBT_APMSUSPEND => {
                if let Ok(mut service) = tray.service.lock() {
                    service.suspend();
                }
            }
            WM_QUERYENDSESSION => return Some(LRESULT(1)),
            WM_ENDSESSION if wparam.0 != 0 => {
                if let Ok(mut service) = tray.service.lock() {
                    service.shutdown();
                }
            }
            WM_SHOW_REQUEST => open(tray, None),
            WM_EXIT_REQUEST => exit(tray),
            WM_DESTROY => {
                remove_icon(tray);
                // SAFETY: stopping our own timer and ending the message loop.
                unsafe {
                    let _ = KillTimer(Some(tray.hwnd), POLL_TIMER);
                    PostQuitMessage(0);
                }
            }
            m if m == tray.taskbar_created && m != 0 => {
                // Explorer restarted: put the icon back.
                let tooltip = tray
                    .service
                    .lock()
                    .map(|s| status_tooltip(&s))
                    .unwrap_or_default();
                add_icon(tray, &tooltip);
            }
            _ => return None,
        }
        Some(LRESULT(0))
    })
    .flatten();
    // SAFETY: default handling for everything we didn't handle.
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, message, wparam, lparam) })
}

/// Runs the tray until exit. `notifications` are shown once the icon is up.
pub fn run(
    service: Arc<Mutex<Service>>,
    instance: Instance,
    notifications: Vec<Notification>,
) -> i32 {
    // SAFETY: registering our window class and creating a hidden window owned by this thread.
    let hwnd = unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return 1;
        };
        let class = w!("PlaytimeTrackerTray");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return 1;
        }
        match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("Playtime Tracker"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(_) => return 1,
        }
    };

    let (tooltip, poll_ms) = match service.lock() {
        Ok(s) => (status_tooltip(&s), s.poll_interval_ms()),
        Err(_) => return 1,
    };
    let tray = Tray {
        hwnd,
        service,
        icon: load_icon(),
        // SAFETY: registering the shell's broadcast message name.
        taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        poll_ms: Cell::new(poll_ms),
    };
    add_icon(&tray, &tooltip);
    for notification in &notifications {
        show_notification(&tray, notification);
    }
    // SAFETY: a timer on our own window.
    unsafe { SetTimer(Some(hwnd), POLL_TIMER, poll_ms, None) };
    TRAY.with(|t| *t.borrow_mut() = Some(Rc::new(tray)));

    // Another copy asking to show the dashboard or exit arrives as kernel events; forward them as messages.
    let target = hwnd.0 as isize;
    let _ = std::thread::Builder::new()
        .name("instance-events".into())
        .spawn(move || {
            // The thread owns the instance (and so the single-instance mutex) until the process ends.
            let instance = instance;
            let handles = [instance.show.0, instance.exit.0];
            loop {
                // SAFETY: waiting on our own event handles, which live as long as `instance` (moved here).
                let result = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                let message = match result.0.wrapping_sub(WAIT_OBJECT_0.0) {
                    0 => WM_SHOW_REQUEST,
                    1 => WM_EXIT_REQUEST,
                    _ => return,
                };
                // SAFETY: posting to our window by handle value; harmless if it's already gone.
                let posted = unsafe {
                    PostMessageW(Some(HWND(target as *mut _)), message, WPARAM(0), LPARAM(0))
                }
                .is_ok();
                if !posted || message == WM_EXIT_REQUEST {
                    return;
                }
            }
        });

    // First poll right away.
    with_tray(poll);

    let mut message = MSG::default();
    // SAFETY: the standard message loop for this thread.
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    TRAY.with(|t| t.borrow_mut().take());
    0
}

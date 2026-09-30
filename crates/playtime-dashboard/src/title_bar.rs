//! The window's title bar follows the page: light or dark, and true black in AMOLED mode (Windows 11 draws the caption
//! colour it's given; earlier versions keep their own).

use std::sync::atomic::{AtomicBool, Ordering};

/// The user's AMOLED mode choice, kept until the settings arrive so the window starts with the last one applied.
static AMOLED: AtomicBool = AtomicBool::new(false);

/// The AMOLED mode choice last applied (off before the settings arrive).
pub fn amoled() -> bool {
    AMOLED.load(Ordering::Relaxed)
}

/// Gives the window with handle `hwnd` a light or dark title bar, true black in AMOLED mode with the dark theme.
pub fn apply(hwnd: isize, dark: bool, amoled: bool) {
    AMOLED.store(amoled, Ordering::Relaxed);
    if hwnd != 0 {
        system::set(hwnd, dark, dark && amoled);
    }
}

#[cfg(windows)]
mod system {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
    };

    pub fn set(hwnd: isize, dark: bool, black: bool) {
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        let dark = BOOL::from(dark);
        let caption = COLORREF(if black { 0 } else { DWMWA_COLOR_DEFAULT });
        // SAFETY: the window handle belongs to this process's live window; each value outlives its call and the
        // sizes match the values.
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark as *const BOOL).cast(),
                std::mem::size_of::<BOOL>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                (&caption as *const COLORREF).cast(),
                std::mem::size_of::<COLORREF>() as u32,
            );
        }
    }
}

#[cfg(not(windows))]
mod system {
    pub fn set(_hwnd: isize, _dark: bool, _black: bool) {}
}

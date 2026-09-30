//! The glass look (phase 15): Windows' own Acrylic material behind the window, so what's behind it shows through,
//! blurred, with WinUI's translucent card and layer colours over it. The material is extended over the whole window
//! (not just the frame), so the page's transparent pixels show it instead of drawing dark. Windows itself turns the
//! material solid when the window isn't focused or in energy saver;
//! the dashboard uses plain solid colours when the user turned glass off, when Windows' transparency effects are off,
//! with a high-contrast theme, on Windows versions without Acrylic, and with the software renderer (which can't draw
//! see-through pixels).

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether this window draws with the GPU (set once at start).
pub static GPU: AtomicBool = AtomicBool::new(false);

/// The user's Glass effects choice: on until the settings say otherwise, so the window starts with the glass look.
static WANTED: AtomicBool = AtomicBool::new(true);

/// The Glass effects choice last applied (on before the settings arrive).
pub fn wanted() -> bool {
    WANTED.load(Ordering::Relaxed)
}

/// Turns the material on or off for the window with handle `hwnd`, with a dark or light title bar. Returns whether
/// the glass look is in use (so the page draws see-through surfaces).
pub fn apply(hwnd: isize, wanted: bool, dark: bool) -> bool {
    WANTED.store(wanted, Ordering::Relaxed);
    let on = wanted && GPU.load(Ordering::Relaxed) && hwnd != 0 && system::allows_transparency();
    system::set_backdrop(hwnd, on, dark) && on
}

#[cfg(windows)]
mod system {
    use windows::core::w;
    use windows::core::BOOL;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW,
        DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWM_SYSTEMBACKDROP_TYPE,
    };
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows::Win32::UI::Controls::MARGINS;
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    /// Windows' "Transparency effects" is on (the default) and no high-contrast theme is active.
    pub fn allows_transparency() -> bool {
        let mut value = 1u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: `value` and `size` outlive the call and `size` is the size of `value`. A missing value keeps 1.
        let _ = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("EnableTransparency"),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut value as *mut u32).cast()),
                Some(&mut size),
            )
        };
        let mut contrast = HIGHCONTRASTW {
            cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        // SAFETY: `contrast` is a valid HIGHCONTRASTW whose size is given, and outlives the call.
        let high_contrast = unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .is_ok()
            && contrast.dwFlags.contains(HCF_HIGHCONTRASTON);
        value != 0 && !high_contrast
    }

    /// Sets the title bar's light or dark look and the backdrop material. Returns whether Windows accepted the
    /// material (it doesn't before Windows 11 22H2).
    pub fn set_backdrop(hwnd: isize, on: bool, dark: bool) -> bool {
        if hwnd == 0 {
            return false;
        }
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        let dark = BOOL::from(dark);
        // LAB (temporary, for reproducing the glass on CI): PLAYTIME_GLASS_LAB=mica,noextend,noblur.
        let lab = std::env::var("PLAYTIME_GLASS_LAB").unwrap_or_default();
        let custom = lab.contains("swca") || lab.contains("blur3");
        let kind: DWM_SYSTEMBACKDROP_TYPE = if !on || custom {
            DWMSBT_NONE
        } else if lab.contains("mica") {
            windows::Win32::Graphics::Dwm::DWMSBT_MAINWINDOW
        } else {
            DWMSBT_TRANSIENTWINDOW
        };
        // -1 on every side extends the frame, and with it the backdrop, over the whole client area.
        let edge = if on && !lab.contains("noextend") && !custom {
            -1
        } else {
            0
        };
        let margins = MARGINS {
            cxLeftWidth: edge,
            cxRightWidth: edge,
            cyTopHeight: edge,
            cyBottomHeight: edge,
        };
        // SAFETY: the window handle belongs to this process's live window; each value outlives its call and the
        // sizes match the values.
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark as *const BOOL).cast(),
                std::mem::size_of::<BOOL>() as u32,
            );
            let accepted = DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                (&kind as *const DWM_SYSTEMBACKDROP_TYPE).cast(),
                std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
            )
            .is_ok();
            // LAB: Windows' blur-behind (SetWindowCompositionAttribute) with our own tint instead of a system backdrop.
            if on && custom {
                super::swca_blur(hwnd, if lab.contains("blur3") { 3 } else { 4 }, 0x6620_2020);
            }
            if on && lab.contains("noblur") {
                let off = windows::Win32::Graphics::Dwm::DWM_BLURBEHIND {
                    dwFlags: windows::Win32::Graphics::Dwm::DWM_BB_ENABLE,
                    fEnable: false.into(),
                    ..Default::default()
                };
                let _ = windows::Win32::Graphics::Dwm::DwmEnableBlurBehindWindow(hwnd, &off);
            }
            if let Ok(log) = std::env::var("PLAYTIME_GLASS_LOG") {
                let _ = std::fs::write(
                    log,
                    format!(
                        "lab={lab} on={on} dark={} accepted={accepted}\n",
                        dark.as_bool()
                    ),
                );
            }
            // Only extend the frame when the backdrop is there to fill it (otherwise it would draw black).
            let _ = DwmExtendFrameIntoClientArea(
                hwnd,
                &if accepted {
                    margins
                } else {
                    MARGINS::default()
                },
            );
            accepted
        }
    }
}

#[cfg(windows)]
/// LAB: `SetWindowCompositionAttribute` accent (3 blur, 4 acrylic blur) with an AABBGGRR tint.
fn swca_blur(hwnd: windows::Win32::Foundation::HWND, state: u32, tint: u32) {
    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient: u32,
        animation: u32,
    }
    #[repr(C)]
    struct Data {
        attribute: u32,
        data: *mut core::ffi::c_void,
        size: usize,
    }
    type Swca = unsafe extern "system" fn(windows::Win32::Foundation::HWND, *mut Data) -> i32;
    // SAFETY: user32 is loaded in every GUI process; the function is looked up by name and called with the layout
    // Windows uses for WCA_ACCENT_POLICY (19); both structs outlive the call.
    unsafe {
        use windows::core::s;
        use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
        let Ok(user32) = GetModuleHandleA(s!("user32.dll")) else {
            return;
        };
        let Some(proc) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else {
            return;
        };
        let swca: Swca = std::mem::transmute(proc);
        let mut policy = AccentPolicy {
            state,
            flags: 2,
            gradient: tint,
            animation: 0,
        };
        let mut data = Data {
            attribute: 19,
            data: (&mut policy as *mut AccentPolicy).cast(),
            size: std::mem::size_of::<AccentPolicy>(),
        };
        swca(hwnd, &mut data);
    }
}

#[cfg(not(windows))]
mod system {
    pub fn allows_transparency() -> bool {
        false
    }

    pub fn set_backdrop(_hwnd: isize, _on: bool, _dark: bool) -> bool {
        false
    }
}

//! Opening things outside the dashboard: the two fixed web pages (in the default browser), the report, and the data
//! folder. Nothing is sent anywhere by the dashboard itself; the browser does the rest after the user's click.

use std::path::Path;

/// The Tip button (phase 14).
pub const DONATE_URL: &str = "https://cash.app/$LunoviaVR";
/// Where a signed-in SteamGridDB user creates their free API key (phase 16).
pub const STEAMGRIDDB_KEY_URL: &str = "https://www.steamgriddb.com/profile/preferences/api";

/// Opens one of the fixed pages above. Anything else is refused, so a changed or injected address can't be opened.
pub fn url(address: &str) -> bool {
    let allowed = (address == DONATE_URL && address.starts_with("https://cash.app/"))
        || (address == STEAMGRIDDB_KEY_URL && address.starts_with("https://www.steamgriddb.com/"));
    allowed && shell_open(address)
}

/// Opens a file with its default program (the report opens in the text editor).
pub fn file(path: &Path) -> bool {
    path.is_file() && shell_open(&path.to_string_lossy())
}

/// Opens a folder in Explorer.
pub fn folder(path: &Path) -> bool {
    path.is_dir()
        && std::process::Command::new("explorer.exe")
            .arg(path)
            .spawn()
            .is_ok()
}

#[cfg(windows)]
fn shell_open(target: &str) -> bool {
    use windows::core::{w, HSTRING};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let target = HSTRING::from(target);
    // SAFETY: the strings outlive the call; no owner window is needed.
    let result = unsafe { ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL) };
    // Values above 32 mean success.
    result.0 as isize > 32
}

#[cfg(not(windows))]
fn shell_open(_target: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_fixed_pages_open() {
        for refused in [
            "https://cash.app/$Someone",
            "http://cash.app/$LunoviaVR",
            "https://cash.app.evil.example/$LunoviaVR",
            "https://www.steamgriddb.com/profile/preferences/api?x=1",
            "file:///C:/Windows/System32/calc.exe",
        ] {
            assert!(!url(refused), "{refused}");
        }
    }
}

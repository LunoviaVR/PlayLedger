//! "Start with Windows" (per user, no admin), exactly as the C# app does it: the value `PlaytimeTracker` under
//! `HKCU\...\CurrentVersion\Run` = `"<exe>" --startup`, respecting Task Manager's on/off switch
//! (`...\Explorer\StartupApproved\Run`), and moving the pre-rename `GameSessionTracker` entry over once.

use crate::reg::RegKey;
use playtime_core::discovery::{Hive, RegView};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Where Task Manager's "Startup apps" page records apps you've disabled.
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const VALUE_NAME: &str = "PlaytimeTracker";
const LEGACY_VALUE_NAME: &str = "GameSessionTracker";
/// The installer's Apps entry; its InstallLocation says where the installed copy lives.
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\PlaytimeTracker";

fn command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" --startup", exe.display()))
}

/// True if the Run entry exists and Task Manager hasn't disabled it.
pub fn is_enabled() -> bool {
    let Some(run) = RegKey::open(Hive::CurrentUser, RegView::Default, RUN_KEY) else {
        return false;
    };
    if run.string(VALUE_NAME).is_none() {
        return false;
    }
    // First byte even (0x02) = enabled, odd (0x03) = disabled in Task Manager; no value = enabled.
    RegKey::open(Hive::CurrentUser, RegView::Default, APPROVED_KEY)
        .and_then(|approved| approved.binary(VALUE_NAME))
        .and_then(|state| state.first().copied())
        .is_none_or(|first| first % 2 == 0)
}

pub fn set_enabled(enabled: bool) -> bool {
    let Some(run) = RegKey::create_current_user(RUN_KEY) else {
        return false;
    };
    if enabled {
        let Some(command) = command() else {
            return false;
        };
        if !run.set_string(VALUE_NAME, &command) {
            return false;
        }
        // Clear a "disabled" flag Task Manager may have set.
        if let Some(approved) = RegKey::open_current_user_writable(APPROVED_KEY) {
            approved.delete_value(VALUE_NAME);
        }
    } else {
        run.delete_value(VALUE_NAME);
    }
    true
}

/// Moves a startup entry written under the old app name to the new one, keeping Task Manager's on/off state.
pub fn migrate_legacy_entry() {
    let Some(run) = RegKey::open_current_user_writable(RUN_KEY) else {
        return;
    };
    if run.string(LEGACY_VALUE_NAME).is_none() {
        return;
    }
    let Some(command) = command() else {
        return;
    };
    let approved = RegKey::open_current_user_writable(APPROVED_KEY);
    let state = approved.as_ref().and_then(|a| a.binary(LEGACY_VALUE_NAME));
    run.set_string(VALUE_NAME, &command);
    run.delete_value(LEGACY_VALUE_NAME);
    if let Some(approved) = approved {
        if let Some(state) = state {
            approved.set_binary(VALUE_NAME, &state);
        }
        approved.delete_value(LEGACY_VALUE_NAME);
    }
}

/// If startup is on but points somewhere else (e.g. the old C# exe after an upgrade), point it at this exe.
pub fn refresh_path_if_enabled() {
    let (Some(run), Some(command)) = (RegKey::open_current_user_writable(RUN_KEY), command())
    else {
        return;
    };
    if run
        .string(VALUE_NAME)
        .is_some_and(|current| !current.eq_ignore_ascii_case(&command))
    {
        run.set_string(VALUE_NAME, &command);
    }
}

/// True if this exe is the copy the installer put in place (the Apps entry's InstallLocation is its folder).
/// Only an installed copy manages "Start with Windows" or updates itself; a copy run from elsewhere never does.
pub fn is_installed_copy() -> bool {
    let Some(location) = RegKey::open(Hive::CurrentUser, RegView::Default, UNINSTALL_KEY)
        .and_then(|key| key.string("InstallLocation"))
    else {
        return false;
    };
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
    else {
        return false;
    };
    let normalize = |p: &str| playtime_core::paths::key(&playtime_core::paths::normalize(p));
    normalize(&location) == normalize(&exe_dir.to_string_lossy())
}

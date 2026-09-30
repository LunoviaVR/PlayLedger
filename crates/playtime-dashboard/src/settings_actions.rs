//! What the Settings page's controls do: every change is saved to the tracker straight away (numbers a moment after
//! the last change), off the UI thread.

use crate::client::ClientError;
use crate::settings::{self, UpdateStatus};
use crate::ui::{AppWindow, SettingsData};
use crate::{accent, dialogs, open, open_dialog, request_in_background, Refresh, APP};
use playtime_core::ipc::{Request, Response};
use playtime_core::settings::{CustomGame, Settings};
use slint::{ComponentHandle, SharedString, Weak};
use std::sync::mpsc::Sender;
use std::time::Duration;

fn update_status(response: &Response) -> Option<UpdateStatus> {
    match response {
        Response::UpdateStatus {
            current_version,
            available_version,
            can_install,
            busy,
            last_error,
            last_checked,
        } => Some(UpdateStatus {
            current_version: current_version.clone(),
            available_version: available_version.clone(),
            can_install: *can_install,
            busy: *busy,
            last_error: last_error.clone(),
            last_checked: last_checked.clone(),
        }),
        _ => None,
    }
}

fn error(weak: &Weak<AppWindow>, e: ClientError) {
    open_dialog(weak, |_| Some(dialogs::Dialog::Error(e.to_string())));
}

/// Asks the tracker for the settings, the update status and its version, then fills the page and applies the
/// appearance settings.
pub fn load(weak: Weak<AppWindow>) {
    let _ = std::thread::Builder::new()
        .name("settings".into())
        .spawn(move || {
            let tracker = crate::client::Tracker::default();
            let loaded = tracker.request(&Request::GetSettings);
            let status = tracker.request(&Request::GetUpdateStatus).ok();
            let version = match tracker.request(&Request::Hello { protocol: 1 }) {
                Ok(Response::Hello { version, .. }) => version,
                _ => "unknown".into(),
            };
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = weak.upgrade() else {
                    return;
                };
                let data = window.global::<SettingsData>();
                match loaded {
                    Ok(Response::Settings {
                        settings: loaded,
                        has_steam_grid_db_key,
                        start_with_windows,
                        is_installed_copy,
                    }) => {
                        APP.with_borrow_mut(|app| {
                            settings::fill(
                                &window,
                                &mut app.settings,
                                *loaded,
                                has_steam_grid_db_key,
                                start_with_windows,
                                is_installed_copy,
                            )
                        });
                        data.set_about(SharedString::from(format!(
                            "PlayLedger {version} (dashboard {}). Your play history stays on this PC. \
                             The dashboard is built with Slint (slint.dev), used under the GPL-3.0.",
                            env!("CARGO_PKG_VERSION")
                        )));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        data.set_loaded(false);
                        data.set_problem(SharedString::from(e.to_string()));
                    }
                }
                if let Some(status) = status.as_ref().and_then(update_status) {
                    settings::show_update_status(&window, &status);
                    watch_updates(&window, status.busy);
                }
            });
        });
}

/// Sends the settings as they are now (after a change on the page).
fn save(window: &AppWindow, refresh: &Sender<Refresh>) {
    let Some(new) = APP.with_borrow_mut(|app| settings::read(window, &mut app.settings)) else {
        return;
    };
    settings::apply_appearance(window, &new);
    send(window, refresh, new);
}

fn send(window: &AppWindow, refresh: &Sender<Refresh>, new: Settings) {
    let weak = window.as_weak();
    let refresh = refresh.clone();
    request_in_background(
        Request::UpdateSettings {
            settings: Box::new(new),
        },
        move |result| match result {
            Ok(_) => {
                let _ = refresh.send(Refresh::Full);
            }
            Err(e) => error(&weak, e),
        },
    );
}

/// Changes the settings in code (lists, accent), then shows and saves them.
fn change(window: &AppWindow, refresh: &Sender<Refresh>, edit: impl FnOnce(&mut Settings)) {
    // Keep the page's other values: read them first, then apply the edit.
    let Some(new) = APP.with_borrow_mut(|app| {
        settings::read(window, &mut app.settings)?;
        let current = app.settings.settings.as_mut()?;
        edit(current);
        let new = current.clone();
        settings::show_lists(window, &mut app.settings);
        Some(new)
    }) else {
        return;
    };
    settings::apply_appearance(window, &new);
    send(window, refresh, new);
}

/// While an update check or download runs, keeps its status current.
fn watch_updates(window: &AppWindow, busy: bool) {
    APP.with_borrow(|app| {
        if !busy {
            app.update_timer.stop();
            return;
        }
        let weak = window.as_weak();
        app.update_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(2),
            move || {
                let weak = weak.clone();
                request_in_background(Request::GetUpdateStatus, move |result| {
                    let Some(window) = weak.upgrade() else {
                        return;
                    };
                    match result.ok().as_ref().and_then(update_status) {
                        Some(status) => {
                            settings::show_update_status(&window, &status);
                            watch_updates(&window, status.busy);
                        }
                        None => watch_updates(&window, false),
                    }
                });
            },
        );
    });
}

fn update_request(window: &AppWindow, request: Request) {
    let weak = window.as_weak();
    request_in_background(request, move |result| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        match result {
            Ok(response) => {
                if let Some(status) = update_status(&response) {
                    settings::show_update_status(&window, &status);
                    watch_updates(&window, true);
                }
            }
            Err(e) => error(&weak, e),
        }
    });
}

/// The data folder (`Documents\Playtime Tracker`).
#[cfg(windows)]
fn data_folder() -> Option<std::path::PathBuf> {
    playtime_windows::folders::documents().map(|d| playtime_windows::folders::data_folder(&d))
}

#[cfg(not(windows))]
fn data_folder() -> Option<std::path::PathBuf> {
    None
}

/// Adds the custom game named in the dialog.
pub fn add_custom_game(window: &AppWindow, refresh: &Sender<Refresh>, path: String) -> bool {
    let name = window.get_dialog_input_text().trim().to_string();
    if name.is_empty() {
        return false;
    }
    change(window, refresh, |s| {
        s.custom_games.push(CustomGame {
            name,
            executable: path,
        })
    });
    true
}

pub fn wire(window: &AppWindow, refresh: &Sender<Refresh>) {
    let data = window.global::<SettingsData>();

    let (weak, r) = (window.as_weak(), refresh.clone());
    data.on_changed(move || {
        if let Some(window) = weak.upgrade() {
            save(&window, &r);
        }
    });
    let (weak, r) = (window.as_weak(), refresh.clone());
    data.on_theme_changed(move || {
        if let Some(window) = weak.upgrade() {
            save(&window, &r);
        }
    });
    // Numbers save a moment after the last change rather than on every step.
    let (weak, r) = (window.as_weak(), refresh.clone());
    data.on_numbers_changed(move || {
        let (weak, r) = (weak.clone(), r.clone());
        APP.with_borrow(|app| {
            app.save_timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(600),
                move || {
                    if let Some(window) = weak.upgrade() {
                        save(&window, &r);
                    }
                },
            )
        });
    });
    let (weak, r) = (window.as_weak(), refresh.clone());
    data.on_accent_changed(move || {
        let Some(window) = weak.upgrade() else { return };
        let index = window.global::<SettingsData>().get_accent();
        // The last entry ("Custom…") shows the colour box; the colour is used once it's applied.
        let key = match index {
            0 => accent::WINDOWS.to_string(),
            i => match accent::PRESETS.get(i as usize - 1) {
                Some(preset) => preset.0.to_string(),
                None => return,
            },
        };
        change(&window, &r, |s| s.accent_color = key);
    });
    let (weak, r) = (window.as_weak(), refresh.clone());
    data.on_apply_custom_accent(move || {
        let Some(window) = weak.upgrade() else { return };
        let data = window.global::<SettingsData>();
        match accent::parse_hex(&data.get_custom_accent()) {
            Some(color) => {
                data.set_custom_accent_valid(true);
                change(&window, &r, |s| s.accent_color = accent::to_hex(color));
            }
            None => data.set_custom_accent_valid(false),
        }
    });
    let weak = window.as_weak();
    data.on_start_with_windows_changed(move || {
        let Some(window) = weak.upgrade() else { return };
        let enabled = window.global::<SettingsData>().get_start_with_windows();
        let weak = weak.clone();
        request_in_background(Request::SetStartWithWindows { enabled }, move |result| {
            if let Err(e) = result {
                if let Some(window) = weak.upgrade() {
                    window
                        .global::<SettingsData>()
                        .set_start_with_windows(!enabled);
                }
                error(&weak, e);
            }
        });
    });

    // SteamGridDB key: sent to the tracker, which keeps it in Credential Manager; the box is cleared at once.
    let weak = window.as_weak();
    data.on_save_key(move || {
        let Some(window) = weak.upgrade() else { return };
        let data = window.global::<SettingsData>();
        let key = data.get_key_text().trim().to_string();
        if key.is_empty() {
            return;
        }
        data.set_key_text(SharedString::default());
        let weak = weak.clone();
        request_in_background(
            Request::SetSteamGridDbKey { key: Some(key) },
            move |result| match result {
                Ok(_) => {
                    if let Some(window) = weak.upgrade() {
                        window.global::<SettingsData>().set_has_key(true);
                    }
                }
                Err(e) => error(&weak, e),
            },
        );
    });
    let weak = window.as_weak();
    data.on_remove_key(move || {
        let weak = weak.clone();
        request_in_background(
            Request::SetSteamGridDbKey { key: None },
            move |result| match result {
                Ok(_) => {
                    if let Some(window) = weak.upgrade() {
                        window.global::<SettingsData>().set_has_key(false);
                    }
                }
                Err(e) => error(&weak, e),
            },
        );
    });
    data.on_get_key(|| {
        open::url(open::STEAMGRIDDB_KEY_URL);
    });
    data.on_donate(|| {
        open::url(open::DONATE_URL);
    });

    // Lists
    let weak = window.as_weak();
    data.on_add_custom_game(move || {
        let Some(window) = weak.upgrade() else { return };
        #[cfg(windows)]
        {
            let owner = crate::owner_of(&window);
            let weak = weak.clone();
            let _ = std::thread::Builder::new()
                .name("open".into())
                .spawn(move || {
                    use playtime_windows::file_dialogs::{open, Filter};
                    let filters = [Filter {
                        name: "Programs",
                        patterns: "*.exe",
                    }];
                    let Some(path) = open(owner, "Choose the game's program", &filters) else {
                        return;
                    };
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(window) = weak.upgrade() else { return };
                        let name = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        open_dialog(&weak, |_| {
                            Some(dialogs::Dialog::NameGame(
                                path.to_string_lossy().into_owned(),
                            ))
                        });
                        window.set_dialog_input_text(SharedString::from(name));
                    });
                });
        }
        #[cfg(not(windows))]
        let _ = window;
    });
    let (weak, r) = (window.as_weak(), refresh.clone());
    data.on_add_folder(move || {
        let Some(window) = weak.upgrade() else { return };
        #[cfg(windows)]
        {
            let owner = crate::owner_of(&window);
            let (weak, r) = (weak.clone(), r.clone());
            let _ = std::thread::Builder::new()
                .name("folder".into())
                .spawn(move || {
                    let Some(path) =
                        playtime_windows::file_dialogs::folder(owner, "Choose a games folder")
                    else {
                        return;
                    };
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(window) = weak.upgrade() else { return };
                        let path = path.to_string_lossy().into_owned();
                        change(&window, &r, |s| {
                            if !s
                                .extra_game_folders
                                .iter()
                                .any(|f| f.eq_ignore_ascii_case(&path))
                            {
                                s.extra_game_folders.push(path);
                            }
                        });
                    });
                });
        }
        #[cfg(not(windows))]
        let _ = (window, &r);
    });
    let (weak, r) = (window.as_weak(), refresh.clone());
    data.on_add_ignored_game(move || {
        let Some(window) = weak.upgrade() else { return };
        let data = window.global::<SettingsData>();
        let name = data.get_new_ignored_game().trim().to_string();
        if name.is_empty() {
            return;
        }
        data.set_new_ignored_game(SharedString::default());
        change(&window, &r, |s| {
            if !s
                .ignored_games
                .iter()
                .any(|g| g.eq_ignore_ascii_case(&name))
            {
                s.ignored_games.push(name);
            }
        });
    });
    let (weak, r) = (window.as_weak(), refresh.clone());
    data.on_add_ignored_program(move || {
        let Some(window) = weak.upgrade() else { return };
        let data = window.global::<SettingsData>();
        let mut name = data.get_new_ignored_program().trim().to_string();
        if name.is_empty() {
            return;
        }
        if !name.to_ascii_lowercase().ends_with(".exe") {
            name.push_str(".exe");
        }
        data.set_new_ignored_program(SharedString::default());
        change(&window, &r, |s| {
            if !s
                .ignored_executables
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&name))
            {
                s.ignored_executables.push(name);
            }
        });
    });
    let (weak, r) = (window.as_weak(), refresh.clone());
    data.on_remove(move |list, index| {
        let Some(window) = weak.upgrade() else { return };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        // The ignored lists may be filtered: map the row back to the entry.
        let (games, programs) = APP.with_borrow(|app| {
            (
                app.settings.games_shown.get(index).copied(),
                app.settings.programs_shown.get(index).copied(),
            )
        });
        change(&window, &r, |s| {
            let remove = |items: &mut Vec<String>, i: Option<usize>| {
                if let Some(i) = i.filter(|&i| i < items.len()) {
                    items.remove(i);
                }
            };
            match list {
                0 if index < s.custom_games.len() => {
                    s.custom_games.remove(index);
                }
                1 => remove(&mut s.extra_game_folders, Some(index)),
                2 => remove(&mut s.ignored_games, games),
                3 => remove(&mut s.ignored_executables, programs),
                _ => {}
            }
        });
    });
    let weak = window.as_weak();
    data.on_search_changed(move || {
        if let Some(window) = weak.upgrade() {
            APP.with_borrow_mut(|app| settings::show_lists(&window, &mut app.settings));
        }
    });

    // Updates
    let weak = window.as_weak();
    data.on_check_now(move || {
        if let Some(window) = weak.upgrade() {
            update_request(&window, Request::CheckForUpdates);
        }
    });
    let weak = window.as_weak();
    data.on_install_now(move || {
        if let Some(window) = weak.upgrade() {
            update_request(&window, Request::InstallUpdate);
        }
    });

    // Your Data
    let weak = window.as_weak();
    data.on_rescan(move || {
        let weak = weak.clone();
        request_in_background(Request::RescanGames, move |result| match result {
            Ok(_) => open_dialog(&weak, |_| {
                Some(dialogs::Dialog::Info(
                    "Games rescanned".into(),
                    "Newly installed games will be recognised from now on.".into(),
                ))
            }),
            Err(e) => error(&weak, e),
        });
    });
    let weak = window.as_weak();
    data.on_export_sessions(move || {
        let weak = weak.clone();
        request_in_background(Request::ExportCsv, move |result| {
            let csv = match result {
                Ok(Response::Csv { text }) => text,
                Ok(_) => return,
                Err(e) => return error(&weak, e),
            };
            #[cfg(windows)]
            if let Some(window) = weak.upgrade() {
                let owner = crate::owner_of(&window);
                let _ = std::thread::Builder::new()
                    .name("save".into())
                    .spawn(move || {
                        use playtime_windows::file_dialogs::{save, Filter};
                        let name = format!(
                            "Playtime sessions {}",
                            chrono::Local::now().format("%Y-%m-%d")
                        );
                        let filters = [Filter {
                            name: "CSV spreadsheet",
                            patterns: "*.csv",
                        }];
                        if let Some(path) = save(owner, "Export sessions", &name, "csv", &filters) {
                            // The tracker's CSV already starts with a byte-order mark for Excel.
                            let written = std::fs::write(&path, csv.as_bytes());
                            if let Err(e) = written {
                                let message = format!("The file couldn't be saved: {e}.");
                                let _ = slint::invoke_from_event_loop(move || {
                                    open_dialog(&weak, |_| Some(dialogs::Dialog::Error(message)));
                                });
                            }
                        }
                    });
            }
            #[cfg(not(windows))]
            let _ = csv;
        });
    });
    data.on_open_report(|| {
        if let Some(folder) = data_folder() {
            open::file(&folder.join("Game Stats.txt"));
        }
    });
    data.on_open_folder(|| {
        if let Some(folder) = data_folder() {
            open::folder(&folder);
        }
    });
    data.on_reopen(crate::reopen);
}

//! The Settings page: filling it from the tracker's settings, reading changes back, the searchable ignored lists,
//! and applying the appearance settings (theme and accent) to the whole window.

use crate::accent;
use crate::format;
use crate::ui::{AppWindow, ListEntry, Palette, SettingsData, Theme};
use playtime_core::settings::{Settings, ThemeMode};
use slint::{Color, ComponentHandle, ModelRc, SharedString, VecModel};

/// What the page knows beyond the settings themselves.
#[derive(Default)]
pub struct SettingsState {
    pub settings: Option<Settings>,
    /// The positions in the settings lists of the ignored entries shown (after the search).
    pub games_shown: Vec<usize>,
    pub programs_shown: Vec<usize>,
    /// Whether this window draws with the GPU, to say when the switch needs a reopen.
    pub drawn_with_gpu: bool,
}

/// The update status, as the tracker reports it.
pub struct UpdateStatus {
    pub current_version: String,
    pub available_version: Option<String>,
    pub can_install: bool,
    pub busy: bool,
    pub last_error: Option<String>,
    pub last_checked: Option<String>,
}

fn s(text: impl Into<SharedString>) -> SharedString {
    text.into()
}

fn entries(items: impl Iterator<Item = (String, String)>) -> ModelRc<ListEntry> {
    ModelRc::new(VecModel::from(
        items
            .map(|(text, detail)| ListEntry {
                text: s(text),
                detail: s(detail),
            })
            .collect::<Vec<_>>(),
    ))
}

/// The positions of the entries matching `query` (case-insensitive, anywhere in the name).
pub fn matching(items: &[String], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| query.is_empty() || item.to_lowercase().contains(&query))
        .map(|(i, _)| i)
        .collect()
}

/// "41", or "3 of 41" while searching.
pub fn count(shown: usize, total: usize, query: &str) -> String {
    if query.trim().is_empty() {
        if total == 1 {
            "1 entry".into()
        } else {
            format!("{total} entries")
        }
    } else {
        format!("{shown} of {total}")
    }
}

/// Shows the four lists (the ignored ones filtered by their search boxes).
pub fn show_lists(window: &AppWindow, state: &mut SettingsState) {
    let Some(settings) = state.settings.as_ref() else {
        return;
    };
    let data = window.global::<SettingsData>();
    data.set_custom_games(entries(
        settings
            .custom_games
            .iter()
            .map(|g| (g.name.clone(), g.executable.clone())),
    ));
    data.set_folders(entries(
        settings
            .extra_game_folders
            .iter()
            .map(|f| (f.clone(), String::new())),
    ));
    let games_query = data.get_ignored_games_query().to_string();
    let programs_query = data.get_ignored_programs_query().to_string();
    state.games_shown = matching(&settings.ignored_games, &games_query);
    state.programs_shown = matching(&settings.ignored_executables, &programs_query);
    data.set_ignored_games(entries(
        state
            .games_shown
            .iter()
            .map(|&i| (settings.ignored_games[i].clone(), String::new())),
    ));
    data.set_ignored_programs(entries(
        state
            .programs_shown
            .iter()
            .map(|&i| (settings.ignored_executables[i].clone(), String::new())),
    ));
    data.set_ignored_games_count(s(count(
        state.games_shown.len(),
        settings.ignored_games.len(),
        &games_query,
    )));
    data.set_ignored_programs_count(s(count(
        state.programs_shown.len(),
        settings.ignored_executables.len(),
        &programs_query,
    )));
}

/// Fills the page from the tracker's answer.
pub fn fill(
    window: &AppWindow,
    state: &mut SettingsState,
    settings: Settings,
    has_key: bool,
    start_with_windows: bool,
    installed: bool,
) {
    let data = window.global::<SettingsData>();
    data.set_theme(match settings.theme_mode {
        ThemeMode::System => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
    });
    data.set_accent_names(ModelRc::new(VecModel::from(
        accent::choice_names()
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    )));
    data.set_accent(accent::choice_index(&settings.accent_color) as i32);
    data.set_custom_accent(s(accent::resolve(&settings.accent_color)
        .map(accent::to_hex)
        .unwrap_or_default()));
    data.set_custom_accent_valid(true);
    data.set_glass(settings.glass_effects);
    data.set_hardware_acceleration(settings.hardware_acceleration);
    data.set_reopen_needed(settings.hardware_acceleration != state.drawn_with_gpu);
    data.set_start_with_windows(start_with_windows);
    data.set_installed(installed);
    data.set_notifications(settings.show_notifications);
    data.set_windows_game_list(settings.use_windows_game_list);
    data.set_poll(settings.poll_interval_seconds);
    data.set_minimum(settings.minimum_session_seconds);
    data.set_grace(settings.grace_period_seconds);
    data.set_online_artwork(settings.online_artwork);
    data.set_has_key(has_key);
    data.set_check_updates(settings.check_for_updates);
    data.set_auto_updates(settings.install_updates_automatically);
    apply_appearance(window, &settings);
    state.settings = Some(settings);
    show_lists(window, state);
    data.set_loaded(true);
    data.set_problem(SharedString::default());
}

/// Copies the page's switches, lists and numbers into the settings. Returns the new settings, if loaded.
pub fn read(window: &AppWindow, state: &mut SettingsState) -> Option<Settings> {
    let data = window.global::<SettingsData>();
    let settings = state.settings.as_mut()?;
    settings.theme_mode = match data.get_theme() {
        1 => ThemeMode::Light,
        2 => ThemeMode::Dark,
        _ => ThemeMode::System,
    };
    settings.glass_effects = data.get_glass();
    settings.hardware_acceleration = data.get_hardware_acceleration();
    settings.show_notifications = data.get_notifications();
    settings.use_windows_game_list = data.get_windows_game_list();
    settings.poll_interval_seconds = data.get_poll().clamp(1, 300);
    settings.minimum_session_seconds = data.get_minimum().clamp(0, 3600);
    settings.grace_period_seconds = data.get_grace().clamp(0, 600);
    settings.online_artwork = data.get_online_artwork();
    settings.check_for_updates = data.get_check_updates();
    settings.install_updates_automatically = data.get_auto_updates();
    data.set_reopen_needed(settings.hardware_acceleration != state.drawn_with_gpu);
    Some(settings.clone())
}

fn color((r, g, b): accent::Rgb) -> Color {
    Color::from_rgb_u8(r, g, b)
}

/// Light or dark, and the accent colour, for every page.
pub fn apply_appearance(window: &AppWindow, settings: &Settings) {
    use slint::language::ColorScheme;
    window
        .global::<Palette>()
        .set_color_scheme(match settings.theme_mode {
            ThemeMode::Light => ColorScheme::Light,
            ThemeMode::Dark => ColorScheme::Dark,
            ThemeMode::System => ColorScheme::Unknown,
        });
    let base = accent::resolve(&settings.accent_color)
        .or_else(accent::windows_accent)
        .unwrap_or(accent::PRESETS[0].2);
    let (on_light, on_dark) = accent::shades(base);
    let theme = window.global::<Theme>();
    theme.set_accent_on_light(color(on_light));
    theme.set_accent_on_dark(color(on_dark));
    let glass = crate::glass::apply(
        crate::owner_of(window),
        settings.glass_effects,
        theme.get_dark(),
    );
    theme.set_glass(glass);
}

/// "You're up to date (3.0.0). Last checked today at 9:00 AM."
pub fn update_text(status: &UpdateStatus) -> String {
    let mut lines = Vec::new();
    if status.busy {
        lines.push("Working…".to_string());
    } else if let Some(version) = &status.available_version {
        lines.push(format!(
            "Version {version} is available (you have {}).",
            status.current_version
        ));
    } else if status.last_checked.is_some() {
        lines.push(format!("You're up to date ({}).", status.current_version));
    } else {
        lines.push(format!("Version {}.", status.current_version));
    }
    if let Some(checked) = status
        .last_checked
        .as_deref()
        .and_then(|t| playtime_core::Timestamp::parse(t).ok())
    {
        lines.push(format!(
            "Last checked {} at {}.",
            when_checked(format::day_of(checked, playtime_core::Timestamp::now())),
            format::time(checked)
        ));
    }
    if let Some(error) = &status.last_error {
        lines.push(error.clone());
    }
    lines.join(" ")
}

/// "today", "yesterday", or "on Sun, Sep 27", to read mid-sentence.
fn when_checked(day: String) -> String {
    if day == "Today" || day == "Yesterday" {
        day.to_lowercase()
    } else {
        format!("on {day}")
    }
}

pub fn show_update_status(window: &AppWindow, status: &UpdateStatus) {
    let data = window.global::<SettingsData>();
    data.set_update_status(s(update_text(status)));
    data.set_can_install(status.can_install);
    data.set_update_busy(status.busy);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_lists_filter_as_you_type() {
        let items: Vec<String> = ["steamwebhelper.exe", "EpicGamesLauncher.exe", "SteamVR"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(matching(&items, "STEAM"), [0, 2]);
        assert_eq!(matching(&items, ""), [0, 1, 2]);
        assert!(matching(&items, "origin").is_empty());
        assert_eq!(count(2, 3, "steam"), "2 of 3");
        assert_eq!(count(3, 3, ""), "3 entries");
        assert_eq!(count(1, 1, " "), "1 entry");
    }

    #[test]
    fn update_status_reads_well() {
        let status = UpdateStatus {
            current_version: "3.0.0".into(),
            available_version: Some("3.0.1".into()),
            can_install: true,
            busy: false,
            last_error: None,
            last_checked: None,
        };
        assert_eq!(
            update_text(&status),
            "Version 3.0.1 is available (you have 3.0.0)."
        );
    }
}

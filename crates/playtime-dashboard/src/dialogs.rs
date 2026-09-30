//! What the dialogs show and what their buttons do: session details, a day's sessions, game details, and the
//! confirmations before anything is deleted. `main.rs` carries out the resulting [`Outcome`].

use crate::format;
use crate::overview::session_row;
use crate::ui::{AppWindow, DetailRow};
use chrono::{Local, NaiveDate};
use playtime_core::dashboard::{split_by_day, SessionView};
use playtime_core::ipc::{DashboardSnapshot, Request};
use playtime_core::{paths, Timestamp};
use slint::{ModelRc, SharedString, VecModel};

/// Something that needs asking twice.
#[derive(Clone, Debug)]
pub enum Confirm {
    DeleteSession(SessionView),
    DeleteHistory(String),
    StopTracking(String),
}

/// The open dialog.
#[derive(Clone, Debug)]
pub enum Dialog {
    Session(SessionView),
    Day(NaiveDate),
    Game(String),
    Confirm(Confirm),
    /// Something the tracker refused, with its message.
    Error(String),
    /// Waiting on something slow (title, message); Cancel only closes the dialog.
    Busy(String, String),
    /// Pictures from SteamGridDB to choose a game's cover from (the pictures themselves are set by `main.rs`).
    Choices(String),
}

/// What a button asks for.
pub enum Outcome {
    Close,
    Open(Dialog),
    /// Send this to the tracker, then close and reload.
    Run(Request),
    /// Open the program's folder with it selected.
    ShowProgram(String),
    /// Show only this game on the Overview.
    ShowOnOverview(String),
}

fn s(text: impl Into<SharedString>) -> SharedString {
    text.into()
}

fn row(label: &str, value: String) -> DetailRow {
    DetailRow {
        label: s(label),
        value: s(value),
    }
}

fn when(ts: Timestamp, now: Timestamp) -> String {
    format!(
        "{} at {}",
        format::day_of(ts, now),
        format::time_with_seconds(ts)
    )
}

/// Seconds played on a local day, across all games.
fn day_total(snapshot: &DashboardSnapshot, day: NaiveDate) -> i64 {
    snapshot
        .model
        .sessions
        .iter()
        .flat_map(|s| split_by_day(s.start, s.end, &Local))
        .filter(|(d, _)| *d == day)
        .map(|(_, t)| t.num_seconds())
        .sum()
}

/// The latest figures for a session (a running one moves on every second); `None` if it was deleted.
fn current(snapshot: &DashboardSnapshot, session: &SessionView) -> Option<SessionView> {
    snapshot
        .model
        .sessions
        .iter()
        .find(|s| s.start == session.start && paths::key(&s.game) == paths::key(&session.game))
        .cloned()
}

struct Content {
    title: String,
    message: String,
    rows: Vec<DetailRow>,
    list: Vec<SessionView>,
    link: String,
    primary: String,
    secondary: String,
    close: String,
}

impl Default for Content {
    fn default() -> Self {
        Self {
            title: String::new(),
            message: String::new(),
            rows: Vec::new(),
            list: Vec::new(),
            link: String::new(),
            primary: String::new(),
            secondary: String::new(),
            close: "Close".into(),
        }
    }
}

fn content(dialog: &Dialog, snapshot: &DashboardSnapshot) -> Content {
    let model = &snapshot.model;
    let now = model.now;
    match dialog {
        Dialog::Session(session) => {
            let session = current(snapshot, session).unwrap_or_else(|| session.clone());
            let mut of_game: Vec<&SessionView> = model.sessions_for(Some(&session.game)).collect();
            of_game.sort_by_key(|s| s.start);
            let number = of_game.iter().position(|s| s.start == session.start).map(|i| i + 1);
            let game_total: i64 = of_game.iter().map(|s| s.seconds).sum();
            let day = format::local_date(session.start);
            let mut rows = vec![
                row("Opened", when(session.start, now)),
                row(
                    "Closed",
                    if session.is_live {
                        "Still running".into()
                    } else {
                        when(session.end, now)
                    },
                ),
                row(
                    if session.is_live { "Running for" } else { "Played for" },
                    if session.is_live {
                        format::live_duration(session.seconds)
                    } else {
                        format::duration(session.seconds)
                    },
                ),
                row(
                    "Session",
                    number.map_or("–".into(), |n| format!("#{n} of {}", of_game.len())),
                ),
                row(&format!("{} total", session.game), format::duration(game_total)),
                row(
                    &format!("{} total", format::day(day, format::local_date(now))),
                    format::duration(day_total(snapshot, day)),
                ),
            ];
            let program = session.executable.clone().unwrap_or_default();
            if !program.is_empty() {
                rows.push(row("Program", program.clone()));
            }
            Content {
                title: session.game.clone(),
                rows,
                primary: if program_exists(&program) { "Show program".into() } else { String::new() },
                secondary: if session.is_live { String::new() } else { "Delete session".into() },
                ..Content::default()
            }
        }
        Dialog::Day(day) => {
            let list: Vec<SessionView> = model.sessions_on(*day, None, &Local).cloned().collect();
            let today = format::local_date(now);
            Content {
                title: format::day(*day, today),
                message: if list.is_empty() {
                    "No play this day.".into()
                } else {
                    format!(
                        "{} in {}",
                        format::duration(day_total(snapshot, *day)),
                        if list.len() == 1 { "1 session".into() } else { format!("{} sessions", list.len()) }
                    )
                },
                list,
                ..Content::default()
            }
        }
        Dialog::Game(name) => {
            let key = paths::key(name);
            let Some(game) = model.games.iter().find(|g| paths::key(&g.name) == key) else {
                return Content {
                    title: name.clone(),
                    message: "This game has no history any more.".into(),
                    ..Content::default()
                };
            };
            let mut rows = vec![
                row("Total", format::duration(game.total_seconds)),
                row("Sessions", game.session_count.to_string()),
                row("Average", format::duration(game.average_seconds)),
                row("Longest", format::duration(game.longest_seconds)),
                row("First played", format::day_of(game.first_played, now)),
                row(
                    "Last played",
                    if game.is_live { "Playing now".into() } else { format::day_of(game.last_played, now) },
                ),
            ];
            if let Some(identity) = snapshot.identities.iter().find(|i| paths::key(&i.game) == key) {
                rows.push(row("Found through", identity.source.clone()));
            }
            Content {
                title: game.name.clone(),
                rows,
                link: "Show on Overview".into(),
                primary: "Stop tracking".into(),
                secondary: "Delete history".into(),
                ..Content::default()
            }
        }
        Dialog::Busy(title, message) => Content {
            title: title.clone(),
            message: message.clone(),
            close: "Cancel".into(),
            ..Content::default()
        },
        Dialog::Choices(game) => Content {
            title: format!("Choose artwork for {game}"),
            message: "Pictures from SteamGridDB. Choosing one downloads it and makes it the game's cover.".into(),
            close: "Cancel".into(),
            ..Content::default()
        },
        Dialog::Error(message) => Content {
            title: "That didn't work".into(),
            message: message.clone(),
            close: "OK".into(),
            ..Content::default()
        },
        Dialog::Confirm(confirm) => match confirm {
            Confirm::DeleteSession(session) => Content {
                title: "Delete this session?".into(),
                message: format!(
                    "The {} session of {} on {} will be removed from your history. This can't be undone.",
                    format::duration(session.seconds),
                    session.game,
                    format::day_of(session.start, now)
                ),
                primary: "Delete".into(),
                close: "Cancel".into(),
                ..Content::default()
            },
            Confirm::DeleteHistory(game) => {
                let found = model.games.iter().find(|g| paths::key(&g.name) == paths::key(game));
                Content {
                    title: format!("Delete all history of {game}?"),
                    message: format!(
                        "{} sessions ({}) will be removed. This can't be undone.",
                        found.map_or(0, |g| g.session_count),
                        format::duration(found.map_or(0, |g| g.total_seconds))
                    ),
                    primary: "Delete history".into(),
                    close: "Cancel".into(),
                    ..Content::default()
                }
            }
            Confirm::StopTracking(game) => Content {
                title: format!("Stop tracking {game}?"),
                message: "It will be added to Settings → Ignored games, where you can track it again. Its history is kept.".into(),
                primary: "Stop tracking".into(),
                close: "Cancel".into(),
                ..Content::default()
            },
        },
    }
}

fn program_exists(path: &str) -> bool {
    !path.is_empty() && std::path::Path::new(path).is_file()
}

/// Shows `dialog` (or refreshes it with newer figures). Returns the sessions its list stands for.
pub fn show(window: &AppWindow, dialog: &Dialog, snapshot: &DashboardSnapshot) -> Vec<SessionView> {
    let c = content(dialog, snapshot);
    let now = snapshot.model.now;
    window.set_dialog_title(s(c.title));
    window.set_dialog_message(s(c.message));
    window.set_dialog_rows(ModelRc::new(VecModel::from(c.rows)));
    window.set_dialog_list(ModelRc::new(VecModel::from(
        c.list
            .iter()
            .map(|x| session_row(x, now))
            .collect::<Vec<_>>(),
    )));
    window.set_dialog_link(s(c.link));
    window.set_dialog_primary(s(c.primary));
    window.set_dialog_secondary(s(c.secondary));
    window.set_dialog_close(s(c.close));
    window.set_dialog_open(true);
    c.list
}

pub fn close(window: &AppWindow) {
    window.set_dialog_open(false);
    window.set_dialog_pictures(ModelRc::default());
    window.set_dialog_picture_names(ModelRc::default());
}

/// What pressing a button (0 primary, 1 secondary, 2 close, 3 link) in `dialog` asks for.
pub fn button(dialog: &Dialog, which: i32) -> Outcome {
    match (dialog, which) {
        (_, 2) => Outcome::Close,
        (Dialog::Session(session), 0) => {
            Outcome::ShowProgram(session.executable.clone().unwrap_or_default())
        }
        (Dialog::Session(session), 1) => {
            Outcome::Open(Dialog::Confirm(Confirm::DeleteSession(session.clone())))
        }
        (Dialog::Game(game), 0) => {
            Outcome::Open(Dialog::Confirm(Confirm::StopTracking(game.clone())))
        }
        (Dialog::Game(game), 1) => {
            Outcome::Open(Dialog::Confirm(Confirm::DeleteHistory(game.clone())))
        }
        (Dialog::Game(game), 3) => Outcome::ShowOnOverview(game.clone()),
        (Dialog::Confirm(Confirm::DeleteSession(session)), 0) => {
            Outcome::Run(Request::DeleteSession {
                game: session.game.clone(),
                start: session.start,
            })
        }
        (Dialog::Confirm(Confirm::DeleteHistory(game)), 0) => {
            Outcome::Run(Request::DeleteGameHistory { game: game.clone() })
        }
        (Dialog::Confirm(Confirm::StopTracking(game)), 0) => {
            Outcome::Run(Request::SetGameIgnored {
                game: game.clone(),
                ignored: true,
            })
        }
        _ => Outcome::Close,
    }
}

/// Opens the program's folder in Explorer with the program selected.
pub fn show_program(path: &str) {
    // Explorer wants `/select,"path"` as one argument; Windows paths can't contain quotes, so a recorded path can't
    // break out of it.
    let usable = !path.is_empty() && !path.contains('"') && program_exists(path);
    #[cfg(windows)]
    if usable {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{path}\""))
            .spawn();
    }
    #[cfg(not(windows))]
    let _ = usable;
}

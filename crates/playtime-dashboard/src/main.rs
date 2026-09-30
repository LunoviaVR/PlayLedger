//! Playtime Tracker's dashboard window. It holds no data of its own: everything comes from `playtime-tracker.exe`
//! over its named pipe (see `client.rs`), shown with Slint in the Windows 11 Fluent style.

// No console window behind the dashboard on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

/// The UI compiled from `ui/*.slint` by `build.rs`. Slint's generated code uses `unwrap`, which the workspace lints
/// otherwise forbid; the allowance is limited to this module.
mod ui {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    slint::include_modules!();
}

mod client;
mod dialogs;
mod format;
mod history;
mod offscreen;
mod overview;
mod statistics;

use client::{ClientError, Tracker};
use overview::OverviewState;
use playtime_core::ipc::{DashboardSnapshot, Event, Request, Response};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use ui::AppWindow;

/// The pages, in navigation order.
const PAGES: [&str; 5] = ["overview", "games", "history", "statistics", "settings"];

/// Everything the UI thread keeps between refreshes.
#[derive(Default)]
struct App {
    snapshot: Option<DashboardSnapshot>,
    overview: OverviewState,
    history: history::HistoryState,
    /// The open dialog, and the sessions its list stands for.
    dialog: Option<dialogs::Dialog>,
    dialog_list: Vec<playtime_core::dashboard::SessionView>,
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::default());
}

/// What a refresh is for: a full reload, or only the times of running games (once a second while one runs).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Refresh {
    Full,
    Times,
}

/// Shows the snapshot on the current page (UI thread).
fn show(window: &AppWindow, snapshot: DashboardSnapshot, kind: Refresh) {
    APP.with_borrow_mut(|app| {
        let only_times = kind == Refresh::Times
            && app.snapshot.as_ref().is_some_and(|old| {
                old.revision == snapshot.revision
                    && old.model.live.len() == snapshot.model.live.len()
            });
        match window.get_page() {
            0 if only_times => overview::render_times(window, &snapshot, &mut app.overview),
            0 => overview::render(window, &snapshot, &mut app.overview),
            2 if !only_times => history::render(window, &snapshot, &mut app.history),
            3 if !only_times => statistics::render(window, &snapshot),
            _ => {}
        }
        // An open dialog shows the newest figures (a running session keeps counting up).
        if let Some(dialog) = &app.dialog {
            app.dialog_list = dialogs::show(window, dialog, &snapshot);
        }
        app.snapshot = Some(snapshot);
    });
}

/// Re-renders the current page from the last snapshot (after navigating or changing a filter).
fn rerender(window: &AppWindow) {
    APP.with_borrow_mut(|app| {
        let App {
            snapshot,
            overview,
            history,
            ..
        } = app;
        if let Some(snapshot) = snapshot.as_ref() {
            match window.get_page() {
                0 => overview::render(window, snapshot, overview),
                2 => history::render(window, snapshot, history),
                3 => statistics::render(window, snapshot),
                _ => {}
            }
        }
    });
}

/// Opens the dialog `pick` chooses (if any) with the latest snapshot.
fn open_dialog(
    window: &slint::Weak<AppWindow>,
    pick: impl FnOnce(&App) -> Option<dialogs::Dialog>,
) {
    let Some(window) = window.upgrade() else {
        return;
    };
    APP.with_borrow_mut(|app| {
        let (Some(dialog), Some(snapshot)) = (pick(app), app.snapshot.as_ref()) else {
            return;
        };
        app.dialog_list = dialogs::show(&window, &dialog, snapshot);
        app.dialog = Some(dialog);
    });
}

fn close_dialog(window: &AppWindow) {
    dialogs::close(window);
    APP.with_borrow_mut(|app| {
        app.dialog = None;
        app.dialog_list.clear();
    });
}

/// The tracker next to the dashboard (installed: `Dashboard\` sits beside `playtime-tracker.exe`).
fn tracker_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [
        dir.join("..").join("playtime-tracker.exe"),
        dir.join("playtime-tracker.exe"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// Starts a worker that fetches snapshots off the UI thread; returns the handle to ask it for one.
fn start_fetcher(tracker: Arc<Tracker>, window: slint::Weak<AppWindow>) -> mpsc::Sender<Refresh> {
    let (sender, requests) = mpsc::channel::<Refresh>();
    let _ = std::thread::Builder::new()
        .name("snapshots".into())
        .spawn(move || {
            while let Ok(mut kind) = requests.recv() {
                // Coalesce: queued requests become one fetch (a full one if any asked for it).
                while let Ok(next) = requests.try_recv() {
                    if next == Refresh::Full {
                        kind = Refresh::Full;
                    }
                }
                let result = tracker.request(&Request::GetDashboard);
                let window = window.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(window) = window.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(Response::Dashboard { snapshot }) => {
                            window.set_connection_problem(Default::default());
                            show(&window, *snapshot, kind);
                        }
                        Ok(_) => {}
                        Err(ClientError::Unavailable(message)) => {
                            window.set_connection_problem(message.into());
                        }
                        Err(ClientError::Tracker(message)) => eprintln!("{message}"),
                    }
                });
            }
        });
    sender
}

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--render") {
        let result = offscreen::RenderArgs::parse(&args[i + 1..], &PAGES).and_then(|render| {
            let dialog = render.dialog.clone();
            offscreen::render(&render, move |window, snapshot| {
                let model = &snapshot.model;
                let pick = match dialog.as_deref() {
                    Some("session") => model
                        .sessions
                        .first()
                        .cloned()
                        .map(dialogs::Dialog::Session),
                    Some("day") => Some(dialogs::Dialog::Day(format::local_date(model.now))),
                    Some("game") => model
                        .games
                        .first()
                        .map(|g| dialogs::Dialog::Game(g.name.clone())),
                    _ => None,
                };
                show(window, snapshot, Refresh::Full);
                open_dialog(&window.as_weak(), |_| pick);
            })
        });
        if let Err(message) = result {
            eprintln!("{message}");
            std::process::exit(2);
        }
        return Ok(());
    }
    let window = AppWindow::new()?;
    if let Some(page) = args
        .iter()
        .position(|a| a == "--page")
        .and_then(|i| args.get(i + 1))
        .and_then(|name| PAGES.iter().position(|p| p == name))
    {
        window.set_page(page as i32);
    }

    let tracker = Arc::new(Tracker::default());
    let refresh = start_fetcher(tracker.clone(), window.as_weak());
    let _ = refresh.send(Refresh::Full);

    // Events: anything that changed the data reloads it.
    {
        let events = refresh.clone();
        let connection = refresh.clone();
        client::watch_events(
            move |event| {
                if matches!(
                    event,
                    Event::SessionStarted { .. }
                        | Event::SessionEnded { .. }
                        | Event::DataChanged { .. }
                ) {
                    let _ = events.send(Refresh::Full);
                }
            },
            move |_connected| {
                let _ = connection.send(Refresh::Full);
            },
        );
    }

    // As in the original dashboard, running games count up every second while the window is open.
    let clock = slint::Timer::default();
    {
        let refresh = refresh.clone();
        clock.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(1),
            move || {
                let playing = APP.with_borrow(|app| {
                    app.snapshot
                        .as_ref()
                        .is_some_and(|s| !s.model.live.is_empty())
                });
                if playing {
                    let _ = refresh.send(Refresh::Times);
                }
            },
        );
    }

    let weak = window.as_weak();
    window.on_navigate(move |page| {
        if let Some(window) = weak.upgrade() {
            window.set_page(page);
            rerender(&window);
        }
    });

    let weak = window.as_weak();
    window.on_overview_game(move |i| {
        if let Some(window) = weak.upgrade() {
            APP.with_borrow_mut(|app| {
                app.overview.game = app.overview.games.get(i as usize).cloned();
                app.overview.shown = overview::PAGE_SIZE;
            });
            rerender(&window);
        }
    });
    let weak = window.as_weak();
    window.on_overview_all_games(move || {
        if let Some(window) = weak.upgrade() {
            APP.with_borrow_mut(|app| {
                app.overview.game = None;
                app.overview.shown = overview::PAGE_SIZE;
            });
            rerender(&window);
        }
    });
    let weak = window.as_weak();
    window.on_overview_show_more(move || {
        if let Some(window) = weak.upgrade() {
            APP.with_borrow_mut(|app| app.overview.shown += overview::PAGE_SIZE);
            rerender(&window);
        }
    });

    let weak = window.as_weak();
    window.on_history_toggle(move |d| {
        if let Some(window) = weak.upgrade() {
            APP.with_borrow_mut(|app| {
                if let Some(day) = app.history.days.get(d as usize).copied() {
                    if !app.history.expanded.remove(&day) {
                        app.history.expanded.insert(day);
                    }
                }
            });
            rerender(&window);
        }
    });

    // Dialogs: opened from the pages, their buttons carried out here.
    let weak = window.as_weak();
    window.on_overview_session(move |i| {
        open_dialog(&weak, |app| {
            app.overview
                .sessions
                .get(i as usize)
                .cloned()
                .map(dialogs::Dialog::Session)
        });
    });
    let weak = window.as_weak();
    window.on_overview_live_session(move |i| {
        open_dialog(&weak, |app| {
            app.overview
                .live
                .get(i as usize)
                .cloned()
                .map(dialogs::Dialog::Session)
        });
    });
    let weak = window.as_weak();
    window.on_overview_day(move |i| {
        open_dialog(&weak, |app| {
            app.overview
                .days
                .get(i as usize)
                .copied()
                .map(dialogs::Dialog::Day)
        });
    });
    let weak = window.as_weak();
    window.on_history_session(move |d, i| {
        open_dialog(&weak, |app| {
            app.history
                .sessions
                .get(d as usize)
                .and_then(|list| list.get(i as usize))
                .cloned()
                .map(dialogs::Dialog::Session)
        });
    });
    let weak = window.as_weak();
    window.on_dialog_list_clicked(move |i| {
        open_dialog(&weak, |app| {
            app.dialog_list
                .get(i as usize)
                .cloned()
                .map(dialogs::Dialog::Session)
        });
    });
    let weak = window.as_weak();
    let dialog_tracker = tracker.clone();
    let dialog_refresh = refresh.clone();
    window.on_dialog_button(move |which| {
        let Some(window) = weak.upgrade() else { return };
        let Some(dialog) = APP.with_borrow(|app| app.dialog.clone()) else {
            return;
        };
        match dialogs::button(&dialog, which) {
            dialogs::Outcome::Close => close_dialog(&window),
            dialogs::Outcome::Open(next) => open_dialog(&weak, |_| Some(next)),
            dialogs::Outcome::ShowProgram(path) => dialogs::show_program(&path),
            dialogs::Outcome::ShowOnOverview(game) => {
                close_dialog(&window);
                APP.with_borrow_mut(|app| {
                    app.overview.game = Some(game);
                    app.overview.shown = overview::PAGE_SIZE;
                });
                window.set_page(0);
                rerender(&window);
            }
            dialogs::Outcome::Run(request) => match dialog_tracker.request(&request) {
                Ok(_) => {
                    close_dialog(&window);
                    let _ = dialog_refresh.send(Refresh::Full);
                }
                Err(e) => open_dialog(&weak, |_| Some(dialogs::Dialog::Error(e.to_string()))),
            },
        }
    });

    let refresh_after_start = refresh.clone();
    window.on_start_tracker(move || {
        if let Some(path) = tracker_path() {
            let _ = std::process::Command::new(path).spawn();
            let _ = refresh_after_start.send(Refresh::Full);
        }
    });

    window.run()
}

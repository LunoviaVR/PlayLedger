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

mod accent;
mod client;
mod crash_log;
mod dialogs;
mod format;
mod games;
mod glass;
mod history;
mod instance;
mod offscreen;
mod open;
mod overview;
mod settings;
mod settings_actions;
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
    games: games::GamesState,
    history: history::HistoryState,
    /// Asks the artwork worker for a game's picture.
    art_loader: Option<mpsc::Sender<String>>,
    /// The SteamGridDB choices the open dialog shows, by the tracker's index.
    choices: Vec<usize>,
    /// The open dialog, and the sessions its list stands for.
    dialog: Option<dialogs::Dialog>,
    dialog_list: Vec<playtime_core::dashboard::SessionView>,
    settings: settings::SettingsState,
    /// Saves the Tracking numbers a moment after the last change.
    save_timer: slint::Timer,
    /// Keeps an update check's status current while it runs.
    update_timer: slint::Timer,
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
            1 if !only_times => {
                let wanted = games::render(window, &snapshot, &mut app.games);
                load_art(app, wanted);
            }
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
        let Some(snapshot) = app.snapshot.as_ref() else {
            return;
        };
        let mut wanted = Vec::new();
        match window.get_page() {
            0 => overview::render(window, snapshot, &mut app.overview),
            1 => wanted = games::render(window, snapshot, &mut app.games),
            2 => history::render(window, snapshot, &mut app.history),
            3 => statistics::render(window, snapshot),
            _ => {}
        }
        load_art(app, wanted);
    });
}

fn load_art(app: &App, games: Vec<String>) {
    if let Some(loader) = &app.art_loader {
        for game in games {
            let _ = loader.send(game);
        }
    }
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

/// Starts a worker that reads each game's picture (box art, else its icon) and hands it to the Games page.
fn start_art_loader(tracker: Arc<Tracker>, window: slint::Weak<AppWindow>) -> mpsc::Sender<String> {
    let (sender, games) = mpsc::channel::<String>();
    let _ = std::thread::Builder::new()
        .name("artwork".into())
        .spawn(move || {
            while let Ok(game) = games.recv() {
                let picture = |kind: &str, size| {
                    let request = Request::GetArtwork {
                        game: game.clone(),
                        kind: kind.into(),
                    };
                    match tracker.request(&request) {
                        Ok(Response::Artwork { path: Some(path) }) => {
                            games::decode(std::path::Path::new(&path), size)
                        }
                        _ => None,
                    }
                };
                // A missing cover may still arrive (an `artworkReady` event then asks again); the icon stands in.
                let found = picture("cover", games::COVER_SIZE)
                    .map(|p| (p, false))
                    .or_else(|| picture("icon", games::ICON_SIZE).map(|p| (p, true)));
                let window = window.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(window) = window.upgrade() else {
                        return;
                    };
                    let art = found.map(|(pixels, icon)| games::Art {
                        image: slint::Image::from_rgba8(pixels),
                        icon,
                    });
                    APP.with_borrow_mut(|app| {
                        games::art_arrived(&window, &mut app.games, &game, art)
                    });
                });
            }
        });
    sender
}

/// Sends `request` to the tracker off the UI thread, on a connection of its own so a slow one (downloading
/// artwork) doesn't hold up the page, then hands the answer to `done` on the UI thread.
fn request_in_background(
    request: Request,
    done: impl FnOnce(Result<Response, ClientError>) + Send + 'static,
) {
    let _ = std::thread::Builder::new()
        .name("request".into())
        .spawn(move || {
            let result = Tracker::default().request(&request);
            let _ = slint::invoke_from_event_loop(move || done(result));
        });
}

/// Sends `request` in the background, then closes the dialog and reloads, or shows what went wrong.
fn run_in_background(
    window: slint::Weak<AppWindow>,
    refresh: mpsc::Sender<Refresh>,
    request: Request,
) {
    request_in_background(request, move |result| {
        let Some(window) = window.upgrade() else {
            return;
        };
        match result {
            Ok(_) => {
                close_dialog(&window);
                let _ = refresh.send(Refresh::Full);
            }
            Err(e) => open_dialog(&window.as_weak(), |_| {
                Some(dialogs::Dialog::Error(e.to_string()))
            }),
        }
    });
}

/// Starts a new dashboard on the Settings page and closes this one ("Reopen now", after changing the renderer).
fn reopen() {
    if let Ok(exe) = std::env::current_exe() {
        instance::release();
        if std::process::Command::new(exe)
            .args(["--page", "settings"])
            .spawn()
            .is_ok()
        {
            let _ = slint::quit_event_loop();
        }
    }
}

/// Picks how the window draws: with the GPU (femtovg, OpenGL) when hardware acceleration is on, else on the CPU.
/// Returns whether the GPU is used.
fn select_renderer(args: &[String]) -> bool {
    let gpu = !args.iter().any(|a| a == "--software")
        && match Tracker::default().request(&Request::GetSettings) {
            Ok(Response::Settings { settings, .. }) => settings.hardware_acceleration,
            // No tracker (yet): the default.
            _ => true,
        };
    let select = |renderer: &str| {
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name(renderer.into())
            .select()
    };
    if gpu {
        // Skia draws text like Windows does (smooth, at subpixel positions); femtovg hints every glyph to the pixel
        // grid, which looks too sharp. Skia on OpenGL keeps the window's transparency for the glass look (its
        // Direct3D path would draw opaque). femtovg is the fallback if Skia can't start.
        for renderer in ["skia-opengl", "femtovg"] {
            match select(renderer) {
                Ok(()) => return true,
                Err(e) => crash_log::write(
                    "Starting the GPU renderer",
                    &format!("{renderer}: {e}; trying the next one"),
                ),
            }
        }
    }
    let _ = select("software");
    false
}

/// The window's handle, to make the Open dialog modal to it (0 if unknown).
fn owner_of(window: &AppWindow) -> isize {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window().window_handle().window_handle() {
            if let RawWindowHandle::Win32(h) = handle.as_raw() {
                return h.hwnd.get();
            }
        }
    }
    let _ = window;
    0
}

/// Carries out a Games tile's menu choice.
fn game_action(
    window: &AppWindow,
    refresh: &mpsc::Sender<Refresh>,
    game: String,
    action: ui::GameAction,
) {
    use ui::GameAction;
    let weak = window.as_weak();
    match action {
        GameAction::ChooseFile => {
            #[cfg(windows)]
            {
                let owner = owner_of(window);
                let refresh = refresh.clone();
                let _ = std::thread::Builder::new()
                    .name("open".into())
                    .spawn(move || {
                        use playtime_windows::file_dialogs::{open, Filter};
                        let filters = [Filter {
                            name: "Pictures (PNG, JPEG, WebP)",
                            patterns: "*.png;*.jpg;*.jpeg;*.webp",
                        }];
                        let Some(path) =
                            open(owner, &format!("Choose artwork for {game}"), &filters)
                        else {
                            return;
                        };
                        let request = Request::SetArtworkFromFile {
                            game,
                            kind: "cover".into(),
                            path: path.to_string_lossy().into_owned(),
                        };
                        let _ = slint::invoke_from_event_loop(move || {
                            run_in_background(weak, refresh, request);
                        });
                    });
            }
            #[cfg(not(windows))]
            let _ = (game, weak, owner_of(window));
        }
        GameAction::PickOnline => {
            let busy = dialogs::Dialog::Busy(
                format!("Choose artwork for {game}"),
                "Looking for pictures on SteamGridDB…".into(),
            );
            open_dialog(&weak, |_| Some(busy.clone()));
            let _ = std::thread::Builder::new()
                .name("choices".into())
                .spawn(move || {
                    let request = Request::ListArtworkChoices {
                        game: game.clone(),
                        kind: "cover".into(),
                    };
                    let result =
                        Tracker::default()
                            .request(&request)
                            .map(|response| match response {
                                Response::ArtworkChoices { choices } => choices
                                    .into_iter()
                                    .filter_map(|c| {
                                        games::decode(
                                            std::path::Path::new(&c.path),
                                            games::PREVIEW_SIZE,
                                        )
                                        .map(|pixels| {
                                            (c.index, format!("{} × {}", c.width, c.height), pixels)
                                        })
                                    })
                                    .collect::<Vec<_>>(),
                                _ => Vec::new(),
                            });
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(window) = weak.upgrade() else {
                            return;
                        };
                        // Cancelled meanwhile: leave whatever is open now alone.
                        let still_waiting = APP.with_borrow(|app| {
                        matches!(&app.dialog, Some(d) if matches!(d, dialogs::Dialog::Busy(..)))
                    });
                        if !still_waiting {
                            return;
                        }
                        match result {
                            Ok(choices) if choices.is_empty() => open_dialog(&weak, |_| {
                                Some(dialogs::Dialog::Error(format!(
                                    "SteamGridDB has no pictures for {game}."
                                )))
                            }),
                            Ok(choices) => {
                                open_dialog(&weak, |_| {
                                    Some(dialogs::Dialog::Choices(game.clone()))
                                });
                                let names: Vec<slint::SharedString> = choices
                                    .iter()
                                    .enumerate()
                                    .map(|(i, (_, size, _))| {
                                        format!("Picture {} of {}, {size}", i + 1, choices.len())
                                            .into()
                                    })
                                    .collect();
                                APP.with_borrow_mut(|app| {
                                    app.choices = choices.iter().map(|c| c.0).collect()
                                });
                                let pictures: Vec<slint::Image> = choices
                                    .into_iter()
                                    .map(|(_, _, pixels)| slint::Image::from_rgba8(pixels))
                                    .collect();
                                window.set_dialog_pictures(slint::ModelRc::new(
                                    slint::VecModel::from(pictures),
                                ));
                                window.set_dialog_picture_names(slint::ModelRc::new(
                                    slint::VecModel::from(names),
                                ));
                            }
                            Err(e) => {
                                open_dialog(&weak, |_| Some(dialogs::Dialog::Error(e.to_string())))
                            }
                        }
                    });
                });
        }
        GameAction::AutomaticArt => run_in_background(
            weak,
            refresh.clone(),
            Request::ResetArtwork {
                game,
                kind: "cover".into(),
            },
        ),
        GameAction::StopTracking => open_dialog(&weak, |_| {
            Some(dialogs::Dialog::Confirm(dialogs::Confirm::StopTracking(
                game,
            )))
        }),
        GameAction::DeleteHistory => open_dialog(&weak, |_| {
            Some(dialogs::Dialog::Confirm(dialogs::Confirm::DeleteHistory(
                game,
            )))
        }),
    }
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
                    Some("name") => Some(dialogs::Dialog::NameGame(
                        "C:\\Games\\Hades\\Hades.exe".into(),
                    )),
                    Some("game") => model
                        .games
                        .first()
                        .map(|g| dialogs::Dialog::Game(g.name.clone())),
                    Some("choices") => model
                        .games
                        .first()
                        .map(|g| dialogs::Dialog::Choices(g.name.clone())),
                    _ => None,
                };
                show(window, snapshot, Refresh::Full);
                let choices = matches!(pick, Some(dialogs::Dialog::Choices(_)));
                open_dialog(&window.as_weak(), |_| pick);
                if choices {
                    // Twelve plain covers in different colours stand in for SteamGridDB's pictures.
                    let pictures: Vec<slint::Image> = (0..12u8)
                        .map(|i| {
                            let colour = slint::Rgba8Pixel::new(40 + i * 17, 90, 220 - i * 13, 255);
                            let mut cover =
                                slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(100, 150);
                            cover.make_mut_slice().fill(colour);
                            slint::Image::from_rgba8(cover)
                        })
                        .collect();
                    let names: Vec<slint::SharedString> =
                        (1..=12).map(|i| format!("Picture {i}").into()).collect();
                    window
                        .set_dialog_pictures(slint::ModelRc::new(slint::VecModel::from(pictures)));
                    window.set_dialog_picture_names(slint::ModelRc::new(slint::VecModel::from(
                        names,
                    )));
                }
            })
        });
        if let Err(message) = result {
            eprintln!("{message}");
            std::process::exit(2);
        }
        return Ok(());
    }
    crash_log::install();
    let wants_settings = args
        .iter()
        .position(|a| a == "--page")
        .and_then(|i| args.get(i + 1))
        .is_some_and(|p| p == "settings");
    if !instance::claim(wants_settings) {
        return Ok(());
    }
    let gpu = select_renderer(&args);
    glass::GPU.store(gpu, std::sync::atomic::Ordering::Relaxed);
    let window = AppWindow::new()?;
    APP.with_borrow_mut(|app| app.settings.drawn_with_gpu = gpu);
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
    let art_loader = start_art_loader(tracker, window.as_weak());
    APP.with_borrow_mut(|app| app.art_loader = Some(art_loader.clone()));
    let _ = refresh.send(Refresh::Full);
    settings_actions::wire(&window, &refresh);

    // Another launch (the tray) brings this window forward, on Settings if asked.
    let wake_window = window.as_weak();
    instance::listen(move |wake| {
        let weak = wake_window.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(window) = weak.upgrade() else { return };
            if matches!(wake, instance::Wake::ShowSettings) {
                window.set_page(4);
                rerender(&window);
                settings_actions::load(weak);
            }
            let _ = window.show();
            instance::bring_to_front(owner_of(&window));
        });
    });
    settings_actions::load(window.as_weak());

    // Events: anything that changed the data reloads it.
    {
        let events = refresh.clone();
        let connection = refresh.clone();
        let settings_window = window.as_weak();
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
                // New or changed artwork: show it on the game's tile.
                if let Event::ArtworkReady { game, kind } = event {
                    if kind == "cover" || kind == "icon" {
                        let _ = art_loader.send(game);
                    }
                }
            },
            move |connected| {
                let _ = connection.send(Refresh::Full);
                // The tracker came back: its settings (theme, accent) may be new to this window.
                if connected {
                    settings_actions::load(settings_window.clone());
                }
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
            if page == 4 {
                settings_actions::load(weak.clone());
            }
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
            dialogs::Outcome::AddCustomGame(path) => {
                if settings_actions::add_custom_game(&window, &dialog_refresh, path) {
                    close_dialog(&window);
                }
            }
            dialogs::Outcome::Run(request) => {
                run_in_background(weak.clone(), dialog_refresh.clone(), request)
            }
        }
    });

    let weak = window.as_weak();
    let picture_refresh = refresh.clone();
    window.on_dialog_picture_clicked(move |i| {
        let Some(dialogs::Dialog::Choices(game)) = APP.with_borrow(|app| app.dialog.clone()) else {
            return;
        };
        let Some(index) = APP.with_borrow(|app| app.choices.get(i as usize).copied()) else {
            return;
        };
        let busy = dialogs::Dialog::Busy(
            format!("Choose artwork for {game}"),
            "Downloading the picture…".into(),
        );
        open_dialog(&weak, |_| Some(busy));
        run_in_background(
            weak.clone(),
            picture_refresh.clone(),
            Request::ApplyArtworkChoice {
                game,
                kind: "cover".into(),
                index,
            },
        );
    });

    // Games
    let weak = window.as_weak();
    window.on_games_search(move |text| {
        if let Some(window) = weak.upgrade() {
            APP.with_borrow_mut(|app| app.games.query = text.to_string());
            rerender(&window);
        }
    });
    let weak = window.as_weak();
    window.on_games_sort_changed(move |sort| {
        if let Some(window) = weak.upgrade() {
            APP.with_borrow_mut(|app| app.games.sort = sort);
            rerender(&window);
        }
    });
    let weak = window.as_weak();
    window.on_games_clicked(move |section, i| {
        open_dialog(&weak, |app| {
            games::game_at(&app.games, section, i).map(dialogs::Dialog::Game)
        });
    });
    let weak = window.as_weak();
    let action_refresh = refresh.clone();
    window.on_games_action(move |section, i, action| {
        let Some(window) = weak.upgrade() else { return };
        if let Some(game) = APP.with_borrow(|app| games::game_at(&app.games, section, i)) {
            game_action(&window, &action_refresh, game, action);
        }
    });

    let refresh_after_start = refresh;
    window.on_start_tracker(move || {
        if let Some(path) = tracker_path() {
            let _ = std::process::Command::new(path).spawn();
            let _ = refresh_after_start.send(Refresh::Full);
        }
    });

    // Shown first, so the window has a handle for the glass look; the settings then keep it up to date.
    let result = window.show().and_then(|()| {
        let theme = window.global::<ui::Theme>();
        theme.set_glass(glass::apply(owner_of(&window), true, theme.get_dark()));
        slint::run_event_loop()
    });
    // The GPU renderer can also fail once the window opens (a broken graphics driver): start again on the CPU.
    if result.is_err() && gpu && !args.iter().any(|a| a == "--software") {
        crash_log::write(
            "Drawing with the GPU",
            &format!("{result:?}; reopening with the software renderer"),
        );
        if let Ok(exe) = std::env::current_exe() {
            instance::release();
            let _ = std::process::Command::new(exe)
                .args(args.iter().skip(1))
                .arg("--software")
                .spawn();
            return Ok(());
        }
    }
    result
}

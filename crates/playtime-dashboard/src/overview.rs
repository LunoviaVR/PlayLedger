//! The Overview page's content, built from the tracker's snapshot: totals, what's playing, the 30-day chart, every
//! game and every session (fifty at a time). Choosing a game shows only that game across the page.

use crate::format;
use crate::ui::{AppWindow, DayBar, GameRow, SessionRow, Tile};
use chrono::{Datelike, Duration, Local, NaiveDate};
use playtime_core::dashboard::{GameView, SessionView};
use playtime_core::ipc::DashboardSnapshot;
use playtime_core::{paths, Timestamp};
use slint::{ModelRc, SharedString, VecModel};

/// Sessions shown at first, and added by "Show more".
pub const PAGE_SIZE: usize = 50;

/// What the page is showing, kept while switching pages.
pub struct OverviewState {
    /// The chosen game, or `None` for all games.
    pub game: Option<String>,
    pub shown: usize,
    /// What the page's rows stand for, so a click can be mapped back.
    pub live: Vec<SessionView>,
    pub sessions: Vec<SessionView>,
    pub games: Vec<String>,
    pub days: Vec<NaiveDate>,
}

impl Default for OverviewState {
    fn default() -> Self {
        Self {
            game: None,
            shown: PAGE_SIZE,
            live: Vec::new(),
            sessions: Vec::new(),
            games: Vec::new(),
            days: Vec::new(),
        }
    }
}

fn s(text: impl Into<SharedString>) -> SharedString {
    text.into()
}

fn rows<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

/// A session row; `with_game` names the game (always, since a single game's list reads the same way).
pub fn session_row(session: &SessionView, now: Timestamp) -> SessionRow {
    let day = format::day_of(session.start, now);
    let (range, duration, accessible) = if session.is_live {
        (
            format!("Since {}", format::time(session.start)),
            format!("{} so far", format::live_duration(session.seconds)),
            format!(
                "{}, playing now, {} so far",
                session.game,
                format::spoken_duration(session.seconds)
            ),
        )
    } else {
        let range = format::range(session.start, session.end, now);
        let accessible = format!(
            "{}, {day}, {range}, {}",
            session.game,
            format::spoken_duration(session.seconds)
        );
        (range, format::duration(session.seconds), accessible)
    };
    SessionRow {
        game: s(session.game.as_str()),
        day: s(day),
        range: s(range),
        duration: s(duration),
        live: session.is_live,
        accessible: s(accessible),
    }
}

fn game_row(game: &GameView, now: Timestamp) -> GameRow {
    let sessions = if game.session_count == 1 {
        "1 session".to_string()
    } else {
        format!("{} sessions", game.session_count)
    };
    let detail = if game.is_live {
        format!("{sessions} · playing now")
    } else {
        format!(
            "{sessions} · last played {}",
            format::day_of(game.last_played, now)
        )
    };
    GameRow {
        name: s(game.name.as_str()),
        total: s(format::duration(game.total_seconds)),
        live: game.is_live,
        accessible: s(format!(
            "{}, {}, {detail}",
            game.name,
            format::spoken_duration(game.total_seconds)
        )),
        detail: s(detail),
    }
}

fn tile(caption: &str, value: String, detail: String) -> Tile {
    Tile {
        caption: s(caption),
        value: s(value),
        detail: s(detail),
    }
}

fn chosen<'a>(snapshot: &'a DashboardSnapshot, game: Option<&str>) -> Option<&'a GameView> {
    let key = paths::key(game?);
    snapshot
        .model
        .games
        .iter()
        .find(|g| paths::key(&g.name) == key)
}

/// The four summary tiles (also refreshed every second while a game runs).
pub fn tiles(snapshot: &DashboardSnapshot, state: &OverviewState) -> Vec<Tile> {
    let model = &snapshot.model;
    let now = model.now;
    let game = chosen(snapshot, state.game.as_deref());
    let sessions: Vec<&SessionView> = model.sessions_for(game.map(|g| g.name.as_str())).collect();
    let total: i64 = sessions.iter().map(|s| s.seconds).sum();
    let count = sessions.len() as i64;
    let week_since = format::start_of_today(now)
        .checked_add(-Duration::days(6))
        .unwrap_or(now);
    let week = match game {
        None => snapshot.past_week_seconds,
        Some(g) => model.total_since(Some(&g.name), week_since).num_seconds(),
    };
    let mut tiles = vec![
        tile(
            "Total playtime",
            format::duration(total),
            match game {
                None if model.games.len() == 1 => "1 game".into(),
                None => format!("{} games", model.games.len()),
                Some(g) => format!("Since {}", format::day_of(g.first_played, now)),
            },
        ),
        tile(
            "Sessions",
            count.to_string(),
            if count > 0 {
                format!("{} on average", format::duration(total / count))
            } else {
                String::new()
            },
        ),
        tile(
            "Past 7 days",
            format::duration(week),
            "Today and the 6 days before".into(),
        ),
    ];
    tiles.push(match game {
        None => {
            let top = model.games.first();
            tile(
                "Most played",
                top.map_or("–".into(), |g| g.name.clone()),
                top.map_or(String::new(), |g| format::duration(g.total_seconds)),
            )
        }
        Some(g) => tile(
            "Longest session",
            format::duration(g.longest_seconds),
            if g.is_live {
                "Playing now".into()
            } else {
                format!("Last played {}", format::day_of(g.last_played, now))
            },
        ),
    });
    tiles
}

/// Fills the whole page from `snapshot`.
pub fn render(window: &AppWindow, snapshot: &DashboardSnapshot, state: &mut OverviewState) {
    let model = &snapshot.model;
    let now = model.now;
    let game = chosen(snapshot, state.game.as_deref());
    if state.game.is_some() && game.is_none() {
        state.game = None; // the game's history was deleted
    }
    let name = game.map(|g| g.name.clone());

    window.set_overview_title(s(name.clone().unwrap_or_else(|| "Overview".into())));
    window.set_overview_one_game(game.is_some());
    window.set_overview_tiles(model_of_tiles(snapshot, state));

    state.live = model
        .sessions_for(name.as_deref())
        .filter(|s| s.is_live)
        .cloned()
        .collect();
    window.set_overview_live(rows(
        state.live.iter().map(|s| session_row(s, now)).collect(),
    ));

    let daily = model.daily_totals(name.as_deref(), &Local);
    let busiest = daily.iter().map(|d| d.seconds).max().unwrap_or(0);
    let today = format::local_date(now);
    let last = daily.len().saturating_sub(1);
    state.days = daily.iter().map(|d| d.day).collect();
    let bars = daily
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let label = if d.day == today {
                "Today".to_string()
            } else {
                d.day.day().to_string()
            };
            DayBar {
                fraction: if busiest > 0 {
                    (d.seconds as f32 / busiest as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                // Today and every fifth day before it, so labels never crowd.
                show_label: (last - i) % 5 == 0,
                label: s(label),
                tooltip: s(format!(
                    "{}: {}",
                    format::day(d.day, today),
                    if d.seconds > 0 {
                        format::spoken_duration(d.seconds)
                    } else {
                        "no play".into()
                    }
                )),
                played: d.seconds > 0,
            }
        })
        .collect();
    window.set_overview_bars(rows(bars));

    state.games = model.games.iter().map(|g| g.name.clone()).collect();
    window.set_overview_games(rows(model.games.iter().map(|g| game_row(g, now)).collect()));

    let finished: Vec<SessionView> = model
        .sessions_for(name.as_deref())
        .filter(|s| !s.is_live)
        .cloned()
        .collect();
    let total = finished.len();
    state.sessions = finished.into_iter().take(state.shown).collect();
    window.set_overview_sessions(rows(
        state.sessions.iter().map(|s| session_row(s, now)).collect(),
    ));
    window.set_overview_sessions_header(s(if total > 0 {
        format!("Sessions ({total})")
    } else {
        "Sessions".into()
    }));
    window.set_overview_show_more_text(s(if total > state.shown {
        format!("Show more ({} older)", total - state.shown)
    } else {
        String::new()
    }));
    window.set_overview_empty(total == 0 && state.live.is_empty());
}

fn model_of_tiles(snapshot: &DashboardSnapshot, state: &OverviewState) -> ModelRc<Tile> {
    rows(tiles(snapshot, state))
}

/// A second passed while a game runs: only the times moved, so update the figures and the running sessions' rows
/// without rebuilding the lists (focus and scrolling stay put).
pub fn render_times(window: &AppWindow, snapshot: &DashboardSnapshot, state: &mut OverviewState) {
    let now = snapshot.model.now;
    let name = chosen(snapshot, state.game.as_deref()).map(|g| g.name.clone());
    let live: Vec<SessionView> = snapshot
        .model
        .sessions_for(name.as_deref())
        .filter(|s| s.is_live)
        .cloned()
        .collect();
    let same = live.len() == state.live.len()
        && live
            .iter()
            .zip(&state.live)
            .all(|(a, b)| a.start == b.start && paths::key(&a.game) == paths::key(&b.game));
    if !same {
        render(window, snapshot, state);
        return;
    }
    window.set_overview_tiles(model_of_tiles(snapshot, state));
    let rows = window.get_overview_live();
    for (i, session) in live.iter().enumerate() {
        use slint::Model;
        rows.set_row_data(i, session_row(session, now));
    }
    state.live = live;
}

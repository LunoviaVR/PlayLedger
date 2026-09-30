//! The History page: the last 30 days, newest first, each expandable into the sessions played that day.

use crate::format;
use crate::overview::session_row;
use crate::ui::{AppWindow, HistoryDay};
use chrono::{Local, NaiveDate};
use playtime_core::dashboard::SessionView;
use playtime_core::ipc::DashboardSnapshot;
use slint::{ModelRc, SharedString, VecModel};
use std::collections::HashSet;

/// Which days are expanded, and what each shown session row stands for (so a click can be mapped back).
#[derive(Default)]
pub struct HistoryState {
    pub expanded: HashSet<NaiveDate>,
    pub days: Vec<NaiveDate>,
    pub sessions: Vec<Vec<SessionView>>,
}

fn s(text: impl Into<SharedString>) -> SharedString {
    text.into()
}

pub fn render(window: &AppWindow, snapshot: &DashboardSnapshot, state: &mut HistoryState) {
    let model = &snapshot.model;
    let now = model.now;
    let today = format::local_date(now);
    let busiest = snapshot
        .history
        .iter()
        .map(|d| d.total_seconds)
        .max()
        .unwrap_or(0);
    state.days.clear();
    state.sessions.clear();
    let mut rows = Vec::new();
    for day in &snapshot.history {
        let label = format::day(day.day, today);
        // A day with sessions was played, even if they were under a second.
        let played = day.total_seconds > 0 || day.session_count > 0;
        let games = if day.games.is_empty() {
            String::new()
        } else {
            let mut text = day
                .games
                .iter()
                .take(3)
                .map(|g| format!("{} {}", g.game, format::duration(g.seconds)))
                .collect::<Vec<_>>()
                .join(", ");
            if day.games.len() > 3 {
                text.push_str(&format!(" and {} more", day.games.len() - 3));
            }
            text
        };
        let sessions: Vec<SessionView> =
            model.sessions_on(day.day, None, &Local).cloned().collect();
        let expanded = played && state.expanded.contains(&day.day);
        rows.push(HistoryDay {
            day: s(label.as_str()),
            total: s(if played {
                format::duration(day.total_seconds)
            } else {
                "No play".into()
            }),
            accessible: s(format!(
                "{label}, {}{}",
                if played {
                    format::spoken_duration(day.total_seconds)
                } else {
                    "no play".into()
                },
                if games.is_empty() {
                    String::new()
                } else {
                    format!(": {games}")
                }
            )),
            games: s(games),
            fraction: if busiest > 0 {
                (day.total_seconds as f32 / busiest as f32).clamp(0.0, 1.0)
            } else {
                0.0
            },
            played,
            expanded,
            sessions: ModelRc::new(VecModel::from(if expanded {
                sessions
                    .iter()
                    .map(|x| session_row(x, now))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            })),
        });
        state.days.push(day.day);
        state.sessions.push(sessions);
    }
    window.set_history_days(ModelRc::new(VecModel::from(rows)));
}

//! The Statistics page: days played and averages over the last 30 days, the longest session, and where the time
//! goes by game, by day of the week and by time of day.

use crate::format;
use crate::ui::{AppWindow, RankRow, Tile};
use chrono::{Datelike, Duration, DurationRound, Timelike};
use playtime_core::ipc::DashboardSnapshot;
use slint::{ModelRc, SharedString, VecModel};

/// Games listed under "Most played".
const TOP_COUNT: usize = 10;

fn s(text: impl Into<SharedString>) -> SharedString {
    text.into()
}

fn rank(name: &str, seconds: i64, max: i64, total: i64) -> RankRow {
    let percent = if total > 0 {
        format!("{:.0}%", 100.0 * seconds as f64 / total as f64)
    } else {
        String::new()
    };
    RankRow {
        name: s(name),
        fraction: if max > 0 {
            seconds as f32 / max as f32
        } else {
            0.0
        },
        value: s(format::duration(seconds)),
        accessible: s(format!(
            "{name}, {}{}",
            format::spoken_duration(seconds),
            if percent.is_empty() {
                String::new()
            } else {
                format!(", {percent}")
            }
        )),
        percent: s(percent),
    }
}

fn rows(items: Vec<RankRow>) -> ModelRc<RankRow> {
    ModelRc::new(VecModel::from(items))
}

/// Seconds per weekday (Monday first) and per part of the day (morning, afternoon, evening, night), splitting every
/// session at each local hour so a long evening session counts where it was played.
pub fn spread(snapshot: &DashboardSnapshot) -> ([i64; 7], [i64; 4]) {
    let mut by_weekday = [0i64; 7];
    let mut by_part = [0i64; 4];
    for session in &snapshot.model.sessions {
        let end = format::local(session.end);
        let mut t = format::local(session.start);
        while t < end {
            let next_hour = t
                .duration_trunc(Duration::hours(1))
                .map(|h| h + Duration::hours(1))
                .unwrap_or(end);
            let until = next_hour.min(end);
            let seconds = (until - t).num_seconds();
            by_weekday[t.weekday().num_days_from_monday() as usize] += seconds;
            by_part[match t.hour() {
                0..=5 => 3,
                6..=11 => 0,
                12..=17 => 1,
                _ => 2,
            }] += seconds;
            if until <= t {
                break;
            }
            t = until;
        }
    }
    (by_weekday, by_part)
}

pub fn render(window: &AppWindow, snapshot: &DashboardSnapshot) {
    let model = &snapshot.model;
    let now = model.now;
    let played: Vec<_> = snapshot
        .history
        .iter()
        .filter(|d| d.session_count > 0)
        .collect();
    let longest = model.sessions.iter().max_by_key(|x| x.seconds);
    let tile = |caption: &str, value: String, detail: String| Tile {
        caption: s(caption),
        value: s(value),
        detail: s(detail),
    };
    window.set_statistics_tiles(ModelRc::new(VecModel::from(vec![
        tile(
            "Days played (last 30)",
            played.len().to_string(),
            String::new(),
        ),
        tile(
            "Average per day played",
            if played.is_empty() {
                "–".into()
            } else {
                format::duration(
                    played.iter().map(|d| d.total_seconds).sum::<i64>() / played.len() as i64,
                )
            },
            String::new(),
        ),
        tile(
            "Longest session",
            longest.map_or("–".into(), |x| format::duration(x.seconds)),
            longest.map_or(String::new(), |x| {
                format!("{}, {}", x.game, format::day_of(x.start, now))
            }),
        ),
        tile("Games played", model.games.len().to_string(), String::new()),
    ])));

    let total: i64 = model.games.iter().map(|g| g.total_seconds).sum();
    let top: Vec<_> = model.games.iter().take(TOP_COUNT).collect();
    let max = top.iter().map(|g| g.total_seconds).max().unwrap_or(0);
    window.set_statistics_top_games(rows(
        top.iter()
            .map(|g| rank(&g.name, g.total_seconds, max, total))
            .collect(),
    ));

    let (by_weekday, by_part) = spread(snapshot);
    let (weekday_max, weekday_total) = (
        by_weekday.iter().copied().max().unwrap_or(0),
        by_weekday.iter().sum(),
    );
    window.set_statistics_weekdays(rows(
        format::week()
            .into_iter()
            .map(|(day, name)| {
                rank(
                    &name,
                    by_weekday[day.num_days_from_monday() as usize],
                    weekday_max,
                    weekday_total,
                )
            })
            .collect(),
    ));
    let parts = [
        "Morning (6–12)",
        "Afternoon (12–18)",
        "Evening (18–24)",
        "Night (0–6)",
    ];
    let (part_max, part_total) = (
        by_part.iter().copied().max().unwrap_or(0),
        by_part.iter().sum(),
    );
    window.set_statistics_day_parts(rows(
        parts
            .iter()
            .enumerate()
            .map(|(i, name)| rank(name, by_part[i], part_max, part_total))
            .collect(),
    ));
}

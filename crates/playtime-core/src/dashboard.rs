//! What the dashboard shows, computed from the play history at one moment: sessions
//! newest first with live ones running up to "now", games most played first, per-day totals for the last 30 days
//! (sessions crossing midnight are split between days), and the History page's days. Serializable, so the tracker
//! can hand it to the dashboard over IPC.

use crate::model::{ActiveSession, SessionRecord};
use crate::paths;
use crate::time::Timestamp;
use chrono::{Duration, NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Days shown in the chart and on the History page.
pub const CHART_DAYS: usize = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub game: String,
    pub start: Timestamp,
    /// For a live session, "now".
    pub end: Timestamp,
    pub is_live: bool,
    pub executable: Option<String>,
    pub seconds: i64,
}

impl SessionView {
    fn duration(&self) -> Duration {
        self.end.since(self.start)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    /// The newest spelling of the name.
    pub name: String,
    pub session_count: usize,
    pub total_seconds: i64,
    pub average_seconds: i64,
    pub longest_seconds: i64,
    pub first_played: Timestamp,
    pub last_played: Timestamp,
    pub is_live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayTotal {
    pub day: NaiveDate,
    pub seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameTime {
    pub game: String,
    pub seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayHistory {
    pub day: NaiveDate,
    pub total_seconds: i64,
    /// Most played first.
    pub games: Vec<GameTime>,
    pub session_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardModel {
    pub now: Timestamp,
    /// Newest first, live sessions included.
    pub sessions: Vec<SessionView>,
    /// Most played first.
    pub games: Vec<GameView>,
    /// Running now, oldest first.
    pub live: Vec<SessionView>,
}

impl DashboardModel {
    pub fn new(finished: &[SessionRecord], active: &[ActiveSession], now: Timestamp) -> Self {
        let view =
            |game: &str, start: Timestamp, end: Timestamp, is_live: bool, exe: &Option<String>| {
                SessionView {
                    game: game.to_string(),
                    start,
                    end,
                    is_live,
                    executable: exe.clone(),
                    seconds: end.since(start).num_seconds(),
                }
            };
        let mut live: Vec<SessionView> = active
            .iter()
            .map(|a| {
                view(
                    &a.game,
                    a.start,
                    if now < a.start { a.start } else { now },
                    true,
                    &a.executable,
                )
            })
            .collect();
        live.sort_by_key(|s| s.start);

        let mut sessions: Vec<SessionView> = finished
            .iter()
            .map(|s| view(&s.game, s.start, s.end, false, &s.executable))
            .chain(live.iter().cloned())
            .collect();
        sessions.sort_by(|a, b| b.start.cmp(&a.start));

        let mut order: Vec<String> = Vec::new();
        let mut groups: HashMap<String, Vec<&SessionView>> = HashMap::new();
        for s in &sessions {
            let key = paths::key(&s.game);
            if !groups.contains_key(&key) {
                order.push(key.clone());
            }
            groups.entry(key).or_default().push(s);
        }
        let mut games: Vec<GameView> = order
            .iter()
            .filter_map(|key| {
                let list = groups.get(key)?;
                let total = list
                    .iter()
                    .fold(Duration::zero(), |sum, s| sum + s.duration());
                let count = list.len();
                Some(GameView {
                    name: list.first()?.game.clone(),
                    session_count: count,
                    total_seconds: total.num_seconds(),
                    average_seconds: total.num_seconds() / i64::try_from(count.max(1)).unwrap_or(1),
                    longest_seconds: list
                        .iter()
                        .map(|s| s.duration().num_seconds())
                        .max()
                        .unwrap_or(0),
                    first_played: list.iter().map(|s| s.start).min()?,
                    last_played: list.iter().map(|s| s.end).max()?,
                    is_live: list.iter().any(|s| s.is_live),
                })
            })
            .collect();
        games.sort_by(|a, b| {
            b.total_seconds
                .cmp(&a.total_seconds)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Self {
            now,
            sessions,
            games,
            live,
        }
    }

    /// Sessions of one game (case-insensitive), or all when `game` is `None`; newest first.
    pub fn sessions_for<'a>(
        &'a self,
        game: Option<&'a str>,
    ) -> impl Iterator<Item = &'a SessionView> + 'a {
        let key = game.map(paths::key);
        self.sessions
            .iter()
            .filter(move |s| key.as_ref().is_none_or(|k| paths::key(&s.game) == *k))
    }

    fn today<Tz: TimeZone>(&self, tz: &Tz) -> NaiveDate {
        self.now.as_datetime().with_timezone(tz).date_naive()
    }

    /// Playtime per local day for the last [`CHART_DAYS`] days, oldest first.
    pub fn daily_totals<Tz: TimeZone>(&self, game: Option<&str>, tz: &Tz) -> Vec<DayTotal> {
        let today = self.today(tz);
        let first = today - Duration::days(CHART_DAYS as i64 - 1);
        let mut totals = vec![Duration::zero(); CHART_DAYS];
        for s in self.sessions_for(game) {
            for (day, time) in split_by_day(s.start, s.end, tz) {
                if day >= first && day <= today {
                    if let Ok(index) = usize::try_from((day - first).num_days()) {
                        totals[index] += time;
                    }
                }
            }
        }
        totals
            .into_iter()
            .enumerate()
            .map(|(i, t)| DayTotal {
                day: first + Duration::days(i as i64),
                seconds: t.num_seconds(),
            })
            .collect()
    }

    /// The last [`CHART_DAYS`] days, newest first, including days without play.
    pub fn history<Tz: TimeZone>(&self, tz: &Tz) -> Vec<DayHistory> {
        let today = self.today(tz);
        let first = today - Duration::days(CHART_DAYS as i64 - 1);
        let mut per_day: HashMap<NaiveDate, Vec<(String, Duration)>> = HashMap::new();
        let mut counts: HashMap<NaiveDate, usize> = HashMap::new();
        for s in &self.sessions {
            let mut parts = split_by_day(s.start, s.end, tz);
            if parts.is_empty() {
                // A session seen by a single poll lasts no time, but it still happened on its start day.
                parts.push((
                    s.start.as_datetime().with_timezone(tz).date_naive(),
                    Duration::zero(),
                ));
            }
            for (day, time) in parts {
                if day < first || day > today {
                    continue;
                }
                let games = per_day.entry(day).or_default();
                match games
                    .iter_mut()
                    .find(|(g, _)| paths::key(g) == paths::key(&s.game))
                {
                    Some((_, total)) => *total += time,
                    None => games.push((s.game.clone(), time)),
                }
                *counts.entry(day).or_default() += 1;
            }
        }
        (0..CHART_DAYS as i64)
            .map(|back| {
                let day = today - Duration::days(back);
                let mut games = per_day.remove(&day).unwrap_or_default();
                games.sort_by(|a, b| b.1.cmp(&a.1));
                DayHistory {
                    day,
                    total_seconds: games
                        .iter()
                        .fold(Duration::zero(), |sum, g| sum + g.1)
                        .num_seconds(),
                    games: games
                        .into_iter()
                        .map(|(game, time)| GameTime {
                            game,
                            seconds: time.num_seconds(),
                        })
                        .collect(),
                    session_count: counts.get(&day).copied().unwrap_or(0),
                }
            })
            .collect()
    }

    /// Sessions overlapping a local day (optionally one game's), newest first.
    pub fn sessions_on<'a, Tz: TimeZone>(
        &'a self,
        day: NaiveDate,
        game: Option<&'a str>,
        tz: &'a Tz,
    ) -> impl Iterator<Item = &'a SessionView> + 'a {
        self.sessions_for(game).filter(move |s| {
            split_by_day(s.start, s.end, tz)
                .iter()
                .any(|(d, _)| *d == day)
        })
    }

    /// Playtime after `since` (the "past 7 days" tile).
    pub fn total_since(&self, game: Option<&str>, since: Timestamp) -> Duration {
        self.sessions_for(game)
            .filter(|s| s.end > since)
            .fold(Duration::zero(), |sum, s| {
                sum + s.end.since(if s.start > since { s.start } else { since })
            })
    }
}

/// The part of a session on each local calendar day.
pub fn split_by_day<Tz: TimeZone>(
    start: Timestamp,
    end: Timestamp,
    tz: &Tz,
) -> Vec<(NaiveDate, Duration)> {
    let local = |t: Timestamp| t.as_datetime().with_timezone(tz).naive_local();
    let (start, end): (NaiveDateTime, NaiveDateTime) = (local(start), local(end));
    let mut out = Vec::new();
    let mut day = start.date();
    while day <= end.date() {
        let (Some(day_start), Some(next)) = (day.and_hms_opt(0, 0, 0), day.succ_opt()) else {
            break;
        };
        let Some(next_start) = next.and_hms_opt(0, 0, 0) else {
            break;
        };
        let from = start.max(day_start);
        let to = end.min(next_start);
        if to > from {
            out.push((day, to - from));
        }
        day = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn at(text: &str) -> Timestamp {
        Timestamp::parse(text).expect("valid")
    }

    fn session(game: &str, start: &str, end: &str) -> SessionRecord {
        SessionRecord {
            game: game.into(),
            start: at(start),
            end: at(end),
            executable: None,
        }
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).expect("offset")
    }

    #[test]
    fn splits_sessions_across_midnight() {
        let parts = split_by_day(
            at("2026-09-28T23:00:00+00:00"),
            at("2026-09-29T01:30:00+00:00"),
            &utc(),
        );
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 9, d).expect("date");
        assert_eq!(
            parts,
            vec![
                (day(28), Duration::hours(1)),
                (day(29), Duration::minutes(90))
            ]
        );
        // The same instants in UTC+2 fall on one day.
        let plus2 = FixedOffset::east_opt(2 * 3600).expect("offset");
        assert_eq!(
            split_by_day(
                at("2026-09-28T23:00:00+00:00"),
                at("2026-09-29T01:30:00+00:00"),
                &plus2
            )
            .len(),
            1
        );
    }

    #[test]
    fn games_history_and_totals() {
        let finished = [
            session(
                "Elden Ring",
                "2026-09-27T20:00:00+00:00",
                "2026-09-27T21:00:00+00:00",
            ),
            session(
                "beat saber",
                "2026-09-28T23:00:00+00:00",
                "2026-09-29T00:30:00+00:00",
            ),
            session(
                "Beat Saber",
                "2026-09-29T08:00:00+00:00",
                "2026-09-29T08:10:00+00:00",
            ),
        ];
        let active = [ActiveSession {
            game: "Hades".into(),
            start: at("2026-09-29T09:00:00+00:00"),
            last_seen: at("2026-09-29T09:05:00+00:00"),
            executable: Some(r"C:\Games\Hades\Hades.exe".into()),
        }];
        let model = DashboardModel::new(&finished, &active, at("2026-09-29T09:20:00+00:00"));

        assert_eq!(model.live.len(), 1);
        assert_eq!(model.live[0].seconds, 20 * 60, "live sessions run to now");
        assert_eq!(model.sessions[0].game, "Hades", "newest first");
        let names: Vec<&str> = model.games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            ["Beat Saber", "Elden Ring", "Hades"],
            "grouped case-insensitively, newest spelling"
        );
        assert_eq!(model.games[0].total_seconds, 100 * 60);
        assert_eq!(model.games[0].longest_seconds, 90 * 60);
        assert!(model.games[2].is_live);

        let days = model.daily_totals(None, &utc());
        assert_eq!(days.len(), CHART_DAYS);
        assert_eq!(days[CHART_DAYS - 1].seconds, (30 + 10 + 20) * 60, "today");
        assert_eq!(
            days[CHART_DAYS - 2].seconds,
            60 * 60,
            "yesterday: the part before midnight"
        );
        assert_eq!(
            model.daily_totals(Some("ELDEN RING"), &utc())[CHART_DAYS - 3].seconds,
            3600
        );

        let history = model.history(&utc());
        assert_eq!(
            history[0].day,
            NaiveDate::from_ymd_opt(2026, 9, 29).expect("date")
        );
        assert_eq!(history[0].session_count, 3);
        assert_eq!(history[0].games[0].game, "Beat Saber");
        assert_eq!(history[0].games[0].seconds, 40 * 60);
        assert_eq!(history[5].total_seconds, 0);

        let today = NaiveDate::from_ymd_opt(2026, 9, 29).expect("date");
        assert_eq!(
            model.sessions_on(today, Some("beat saber"), &utc()).count(),
            2
        );

        // A session seen by a single poll (no length) still counts on its day.
        let blip = [session(
            "Cookie Clicker",
            "2026-09-29T08:30:00+00:00",
            "2026-09-29T08:30:00+00:00",
        )];
        let history =
            DashboardModel::new(&blip, &[], at("2026-09-29T09:00:00+00:00")).history(&utc());
        assert_eq!(history[0].session_count, 1);
        assert_eq!(history[0].games[0].game, "Cookie Clicker");
        assert_eq!(history[0].total_seconds, 0);
        assert_eq!(
            model.total_since(None, at("2026-09-29T00:00:00+00:00")),
            Duration::minutes(30 + 10 + 20)
        );
    }
}

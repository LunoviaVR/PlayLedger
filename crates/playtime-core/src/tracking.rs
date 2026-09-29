//! Turns "which games are running right now" snapshots into start/end play sessions.
//! A port of the C# `SessionTracker`, with the same rules:
//! - a session starts when a game's process first appears and ends when it has been gone for the grace period
//!   (so a launcher handing off to the game, or a quick restart, stays one session);
//! - a session ends at the moment the game was *last seen*, not when the gap was noticed;
//! - sessions shorter than the minimum length are dropped (the minimum is off by default);
//! - sessions still open from a previous run (crash, power cut) are closed at their last-seen time;
//! - after sleep, [`SessionTracker::end_all`] closes everything at last-seen so sleep never counts as play.

use crate::model::{ActiveSession, SessionRecord, TrackerData};
use crate::paths;
use crate::time::Timestamp;
use std::collections::HashMap;

/// Tracking rules (a subset of the settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackingRules {
    pub minimum_session_seconds: i64,
    pub grace_period_seconds: i64,
}

/// What changed in one update.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TrackingChanges {
    pub started: Vec<String>,
    pub ended: Vec<SessionRecord>,
}

impl TrackingChanges {
    pub fn is_empty(&self) -> bool {
        self.started.is_empty() && self.ended.is_empty()
    }
}

#[derive(Debug)]
pub struct SessionTracker {
    rules: TrackingRules,
    /// Keyed case-insensitively by game name.
    active: HashMap<String, ActiveSession>,
}

impl SessionTracker {
    /// Starts tracking, first closing any sessions `data` still has open from a previous run
    /// (recorded into `data.sessions` at their last-seen time).
    pub fn new(data: &mut TrackerData, rules: TrackingRules) -> Self {
        let tracker = Self {
            rules,
            active: HashMap::new(),
        };
        for interrupted in std::mem::take(&mut data.active) {
            let end = interrupted.last_seen;
            tracker.record(data, interrupted, end);
        }
        tracker
    }

    pub fn set_rules(&mut self, rules: TrackingRules) {
        self.rules = rules;
    }

    /// Currently running sessions, oldest first.
    pub fn active(&self) -> Vec<&ActiveSession> {
        let mut active: Vec<_> = self.active.values().collect();
        active.sort_by_key(|a| a.start);
        active
    }

    /// Applies a snapshot of running games (game name → executable path).
    pub fn update(
        &mut self,
        data: &mut TrackerData,
        now: Timestamp,
        running: &HashMap<String, String>,
    ) -> TrackingChanges {
        let mut changes = TrackingChanges::default();
        let running_keys: HashMap<String, (&String, &String)> = running
            .iter()
            .map(|(game, exe)| (paths::key(game), (game, exe)))
            .collect();

        for (key, (game, exe)) in &running_keys {
            match self.active.get_mut(key) {
                Some(session) => session.last_seen = now,
                None => {
                    self.active.insert(
                        key.clone(),
                        ActiveSession {
                            game: (*game).clone(),
                            start: now,
                            last_seen: now,
                            executable: Some((*exe).clone()),
                        },
                    );
                    changes.started.push((*game).clone());
                }
            }
        }

        let gone: Vec<String> = self
            .active
            .iter()
            .filter(|(key, session)| {
                !running_keys.contains_key(*key)
                    && now.since(session.last_seen).num_seconds() >= self.rules.grace_period_seconds
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in gone {
            if let Some(session) = self.active.remove(&key) {
                let end = session.last_seen;
                if let Some(record) = self.record(data, session, end) {
                    changes.ended.push(record);
                }
            }
        }

        self.sync_active(data);
        changes
    }

    /// Ends every running session: at `at` (the tracker is exiting), or at each game's last-seen time
    /// when `at` is `None` (the PC slept or hibernated, so the gap must not count).
    pub fn end_all(&mut self, data: &mut TrackerData, at: Option<Timestamp>) -> Vec<SessionRecord> {
        let mut ended = Vec::new();
        let mut sessions: Vec<_> = self.active.drain().map(|(_, s)| s).collect();
        sessions.sort_by_key(|s| s.start);
        for session in sessions {
            let end = at.unwrap_or(session.last_seen);
            if let Some(record) = self.record(data, session, end) {
                ended.push(record);
            }
        }
        self.sync_active(data);
        ended
    }

    fn record(
        &self,
        data: &mut TrackerData,
        session: ActiveSession,
        end: Timestamp,
    ) -> Option<SessionRecord> {
        let length = end.since(session.start);
        if length.num_milliseconds() < 0
            || length.num_seconds() < self.rules.minimum_session_seconds
        {
            return None;
        }
        let record = SessionRecord {
            game: session.game,
            start: session.start,
            end,
            executable: session.executable,
        };
        data.sessions.push(record.clone());
        Some(record)
    }

    fn sync_active(&self, data: &mut TrackerData) {
        data.active = self.active().into_iter().cloned().collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    const RULES: TrackingRules = TrackingRules {
        minimum_session_seconds: 0,
        grace_period_seconds: 20,
    };

    fn t(seconds: i64) -> Timestamp {
        Timestamp::parse("2026-09-29T12:00:00+02:00")
            .expect("valid")
            .checked_add(Duration::seconds(seconds))
            .expect("in range")
    }

    fn running(games: &[&str]) -> HashMap<String, String> {
        games
            .iter()
            .map(|g| ((*g).to_string(), format!(r"C:\Games\{g}\{g}.exe")))
            .collect()
    }

    #[test]
    fn short_session_is_recorded_by_default() {
        let mut data = TrackerData::default();
        let mut tracker = SessionTracker::new(
            &mut data,
            TrackingRules {
                grace_period_seconds: 0,
                ..RULES
            },
        );
        tracker.update(&mut data, t(0), &running(&["Cookie Clicker"]));
        tracker.update(&mut data, t(22), &running(&["Cookie Clicker"]));
        let changes = tracker.update(&mut data, t(27), &running(&[]));
        assert_eq!(changes.ended.len(), 1);
        assert_eq!(data.sessions[0].duration(), Duration::seconds(22));
        assert!(data.active.is_empty());
    }

    #[test]
    fn minimum_length_drops_short_sessions() {
        let mut data = TrackerData::default();
        let mut tracker = SessionTracker::new(
            &mut data,
            TrackingRules {
                minimum_session_seconds: 30,
                grace_period_seconds: 0,
            },
        );
        tracker.update(&mut data, t(0), &running(&["Game"]));
        tracker.update(&mut data, t(10), &running(&["Game"]));
        tracker.update(&mut data, t(15), &running(&[]));
        assert!(data.sessions.is_empty());
    }

    #[test]
    fn grace_period_keeps_one_session_across_a_restart() {
        let mut data = TrackerData::default();
        let mut tracker = SessionTracker::new(&mut data, RULES);
        tracker.update(&mut data, t(0), &running(&["Game"]));
        tracker.update(&mut data, t(60), &running(&["Game"]));
        tracker.update(&mut data, t(65), &running(&[])); // closed…
        tracker.update(&mut data, t(75), &running(&["Game"])); // …and back within 20 s
        tracker.update(&mut data, t(120), &running(&["Game"]));
        assert!(data.sessions.is_empty());
        let changes = tracker.update(&mut data, t(150), &running(&[]));
        assert_eq!(changes.ended.len(), 1);
        assert_eq!(changes.ended[0].start, t(0));
        assert_eq!(
            changes.ended[0].end,
            t(120),
            "ends when last seen, not when noticed"
        );
    }

    #[test]
    fn names_match_case_insensitively() {
        let mut data = TrackerData::default();
        let mut tracker = SessionTracker::new(&mut data, RULES);
        tracker.update(&mut data, t(0), &running(&["Elden Ring"]));
        let changes = tracker.update(&mut data, t(5), &running(&["ELDEN RING"]));
        assert!(changes.is_empty());
        assert_eq!(tracker.active().len(), 1);
    }

    #[test]
    fn sleep_is_not_counted() {
        let mut data = TrackerData::default();
        let mut tracker = SessionTracker::new(&mut data, RULES);
        tracker.update(&mut data, t(0), &running(&["Game"]));
        tracker.update(&mut data, t(600), &running(&["Game"]));
        // PC sleeps for hours; on resume everything ends at last-seen.
        let ended = tracker.end_all(&mut data, None);
        assert_eq!(ended[0].end, t(600));
    }

    #[test]
    fn exit_ends_at_the_given_time() {
        let mut data = TrackerData::default();
        let mut tracker = SessionTracker::new(&mut data, RULES);
        tracker.update(&mut data, t(0), &running(&["A", "B"]));
        let ended = tracker.end_all(&mut data, Some(t(90)));
        assert_eq!(ended.len(), 2);
        assert!(ended.iter().all(|s| s.end == t(90)));
    }

    #[test]
    fn crash_recovery_closes_open_sessions_at_last_seen() {
        let mut data = TrackerData::default();
        data.active.push(ActiveSession {
            game: "Game".into(),
            start: t(0),
            last_seen: t(300),
            executable: None,
        });
        let _tracker = SessionTracker::new(&mut data, RULES);
        assert!(data.active.is_empty());
        assert_eq!(data.sessions[0].end, t(300));
    }

    #[test]
    fn active_list_is_kept_in_data_for_crash_recovery() {
        let mut data = TrackerData::default();
        let mut tracker = SessionTracker::new(&mut data, RULES);
        tracker.update(&mut data, t(0), &running(&["B"]));
        tracker.update(&mut data, t(5), &running(&["A", "B"]));
        let names: Vec<_> = data.active.iter().map(|a| a.game.as_str()).collect();
        assert_eq!(names, ["B", "A"], "oldest first");
    }
}

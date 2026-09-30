//! The tracker's brain, independent of Windows: turns process snapshots into sessions with the tracking rules
//! (see `docs/architecture/compatibility.md`) and says when data must be saved. The Windows service only
//! supplies snapshots, power events and storage.

use crate::catalog::{GameCatalog, GameMatch};
use crate::model::{SessionRecord, TrackerData};
use crate::paths;
use crate::settings::Settings;
use crate::time::Timestamp;
use crate::tracking::{SessionTracker, TrackingChanges};
use chrono::Duration;
use std::collections::{HashMap, HashSet};

/// Running sessions are saved at least this often, so a crash loses at most about a minute.
pub const ACTIVE_SAVE_SECONDS: i64 = 60;
/// The game catalog is rebuilt this often (new installs show up without a restart).
pub const CATALOG_REFRESH_SECONDS: i64 = 10 * 60;

/// One running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    /// Image name (e.g. `game.exe`); with the pid, identifies a process across polls.
    pub name: String,
    /// Full executable path, if it could be read.
    pub path: Option<String>,
}

/// What one poll did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickOutcome {
    pub changes: TrackingChanges,
    /// Sessions ended because the gap since the last poll means the PC was asleep.
    pub slept: Vec<SessionRecord>,
    /// The play history changed (or running sessions are due to be saved).
    pub save: bool,
}

pub struct Engine {
    data: TrackerData,
    settings: Settings,
    catalog: GameCatalog,
    tracker: SessionTracker,
    /// pid → (image name, match), so each process is matched once. A reused pid with another name is matched again.
    matches: HashMap<u32, (String, Option<GameMatch>)>,
    /// Game name (lower-case) → how it was identified, for artwork and details.
    identities: HashMap<String, GameMatch>,
    last_poll: Option<Timestamp>,
    last_save: Option<Timestamp>,
    catalog_built: Timestamp,
    /// Bumped on every change, so views can tell when to refresh.
    revision: u64,
}

impl Engine {
    /// Starts tracking. Sessions left open by a previous run are closed at their last-seen time; if there were
    /// any, the returned engine reports `needs_save()`.
    pub fn new(
        mut data: TrackerData,
        settings: Settings,
        catalog: GameCatalog,
        now: Timestamp,
    ) -> Self {
        let recovered = !data.active.is_empty();
        let tracker = SessionTracker::new(&mut data, settings.tracking_rules());
        Self {
            data,
            settings,
            catalog,
            tracker,
            matches: HashMap::new(),
            identities: HashMap::new(),
            last_poll: None,
            last_save: if recovered { None } else { Some(now) },
            catalog_built: now,
            revision: 1,
        }
    }

    pub fn data(&self) -> &TrackerData {
        &self.data
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// True until the first save after recovering interrupted sessions.
    pub fn needs_save(&self) -> bool {
        self.last_save.is_none()
    }

    /// Call after the data was written.
    pub fn saved(&mut self, now: Timestamp) {
        self.last_save = Some(now);
    }

    /// How a game (by name) was identified, if it has run since the tracker started.
    pub fn identity(&self, game: &str) -> Option<&GameMatch> {
        self.identities.get(&paths::key(game))
    }

    /// How a game from the history is identified: from this run if it has been seen, else by matching the most
    /// recent executable recorded for it against the current catalog.
    pub fn identify(&mut self, game: &str) -> Option<GameMatch> {
        let key = paths::key(game);
        if let Some(known) = self.identities.get(&key) {
            return Some(known.clone());
        }
        let exe = self
            .data
            .active
            .iter()
            .filter(|a| paths::key(&a.game) == key)
            .filter_map(|a| a.executable.clone())
            .next()
            .or_else(|| {
                self.data
                    .sessions
                    .iter()
                    .filter(|s| paths::key(&s.game) == key && s.executable.is_some())
                    .max_by_key(|s| s.start)
                    .and_then(|s| s.executable.clone())
            })?;
        let mut found = self.catalog.match_exe(&exe)?;
        // Keep the history's name; the catalog may spell it differently today.
        found.name = game.to_string();
        self.identities.insert(key, found.clone());
        Some(found)
    }

    /// The most recent executable recorded for a game.
    pub fn executable_of(&self, game: &str) -> Option<String> {
        let key = paths::key(game);
        self.data
            .sessions
            .iter()
            .filter(|s| paths::key(&s.game) == key)
            .max_by_key(|s| s.start)
            .and_then(|s| s.executable.clone())
            .or_else(|| {
                self.data
                    .active
                    .iter()
                    .find(|a| paths::key(&a.game) == key)
                    .and_then(|a| a.executable.clone())
            })
    }

    pub fn is_running(&self, game: &str) -> bool {
        let key = paths::key(game);
        self.data.active.iter().any(|a| paths::key(&a.game) == key)
    }

    /// Longer than this between polls means the PC slept: max(90 s, 6 × poll interval).
    pub fn sleep_gap(&self) -> Duration {
        Duration::seconds(90.max(6 * i64::from(self.settings.poll_interval_seconds)))
    }

    pub fn catalog_due(&self, now: Timestamp) -> bool {
        now.since(self.catalog_built).num_seconds() >= CATALOG_REFRESH_SECONDS
    }

    pub fn set_catalog(&mut self, catalog: GameCatalog, now: Timestamp) {
        self.catalog = catalog;
        self.catalog_built = now;
        self.matches.clear();
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.tracker.set_rules(settings.tracking_rules());
        self.settings = settings;
        self.matches.clear();
        self.revision += 1;
    }

    /// One poll with the current process list.
    pub fn tick(&mut self, now: Timestamp, processes: &[ProcessInfo]) -> TickOutcome {
        let mut outcome = TickOutcome::default();
        if let Some(last) = self.last_poll {
            if now.since(last) > self.sleep_gap() {
                outcome.slept = self.tracker.end_all(&mut self.data, None);
            }
        }
        self.last_poll = Some(now);

        let running = self.running_games(processes);
        outcome.changes = self.tracker.update(&mut self.data, now, &running);

        let changed = !outcome.slept.is_empty() || !outcome.changes.is_empty();
        let active_due = !self.data.active.is_empty()
            && self
                .last_save
                .is_none_or(|last| now.since(last).num_seconds() >= ACTIVE_SAVE_SECONDS);
        outcome.save = changed || active_due || self.needs_save();
        if changed {
            self.revision += 1;
        }
        outcome
    }

    fn running_games(&mut self, processes: &[ProcessInfo]) -> HashMap<String, String> {
        let mut running: HashMap<String, String> = HashMap::new();
        let mut seen_keys: HashSet<String> = HashSet::new();
        let mut alive: HashSet<u32> = HashSet::with_capacity(processes.len());
        for process in processes {
            if process.pid <= 4 {
                continue; // System Idle / System
            }
            alive.insert(process.pid);
            let catalog = &self.catalog;
            let fresh = || process.path.as_deref().and_then(|p| catalog.match_exe(p));
            let entry = self
                .matches
                .entry(process.pid)
                .and_modify(|(name, matched)| {
                    if *name != process.name {
                        process.name.clone_into(name);
                        *matched = fresh();
                    }
                })
                .or_insert_with(|| (process.name.clone(), fresh()));
            let (Some(game), Some(path)) = (entry.1.as_ref(), process.path.as_ref()) else {
                continue;
            };
            let game_key = paths::key(&game.name);
            if seen_keys.insert(game_key.clone()) {
                running.insert(game.name.clone(), path.clone());
                self.identities.insert(game_key, game.clone());
            }
        }
        self.matches.retain(|pid, _| alive.contains(pid));
        running
    }

    /// The PC is going to sleep: end everything at last-seen so sleep never counts.
    pub fn suspend(&mut self) -> Vec<SessionRecord> {
        self.last_poll = None;
        let ended = self.tracker.end_all(&mut self.data, None);
        if !ended.is_empty() {
            self.revision += 1;
        }
        ended
    }

    /// The tracker is exiting (or Windows is shutting down): end everything now.
    pub fn shutdown(&mut self, now: Timestamp) -> Vec<SessionRecord> {
        let ended = self.tracker.end_all(&mut self.data, Some(now));
        self.revision += 1;
        ended
    }

    /// Deletes one finished session (matched by game and start). Returns whether one was removed.
    pub fn delete_session(&mut self, game: &str, start: Timestamp) -> bool {
        let key = paths::key(game);
        let before = self.data.sessions.len();
        self.data
            .sessions
            .retain(|s| !(s.start == start && paths::key(&s.game) == key));
        let removed = self.data.sessions.len() != before;
        if removed {
            self.revision += 1;
        }
        removed
    }

    /// Deletes every finished session of a game. Returns how many were removed.
    pub fn delete_game_history(&mut self, game: &str) -> usize {
        let key = paths::key(game);
        let before = self.data.sessions.len();
        self.data.sessions.retain(|s| paths::key(&s.game) != key);
        let removed = before - self.data.sessions.len();
        if removed > 0 {
            self.revision += 1;
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CatalogBuilder;
    use crate::launchers::GameSource;
    use crate::model::ActiveSession;

    fn t(seconds: i64) -> Timestamp {
        Timestamp::parse("2026-09-29T12:00:00+00:00")
            .expect("valid")
            .checked_add(Duration::seconds(seconds))
            .expect("in range")
    }

    fn catalog(settings: &Settings) -> GameCatalog {
        let mut builder = CatalogBuilder::default();
        builder.add_root(r"C:\Games", GameSource::CustomFolder);
        builder.build(settings)
    }

    fn process(pid: u32, path: &str) -> ProcessInfo {
        ProcessInfo {
            pid,
            name: paths::file_name(path).to_string(),
            path: Some(path.to_string()),
        }
    }

    fn engine(data: TrackerData) -> Engine {
        let settings = Settings::default();
        let catalog = catalog(&settings);
        Engine::new(data, settings, catalog, t(0))
    }

    #[test]
    fn sessions_start_end_and_save() {
        let mut engine = engine(TrackerData::default());
        assert!(!engine.needs_save());
        let hades = [
            process(100, r"C:\Games\Hades\Hades.exe"),
            process(200, r"C:\Windows\explorer.exe"),
        ];

        let first = engine.tick(t(0), &hades);
        assert_eq!(first.changes.started, ["Hades"]);
        assert!(first.save);
        engine.saved(t(0));
        assert!(engine.is_running("hades"));
        assert_eq!(
            engine.identity("Hades").map(|m| m.source),
            Some(GameSource::CustomFolder)
        );

        assert!(!engine.tick(t(5), &hades).save, "nothing new");
        assert!(
            engine.tick(t(60), &hades).save,
            "running sessions saved every minute"
        );
        engine.saved(t(60));

        // Gone, then past the 20 s grace period: ends at last seen.
        assert!(!engine.tick(t(65), &[]).save);
        let ended = engine.tick(t(85), &[]);
        assert_eq!(ended.changes.ended.len(), 1);
        assert_eq!(ended.changes.ended[0].end, t(60));
        assert!(ended.save);
    }

    #[test]
    fn a_reused_pid_is_matched_again() {
        let mut engine = engine(TrackerData::default());
        engine.tick(t(0), &[process(100, r"C:\Windows\explorer.exe")]);
        // Windows gave the pid to a game after explorer.exe exited.
        let after = engine.tick(t(5), &[process(100, r"C:\Games\Hades\Hades.exe")]);
        assert_eq!(after.changes.started, ["Hades"]);
        // And back again: the game is gone (it ends once the grace period has passed).
        let explorer = [process(100, r"C:\Windows\explorer.exe")];
        engine.tick(t(10), &explorer);
        assert!(!engine.tick(t(60), &explorer).changes.ended.is_empty());
        assert!(!engine.is_running("hades"));
    }

    #[test]
    fn a_long_gap_is_sleep() {
        let mut engine = engine(TrackerData::default());
        let game = [process(100, r"C:\Games\Celeste\Celeste.exe")];
        engine.tick(t(0), &game);
        engine.tick(t(5), &game);
        // Two hours later (the PC slept with the game open): the old session ends at 5 s, a new one starts.
        let after = engine.tick(t(7200), &game);
        assert_eq!(after.slept.len(), 1);
        assert_eq!(after.slept[0].end, t(5));
        assert_eq!(after.changes.started, ["Celeste"]);
    }

    #[test]
    fn recovers_interrupted_sessions_and_deletes() {
        let data = TrackerData {
            active: vec![ActiveSession {
                game: "Hades".into(),
                start: t(-3600),
                last_seen: t(-60),
                executable: None,
            }],
            ..TrackerData::default()
        };
        let mut engine = engine(data);
        assert!(engine.needs_save());
        assert_eq!(engine.data().sessions.len(), 1);
        assert_eq!(engine.data().sessions[0].end, t(-60));
        assert!(engine.tick(t(0), &[]).save);

        assert_eq!(engine.identify("Hades"), None, "no executable recorded");
        assert!(!engine.delete_session("hades", t(0)));
        assert!(engine.delete_session("HADES", t(-3600)));
        assert_eq!(engine.delete_game_history("Hades"), 0);
    }

    #[test]
    fn shutdown_ends_now_and_suspend_at_last_seen() {
        let mut engine = engine(TrackerData::default());
        let game = [process(100, r"C:\Games\Hades\Hades.exe")];
        engine.tick(t(0), &game);
        engine.tick(t(30), &game);
        let ended = engine.shutdown(t(40));
        assert_eq!(ended[0].end, t(40));
        assert_eq!(
            engine.executable_of("hades").as_deref(),
            Some(r"C:\Games\Hades\Hades.exe")
        );
        let mut fresh = self::engine(engine.data().clone());
        let identified = fresh
            .identify("Hades")
            .expect("matched from the recorded exe");
        assert_eq!(identified.source, GameSource::CustomFolder);

        engine.tick(t(50), &game);
        engine.tick(t(55), &game);
        assert_eq!(engine.suspend()[0].end, t(55));
    }
}

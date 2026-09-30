//! Everything the tracker does, independent of the tray window: polling, saving, settings, artwork and answering
//! the dashboard. Shared between the tray (main thread) and pipe clients behind a mutex; slow work (artwork
//! downloads, catalog rebuilds) runs on worker threads without holding it.

use crate::updater::UpdateState;
use playtime_artwork::cache::ArtworkCache;
use playtime_artwork::providers::{SteamCdnProvider, SteamGridDbProvider, SteamLocalProvider};
use playtime_artwork::service::{ArtworkService, MAX_CHOICES};
use playtime_artwork::{ArtworkKind, ArtworkProvider, ArtworkRequest};
use playtime_core::catalog::GameCatalog;
use playtime_core::dashboard::DashboardModel;
use playtime_core::discovery;
use playtime_core::engine::{Engine, ProcessInfo};
use playtime_core::ipc::{
    ArtworkChoiceInfo, DashboardSnapshot, Event, GameIdentity, Request, Response, PROTOCOL_VERSION,
};
use playtime_core::launchers::GameId;
use playtime_core::migration;
use playtime_core::protected::{self, Purpose};
use playtime_core::reports::{self, format_duration};
use playtime_core::{paths, SessionRecord, Settings, Timestamp};
use playtime_windows::discovery::WindowsHost;
use playtime_windows::dpapi::Dpapi;
use playtime_windows::http::WinHttpClient;
use playtime_windows::icons::ExeIconProvider;
use playtime_windows::registry::RegistryGenerations;
use playtime_windows::store::{self, Store};
use playtime_windows::{credentials, folders, processes, startup};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Things the update thread is asked to do right away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCommand {
    CheckNow,
    InstallNow,
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Subscribers to change events (pipe connections that sent `subscribe`).
#[derive(Clone, Default)]
pub struct EventHub(Arc<Mutex<Vec<Sender<Event>>>>);

impl EventHub {
    pub fn subscribe(&self) -> Receiver<Event> {
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut subscribers) = self.0.lock() {
            subscribers.push(sender);
        }
        receiver
    }

    pub fn publish(&self, event: Event) {
        if let Ok(mut subscribers) = self.0.lock() {
            subscribers.retain(|s| s.send(event.clone()).is_ok());
        }
    }
}

/// A tray notification to show.
pub struct Notification {
    pub title: String,
    pub body: String,
    pub warning: bool,
}

/// Artwork to look up on the worker thread.
struct ArtworkJob {
    service: Arc<ArtworkService>,
    request: ArtworkRequest,
    kind: ArtworkKind,
    game: String,
}

pub struct Service {
    engine: Engine,
    /// `None` if the data folder couldn't be opened: tracking continues in memory but nothing is written, so an
    /// unreadable history is never replaced.
    store: Option<Store>,
    data_folder: PathBuf,
    processes: Vec<ProcessInfo>,
    artwork: Arc<ArtworkService>,
    artwork_jobs: Sender<ArtworkJob>,
    events: EventHub,
    notifications: Vec<Notification>,
    shut_down: bool,
    /// Only the installed copy manages "Start with Windows" and installs updates.
    installed: bool,
    pub update: UpdateState,
    update_commands: Option<Sender<UpdateCommand>>,
}

fn build_catalog(settings: &Settings) -> (GameCatalog, Vec<String>) {
    let found = discovery::discover(&WindowsHost, settings);
    let issues = found
        .issues
        .into_iter()
        .map(|i| format!("Could not read games from {}: {}", i.source, i.message))
        .collect();
    (found.builder.build(settings), issues)
}

fn build_artwork(online: bool) -> Arc<ArtworkService> {
    let cache_root = folders::local_app_data()
        .map(|p| folders::artwork_cache(&p))
        .unwrap_or_else(|| std::env::temp_dir().join("Playtime Tracker Artwork"));
    let mut providers: Vec<Arc<dyn ArtworkProvider>> = Vec::new();
    if let Some(steam) = discovery::steam_install_dir(&WindowsHost) {
        providers.push(Arc::new(SteamLocalProvider::new(steam)));
    }
    providers.push(Arc::new(ExeIconProvider));
    if let Ok(http) = WinHttpClient::new(&format!("PlayLedger/{VERSION}")) {
        let http: Arc<WinHttpClient> = Arc::new(http);
        providers.push(Arc::new(SteamCdnProvider::new(http.clone())));
        if let Some(key) = credentials::read(credentials::STEAMGRIDDB_TARGET) {
            if let Some(provider) = SteamGridDbProvider::new(http, key.expose()) {
                providers.push(Arc::new(provider));
            }
        }
    }
    // The user's own choices live beside the automatic cache, so clearing the cache never loses them.
    let overrides = cache_root.with_file_name("Artwork Choices");
    let choices = cache_root.with_file_name("Artwork Previews");
    let service = ArtworkService::new(ArtworkCache::new(cache_root), providers)
        .with_overrides(ArtworkCache::new(overrides), choices);
    service.set_online_allowed(online);
    Arc::new(service)
}

fn artwork_kind(kind: &str) -> Option<ArtworkKind> {
    ArtworkKind::ALL.into_iter().find(|k| k.as_str() == kind)
}

impl Service {
    /// Opens the data folder and starts tracking. Returns warnings to show the user.
    pub fn start(events: EventHub) -> (Self, Vec<Notification>) {
        let now = Timestamp::now();
        let mut notifications = Vec::new();

        // One-time migrations first (folder move, 1.x import, verified backup before a new version), so the store
        // only ever opens data in the current format.
        let documents = folders::documents().unwrap_or_else(|| PathBuf::from("."));
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let report = migration::run(
            &Dpapi,
            &RegistryGenerations,
            &documents,
            VERSION,
            |folder| {
                let (json, _) = protected::read_file(
                    &Dpapi,
                    Purpose::Settings,
                    &folder.join(folders::SETTINGS_FILE),
                )
                .ok()?;
                Settings::from_json(&json).ok().map(|s| s.last_run_version)
            },
            &stamp,
        );
        let data_folder = report.data_folder.clone();
        if let Err(e) = migration::write_log(&data_folder, &stamp, &report.log) {
            store::log_to(
                &data_folder,
                &format!("Could not write the migration log: {e}"),
            );
        }
        for notice in report.notices {
            notifications.push(Notification {
                title: "PlayLedger".into(),
                body: notice,
                warning: true,
            });
        }

        let (store, data, settings) = match Store::open(&data_folder) {
            Ok(opened) => {
                for warning in opened.warnings {
                    notifications.push(Notification {
                        title: "PlayLedger".into(),
                        body: warning,
                        warning: true,
                    });
                }
                let mut store = opened.store;
                let mut settings = opened.settings;
                // Remember which version ran, as earlier versions did (the next version change triggers a backup).
                if settings.last_run_version != VERSION {
                    if !settings.last_run_version.is_empty() {
                        notifications.push(Notification {
                            title: "PlayLedger updated".into(),
                            body: format!(
                                "You're now on version {VERSION} (was {}).",
                                settings.last_run_version
                            ),
                            warning: false,
                        });
                    }
                    settings.last_run_version = VERSION.into();
                    if let Err(e) = store.save_settings(&settings) {
                        store.log(&format!(
                            "Could not record the version in the settings: {e}"
                        ));
                    }
                }
                (Some(store), opened.data, settings)
            }
            Err(e) => {
                store::log_to(
                    &data_folder,
                    &format!("Could not open the data folder: {e}"),
                );
                notifications.push(Notification {
                    title: "PlayLedger history".into(),
                    body: "Your play history couldn't be opened, so this session won't be saved (your existing \
                           history is untouched). Restart PlayLedger to try again; details are in errors.log."
                        .into(),
                    warning: true,
                });
                (None, Default::default(), Settings::default())
            }
        };

        // "Start with Windows", as before: on by default the first time, the pre-rename entry moved over, and
        // the entry pointed at this exe. Only for the installed copy, so a copy run from elsewhere never takes over.
        let installed = startup::is_installed_copy();
        let (mut store, mut settings) = (store, settings);
        if installed {
            startup::migrate_legacy_entry();
            if !settings.startup_configured {
                startup::set_enabled(true);
                settings.startup_configured = true;
                if let Some(store) = store.as_mut() {
                    if let Err(e) = store.save_settings(&settings) {
                        store.log(&format!("Could not save the settings: {e}"));
                    }
                }
            }
            startup::refresh_path_if_enabled();
        }

        let (catalog, issues) = build_catalog(&settings);
        for issue in issues {
            store::log_to(&data_folder, &issue);
        }
        let artwork = build_artwork(settings.online_artwork);
        let (jobs, receiver) = mpsc::channel::<ArtworkJob>();
        let hub = events.clone();
        std::thread::Builder::new()
            .name("artwork".into())
            .spawn(move || artwork_worker(receiver, hub))
            .ok();

        let mut service = Self {
            engine: Engine::new(data, settings, catalog, now),
            store,
            data_folder,
            processes: Vec::new(),
            artwork,
            artwork_jobs: jobs,
            events,
            notifications,
            shut_down: false,
            installed,
            update: UpdateState::default(),
            update_commands: None,
        };
        service.save(now);
        let notifications = std::mem::take(&mut service.notifications);
        (service, notifications)
    }

    pub fn set_update_commands(&mut self, commands: Sender<UpdateCommand>) {
        self.update_commands = Some(commands);
    }

    pub fn settings_snapshot(&self) -> Settings {
        self.engine.settings().clone()
    }

    pub fn is_installed(&self) -> bool {
        self.installed
    }

    /// True if any game is running (automatic updates wait until none is).
    pub fn is_playing(&self) -> bool {
        !self.engine.data().active.is_empty()
    }

    pub fn queue_notification(&mut self, notification: Notification) {
        self.notifications.push(notification);
    }

    pub fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.notifications)
    }

    pub fn publish(&self, event: Event) {
        self.events.publish(event);
    }

    pub fn request_update(&self, command: UpdateCommand) {
        if let Some(commands) = &self.update_commands {
            let _ = commands.send(command);
        }
    }

    fn update_status(&self) -> Response {
        let available = self.update.available.as_ref();
        Response::UpdateStatus {
            current_version: VERSION.into(),
            available_version: available.map(|u| u.version.to_string()),
            can_install: self.installed && available.is_some_and(|u| u.can_install()),
            busy: self.update.busy,
            last_error: self.update.last_error.clone(),
            last_checked: self.update.last_checked.map(|t| t.to_rfc3339()),
        }
    }

    pub fn data_folder(&self) -> &PathBuf {
        &self.data_folder
    }

    pub fn poll_interval_ms(&self) -> u32 {
        u32::try_from(self.engine.settings().poll_interval_seconds.clamp(1, 300)).unwrap_or(5)
            * 1000
    }

    fn log(&self, message: &str) {
        store::log_to(&self.data_folder, message);
    }

    fn save(&mut self, now: Timestamp) {
        let Some(store) = self.store.as_mut() else {
            return;
        };
        match store.save_sessions(self.engine.data(), now) {
            Ok(()) => self.engine.saved(now),
            Err(e) => store.log(&format!("Could not save the play history: {e}")),
        }
    }

    /// One poll. Returns notifications to show (sessions logged).
    pub fn tick(&mut self) -> Vec<Notification> {
        if self.shut_down {
            return Vec::new();
        }
        let now = Timestamp::now();
        if self.engine.catalog_due(now) {
            let (catalog, issues) = build_catalog(self.engine.settings());
            for issue in issues {
                self.log(&issue);
            }
            self.engine.set_catalog(catalog, now);
        }
        self.processes = processes::snapshot(&self.processes);
        let outcome = self.engine.tick(now, &self.processes);
        if outcome.save {
            self.save(now);
        }
        for game in &outcome.changes.started {
            self.events
                .publish(Event::SessionStarted { game: game.clone() });
        }
        let ended: Vec<SessionRecord> = outcome
            .slept
            .into_iter()
            .chain(outcome.changes.ended)
            .collect();
        let changed = !ended.is_empty() || !outcome.changes.started.is_empty();
        let mut notifications = Vec::new();
        for session in ended {
            if self.engine.settings().show_notifications {
                notifications.push(Notification {
                    title: "Session logged".into(),
                    body: format!("{}: {}", session.game, format_duration(session.duration())),
                    warning: false,
                });
            }
            self.events.publish(Event::SessionEnded { session });
        }
        if changed {
            self.publish_changed();
        }
        notifications
    }

    fn publish_changed(&self) {
        self.events.publish(Event::DataChanged {
            revision: self.engine.revision(),
        });
    }

    /// "Playing: Hades (1h 02m)" or "Not playing anything", for the tray tooltip and menu.
    pub fn status(&self) -> String {
        let now = Timestamp::now();
        let active = &self.engine.data().active;
        if active.is_empty() {
            return "Not playing anything".into();
        }
        let mut running: Vec<_> = active.iter().collect();
        running.sort_by_key(|a| a.start);
        let list: Vec<String> = running
            .iter()
            .map(|a| format!("{} ({})", a.game, format_duration(now.since(a.start))))
            .collect();
        format!("Playing: {}", list.join(", "))
    }

    /// The PC is going to sleep.
    pub fn suspend(&mut self) {
        let ended = self.engine.suspend();
        self.save(Timestamp::now());
        if !ended.is_empty() {
            self.publish_changed();
        }
    }

    /// Exiting or Windows is shutting down: end sessions now and save.
    pub fn shutdown(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        let now = Timestamp::now();
        self.engine.shutdown(now);
        self.save(now);
    }

    fn snapshot(&self) -> DashboardSnapshot {
        let data = self.engine.data();
        let now = Timestamp::now();
        let model = DashboardModel::new(&data.sessions, &data.active, now);
        let tz = chrono::Local;
        let today_start = now
            .as_datetime()
            .with_timezone(&tz)
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|midnight| midnight.and_local_timezone(tz).earliest())
            .map(|t| Timestamp::from_datetime(t.fixed_offset()));
        let past_week_seconds = today_start
            .and_then(|t| t.checked_add(chrono::Duration::days(-6)))
            .map(|since| model.total_since(None, since).num_seconds())
            .unwrap_or(0);
        let identities = model
            .games
            .iter()
            .filter_map(|g| {
                let m = self.engine.identity(&g.name)?;
                Some(GameIdentity {
                    game: g.name.clone(),
                    source: m.source.display_name().into(),
                    artwork_key: m.id.as_ref().map(GameId::cache_key),
                })
            })
            .collect();
        DashboardSnapshot {
            revision: self.engine.revision(),
            daily: model.daily_totals(None, &tz),
            history: model.history(&tz),
            past_week_seconds,
            identities,
            model,
        }
    }

    fn update_settings(&mut self, mut settings: Settings) -> Response {
        settings.normalize();
        // Fields the dashboard doesn't own keep their current values.
        let current = self.engine.settings();
        settings.last_run_version = current.last_run_version.clone();
        settings.settings_version = current.settings_version;
        settings.startup_configured = current.startup_configured;

        let detection_changed = settings.use_windows_game_list != current.use_windows_game_list
            || settings.extra_game_folders != current.extra_game_folders
            || settings.custom_games != current.custom_games
            || settings.ignored_games != current.ignored_games
            || settings.ignored_executables != current.ignored_executables;
        if let Some(store) = self.store.as_mut() {
            if let Err(e) = store.save_settings(&settings) {
                return Response::Error {
                    message: format!("Your settings couldn't be saved: {e}"),
                };
            }
        }
        self.artwork.set_online_allowed(settings.online_artwork);
        let now = Timestamp::now();
        if detection_changed {
            let (catalog, _) = build_catalog(&settings);
            self.engine.set_catalog(catalog, now);
        }
        self.engine.set_settings(settings);
        self.publish_changed();
        Response::Ok
    }

    /// What the artwork service needs to know about `game`, or the response to send if it can't be looked up.
    fn artwork_target(
        &mut self,
        game: &str,
        kind: &str,
    ) -> Result<(ArtworkRequest, ArtworkKind), Response> {
        let Some(kind) = artwork_kind(kind) else {
            return Err(Response::Error {
                message: format!("unknown artwork kind {kind:?}"),
            });
        };
        let exe_path = self.engine.executable_of(game);
        let Some(found) = self.engine.identify(game) else {
            return Err(Response::Error {
                message: format!("{game} isn't a known game"),
            });
        };
        let id = found.id.unwrap_or_else(|| GameId::Path {
            normalized: paths::key(exe_path.as_deref().unwrap_or(game)),
        });
        Ok((
            ArtworkRequest {
                id,
                name: game.to_string(),
                exe_path,
            },
            kind,
        ))
    }

    /// For requests that go online (listing and applying SteamGridDB choices): everything they need, taken under
    /// the service lock so the slow part can run without it (see `pipe.rs`).
    pub fn artwork_job(&mut self, game: &str, kind: &str) -> Result<ArtworkTask, Response> {
        let (request, kind) = self.artwork_target(game, kind)?;
        Ok(ArtworkTask {
            service: self.artwork.clone(),
            request,
            kind,
            game: game.to_string(),
            events: self.events.clone(),
        })
    }

    fn artwork_changed(&self, game: &str, kind: ArtworkKind) {
        self.events.publish(Event::ArtworkReady {
            game: game.to_string(),
            kind: kind.as_str().into(),
        });
    }

    fn artwork(&mut self, game: &str, kind: &str) -> Response {
        let (request, kind) = match self.artwork_target(game, kind) {
            Ok(target) => target,
            Err(Response::Error { .. }) if artwork_kind(kind).is_some() => {
                return Response::Artwork { path: None }
            }
            Err(response) => return response,
        };
        if let Some(cached) = self.artwork.cached(&request, kind) {
            return Response::Artwork {
                path: Some(cached.path.to_string_lossy().into_owned()),
            };
        }
        let _ = self.artwork_jobs.send(ArtworkJob {
            service: self.artwork.clone(),
            request,
            kind,
            game: game.to_string(),
        });
        Response::Artwork { path: None }
    }

    fn set_ignored(&mut self, game: &str, ignored: bool) -> Response {
        let mut settings = self.engine.settings().clone();
        let key = paths::key(game);
        settings.ignored_games.retain(|g| paths::key(g) != key);
        if ignored {
            settings.ignored_games.push(game.to_string());
        }
        self.update_settings(settings)
    }

    pub fn handle(&mut self, request: Request) -> Response {
        match request {
            Request::Hello { .. } => Response::Hello {
                protocol: PROTOCOL_VERSION,
                version: VERSION.into(),
            },
            Request::GetDashboard => Response::Dashboard {
                snapshot: Box::new(self.snapshot()),
            },
            Request::GetSettings => Response::Settings {
                settings: Box::new(self.engine.settings().clone()),
                has_steam_grid_db_key: credentials::read(credentials::STEAMGRIDDB_TARGET).is_some(),
                start_with_windows: startup::is_enabled(),
                is_installed_copy: self.installed,
            },
            Request::UpdateSettings { settings } => self.update_settings(*settings),
            Request::DeleteSession { game, start } => {
                if self.engine.delete_session(&game, start) {
                    self.save(Timestamp::now());
                    self.publish_changed();
                    Response::Ok
                } else {
                    Response::Error {
                        message: "That session wasn't found (it may still be running).".into(),
                    }
                }
            }
            Request::DeleteGameHistory { game } => {
                self.engine.delete_game_history(&game);
                self.save(Timestamp::now());
                self.publish_changed();
                Response::Ok
            }
            Request::SetGameIgnored { game, ignored } => self.set_ignored(&game, ignored),
            Request::RescanGames => {
                let (catalog, issues) = build_catalog(self.engine.settings());
                for issue in issues {
                    self.log(&issue);
                }
                self.engine.set_catalog(catalog, Timestamp::now());
                Response::Ok
            }
            Request::GetArtwork { game, kind } => self.artwork(&game, &kind),
            Request::SetArtworkFromFile { game, kind, path } => {
                let (request, kind) = match self.artwork_target(&game, &kind) {
                    Ok(target) => target,
                    Err(response) => return response,
                };
                match self.artwork.set_override_from_file(
                    &request,
                    kind,
                    std::path::Path::new(&path),
                ) {
                    Ok(_) => {
                        self.artwork_changed(&game, kind);
                        Response::Ok
                    }
                    Err(e) => Response::Error {
                        message: format!("That picture can't be used: {e}"),
                    },
                }
            }
            Request::ResetArtwork { game, kind } => {
                let (request, kind) = match self.artwork_target(&game, &kind) {
                    Ok(target) => target,
                    Err(response) => return response,
                };
                match self.artwork.clear_override(&request, kind) {
                    Ok(()) => {
                        self.artwork_changed(&game, kind);
                        Response::Ok
                    }
                    Err(e) => Response::Error {
                        message: format!("The artwork couldn't be reset: {e}"),
                    },
                }
            }
            // Online, so run by the pipe outside the service lock (`artwork_job`); only reached if called directly.
            Request::ListArtworkChoices { game, kind } => match self.artwork_job(&game, &kind) {
                Ok(task) => task.list_choices(),
                Err(response) => response,
            },
            Request::ApplyArtworkChoice { game, kind, index } => {
                match self.artwork_job(&game, &kind) {
                    Ok(task) => task.apply_choice(index),
                    Err(response) => response,
                }
            }
            Request::ExportCsv => {
                let now = Timestamp::now();
                let offset = *now.as_datetime().with_timezone(&chrono::Local).offset();
                Response::Csv {
                    text: reports::sessions_csv(&self.engine.data().sessions, offset),
                }
            }
            Request::SetSteamGridDbKey { key } => {
                match key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
                    Some(key)
                        if !playtime_artwork::providers::steamgriddb::is_plausible_key(key) =>
                    {
                        return Response::Error {
                            message: "That doesn't look like a SteamGridDB API key (32 letters and digits).".into(),
                        };
                    }
                    Some(key) => {
                        if credentials::write(credentials::STEAMGRIDDB_TARGET, key).is_err() {
                            return Response::Error {
                                message: "The key couldn't be saved to Windows Credential Manager."
                                    .into(),
                            };
                        }
                    }
                    None => credentials::delete(credentials::STEAMGRIDDB_TARGET),
                }
                self.artwork = build_artwork(self.engine.settings().online_artwork);
                Response::Ok
            }
            Request::CheckForUpdates => {
                self.request_update(UpdateCommand::CheckNow);
                self.update_status()
            }
            Request::GetUpdateStatus => self.update_status(),
            Request::InstallUpdate => {
                if !self.installed {
                    return Response::Error {
                        message: "Only an installed copy of PlayLedger updates itself.".into(),
                    };
                }
                self.request_update(UpdateCommand::InstallNow);
                self.update_status()
            }
            Request::SetStartWithWindows { enabled } => {
                if !self.installed {
                    return Response::Error {
                        message: "Only an installed copy of PlayLedger can start with Windows."
                            .into(),
                    };
                }
                if startup::set_enabled(enabled) {
                    Response::Ok
                } else {
                    Response::Error {
                        message: "Windows didn't accept the change.".into(),
                    }
                }
            }
            // Handled by the pipe connection itself.
            Request::Subscribe => Response::Ok,
        }
    }
}

/// An artwork request that goes online, prepared under the service lock and run without it.
pub struct ArtworkTask {
    service: Arc<ArtworkService>,
    request: ArtworkRequest,
    kind: ArtworkKind,
    game: String,
    events: EventHub,
}

impl ArtworkTask {
    pub fn list_choices(&self) -> Response {
        match self.service.choices(&self.request, self.kind, MAX_CHOICES) {
            Ok(previews) => Response::ArtworkChoices {
                choices: previews
                    .into_iter()
                    .map(|p| ArtworkChoiceInfo {
                        index: p.index,
                        path: p.path.to_string_lossy().into_owned(),
                        width: p.width,
                        height: p.height,
                    })
                    .collect(),
            },
            Err(e) => Response::Error {
                message: format!("Pictures couldn't be listed: {e}"),
            },
        }
    }

    pub fn apply_choice(&self, index: usize) -> Response {
        match self.service.apply_choice(&self.request, self.kind, index) {
            Ok(_) => {
                self.events.publish(Event::ArtworkReady {
                    game: self.game.clone(),
                    kind: self.kind.as_str().into(),
                });
                Response::Ok
            }
            Err(e) => Response::Error {
                message: format!("That picture couldn't be used: {e}"),
            },
        }
    }
}

fn artwork_worker(jobs: Receiver<ArtworkJob>, events: EventHub) {
    for job in jobs {
        let lookup = job.service.get(&job.request, job.kind);
        if lookup.image.is_some() {
            events.publish(Event::ArtworkReady {
                game: job.game,
                kind: job.kind.as_str().into(),
            });
        }
        // Issues are expected (no artwork, offline); they aren't logged to keep errors.log meaningful.
        let _ = lookup.issues;
    }
}

//! Updates from this repository's GitHub releases, with these safety rules:
//! - only the latest published (non-draft, non-prerelease) plain `vX.Y.Z` release newer than this version;
//! - the installer (`Setup.exe`) comes from this repository's release download URL over HTTPS, following redirects
//!   only to GitHub's own hosts, and must match the size and SHA-256 digest GitHub recorded when it was uploaded;
//! - it only ever runs for the installed copy, silently (`/S /relaunch`), and automatic installs wait until no game
//!   is running. Nothing is sent except a normal request for the latest release.

use crate::service::{Notification, Service, UpdateCommand};
use playtime_artwork::http::{HttpError, Request};
use playtime_core::ipc::Event;
use playtime_core::updates::{self, UpdateInfo, Version};
use playtime_windows::http::{github_policy, WinHttpClient};
use playtime_windows::startup;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Checked a minute after start, then every 6 hours.
pub const FIRST_CHECK: Duration = Duration::from_secs(60);
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_API_BYTES: usize = 2 * 1024 * 1024;
const MAX_INSTALLER_BYTES: u64 = 500 * 1024 * 1024;

/// Where the updater stands, for the dashboard's Settings page.
#[derive(Debug, Clone, Default)]
pub struct UpdateState {
    pub available: Option<UpdateInfo>,
    pub last_checked: Option<chrono::DateTime<chrono::Local>>,
    pub last_error: Option<String>,
    pub busy: bool,
    /// The release the user has already been told about (so it's announced once).
    pub announced: Option<Version>,
}

pub fn current_version() -> Version {
    Version::parse_tag(crate::service::VERSION).unwrap_or(Version::new(0, 0, 0))
}

fn client() -> Result<WinHttpClient, HttpError> {
    WinHttpClient::with_policy(
        &format!("PlaytimeTracker/{}", crate::service::VERSION),
        github_policy,
    )
}

/// Asks GitHub for the latest release. `Ok(None)` if there's nothing newer (or no release yet).
pub fn check() -> Result<Option<UpdateInfo>, String> {
    let request = Request::get(updates::latest_release_api())
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    let mut body = Vec::new();
    let head = client()
        .and_then(|c| {
            c.get_streaming(&request, &mut |chunk| {
                if body.len() + chunk.len() > MAX_API_BYTES {
                    return Err(HttpError::TooLarge);
                }
                body.extend_from_slice(chunk);
                Ok(())
            })
        })
        .map_err(|_| "Couldn't reach GitHub to check for updates.".to_string())?;
    match head.status {
        404 => Ok(None), // no published release yet
        200 => {
            let text = String::from_utf8(body)
                .map_err(|_| "GitHub's answer wasn't readable.".to_string())?;
            updates::parse_latest_release(&text, current_version())
                .map_err(|_| "GitHub's answer wasn't readable.".to_string())
        }
        status => Err(format!("GitHub answered {status}; will try again later.")),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads and verifies the installer, then starts it silently; it closes the tracker (`--exit`), updates it in
/// place and starts the new version. Returns an error message for the user if anything doesn't check out.
pub fn download_and_start(update: &UpdateInfo) -> Result<(), String> {
    let Some(asset) = update.installer.as_ref().filter(|_| update.can_install()) else {
        return Err(
            "This release can't be installed from inside the app; download it from GitHub.".into(),
        );
    };
    if !startup::is_installed_copy() {
        return Err("Only an installed copy updates itself.".into());
    }
    if asset.size == 0 || asset.size > MAX_INSTALLER_BYTES {
        return Err("The update's size isn't plausible, so it wasn't installed.".into());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let folder = std::env::temp_dir().join(format!(
        "PlaytimeTracker-update-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&folder)
        .map_err(|_| "Couldn't prepare the update download.".to_string())?;
    let installer = folder.join(updates::INSTALLER_ASSET);
    let result = download(asset, &installer).and_then(|()| {
        std::process::Command::new(&installer)
            .args(["/S", "/relaunch"])
            .current_dir(&folder)
            .spawn()
            .map(|_| ())
            .map_err(|_| "The update couldn't be started.".to_string())
    });
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&folder);
    }
    result
}

fn download(asset: &updates::InstallerAsset, destination: &PathBuf) -> Result<(), String> {
    if !updates::is_trusted_download_url(&asset.url) {
        return Err(
            "The update's address isn't this project's GitHub release, so it wasn't installed."
                .into(),
        );
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| "Couldn't save the update.".to_string())?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let head = client()
        .and_then(|c| {
            c.get_streaming(&Request::get(asset.url.clone()), &mut |chunk| {
                total += chunk.len() as u64;
                if total > asset.size {
                    return Err(HttpError::TooLarge);
                }
                hasher.update(chunk);
                file.write_all(chunk)
                    .map_err(|e| HttpError::Transport(e.to_string()))
            })
        })
        .map_err(|e| match e {
            HttpError::TooLarge => {
                "The update download was larger than expected, so it wasn't installed.".to_string()
            }
            HttpError::NotAllowed(_) => {
                "The update was served from an unexpected address, so it wasn't installed."
                    .to_string()
            }
            HttpError::Transport(_) => {
                "The update couldn't be downloaded. Try again later.".to_string()
            }
        })?;
    file.flush()
        .map_err(|_| "Couldn't save the update.".to_string())?;
    drop(file);
    if head.status != 200 {
        return Err("The update couldn't be downloaded. Try again later.".into());
    }
    if total != asset.size {
        return Err("The update download was incomplete, so it wasn't installed.".into());
    }
    if !hex(&hasher.finalize()).eq_ignore_ascii_case(&asset.sha256) {
        return Err("The update didn't match GitHub's checksum, so it wasn't installed.".into());
    }
    Ok(())
}

/// While an automatic install waits for games to close, look again this often.
const RETRY_WHILE_PLAYING: Duration = Duration::from_secs(5 * 60);

/// Starts the update thread: scheduled checks, plus "check now" / "install now" from the dashboard or tray.
pub fn spawn(service: Arc<Mutex<Service>>) {
    let (commands, receiver) = mpsc::channel();
    if let Ok(mut s) = service.lock() {
        s.set_update_commands(commands);
    }
    let _ = std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || run(&service, &receiver));
}

fn run(service: &Mutex<Service>, commands: &Receiver<UpdateCommand>) {
    let mut next_check = Instant::now() + FIRST_CHECK;
    let mut retry_install: Option<Instant> = None;
    loop {
        let due = retry_install.map_or(next_check, |r| r.min(next_check));
        let command = match commands.recv_timeout(due.saturating_duration_since(Instant::now())) {
            Ok(command) => Some(command),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let Ok((enabled, automatic)) = service.lock().map(|s| {
            let settings = s.settings_snapshot();
            (
                settings.check_for_updates,
                settings.install_updates_automatically,
            )
        }) else {
            return;
        };
        match command {
            Some(UpdateCommand::InstallNow) => install(service),
            Some(UpdateCommand::CheckNow) => check_and_announce(service, automatic),
            None if retry_install.is_some_and(|r| r <= Instant::now()) => retry_install = None,
            None => {
                next_check = Instant::now() + CHECK_INTERVAL;
                if !enabled {
                    continue;
                }
                check_and_announce(service, automatic);
            }
        }
        if automatic && enabled && command != Some(UpdateCommand::InstallNow) {
            retry_install = match auto_install_ready(service) {
                AutoInstall::Ready => {
                    install(service);
                    None
                }
                AutoInstall::WaitForGames => Some(Instant::now() + RETRY_WHILE_PLAYING),
                AutoInstall::Nothing => None,
            };
        }
    }
}

enum AutoInstall {
    Ready,
    WaitForGames,
    Nothing,
}

fn auto_install_ready(service: &Mutex<Service>) -> AutoInstall {
    let Ok(s) = service.lock() else {
        return AutoInstall::Nothing;
    };
    let installable = s.is_installed()
        && !s.update.busy
        && s.update
            .available
            .as_ref()
            .is_some_and(UpdateInfo::can_install);
    match (installable, s.is_playing()) {
        (false, _) => AutoInstall::Nothing,
        (true, true) => AutoInstall::WaitForGames, // never in the middle of a session
        (true, false) => AutoInstall::Ready,
    }
}

fn check_and_announce(service: &Mutex<Service>, automatic: bool) {
    match service.lock() {
        Ok(mut s) if !s.update.busy => s.update.busy = true,
        _ => return,
    }
    let result = check();
    let Ok(mut s) = service.lock() else { return };
    s.update.busy = false;
    s.update.last_checked = Some(chrono::Local::now());
    match result {
        Ok(available) => {
            s.update.last_error = None;
            s.update.available = available;
        }
        Err(message) => s.update.last_error = Some(message),
    }
    let Some(update) = s.update.available.clone() else {
        return;
    };
    if s.update.announced == Some(update.version) {
        return;
    }
    s.update.announced = Some(update.version);
    let installed = s.is_installed();
    let body = if !installed || !update.can_install() {
        format!("Download it from {}", updates::releases_page())
    } else if automatic {
        "It will be installed when no game is running.".to_string()
    } else {
        "Install it from Settings → Updates in the dashboard.".to_string()
    };
    s.queue_notification(Notification {
        title: format!("Playtime Tracker {} is available", update.version),
        body,
        warning: false,
    });
    s.publish(Event::UpdateAvailable {
        version: update.version.to_string(),
    });
}

fn install(service: &Mutex<Service>) {
    let update = match service.lock() {
        Ok(mut s) if !s.update.busy && s.is_installed() => {
            let Some(update) = s.update.available.clone() else {
                return;
            };
            s.update.busy = true;
            update
        }
        _ => return,
    };
    let result = download_and_start(&update);
    let Ok(mut s) = service.lock() else { return };
    s.update.busy = false;
    match result {
        // The installer now closes the tracker (--exit), updates it and starts the new version.
        Ok(()) => s.update.last_error = None,
        Err(message) => {
            s.update.last_error = Some(message.clone());
            s.queue_notification(Notification {
                title: "Update not installed".into(),
                body: message,
                warning: true,
            });
        }
    }
}

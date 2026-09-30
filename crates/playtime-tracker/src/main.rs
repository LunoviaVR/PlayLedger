//! Playtime Tracker's background service. It owns the tray icon, polls for running games, keeps the protected
//! history and settings, finds artwork, and serves the dashboard over a named pipe. It uses the same
//! single-instance mutex as earlier versions, so an old and a new copy never run (and write the data folder) at the
//! same time.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod instance;
#[cfg(windows)]
mod pipe;
#[cfg(windows)]
mod service;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod updater;

#[cfg(windows)]
fn main() {
    std::process::exit(app::run(std::env::args().skip(1).collect()));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Playtime Tracker runs on Windows.");
    std::process::exit(1);
}

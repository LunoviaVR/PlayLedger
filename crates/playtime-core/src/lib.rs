//! Platform-independent core of Playtime Tracker.
//!
//! Everything here is plain Rust with no Windows calls, so it builds and is tested on any OS:
//! the domain model and its on-disk JSON shape (compatible with the C# app), session tracking,
//! game identification, launcher metadata parsing, settings, the protected-file format, reports
//! and update-release parsing. Windows specifics (DPAPI, registry, processes) live in
//! `playtime-windows` behind the traits defined here.

pub mod catalog;
pub mod dashboard;
pub mod discovery;
pub mod engine;
pub mod ipc;
pub mod launchers;
pub mod model;
pub mod paths;
pub mod protected;
pub mod reports;
pub mod settings;
pub mod time;
pub mod tracking;
pub mod updates;

pub use model::{ActiveSession, SessionRecord, TrackerData};
pub use settings::Settings;
pub use time::Timestamp;

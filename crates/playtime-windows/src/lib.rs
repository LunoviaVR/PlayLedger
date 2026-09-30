//! Windows integrations for Playtime Tracker, behind the platform-neutral traits in `playtime-core`.
//!
//! - [`dpapi::Dpapi`]: `DataProtector` backed by Windows DPAPI (current user), byte-compatible with the files earlier versions wrote
//!   `ProtectedData.Protect(..., DataProtectionScope.CurrentUser)`.
//! - [`registry::RegistryGenerations`]: `GenerationStore` in `HKCU\Software\Playtime Tracker\Integrity`, same values
//!   as earlier versions.
//! - [`locks::FileLocks`]: holds data files open read-only so other programs can't change them while the app runs.
//! - [`discovery::WindowsHost`]: the real files, registry and exe version info behind game discovery.
//! - [`http::WinHttpClient`]: HTTPS for online artwork (allow-listed hosts, system certificate validation).
//! - [`credentials`]: API keys in Windows Credential Manager.
//! - [`file_dialogs`]: the Windows Open and Save As dialogs.
//! - [`icons::ExeIconProvider`]: a game's own icon as artwork.
//! - [`processes`]: running processes and their exe paths.
//! - [`store::Store`]: the data folder (protected files, read-only reports, error log).
//! - [`startup`]: "Start with Windows" and whether this is the installed copy.
//! - [`folders`]: where the data lives.
//!
//! On other platforms the crate compiles to stubs so the workspace builds and tests everywhere.

pub mod folders;

#[cfg(windows)]
pub mod credentials;
#[cfg(windows)]
pub mod discovery;
#[cfg(windows)]
pub mod dpapi;
#[cfg(windows)]
pub mod file_dialogs;
#[cfg(windows)]
pub mod http;
#[cfg(windows)]
pub mod icons;
#[cfg(windows)]
pub mod locks;
#[cfg(windows)]
pub mod pipe;
#[cfg(windows)]
pub mod processes;
#[cfg(windows)]
mod reg;
#[cfg(windows)]
pub mod registry;
#[cfg(windows)]
pub mod startup;
#[cfg(windows)]
pub mod store;

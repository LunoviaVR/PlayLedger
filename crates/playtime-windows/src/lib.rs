//! Windows integrations for Playtime Tracker, behind the platform-neutral traits in `playtime-core`.
//!
//! - [`dpapi::Dpapi`]: `DataProtector` backed by Windows DPAPI (current user), byte-compatible with the C# app's
//!   `ProtectedData.Protect(..., DataProtectionScope.CurrentUser)`.
//! - [`registry::RegistryGenerations`]: `GenerationStore` in `HKCU\Software\Playtime Tracker\Integrity`, same values
//!   as the C# app.
//! - [`locks::FileLocks`]: holds data files open read-only so other programs can't change them while the app runs.
//! - [`discovery::WindowsHost`]: the real files, registry and exe version info behind game discovery.
//! - [`folders`]: where the data lives.
//!
//! On other platforms the crate compiles to stubs so the workspace builds and tests everywhere.

pub mod folders;

#[cfg(windows)]
pub mod discovery;
#[cfg(windows)]
pub mod dpapi;
#[cfg(windows)]
pub mod locks;
#[cfg(windows)]
mod reg;
#[cfg(windows)]
pub mod registry;

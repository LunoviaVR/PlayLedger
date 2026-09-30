//! End-to-end smoke test of an installed PlayLedger on a real Windows machine:
//!   playtime-smoke-test <install folder> [screenshot folder]
//! Starts the tracker, talks to it over its pipe exactly as the dashboard does, makes it track a real process,
//! checks the protected history on disk, restarts the tracker to check the history survives, and (optionally)
//! screenshots each dashboard page at a few sizes and checks the accent colour setting shows. Exits non-zero on the
//! first failure.

#[cfg(windows)]
mod windows_test;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    windows_test::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The smoke test runs on Windows only.");
}

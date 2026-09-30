//! Records unexpected errors in `%LocalAppData%\Playtime Tracker\dashboard-errors.log` (kept under 1 MB), so a
//! dashboard that fails before its window can show anything still leaves a trace. It holds no personal data: only
//! the error and where it happened.

use std::io::Write;
use std::path::PathBuf;

const MAX_BYTES: u64 = 1024 * 1024;

fn path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        playtime_windows::folders::local_app_data()
            .map(|d| d.join("Playtime Tracker").join("dashboard-errors.log"))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Adds a line to the log (and prints it, for a console).
pub fn write(context: &str, message: &str) {
    eprintln!("{context}: {message}");
    let Some(path) = path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(
            file,
            "{}  {context}: {message}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );
    }
}

/// Logs panics before the default report.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write("Unexpected error", &info.to_string());
        default(info);
    }));
}

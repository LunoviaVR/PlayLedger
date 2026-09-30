//! Where PlayLedger keeps its files (the same locations as earlier versions).

use std::path::PathBuf;

/// The data folder's name under Documents.
pub const DATA_FOLDER_NAME: &str = "Playtime Tracker";
/// The name it had before the rename (moved on first start by version 2.0).
pub const LEGACY_DATA_FOLDER_NAME: &str = "Game Session Tracker";

pub const SESSIONS_FILE: &str = "sessions.dat";
pub const SETTINGS_FILE: &str = "settings.dat";
pub const STATS_FILE: &str = "Game Stats.txt";
pub const CSV_FILE: &str = "Sessions.csv";
pub const ERROR_LOG_FILE: &str = "errors.log";

/// `Documents\Playtime Tracker`, from the user's Documents folder (`documents`).
pub fn data_folder(documents: &std::path::Path) -> PathBuf {
    documents.join(DATA_FOLDER_NAME)
}

/// `%LocalAppData%\Playtime Tracker\Cache\Artwork`: artwork is kept apart from the play history.
pub fn artwork_cache(local_app_data: &std::path::Path) -> PathBuf {
    local_app_data
        .join(DATA_FOLDER_NAME)
        .join("Cache")
        .join("Artwork")
}

/// The user's Documents folder (wherever they've moved it, e.g. into OneDrive).
#[cfg(windows)]
pub fn documents() -> Option<PathBuf> {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_Documents)
}

/// `%LocalAppData%`.
#[cfg(windows)]
pub fn local_app_data() -> Option<PathBuf> {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_LocalAppData)
}

#[cfg(windows)]
fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    // SAFETY: on success the shell allocates the string; we copy it and free it with CoTaskMemFree.
    unsafe {
        let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const _));
        path.map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn locations() {
        assert!(data_folder(Path::new("docs")).ends_with("Playtime Tracker"));
        assert!(artwork_cache(Path::new("local"))
            .ends_with(Path::new("Playtime Tracker").join("Cache").join("Artwork")));
    }
}

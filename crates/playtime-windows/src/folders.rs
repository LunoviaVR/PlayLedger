//! Where Playtime Tracker keeps its files (same locations as the C# app).

use std::path::PathBuf;

/// The data folder's name under Documents.
pub const DATA_FOLDER_NAME: &str = "Playtime Tracker";
/// The name it had before the rename (moved on first start by the C# app).
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

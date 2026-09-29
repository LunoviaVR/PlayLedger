//! Steam's own library artwork, already on disk in `<Steam>\appcache\librarycache` for every game the Steam client
//! has shown. Nothing leaves the PC.

use crate::{ArtworkError, ArtworkKind, ArtworkProvider, ArtworkRequest, FetchedImage};
use playtime_core::launchers::GameId;
use std::fs;
use std::path::{Path, PathBuf};

pub struct SteamLocalProvider {
    library_cache: PathBuf,
}

impl SteamLocalProvider {
    /// `steam_dir` is Steam's install folder (see `playtime_core::discovery::steam_install_dir`).
    pub fn new(steam_dir: impl AsRef<Path>) -> Self {
        Self {
            library_cache: steam_dir.as_ref().join("appcache").join("librarycache"),
        }
    }

    fn file_names(kind: ArtworkKind) -> &'static [&'static str] {
        match kind {
            ArtworkKind::Cover => &["library_600x900_2x.jpg", "library_600x900.jpg"],
            ArtworkKind::Header => &["header.jpg"],
            ArtworkKind::Hero => &["library_hero_2x.jpg", "library_hero.jpg"],
            ArtworkKind::Logo => &["logo_2x.png", "logo.png"],
            ArtworkKind::Icon => &["icon.jpg"],
        }
    }

    fn find(&self, app_id: u32, kind: ArtworkKind) -> Option<PathBuf> {
        for name in Self::file_names(kind) {
            // Older clients: librarycache\<appid>_<name>
            let flat = self.library_cache.join(format!("{app_id}_{name}"));
            if flat.is_file() {
                return Some(flat);
            }
            // Current clients: librarycache\<appid>\<name>, sometimes one hashed folder deeper.
            let folder = self.library_cache.join(app_id.to_string());
            let direct = folder.join(name);
            if direct.is_file() {
                return Some(direct);
            }
            if let Ok(entries) = fs::read_dir(&folder) {
                for entry in entries.flatten() {
                    let nested = entry.path().join(name);
                    if entry.file_type().is_ok_and(|t| t.is_dir()) && nested.is_file() {
                        return Some(nested);
                    }
                }
            }
        }
        None
    }
}

impl ArtworkProvider for SteamLocalProvider {
    fn name(&self) -> &'static str {
        "steam-local"
    }

    fn is_online(&self) -> bool {
        false
    }

    fn fetch(
        &self,
        request: &ArtworkRequest,
        kind: ArtworkKind,
    ) -> Result<Option<FetchedImage>, ArtworkError> {
        let GameId::Steam { app_id } = request.id else {
            return Ok(None);
        };
        let Some(path) = self.find(app_id, kind) else {
            return Ok(None);
        };
        let len = fs::metadata(&path)?.len();
        if len > crate::image::MAX_IMAGE_BYTES as u64 {
            return Ok(None);
        }
        Ok(Some(FetchedImage {
            bytes: fs::read(&path)?,
            content_type: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::test_dir::TempDir;
    use crate::image::samples;

    fn request(app_id: u32) -> ArtworkRequest {
        ArtworkRequest {
            id: GameId::Steam { app_id },
            name: "Game".into(),
            exe_path: None,
        }
    }

    #[test]
    fn finds_old_and_new_layouts() {
        let dir = TempDir::new("steam-local");
        let cache = dir.path().join("appcache").join("librarycache");
        fs::create_dir_all(cache.join("620").join("a1b2c3")).expect("dirs");
        fs::write(
            cache.join("620_library_600x900.jpg"),
            samples::jpeg(600, 900),
        )
        .expect("w");
        fs::write(
            cache.join("620").join("a1b2c3").join("logo.png"),
            samples::png(640, 360),
        )
        .expect("w");
        let provider = SteamLocalProvider::new(dir.path());

        let cover = provider
            .fetch(&request(620), ArtworkKind::Cover)
            .expect("ok");
        assert_eq!(cover.map(|c| c.bytes), Some(samples::jpeg(600, 900)));
        assert!(provider
            .fetch(&request(620), ArtworkKind::Logo)
            .expect("ok")
            .is_some());
        assert!(provider
            .fetch(&request(620), ArtworkKind::Hero)
            .expect("ok")
            .is_none());
        assert!(provider
            .fetch(&request(999), ArtworkKind::Cover)
            .expect("ok")
            .is_none());

        let not_steam = ArtworkRequest {
            id: GameId::BuiltIn { key: "roblox" },
            ..request(620)
        };
        assert!(provider
            .fetch(&not_steam, ArtworkKind::Cover)
            .expect("ok")
            .is_none());
    }
}

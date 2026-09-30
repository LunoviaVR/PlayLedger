//! Artwork on disk: `<root>\<game cache key>\<kind>.<ext>` plus a small `meta.json` per game recording where each
//! image came from and which lookups found nothing (so they aren't repeated on every start).
//!
//! The cache holds nothing private (only pictures and where they came from) and is kept apart from the play
//! history; deleting it only means artwork is fetched again.

use crate::image::{self, ImageFormat, ImageInfo};
use crate::ArtworkKind;
use playtime_core::launchers::GameId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// After a local lookup found nothing, try again after a day (the game may have been installed through Steam since).
pub const LOCAL_MISS_SECONDS: u64 = 24 * 60 * 60;
/// After an online lookup found nothing, wait a week before asking again.
pub const ONLINE_MISS_SECONDS: u64 = 7 * 24 * 60 * 60;

const META_FILE: &str = "meta.json";
/// meta.json is tiny; refuse to parse anything big.
const MAX_META_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Meta {
    entries: BTreeMap<String, Entry>,
    misses: BTreeMap<String, Miss>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    source: String,
    ext: String,
    width: u32,
    height: u32,
    stored_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Miss {
    local_at: Option<u64>,
    online_at: Option<u64>,
}

/// A cached image ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedImage {
    pub path: PathBuf,
    pub source: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct ArtworkCache {
    root: PathBuf,
}

impl ArtworkCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn game_dir(&self, id: &GameId) -> PathBuf {
        self.root.join(id.cache_key())
    }

    fn read_meta(&self, id: &GameId) -> Meta {
        let path = self.game_dir(id).join(META_FILE);
        let Ok(file) = fs::metadata(&path) else {
            return Meta::default();
        };
        if file.len() > MAX_META_BYTES {
            return Meta::default();
        }
        fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write_meta(&self, id: &GameId, meta: &Meta) -> io::Result<()> {
        let dir = self.game_dir(id);
        fs::create_dir_all(&dir)?;
        let json = serde_json::to_vec_pretty(meta).map_err(io::Error::other)?;
        write_atomic(&dir.join(META_FILE), &json)
    }

    /// The cached image for `kind`, if there is one on disk.
    pub fn get(&self, id: &GameId, kind: ArtworkKind) -> Option<CachedImage> {
        let meta = self.read_meta(id);
        let entry = meta.entries.get(kind.as_str())?;
        // Only extensions we write ourselves; meta.json can't point anywhere else.
        let format = ImageFormat::from_extension(&entry.ext)?;
        let path = self
            .game_dir(id)
            .join(format!("{}.{}", kind.as_str(), format.extension()));
        let size = fs::metadata(&path).ok()?.len();
        (size > 0 && size <= image::MAX_IMAGE_BYTES as u64).then(|| CachedImage {
            path,
            source: entry.source.clone(),
            width: entry.width,
            height: entry.height,
        })
    }

    /// Stores a validated image, replacing any earlier one of the same kind, and clears its recorded misses.
    pub fn store(
        &self,
        id: &GameId,
        kind: ArtworkKind,
        bytes: &[u8],
        info: ImageInfo,
        source: &str,
        now: u64,
    ) -> io::Result<CachedImage> {
        let dir = self.game_dir(id);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.{}", kind.as_str(), info.format.extension()));
        write_atomic(&path, bytes)?;
        // Remove the same kind in another format so only one copy is kept.
        for other in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            if other != info.format {
                let _ =
                    fs::remove_file(dir.join(format!("{}.{}", kind.as_str(), other.extension())));
            }
        }
        let mut meta = self.read_meta(id);
        meta.misses.remove(kind.as_str());
        meta.entries.insert(
            kind.as_str().to_string(),
            Entry {
                source: source.to_string(),
                ext: info.format.extension().to_string(),
                width: info.width,
                height: info.height,
                stored_at: now,
            },
        );
        self.write_meta(id, &meta)?;
        Ok(CachedImage {
            path,
            source: source.to_string(),
            width: info.width,
            height: info.height,
        })
    }

    /// Records that no provider had `kind` (locally, or online too).
    pub fn record_miss(
        &self,
        id: &GameId,
        kind: ArtworkKind,
        online: bool,
        now: u64,
    ) -> io::Result<()> {
        let mut meta = self.read_meta(id);
        let miss = meta
            .misses
            .entry(kind.as_str().to_string())
            .or_insert(Miss {
                local_at: None,
                online_at: None,
            });
        miss.local_at = Some(now);
        if online {
            miss.online_at = Some(now);
        }
        self.write_meta(id, &meta)
    }

    /// Whether a lookup of this kind found nothing recently enough that it shouldn't be repeated yet.
    pub fn recently_missed(&self, id: &GameId, kind: ArtworkKind, online: bool, now: u64) -> bool {
        let meta = self.read_meta(id);
        let Some(miss) = meta.misses.get(kind.as_str()) else {
            return false;
        };
        let fresh = |at: Option<u64>, window: u64| {
            at.is_some_and(|t| now.saturating_sub(t) < window && t <= now)
        };
        if online {
            fresh(miss.online_at, ONLINE_MISS_SECONDS)
        } else {
            fresh(miss.local_at, LOCAL_MISS_SECONDS)
        }
    }

    /// Forgets one image (and any recorded miss) of one game.
    pub fn remove(&self, id: &GameId, kind: ArtworkKind) -> io::Result<()> {
        let dir = self.game_dir(id);
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            match fs::remove_file(dir.join(format!("{}.{}", kind.as_str(), format.extension()))) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        let mut meta = self.read_meta(id);
        if meta.entries.remove(kind.as_str()).is_some()
            | meta.misses.remove(kind.as_str()).is_some()
        {
            self.write_meta(id, &meta)?;
        }
        Ok(())
    }

    /// Forgets one game's artwork (e.g. the user picked "Refresh artwork").
    pub fn remove_game(&self, id: &GameId) -> io::Result<()> {
        match fs::remove_dir_all(self.game_dir(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Total bytes used, for Settings → Storage.
    pub fn size_bytes(&self) -> u64 {
        fn walk(dir: &Path, depth: u32) -> u64 {
            let Ok(entries) = fs::read_dir(dir) else {
                return 0;
            };
            entries
                .flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() && depth < 2 => walk(&e.path(), depth + 1),
                    Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
                    _ => 0,
                })
                .sum()
        }
        walk(&self.root, 0)
    }

    /// Deletes all cached artwork. Only ever touches the cache folder's own game sub-folders.
    pub fn clear(&self) -> io::Result<()> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                fs::remove_dir_all(entry.path())?;
            }
        }
        Ok(())
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Seconds since the Unix epoch.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod test_dir {
    use std::path::{Path, PathBuf};

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("pt-artwork-test-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).ok();
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_dir::TempDir;
    use super::*;
    use crate::image::samples;

    #[test]
    fn store_get_replace_and_clear() {
        let dir = TempDir::new("cache");
        let cache = ArtworkCache::new(dir.path().join("Artwork"));
        let id = GameId::Steam { app_id: 620 };
        assert_eq!(cache.get(&id, ArtworkKind::Cover), None);

        let jpeg = samples::jpeg(600, 900);
        let info = image::validate(&jpeg, None).expect("valid");
        let stored = cache
            .store(&id, ArtworkKind::Cover, &jpeg, info, "steam-local", 100)
            .expect("stored");
        assert!(stored
            .path
            .ends_with(Path::new("steam_620").join("cover.jpg")));
        assert_eq!(cache.get(&id, ArtworkKind::Cover), Some(stored));

        let png = samples::png(600, 900);
        let info = image::validate(&png, None).expect("valid");
        let replaced = cache
            .store(&id, ArtworkKind::Cover, &png, info, "steamgriddb", 200)
            .expect("stored");
        assert!(replaced.path.ends_with("cover.png"));
        assert!(
            !replaced.path.with_extension("jpg").exists(),
            "old format removed"
        );
        assert!(cache.size_bytes() > 0);

        cache.clear().expect("cleared");
        assert_eq!(cache.get(&id, ArtworkKind::Cover), None);
        assert!(cache.root().exists(), "the cache folder itself stays");
    }

    #[test]
    fn misses_expire() {
        let dir = TempDir::new("misses");
        let cache = ArtworkCache::new(dir.path());
        let id = GameId::BuiltIn { key: "roblox" };
        cache
            .record_miss(&id, ArtworkKind::Hero, false, 1_000)
            .expect("ok");
        assert!(cache.recently_missed(&id, ArtworkKind::Hero, false, 1_000 + 60));
        assert!(
            !cache.recently_missed(&id, ArtworkKind::Hero, true, 1_000 + 60),
            "online wasn't tried"
        );
        assert!(!cache.recently_missed(&id, ArtworkKind::Hero, false, 1_000 + LOCAL_MISS_SECONDS));

        cache
            .record_miss(&id, ArtworkKind::Hero, true, 2_000)
            .expect("ok");
        assert!(cache.recently_missed(&id, ArtworkKind::Hero, true, 2_000 + LOCAL_MISS_SECONDS));
        assert!(!cache.recently_missed(&id, ArtworkKind::Hero, true, 2_000 + ONLINE_MISS_SECONDS));
    }

    #[test]
    fn tampered_meta_cannot_point_outside() {
        let dir = TempDir::new("tamper");
        let cache = ArtworkCache::new(dir.path());
        let id = GameId::Steam { app_id: 1 };
        let game = dir.path().join(id.cache_key());
        fs::create_dir_all(&game).expect("dir");
        fs::write(
            game.join(META_FILE),
            r#"{"entries":{"cover":{"source":"x","ext":"..\\..\\secret","width":1,"height":1,"storedAt":0}}}"#,
        )
        .expect("write");
        assert_eq!(cache.get(&id, ArtworkKind::Cover), None);
    }
}

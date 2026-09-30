//! Finds artwork for a game: the user's own choice first, then the cache, then each provider in order (online ones
//! only when allowed), validating and caching whatever is found.

use crate::cache::{unix_now, ArtworkCache, CachedImage};
use crate::image::{self, ImageInfo};
use crate::{ArtworkError, ArtworkKind, ArtworkProvider, ArtworkRequest};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// How many pictures to offer when the user picks artwork from an online source.
pub const MAX_CHOICES: usize = 12;

/// The result of a lookup: the image if one was found, and anything worth logging along the way.
#[derive(Debug, Default)]
pub struct Lookup {
    pub image: Option<CachedImage>,
    /// `(provider, problem)`; messages never contain API keys (requests aren't echoed).
    pub issues: Vec<(&'static str, String)>,
}

/// A picture offered to choose from, previewed from a local file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoicePreview {
    /// Pass back to [`ArtworkService::apply_choice`].
    pub index: usize,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

/// For each game (cache key) and kind, the choices last offered: `(provider index, full image URL)`.
type Offered = HashMap<(String, ArtworkKind), Vec<(usize, String)>>;

pub struct ArtworkService {
    cache: ArtworkCache,
    /// Pictures the user chose themselves; always win over automatic artwork and survive clearing the cache.
    overrides: Option<ArtworkCache>,
    /// Where previews of the offered choices are kept (replaced on every listing).
    choices_dir: Option<PathBuf>,
    /// For each game and kind, the choices last offered: `(provider index, full image URL)`. Only these are ever
    /// downloaded when the user picks one.
    offered: Mutex<Offered>,
    providers: Vec<Arc<dyn ArtworkProvider>>,
    online_allowed: AtomicBool,
}

impl ArtworkService {
    /// `providers` in priority order; put local ones first. Online providers are skipped until
    /// [`set_online_allowed`](Self::set_online_allowed) is called with `true` (the user's "Online artwork" setting).
    pub fn new(cache: ArtworkCache, providers: Vec<Arc<dyn ArtworkProvider>>) -> Self {
        Self {
            cache,
            overrides: None,
            choices_dir: None,
            offered: Mutex::new(HashMap::new()),
            providers,
            online_allowed: AtomicBool::new(false),
        }
    }

    /// Enables the user's own artwork choices, kept in `overrides`, with previews of online choices in
    /// `choices_dir`.
    pub fn with_overrides(mut self, overrides: ArtworkCache, choices_dir: PathBuf) -> Self {
        self.overrides = Some(overrides);
        self.choices_dir = Some(choices_dir);
        self
    }

    pub fn set_online_allowed(&self, allowed: bool) {
        self.online_allowed.store(allowed, Ordering::Relaxed);
    }

    pub fn online_allowed(&self) -> bool {
        self.online_allowed.load(Ordering::Relaxed)
    }

    pub fn cache(&self) -> &ArtworkCache {
        &self.cache
    }

    fn chosen(&self, request: &ArtworkRequest, kind: ArtworkKind) -> Option<CachedImage> {
        self.overrides.as_ref()?.get(&request.id, kind)
    }

    /// The user's choice or cached artwork only; never asks a provider. For drawing a list quickly.
    pub fn cached(&self, request: &ArtworkRequest, kind: ArtworkKind) -> Option<CachedImage> {
        self.chosen(request, kind)
            .or_else(|| self.cache.get(&request.id, kind))
    }

    /// The user's choice, cached artwork, or the first valid image a provider has (which is then cached).
    pub fn get(&self, request: &ArtworkRequest, kind: ArtworkKind) -> Lookup {
        self.get_at(request, kind, unix_now())
    }

    fn get_at(&self, request: &ArtworkRequest, kind: ArtworkKind, now: u64) -> Lookup {
        let mut lookup = Lookup::default();
        if let Some(image) = self.cached(request, kind) {
            lookup.image = Some(image);
            return lookup;
        }
        let online = self.online_allowed();
        if self.cache.recently_missed(&request.id, kind, online, now) {
            return lookup;
        }

        let mut network_failed = false;
        for provider in &self.providers {
            if provider.is_online() && !online {
                continue;
            }
            let fetched = match provider.fetch(request, kind) {
                Ok(Some(fetched)) => fetched,
                Ok(None) => continue,
                Err(e) => {
                    network_failed |= matches!(e, ArtworkError::Http(_)) || provider.is_online();
                    lookup.issues.push((provider.name(), e.to_string()));
                    continue;
                }
            };
            let info = match image::validate(&fetched.bytes, fetched.content_type.as_deref()) {
                Ok(info) => info,
                Err(e) => {
                    lookup
                        .issues
                        .push((provider.name(), format!("rejected {kind} image: {e}")));
                    continue;
                }
            };
            match self.cache.store(
                &request.id,
                kind,
                &fetched.bytes,
                info,
                provider.name(),
                now,
            ) {
                Ok(image) => {
                    lookup.image = Some(image);
                    return lookup;
                }
                Err(e) => lookup.issues.push(("cache", e.to_string())),
            }
        }

        // Don't remember a miss caused by being offline; try again next time.
        if !network_failed {
            if let Err(e) = self.cache.record_miss(&request.id, kind, online, now) {
                lookup.issues.push(("cache", e.to_string()));
            }
        }
        lookup
    }

    fn overrides(&self) -> Result<&ArtworkCache, ArtworkError> {
        self.overrides
            .as_ref()
            .ok_or_else(|| ArtworkError::Response("choosing artwork isn't available".into()))
    }

    /// Makes `bytes` (after the same checks as downloaded artwork) the game's `kind` image, stored as is.
    pub fn set_override(
        &self,
        request: &ArtworkRequest,
        kind: ArtworkKind,
        bytes: &[u8],
        source: &str,
    ) -> Result<CachedImage, ArtworkError> {
        let info: ImageInfo = image::validate(bytes, None)?;
        Ok(self
            .overrides()?
            .store(&request.id, kind, bytes, info, source, unix_now())?)
    }

    /// Makes a PNG, JPEG or WebP file from this PC the game's `kind` image. The file is copied, not referenced.
    pub fn set_override_from_file(
        &self,
        request: &ArtworkRequest,
        kind: ArtworkKind,
        path: &Path,
    ) -> Result<CachedImage, ArtworkError> {
        let meta = fs::metadata(path)?;
        if !meta.is_file() || meta.len() > image::MAX_IMAGE_BYTES as u64 {
            return Err(ArtworkError::Response(format!(
                "choose a picture file of at most {} MB",
                image::MAX_IMAGE_BYTES / (1024 * 1024)
            )));
        }
        let bytes = fs::read(path)?;
        self.set_override(request, kind, &bytes, "file")
    }

    /// Goes back to automatic artwork for the game's `kind` image.
    pub fn clear_override(
        &self,
        request: &ArtworkRequest,
        kind: ArtworkKind,
    ) -> Result<(), ArtworkError> {
        Ok(self.overrides()?.remove(&request.id, kind)?)
    }

    /// Pictures of `kind` to choose from, from the online providers that offer several (only when online artwork
    /// is on). Previews are downloaded, checked and saved locally; see [`apply_choice`](Self::apply_choice).
    pub fn choices(
        &self,
        request: &ArtworkRequest,
        kind: ArtworkKind,
        limit: usize,
    ) -> Result<Vec<ChoicePreview>, ArtworkError> {
        if !self.online_allowed() {
            return Err(ArtworkError::Response(
                "turn on online artwork in Settings to pick from SteamGridDB".into(),
            ));
        }
        let root = self
            .choices_dir
            .as_ref()
            .ok_or_else(|| ArtworkError::Response("choosing artwork isn't available".into()))?;
        let dir = root.join(request.id.cache_key());
        match fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        fs::create_dir_all(&dir)?;

        let mut offered = Vec::new();
        let mut previews = Vec::new();
        for (index, provider) in self.providers.iter().enumerate() {
            if !provider.is_online() || previews.len() >= limit {
                continue;
            }
            for choice in provider.choices(request, kind, limit - previews.len())? {
                let Some(fetched) = provider.download(&choice.thumb_url)? else {
                    continue;
                };
                let Ok(info) = image::validate(&fetched.bytes, fetched.content_type.as_deref())
                else {
                    continue;
                };
                let path = dir.join(format!(
                    "{}-{}.{}",
                    kind.as_str(),
                    offered.len(),
                    info.format.extension()
                ));
                fs::write(&path, &fetched.bytes)?;
                previews.push(ChoicePreview {
                    index: offered.len(),
                    path,
                    width: info.width,
                    height: info.height,
                });
                offered.push((index, choice.url));
            }
        }
        if let Ok(mut map) = self.offered.lock() {
            map.insert((request.id.cache_key(), kind), offered);
        }
        Ok(previews)
    }

    /// Downloads the full picture of one of the last [`choices`](Self::choices) offered for this game and makes it
    /// the game's `kind` image. Nothing but an offered picture is ever downloaded.
    pub fn apply_choice(
        &self,
        request: &ArtworkRequest,
        kind: ArtworkKind,
        index: usize,
    ) -> Result<CachedImage, ArtworkError> {
        let chosen = self.offered.lock().ok().and_then(|map| {
            map.get(&(request.id.cache_key(), kind))?
                .get(index)
                .cloned()
        });
        let Some((provider, url)) = chosen else {
            return Err(ArtworkError::Response(
                "that picture is no longer offered; list them again".into(),
            ));
        };
        let provider = &self.providers[provider];
        let Some(fetched) = provider.download(&url)? else {
            return Err(ArtworkError::Response(
                "the picture couldn't be downloaded".into(),
            ));
        };
        self.set_override(request, kind, &fetched.bytes, provider.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::test_dir::TempDir;
    use crate::http::HttpError;
    use crate::image::samples;
    use crate::FetchedImage;
    use playtime_core::launchers::GameId;
    use std::sync::atomic::AtomicUsize;

    struct Stub {
        name: &'static str,
        online: bool,
        result: fn() -> Result<Option<FetchedImage>, ArtworkError>,
        calls: AtomicUsize,
    }

    impl Stub {
        fn new(
            name: &'static str,
            online: bool,
            result: fn() -> Result<Option<FetchedImage>, ArtworkError>,
        ) -> Arc<Self> {
            Arc::new(Self {
                name,
                online,
                result,
                calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }

    impl ArtworkProvider for Stub {
        fn name(&self) -> &'static str {
            self.name
        }
        fn is_online(&self) -> bool {
            self.online
        }
        fn fetch(
            &self,
            _: &ArtworkRequest,
            _: ArtworkKind,
        ) -> Result<Option<FetchedImage>, ArtworkError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            (self.result)()
        }
    }

    fn request() -> ArtworkRequest {
        ArtworkRequest {
            id: GameId::Steam { app_id: 620 },
            name: "Portal 2".into(),
            exe_path: None,
        }
    }

    fn png() -> Result<Option<FetchedImage>, ArtworkError> {
        Ok(Some(FetchedImage {
            bytes: samples::png(600, 900),
            content_type: Some("image/png".into()),
        }))
    }

    #[test]
    fn online_only_when_allowed_then_cached() {
        let dir = TempDir::new("service-online");
        let local = Stub::new("local", false, || Ok(None));
        let online = Stub::new("online", true, png);
        let service = ArtworkService::new(
            ArtworkCache::new(dir.path()),
            vec![local.clone() as Arc<dyn ArtworkProvider>, online.clone()],
        );

        assert!(service
            .get_at(&request(), ArtworkKind::Cover, 100)
            .image
            .is_none());
        assert_eq!(online.calls(), 0, "never online by default");

        // The local miss is remembered for a day…
        assert!(service
            .get_at(&request(), ArtworkKind::Cover, 200)
            .image
            .is_none());
        assert_eq!(local.calls(), 1);

        // …but turning online artwork on is a different lookup.
        service.set_online_allowed(true);
        let found = service
            .get_at(&request(), ArtworkKind::Cover, 300)
            .image
            .expect("found");
        assert_eq!(found.source, "online");
        assert!(service
            .get_at(&request(), ArtworkKind::Cover, 400)
            .image
            .is_some());
        assert_eq!(online.calls(), 1, "served from the cache afterwards");
    }

    #[test]
    fn invalid_images_are_skipped_and_offline_misses_not_remembered() {
        let dir = TempDir::new("service-invalid");
        let html = Stub::new("html", true, || {
            Ok(Some(FetchedImage {
                bytes: b"<html>".to_vec(),
                content_type: Some("text/html".into()),
            }))
        });
        let offline = Stub::new("offline", true, || {
            Err(ArtworkError::Http(HttpError::Transport(
                "no network".into(),
            )))
        });
        let service = ArtworkService::new(
            ArtworkCache::new(dir.path()),
            vec![html.clone() as Arc<dyn ArtworkProvider>, offline.clone()],
        );
        service.set_online_allowed(true);

        let lookup = service.get_at(&request(), ArtworkKind::Hero, 100);
        assert!(lookup.image.is_none());
        assert_eq!(lookup.issues.len(), 2);
        service.get_at(&request(), ArtworkKind::Hero, 200);
        assert_eq!(
            offline.calls(),
            2,
            "tried again because the first attempt was offline"
        );
    }

    struct Chooser;

    impl ArtworkProvider for Chooser {
        fn name(&self) -> &'static str {
            "chooser"
        }
        fn is_online(&self) -> bool {
            true
        }
        fn fetch(
            &self,
            _: &ArtworkRequest,
            _: ArtworkKind,
        ) -> Result<Option<FetchedImage>, ArtworkError> {
            Ok(None)
        }
        fn choices(
            &self,
            _: &ArtworkRequest,
            _: ArtworkKind,
            limit: usize,
        ) -> Result<Vec<crate::ArtworkChoice>, ArtworkError> {
            Ok((0..3)
                .map(|i| crate::ArtworkChoice {
                    url: format!("https://cdn2.steamgriddb.com/full/{i}.png"),
                    thumb_url: format!("https://cdn2.steamgriddb.com/thumb/{i}.png"),
                })
                .take(limit)
                .collect())
        }
        fn download(&self, url: &str) -> Result<Option<FetchedImage>, ArtworkError> {
            let (w, h) = if url.contains("/thumb/") {
                (60, 90)
            } else {
                (600, 900)
            };
            Ok(Some(FetchedImage {
                bytes: samples::png(w, h),
                content_type: Some("image/png".into()),
            }))
        }
    }

    fn with_overrides(dir: &TempDir, providers: Vec<Arc<dyn ArtworkProvider>>) -> ArtworkService {
        ArtworkService::new(ArtworkCache::new(dir.path().join("cache")), providers).with_overrides(
            ArtworkCache::new(dir.path().join("overrides")),
            dir.path().join("choices"),
        )
    }

    #[test]
    fn the_users_choice_wins_and_can_be_undone() {
        let dir = TempDir::new("service-override");
        let auto = Stub::new("auto", false, png);
        let service = with_overrides(&dir, vec![auto.clone() as Arc<dyn ArtworkProvider>]);
        assert_eq!(
            service
                .get_at(&request(), ArtworkKind::Cover, 1)
                .image
                .expect("auto")
                .source,
            "auto"
        );

        let file = dir.path().join("mine.png");
        std::fs::write(&file, samples::png(320, 480)).expect("write");
        let chosen = service
            .set_override_from_file(&request(), ArtworkKind::Cover, &file)
            .expect("set");
        assert_eq!((chosen.source.as_str(), chosen.width), ("file", 320));
        assert_eq!(
            service
                .cached(&request(), ArtworkKind::Cover)
                .expect("cached")
                .source,
            "file"
        );

        // Clearing the automatic cache keeps the user's choice.
        service.cache().clear().expect("clear");
        assert_eq!(
            service
                .get_at(&request(), ArtworkKind::Cover, 2)
                .image
                .expect("still")
                .source,
            "file"
        );

        service
            .clear_override(&request(), ArtworkKind::Cover)
            .expect("reset");
        assert_eq!(
            service
                .get_at(&request(), ArtworkKind::Cover, 3)
                .image
                .expect("auto again")
                .source,
            "auto"
        );
    }

    #[test]
    fn files_that_arent_pictures_are_refused() {
        let dir = TempDir::new("service-override-bad");
        let service = with_overrides(&dir, Vec::new());
        let file = dir.path().join("notes.png");
        std::fs::write(&file, b"not a picture").expect("write");
        assert!(service
            .set_override_from_file(&request(), ArtworkKind::Cover, &file)
            .is_err());
        assert!(service.cached(&request(), ArtworkKind::Cover).is_none());
    }

    #[test]
    fn choices_are_online_only_and_only_offered_pictures_are_downloaded() {
        let dir = TempDir::new("service-choices");
        let service = with_overrides(&dir, vec![Arc::new(Chooser) as Arc<dyn ArtworkProvider>]);
        assert!(
            service.choices(&request(), ArtworkKind::Cover, 12).is_err(),
            "online artwork is off"
        );

        service.set_online_allowed(true);
        let previews = service
            .choices(&request(), ArtworkKind::Cover, 2)
            .expect("choices");
        assert_eq!(previews.len(), 2, "limited");
        assert!(previews.iter().all(|p| p.path.is_file() && p.width == 60));

        let chosen = service
            .apply_choice(&request(), ArtworkKind::Cover, 1)
            .expect("apply");
        assert_eq!((chosen.source.as_str(), chosen.width), ("chooser", 600));
        assert!(
            service
                .apply_choice(&request(), ArtworkKind::Cover, 2)
                .is_err(),
            "never offered"
        );
        assert!(
            service
                .apply_choice(&request(), ArtworkKind::Hero, 0)
                .is_err(),
            "other kind"
        );
    }
}

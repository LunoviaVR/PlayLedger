//! Finds artwork for a game: cache first, then each provider in order (online ones only when allowed), validating
//! and caching whatever is found.

use crate::cache::{unix_now, ArtworkCache, CachedImage};
use crate::{image, ArtworkError, ArtworkKind, ArtworkProvider, ArtworkRequest};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The result of a lookup: the image if one was found, and anything worth logging along the way.
#[derive(Debug, Default)]
pub struct Lookup {
    pub image: Option<CachedImage>,
    /// `(provider, problem)`; messages never contain API keys (requests aren't echoed).
    pub issues: Vec<(&'static str, String)>,
}

pub struct ArtworkService {
    cache: ArtworkCache,
    providers: Vec<Arc<dyn ArtworkProvider>>,
    online_allowed: AtomicBool,
}

impl ArtworkService {
    /// `providers` in priority order; put local ones first. Online providers are skipped until
    /// [`set_online_allowed`](Self::set_online_allowed) is called with `true` (the user's "Online artwork" setting).
    pub fn new(cache: ArtworkCache, providers: Vec<Arc<dyn ArtworkProvider>>) -> Self {
        Self {
            cache,
            providers,
            online_allowed: AtomicBool::new(false),
        }
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

    /// Cached artwork only; never asks a provider. For drawing a list quickly.
    pub fn cached(&self, request: &ArtworkRequest, kind: ArtworkKind) -> Option<CachedImage> {
        self.cache.get(&request.id, kind)
    }

    /// Cached artwork, or the first valid image a provider has (which is then cached).
    pub fn get(&self, request: &ArtworkRequest, kind: ArtworkKind) -> Lookup {
        self.get_at(request, kind, unix_now())
    }

    fn get_at(&self, request: &ArtworkRequest, kind: ArtworkKind, now: u64) -> Lookup {
        let mut lookup = Lookup::default();
        if let Some(image) = self.cache.get(&request.id, kind) {
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
}

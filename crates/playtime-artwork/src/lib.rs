//! Game artwork for PlayLedger.
//!
//! - [`ArtworkProvider`]: one source of pictures (Steam's local library cache, the Steam store CDN, SteamGridDB,
//!   the exe's own icon on Windows, …).
//! - [`cache::ArtworkCache`]: validated images on disk under `%LocalAppData%\Playtime Tracker\Cache\Artwork`, kept
//!   apart from the play history, plus remembered misses so nothing is re-requested constantly.
//! - [`service::ArtworkService`]: asks providers in order (local ones first), never goes online unless the user
//!   turned online artwork on, validates what comes back and caches it.
//!
//! Privacy: online providers receive only what identifies the picture (a Steam app ID, or a game's name for a
//! SteamGridDB search) and the user's own API key where one is required. Play history is never sent.

pub mod cache;
pub mod http;
pub mod image;
pub mod png;
pub mod providers;
pub mod service;

use playtime_core::launchers::GameId;
use std::fmt;

/// The kinds of artwork the dashboard shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArtworkKind {
    /// Portrait box art (600×900), the Games page tiles.
    Cover,
    /// Wide banner (Steam's 460×215 header), list rows.
    Header,
    /// Large background behind the game details.
    Hero,
    /// Transparent title logo drawn over the hero.
    Logo,
    /// Small square icon.
    Icon,
}

impl ArtworkKind {
    pub const ALL: [ArtworkKind; 5] = [
        Self::Cover,
        Self::Header,
        Self::Hero,
        Self::Logo,
        Self::Icon,
    ];

    /// File stem in the cache folder.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Header => "header",
            Self::Hero => "hero",
            Self::Logo => "logo",
            Self::Icon => "icon",
        }
    }
}

impl fmt::Display for ArtworkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What providers know about the game whose artwork is wanted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkRequest {
    pub id: GameId,
    /// Display name (only sent online for a name search, and only when online artwork is on).
    pub name: String,
    /// The game's exe, when known (for the icon provider).
    pub exe_path: Option<String>,
}

/// An image a provider returned, before validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedImage {
    pub bytes: Vec<u8>,
    /// Content type the source claimed, if any (checked against the actual bytes).
    pub content_type: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ArtworkError {
    #[error("network: {0}")]
    Http(#[from] http::HttpError),
    #[error("{0}")]
    Invalid(#[from] image::ImageError),
    #[error("unexpected response: {0}")]
    Response(String),
    #[error("file: {0}")]
    Io(#[from] std::io::Error),
}

/// One picture a provider offers to choose from (only online providers with several images offer any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkChoice {
    /// The full image (on an allowed artwork host).
    pub url: String,
    /// A smaller preview of it, for showing the choices.
    pub thumb_url: String,
}

/// One source of artwork.
pub trait ArtworkProvider: Send + Sync {
    /// Short name recorded in the cache (e.g. "steam-local").
    fn name(&self) -> &'static str;
    /// True if this provider makes network requests (only used when online artwork is on).
    fn is_online(&self) -> bool;
    /// The image for `kind`, `Ok(None)` if this provider has none for the game.
    fn fetch(
        &self,
        request: &ArtworkRequest,
        kind: ArtworkKind,
    ) -> Result<Option<FetchedImage>, ArtworkError>;

    /// Up to `limit` pictures of `kind` to choose from. Most providers have exactly one and offer none.
    fn choices(
        &self,
        _request: &ArtworkRequest,
        _kind: ArtworkKind,
        _limit: usize,
    ) -> Result<Vec<ArtworkChoice>, ArtworkError> {
        Ok(Vec::new())
    }

    /// Downloads one of this provider's [`choices`](Self::choices) (the full image or its preview).
    fn download(&self, _url: &str) -> Result<Option<FetchedImage>, ArtworkError> {
        Ok(None)
    }
}

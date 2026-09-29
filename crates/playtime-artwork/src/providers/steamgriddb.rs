//! SteamGridDB (<https://www.steamgriddb.com>): community artwork for any game, including ones outside Steam.
//! Online, opt-in, and only with the user's own API key (entered in Settings, stored in Windows Credential
//! Manager, never in the repository or the data files). Requests carry the Steam app ID, or for other games the
//! game's name for a search; the key is sent only to the API host, never to the image CDN.

use crate::http::{check_url, encode_segment, HttpClient, Request};
use crate::{ArtworkError, ArtworkKind, ArtworkProvider, ArtworkRequest, FetchedImage};
use playtime_core::launchers::GameId;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

const API: &str = "https://www.steamgriddb.com/api/v2";

pub struct SteamGridDbProvider {
    http: Arc<dyn HttpClient>,
    api_key: String,
    /// Game name → SteamGridDB game ID (or none), so a name is searched once per run.
    search_cache: Mutex<HashMap<String, Option<u64>>>,
}

impl fmt::Debug for SteamGridDbProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamGridDbProvider")
            .field("api_key", &"<redacted>")
            .finish()
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    success: bool,
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Deserialize)]
struct Asset {
    url: String,
}

#[derive(Deserialize)]
struct SearchHit {
    id: u64,
}

impl SteamGridDbProvider {
    /// `None` if the key is blank or isn't plausibly a SteamGridDB key (32 hex characters).
    pub fn new(http: Arc<dyn HttpClient>, api_key: &str) -> Option<Self> {
        let api_key = api_key.trim();
        if !is_plausible_key(api_key) {
            return None;
        }
        Some(Self {
            http,
            api_key: api_key.to_string(),
            search_cache: Mutex::new(HashMap::new()),
        })
    }

    fn api_get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
    ) -> Result<Option<Vec<T>>, ArtworkError> {
        let request = Request::get(format!("{API}{path}"))
            .header("Authorization", &format!("Bearer {}", self.api_key));
        let response = self.http.get(&request)?;
        match response.status {
            200 => {}
            404 => return Ok(None),
            401 | 403 => {
                return Err(ArtworkError::Response(
                    "SteamGridDB didn't accept the API key".into(),
                ))
            }
            status => {
                return Err(ArtworkError::Response(format!(
                    "SteamGridDB returned HTTP {status}"
                )))
            }
        }
        let envelope: Envelope<T> = serde_json::from_slice(&response.body)
            .map_err(|e| ArtworkError::Response(format!("SteamGridDB: {e}")))?;
        Ok(envelope.success.then_some(envelope.data))
    }

    fn game_id_for_name(&self, name: &str) -> Result<Option<u64>, ArtworkError> {
        let key = name.trim().to_lowercase();
        if key.is_empty() {
            return Ok(None);
        }
        if let Some(hit) = self
            .search_cache
            .lock()
            .ok()
            .and_then(|c| c.get(&key).copied())
        {
            return Ok(hit);
        }
        let hits: Vec<SearchHit> = self
            .api_get(&format!(
                "/search/autocomplete/{}",
                encode_segment(name.trim())
            ))?
            .unwrap_or_default();
        let id = hits.first().map(|h| h.id);
        if let Ok(mut cache) = self.search_cache.lock() {
            cache.insert(key, id);
        }
        Ok(id)
    }

    fn endpoint(kind: ArtworkKind) -> (&'static str, &'static str) {
        match kind {
            ArtworkKind::Cover => ("grids", "?dimensions=600x900&types=static"),
            ArtworkKind::Header => ("grids", "?dimensions=460x215,920x430&types=static"),
            ArtworkKind::Hero => ("heroes", "?types=static"),
            ArtworkKind::Logo => ("logos", "?types=static"),
            ArtworkKind::Icon => ("icons", "?types=static"),
        }
    }
}

/// SteamGridDB keys are 32 hexadecimal characters.
pub fn is_plausible_key(key: &str) -> bool {
    key.len() == 32 && key.chars().all(|c| c.is_ascii_hexdigit())
}

impl ArtworkProvider for SteamGridDbProvider {
    fn name(&self) -> &'static str {
        "steamgriddb"
    }

    fn is_online(&self) -> bool {
        true
    }

    fn fetch(
        &self,
        request: &ArtworkRequest,
        kind: ArtworkKind,
    ) -> Result<Option<FetchedImage>, ArtworkError> {
        let target = match &request.id {
            GameId::Steam { app_id } => format!("steam/{app_id}"),
            _ => match self.game_id_for_name(&request.name)? {
                Some(id) => format!("game/{id}"),
                None => return Ok(None),
            },
        };
        let (collection, query) = Self::endpoint(kind);
        let Some(assets) = self.api_get::<Asset>(&format!("/{collection}/{target}{query}"))? else {
            return Ok(None);
        };
        let Some(asset) = assets.into_iter().find(|a| check_url(&a.url).is_ok()) else {
            return Ok(None);
        };
        // The image CDN gets a plain request: no key, no game name.
        let response = self.http.get(&Request::get(asset.url))?;
        if response.status != 200 {
            return Ok(None);
        }
        Ok(Some(FetchedImage {
            bytes: response.body,
            content_type: response.content_type,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeClient;
    use crate::image::samples;

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn keys_are_checked_and_never_printed() {
        let client: Arc<dyn HttpClient> = Arc::new(FakeClient::default());
        assert!(SteamGridDbProvider::new(client.clone(), "").is_none());
        assert!(SteamGridDbProvider::new(client.clone(), "not-a-key").is_none());
        let provider = SteamGridDbProvider::new(client, &format!(" {KEY} ")).expect("valid");
        assert!(!format!("{provider:?}").contains(KEY));
    }

    #[test]
    fn steam_games_by_app_id_others_by_name() {
        let mut client = FakeClient::default();
        client.serve(
            &format!("{API}/grids/steam/620?dimensions=600x900&types=static"),
            "application/json",
            br#"{"success":true,"data":[{"url":"https://evil.example/x.png"},{"url":"https://cdn2.steamgriddb.com/grid/a.png"}]}"#.to_vec(),
        );
        client.serve(
            "https://cdn2.steamgriddb.com/grid/a.png",
            "image/png",
            samples::png(600, 900),
        );
        client.serve(
            &format!("{API}/search/autocomplete/Cookie%20Clicker"),
            "application/json",
            br#"{"success":true,"data":[{"id":5247497,"name":"Cookie Clicker"}]}"#.to_vec(),
        );
        client.serve(
            &format!("{API}/heroes/game/5247497?types=static"),
            "application/json",
            br#"{"success":true,"data":[{"url":"https://cdn2.steamgriddb.com/hero/b.png"}]}"#
                .to_vec(),
        );
        client.serve(
            "https://cdn2.steamgriddb.com/hero/b.png",
            "image/png",
            samples::png(1920, 620),
        );
        let client = Arc::new(client);
        let provider = SteamGridDbProvider::new(client.clone(), KEY).expect("valid");

        let steam = ArtworkRequest {
            id: GameId::Steam { app_id: 620 },
            name: "Portal 2".into(),
            exe_path: None,
        };
        let cover = provider
            .fetch(&steam, ArtworkKind::Cover)
            .expect("ok")
            .expect("found");
        assert_eq!(
            cover.bytes,
            samples::png(600, 900),
            "the off-list URL was skipped"
        );

        let other = ArtworkRequest {
            id: GameId::Path {
                normalized: r"c:\games\cookie clicker".into(),
            },
            name: "Cookie Clicker".into(),
            exe_path: None,
        };
        assert!(provider
            .fetch(&other, ArtworkKind::Hero)
            .expect("ok")
            .is_some());
        assert!(provider
            .fetch(&other, ArtworkKind::Hero)
            .expect("ok")
            .is_some());

        let requests = client.requests.lock().expect("lock").clone();
        let searches = requests
            .iter()
            .filter(|r| r.url.contains("/search/"))
            .count();
        assert_eq!(searches, 1, "a name is searched once");
        for request in &requests {
            let has_key = request.headers.iter().any(|(_, v)| v.contains(KEY));
            assert_eq!(
                has_key,
                request.url.starts_with(API),
                "key only goes to the API: {}",
                request.url
            );
            assert!(!request.url.contains("evil.example"));
        }
    }
}

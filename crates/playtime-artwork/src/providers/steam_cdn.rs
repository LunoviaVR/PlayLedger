//! The Steam store's public image CDN. Online and opt-in: the request carries only the game's Steam app ID.

use crate::http::{HttpClient, Request};
use crate::{ArtworkError, ArtworkKind, ArtworkProvider, ArtworkRequest, FetchedImage};
use playtime_core::launchers::GameId;
use std::sync::Arc;

const BASE: &str = "https://shared.steamstatic.com/store_item_assets/steam/apps";

pub struct SteamCdnProvider {
    http: Arc<dyn HttpClient>,
}

impl SteamCdnProvider {
    pub fn new(http: Arc<dyn HttpClient>) -> Self {
        Self { http }
    }

    fn file_name(kind: ArtworkKind) -> Option<&'static str> {
        match kind {
            ArtworkKind::Cover => Some("library_600x900_2x.jpg"),
            ArtworkKind::Header => Some("header.jpg"),
            ArtworkKind::Hero => Some("library_hero.jpg"),
            ArtworkKind::Logo => Some("logo.png"),
            ArtworkKind::Icon => None, // not at a predictable URL
        }
    }
}

impl ArtworkProvider for SteamCdnProvider {
    fn name(&self) -> &'static str {
        "steam-cdn"
    }

    fn is_online(&self) -> bool {
        true
    }

    fn fetch(
        &self,
        request: &ArtworkRequest,
        kind: ArtworkKind,
    ) -> Result<Option<FetchedImage>, ArtworkError> {
        let (GameId::Steam { app_id }, Some(file)) = (&request.id, Self::file_name(kind)) else {
            return Ok(None);
        };
        let response = self
            .http
            .get(&Request::get(format!("{BASE}/{app_id}/{file}")))?;
        match response.status {
            200 => Ok(Some(FetchedImage {
                bytes: response.body,
                content_type: response.content_type,
            })),
            404 | 403 => Ok(None),
            status => Err(ArtworkError::Response(format!(
                "Steam returned HTTP {status}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeClient;
    use crate::image::samples;

    #[test]
    fn sends_only_the_app_id() {
        let mut client = FakeClient::default();
        client.serve(
            &format!("{BASE}/620/header.jpg"),
            "image/jpeg",
            samples::jpeg(460, 215),
        );
        let client = Arc::new(client);
        let provider = SteamCdnProvider::new(client.clone());
        let request = ArtworkRequest {
            id: GameId::Steam { app_id: 620 },
            name: "Portal 2".into(),
            exe_path: Some(r"C:\Steam\steamapps\common\Portal 2\portal2.exe".into()),
        };
        assert!(provider
            .fetch(&request, ArtworkKind::Header)
            .expect("ok")
            .is_some());
        assert!(provider
            .fetch(&request, ArtworkKind::Hero)
            .expect("ok")
            .is_none());
        assert!(provider
            .fetch(&request, ArtworkKind::Icon)
            .expect("ok")
            .is_none());
        let urls = client.urls();
        assert_eq!(urls.len(), 2);
        assert!(urls
            .iter()
            .all(|u| !u.contains("Portal") && !u.contains("portal2")));
    }
}

//! The network boundary for online artwork. Providers build requests; an [`HttpClient`] (WinHTTP on Windows, a
//! fake in tests) performs them. [`check_url`] is applied to every URL before it's requested, including image URLs
//! that come back inside API responses, so a provider can only ever reach the hosts listed here, over HTTPS.

/// Hosts online artwork may contact. Anything else is refused before a connection is made.
pub const ALLOWED_HOSTS: &[&str] = &[
    // Steam store images (public; the request carries only the app ID).
    "shared.steamstatic.com",
    "shared.akamai.steamstatic.com",
    "shared.cloudflare.steamstatic.com",
    "cdn.akamai.steamstatic.com",
    "cdn.cloudflare.steamstatic.com",
    "steamcdn-a.akamaihd.net",
    // SteamGridDB API and its image CDN (only with the user's own API key).
    "www.steamgriddb.com",
    "cdn2.steamgriddb.com",
];

/// Largest response body accepted (images are validated separately against a smaller limit).
pub const MAX_RESPONSE_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    /// Extra headers, e.g. `Authorization`. Never logged.
    pub headers: Vec<(String, String)>,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HttpError {
    #[error("refused to contact {0}: only HTTPS requests to known artwork hosts are allowed")]
    NotAllowed(String),
    #[error("the response was larger than allowed")]
    TooLarge,
    #[error("{0}")]
    Transport(String),
}

pub trait HttpClient: Send + Sync {
    /// Performs a GET. Implementations must verify TLS certificates, must not follow redirects to hosts outside
    /// [`ALLOWED_HOSTS`] or to plain HTTP, and must stop reading past [`MAX_RESPONSE_BYTES`].
    fn get(&self, request: &Request) -> Result<Response, HttpError>;
}

/// The URL's host if it is `https://` and on the allow-list, else an error. No userinfo, no custom ports.
pub fn check_url(url: &str) -> Result<&str, HttpError> {
    let refuse = || HttpError::NotAllowed(redact(url));
    let rest = url.strip_prefix("https://").ok_or_else(refuse)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains(['@', ':', '\\']) || authority.is_empty() {
        return Err(refuse());
    }
    let host = authority;
    if ALLOWED_HOSTS
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
    {
        Ok(host)
    } else {
        Err(refuse())
    }
}

/// The URL without its query string, for error messages and logs (queries could carry search terms).
pub fn redact(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

/// Percent-encodes one URL path segment (for a game name in a search URL).
pub fn encode_segment(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Serves canned responses by URL and records every request made.
    #[derive(Default)]
    pub struct FakeClient {
        pub responses: HashMap<String, Response>,
        pub requests: Mutex<Vec<Request>>,
    }

    impl FakeClient {
        pub fn serve(&mut self, url: &str, content_type: &str, body: Vec<u8>) {
            self.responses.insert(
                url.to_string(),
                Response {
                    status: 200,
                    content_type: Some(content_type.to_string()),
                    body,
                },
            );
        }

        pub fn urls(&self) -> Vec<String> {
            self.requests
                .lock()
                .map(|r| r.iter().map(|q| q.url.clone()).collect())
                .unwrap_or_default()
        }
    }

    impl HttpClient for FakeClient {
        fn get(&self, request: &Request) -> Result<Response, HttpError> {
            check_url(&request.url)?;
            if let Ok(mut log) = self.requests.lock() {
                log.push(request.clone());
            }
            Ok(self
                .responses
                .get(&request.url)
                .cloned()
                .unwrap_or(Response {
                    status: 404,
                    content_type: Some("text/html".into()),
                    body: b"not found".to_vec(),
                }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_to_known_hosts() {
        assert_eq!(
            check_url("https://shared.steamstatic.com/store_item_assets/steam/apps/620/header.jpg"),
            Ok("shared.steamstatic.com")
        );
        assert_eq!(
            check_url("https://CDN2.steamgriddb.com/grid/x.png"),
            Ok("CDN2.steamgriddb.com")
        );
        for bad in [
            "http://shared.steamstatic.com/x.jpg",
            "https://evil.example/x.jpg",
            "https://shared.steamstatic.com.evil.example/x.jpg",
            "https://user@shared.steamstatic.com/x.jpg",
            "https://shared.steamstatic.com:8443/x.jpg",
            "https:///x.jpg",
            "file:///C:/x.jpg",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn redacts_queries_and_encodes_segments() {
        assert_eq!(redact("https://a/b?term=secret#x"), "https://a/b");
        assert_eq!(
            encode_segment("Assassin's Creed: Origins/2"),
            "Assassin%27s%20Creed%3A%20Origins%2F2"
        );
    }
}

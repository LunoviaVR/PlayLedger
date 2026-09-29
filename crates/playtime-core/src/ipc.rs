//! The protocol between the tracker (server) and the dashboard (client) over the named pipe
//! `\\.\pipe\PlaytimeTracker.<user SID>`: one JSON object per line, UTF-8, at most [`MAX_MESSAGE_BYTES`].
//!
//! The dashboard sends [`Request`]s and gets one [`Response`] each. After `subscribe`, the connection also receives
//! [`Response::Event`]s whenever something changes. Secrets only ever flow into the tracker (an API key is set,
//! never read back).

use crate::dashboard::{DashboardModel, DayHistory, DayTotal};
use crate::model::SessionRecord;
use crate::settings::Settings;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};

/// Bumped when a message changes incompatibly.
pub const PROTOCOL_VERSION: u32 = 1;
/// Largest message accepted either way (a dashboard snapshot of years of play is well under this).
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

pub fn pipe_name(user_sid: &str) -> String {
    format!(r"\\.\pipe\PlaytimeTracker.{user_sid}")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Request {
    #[serde(rename_all = "camelCase")]
    Hello {
        protocol: u32,
    },
    GetDashboard,
    GetSettings,
    UpdateSettings {
        settings: Box<Settings>,
    },
    #[serde(rename_all = "camelCase")]
    DeleteSession {
        game: String,
        start: Timestamp,
    },
    #[serde(rename_all = "camelCase")]
    DeleteGameHistory {
        game: String,
    },
    #[serde(rename_all = "camelCase")]
    SetGameIgnored {
        game: String,
        ignored: bool,
    },
    RescanGames,
    /// `kind`: cover | header | hero | logo | icon.
    #[serde(rename_all = "camelCase")]
    GetArtwork {
        game: String,
        kind: String,
    },
    ExportCsv,
    /// Stores (or with `None`, removes) the user's SteamGridDB API key in Credential Manager.
    #[serde(rename_all = "camelCase")]
    SetSteamGridDbKey {
        key: Option<String>,
    },
    CheckForUpdates,
    Subscribe,
}

/// How a game was identified: where it was found, and the key its artwork is cached under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameIdentity {
    pub game: String,
    pub source: String,
    pub artwork_key: Option<String>,
}

/// Everything the dashboard's pages need, in one message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSnapshot {
    pub revision: u64,
    #[serde(flatten)]
    pub model: DashboardModel,
    /// Last 30 days, oldest first.
    pub daily: Vec<DayTotal>,
    /// Last 30 days, newest first.
    pub history: Vec<DayHistory>,
    pub past_week_seconds: i64,
    pub identities: Vec<GameIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    #[serde(rename_all = "camelCase")]
    SessionStarted { game: String },
    #[serde(rename_all = "camelCase")]
    SessionEnded { session: SessionRecord },
    #[serde(rename_all = "camelCase")]
    DataChanged { revision: u64 },
    #[serde(rename_all = "camelCase")]
    ArtworkReady { game: String, kind: String },
    #[serde(rename_all = "camelCase")]
    UpdateAvailable { version: String },
    /// Sent when nothing else happened for a while, so a closed connection is noticed; clients ignore it.
    Heartbeat,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Response {
    #[serde(rename_all = "camelCase")]
    Hello {
        protocol: u32,
        version: String,
    },
    Dashboard {
        snapshot: Box<DashboardSnapshot>,
    },
    #[serde(rename_all = "camelCase")]
    Settings {
        settings: Box<Settings>,
        has_steam_grid_db_key: bool,
    },
    Ok,
    Csv {
        text: String,
    },
    /// A local file path, or `None` if there's no artwork (yet: an `artworkReady` event follows if it arrives).
    Artwork {
        path: Option<String>,
    },
    Error {
        message: String,
    },
    Event {
        event: Event,
    },
}

/// Reads one message line. `Ok(None)` at end of stream. Over-long lines and invalid UTF-8 are errors.
pub fn read_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut buffer = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if buffer.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "message cut off",
                ))
            };
        }
        let (take, done) = match available.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (available.len(), false),
        };
        if buffer.len() + take > MAX_MESSAGE_BYTES + 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message too large",
            ));
        }
        buffer.extend_from_slice(&available[..take]);
        reader.consume(take);
        if done {
            buffer.pop();
            if buffer.last() == Some(&b'\r') {
                buffer.pop();
            }
            return String::from_utf8(buffer)
                .map(Some)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "message isn't UTF-8"));
        }
    }
}

/// Writes one message as a JSON line.
pub fn write_message<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    let mut json = serde_json::to_vec(message).map_err(io::Error::other)?;
    if json.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    json.push(b'\n');
    writer.write_all(&json)?;
    writer.flush()
}

pub fn parse_request(line: &str) -> Result<Request, String> {
    serde_json::from_str(line).map_err(|e| format!("invalid request: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn requests_parse_from_camel_case_json() {
        assert_eq!(
            parse_request(r#"{"type":"hello","protocol":1}"#),
            Ok(Request::Hello { protocol: 1 })
        );
        assert_eq!(
            parse_request(r#"{"type":"getDashboard"}"#),
            Ok(Request::GetDashboard)
        );
        assert_eq!(
            parse_request(
                r#"{"type":"deleteSession","game":"Hades","start":"2026-09-29T12:00:00+00:00"}"#
            ),
            Ok(Request::DeleteSession {
                game: "Hades".into(),
                start: Timestamp::parse("2026-09-29T12:00:00+00:00").expect("valid"),
            })
        );
        assert_eq!(
            parse_request(r#"{"type":"setSteamGridDbKey","key":null}"#),
            Ok(Request::SetSteamGridDbKey { key: None })
        );
        assert!(parse_request(r#"{"type":"formatDisk"}"#).is_err());
        assert!(parse_request("not json").is_err());
    }

    #[test]
    fn responses_are_tagged() {
        let mut out = Vec::new();
        write_message(
            &mut out,
            &Response::Event {
                event: Event::DataChanged { revision: 7 },
            },
        )
        .expect("written");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "{\"type\":\"event\",\"event\":{\"type\":\"dataChanged\",\"revision\":7}}\n"
        );
    }

    #[test]
    fn line_framing() {
        let mut reader = BufReader::with_capacity(4, "one\r\ntwo\nthree".as_bytes());
        assert_eq!(read_line(&mut reader).expect("ok").as_deref(), Some("one"));
        assert_eq!(read_line(&mut reader).expect("ok").as_deref(), Some("two"));
        assert!(read_line(&mut reader).is_err(), "cut off");

        let mut empty = BufReader::new(&b""[..]);
        assert_eq!(read_line(&mut empty).expect("ok"), None);

        let huge = vec![b'a'; MAX_MESSAGE_BYTES + 2];
        let mut reader = BufReader::new(&huge[..]);
        assert_eq!(
            read_line(&mut reader).map_err(|e| e.kind()),
            Err(io::ErrorKind::InvalidData)
        );
    }
}

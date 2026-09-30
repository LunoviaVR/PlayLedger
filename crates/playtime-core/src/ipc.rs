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
    /// Makes a picture file from this PC the game's `kind` artwork (copied into the tracker's own folder).
    #[serde(rename_all = "camelCase")]
    SetArtworkFromFile {
        game: String,
        kind: String,
        path: String,
    },
    /// Pictures of `kind` for the game to choose from (SteamGridDB; only with online artwork on and a key).
    #[serde(rename_all = "camelCase")]
    ListArtworkChoices {
        game: String,
        kind: String,
    },
    /// Makes one of the pictures last listed by `listArtworkChoices` the game's `kind` artwork.
    #[serde(rename_all = "camelCase")]
    ApplyArtworkChoice {
        game: String,
        kind: String,
        index: usize,
    },
    /// Goes back to automatic artwork for the game's `kind`.
    #[serde(rename_all = "camelCase")]
    ResetArtwork {
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
    GetUpdateStatus,
    InstallUpdate,
    #[serde(rename_all = "camelCase")]
    SetStartWithWindows {
        enabled: bool,
    },
    Subscribe,
}

/// One picture offered by `listArtworkChoices`; `index` goes back in `applyArtworkChoice`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkChoiceInfo {
    pub index: usize,
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// How a game was identified: where it was found, and the key its artwork is cached under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameIdentity {
    pub game: String,
    pub source: String,
    pub artwork_key: Option<String>,
}

/// Everything the dashboard's pages need, in one message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        start_with_windows: bool,
        /// Only the installed copy manages "Start with Windows" and updates itself.
        is_installed_copy: bool,
    },
    #[serde(rename_all = "camelCase")]
    UpdateStatus {
        current_version: String,
        available_version: Option<String>,
        can_install: bool,
        busy: bool,
        last_error: Option<String>,
        /// Local time of the last check, ISO 8601.
        last_checked: Option<String>,
    },
    Ok,
    Csv {
        text: String,
    },
    /// A local file path, or `None` if there's no artwork (yet: an `artworkReady` event follows if it arrives).
    Artwork {
        path: Option<String>,
    },
    /// Pictures to choose from, previewed from local files.
    ArtworkChoices {
        choices: Vec<ArtworkChoiceInfo>,
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

    /// Messages as the dashboard receives them, kept in the dashboard crate (which renders its screenshots from
    /// them), so a protocol change shows up as a failing test. Regenerate with `PT_UPDATE_FIXTURES=1 cargo test`.
    #[test]
    fn dashboard_fixture_matches() {
        use crate::dashboard::DashboardModel;
        use crate::model::ActiveSession;
        let at = |t: &str| Timestamp::parse(t).expect("valid");
        let finished = [
            SessionRecord {
                game: "Elden Ring".into(),
                start: at("2026-09-27T20:00:00+00:00"),
                end: at("2026-09-27T21:35:00+00:00"),
                executable: Some(
                    r"D:\SteamLibrary\steamapps\common\ELDEN RING\Game\eldenring.exe".into(),
                ),
            },
            SessionRecord {
                game: "Beat Saber".into(),
                start: at("2026-09-28T23:00:00+00:00"),
                end: at("2026-09-29T00:01:00+00:00"),
                executable: None,
            },
        ];
        let active = [ActiveSession {
            game: "Hades".into(),
            start: at("2026-09-29T09:00:00+00:00"),
            last_seen: at("2026-09-29T09:05:00+00:00"),
            executable: Some(r"C:\Games\Hades\Hades.exe".into()),
        }];
        let now = at("2026-09-29T09:20:00+00:00");
        let tz = chrono::FixedOffset::east_opt(0).expect("offset");
        let model = DashboardModel::new(&finished, &active, now);
        let mut history = model.history(&tz);
        history.truncate(3);
        let mut daily = model.daily_totals(None, &tz);
        daily.drain(..daily.len() - 3);
        let snapshot = Response::Dashboard {
            snapshot: Box::new(DashboardSnapshot {
                revision: 42,
                daily,
                history,
                past_week_seconds: 11_460,
                identities: vec![GameIdentity {
                    game: "Elden Ring".into(),
                    source: "Steam".into(),
                    artwork_key: Some("steam_1245620".into()),
                }],
                model,
            }),
        };
        let settings = Response::Settings {
            settings: Box::default(),
            has_steam_grid_db_key: false,
            start_with_windows: true,
            is_installed_copy: true,
        };
        let events: Vec<Response> = vec![
            Event::SessionStarted { game: "Hades".into() },
            Event::SessionEnded {
                session: finished[0].clone(),
            },
            Event::DataChanged { revision: 43 },
            Event::ArtworkReady {
                game: "Hades".into(),
                kind: "cover".into(),
            },
            Event::UpdateAvailable {
                version: "2.3.0".into(),
            },
            Event::Heartbeat,
        ]
        .into_iter()
        .map(|event| Response::Event { event })
        .chain([
            Response::Hello {
                protocol: PROTOCOL_VERSION,
                version: "2.2.0".into(),
            },
            Response::Ok,
            Response::Artwork {
                path: Some(r"C:\Users\me\AppData\Local\Playtime Tracker\Cache\Artwork\steam_1245620\cover.jpg".into()),
            },
            Response::Csv {
                text: "Game,Start\r\n".into(),
            },
            Response::Error {
                message: "nope".into(),
            },
            Response::UpdateStatus {
                current_version: "2.2.0".into(),
                available_version: Some("2.3.0".into()),
                can_install: true,
                busy: false,
                last_error: None,
                last_checked: Some("2026-09-29T09:00:00+00:00".into()),
            },
        ])
        .collect();
        let mut messages = vec![snapshot, settings];
        messages.extend(events);
        let mut actual = String::new();
        for message in &messages {
            let json = serde_json::to_string(message).expect("serializable");
            // The Rust dashboard reads the same types back: every message must round-trip unchanged.
            let back: Response = serde_json::from_str(&json).expect("readable");
            assert_eq!(&back, message, "round trip of {json}");
            actual.push_str(&json);
            actual.push('\n');
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../playtime-dashboard/fixtures/responses.jsonl");
        if std::env::var_os("PT_UPDATE_FIXTURES").is_some() {
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            std::fs::write(&path, &actual).expect("written");
        }
        let expected = std::fs::read_to_string(&path)
            .expect("fixture exists")
            .replace("\r\n", "\n");
        assert_eq!(
            actual, expected,
            "protocol changed: update the dashboard, then the fixture"
        );
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

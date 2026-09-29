//! The tracker's data, in the exact JSON shape the C# app stores inside `sessions.dat`
//! (camelCase: `{"version":1,"sessions":[...],"active":[...]}`), so both versions read each other's files.

use crate::time::Timestamp;
use chrono::Duration;
use serde::{Deserialize, Serialize};

/// One finished play session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub game: String,
    pub start: Timestamp,
    pub end: Timestamp,
    #[serde(default)]
    pub executable: Option<String>,
}

impl SessionRecord {
    pub fn duration(&self) -> Duration {
        self.end.since(self.start)
    }
}

/// A game that is running right now. Saved regularly so a crash or power cut loses at most a minute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSession {
    pub game: String,
    pub start: Timestamp,
    pub last_seen: Timestamp,
    #[serde(default)]
    pub executable: Option<String>,
}

/// Everything the tracker stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerData {
    #[serde(default = "default_version")]
    pub version: i32,
    #[serde(default)]
    pub sessions: Vec<SessionRecord>,
    #[serde(default)]
    pub active: Vec<ActiveSession>,
}

fn default_version() -> i32 {
    1
}

impl Default for TrackerData {
    fn default() -> Self {
        Self {
            version: 1,
            sessions: Vec::new(),
            active: Vec::new(),
        }
    }
}

impl TrackerData {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        // The C# app writes `null` for empty lists in some edge cases; treat it as empty.
        let mut value: serde_json::Value = serde_json::from_str(json)?;
        if let Some(object) = value.as_object_mut() {
            for key in ["sessions", "active"] {
                if object.get(key).is_some_and(serde_json::Value::is_null) {
                    object.remove(key);
                }
            }
        }
        serde_json::from_value(value)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file written by the C# app (System.Text.Json, camelCase, indented).
    const CSHARP_JSON: &str = r#"{
  "version": 1,
  "sessions": [
    {
      "game": "Elden Ring",
      "start": "2026-09-27T14:49:03.5120000+02:00",
      "end": "2026-09-27T17:01:41.9870000+02:00",
      "executable": "D:\\SteamLibrary\\steamapps\\common\\ELDEN RING\\Game\\eldenring.exe"
    },
    { "game": "Cookie Clicker", "start": "2026-09-29T06:58:00+02:00", "end": "2026-09-29T06:58:22+02:00", "executable": null }
  ],
  "active": [
    { "game": "VRChat", "start": "2026-09-29T06:52:00+02:00", "lastSeen": "2026-09-29T06:57:30+02:00", "executable": "C:\\VRChat\\VRChat.exe" }
  ]
}"#;

    #[test]
    fn reads_csharp_file() {
        let data = TrackerData::from_json(CSHARP_JSON).expect("parses");
        assert_eq!(data.sessions.len(), 2);
        assert_eq!(
            data.sessions[0].duration().num_seconds(),
            2 * 3600 + 12 * 60 + 38
        );
        assert_eq!(data.sessions[1].executable, None);
        assert_eq!(data.active[0].game, "VRChat");
    }

    #[test]
    fn round_trips() {
        let data = TrackerData::from_json(CSHARP_JSON).expect("parses");
        let again =
            TrackerData::from_json(&data.to_json().expect("serializes")).expect("parses again");
        assert_eq!(data, again);
    }

    #[test]
    fn null_lists_and_missing_fields_are_empty() {
        let data = TrackerData::from_json(r#"{"sessions":null}"#).expect("parses");
        assert_eq!(data, TrackerData::default());
    }

    #[test]
    fn malformed_is_an_error_not_a_panic() {
        assert!(TrackerData::from_json("{ not json").is_err());
        assert!(TrackerData::from_json(
            r#"{"sessions":[{"game":"x","start":"nope","end":"nope"}]}"#
        )
        .is_err());
    }
}

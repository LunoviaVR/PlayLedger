//! Timestamps compatible with the C# app's `DateTimeOffset` JSON (ISO 8601 with a UTC offset and
//! up to 7 fractional digits, e.g. `2026-09-29T20:15:03.1234567+02:00`).

use chrono::{DateTime, Duration, FixedOffset, Local, SecondsFormat};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A moment in time plus the local UTC offset it was recorded in (like .NET's `DateTimeOffset`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(DateTime<FixedOffset>);

impl Timestamp {
    pub fn now() -> Self {
        Self(Local::now().fixed_offset())
    }

    pub fn from_datetime(value: DateTime<FixedOffset>) -> Self {
        // .NET stores 100 ns ticks; drop anything finer so values round-trip exactly.
        let nanos = value.timestamp_subsec_nanos();
        let trimmed = value - Duration::nanoseconds(i64::from(nanos % 100));
        Self(trimmed)
    }

    pub fn parse(text: &str) -> Result<Self, chrono::ParseError> {
        DateTime::parse_from_rfc3339(text).map(Self::from_datetime)
    }

    pub fn as_datetime(&self) -> DateTime<FixedOffset> {
        self.0
    }

    /// The local calendar date this moment falls on, in the given offset.
    pub fn local_date_in(&self, offset: FixedOffset) -> chrono::NaiveDate {
        self.0.with_timezone(&offset).date_naive()
    }

    pub fn checked_add(&self, duration: Duration) -> Option<Self> {
        self.0.checked_add_signed(duration).map(Self)
    }

    /// `self - earlier` (negative if `earlier` is later).
    pub fn since(&self, earlier: Timestamp) -> Duration {
        self.0.signed_duration_since(earlier.0)
    }

    /// Formats like .NET: 7 fractional digits when there's a fraction, none otherwise.
    pub fn to_dotnet_string(&self) -> String {
        let ticks = self.0.timestamp_subsec_nanos() / 100;
        if ticks == 0 {
            return self.0.to_rfc3339_opts(SecondsFormat::Secs, false);
        }
        let base = self.0.format("%Y-%m-%dT%H:%M:%S").to_string();
        let offset = self.0.format("%:z").to_string();
        format!("{base}.{ticks:07}{offset}")
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_dotnet_string())
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_dotnet_string())
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_dotnet_string())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Timestamp::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_dotnet_format() {
        for text in [
            "2026-09-29T20:15:03.1234567+02:00",
            "2026-09-29T20:15:03+02:00",
            "2026-01-01T00:00:00.0000001-05:00",
        ] {
            let parsed = Timestamp::parse(text).expect("valid");
            assert_eq!(parsed.to_dotnet_string(), text);
        }
    }

    #[test]
    fn utc_z_is_accepted() {
        let parsed = Timestamp::parse("2026-09-29T18:15:03Z").expect("valid");
        assert_eq!(parsed.to_dotnet_string(), "2026-09-29T18:15:03+00:00");
    }

    #[test]
    fn finer_than_ticks_is_truncated() {
        let parsed = Timestamp::parse("2026-09-29T20:15:03.123456789+02:00").expect("valid");
        assert_eq!(
            parsed.to_dotnet_string(),
            "2026-09-29T20:15:03.1234567+02:00"
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(Timestamp::parse("yesterday").is_err());
    }
}

//! Human-readable durations and the spreadsheet (CSV) export, matching the C# app's output.

use crate::model::SessionRecord;
use chrono::Duration;
use std::fmt::Write as _;

/// "2h 05m", "12m 03s", "45s", or "0m" for under a second (same as the C# `ReportWriter.FormatDuration`).
pub fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.num_seconds();
    if duration < Duration::seconds(1) {
        return "0m".into();
    }
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

/// One CSV cell. Values starting with `= + - @` (or tab/CR) are prefixed with `'` so spreadsheets treat them as
/// text rather than formulas ("CSV injection"); game names come from folder names and other programs' files.
pub fn csv_cell(value: &str) -> String {
    let mut value = value.to_string();
    if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        value.insert(0, '\'');
    }
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

/// The `Sessions.csv` export: every session oldest first, local times, with a UTF-8 BOM for Excel.
pub fn sessions_csv(sessions: &[SessionRecord], offset: chrono::FixedOffset) -> String {
    let mut sorted: Vec<&SessionRecord> = sessions.iter().collect();
    sorted.sort_by_key(|s| s.start);
    let mut out = String::from("\u{feff}Game,Start,End,Duration (minutes),Duration,Executable\r\n");
    for s in sorted {
        let local = |t: crate::time::Timestamp| {
            t.as_datetime()
                .with_timezone(&offset)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        };
        let minutes = s.duration().num_milliseconds() as f64 / 60_000.0;
        let _ = write!(
            out,
            "{},{},{},{:.1},{},{}\r\n",
            csv_cell(&s.game),
            csv_cell(&local(s.start)),
            csv_cell(&local(s.end)),
            minutes,
            csv_cell(&format_duration(s.duration())),
            csv_cell(s.executable.as_deref().unwrap_or("")),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Timestamp;

    #[test]
    fn durations() {
        assert_eq!(format_duration(Duration::milliseconds(400)), "0m");
        assert_eq!(format_duration(Duration::seconds(45)), "45s");
        assert_eq!(format_duration(Duration::seconds(12 * 60 + 3)), "12m 03s");
        assert_eq!(
            format_duration(Duration::seconds(2 * 3600 + 5 * 60 + 59)),
            "2h 05m"
        );
        assert_eq!(
            format_duration(Duration::hours(1248) + Duration::minutes(32)),
            "1248h 32m"
        );
    }

    #[test]
    fn csv_cells_are_escaped_and_neutralised() {
        assert_eq!(csv_cell("Elden Ring"), "Elden Ring");
        assert_eq!(
            csv_cell("Baldur's Gate 3, Deluxe"),
            "\"Baldur's Gate 3, Deluxe\""
        );
        assert_eq!(csv_cell("=HYPERLINK(1)"), "'=HYPERLINK(1)");
        assert_eq!(csv_cell("@x\"q"), "\"'@x\"\"q\"");
    }

    #[test]
    fn csv_export() {
        let start = Timestamp::parse("2026-09-27T14:00:00+02:00").expect("valid");
        let end = Timestamp::parse("2026-09-27T15:30:00+02:00").expect("valid");
        let csv = sessions_csv(
            &[SessionRecord {
                game: "+Game".into(),
                start,
                end,
                executable: None,
            }],
            chrono::FixedOffset::east_opt(2 * 3600).expect("offset"),
        );
        assert!(csv.starts_with('\u{feff}'));
        assert!(csv.contains("'+Game,2026-09-27 14:00:00,2026-09-27 15:30:00,90.0,1h 30m,\r\n"));
    }
}

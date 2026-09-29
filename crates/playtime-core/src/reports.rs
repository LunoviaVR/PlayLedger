//! Human-readable durations, the `Game Stats.txt` report and the spreadsheet (CSV) export, matching the C# app's
//! output.

use crate::model::{ActiveSession, SessionRecord};
use crate::paths;
use crate::time::Timestamp;
use chrono::Duration;
use chrono::TimeZone;
use std::collections::HashMap;
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

/// "Tue Sep 29, 2026  9:35 PM" in `tz` (the C# `FormatDateTime`).
fn format_date_time<Tz: TimeZone>(t: Timestamp, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    t.as_datetime()
        .with_timezone(tz)
        .format("%a %b %-d, %Y  %-I:%M %p")
        .to_string()
}

fn format_end<Tz: TimeZone>(start: Timestamp, end: Timestamp, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let (s, e) = (
        start.as_datetime().with_timezone(tz),
        end.as_datetime().with_timezone(tz),
    );
    if s.date_naive() == e.date_naive() {
        e.format("%-I:%M %p").to_string()
    } else {
        format_date_time(end, tz)
    }
}

/// Truncates with "~" or pads to `width` characters.
fn pad(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count > width {
        let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
        out.push('~');
        out
    } else {
        format!("{text}{}", " ".repeat(width - count))
    }
}

/// The `Game Stats.txt` report (without the BOM; the writer adds it). Local times in `tz`.
pub fn stats_text<Tz: TimeZone>(
    sessions: &[SessionRecord],
    active: &[ActiveSession],
    now: Timestamp,
    tz: &Tz,
) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let mut out = String::new();
    let mut line = |text: &str| {
        out.push_str(text);
        out.push_str("\r\n");
    };
    line("PLAYTIME TRACKER");
    line("Generated from your protected play history and rewritten on every save; edits to this file aren't kept.");
    line(&format!("Last updated: {}", format_date_time(now, tz)));
    line("");

    if !active.is_empty() {
        line("NOW PLAYING");
        let mut running: Vec<&ActiveSession> = active.iter().collect();
        running.sort_by_key(|a| a.start);
        for session in running {
            line(&format!(
                "  {} - started {} ({} so far)",
                session.game,
                format_date_time(session.start, tz),
                format_duration(now.since(session.start))
            ));
        }
        line("");
    }

    struct Game<'a> {
        name: &'a str,
        sessions: Vec<&'a SessionRecord>,
        total: Duration,
        last_played: Timestamp,
    }
    let mut groups: HashMap<String, Vec<&SessionRecord>> = HashMap::new();
    for s in sessions {
        groups.entry(paths::key(&s.game)).or_default().push(s);
    }
    let mut games: Vec<Game> = groups
        .into_values()
        .filter_map(|mut list| {
            list.sort_by_key(|s| s.start);
            let newest = *list.last()?;
            Some(Game {
                name: &newest.game,
                total: list
                    .iter()
                    .fold(Duration::zero(), |sum, s| sum + s.duration()),
                last_played: list.iter().map(|s| s.end).max()?,
                sessions: list,
            })
        })
        .collect();
    // Most played first; ties by name so the order is stable.
    games.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.name.cmp(b.name)));

    if games.is_empty() {
        line("No sessions recorded yet. Launch a game and it will show up here once you close it.");
        return out;
    }

    let all_time = games.iter().fold(Duration::zero(), |sum, g| sum + g.total);
    let name_width = games
        .iter()
        .map(|g| g.name.chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, 40);
    line(&format!(
        "SUMMARY  ({} games, {} sessions, {} total)",
        games.len(),
        sessions.len(),
        format_duration(all_time)
    ));
    line(&format!(
        "  {}  {:>12}  {:>12}  {:>10}  Last played",
        pad("Game", name_width),
        "Times played",
        "Total time",
        "Average"
    ));
    line(&format!(
        "  {}  {}  {}  {}  {}",
        "-".repeat(name_width),
        "-".repeat(12),
        "-".repeat(12),
        "-".repeat(10),
        "-".repeat(22)
    ));
    for g in &games {
        let average = g.total / i32::try_from(g.sessions.len()).unwrap_or(i32::MAX);
        line(&format!(
            "  {}  {:>12}  {:>12}  {:>10}  {}",
            pad(g.name, name_width),
            g.sessions.len(),
            format_duration(g.total),
            format_duration(average),
            format_date_time(g.last_played, tz)
        ));
    }

    line("");
    line("SESSIONS BY GAME  (newest first)");
    for g in &games {
        line("");
        line(g.name);
        let count = g.sessions.len();
        line(&format!(
            "  Played {count} {}, {} total",
            if count == 1 { "time" } else { "times" },
            format_duration(g.total)
        ));
        for (i, s) in g.sessions.iter().enumerate().rev() {
            line(&format!(
                "  #{:<4} {}  ->  {}   {}",
                i + 1,
                format_date_time(s.start, tz),
                format_end(s.start, s.end, tz),
                format_duration(s.duration())
            ));
        }
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
    fn stats_report_matches_the_csharp_layout() {
        let tz = chrono::FixedOffset::east_opt(0).expect("offset");
        let at = |text: &str| Timestamp::parse(text).expect("valid");
        let session = |game: &str, start: &str, end: &str| SessionRecord {
            game: game.into(),
            start: at(start),
            end: at(end),
            executable: None,
        };
        let sessions = [
            session(
                "Beat Saber",
                "2026-09-28T20:00:00+00:00",
                "2026-09-28T21:00:00+00:00",
            ),
            session(
                "Beat Saber",
                "2026-09-28T23:00:00+00:00",
                "2026-09-29T00:01:00+00:00",
            ),
            session(
                "Elden Ring",
                "2026-09-27T20:00:00+00:00",
                "2026-09-27T21:35:00+00:00",
            ),
        ];
        let text = stats_text(&sessions, &[], at("2026-09-29T09:00:00+00:00"), &tz);
        let expected = [
            "PLAYTIME TRACKER",
            "Generated from your protected play history and rewritten on every save; edits to this file aren't kept.",
            "Last updated: Tue Sep 29, 2026  9:00 AM",
            "",
            "SUMMARY  (2 games, 3 sessions, 3h 36m total)",
            "  Game        Times played    Total time     Average  Last played",
            "  ----------  ------------  ------------  ----------  ----------------------",
            "  Beat Saber             2        2h 01m      1h 00m  Tue Sep 29, 2026  12:01 AM",
            "  Elden Ring             1        1h 35m      1h 35m  Sun Sep 27, 2026  9:35 PM",
            "",
            "SESSIONS BY GAME  (newest first)",
            "",
            "Beat Saber",
            "  Played 2 times, 2h 01m total",
            "  #2    Mon Sep 28, 2026  11:00 PM  ->  Tue Sep 29, 2026  12:01 AM   1h 01m",
            "  #1    Mon Sep 28, 2026  8:00 PM  ->  9:00 PM   1h 00m",
            "",
            "Elden Ring",
            "  Played 1 time, 1h 35m total",
            "  #1    Sun Sep 27, 2026  8:00 PM  ->  9:35 PM   1h 35m",
            "",
        ]
        .join("\r\n");
        assert_eq!(text, expected);

        let empty = stats_text(&[], &[], at("2026-09-29T09:00:00+00:00"), &tz);
        assert!(empty.ends_with("No sessions recorded yet. Launch a game and it will show up here once you close it.\r\n"));
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

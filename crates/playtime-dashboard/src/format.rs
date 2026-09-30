//! How times and durations read in the dashboard: the same wording as the tray, the reports and the original app.
//! Dates and clock times follow the user's Windows locale (12- or 24-hour clock, local month and day names).

use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone};
use playtime_core::reports::format_duration;
use playtime_core::Timestamp;

/// "2h 05m", "12m 03s", "45s", or "0m" under a second.
pub fn duration(seconds: i64) -> String {
    format_duration(Duration::seconds(seconds.max(0)))
}

/// A running session's time, which counts up every second: like [`duration`], but past an hour it keeps the seconds
/// ("1h 05m 12s") so it visibly moves.
pub fn live_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let hours = seconds / 3600;
    if hours > 0 {
        format!(
            "{hours}h {:02}m {:02}s",
            (seconds % 3600) / 60,
            seconds % 60
        )
    } else {
        duration(seconds)
    }
}

/// A duration for screen readers: "2 hours 5 minutes", "45 seconds".
pub fn spoken_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let (hours, minutes, secs) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    let unit = |n: i64, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(unit(hours, "hour", "hours"));
    }
    if minutes > 0 {
        parts.push(unit(minutes, "minute", "minutes"));
    }
    if hours == 0 && minutes == 0 {
        parts.push(unit(secs, "second", "seconds"));
    }
    parts.join(" ")
}

pub fn local(ts: Timestamp) -> DateTime<Local> {
    ts.as_datetime().with_timezone(&Local)
}

pub fn local_date(ts: Timestamp) -> NaiveDate {
    local(ts).date_naive()
}

/// "Today", "Yesterday", "Mon, Sep 28", or "Sep 28, 2025" for other years.
pub fn day(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        "Today".into()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".into()
    } else if chrono::Datelike::year(&day) == chrono::Datelike::year(&today) {
        locale::weekday_month_day(day)
    } else {
        locale::month_day_year(day)
    }
}

/// [`day`] for a moment, in local time.
pub fn day_of(ts: Timestamp, now: Timestamp) -> String {
    day(local_date(ts), local_date(now))
}

/// "9:05 PM" (or "21:05"), in local time.
pub fn time(ts: Timestamp) -> String {
    locale::time(&local(ts), false)
}

/// "9:05:12 PM" (or "21:05:12"), in local time.
pub fn time_with_seconds(ts: Timestamp) -> String {
    locale::time(&local(ts), true)
}

/// "9:00 PM – 10:35 PM", with the end's day when the session crossed midnight.
pub fn range(start: Timestamp, end: Timestamp, now: Timestamp) -> String {
    let end_text = if local_date(start) == local_date(end) {
        time(end)
    } else {
        format!("{} {}", day_of(end, now), time(end))
    };
    format!("{} – {end_text}", time(start))
}

/// The days of the week in the order the user's locale starts them, with their local names ("Monday", …).
pub fn week() -> Vec<(chrono::Weekday, String)> {
    let first = locale::first_day_of_week().num_days_from_monday();
    (0..7)
        .map(|i| {
            let day =
                chrono::Weekday::try_from(((first + i) % 7) as u8).unwrap_or(chrono::Weekday::Mon);
            (day, locale::day_name(day))
        })
        .collect()
}

/// Midnight today, local time.
pub fn start_of_today(now: Timestamp) -> Timestamp {
    let date = local_date(now);
    let midnight = date.and_hms_opt(0, 0, 0).unwrap_or_default();
    let local = Local
        .from_local_datetime(&midnight)
        .earliest()
        .unwrap_or_else(|| local(now));
    Timestamp::from_datetime(local.fixed_offset())
}

#[cfg(windows)]
mod locale {
    //! Windows formats dates and times in the user's own locale and clock style.
    use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike};
    use windows::core::HSTRING;
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::Globalization::{
        GetDateFormatEx, GetTimeFormatEx, TIME_FORMAT_FLAGS, TIME_NOSECONDS,
    };

    fn system_time(date: NaiveDate, hour: u32, minute: u32, second: u32) -> SYSTEMTIME {
        SYSTEMTIME {
            wYear: date.year() as u16,
            wMonth: date.month() as u16,
            wDayOfWeek: date.weekday().num_days_from_sunday() as u16,
            wDay: date.day() as u16,
            wHour: hour as u16,
            wMinute: minute as u16,
            wSecond: second as u16,
            wMilliseconds: 0,
        }
    }

    fn date(date: NaiveDate, picture: &str, fallback: impl FnOnce() -> String) -> String {
        let st = system_time(date, 0, 0, 0);
        let picture = HSTRING::from(picture);
        let mut buffer = [0u16; 128];
        // SAFETY: `st`, `picture` and `buffer` outlive the call; the user's default locale is used.
        let written = unsafe {
            GetDateFormatEx(
                None,
                Default::default(),
                Some(&st),
                &picture,
                Some(&mut buffer),
                None,
            )
        };
        if written > 1 {
            String::from_utf16_lossy(&buffer[..written as usize - 1])
        } else {
            fallback()
        }
    }

    pub fn weekday_month_day(day: NaiveDate) -> String {
        date(day, "ddd, MMM d", || {
            super::fallback::weekday_month_day(day)
        })
    }

    fn locale_info(kind: u32) -> Option<String> {
        let mut buffer = [0u16; 128];
        // SAFETY: `buffer` outlives the call; the user's default locale is used.
        let written = unsafe {
            windows::Win32::Globalization::GetLocaleInfoEx(None, kind, Some(&mut buffer))
        };
        (written > 1).then(|| String::from_utf16_lossy(&buffer[..written as usize - 1]))
    }

    pub fn day_name(day: chrono::Weekday) -> String {
        use windows::Win32::Globalization::LOCALE_SDAYNAME1;
        // LOCALE_SDAYNAME1 is Monday … LOCALE_SDAYNAME7 is Sunday.
        locale_info(LOCALE_SDAYNAME1 + day.num_days_from_monday())
            .unwrap_or_else(|| super::fallback::day_name(day))
    }

    pub fn first_day_of_week() -> chrono::Weekday {
        use windows::Win32::Globalization::LOCALE_IFIRSTDAYOFWEEK;
        // "0" is Monday … "6" is Sunday.
        locale_info(LOCALE_IFIRSTDAYOFWEEK)
            .and_then(|v| v.trim().parse::<u8>().ok())
            .and_then(|n| chrono::Weekday::try_from(n).ok())
            .unwrap_or_else(super::fallback::first_day_of_week)
    }

    pub fn month_day_year(day: NaiveDate) -> String {
        date(day, "MMM d, yyyy", || super::fallback::month_day_year(day))
    }

    pub fn time(at: &DateTime<Local>, seconds: bool) -> String {
        let st = system_time(at.date_naive(), at.hour(), at.minute(), at.second());
        let flags = if seconds {
            TIME_FORMAT_FLAGS(0)
        } else {
            TIME_NOSECONDS
        };
        let mut buffer = [0u16; 64];
        // SAFETY: `st` and `buffer` outlive the call; the user's default locale and clock style are used.
        let written = unsafe { GetTimeFormatEx(None, flags, Some(&st), None, Some(&mut buffer)) };
        if written > 1 {
            String::from_utf16_lossy(&buffer[..written as usize - 1])
        } else {
            super::fallback::time(at, seconds)
        }
    }
}

/// English formats, for other platforms (tests) and if Windows can't format a value.
mod fallback {
    use chrono::{DateTime, Local, NaiveDate};

    pub fn weekday_month_day(day: NaiveDate) -> String {
        day.format("%a, %b %-d").to_string()
    }

    pub fn month_day_year(day: NaiveDate) -> String {
        day.format("%b %-d, %Y").to_string()
    }

    pub fn day_name(day: chrono::Weekday) -> String {
        [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ][day.num_days_from_monday() as usize]
            .into()
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub fn first_day_of_week() -> chrono::Weekday {
        chrono::Weekday::Sun
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub fn time(at: &DateTime<Local>, seconds: bool) -> String {
        at.format(if seconds { "%-I:%M:%S %p" } else { "%-I:%M %p" })
            .to_string()
    }
}

#[cfg(not(windows))]
use fallback as locale;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_like_the_rest_of_the_app() {
        assert_eq!(duration(0), "0m");
        assert_eq!(duration(45), "45s");
        assert_eq!(duration(12 * 60 + 3), "12m 03s");
        assert_eq!(duration(1248 * 3600 + 32 * 60), "1248h 32m");
        assert_eq!(duration(-5), "0m");
        // A running session keeps its seconds past an hour, so it visibly counts up.
        assert_eq!(live_duration(12 * 60 + 3), "12m 03s");
        assert_eq!(live_duration(3600 + 5 * 60 + 9), "1h 05m 09s");
        assert_eq!(spoken_duration(2 * 3600 + 5 * 60), "2 hours 5 minutes");
        assert_eq!(spoken_duration(3600 + 60), "1 hour 1 minute");
        assert_eq!(spoken_duration(1), "1 second");
    }

    #[test]
    fn days_are_relative_when_recent() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap_or_default();
        assert_eq!(day(today, today), "Today");
        assert_eq!(
            day(today.pred_opt().unwrap_or_default(), today),
            "Yesterday"
        );
        let earlier = NaiveDate::from_ymd_opt(2026, 9, 27).unwrap_or_default();
        let last_year = NaiveDate::from_ymd_opt(2025, 9, 27).unwrap_or_default();
        #[cfg(not(windows))]
        {
            assert_eq!(day(earlier, today), "Sun, Sep 27");
            assert_eq!(day(last_year, today), "Sep 27, 2025");
        }
        #[cfg(windows)]
        {
            assert!(!day(earlier, today).is_empty());
            assert!(day(last_year, today).contains("2025"));
        }
    }
}

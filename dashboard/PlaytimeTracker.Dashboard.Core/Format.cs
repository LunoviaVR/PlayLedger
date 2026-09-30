using System.Globalization;

namespace PlaytimeTracker.Dashboard.Core;

/// <summary>How times and durations read in the dashboard (same wording as the C# app).</summary>
public static class Format
{
    /// <summary>"2h 05m", "12m 03s", "45s", or "0m" under a second.</summary>
    public static string Duration(TimeSpan duration)
    {
        if (duration < TimeSpan.FromSeconds(1))
            return "0m";
        var hours = (long)duration.TotalHours;
        if (hours > 0)
            return $"{hours}h {duration.Minutes:00}m";
        if (duration.Minutes > 0)
            return $"{duration.Minutes}m {duration.Seconds:00}s";
        return $"{duration.Seconds}s";
    }

    public static string Duration(long seconds) => Duration(TimeSpan.FromSeconds(Math.Max(0, seconds)));

    /// <summary>
    /// A running session's time, which counts up every second while the dashboard is open: like
    /// <see cref="Duration(long)"/>, but past an hour it keeps the seconds too ("1h 05m 12s") so it visibly moves.
    /// </summary>
    public static string LiveDuration(long seconds)
    {
        var t = TimeSpan.FromSeconds(Math.Max(0, seconds));
        var hours = (long)t.TotalHours;
        return hours > 0 ? $"{hours}h {t.Minutes:00}m {t.Seconds:00}s" : Duration(t);
    }

    /// <summary>A short, readable duration for screen readers: "2 hours 5 minutes".</summary>
    public static string SpokenDuration(long seconds)
    {
        var t = TimeSpan.FromSeconds(Math.Max(0, seconds));
        var parts = new List<string>();
        var hours = (long)t.TotalHours;
        if (hours > 0)
            parts.Add(hours == 1 ? "1 hour" : $"{hours} hours");
        if (t.Minutes > 0)
            parts.Add(t.Minutes == 1 ? "1 minute" : $"{t.Minutes} minutes");
        if (hours == 0 && t.Minutes == 0)
            parts.Add(t.Seconds == 1 ? "1 second" : $"{t.Seconds} seconds");
        return string.Join(" ", parts);
    }

    /// <summary>"Today", "Yesterday", "Mon, Sep 28", or "Sep 28, 2025" for other years.</summary>
    public static string Day(DateOnly day, DateOnly today, CultureInfo? culture = null)
    {
        culture ??= CultureInfo.CurrentCulture;
        if (day == today)
            return "Today";
        if (day == today.AddDays(-1))
            return "Yesterday";
        return day.Year == today.Year
            ? day.ToString("ddd, MMM d", culture)
            : day.ToString("MMM d, yyyy", culture);
    }

    public static string Day(DateTimeOffset value, DateTimeOffset now) =>
        Day(DateOnly.FromDateTime(value.ToLocalTime().DateTime), DateOnly.FromDateTime(now.ToLocalTime().DateTime));

    public static string Time(DateTimeOffset value, CultureInfo? culture = null) =>
        value.ToLocalTime().ToString("t", culture ?? CultureInfo.CurrentCulture);

    public static string TimeWithSeconds(DateTimeOffset value, CultureInfo? culture = null) =>
        value.ToLocalTime().ToString("T", culture ?? CultureInfo.CurrentCulture);

    /// <summary>"9:00 PM – 10:35 PM", with the end date when the session crossed midnight.</summary>
    public static string Range(DateTimeOffset start, DateTimeOffset end, DateTimeOffset now, CultureInfo? culture = null)
    {
        var sameDay = start.ToLocalTime().Date == end.ToLocalTime().Date;
        var endText = sameDay ? Time(end, culture) : $"{Day(end, now)} {Time(end, culture)}";
        return $"{Time(start, culture)} – {endText}";
    }
}

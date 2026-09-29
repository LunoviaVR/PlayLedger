using System.Globalization;

namespace GameSessionTracker.Ui;

/// <summary>Dates, times and counts as the dashboard writes them.</summary>
internal static class Format
{
    /// <summary>"Today", "Yesterday", "Mon, Sep 28", or "Sep 28, 2025" for other years.</summary>
    public static string Day(DateTimeOffset value) => Day(value.ToLocalTime().Date);

    public static string Day(DateTime localDate)
    {
        var today = DateTime.Now.Date;
        if (localDate == today)
            return "Today";
        if (localDate == today.AddDays(-1))
            return "Yesterday";
        return localDate.Year == today.Year
            ? localDate.ToString("ddd, MMM d", CultureInfo.CurrentCulture)
            : localDate.ToString("MMM d, yyyy", CultureInfo.CurrentCulture);
    }

    /// <summary>"Monday, September 28" (with the year if it isn't this year).</summary>
    public static string LongDay(DateTime localDate) =>
        localDate.ToString(localDate.Year == DateTime.Now.Year ? "dddd, MMMM d" : "dddd, MMMM d, yyyy", CultureInfo.CurrentCulture);

    public static string Time(DateTimeOffset value) => value.ToLocalTime().ToString("h:mm tt", CultureInfo.CurrentCulture);

    public static string TimeWithSeconds(DateTimeOffset value) => value.ToLocalTime().ToString("h:mm:ss tt", CultureInfo.CurrentCulture);

    /// <summary>"8:00 PM – 9:00 PM", "8:00 PM – now", or with the end date when the session crosses midnight.</summary>
    public static string TimeRange(SessionView s)
    {
        var start = s.Start.ToLocalTime();
        var end = s.End.ToLocalTime();
        if (s.IsLive)
            return $"{Time(start)} – now";
        return start.Date == end.Date
            ? $"{Time(start)} – {Time(end)}"
            : $"{Time(start)} – {end.ToString("MMM d, h:mm tt", CultureInfo.CurrentCulture)}";
    }

    public static string Plural(int count, string noun) =>
        $"{count.ToString("N0", CultureInfo.CurrentCulture)} {noun}{(count == 1 ? "" : "s")}";
}

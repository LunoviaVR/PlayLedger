namespace PlaytimeTracker.Dashboard.Core;

/// <summary>
/// Per-game and per-day figures the dashboard computes from the session list (the tracker's snapshot has them for all
/// games together): the same rules as the tracker's view model, in local time. Sessions crossing midnight are split
/// between days; a session with no length (seen by a single poll) counts on its start day.
/// </summary>
public static class SessionMath
{
    private static DateTime Local(DateTimeOffset value, TimeZoneInfo zone) => TimeZoneInfo.ConvertTime(value, zone).DateTime;

    /// <summary>The part of a session on each local calendar day.</summary>
    public static IReadOnlyList<(DateOnly Day, TimeSpan Time)> SplitByDay(DateTimeOffset start, DateTimeOffset end, TimeZoneInfo? zone = null)
    {
        zone ??= TimeZoneInfo.Local;
        var (from, to) = (Local(start, zone), Local(end, zone));
        var parts = new List<(DateOnly, TimeSpan)>();
        for (var day = from.Date; day <= to.Date; day = day.AddDays(1))
        {
            var a = from > day ? from : day;
            var b = to < day.AddDays(1) ? to : day.AddDays(1);
            if (b > a)
                parts.Add((DateOnly.FromDateTime(day), b - a));
        }
        if (parts.Count == 0)
            parts.Add((DateOnly.FromDateTime(from), TimeSpan.Zero));
        return parts;
    }

    public static IEnumerable<SessionView> ForGame(IEnumerable<SessionView> sessions, string? game) =>
        game is null ? sessions : sessions.Where(s => string.Equals(s.Game, game, StringComparison.OrdinalIgnoreCase));

    /// <summary>Playtime per local day for the <paramref name="days"/> days up to today, oldest first.</summary>
    public static IReadOnlyList<DayTotal> DailyTotals(IEnumerable<SessionView> sessions, DateTimeOffset now, int days = 30, TimeZoneInfo? zone = null)
    {
        zone ??= TimeZoneInfo.Local;
        var today = DateOnly.FromDateTime(Local(now, zone));
        var first = today.AddDays(-(days - 1));
        var totals = new double[days];
        foreach (var s in sessions)
        {
            foreach (var (day, time) in SplitByDay(s.Start, s.End, zone))
            {
                if (day >= first && day <= today)
                    totals[day.DayNumber - first.DayNumber] += time.TotalSeconds;
            }
        }
        return totals.Select((t, i) => new DayTotal(first.AddDays(i), (long)t)).ToList();
    }

    /// <summary>Seconds played after <paramref name="since"/>.</summary>
    public static long TotalSince(IEnumerable<SessionView> sessions, DateTimeOffset since) =>
        (long)sessions.Where(s => s.End > since).Sum(s => (s.End - (s.Start > since ? s.Start : since)).TotalSeconds);

    /// <summary>Sessions that were running at some point on a local day, newest first.</summary>
    public static IReadOnlyList<SessionView> On(IEnumerable<SessionView> sessions, DateOnly day, TimeZoneInfo? zone = null) =>
        sessions.Where(s => SplitByDay(s.Start, s.End, zone).Any(p => p.Day == day)).OrderByDescending(s => s.Start).ToList();

    /// <summary>Seconds played on a local day (all sessions given).</summary>
    public static long DayTotal(IEnumerable<SessionView> sessions, DateOnly day, TimeZoneInfo? zone = null) =>
        (long)sessions.SelectMany(s => SplitByDay(s.Start, s.End, zone)).Where(p => p.Day == day).Sum(p => p.Time.TotalSeconds);

    /// <summary>Which session of its game this is, oldest = 1, and how many that game has: "#3 of 12".</summary>
    public static (int Number, int Count) Number(IEnumerable<SessionView> sessions, SessionView session)
    {
        var ofGame = ForGame(sessions, session.Game).OrderBy(s => s.Start).ToList();
        var index = ofGame.FindIndex(s => s.Start == session.Start);
        return (index + 1, ofGame.Count);
    }
}

namespace GameSessionTracker.Ui;

/// <summary>A session as shown in the dashboard; in-progress sessions run up to "now".</summary>
internal sealed record SessionView(string Game, DateTimeOffset Start, DateTimeOffset End, bool IsLive, SessionRecord? Source = null, string? Executable = null)
{
    public TimeSpan Duration => End - Start;

    /// <summary>True if this is the same play session as <paramref name="other"/> (e.g. a live session that has since finished).</summary>
    public bool SameSessionAs(SessionView other) =>
        string.Equals(Game, other.Game, StringComparison.OrdinalIgnoreCase) && Start == other.Start;
}

internal sealed record GameView(string Name, int SessionCount, TimeSpan Total, DateTimeOffset LastPlayed, bool IsLive, DateTimeOffset FirstPlayed)
{
    public TimeSpan Average => SessionCount == 0 ? TimeSpan.Zero : TimeSpan.FromTicks(Total.Ticks / SessionCount);
}

internal sealed record DayTotal(DateTime Day, TimeSpan Total);

/// <summary>One local calendar day of the history: total playtime and how it splits between games (most played first).</summary>
internal sealed record DayHistory(DateTime Day, TimeSpan Total, IReadOnlyList<(string Game, TimeSpan Time)> Games, int SessionCount);

/// <summary>Everything the dashboard shows, computed from the tracker's data at one moment.</summary>
internal sealed class DashboardModel
{
    public const int ChartDays = 30;

    public DateTimeOffset Now { get; }
    public IReadOnlyList<SessionView> Sessions { get; }   // newest first
    public IReadOnlyList<GameView> Games { get; }         // most played first
    public IReadOnlyList<SessionView> Live { get; }

    /// <summary>Changes whenever a session starts or ends; used to avoid rebuilding lists needlessly.</summary>
    public string Signature { get; }

    public DashboardModel(IEnumerable<SessionRecord> finished, IEnumerable<ActiveSession> active, DateTimeOffset now, long dataVersion = 0)
    {
        Now = now;
        var finishedList = finished.ToList();
        var activeList = active.ToList();

        Live = activeList
            .Select(a => new SessionView(a.Game, a.Start, now < a.Start ? a.Start : now, IsLive: true, Executable: a.Executable))
            .OrderBy(s => s.Start)
            .ToList();

        Sessions = finishedList
            .Select(s => new SessionView(s.Game, s.Start, s.End, IsLive: false, s, s.Executable))
            .Concat(Live)
            .OrderByDescending(s => s.Start)
            .ToList();

        Games = Sessions
            .GroupBy(s => s.Game, StringComparer.OrdinalIgnoreCase)
            .Select(g => new GameView(
                g.First().Game, // newest spelling of the name
                g.Count(),
                TimeSpan.FromTicks(g.Sum(s => s.Duration.Ticks)),
                g.Max(s => s.End),
                g.Any(s => s.IsLive),
                g.Min(s => s.Start)))
            .OrderByDescending(g => g.Total)
            .ThenBy(g => g.Name, StringComparer.OrdinalIgnoreCase)
            .ToList();

        Signature = $"{dataVersion}|{finishedList.Count}|{string.Join(",", activeList.Select(a => a.Game + a.Start.Ticks))}";
    }

    public IEnumerable<SessionView> SessionsFor(string? game) =>
        game is null ? Sessions : Sessions.Where(s => string.Equals(s.Game, game, StringComparison.OrdinalIgnoreCase));

    /// <summary>Playtime per local calendar day for the last <see cref="ChartDays"/> days, oldest first.
    /// Sessions that cross midnight are split between the two days.</summary>
    public IReadOnlyList<DayTotal> DailyTotals(string? game)
    {
        var today = Now.ToLocalTime().Date;
        var firstDay = today.AddDays(-(ChartDays - 1));
        var totals = new TimeSpan[ChartDays];

        foreach (var s in SessionsFor(game))
        {
            foreach (var (day, time) in SplitByDay(s))
            {
                if (day >= firstDay && day <= today)
                    totals[(day - firstDay).Days] += time;
            }
        }

        return totals.Select((t, i) => new DayTotal(firstDay.AddDays(i), t)).ToList();
    }

    /// <summary>The last <see cref="ChartDays"/> days, newest first, including days without play.</summary>
    public IReadOnlyList<DayHistory> History()
    {
        var today = Now.ToLocalTime().Date;
        var firstDay = today.AddDays(-(ChartDays - 1));
        var perDay = new Dictionary<DateTime, Dictionary<string, TimeSpan>>();
        var counts = new Dictionary<DateTime, int>();
        foreach (var s in Sessions)
        {
            foreach (var (day, time) in SplitByDay(s))
            {
                if (day < firstDay || day > today)
                    continue;
                if (!perDay.TryGetValue(day, out var games))
                    perDay[day] = games = new Dictionary<string, TimeSpan>(StringComparer.OrdinalIgnoreCase);
                games[s.Game] = games.GetValueOrDefault(s.Game) + time;
                counts[day] = counts.GetValueOrDefault(day) + 1;
            }
        }

        var result = new List<DayHistory>(ChartDays);
        for (var day = today; day >= firstDay; day = day.AddDays(-1))
        {
            var games = perDay.TryGetValue(day, out var g)
                ? g.OrderByDescending(p => p.Value).Select(p => (p.Key, p.Value)).ToList()
                : new List<(string, TimeSpan)>();
            result.Add(new DayHistory(day, TimeSpan.FromTicks(games.Sum(p => p.Item2.Ticks)), games, counts.GetValueOrDefault(day)));
        }
        return result;
    }

    /// <summary>Sessions that overlap the given local day (optionally for one game), newest first.</summary>
    public IEnumerable<SessionView> SessionsOn(DateTime day, string? game = null) =>
        SessionsFor(game).Where(s => SplitByDay(s).Any(p => p.Day == day.Date));

    /// <summary>The part of <paramref name="s"/> that falls on each local calendar day (sessions can cross midnight).</summary>
    public static IEnumerable<(DateTime Day, TimeSpan Time)> SplitByDay(SessionView s)
    {
        var start = s.Start.ToLocalTime().DateTime;
        var end = s.End.ToLocalTime().DateTime;
        for (var day = start.Date; day <= end.Date; day = day.AddDays(1))
        {
            var from = start > day ? start : day;
            var to = end < day.AddDays(1) ? end : day.AddDays(1);
            if (to > from)
                yield return (day, to - from);
        }
    }

    public TimeSpan TotalSince(string? game, DateTimeOffset since) =>
        TimeSpan.FromTicks(SessionsFor(game)
            .Where(s => s.End > since)
            .Sum(s => (s.End - (s.Start > since ? s.Start : since)).Ticks));
}

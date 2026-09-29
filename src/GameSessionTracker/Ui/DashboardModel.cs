namespace GameSessionTracker.Ui;

/// <summary>A session as shown in the dashboard; in-progress sessions run up to "now".</summary>
internal sealed record SessionView(string Game, DateTimeOffset Start, DateTimeOffset End, bool IsLive)
{
    public TimeSpan Duration => End - Start;
}

internal sealed record GameView(string Name, int SessionCount, TimeSpan Total, DateTimeOffset LastPlayed, bool IsLive);

internal sealed record DayTotal(DateTime Day, TimeSpan Total);

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

    public DashboardModel(IEnumerable<SessionRecord> finished, IEnumerable<ActiveSession> active, DateTimeOffset now)
    {
        Now = now;
        var finishedList = finished.ToList();
        var activeList = active.ToList();

        Live = activeList
            .Select(a => new SessionView(a.Game, a.Start, now < a.Start ? a.Start : now, IsLive: true))
            .OrderBy(s => s.Start)
            .ToList();

        Sessions = finishedList
            .Select(s => new SessionView(s.Game, s.Start, s.End, IsLive: false))
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
                g.Any(s => s.IsLive)))
            .OrderByDescending(g => g.Total)
            .ThenBy(g => g.Name, StringComparer.OrdinalIgnoreCase)
            .ToList();

        Signature = $"{finishedList.Count}|{string.Join(",", activeList.Select(a => a.Game + a.Start.Ticks))}";
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
            var start = s.Start.ToLocalTime().DateTime;
            var end = s.End.ToLocalTime().DateTime;
            if (end <= firstDay)
                continue;
            for (var day = start.Date < firstDay ? firstDay : start.Date; day <= end.Date && day <= today; day = day.AddDays(1))
            {
                var from = start > day ? start : day;
                var to = end < day.AddDays(1) ? end : day.AddDays(1);
                if (to > from)
                    totals[(day - firstDay).Days] += to - from;
            }
        }

        return totals.Select((t, i) => new DayTotal(firstDay.AddDays(i), t)).ToList();
    }

    public TimeSpan TotalSince(string? game, DateTimeOffset since) =>
        TimeSpan.FromTicks(SessionsFor(game)
            .Where(s => s.End > since)
            .Sum(s => (s.End - (s.Start > since ? s.Start : since)).Ticks));
}

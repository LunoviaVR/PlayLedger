using PlaytimeTracker.Dashboard.Core;
using Xunit;

namespace PlaytimeTracker.Dashboard.Tests;

public class SessionMathTests
{
    private static readonly TimeZoneInfo Utc = TimeZoneInfo.Utc;

    private static DateTimeOffset At(string text) => DateTimeOffset.Parse(text, System.Globalization.CultureInfo.InvariantCulture);

    private static SessionView Session(string game, string start, string end, bool live = false) =>
        new(game, At(start), At(end), live, null, (long)(At(end) - At(start)).TotalSeconds);

    private static readonly SessionView[] Sessions =
    {
        Session("Hades", "2026-09-29T09:00:00Z", "2026-09-29T09:20:00Z", live: true),
        Session("Beat Saber", "2026-09-28T23:00:00Z", "2026-09-29T00:30:00Z"),
        Session("beat saber", "2026-09-27T10:00:00Z", "2026-09-27T10:10:00Z"),
        Session("Cookie Clicker", "2026-09-26T08:00:00Z", "2026-09-26T08:00:00Z"),
    };

    [Fact]
    public void Splits_across_midnight_and_keeps_zero_length_sessions()
    {
        var parts = SessionMath.SplitByDay(At("2026-09-28T23:00:00Z"), At("2026-09-29T00:30:00Z"), Utc);
        Assert.Equal(new[] { (new DateOnly(2026, 9, 28), TimeSpan.FromHours(1)), (new DateOnly(2026, 9, 29), TimeSpan.FromMinutes(30)) }, parts);
        var blip = SessionMath.SplitByDay(At("2026-09-26T08:00:00Z"), At("2026-09-26T08:00:00Z"), Utc);
        Assert.Equal(new[] { (new DateOnly(2026, 9, 26), TimeSpan.Zero) }, blip);
    }

    [Fact]
    public void Per_game_daily_totals_and_week()
    {
        var now = At("2026-09-29T09:20:00Z");
        var beat = SessionMath.ForGame(Sessions, "BEAT SABER").ToList();
        Assert.Equal(2, beat.Count);
        var days = SessionMath.DailyTotals(beat, now, 30, Utc);
        Assert.Equal(30, days.Count);
        Assert.Equal(new DateOnly(2026, 9, 29), days[^1].Day);
        Assert.Equal(30 * 60, days[^1].Seconds);
        Assert.Equal(60 * 60, days[^2].Seconds);
        Assert.Equal(10 * 60, days[^3].Seconds);
        Assert.Equal((30 + 20) * 60, SessionMath.TotalSince(Sessions, At("2026-09-29T00:00:00Z")));
    }

    [Fact]
    public void Day_lists_totals_and_numbering()
    {
        var sep29 = new DateOnly(2026, 9, 29);
        var on = SessionMath.On(Sessions, sep29, Utc);
        Assert.Equal(new[] { "Hades", "Beat Saber" }, on.Select(s => s.Game));
        Assert.Equal((20 + 30) * 60, SessionMath.DayTotal(Sessions, sep29, Utc));
        Assert.Single(SessionMath.On(Sessions, new DateOnly(2026, 9, 26), Utc));
        Assert.Equal((2, 2), SessionMath.Number(Sessions, Sessions[1]));
        Assert.Equal((1, 2), SessionMath.Number(Sessions, Sessions[2]));
    }
}

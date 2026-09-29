using System.Globalization;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using PlaytimeTracker.Dashboard.Core;
using PlaytimeTracker.Dashboard.ViewModels;

namespace PlaytimeTracker.Dashboard.Pages;

public sealed partial class StatisticsPage : Page
{
    private const int TopCount = 10;

    public StatisticsPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        App.State.SnapshotChanged += Render;
        Render();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e) => App.State.SnapshotChanged -= Render;

    private void Render()
    {
        if (App.State.Snapshot is not { } s)
            return;
        var culture = CultureInfo.CurrentCulture;
        var played = s.History.Where(d => d.TotalSeconds > 0).ToList();
        DaysPlayed.Text = played.Count.ToString(culture);
        PerDay.Text = played.Count == 0 ? "–" : Format.Duration(played.Sum(d => d.TotalSeconds) / played.Count);
        var longest = s.Sessions.OrderByDescending(x => x.Seconds).FirstOrDefault();
        Longest.Text = longest is null ? "–" : Format.Duration(longest.Seconds);
        LongestDetail.Text = longest is null ? "" : $"{longest.Game}, {Format.Day(longest.Start, s.Now)}";
        GameCount.Text = s.Games.Count.ToString(culture);

        var total = s.Games.Sum(g => g.TotalSeconds);
        var top = s.Games.Take(TopCount).ToList();
        var max = top.Count == 0 ? 0 : top.Max(g => g.TotalSeconds);
        TopGames.ItemsSource = top.Select(g => new RankItem(g.Name, g.TotalSeconds, max, total)).ToList();

        // Split every session by local day of week and by part of day.
        var byWeekday = new long[7];
        var byPart = new long[4];
        foreach (var session in s.Sessions)
        {
            var start = session.Start.ToLocalTime();
            var end = session.End.ToLocalTime();
            for (var t = start; t < end;)
            {
                var nextHour = new DateTimeOffset(t.Year, t.Month, t.Day, t.Hour, 0, 0, t.Offset).AddHours(1);
                var until = nextHour < end ? nextHour : end;
                var seconds = (long)(until - t).TotalSeconds;
                byWeekday[(int)t.DayOfWeek] += seconds;
                byPart[t.Hour switch { < 6 => 3, < 12 => 0, < 18 => 1, _ => 2 }] += seconds;
                t = until;
            }
        }
        var firstDay = (int)culture.DateTimeFormat.FirstDayOfWeek;
        var weekdays = Enumerable.Range(0, 7).Select(i => (firstDay + i) % 7).ToList();
        var weekdayMax = byWeekday.Max();
        Weekdays.ItemsSource = weekdays
            .Select(d => new RankItem(culture.DateTimeFormat.GetDayName((DayOfWeek)d), byWeekday[d], weekdayMax, byWeekday.Sum()))
            .ToList();
        var parts = new[] { "Morning (6–12)", "Afternoon (12–18)", "Evening (18–24)", "Night (0–6)" };
        var partMax = byPart.Max();
        DayParts.ItemsSource = parts.Select((name, i) => new RankItem(name, byPart[i], partMax, byPart.Sum())).ToList();
    }
}

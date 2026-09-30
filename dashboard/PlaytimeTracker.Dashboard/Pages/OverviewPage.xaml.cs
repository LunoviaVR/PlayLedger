using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using Microsoft.UI.Xaml.Shapes;
using PlaytimeTracker.Dashboard.Core;
using PlaytimeTracker.Dashboard.ViewModels;

namespace PlaytimeTracker.Dashboard.Pages;

/// <summary>
/// Totals, what's playing, the 30-day chart, every game and every session. Choosing a game shows only that game
/// across the whole page (as in the original dashboard); "All games" goes back.
/// </summary>
public sealed partial class OverviewPage : Page
{
    private const int PageSize = 50;
    private int _shown = PageSize;

    public OverviewPage()
    {
        InitializeComponent();
    }

    /// <summary>The game the page is showing, or null for all games. Also set by the Games page ("Show on Overview").</summary>
    private static string? Game
    {
        get => App.State.OverviewGame;
        set => App.State.OverviewGame = value;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        App.State.SnapshotChanged += Render;
        App.State.TimesChanged += RenderTimes;
        Render();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        App.State.SnapshotChanged -= Render;
        App.State.TimesChanged -= RenderTimes;
    }

    /// <summary>The chosen game (null for all games) and the sessions the page shows.</summary>
    private static (GameView? Game, List<SessionView> Sessions) Scope(DashboardSnapshot s)
    {
        var game = Game is { } g ? s.Games.FirstOrDefault(x => string.Equals(x.Name, g, StringComparison.OrdinalIgnoreCase)) : null;
        return (game, SessionMath.ForGame(s.Sessions, game?.Name).ToList());
    }

    private void Render()
    {
        if (App.State.Snapshot is not { } s)
            return;
        var culture = CultureInfo.CurrentCulture;
        var (game, sessions) = Scope(s);
        if (Game is not null && game is null)
            Game = null; // the game's history was deleted
        var today = DateOnly.FromDateTime(s.Now.ToLocalTime().DateTime);

        PageTitle.Text = game?.Name ?? "Overview";
        AllGames.Visibility = game is null ? Visibility.Collapsed : Visibility.Visible;
        GamesPanel.Visibility = game is null ? Visibility.Visible : Visibility.Collapsed;
        RenderTiles(s, game, sessions);

        var live = sessions.Where(x => x.IsLive).ToList();
        LivePanel.Visibility = live.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        LiveList.ItemsSource = live.Select(l => new SessionItem(l, s.Now)).ToList();

        var daily = game is null ? s.Daily : SessionMath.DailyTotals(sessions, s.Now);
        var busiest = daily.Count == 0 ? 0 : daily.Max(d => d.Seconds);
        RenderChart(daily.Select(d => new DayBar(d, busiest, today)).ToList());

        // Rows built in code: an x:Bind template for them crashes the XAML compiler (WMC9999).
        GamesList.ItemsSource = s.Games.Select(x => new GameRow(x, s.Now))
            .Select(r => Rows.Create(r, r.Name, r.Detail, r.TotalText, r.AutomationName, r.Game.IsLive, strong: true)).ToList();

        var finished = sessions.Where(x => !x.IsLive).ToList();
        SessionsHeader.Text = finished.Count > 0 ? $"Sessions ({finished.Count.ToString(culture)})" : "Sessions";
        SessionsList.ItemsSource = finished.Take(_shown).Select(x => new SessionItem(x, s.Now, showGame: game is null)).ToList();
        ShowMore.Visibility = finished.Count > _shown ? Visibility.Visible : Visibility.Collapsed;
        ShowMore.Content = $"Show more ({(finished.Count - _shown).ToString(culture)} older)";
        EmptyText.Visibility = finished.Count == 0 && live.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>
    /// A second passed while a game runs: update the figures and the running sessions' times in place, so nothing
    /// the user is looking at or has focused is rebuilt. The chart and lists follow on the next real change.
    /// </summary>
    private void RenderTimes()
    {
        if (App.State.Snapshot is not { } s)
            return;
        var (game, sessions) = Scope(s);
        var live = sessions.Where(x => x.IsLive).ToList();
        var rows = LiveList.ItemsSource as List<SessionItem> ?? new List<SessionItem>();
        if (rows.Count != live.Count)
        {
            Render();
            return;
        }
        foreach (var row in rows)
        {
            var current = live.FirstOrDefault(x => x.Start == row.Session.Start
                && string.Equals(x.Game, row.Session.Game, StringComparison.OrdinalIgnoreCase));
            if (current is null)
            {
                Render();
                return;
            }
            row.Update(current);
        }
        RenderTiles(s, game, sessions);
    }

    private void RenderTiles(DashboardSnapshot s, GameView? game, List<SessionView> sessions)
    {
        var culture = CultureInfo.CurrentCulture;
        var todayStart = new DateTimeOffset(s.Now.ToLocalTime().Date, s.Now.ToLocalTime().Offset);

        var total = sessions.Sum(x => x.Seconds);
        var count = sessions.Count;
        TotalText.Text = Format.Duration(total);
        TotalDetail.Text = game is null
            ? (s.Games.Count == 1 ? "1 game" : $"{s.Games.Count.ToString(culture)} games")
            : $"Since {Format.Day(game.FirstPlayed, s.Now)}";
        SessionsText.Text = count.ToString(culture);
        SessionsDetail.Text = count > 0 ? $"{Format.Duration(total / count)} on average" : "";
        WeekText.Text = Format.Duration(game is null ? s.PastWeekSeconds : SessionMath.TotalSince(sessions, todayStart.AddDays(-6)));
        WeekDetail.Text = "Today and the 6 days before";
        if (game is null)
        {
            var top = s.Games.FirstOrDefault();
            FourthCaption.Text = "Most played";
            TopText.Text = top?.Name ?? "–";
            TopDetail.Text = top is null ? "" : Format.Duration(top.TotalSeconds);
        }
        else
        {
            FourthCaption.Text = "Longest session";
            TopText.Text = Format.Duration(game.LongestSeconds);
            TopDetail.Text = game.IsLive ? "Playing now" : $"Last played {Format.Day(game.LastPlayed, s.Now)}";
        }
    }

    private void RenderChart(IReadOnlyList<DayBar> bars)
    {
        Chart.Children.Clear();
        Chart.ColumnDefinitions.Clear();
        Chart.RowDefinitions.Clear();
        Chart.RowDefinitions.Add(new RowDefinition { Height = new GridLength(DayBar.MaxHeight) });
        Chart.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        var accent = (Brush)Application.Current.Resources["AccentFillColorDefaultBrush"];
        var secondary = (Brush)Application.Current.Resources["TextFillColorSecondaryBrush"];
        for (var i = 0; i < bars.Count; i++)
        {
            var bar = bars[i];
            Chart.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            var rectangle = new Rectangle
            {
                Height = bar.Height,
                MaxWidth = 22,
                VerticalAlignment = VerticalAlignment.Bottom,
                RadiusX = 3,
                RadiusY = 3,
                Opacity = bar.Opacity,
                Fill = accent,
            };
            // Each day is a button: hover for its time, select to list its sessions (keyboard and screen readers too).
            var cell = new Button
            {
                Content = rectangle,
                Padding = new Thickness(0),
                BorderThickness = new Thickness(0),
                Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
                HorizontalAlignment = HorizontalAlignment.Stretch,
                VerticalAlignment = VerticalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Stretch, // a Rectangle has no width of its own
                VerticalContentAlignment = VerticalAlignment.Bottom,
                Tag = bar.Day,
            };
            cell.Click += Day_Click;
            ToolTipService.SetToolTip(cell, bar.Tooltip);
            AutomationProperties.SetName(cell, bar.AutomationName);
            Grid.SetColumn(cell, i);
            Chart.Children.Add(cell);

            // Label today and every fifth day before it, so labels never crowd.
            var fromToday = bars.Count - 1 - i;
            if (fromToday % 5 == 0)
            {
                var label = new TextBlock
                {
                    Text = bar.Label,
                    FontSize = 11,
                    Foreground = secondary,
                    Margin = new Thickness(0, 6, 0, 0),
                    TextWrapping = TextWrapping.NoWrap,
                };
                AutomationProperties.SetAccessibilityView(label, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
                // A label spans three columns so it has room ("Today" is wider than one bar); the last one is
                // right-aligned so it stays inside the chart.
                var first = Math.Max(0, i - 1);
                var span = Math.Min(bars.Count, i + 2) - first;
                label.HorizontalAlignment = fromToday == 0 ? HorizontalAlignment.Right : HorizontalAlignment.Center;
                Grid.SetColumn(label, fromToday == 0 ? Math.Max(0, i - 2) : first);
                Grid.SetColumnSpan(label, fromToday == 0 ? Math.Min(3, bars.Count) : span);
                Grid.SetRow(label, 1);
                Chart.Children.Add(label);
            }
        }
    }

    private async void Day_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.Tag is not DateOnly day || App.State.Snapshot is not { } s)
            return;
        var sessions = SessionMath.On(SessionMath.ForGame(s.Sessions, Game), day);
        await Dialogs.ShowDayAsync(XamlRoot, day, sessions, s.Now);
    }

    private void Game_Click(object sender, ItemClickEventArgs e)
    {
        if (Rows.ItemOf<GameRow>(e.ClickedItem) is not { } row)
            return;
        Game = row.Name;
        _shown = PageSize;
        Render();
    }

    private void AllGames_Click(object sender, RoutedEventArgs e)
    {
        Game = null;
        _shown = PageSize;
        Render();
    }

    private void ShowMore_Click(object sender, RoutedEventArgs e)
    {
        _shown += PageSize;
        Render();
    }

    private async void Session_Click(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is SessionItem item && App.State.Snapshot is { } s)
            await Dialogs.ShowSessionAsync(XamlRoot, item.Session, s.Now);
    }
}

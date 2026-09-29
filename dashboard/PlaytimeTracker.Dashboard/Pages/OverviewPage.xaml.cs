using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using PlaytimeTracker.Dashboard.Core;
using PlaytimeTracker.Dashboard.ViewModels;

namespace PlaytimeTracker.Dashboard.Pages;

public sealed partial class OverviewPage : Page
{
    private const int RecentSessions = 25;

    public OverviewPage()
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
        var culture = System.Globalization.CultureInfo.CurrentCulture;
        var total = s.Games.Sum(g => g.TotalSeconds);
        var count = s.Sessions.Count;
        TotalText.Text = Format.Duration(total);
        TotalDetail.Text = s.Games.Count == 1 ? "1 game" : $"{s.Games.Count.ToString(culture)} games";
        SessionsText.Text = count.ToString(culture);
        SessionsDetail.Text = count > 0 ? $"{Format.Duration(total / count)} on average" : "";
        WeekText.Text = Format.Duration(s.PastWeekSeconds);
        WeekDetail.Text = "Today and the 6 days before";
        var top = s.Games.FirstOrDefault();
        TopText.Text = top?.Name ?? "–";
        TopDetail.Text = top is null ? "" : Format.Duration(top.TotalSeconds);

        LivePanel.Visibility = s.Live.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        LiveList.ItemsSource = s.Live.Select(l => new SessionItem(l, s.Now)).ToList();

        var today = DateOnly.FromDateTime(s.Now.ToLocalTime().DateTime);
        var busiest = s.Daily.Count == 0 ? 0 : s.Daily.Max(d => d.Seconds);
        Bars.ItemsSource = s.Daily.Select(d => new DayBar(d, busiest, today)).ToList();

        var recent = s.Sessions.Where(x => !x.IsLive).Take(RecentSessions).Select(x => new SessionItem(x, s.Now)).ToList();
        SessionsList.ItemsSource = recent;
        EmptyText.Visibility = recent.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private async void Session_Click(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is SessionItem item && App.State.Snapshot is { } s)
            await Dialogs.ShowSessionAsync(XamlRoot, item.Session, s.Now);
    }
}

using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using PlaytimeTracker.Dashboard.ViewModels;

namespace PlaytimeTracker.Dashboard.Pages;

public sealed partial class HistoryPage : Page
{
    // Re-render only when the data or the date changed, so expanded days stay open across the periodic refresh.
    private (long Revision, DateOnly Today) _shown;

    public HistoryPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        _shown = default;
        App.State.SnapshotChanged += Render;
        Render();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e) => App.State.SnapshotChanged -= Render;

    private void Render()
    {
        if (App.State.Snapshot is not { } s)
            return;
        var today = DateOnly.FromDateTime(s.Now.ToLocalTime().DateTime);
        if (_shown == (s.Revision, today))
            return;
        _shown = (s.Revision, today);
        var busiest = s.History.Count == 0 ? 0 : s.History.Max(d => d.TotalSeconds);
        Days.ItemsSource = s.History.Select(d => new DayItem(d, busiest, s, today)).ToList();
    }

    private async void Session_Click(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is SessionItem item && App.State.Snapshot is { } s)
            await Dialogs.ShowSessionAsync(XamlRoot, item.Session, s.Now);
    }
}

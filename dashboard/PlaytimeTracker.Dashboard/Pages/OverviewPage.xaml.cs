using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
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
        RenderChart(s.Daily.Select(d => new DayBar(d, busiest, today)).ToList());

        var recent = s.Sessions.Where(x => !x.IsLive).Take(RecentSessions).Select(x => new SessionItem(x, s.Now)).ToList();
        SessionsList.ItemsSource = recent;
        EmptyText.Visibility = recent.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
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
            // A transparent cell makes the whole column hoverable for the tooltip, not just the bar.
            var cell = new Grid { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
            cell.Children.Add(rectangle);
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
                    HorizontalAlignment = HorizontalAlignment.Center,
                    Margin = new Thickness(0, 6, 0, 0),
                    TextWrapping = TextWrapping.NoWrap,
                };
                AutomationProperties.SetAccessibilityView(label, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
                Grid.SetColumn(label, i);
                Grid.SetRow(label, 1);
                Chart.Children.Add(label);
            }
        }
    }

    private async void Session_Click(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is SessionItem item && App.State.Snapshot is { } s)
            await Dialogs.ShowSessionAsync(XamlRoot, item.Session, s.Now);
    }
}

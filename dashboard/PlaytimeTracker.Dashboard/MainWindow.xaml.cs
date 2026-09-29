using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PlaytimeTracker.Dashboard.Core;
using PlaytimeTracker.Dashboard.Pages;
using Windows.Graphics;

namespace PlaytimeTracker.Dashboard;

public sealed partial class MainWindow : Window
{
    private static readonly Dictionary<string, Type> Pages = new(StringComparer.OrdinalIgnoreCase)
    {
        ["overview"] = typeof(OverviewPage),
        ["games"] = typeof(GamesPage),
        ["history"] = typeof(HistoryPage),
        ["statistics"] = typeof(StatisticsPage),
        ["settings"] = typeof(SettingsPage),
    };

    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer _tick;

    public MainWindow(string? page)
    {
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(TitleBar);
        AppWindow.Resize(new SizeInt32(1180, 800));
        var icon = Path.Combine(AppContext.BaseDirectory, "app.ico");
        if (File.Exists(icon))
            AppWindow.SetIcon(icon);

        var state = App.State;
        state.Attach(DispatcherQueue);
        state.ConnectionStateChanged += ShowConnectionState;
        state.ErrorReported += message => ShowStatus(InfoBarSeverity.Error, "Something went wrong", message, showStart: false);
        state.SnapshotChanged += ApplyTheme;

        Navigate(page ?? "overview");
        _ = state.RefreshAsync();

        // Live sessions count up; refresh the numbers every 30 seconds while the window is open.
        _tick = DispatcherQueue.CreateTimer();
        _tick.Interval = TimeSpan.FromSeconds(30);
        _tick.Tick += (_, _) => _ = state.RefreshAsync();
        _tick.Start();
        Closed += (_, _) => _tick.Stop();
    }

    public void BringToFront(string? page)
    {
        if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized } presenter)
            presenter.Restore();
        AppWindow.Show();
        Activate();
        if (page is not null)
            Navigate(page);
    }

    private void Navigate(string tag)
    {
        if (!Pages.TryGetValue(tag, out var type))
            return;
        if (ContentFrame.CurrentSourcePageType != type)
            ContentFrame.Navigate(type);
        Nav.SelectedItem = tag == "settings"
            ? Nav.SettingsItem
            : Nav.MenuItems.OfType<NavigationViewItem>().FirstOrDefault(i => (string)i.Tag == tag);
    }

    private void Nav_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        var tag = args.IsSettingsSelected ? "settings" : (args.SelectedItem as NavigationViewItem)?.Tag as string;
        if (tag is not null)
            Navigate(tag);
    }

    private void ShowConnectionState()
    {
        var state = App.State;
        if (state.Connected)
        {
            if (Status.Severity == InfoBarSeverity.Warning)
                Status.IsOpen = false;
            return;
        }
        ShowStatus(InfoBarSeverity.Warning, "Not connected", state.ConnectionProblem ?? "Playtime Tracker isn't running.",
            showStart: File.Exists(TrackerClient.TrackerPath));
    }

    private void ShowStatus(InfoBarSeverity severity, string title, string message, bool showStart)
    {
        Status.Severity = severity;
        Status.Title = title;
        Status.Message = message;
        StatusAction.Visibility = showStart ? Visibility.Visible : Visibility.Collapsed;
        Status.IsOpen = true;
    }

    private async void StatusAction_Click(object sender, RoutedEventArgs e)
    {
        if (!TrackerClient.StartTracker())
            return;
        StatusAction.IsEnabled = false;
        // Give the tracker a moment to open its connection.
        await Task.Delay(TimeSpan.FromSeconds(2));
        StatusAction.IsEnabled = true;
        await App.State.RefreshAsync();
    }

    /// <summary>Follows Settings → Theme (system, light or dark) and the accent colour.</summary>
    private async void ApplyTheme()
    {
        try
        {
            var settings = await App.State.Client.GetSettingsAsync();
            Root.RequestedTheme = settings.Settings.ThemeMode switch
            {
                "dark" => ElementTheme.Dark,
                "light" => ElementTheme.Light,
                _ => ElementTheme.Default,
            };
            AccentTheme.Apply(settings.Settings.AccentColor, Root);
            App.State.SnapshotChanged -= ApplyTheme; // once is enough; the Settings page applies changes directly
        }
        catch (Exception ex) when (ex is TrackerUnavailableException or TrackerErrorException)
        {
        }
    }

    /// <summary>The Settings page calls this when the accent changes.</summary>
    public static void SetAccent(FrameworkElement anyElement, string accent) =>
        AccentTheme.Apply(accent, anyElement.XamlRoot?.Content as FrameworkElement);

    /// <summary>The Settings page calls this when the theme changes.</summary>
    public static void SetTheme(FrameworkElement anyElement, string mode)
    {
        if (anyElement.XamlRoot?.Content is FrameworkElement root)
        {
            root.RequestedTheme = mode switch
            {
                "dark" => ElementTheme.Dark,
                "light" => ElementTheme.Light,
                _ => ElementTheme.Default,
            };
        }
    }
}

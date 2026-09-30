using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using PlaytimeTracker.Dashboard.Core;

namespace PlaytimeTracker.Dashboard;

/// <summary>Session and game details, and confirmations before anything is deleted.</summary>
public static class Dialogs
{
    private static Grid Details(IEnumerable<(string Label, string Value)> rows) => Details(rows, out _);

    private static Grid Details(IEnumerable<(string Label, string Value)> rows, out List<TextBlock> values)
    {
        values = new List<TextBlock>();
        var grid = new Grid { ColumnSpacing = 24, RowSpacing = 8 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var row = 0;
        foreach (var (label, value) in rows)
        {
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var name = new TextBlock { Text = label, Style = (Style)Application.Current.Resources["CaptionStyle"] };
            var text = new TextBlock { Text = value, TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true };
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(text, $"{label}: {value}");
            Grid.SetRow(name, row);
            Grid.SetRow(text, row);
            Grid.SetColumn(text, 1);
            grid.Children.Add(name);
            grid.Children.Add(text);
            values.Add(text);
            row++;
        }
        return grid;
    }

    private static string When(DateTimeOffset value, DateTimeOffset now) =>
        $"{Format.Day(value, now)} at {Format.TimeWithSeconds(value)}";

    private static async Task<bool> ConfirmAsync(XamlRoot root, string title, string message, string action)
    {
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = title,
            Content = new TextBlock { Text = message, TextWrapping = TextWrapping.Wrap },
            PrimaryButtonText = action,
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
        };
        return await dialog.ShowAsync() == ContentDialogResult.Primary;
    }

    /// <summary>When a session was opened and closed, how long it ran, and which program; finished ones can be deleted.</summary>
    public static async Task ShowSessionAsync(XamlRoot root, SessionView session, DateTimeOffset now)
    {
        var all = App.State.Snapshot?.Sessions ?? (IReadOnlyList<SessionView>)new[] { session };
        var (number, count) = SessionMath.Number(all, session);
        var day = DateOnly.FromDateTime(session.Start.ToLocalTime().DateTime);
        var gameTotal = SessionMath.ForGame(all, session.Game).Sum(s => s.Seconds);
        var rows = new List<(string, string)>
        {
            ("Opened", When(session.Start, now)),
            ("Closed", session.IsLive ? "Still running" : When(session.End, now)),
            (session.IsLive ? "Running for" : "Played for", Format.Duration(session.Seconds)),
            ("Session", count > 0 ? $"#{number} of {count}" : "–"),
            ($"{session.Game} total", Format.Duration(gameTotal)),
            ($"{Format.Day(day, DateOnly.FromDateTime(now.ToLocalTime().DateTime))} total", Format.Duration(SessionMath.DayTotal(all, day))),
        };
        var exe = session.Executable;
        if (!string.IsNullOrEmpty(exe))
            rows.Add(("Program", exe));

        var content = Details(rows, out var values);
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = session.Game,
            Content = content,
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
        };
        if (!session.IsLive)
            dialog.SecondaryButtonText = "Delete session";
        if (exe is not null && File.Exists(exe))
            dialog.PrimaryButtonText = "Show program";

        // A running session keeps counting up while its details are open.
        Microsoft.UI.Dispatching.DispatcherQueueTimer? tick = null;
        if (session.IsLive)
        {
            tick = content.DispatcherQueue.CreateTimer();
            tick.Interval = TimeSpan.FromSeconds(1);
            tick.Tick += (_, _) => values[2].Text = Format.Duration((long)(DateTimeOffset.Now - session.Start).TotalSeconds);
            tick.Start();
        }
        var result = await dialog.ShowAsync();
        tick?.Stop();

        if (result == ContentDialogResult.Primary && exe is not null)
        {
            // Opens the program's folder with it selected. Explorer wants `/select,"path"` as one token; Windows paths
            // can't contain quotes, so the recorded path can't break out of it.
            if (!exe.Contains('"'))
            {
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo("explorer.exe", $"/select,\"{exe}\"")
                {
                    UseShellExecute = false,
                })?.Dispose();
            }
            return;
        }
        if (result != ContentDialogResult.Secondary)
            return;
        if (await ConfirmAsync(root, "Delete this session?",
                $"The {Format.Duration(session.Seconds)} session of {session.Game} on {Format.Day(session.Start, now)} will be removed from your history. This can't be undone.",
                "Delete"))
        {
            if (await App.State.RunAsync(c => c.DeleteSessionAsync(session.Game, session.Start)))
                await App.State.RefreshAsync();
        }
    }

    /// <summary>One day's sessions (from a chart bar); choosing one opens its details.</summary>
    public static async Task ShowDayAsync(XamlRoot root, DateOnly day, IReadOnlyList<SessionView> sessions, DateTimeOffset now)
    {
        var today = DateOnly.FromDateTime(now.ToLocalTime().DateTime);
        var list = new ListView
        {
            SelectionMode = ListViewSelectionMode.None,
            IsItemClickEnabled = true,
            MaxHeight = 360,
            ItemsSource = sessions.Select(s => new ViewModels.SessionItem(s, now)).ToList(),
            ItemTemplate = (DataTemplate)Application.Current.Resources["DialogSessionTemplate"],
        };
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(list, $"Sessions on {Format.Day(day, today)}");
        var panel = new StackPanel { Spacing = 8, MinWidth = 420 };
        panel.Children.Add(new TextBlock
        {
            Text = sessions.Count == 0
                ? "No play this day."
                : $"{Format.Duration(SessionMath.DayTotal(sessions, day))} in {(sessions.Count == 1 ? "1 session" : $"{sessions.Count} sessions")}",
            Style = (Style)Application.Current.Resources["CaptionStyle"],
        });
        panel.Children.Add(list);
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = Format.Day(day, today),
            Content = panel,
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
        };
        SessionView? chosen = null;
        list.ItemClick += (_, e) =>
        {
            chosen = (e.ClickedItem as ViewModels.SessionItem)?.Session;
            dialog.Hide();
        };
        await dialog.ShowAsync();
        // Only one dialog can be open at a time, so the details open after the day closes.
        if (chosen is not null)
            await ShowSessionAsync(root, chosen, now);
    }

    /// <summary>A game's totals, with Stop tracking and Delete history.</summary>
    public static async Task ShowGameAsync(XamlRoot root, GameView game, DashboardSnapshot snapshot)
    {
        var identity = snapshot.IdentityOf(game.Name);
        var rows = new List<(string, string)>
        {
            ("Total", Format.Duration(game.TotalSeconds)),
            ("Sessions", game.SessionCount.ToString(System.Globalization.CultureInfo.CurrentCulture)),
            ("Average", Format.Duration(game.AverageSeconds)),
            ("Longest", Format.Duration(game.LongestSeconds)),
            ("First played", Format.Day(game.FirstPlayed, snapshot.Now)),
            ("Last played", game.IsLive ? "Playing now" : Format.Day(game.LastPlayed, snapshot.Now)),
        };
        if (identity is not null)
            rows.Add(("Found through", identity.Source));

        var content = new StackPanel { Spacing = 16 };
        content.Children.Add(Details(rows));
        var show = new HyperlinkButton { Content = "Show on Overview", Padding = new Thickness(0) };
        content.Children.Add(show);
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = game.Name,
            Content = content,
            PrimaryButtonText = "Stop tracking",
            SecondaryButtonText = "Delete history",
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
        };
        show.Click += (_, _) =>
        {
            App.State.OverviewGame = game.Name;
            dialog.Hide();
            (App.CurrentWindow as MainWindow)?.BringToFront("overview");
        };
        switch (await dialog.ShowAsync())
        {
            case ContentDialogResult.Primary:
                if (await ConfirmAsync(root, $"Stop tracking {game.Name}?",
                        "It will be added to Settings → Ignored games, where you can track it again. Its history is kept.",
                        "Stop tracking"))
                    await App.State.RunAsync(c => c.SetGameIgnoredAsync(game.Name, true));
                break;
            case ContentDialogResult.Secondary:
                if (await ConfirmAsync(root, $"Delete all history of {game.Name}?",
                        $"{game.SessionCount} sessions ({Format.Duration(game.TotalSeconds)}) will be removed. This can't be undone.",
                        "Delete history"))
                {
                    if (await App.State.RunAsync(c => c.DeleteGameHistoryAsync(game.Name)))
                        await App.State.RefreshAsync();
                }
                break;
        }
    }

    /// <summary>A short message with an OK button.</summary>
    public static async Task InformAsync(XamlRoot root, string title, string message)
    {
        var paragraph = new Paragraph();
        paragraph.Inlines.Add(new Run { Text = message });
        var text = new RichTextBlock { TextWrapping = TextWrapping.Wrap };
        text.Blocks.Add(paragraph);
        await new ContentDialog { XamlRoot = root, Title = title, Content = text, CloseButtonText = "OK" }.ShowAsync();
    }
}

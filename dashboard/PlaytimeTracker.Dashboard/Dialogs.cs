using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using PlaytimeTracker.Dashboard.Core;

namespace PlaytimeTracker.Dashboard;

/// <summary>Session and game details, and confirmations before anything is deleted.</summary>
public static class Dialogs
{
    private static Grid Details(IEnumerable<(string Label, string Value)> rows)
    {
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
        var rows = new List<(string, string)>
        {
            ("Opened", When(session.Start, now)),
            ("Closed", session.IsLive ? "Still running" : When(session.End, now)),
            (session.IsLive ? "Running for" : "Played for", Format.Duration(session.Seconds)),
        };
        if (!string.IsNullOrEmpty(session.Executable))
            rows.Add(("Program", session.Executable));

        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = session.Game,
            Content = Details(rows),
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
        };
        if (!session.IsLive)
            dialog.SecondaryButtonText = "Delete session";
        if (await dialog.ShowAsync() != ContentDialogResult.Secondary)
            return;
        if (await ConfirmAsync(root, "Delete this session?",
                $"The {Format.Duration(session.Seconds)} session of {session.Game} on {Format.Day(session.Start, now)} will be removed from your history. This can't be undone.",
                "Delete"))
        {
            if (await App.State.RunAsync(c => c.DeleteSessionAsync(session.Game, session.Start)))
                await App.State.RefreshAsync();
        }
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

        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = game.Name,
            Content = Details(rows),
            PrimaryButtonText = "Stop tracking",
            SecondaryButtonText = "Delete history",
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
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

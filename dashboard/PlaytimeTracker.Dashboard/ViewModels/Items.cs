using System.ComponentModel;
using System.Runtime.CompilerServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using PlaytimeTracker.Dashboard.Core;

namespace PlaytimeTracker.Dashboard.ViewModels;

/// <summary>A session row.</summary>
public sealed class SessionItem
{
    public SessionItem(SessionView session, DateTimeOffset now, bool showGame = true)
    {
        Session = session;
        Game = showGame ? session.Game : "";
        Day = Format.Day(session.Start, now);
        Range = session.IsLive ? $"Since {Format.Time(session.Start)}" : Format.Range(session.Start, session.End, now);
        DurationText = session.IsLive ? $"{Format.Duration(session.Seconds)} so far" : Format.Duration(session.Seconds);
        LiveVisibility = session.IsLive ? Visibility.Visible : Visibility.Collapsed;
        AutomationName = session.IsLive
            ? $"{session.Game}, playing now, {Format.SpokenDuration(session.Seconds)} so far"
            : $"{session.Game}, {Day}, {Range}, {Format.SpokenDuration(session.Seconds)}";
    }

    public SessionView Session { get; }
    public string Game { get; }
    public string Day { get; }
    public string Range { get; }
    public string DurationText { get; }
    public Visibility LiveVisibility { get; }
    public string AutomationName { get; }
}

/// <summary>A game tile; its artwork loads after the tile appears.</summary>
public sealed class GameItem : INotifyPropertyChanged
{
    private ImageSource? _cover;
    private Stretch _coverStretch = Stretch.UniformToFill;

    public GameItem(GameView game, DashboardSnapshot snapshot)
    {
        Game = game;
        Name = game.Name;
        TotalText = Format.Duration(game.TotalSeconds);
        var sessions = game.SessionCount == 1 ? "1 session" : $"{game.SessionCount} sessions";
        Detail = game.IsLive ? $"Playing now · {sessions}" : $"{sessions} · {Format.Day(game.LastPlayed, snapshot.Now)}";
        Initial = string.IsNullOrEmpty(game.Name) ? "?" : game.Name[..1].ToUpperInvariant();
        Source = snapshot.IdentityOf(game.Name)?.Source ?? "";
        LiveVisibility = game.IsLive ? Visibility.Visible : Visibility.Collapsed;
        AutomationName = $"{game.Name}, {Format.SpokenDuration(game.TotalSeconds)} played, {sessions}{(game.IsLive ? ", playing now" : "")}";
    }

    public GameView Game { get; }
    public string Name { get; }
    public string TotalText { get; }
    public string Detail { get; }
    public string Initial { get; }
    public string Source { get; }
    public Visibility LiveVisibility { get; }
    public string AutomationName { get; }

    public ImageSource? Cover
    {
        get => _cover;
        private set { _cover = value; Changed(); Changed(nameof(InitialVisibility)); }
    }

    public Stretch CoverStretch
    {
        get => _coverStretch;
        private set { _coverStretch = value; Changed(); }
    }

    public Visibility InitialVisibility => _cover is null ? Visibility.Visible : Visibility.Collapsed;

    /// <summary>Box art if there is any, else the game's icon (centered, not cropped).</summary>
    public async Task LoadArtworkAsync()
    {
        var cover = await App.State.ArtworkAsync(Name, ArtworkKinds.Cover);
        if (cover is not null && File.Exists(cover))
        {
            CoverStretch = Stretch.UniformToFill;
            Cover = new BitmapImage(new Uri(cover));
            return;
        }
        var icon = await App.State.ArtworkAsync(Name, ArtworkKinds.Icon);
        if (icon is not null && File.Exists(icon))
        {
            CoverStretch = Stretch.None;
            Cover = new BitmapImage(new Uri(icon)) { DecodePixelWidth = 96 };
        }
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    private void Changed([CallerMemberName] string? name = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}

/// <summary>One bar of the daily chart.</summary>
public sealed class DayBar
{
    public const double MaxHeight = 140;

    public DayBar(DayTotal day, long busiest, DateOnly today)
    {
        Day = day.Day;
        Height = busiest <= 0 || day.Seconds <= 0 ? 2 : Math.Max(4, MaxHeight * day.Seconds / busiest);
        Label = day.Day == today ? "Today" : day.Day.Day.ToString(System.Globalization.CultureInfo.CurrentCulture);
        Tooltip = $"{Format.Day(day.Day, today)}: {Format.Duration(day.Seconds)}";
        AutomationName = $"{Format.Day(day.Day, today)}, {Format.SpokenDuration(day.Seconds)}";
        Opacity = day.Seconds > 0 ? 1.0 : 0.35;
    }

    public DateOnly Day { get; }
    public double Height { get; }
    public string Label { get; }
    public string Tooltip { get; }
    public string AutomationName { get; }
    public double Opacity { get; }
}

/// <summary>A game in the Overview's Games list.</summary>
public sealed class GameRow
{
    public GameRow(GameView game, DateTimeOffset now)
    {
        Game = game;
        Name = game.Name;
        TotalText = Format.Duration(game.TotalSeconds);
        var sessions = game.SessionCount == 1 ? "1 session" : $"{game.SessionCount} sessions";
        Detail = game.IsLive ? $"{sessions} · playing now" : $"{sessions} · last played {Format.Day(game.LastPlayed, now)}";
        AutomationName = $"{game.Name}, {Format.SpokenDuration(game.TotalSeconds)}, {Detail}";
    }

    public GameView Game { get; }
    public string Name { get; }
    public string TotalText { get; }
    public string Detail { get; }
    public string AutomationName { get; }
}

/// <summary>A day on the History page.</summary>
public sealed class DayItem
{
    public const double BarMaxWidth = 220;

    public DayItem(DayHistory day, long busiest, DashboardSnapshot snapshot, DateOnly today)
    {
        Day = day.Day;
        DayText = Format.Day(day.Day, today);
        // A day with sessions was played, even if they were under a second.
        TotalText = day.TotalSeconds > 0 || day.SessionCount > 0 ? Format.Duration(day.TotalSeconds) : "No play";
        GamesText = day.Games.Count == 0
            ? ""
            : string.Join(", ", day.Games.Take(3).Select(g => $"{g.Game} {Format.Duration(g.Seconds)}"))
              + (day.Games.Count > 3 ? $" and {day.Games.Count - 3} more" : "");
        BarWidth = busiest <= 0 ? 0 : BarMaxWidth * day.TotalSeconds / busiest;
        Sessions = snapshot.Sessions
            .Where(s => Overlaps(s, day.Day))
            .Select(s => new SessionItem(s, snapshot.Now))
            .ToList();
        HasSessions = Sessions.Count > 0;
        AutomationName = $"{DayText}, {(day.SessionCount > 0 ? Format.SpokenDuration(day.TotalSeconds) : "no play")}";
    }

    private static bool Overlaps(SessionView s, DateOnly day)
    {
        var start = DateOnly.FromDateTime(s.Start.ToLocalTime().DateTime);
        var end = DateOnly.FromDateTime(s.End.ToLocalTime().DateTime);
        return start <= day && day <= end;
    }

    public DateOnly Day { get; }
    public string DayText { get; }
    public string TotalText { get; }
    public string GamesText { get; }
    public double BarWidth { get; }
    public IReadOnlyList<SessionItem> Sessions { get; }
    public bool HasSessions { get; }
    public string AutomationName { get; }
}

/// <summary>A labelled bar (Statistics page).</summary>
public sealed class RankItem
{
    public const double BarMaxWidth = 320;

    public RankItem(string name, long seconds, long max, long total)
    {
        Name = name;
        ValueText = Format.Duration(seconds);
        PercentText = total > 0 ? $"{100.0 * seconds / total:0}%" : "";
        BarWidth = max <= 0 ? 0 : Math.Max(seconds > 0 ? 3 : 0, BarMaxWidth * seconds / max);
        AutomationName = $"{name}, {Format.SpokenDuration(seconds)}{(PercentText.Length > 0 ? $", {PercentText}" : "")}";
    }

    public string Name { get; }
    public string ValueText { get; }
    public string PercentText { get; }
    public double BarWidth { get; }
    public string AutomationName { get; }
}

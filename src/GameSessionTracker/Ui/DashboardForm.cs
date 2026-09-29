using System.Drawing.Drawing2D;
using System.Globalization;
using System.Reflection;

namespace GameSessionTracker.Ui;

/// <summary>The main window: all play data in one place. Closing it leaves the tracker running in the tray.</summary>
internal sealed class DashboardForm : Form
{
    private const string AllGames = "\0all"; // sentinel item for the "All games" row

    private readonly Func<DashboardModel> _getModel;
    private readonly Action _openDataFolder;
    private readonly System.Windows.Forms.Timer _refreshTimer;

    private readonly HeaderBar _header = new();
    private readonly StatTile[] _tiles = { new(), new(), new(), new() };
    private readonly DailyChart _chart = new();
    private readonly Card _gamesCard = new() { Title = "Games" };
    private readonly Card _sessionsCard = new() { Title = "Sessions" };
    private readonly RowList _gamesList = new();
    private readonly RowList _sessionsList = new() { SelectionMode = SelectionMode.None };
    private readonly FooterBar _footer = new()
    {
        Message = "Tracking continues in the system tray when you close this window.",
        LinkText = "Open data folder",
    };

    private Theme _theme = Theme.Current();
    private Fonts _fonts;
    private DashboardModel _model;
    private string? _selectedGame; // null = all games
    private bool _sized;
    private List<SessionView> _visibleSessions = new();

    public DashboardForm(Func<DashboardModel> getModel, Action openDataFolder)
    {
        _getModel = getModel;
        _openDataFolder = openDataFolder;
        _model = getModel();
        _fonts = new Fonts(DeviceDpi / 96f);

        Text = "Game Session Tracker";
        AutoScaleMode = AutoScaleMode.None; // layout and fonts are scaled by hand in ApplyMetrics
        StartPosition = FormStartPosition.CenterScreen;
        DoubleBuffered = true;
        KeyPreview = true;
        using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("GameSessionTracker.app.ico"))
        {
            if (stream is not null)
                Icon = new Icon(stream);
        }

        _gamesCard.Controls.Add(_gamesList);
        _sessionsCard.Controls.Add(_sessionsList);
        _gamesList.Dock = DockStyle.Fill;
        _sessionsList.Dock = DockStyle.Fill;
        Controls.Add(_header);
        Controls.AddRange(_tiles);
        Controls.Add(_chart);
        Controls.Add(_gamesCard);
        Controls.Add(_sessionsCard);
        Controls.Add(_footer);

        _gamesList.DrawRow += DrawGameRow;
        _sessionsList.DrawRow += DrawSessionRow;
        _sessionsList.EmptyText = "Your sessions will appear here after you play a game.";
        _gamesList.SelectedIndexChanged += (_, _) => OnGameSelected();

        _footer.LinkClicked += () => _openDataFolder();

        KeyDown += (_, e) => { if (e.KeyCode == Keys.Escape) Close(); };
        DpiChanged += (_, _) => ApplyMetrics();

        ApplyTheme();
        ApplyMetrics();
        RebuildLists();
        UpdateSummary();

        // Live durations tick while a game is running; lists only rebuild when a session starts or ends.
        _refreshTimer = new System.Windows.Forms.Timer { Interval = 1000 };
        _refreshTimer.Tick += (_, _) => RefreshData();
        _refreshTimer.Start();
    }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        Theme.ApplyWindowChrome(this, _theme.IsDark);
        Theme.ApplyScrollbarTheme(_gamesList, _theme.IsDark);
        Theme.ApplyScrollbarTheme(_sessionsList, _theme.IsDark);
    }

    protected override void OnFormClosed(FormClosedEventArgs e)
    {
        _refreshTimer.Stop();
        _refreshTimer.Dispose();
        base.OnFormClosed(e);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            _fonts.Dispose();
        base.Dispose(disposing);
    }

    /// <summary>Called by the tray when something changed, and by the timer.</summary>
    public void RefreshData()
    {
        var model = _getModel();
        var structureChanged = model.Signature != _model.Signature;
        _model = model;

        var theme = Theme.Current();
        if (theme != _theme)
        {
            _theme = theme;
            ApplyTheme();
            if (IsHandleCreated)
            {
                Theme.ApplyWindowChrome(this, _theme.IsDark);
                Theme.ApplyScrollbarTheme(_gamesList, _theme.IsDark);
                Theme.ApplyScrollbarTheme(_sessionsList, _theme.IsDark);
            }
        }

        if (structureChanged)
            RebuildLists();
        UpdateSummary();

        if (_model.Live.Count > 0)
        {
            _gamesList.Invalidate();
            _sessionsList.Invalidate();
        }
    }

    // ---------- Layout & theme ----------

    private int S(float logical) => (int)Math.Round(logical * DeviceDpi / 96f);

    private void ApplyMetrics()
    {
        _fonts.Dispose();
        _fonts = new Fonts(DeviceDpi / 96f);
        foreach (var c in AllPainted())
            c.Fonts = _fonts;
        _gamesCard.Fonts = _fonts;
        _sessionsCard.Fonts = _fonts;
        _gamesCard.ApplyPadding();
        _sessionsCard.ApplyPadding();
        _gamesList.ItemHeight = S(60);
        _sessionsList.ItemHeight = S(44);
        _sessionsList.Font = _fonts.Body;

        MinimumSize = new Size(S(860), S(640));
        if (!_sized)
        {
            // First show: a comfortable size, but never bigger than the screen.
            var area = Screen.FromPoint(Cursor.Position).WorkingArea;
            Size = new Size(Math.Min(S(1100), area.Width), Math.Min(S(800), area.Height));
            _sized = true;
        }
        LayoutChildren();
        Invalidate(true);
    }

    private void ApplyTheme()
    {
        BackColor = _theme.Window;
        foreach (var c in AllPainted())
            c.Theme = _theme;
        _gamesCard.Theme = _theme;
        _sessionsCard.Theme = _theme;
        _gamesList.BackColor = _theme.Surface;
        _sessionsList.BackColor = _theme.Surface;
        _sessionsList.ForeColor = _theme.TextMuted;
        Invalidate(true);
    }

    private IEnumerable<PaintedControl> AllPainted() => new PaintedControl[] { _header, _chart, _footer }.Concat(_tiles);

    protected override void OnResize(EventArgs e)
    {
        base.OnResize(e);
        LayoutChildren();
    }

    private void LayoutChildren()
    {
        if (_tiles is null || ClientSize.Width == 0)
            return;
        var pad = S(24);
        var gap = S(16);
        var width = ClientSize.Width - pad * 2;
        var y = S(12);

        _header.Bounds = new Rectangle(pad, y, width, S(56));
        y = _header.Bottom + S(8);

        var tileWidth = (width - gap * 3) / 4;
        for (var i = 0; i < _tiles.Length; i++)
            _tiles[i].Bounds = new Rectangle(pad + i * (tileWidth + gap), y, i == 3 ? width - 3 * (tileWidth + gap) : tileWidth, S(96));
        y = _tiles[0].Bottom + gap;

        _chart.Bounds = new Rectangle(pad, y, width, S(210));
        y = _chart.Bottom + gap;

        var footerHeight = S(32);
        var listsHeight = Math.Max(S(160), ClientSize.Height - y - footerHeight - S(8));
        var gamesWidth = (int)(width * 0.38);
        _gamesCard.Bounds = new Rectangle(pad, y, gamesWidth, listsHeight);
        _sessionsCard.Bounds = new Rectangle(pad + gamesWidth + gap, y, width - gamesWidth - gap, listsHeight);
        _footer.Bounds = new Rectangle(pad, _gamesCard.Bottom, width, footerHeight);
    }

    // ---------- Data ----------

    private void RebuildLists()
    {
        var keepGame = _selectedGame;
        _gamesList.BeginUpdate();
        _gamesList.Items.Clear();
        _gamesList.Items.Add(AllGames);
        foreach (var game in _model.Games)
            _gamesList.Items.Add(game.Name);
        var index = keepGame is null ? 0 : _gamesList.Items.IndexOf(keepGame);
        _gamesList.SelectedIndex = index < 0 ? 0 : index;
        _gamesList.EndUpdate();
        RebuildSessions(keepScroll: true);
    }

    private void OnGameSelected()
    {
        var item = _gamesList.SelectedItem as string;
        var game = item is null || item == AllGames ? null : item;
        if (game == _selectedGame)
            return;
        _selectedGame = game;
        RebuildSessions(keepScroll: false);
        UpdateSummary();
    }

    private void RebuildSessions(bool keepScroll)
    {
        var top = keepScroll && _sessionsList.Items.Count > 0 ? _sessionsList.TopIndex : 0;
        _visibleSessions = _model.SessionsFor(_selectedGame).ToList();
        _sessionsList.BeginUpdate();
        _sessionsList.Items.Clear();
        for (var i = 0; i < _visibleSessions.Count; i++)
            _sessionsList.Items.Add(i);
        if (_visibleSessions.Count > 0)
            _sessionsList.TopIndex = Math.Min(top, _visibleSessions.Count - 1);
        _sessionsList.EndUpdate();
    }

    private GameView? SelectedGameView =>
        _selectedGame is null ? null : _model.Games.FirstOrDefault(g => string.Equals(g.Name, _selectedGame, StringComparison.OrdinalIgnoreCase));

    private void UpdateSummary()
    {
        var now = _model.Now;
        var live = _model.Live;

        _header.Title = SelectedGameView?.Name ?? "Your playtime";
        _header.IsLive = live.Count > 0;
        _header.Status = live.Count == 0
            ? "Not playing right now"
            : "Playing " + string.Join(", ", live.Select(l => $"{l.Game} · {ReportWriter.FormatDuration(l.Duration)}"));

        var sessions = _model.SessionsFor(_selectedGame).ToList();
        var total = TimeSpan.FromTicks(sessions.Sum(s => s.Duration.Ticks));
        var todayStart = new DateTimeOffset(now.ToLocalTime().Date, now.ToLocalTime().Offset);
        var week = _model.TotalSince(_selectedGame, todayStart.AddDays(-6));
        var today = _model.TotalSince(_selectedGame, todayStart);
        var average = sessions.Count == 0 ? TimeSpan.Zero : TimeSpan.FromTicks(total.Ticks / sessions.Count);

        SetTile(0, "Total playtime", ReportWriter.FormatDuration(total),
            _selectedGame is null ? Plural(_model.Games.Count, "game") : $"{Share(total)} of all your playtime");
        SetTile(1, "Sessions", sessions.Count.ToString("N0", CultureInfo.CurrentCulture),
            sessions.Count == 0 ? "—" : $"{ReportWriter.FormatDuration(average)} on average");
        SetTile(2, "Past 7 days", ReportWriter.FormatDuration(week), today > TimeSpan.Zero ? $"{ReportWriter.FormatDuration(today)} today" : "Nothing today");

        if (SelectedGameView is { } game)
        {
            SetTile(3, "Last played", game.IsLive ? "Now" : FormatDay(game.LastPlayed),
                game.IsLive ? "Session in progress" : game.LastPlayed.ToLocalTime().ToString("h:mm tt", CultureInfo.CurrentCulture));
        }
        else
        {
            var top = _model.Games.FirstOrDefault();
            SetTile(3, "Most played", top?.Name ?? "—", top is null ? "Play a game to see it here" : ReportWriter.FormatDuration(top.Total));
        }

        var days = _model.DailyTotals(_selectedGame);
        _chart.Title = "Daily playtime";
        _chart.Detail = $"Last {DashboardModel.ChartDays} days · {ReportWriter.FormatDuration(TimeSpan.FromTicks(days.Sum(d => d.Total.Ticks)))}";
        _chart.Days = days;

        _gamesCard.Detail = Plural(_model.Games.Count, "game");
        _sessionsCard.Title = _selectedGame is null ? "Sessions" : $"Sessions · {SelectedGameView?.Name ?? _selectedGame}";
        _sessionsCard.Detail = Plural(sessions.Count, "session");

        _header.Invalidate();
        foreach (var tile in _tiles)
            tile.Invalidate();
        _gamesCard.Invalidate(false);
        _sessionsCard.Invalidate(false);
    }

    private void SetTile(int index, string label, string value, string caption)
    {
        _tiles[index].Label = label;
        _tiles[index].Value = value;
        _tiles[index].Caption = caption;
    }

    private string Share(TimeSpan part)
    {
        var all = _model.Sessions.Sum(s => s.Duration.Ticks);
        return all == 0 ? "0%" : (part.Ticks / (double)all).ToString("P0", CultureInfo.CurrentCulture);
    }

    // ---------- Row drawing ----------

    private void DrawGameRow(Graphics g, Rectangle bounds, int index, bool selected, bool hot)
    {
        g.SmoothingMode = SmoothingMode.AntiAlias;
        using (var bg = new SolidBrush(_theme.Surface))
            g.FillRectangle(bg, bounds);
        DrawRowHighlight(g, bounds, selected, hot);

        var x = bounds.X + S(12);
        var w = bounds.Width - S(24);
        string name, sub, total;
        double share;
        var live = false;

        if (index == 0)
        {
            var all = TimeSpan.FromTicks(_model.Sessions.Sum(s => s.Duration.Ticks));
            name = "All games";
            sub = $"{Plural(_model.Games.Count, "game")} · {Plural(_model.Sessions.Count, "session")}";
            total = ReportWriter.FormatDuration(all);
            share = 0;
        }
        else
        {
            var game = _model.Games.ElementAtOrDefault(index - 1);
            if (game is null)
                return;
            live = game.IsLive;
            name = game.Name;
            sub = (live ? "Playing now · " : "") + $"{Plural(game.SessionCount, "session")} · last {FormatDay(game.LastPlayed)}";
            total = ReportWriter.FormatDuration(game.Total);
            var max = _model.Games[0].Total.Ticks;
            share = max == 0 ? 0 : game.Total.Ticks / (double)max;
        }

        var flags = TextFormatFlags.NoPrefix | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding;
        var totalSize = TextRenderer.MeasureText(g, total, _fonts.BodyStrong, Size.Empty, MeasureFlags);
        var totalWidth = totalSize.Width + S(4);
        TextRenderer.DrawText(g, total, _fonts.BodyStrong, new Rectangle(x + w - totalWidth, bounds.Y + S(11), totalWidth, S(22)), _theme.TextPrimary, flags | TextFormatFlags.Right);

        var nameX = x;
        if (live)
        {
            using var dot = new SolidBrush(_theme.Live);
            g.FillEllipse(dot, x, bounds.Y + S(17), S(7), S(7));
            nameX += S(13);
        }
        TextRenderer.DrawText(g, name, _fonts.BodyStrong, new Rectangle(nameX, bounds.Y + S(11), w - totalWidth - S(12) - (nameX - x), S(22)), _theme.TextPrimary, flags);
        TextRenderer.DrawText(g, sub, _fonts.Small, new Rectangle(x, bounds.Y + S(31), w, S(18)), _theme.TextSecondary, flags);

        if (index > 0)
        {
            // Thin share bar relative to the most-played game.
            var barY = bounds.Bottom - S(8);
            var barH = S(3);
            using var track = new SolidBrush(_theme.Track);
            using var fill = new SolidBrush(_theme.Accent);
            using (var trackPath = Theme.RoundedRect(new RectangleF(x, barY, w, barH), barH / 2f))
                g.FillPath(track, trackPath);
            var fillWidth = Math.Max(barH, (float)(w * share));
            using var fillPath = Theme.RoundedRect(new RectangleF(x, barY, fillWidth, barH), barH / 2f);
            g.FillPath(fill, fillPath);
        }
    }

    private void DrawSessionRow(Graphics g, Rectangle bounds, int index, bool selected, bool hot)
    {
        g.SmoothingMode = SmoothingMode.AntiAlias;
        using (var bg = new SolidBrush(_theme.Surface))
            g.FillRectangle(bg, bounds);
        DrawRowHighlight(g, bounds, false, hot);

        if (index >= _visibleSessions.Count)
            return;
        var s = _visibleSessions[index];
        if (s.IsLive)
        {
            // Live rows keep ticking: re-read from the current model.
            s = _model.Live.FirstOrDefault(l => l.Game == s.Game && l.Start == s.Start) ?? s;
        }

        var flags = TextFormatFlags.NoPrefix | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding | TextFormatFlags.VerticalCenter;
        var x = bounds.X + S(12);
        var right = bounds.Right - S(12);
        var row = new Rectangle(0, bounds.Y, 0, bounds.Height);

        var duration = ReportWriter.FormatDuration(s.Duration);
        var durationWidth = S(76);
        TextRenderer.DrawText(g, duration, _fonts.BodyStrong, row with { X = right - durationWidth, Width = durationWidth }, _theme.TextPrimary, flags | TextFormatFlags.Right);

        var start = s.Start.ToLocalTime();
        var end = s.End.ToLocalTime();
        var time = s.IsLive
            ? $"{start.ToString("h:mm tt", CultureInfo.CurrentCulture)} – now"
            : start.Date == end.Date
                ? $"{start.ToString("h:mm tt", CultureInfo.CurrentCulture)} – {end.ToString("h:mm tt", CultureInfo.CurrentCulture)}"
                : $"{start.ToString("h:mm tt", CultureInfo.CurrentCulture)} – {end.ToString("MMM d, h:mm tt", CultureInfo.CurrentCulture)}";

        var dateWidth = S(104);
        TextRenderer.DrawText(g, FormatDay(s.Start), _fonts.Body, row with { X = x, Width = dateWidth }, _theme.TextPrimary, flags);

        var timeWidth = TextRenderer.MeasureText(g, time, _fonts.Body, Size.Empty, MeasureFlags).Width + S(4);
        var timeX = right - durationWidth - S(12) - timeWidth;
        if (_selectedGame is null)
        {
            var gameX = x + dateWidth + S(12);
            var gameWidth = Math.Max(0, timeX - S(12) - gameX);
            if (s.IsLive)
            {
                using var dot = new SolidBrush(_theme.Live);
                g.FillEllipse(dot, gameX, bounds.Y + (bounds.Height - S(7)) / 2f, S(7), S(7));
                gameX += S(13);
                gameWidth -= S(13);
            }
            TextRenderer.DrawText(g, s.Game, _fonts.Body, row with { X = gameX, Width = Math.Max(0, gameWidth) }, _theme.TextPrimary, flags);
        }
        else
        {
            timeX = x + dateWidth + S(12);
            timeWidth = right - durationWidth - S(12) - timeX;
        }
        TextRenderer.DrawText(g, time, _fonts.Body, row with { X = timeX, Width = Math.Max(0, timeWidth) }, _theme.TextSecondary, flags);

        // Hairline separator between rows.
        if (index < _visibleSessions.Count - 1)
        {
            using var pen = new Pen(_theme.Track, 1);
            g.SmoothingMode = SmoothingMode.None;
            g.DrawLine(pen, x, bounds.Bottom - 1, right, bounds.Bottom - 1);
        }
    }

    private void DrawRowHighlight(Graphics g, Rectangle bounds, bool selected, bool hot)
    {
        if (!selected && !hot)
            return;
        var rect = new RectangleF(bounds.X + S(2), bounds.Y + S(2), bounds.Width - S(4), bounds.Height - S(4));
        using var path = Theme.RoundedRect(rect, S(6));
        using var brush = new SolidBrush(selected ? _theme.Selection : _theme.Hover);
        g.FillPath(brush, path);
    }

    private const TextFormatFlags MeasureFlags = TextFormatFlags.NoPrefix | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding;

    private static string FormatDay(DateTimeOffset value)
    {
        var local = value.ToLocalTime().Date;
        var today = DateTime.Now.Date;
        if (local == today)
            return "Today";
        if (local == today.AddDays(-1))
            return "Yesterday";
        return local.Year == today.Year
            ? local.ToString("ddd, MMM d", CultureInfo.CurrentCulture)
            : local.ToString("MMM d, yyyy", CultureInfo.CurrentCulture);
    }

    private static string Plural(int count, string noun) =>
        $"{count.ToString("N0", CultureInfo.CurrentCulture)} {noun}{(count == 1 ? "" : "s")}";
}

using System.Drawing.Drawing2D;
using System.Globalization;
using System.Reflection;

namespace GameSessionTracker.Ui;

/// <summary>The main window: all play data in one place. Closing it leaves the tracker running in the tray.</summary>
internal sealed class DashboardForm : Form
{
    private const string AllGames = "\0all"; // sentinel item for the "All games" row

    private const int OverviewTab = 0;
    private const int GamesTab = 1;
    private const int HistoryTab = 2;
    public const int SettingsTab = 3;

    private readonly ITrackerHost _host;
    private readonly SettingsView _settingsView;
    private readonly System.Windows.Forms.Timer _refreshTimer;
    private readonly Backdrop _backdrop = new();

    private readonly HeaderBar _header = new() { Tabs = new[] { "Overview", "Games", "History", "Settings" } };

    // Overview
    private readonly StatTile[] _tiles = { new(), new(), new(), new() };
    private readonly DailyChart _chart = new();
    private readonly Card _gamesCard = new() { Title = "Games" };
    private readonly Card _sessionsCard = new() { Title = "Sessions" };
    private readonly RowList _gamesList = new();
    private readonly RowList _sessionsList = new();

    // Games page: what's running now, and everything played before
    private readonly Card _libraryCard = new() { Title = "Your games" };
    private readonly RowList _libraryList = new();

    // History page: the last 30 days, one row per day
    private readonly Card _historyCard = new() { Title = "Playtime history" };
    private readonly RowList _historyList = new();

    private readonly FooterBar _footer = new()
    {
        Message = "Tracking continues in the system tray when you close this window.",
        LinkText = "Open data folder",
    };

    private Theme _theme;
    private Fonts _fonts;
    private readonly List<Fonts> _retiredFonts = new(); // replaced on DPI change; disposed with the window, never while in use
    private DashboardModel _model;
    private string? _selectedGame; // null = all games
    private bool _sized;
    private List<SessionView> _visibleSessions = new();
    private List<LibraryRow> _libraryRows = new();
    private IReadOnlyList<DayHistory> _history = Array.Empty<DayHistory>();

    /// <summary>A row on the Games page: a section heading, a game, or a note for an empty section.</summary>
    private sealed record LibraryRow(string? Heading = null, GameView? Game = null, string? Note = null);

    public DashboardForm(ITrackerHost host)
    {
        _host = host;
        _model = host.GetModel();
        _theme = Theme.Resolve(host.Settings);
        _fonts = new Fonts(DeviceDpi / 96f);
        _settingsView = new SettingsView(host, () => (_theme, _fonts)) { Visible = false };

        Text = "Playtime Tracker";
        AutoScaleMode = AutoScaleMode.None; // layout and fonts are scaled by hand in ApplyMetrics
        StartPosition = FormStartPosition.CenterScreen;
        DoubleBuffered = true;
        ResizeRedraw = true;
        using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("GameSessionTracker.app.ico"))
        {
            if (stream is not null)
                Icon = new Icon(stream);
        }

        foreach (var (card, list) in Pairs())
        {
            card.Controls.Add(list);
            list.Dock = DockStyle.Fill;
        }
        _libraryCard.Visible = false;
        _historyCard.Visible = false;
        Controls.Add(_header);
        Controls.AddRange(_tiles);
        Controls.Add(_chart);
        Controls.Add(_gamesCard);
        Controls.Add(_sessionsCard);
        Controls.Add(_libraryCard);
        Controls.Add(_historyCard);
        Controls.Add(_footer);
        Controls.Add(_settingsView);

        _gamesList.DrawRow += DrawGameRow;
        _sessionsList.DrawRow += DrawSessionRow;
        _libraryList.DrawRow += DrawLibraryRow;
        _historyList.DrawRow += DrawHistoryRow;
        _sessionsList.EmptyText = "Your sessions will appear here after you play a game.";
        _gamesList.SelectedIndexChanged += (_, _) => OnGameSelected();

        // Sessions, games and days open on click (or Enter when the list has focus).
        Activatable(_sessionsList, i => i < _visibleSessions.Count, i => OpenSession(_visibleSessions[i]));
        Activatable(_libraryList, i => i < _libraryRows.Count && _libraryRows[i].Game is not null, i => ShowGameOnOverview(_libraryRows[i].Game!.Name));
        Activatable(_historyList, i => i < _history.Count && _history[i].SessionCount > 0, i => OpenDay(_history[i].Day, null));
        _chart.DayClicked += day => OpenDay(day, _selectedGame);

        _header.TabIndex = 100; // the games list keeps first focus; Tab reaches the page switcher afterwards
        _footer.LinkClicked += () => _host.OpenDataFolder();
        _header.TabClicked += ShowTab;
        _settingsView.AppearanceChanged += RefreshData;
        _gamesList.ContextMenuStrip = new ContextMenuStrip();
        _gamesList.ContextMenuStrip.Opening += OnGamesMenuOpening;
        _sessionsList.ContextMenuStrip = new ContextMenuStrip();
        _sessionsList.ContextMenuStrip.Opening += OnSessionsMenuOpening;

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

    private IEnumerable<(Card Card, RowList List)> Pairs() => new[]
    {
        (_gamesCard, _gamesList), (_sessionsCard, _sessionsList), (_libraryCard, _libraryList), (_historyCard, _historyList),
    };

    /// <summary>Makes rows that pass <paramref name="canOpen"/> open with a click or Enter, with a hand cursor on hover.</summary>
    private static void Activatable(RowList list, Func<int, bool> canOpen, Action<int> open)
    {
        list.MouseClick += (_, e) =>
        {
            var index = list.IndexFromPoint(e.Location);
            if (e.Button == MouseButtons.Left && index >= 0 && canOpen(index))
                open(index);
        };
        list.MouseMove += (_, _) => list.Cursor = list.HotIndex >= 0 && canOpen(list.HotIndex) ? Cursors.Hand : Cursors.Default;
        list.KeyDown += (_, e) =>
        {
            if (e.KeyCode == Keys.Enter && list.SelectedIndex >= 0 && canOpen(list.SelectedIndex))
            {
                open(list.SelectedIndex);
                e.Handled = true;
            }
        };
        // Keyboard selection is only drawn while the list has focus.
        list.GotFocus += (_, _) => list.Invalidate();
        list.LostFocus += (_, _) => list.Invalidate();
    }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        ApplyChrome();
    }

    private void ApplyChrome()
    {
        Theme.ApplyWindowChrome(this, _theme);
        foreach (var (_, list) in Pairs())
            Theme.ApplyScrollbarTheme(list, _theme.IsDark);
    }

    /// <summary>The atmospheric backdrop plus soft shadows under the glass cards; every glass surface shows through to this.</summary>
    protected override void OnPaintBackground(PaintEventArgs e)
    {
        _backdrop.Paint(e.Graphics, e.ClipRectangle, ClientSize, _theme);
        Glass.PaintChildShadows(e.Graphics, this, e.ClipRectangle, _theme);
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
        {
            _fonts.Dispose();
            foreach (var fonts in _retiredFonts)
                fonts.Dispose();
            _backdrop.Dispose();
        }
        base.Dispose(disposing);
    }

    /// <summary>Called by the tray when something changed, by the timer, and after appearance settings change.</summary>
    public void RefreshData()
    {
        var model = _host.GetModel();
        var structureChanged = model.Signature != _model.Signature;
        _model = model;

        var theme = Theme.Resolve(_host.Settings);
        if (!ReferenceEquals(theme, _theme))
        {
            _theme = theme;
            ApplyTheme();
            if (IsHandleCreated)
                ApplyChrome();
        }

        if (structureChanged)
            RebuildLists();
        UpdateSummary();

        if (_model.Live.Count > 0)
        {
            foreach (var (_, list) in Pairs())
                list.Invalidate();
        }
    }

    // ---------- Layout & theme ----------

    private int S(float logical) => (int)Math.Round(logical * DeviceDpi / 96f);
    private float UiScale => DeviceDpi / 96f;

    private void ApplyMetrics()
    {
        // Old fonts may still be referenced by controls or an open dialog until everything is repainted, so they're
        // retired rather than disposed here.
        _retiredFonts.Add(_fonts);
        _fonts = new Fonts(DeviceDpi / 96f);
        foreach (var c in AllPainted())
            c.Fonts = _fonts;
        _settingsView.ApplyStyle();
        foreach (var (card, list) in Pairs())
        {
            card.Fonts = _fonts;
            card.ApplyPadding();
            list.EmptyFont = _fonts.Body;
        }
        _gamesList.ItemHeight = S(64);
        _sessionsList.ItemHeight = S(44);
        _libraryList.ItemHeight = S(64);
        _historyList.ItemHeight = S(64);

        MinimumSize = new Size(S(760), S(640));
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
        BackColor = _theme.Base;
        foreach (var c in AllPainted())
            c.Theme = _theme;
        foreach (var (card, list) in Pairs())
        {
            card.Theme = _theme;
            list.ForeColor = _theme.TextMuted;
        }
        _settingsView.ApplyStyle();
        Invalidate(true);
    }

    private IEnumerable<PaintedControl> AllPainted() => new PaintedControl[] { _header, _chart, _footer }.Concat(_tiles);

    protected override void OnResize(EventArgs e)
    {
        base.OnResize(e);
        LayoutChildren();
        Invalidate(true); // the backdrop is sized to the window, so everything glass shows a new part of it
    }

    private void LayoutChildren()
    {
        if (_tiles is null || ClientSize.Width == 0)
            return;
        // Roomier gutters on large windows, tighter ones on small laptops.
        var pad = ClientSize.Width >= S(1280) ? S(32) : ClientSize.Width >= S(960) ? S(24) : S(16);
        var gap = ClientSize.Width >= S(960) ? S(16) : S(12);
        var width = ClientSize.Width - pad * 2;
        var y = S(12);

        _header.Bounds = new Rectangle(pad, y, width, S(64));
        y = _header.Bottom + S(8);
        var contentTop = y;

        var tileWidth = (width - gap * 3) / 4;
        for (var i = 0; i < _tiles.Length; i++)
            _tiles[i].Bounds = new Rectangle(pad + i * (tileWidth + gap), y, i == 3 ? width - 3 * (tileWidth + gap) : tileWidth, S(100));
        y = _tiles[0].Bottom + gap;

        // The chart gives up some height on short windows so the lists stay usable.
        var chartHeight = ClientSize.Height >= S(820) ? S(236) : ClientSize.Height >= S(700) ? S(210) : S(184);
        _chart.Bounds = new Rectangle(pad, y, width, chartHeight);
        y = _chart.Bottom + gap;

        var footerHeight = S(36);
        var listsHeight = Math.Max(S(160), ClientSize.Height - y - footerHeight - S(4));
        var gamesWidth = (int)(width * (width < S(900) ? 0.42 : 0.38));
        _gamesCard.Bounds = new Rectangle(pad, y, gamesWidth, listsHeight);
        _sessionsCard.Bounds = new Rectangle(pad + gamesWidth + gap, y, width - gamesWidth - gap, listsHeight);
        _footer.Bounds = new Rectangle(pad, _gamesCard.Bottom, width, footerHeight);

        // Games and History pages: one full-width card each; very wide windows don't stretch rows past a readable width.
        var pageWidth = Math.Min(width, S(1100));
        var page = new Rectangle(pad, contentTop, pageWidth, Math.Max(S(160), _footer.Top - contentTop));
        _libraryCard.Bounds = page;
        _historyCard.Bounds = page;

        _settingsView.Bounds = new Rectangle(0, contentTop, ClientSize.Width, Math.Max(0, _footer.Top - contentTop));
    }

    // ---------- Pages ----------

    public void ShowTab(int tab)
    {
        _header.SelectedTab = tab;
        SuspendLayout();
        foreach (var c in new Control[] { _chart, _gamesCard, _sessionsCard }.Concat(_tiles))
            c.Visible = tab == OverviewTab;
        _libraryCard.Visible = tab == GamesTab;
        _historyCard.Visible = tab == HistoryTab;
        _settingsView.Visible = tab == SettingsTab;
        if (tab == SettingsTab)
        {
            _settingsView.Reload();
            _settingsView.BringToFront();
        }
        ResumeLayout(true);
        UpdateSummary();
        Invalidate(true);
    }

    /// <summary>Called when settings changed outside the window (e.g. a verified settings file restored on disk).</summary>
    public void ReloadSettings()
    {
        if (_settingsView.Visible)
            _settingsView.Reload();
    }

    /// <summary>From the Games page: filter the Overview to one game.</summary>
    private void ShowGameOnOverview(string game)
    {
        var index = _gamesList.Items.IndexOf(game);
        if (index < 0)
            index = _model.Games.ToList().FindIndex(g => string.Equals(g.Name, game, StringComparison.OrdinalIgnoreCase)) + 1;
        if (index > 0)
            _gamesList.SelectedIndex = index;
        ShowTab(OverviewTab);
    }

    private void OpenSession(SessionView session)
    {
        if (SessionDetailsDialog.Show(this, _host, _theme, _fonts, session))
            RefreshData();
    }

    private void OpenDay(DateTime day, string? game)
    {
        if (DayDetailsDialog.Show(this, _host, _theme, _fonts, day, game))
            RefreshData();
    }

    // ---------- Right-click actions ----------

    private void OnGamesMenuOpening(object? sender, System.ComponentModel.CancelEventArgs e)
    {
        var menu = (ContextMenuStrip)sender!;
        var index = _gamesList.IndexFromPoint(_gamesList.PointToClient(Cursor.Position));
        var game = index > 0 ? _model.Games.ElementAtOrDefault(index - 1) : null; // "All games" has no actions
        if (game is null)
        {
            e.Cancel = true;
            return;
        }
        _gamesList.SelectedIndex = index;
        PrepareMenu(menu, e);

        var ignored = _host.Settings.IgnoredGames.Contains(game.Name, StringComparer.OrdinalIgnoreCase);
        menu.Items.Add(ignored
            ? ThemedMenu.Item("Track this game again", _theme, () => SetIgnored(game.Name, false))
            : ThemedMenu.Item("Stop tracking this game", _theme, () => SetIgnored(game.Name, true)));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add(ThemedMenu.Item("Delete this game's history...", _theme, () =>
        {
            var records = _model.SessionsFor(game.Name).Where(s => s.Source is not null).Select(s => s.Source!).ToList();
            if (records.Count == 0)
                return;
            if (Confirm($"Delete all {Format.Plural(records.Count, "session")} of {game.Name}? This can't be undone.", "Delete history") != DialogResult.OK)
                return;
            _host.DeleteSessions(records);
            RefreshData();
        }, destructive: true));
    }

    private void OnSessionsMenuOpening(object? sender, System.ComponentModel.CancelEventArgs e)
    {
        var menu = (ContextMenuStrip)sender!;
        var index = _sessionsList.IndexFromPoint(_sessionsList.PointToClient(Cursor.Position));
        if (index < 0 || index >= _visibleSessions.Count)
        {
            e.Cancel = true; // nothing there
            return;
        }
        var session = _visibleSessions[index];
        PrepareMenu(menu, e);
        menu.Items.Add(ThemedMenu.Item("Details...", _theme, () => OpenSession(session)));
        if (session.Source is not { } record)
            return; // a session still in progress can't be deleted
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add(ThemedMenu.Item("Delete this session", _theme, () =>
        {
            var when = $"{Format.Day(record.Start)}, {Format.Time(record.Start)}";
            if (Confirm($"Delete the {ReportWriter.FormatDuration(record.Duration)} session of {record.Game} from {when}?", "Delete session") != DialogResult.OK)
                return;
            _host.DeleteSessions(new[] { record });
            RefreshData();
        }, destructive: true));
    }

    private DialogResult Confirm(string text, string caption) =>
        ModalScrim.Show(this, _theme, () =>
            MessageBox.Show(this, text, caption, MessageBoxButtons.OKCancel, MessageBoxIcon.Warning, MessageBoxDefaultButton.Button2));

    private void PrepareMenu(ContextMenuStrip menu, System.ComponentModel.CancelEventArgs e)
    {
        e.Cancel = false; // WinForms pre-cancels opening a menu that was empty; we fill it here
        menu.Items.Clear();
        ThemedMenu.Apply(menu, _theme, _fonts.Body);
    }

    private void SetIgnored(string game, bool ignore)
    {
        _host.UpdateSettings(s =>
        {
            s.IgnoredGames.RemoveAll(n => string.Equals(n, game, StringComparison.OrdinalIgnoreCase));
            if (ignore)
                s.IgnoredGames.Add(game);
        }, affectsGameDetection: true);
        _gamesList.Invalidate();
        _libraryList.Invalidate();
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
        if (index < 0)
            _selectedGame = null; // the selected game's history was deleted
        _gamesList.SelectedIndex = index < 0 ? 0 : index;
        _gamesList.EndUpdate();
        RebuildSessions(keepScroll: true);
        RebuildLibrary();
    }

    private void RebuildLibrary()
    {
        var rows = new List<LibraryRow>();
        var live = _model.Games.Where(g => g.IsLive).ToList();
        var played = _model.Games.Where(g => !g.IsLive).OrderByDescending(g => g.LastPlayed).ToList();
        rows.Add(new LibraryRow(Heading: $"Playing now · {live.Count}"));
        if (live.Count == 0)
            rows.Add(new LibraryRow(Note: "Nothing running right now. Start a game and it shows up here within a few seconds."));
        rows.AddRange(live.Select(g => new LibraryRow(Game: g)));
        rows.Add(new LibraryRow(Heading: $"Played before · {played.Count}"));
        if (played.Count == 0)
            rows.Add(new LibraryRow(Note: "Games you've played will be listed here, most recent first."));
        rows.AddRange(played.Select(g => new LibraryRow(Game: g)));
        _libraryRows = rows;
        SetRows(_libraryList, rows.Count);
    }

    /// <summary>Fills a list with <paramref name="count"/> index items, keeping the scroll position.</summary>
    private static void SetRows(RowList list, int count)
    {
        var top = list.Items.Count > 0 ? list.TopIndex : 0;
        list.BeginUpdate();
        list.Items.Clear();
        for (var i = 0; i < count; i++)
            list.Items.Add(i);
        if (count > 0)
            list.TopIndex = Math.Min(top, count - 1);
        list.EndUpdate();
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

        _header.Title = _header.SelectedTab switch
        {
            GamesTab => "Games",
            HistoryTab => "History",
            SettingsTab => "Settings",
            _ => SelectedGameView?.Name ?? "Your playtime",
        };
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
            _selectedGame is null ? Format.Plural(_model.Games.Count, "game") : $"{Share(total)} of all your playtime");
        var longest = sessions.Count == 0 ? TimeSpan.Zero : sessions.Max(s => s.Duration);
        SetTile(1, "Sessions", sessions.Count.ToString("N0", CultureInfo.CurrentCulture),
            sessions.Count == 0 ? "—" : $"{ReportWriter.FormatDuration(average)} avg · {ReportWriter.FormatDuration(longest)} longest");
        SetTile(2, "Past 7 days", ReportWriter.FormatDuration(week), today > TimeSpan.Zero ? $"{ReportWriter.FormatDuration(today)} today" : "Nothing today");

        if (SelectedGameView is { } game)
        {
            SetTile(3, "Last played", game.IsLive ? "Now" : Format.Day(game.LastPlayed),
                $"First played {Format.Day(game.FirstPlayed).Replace("Today", "today").Replace("Yesterday", "yesterday")}");
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

        _gamesCard.Detail = Format.Plural(_model.Games.Count, "game");
        _sessionsCard.Title = _selectedGame is null ? "Sessions" : $"Sessions · {SelectedGameView?.Name ?? _selectedGame}";
        _sessionsCard.Detail = Format.Plural(sessions.Count, "session");

        var liveGames = _model.Games.Count(g => g.IsLive);
        _libraryCard.Detail = $"{Format.Plural(_model.Games.Count, "game")} · {liveGames} playing now";

        _history = _model.History();
        if (_historyList.Items.Count != _history.Count)
            SetRows(_historyList, _history.Count);
        var historyTotal = TimeSpan.FromTicks(_history.Sum(d => d.Total.Ticks));
        _historyCard.Detail = $"Last {DashboardModel.ChartDays} days · {ReportWriter.FormatDuration(historyTotal)} · {Format.Plural(_history.Count(d => d.SessionCount > 0), "day")} played";

        _header.Invalidate();
        foreach (var tile in _tiles)
            tile.Invalidate();
        foreach (var (card, _) in Pairs())
            card.Invalidate(false);
        if (_historyCard.Visible)
            _historyList.Invalidate();
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

    private const TextFormatFlags TextFlags = TextFormatFlags.NoPrefix | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding;

    private void DrawGameRow(Graphics g, Rectangle bounds, int index, bool selected, bool hot)
    {
        RowPainter.Highlight(g, bounds, selected, hot, _theme, UiScale);

        var x = bounds.X + S(14);
        var w = bounds.Width - S(28);
        string name, sub, total;
        double share;
        var live = false;

        if (index == 0)
        {
            var all = TimeSpan.FromTicks(_model.Sessions.Sum(s => s.Duration.Ticks));
            name = "All games";
            sub = $"{Format.Plural(_model.Games.Count, "game")} · {Format.Plural(_model.Sessions.Count, "session")}";
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
            var ignored = _host.Settings.IgnoredGames.Contains(game.Name, StringComparer.OrdinalIgnoreCase);
            sub = (live ? "Playing now · " : ignored ? "Not tracked · " : "") + $"{Format.Plural(game.SessionCount, "session")} · last {Format.Day(game.LastPlayed)}";
            total = ReportWriter.FormatDuration(game.Total);
            var max = _model.Games[0].Total.Ticks;
            share = max == 0 ? 0 : game.Total.Ticks / (double)max;
        }

        var totalSize = TextRenderer.MeasureText(g, total, _fonts.BodyStrong, Size.Empty, RowPainter.MeasureFlags);
        var totalWidth = totalSize.Width + S(4);
        TextRenderer.DrawText(g, total, _fonts.BodyStrong, new Rectangle(x + w - totalWidth, bounds.Y + S(10), totalWidth, S(22)), _theme.TextPrimary, TextFlags | TextFormatFlags.Right);

        var nameX = x;
        if (live)
        {
            using var dot = new SolidBrush(_theme.Success);
            g.FillEllipse(dot, x, bounds.Y + S(17), S(7), S(7));
            nameX += S(13);
        }
        TextRenderer.DrawText(g, name, _fonts.BodyStrong, new Rectangle(nameX, bounds.Y + S(10), w - totalWidth - S(12) - (nameX - x), S(22)), _theme.TextPrimary, TextFlags);
        TextRenderer.DrawText(g, sub, _fonts.Small, new Rectangle(x, bounds.Y + S(32), w, S(18)), live ? _theme.SuccessText : _theme.TextSecondary, TextFlags);

        if (index > 0)
            RowPainter.ShareBar(g, new RectangleF(x, bounds.Bottom - S(11), w, S(3)), share, _theme); // relative to the most-played game
    }

    private void DrawSessionRow(Graphics g, Rectangle bounds, int index, bool selected, bool hot)
    {
        if (index >= _visibleSessions.Count)
            return;
        var s = _visibleSessions[index];
        if (s.IsLive)
        {
            // Live rows keep ticking: re-read from the current model.
            s = _model.Live.FirstOrDefault(l => l.SameSessionAs(s)) ?? s;
        }
        RowPainter.Session(g, bounds, s, _theme, _fonts, UiScale, showDate: true, showGame: _selectedGame is null,
            hot, selected && _sessionsList.Focused, separator: index < _visibleSessions.Count - 1);
    }

    private void DrawLibraryRow(Graphics g, Rectangle bounds, int index, bool selected, bool hot)
    {
        if (index >= _libraryRows.Count)
            return;
        var row = _libraryRows[index];
        var x = bounds.X + S(14);
        var w = bounds.Width - S(28);

        if (row.Heading is { } heading)
        {
            // Section heading, sitting low in its row so it groups with the games below it.
            TextRenderer.DrawText(g, heading.ToUpperInvariant(), _fonts.Small, new Rectangle(x, bounds.Bottom - S(28), w, S(18)), _theme.TextMuted, TextFlags);
            using var pen = new Pen(_theme.Separator, 1);
            g.SmoothingMode = SmoothingMode.None;
            g.DrawLine(pen, x, bounds.Bottom - S(6), x + w, bounds.Bottom - S(6));
            return;
        }
        if (row.Note is { } note)
        {
            TextRenderer.DrawText(g, note, _fonts.Body, new Rectangle(x, bounds.Y, w, bounds.Height), _theme.TextSecondary, TextFlags | TextFormatFlags.VerticalCenter);
            return;
        }
        if (row.Game is not { } game)
            return;

        RowPainter.Highlight(g, bounds, selected && _libraryList.Focused, hot, _theme, UiScale);
        var right = x + w - RowPainter.ChevronSpace(UiScale) + S(8);
        var total = ReportWriter.FormatDuration(game.Total);
        var totalWidth = TextRenderer.MeasureText(g, total, _fonts.BodyStrong, Size.Empty, RowPainter.MeasureFlags).Width + S(4);
        TextRenderer.DrawText(g, total, _fonts.BodyStrong, new Rectangle(right - totalWidth, bounds.Y + S(12), totalWidth, S(22)), _theme.TextPrimary, TextFlags | TextFormatFlags.Right);
        var avg = $"{ReportWriter.FormatDuration(game.Average)} avg";
        TextRenderer.DrawText(g, avg, _fonts.Small, new Rectangle(right - S(140), bounds.Y + S(34), S(140), S(18)), _theme.TextMuted, TextFlags | TextFormatFlags.Right);

        var nameX = x;
        string sub;
        Color subColor;
        if (game.IsLive && _model.Live.FirstOrDefault(l => string.Equals(l.Game, game.Name, StringComparison.OrdinalIgnoreCase)) is { } now)
        {
            using var dot = new SolidBrush(_theme.Success);
            g.FillEllipse(dot, x, bounds.Y + S(19), S(7), S(7));
            nameX += S(13);
            sub = $"Playing now · opened {Format.Time(now.Start)} · {ReportWriter.FormatDuration(now.Duration)} so far";
            subColor = _theme.SuccessText;
        }
        else
        {
            var ignored = _host.Settings.IgnoredGames.Contains(game.Name, StringComparer.OrdinalIgnoreCase);
            sub = (ignored ? "Not tracked · " : "") +
                  $"{Format.Plural(game.SessionCount, "session")} · first played {Format.Day(game.FirstPlayed)} · last played {Format.Day(game.LastPlayed)}";
            subColor = _theme.TextSecondary;
        }
        var textWidth = right - S(152) - x;
        TextRenderer.DrawText(g, game.Name, _fonts.BodyStrong, new Rectangle(nameX, bounds.Y + S(12), textWidth - (nameX - x), S(22)), _theme.TextPrimary, TextFlags);
        TextRenderer.DrawText(g, sub, _fonts.Small, new Rectangle(x, bounds.Y + S(34), textWidth, S(18)), subColor, TextFlags);
        RowPainter.Chevron(g, bounds, hot, _theme, _fonts, UiScale);
    }

    private void DrawHistoryRow(Graphics g, Rectangle bounds, int index, bool selected, bool hot)
    {
        if (index >= _history.Count)
            return;
        var day = _history[index];
        var played = day.SessionCount > 0;
        RowPainter.Highlight(g, bounds, played && selected && _historyList.Focused, played && hot, _theme, UiScale);

        var x = bounds.X + S(14);
        var w = bounds.Width - S(28);
        var right = x + w - RowPainter.ChevronSpace(UiScale) + S(8);
        var dateWidth = S(150);

        TextRenderer.DrawText(g, Format.Day(day.Day), _fonts.BodyStrong, new Rectangle(x, bounds.Y + S(11), dateWidth, S(22)),
            played ? _theme.TextPrimary : _theme.TextMuted, TextFlags);
        // Second line adds what the first doesn't say: the date for "Today"/"Yesterday", otherwise how long ago.
        var daysAgo = (_model.Now.ToLocalTime().Date - day.Day).Days;
        var dateDetail = daysAgo <= 1 ? day.Day.ToString("ddd, MMM d", CultureInfo.CurrentCulture) : $"{daysAgo} days ago";
        TextRenderer.DrawText(g, dateDetail, _fonts.Small, new Rectangle(x, bounds.Y + S(34), dateWidth, S(18)), _theme.TextMuted, TextFlags);

        var total = played ? ReportWriter.FormatDuration(day.Total) : "—";
        TextRenderer.DrawText(g, total, _fonts.BodyStrong, new Rectangle(right - S(100), bounds.Y + S(11), S(100), S(22)),
            played ? _theme.TextPrimary : _theme.TextMuted, TextFlags | TextFormatFlags.Right);
        if (played)
            TextRenderer.DrawText(g, Format.Plural(day.SessionCount, "session"), _fonts.Small, new Rectangle(right - S(100), bounds.Y + S(34), S(100), S(18)), _theme.TextMuted, TextFlags | TextFormatFlags.Right);

        var gamesX = x + dateWidth + S(12);
        var gamesWidth = right - S(112) - gamesX;
        var games = played
            ? string.Join(" · ", day.Games.Select(p => $"{p.Game} {ReportWriter.FormatDuration(p.Time)}"))
            : "No play";
        TextRenderer.DrawText(g, games, _fonts.Body, new Rectangle(gamesX, bounds.Y + S(11), Math.Max(0, gamesWidth), S(22)),
            played ? _theme.TextSecondary : _theme.TextMuted, TextFlags);
        if (played)
        {
            // Bar relative to the busiest day in the range.
            var max = _history.Max(d => d.Total.Ticks);
            RowPainter.ShareBar(g, new RectangleF(gamesX, bounds.Y + S(40), Math.Max(0, gamesWidth), S(4)), max == 0 ? 0 : day.Total.Ticks / (double)max, _theme);
            RowPainter.Chevron(g, bounds, hot, _theme, _fonts, UiScale);
        }
        if (index < _history.Count - 1 && !hot)
        {
            using var pen = new Pen(_theme.Separator, 1);
            g.SmoothingMode = SmoothingMode.None;
            g.DrawLine(pen, x, bounds.Bottom - 1, x + w, bounds.Bottom - 1);
        }
    }
}

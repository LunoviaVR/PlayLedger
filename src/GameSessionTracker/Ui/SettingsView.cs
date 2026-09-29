using System.Diagnostics;
using System.Reflection;

namespace GameSessionTracker.Ui;

/// <summary>Every setting, editable in place. Changes save immediately.</summary>
internal sealed class SettingsView : Panel
{
    private readonly ITrackerHost _host;
    private readonly Func<(Theme Theme, Fonts Fonts)> _style;

    private readonly SettingsCard _general = new() { Title = "General" };
    private readonly ToggleSwitch _startWithWindows = new();
    private readonly ToggleSwitch _notifications = new();
    private readonly ToggleSwitch _windowsGameList = new();

    private readonly SettingsCard _tracking = new() { Title = "Tracking" };
    private readonly Stepper _pollInterval = new() { Minimum = 1, Maximum = 300, Step = 1 };
    private readonly Stepper _minimumSession = new() { Minimum = 0, Maximum = 600, Step = 10 };
    private readonly Stepper _gracePeriod = new() { Minimum = 0, Maximum = 300, Step = 5 };

    private readonly ListEditor _customGames = new()
    {
        Title = "Custom games",
        Description = "Games the tracker doesn't find on its own. Pick the game's .exe file.",
        EmptyText = "No custom games. Use \"Add game...\" to pick one.",
    };
    private readonly ListEditor _gameFolders = new()
    {
        Title = "Game folders",
        Description = "Extra folders where every sub-folder is a game, e.g. D:\\Games.",
        EmptyText = "No extra folders.",
    };
    private readonly ListEditor _ignoredGames = new()
    {
        Title = "Ignored games",
        Description = "Never tracked. You can also right-click a game on the Overview.",
        EmptyText = "Nothing ignored.",
    };
    private readonly ListEditor _ignoredPrograms = new()
    {
        Title = "Ignored programs",
        Description = "Launchers, crash reporters and tools that never count as playing.",
        EmptyText = "Nothing ignored.",
    };

    private readonly SettingsCard _data = new() { Title = "Your data" };
    private readonly PillButton _rescan = new("Rescan");
    private readonly PillButton _export = new("Export...");
    private readonly PillButton _report = new("Open");
    private readonly PillButton _folder = new("Open folder");

    private bool _loading;

    public SettingsView(ITrackerHost host, Func<(Theme, Fonts)> style)
    {
        _host = host;
        _style = style;
        AutoScroll = true;
        DoubleBuffered = true;
        SetStyle(ControlStyles.SupportsTransparentBackColor | ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer, true);
        BackColor = Color.Transparent; // the window's backdrop shows through between the cards

        _general.AddRow("Start with Windows", "Start quietly in the system tray when you sign in.", _startWithWindows);
        _general.AddRow("Notifications", "Show a notification each time a session is logged.", _notifications);
        _general.AddRow("Use Windows' game list", "Also track programs the Xbox Game Bar recognises as games.", _windowsGameList);

        _pollInterval.Format = v => v == 1 ? "1 second" : $"{v} seconds";
        _minimumSession.Format = FormatSeconds;
        _gracePeriod.Format = FormatSeconds;
        _tracking.AddRow("Check for games every", "How often running programs are checked.", _pollInterval);
        _tracking.AddRow("Shortest session to keep", "Shorter sessions (updaters, crashes on launch) aren't recorded.", _minimumSession);
        _tracking.AddRow("Same-session window", "A game that closes and reopens within this time stays one session.", _gracePeriod);

        var version = Assembly.GetExecutingAssembly().GetName().Version;
        _data.Description = $"Game Session Tracker {version?.ToString(3)}";
        _data.AddRow("Rescan installed games", "Look for newly installed games right now.", _rescan);
        _data.AddRow("Export sessions", "Save every session to a spreadsheet file (.csv).", _export);
        _data.AddRow("Text report", "A plain-text summary of all your stats.", _report);
        _data.AddRow("Data folder", host.DataFolder, _folder);

        Controls.AddRange(new Control[] { _general, _tracking, _customGames, _gameFolders, _ignoredGames, _ignoredPrograms, _data });

        // ---- events ----
        _startWithWindows.CheckedChanged += (_, _) => { if (!_loading) _host.StartWithWindows = _startWithWindows.Checked; };
        _notifications.CheckedChanged += (_, _) => Change(s => s.ShowNotifications = _notifications.Checked);
        _windowsGameList.CheckedChanged += (_, _) => Change(s => s.UseWindowsGameList = _windowsGameList.Checked, detection: true);
        _pollInterval.ValueChanged += (_, _) => Change(s => s.PollIntervalSeconds = _pollInterval.Value);
        _minimumSession.ValueChanged += (_, _) => Change(s => s.MinimumSessionSeconds = _minimumSession.Value);
        _gracePeriod.ValueChanged += (_, _) => Change(s => s.GracePeriodSeconds = _gracePeriod.Value);

        _customGames.AddButton("Add game...").Click += (_, _) => AddCustomGame();
        _customGames.RemoveRequested += i => ChangeList(s => s.CustomGames.RemoveAt(i));

        _gameFolders.AddButton("Add folder...").Click += (_, _) => AddFolder();
        _gameFolders.RemoveRequested += i => ChangeList(s => s.ExtraGameFolders.RemoveAt(i));

        _ignoredGames.AddButton("Add...").Click += (_, _) => AddText(_ignoredGames.Title, "Game or folder name to ignore:", s => s.IgnoredGames);
        _ignoredGames.RemoveRequested += i => ChangeList(s => s.IgnoredGames.RemoveAt(i));

        _ignoredPrograms.AddButton("Browse...").Click += (_, _) => BrowseIgnoredProgram();
        _ignoredPrograms.AddButton("Add...").Click += (_, _) => AddText(_ignoredPrograms.Title, "Program file name to ignore (e.g. Launcher.exe):", s => s.IgnoredExecutables);
        _ignoredPrograms.RemoveRequested += i => ChangeList(s => s.IgnoredExecutables.RemoveAt(i));

        _rescan.Click += (_, _) =>
        {
            Cursor = Cursors.WaitCursor;
            var summary = _host.Rescan();
            Cursor = Cursors.Default;
            _data.SetRowDescription(_rescan, summary);
        };
        _export.Click += (_, _) => Export();
        _report.Click += (_, _) => _host.OpenTextReport();
        _folder.Click += (_, _) => _host.OpenDataFolder();

        Reload();
    }

    /// <summary>Re-reads every value from the tracker's settings (e.g. after settings.json was edited by hand).</summary>
    public void Reload()
    {
        _loading = true;
        try
        {
            var s = _host.Settings;
            _startWithWindows.Checked = _host.StartWithWindows;
            _notifications.Checked = s.ShowNotifications;
            _windowsGameList.Checked = s.UseWindowsGameList;
            _pollInterval.Value = s.PollIntervalSeconds;
            _minimumSession.Value = s.MinimumSessionSeconds;
            _gracePeriod.Value = s.GracePeriodSeconds;
            _customGames.SetItems(s.CustomGames.Select(g => (g.Name, g.Executable)));
            _gameFolders.SetItems(s.ExtraGameFolders.Select(f => (f, "")));
            _ignoredGames.SetItems(s.IgnoredGames.Select(n => (n, "")));
            _ignoredPrograms.SetItems(s.IgnoredExecutables.Select(n => (n, "")));
        }
        finally
        {
            _loading = false;
        }
        PerformLayout();
    }

    public void ApplyStyle()
    {
        var (theme, fonts) = _style();
        foreach (var control in AllPainted())
        {
            control.Theme = theme;
            control.Fonts = fonts;
            control.Invalidate();
        }
        foreach (var editor in new[] { _customGames, _gameFolders, _ignoredGames, _ignoredPrograms })
            editor.ApplyTheme();
        if (IsHandleCreated)
            Theme.ApplyScrollbarTheme(this, theme.IsDark);
        PerformLayout();
    }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        Theme.ApplyScrollbarTheme(this, _style().Theme.IsDark);
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        Glass.PaintChildShadows(e.Graphics, this, e.ClipRectangle, _style().Theme);
    }

    // Native scrolling moves pixels, including the backdrop behind the cards; repaint so the backdrop stays put.
    protected override void OnScroll(ScrollEventArgs se)
    {
        base.OnScroll(se);
        Invalidate(true);
    }

    protected override void OnMouseWheel(MouseEventArgs e)
    {
        base.OnMouseWheel(e);
        Invalidate(true);
    }

    protected override void OnResize(EventArgs eventargs)
    {
        base.OnResize(eventargs);
        Invalidate(true);
    }

    private IEnumerable<PaintedControl> AllPainted() =>
        new PaintedControl[]
        {
            _general, _startWithWindows, _notifications, _windowsGameList,
            _tracking, _pollInterval, _minimumSession, _gracePeriod,
            _customGames, _gameFolders, _ignoredGames, _ignoredPrograms,
            _data, _rescan, _export, _report, _folder,
        };

    protected override void OnLayout(LayoutEventArgs levent)
    {
        base.OnLayout(levent);
        var scale = DeviceDpi / 96f;
        int S(float v) => (int)Math.Round(v * scale);

        // Left-aligned with the header; very wide windows don't stretch the rows past a readable width.
        var windowWidth = Parent?.ClientSize.Width ?? ClientSize.Width; // same gutters as the dashboard
        var gutter = windowWidth >= S(1280) ? S(32) : windowWidth >= S(960) ? S(24) : S(16);
        var width = Math.Min(S(960), ClientSize.Width - gutter * 2);
        if (width <= 0)
            return;
        var x = gutter;
        var y = S(8) + AutoScrollPosition.Y;
        var gap = S(16);

        void Place(Control c, int height)
        {
            c.Bounds = new Rectangle(x, y, width, height);
            y += height + gap;
        }

        Place(_general, _general.PreferredHeight);
        Place(_tracking, _tracking.PreferredHeight);
        Place(_customGames, _customGames.PreferredHeight);
        Place(_gameFolders, _gameFolders.PreferredHeight);
        Place(_ignoredGames, _ignoredGames.PreferredHeight);
        Place(_ignoredPrograms, _ignoredPrograms.PreferredHeight);
        Place(_data, _data.PreferredHeight);
        AutoScrollMinSize = new Size(0, y - AutoScrollPosition.Y + S(16));
    }

    // ---------- changes ----------

    private void Change(Action<Settings> change, bool detection = false)
    {
        if (!_loading)
            _host.UpdateSettings(change, detection);
    }

    private void ChangeList(Action<Settings> change)
    {
        _host.UpdateSettings(change, affectsGameDetection: true);
        Reload();
    }

    private void AddCustomGame()
    {
        using var dialog = new OpenFileDialog
        {
            Title = "Pick the game's program file",
            Filter = "Programs (*.exe)|*.exe",
            CheckFileExists = true,
        };
        if (dialog.ShowDialog(FindForm()) != DialogResult.OK)
            return;
        var (theme, fonts) = _style();
        var name = PromptDialog.Ask(FindForm()!, theme, fonts, "Add custom game", "Name to show for this game:", SuggestName(dialog.FileName));
        if (name is null)
            return;
        var path = dialog.FileName;
        ChangeList(s => s.CustomGames.Add(new CustomGame { Name = name, Executable = path }));
    }

    private void AddFolder()
    {
        using var dialog = new FolderBrowserDialog
        {
            Description = "Pick a folder where each sub-folder is a game",
            UseDescriptionForTitle = true,
        };
        if (dialog.ShowDialog(FindForm()) != DialogResult.OK)
            return;
        var path = dialog.SelectedPath;
        if (_host.Settings.ExtraGameFolders.Contains(path, StringComparer.OrdinalIgnoreCase))
            return;
        ChangeList(s => s.ExtraGameFolders.Add(path));
    }

    private void AddText(string title, string label, Func<Settings, List<string>> list)
    {
        var (theme, fonts) = _style();
        var value = PromptDialog.Ask(FindForm()!, theme, fonts, title, label);
        if (value is null || list(_host.Settings).Contains(value, StringComparer.OrdinalIgnoreCase))
            return;
        ChangeList(s => list(s).Add(value));
    }

    private void BrowseIgnoredProgram()
    {
        using var dialog = new OpenFileDialog { Title = "Pick a program to ignore", Filter = "Programs (*.exe)|*.exe", CheckFileExists = true };
        if (dialog.ShowDialog(FindForm()) != DialogResult.OK)
            return;
        var name = Path.GetFileName(dialog.FileName);
        if (_host.Settings.IgnoredExecutables.Contains(name, StringComparer.OrdinalIgnoreCase))
            return;
        ChangeList(s => s.IgnoredExecutables.Add(name));
    }

    private void Export()
    {
        using var dialog = new SaveFileDialog
        {
            Title = "Export sessions",
            Filter = "Spreadsheet (*.csv)|*.csv",
            FileName = $"Game sessions {DateTime.Now:yyyy-MM-dd}.csv",
            OverwritePrompt = true,
        };
        if (dialog.ShowDialog(FindForm()) != DialogResult.OK)
            return;
        try
        {
            _host.ExportCsv(dialog.FileName);
            _data.SetRowDescription(_export, $"Saved to {dialog.FileName}");
        }
        catch (Exception ex)
        {
            MessageBox.Show(FindForm(), $"Couldn't save the file:\n{ex.Message}", "Export sessions", MessageBoxButtons.OK, MessageBoxIcon.Warning);
        }
    }

    private static string SuggestName(string exe)
    {
        try
        {
            var info = FileVersionInfo.GetVersionInfo(exe);
            if (!string.IsNullOrWhiteSpace(info.ProductName) && info.ProductName.Trim() is not ("Unity" or "UnrealGame"))
                return info.ProductName.Trim();
        }
        catch
        {
            // fall through
        }
        return Path.GetFileNameWithoutExtension(exe);
    }

    private static string FormatSeconds(int seconds) =>
        seconds == 0 ? "Off" : seconds < 60 ? $"{seconds} seconds" : seconds % 60 == 0 ? $"{seconds / 60} min" : $"{seconds / 60} min {seconds % 60} s";
}

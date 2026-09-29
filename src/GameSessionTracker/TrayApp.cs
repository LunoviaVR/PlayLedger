using System.Diagnostics;
using System.Reflection;
using GameSessionTracker.Ui;
using Microsoft.Win32;

namespace GameSessionTracker;

/// <summary>The system-tray icon and the polling loop that drives everything.</summary>
internal sealed class TrayApp : ApplicationContext, ITrackerHost
{
    private static readonly TimeSpan CatalogRefreshInterval = TimeSpan.FromMinutes(10);
    private static readonly TimeSpan ActiveSaveInterval = TimeSpan.FromSeconds(60);

    private readonly string _dataFolder;
    private readonly string _settingsPath;
    private readonly string _dataPath;
    private readonly string _statsPath;
    private readonly string _csvPath;

    private readonly NotifyIcon _trayIcon;
    private readonly ToolStripMenuItem _statusItem;
    private readonly System.Windows.Forms.Timer _timer;
    private readonly SynchronizationContext _ui;
    private readonly ProcessScanner _scanner = new();
    private readonly TrackerData _data;
    private readonly SessionTracker _tracker;

    private Settings _settings;
    private DateTime _settingsWriteTime;
    private GameCatalog _catalog;
    private DateTimeOffset _catalogBuiltAt;
    private DateTimeOffset _lastPoll;
    private DateTimeOffset _lastSave;
    private readonly RegisteredWaitHandle _exitWait;
    private readonly RegisteredWaitHandle _showWait;
    private DashboardForm? _dashboard;
    private long _dataVersion;
    private bool _shutDown;

    public TrayApp(bool launchedAtStartup, WaitHandle exitRequested, WaitHandle showRequested)
    {
        _dataFolder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "Game Session Tracker");
        Directory.CreateDirectory(_dataFolder);
        _settingsPath = Path.Combine(_dataFolder, "settings.json");
        _dataPath = Path.Combine(_dataFolder, "sessions.json");
        _statsPath = Path.Combine(_dataFolder, "Game Stats.txt");
        _csvPath = Path.Combine(_dataFolder, "Sessions.csv");
        ErrorLog.Initialize(Path.Combine(_dataFolder, "errors.log"));

        _settings = LoadSettingsOrDefaults(out var settingsError);
        _settingsWriteTime = SafeWriteTime(_settingsPath);
        ConfigureStartupOnFirstRun();

        _data = TrackerData.Load(_dataPath);
        _tracker = new SessionTracker(_data, _settings);
        _tracker.SessionEnded += OnSessionEnded;
        _catalog = BuildCatalog();

        // ---- Tray icon & menu ----
        _statusItem = new ToolStripMenuItem("Not playing anything") { Enabled = false };
        var openDashboard = new ToolStripMenuItem("Open dashboard", null, (_, _) => ShowDashboard()) { Font = new Font(SystemFonts.MenuFont ?? Control.DefaultFont, FontStyle.Bold) };

        var menu = new ContextMenuStrip();
        menu.Items.AddRange(new ToolStripItem[]
        {
            _statusItem,
            new ToolStripSeparator(),
            openDashboard,
            new ToolStripMenuItem("Settings", null, (_, _) => ShowDashboard(DashboardForm.SettingsTab)),
            new ToolStripSeparator(),
            new ToolStripMenuItem("Exit", null, (_, _) => ExitApp()),
        });
        // Same glass menu styling as the dashboard, following the current light/dark setting.
        menu.Opening += (_, _) => ThemedMenu.Apply(menu, Ui.Theme.Current(), SystemFonts.MenuFont ?? Control.DefaultFont);

        _trayIcon = new NotifyIcon
        {
            Icon = LoadIcon(),
            Text = "Game Session Tracker",
            ContextMenuStrip = menu,
            Visible = true,
        };
        _trayIcon.MouseClick += (_, e) => { if (e.Button == MouseButtons.Left) ShowDashboard(); };

        // Creating controls above installed the WinForms synchronization context.
        _ui = SynchronizationContext.Current ?? new WindowsFormsSynchronizationContext();

        SystemEvents.SessionEnding += OnSessionEnding;
        SystemEvents.PowerModeChanged += OnPowerModeChanged;
        Application.ApplicationExit += (_, _) => Shutdown();

        // Another copy started with --exit (the installer/uninstaller) asked us to close.
        _exitWait = ThreadPool.RegisterWaitForSingleObject(
            exitRequested, (_, _) => _ui.Post(_ => ExitApp(), null), null, Timeout.Infinite, executeOnlyOnce: true);
        // The app was launched again (e.g. from the Start menu): show the dashboard instead.
        _showWait = ThreadPool.RegisterWaitForSingleObject(
            showRequested, (_, _) => _ui.Post(_ => ShowDashboard(), null), null, Timeout.Infinite, executeOnlyOnce: false);

        SaveAll();

        _timer = new System.Windows.Forms.Timer { Interval = _settings.PollIntervalSeconds * 1000 };
        _timer.Tick += (_, _) => Poll();
        _timer.Start();
        Poll();

        if (settingsError is not null)
            Notify("settings.json has a problem", $"Using default settings until it's fixed. {settingsError}", ToolTipIcon.Warning);
        else if (!launchedAtStartup)
            ShowDashboard();
    }

    // ---------- Polling ----------

    private void Poll()
    {
        if (_shutDown)
            return;
        try
        {
            var now = DateTimeOffset.Now;
            var changed = false;

            ReloadSettingsIfChanged();

            if (now - _catalogBuiltAt > CatalogRefreshInterval)
            {
                _catalog = BuildCatalog();
                _scanner.Invalidate();
            }

            // A big gap between polls means the PC was asleep (and we missed the event), so
            // close sessions where they were last seen rather than counting the sleep as playtime.
            if (_lastPoll != default && (now - _lastPoll).TotalSeconds > Math.Max(90, _settings.PollIntervalSeconds * 6))
                changed |= _tracker.EndAll();

            changed |= _tracker.Update(now, _scanner.Scan(_catalog));
            _lastPoll = now;

            if (changed || (_tracker.Active.Count > 0 && now - _lastSave >= ActiveSaveInterval))
                SaveAll();
            if (changed)
                _dashboard?.RefreshData();

            UpdateStatus(now);
        }
        catch (Exception ex)
        {
            ErrorLog.Write("Error while checking for running games", ex);
        }
    }

    private void SaveAll()
    {
        var now = DateTimeOffset.Now;
        try
        {
            _data.Save(_dataPath);
            ReportWriter.WriteStats(_statsPath, _data.Sessions, _tracker.Active, now);
            ReportWriter.WriteCsv(_csvPath, _data.Sessions);
        }
        catch (Exception ex)
        {
            // Most likely the CSV is open in Excel (which locks it); we'll try again next time.
            ErrorLog.Write("Could not save session files", ex);
        }
        _lastSave = now;
    }

    private void UpdateStatus(DateTimeOffset now)
    {
        var active = _tracker.Active.OrderBy(a => a.Start).ToList();
        string status = active.Count == 0
            ? "Not playing anything"
            : "Playing: " + string.Join(", ", active.Select(a => $"{a.Game} ({ReportWriter.FormatDuration(now - a.Start)})"));

        _statusItem.Text = status.Length > 100 ? status[..97] + "..." : status;
        var tooltip = "Game Session Tracker\n" + status;
        _trayIcon.Text = tooltip.Length > 127 ? tooltip[..124] + "..." : tooltip; // Windows limit is 127 chars
    }

    private void OnSessionEnded(SessionRecord session)
    {
        if (_settings.ShowNotifications && !_shutDown)
            Notify("Session logged", $"{session.Game}: {ReportWriter.FormatDuration(session.Duration)}", ToolTipIcon.Info);
    }

    // ---------- Settings & catalog ----------

    private Settings LoadSettingsOrDefaults(out string? error)
    {
        error = null;
        try
        {
            return Settings.Load(_settingsPath);
        }
        catch (Exception ex)
        {
            ErrorLog.Write("Could not read settings.json", ex);
            error = ex.Message;
            return new Settings();
        }
    }

    private void ReloadSettingsIfChanged()
    {
        var writeTime = SafeWriteTime(_settingsPath);
        if (writeTime == _settingsWriteTime)
            return;
        _settingsWriteTime = writeTime;

        try
        {
            _settings = Settings.Load(_settingsPath);
        }
        catch (Exception ex)
        {
            ErrorLog.Write("Could not read settings.json", ex);
            Notify("settings.json has a problem", $"Your changes weren't applied. {ex.Message}", ToolTipIcon.Warning);
            return;
        }

        ApplySettings(rebuildCatalog: true);
        _dashboard?.ReloadSettings();
    }

    private void ApplySettings(bool rebuildCatalog)
    {
        _tracker.Settings = _settings;
        _timer.Interval = _settings.PollIntervalSeconds * 1000;
        if (rebuildCatalog)
        {
            _catalog = BuildCatalog();
            _scanner.Invalidate();
        }
    }

    private GameCatalog BuildCatalog()
    {
        _catalogBuiltAt = DateTimeOffset.Now;
        return GameCatalog.Build(_settings);
    }

    // ---------- ITrackerHost (used by the dashboard) ----------

    public DashboardModel GetModel() => new(_data.Sessions, _tracker.Active, DateTimeOffset.Now, _dataVersion);

    public Settings Settings => _settings;

    public string DataFolder => _dataFolder;

    public void UpdateSettings(Action<Settings> change, bool affectsGameDetection = false)
    {
        change(_settings);
        _settings.Normalize();
        try
        {
            _settings.Save(_settingsPath);
            _settingsWriteTime = SafeWriteTime(_settingsPath);
        }
        catch (Exception ex)
        {
            ErrorLog.Write("Could not save settings.json", ex);
        }
        ApplySettings(affectsGameDetection);
        if (affectsGameDetection)
            Poll();
    }

    public bool StartWithWindows
    {
        get => SafeIsStartupEnabled();
        set
        {
            try
            {
                StartupManager.SetEnabled(value);
            }
            catch (Exception ex)
            {
                ErrorLog.Write("Could not change start with Windows", ex);
                MessageBox.Show($"Couldn't change the startup setting:\n{ex.Message}", "Game Session Tracker", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }
    }

    public int DeleteSessions(IReadOnlyCollection<SessionRecord> sessions)
    {
        var toDelete = new HashSet<SessionRecord>(sessions, ReferenceEqualityComparer.Instance);
        var removed = _data.Sessions.RemoveAll(toDelete.Contains);
        if (removed > 0)
        {
            _dataVersion++;
            SaveAll();
        }
        return removed;
    }

    public string Rescan()
    {
        _catalog = BuildCatalog();
        _scanner.Invalidate();
        Poll();
        return $"Found {_catalog.LocationCount} installed games in {_catalog.RootCount} library folders.";
    }

    public void ExportCsv(string path) => ReportWriter.WriteCsv(path, _data.Sessions);

    public void OpenDataFolder() => Open(_dataFolder);

    public void OpenTextReport() => OpenStats();

    private void ConfigureStartupOnFirstRun()
    {
        try
        {
            if (!_settings.StartupConfigured)
            {
                StartupManager.SetEnabled(true);
                _settings.StartupConfigured = true;
                _settings.Save(_settingsPath);
                _settingsWriteTime = SafeWriteTime(_settingsPath);
            }
            else
            {
                StartupManager.RefreshPathIfEnabled();
            }
        }
        catch (Exception ex)
        {
            ErrorLog.Write("Could not configure start with Windows", ex);
        }
    }

    private static bool SafeIsStartupEnabled()
    {
        try
        {
            return StartupManager.IsEnabled();
        }
        catch
        {
            return false;
        }
    }

    // ---------- System events ----------

    private void OnSessionEnding(object? sender, SessionEndingEventArgs e) =>
        _ui.Send(_ => Shutdown(), null); // Windows is signing out / shutting down: log running games now.

    private void OnPowerModeChanged(object? sender, PowerModeChangedEventArgs e)
    {
        _ui.Post(_ =>
        {
            if (_shutDown)
                return;
            if (e.Mode == PowerModes.Suspend)
            {
                // Don't count time asleep as playtime; still-running games start a new session on wake.
                // (If a poll sneaks in before the PC actually sleeps, the gap check in Poll closes it.)
                if (_tracker.EndAll(DateTimeOffset.Now))
                    SaveAll();
            }
            else if (e.Mode == PowerModes.Resume)
            {
                Poll();
            }
        }, null);
    }

    // ---------- Actions ----------

    private void ShowDashboard() => ShowDashboard(tab: null);

    private void ShowDashboard(int? tab)
    {
        if (_shutDown)
            return;
        if (_dashboard is null || _dashboard.IsDisposed)
        {
            _dashboard = new DashboardForm(this);
            _dashboard.FormClosed += (_, _) => _dashboard = null;
            _dashboard.Show();
        }
        if (tab is { } t)
            _dashboard.ShowTab(t);
        if (_dashboard.WindowState == FormWindowState.Minimized)
            _dashboard.WindowState = FormWindowState.Normal;
        _dashboard.Activate();
        _dashboard.BringToFront();
    }

    private void OpenStats()
    {
        SaveAll(); // so "now playing" times are current
        Open(_statsPath);
    }

    private static void Open(string path)
    {
        try
        {
            Process.Start(new ProcessStartInfo(path) { UseShellExecute = true });
        }
        catch (Exception ex)
        {
            ErrorLog.Write($"Could not open {path}", ex);
        }
    }

    private void Notify(string title, string text, ToolTipIcon icon)
    {
        try
        {
            _trayIcon.ShowBalloonTip(5000, title, text, icon);
        }
        catch
        {
            // Notifications are best-effort.
        }
    }

    private void ExitApp()
    {
        Shutdown();
        ExitThread();
    }

    /// <summary>Logs any running games and saves. Safe to call more than once.</summary>
    private void Shutdown()
    {
        if (_shutDown)
            return;
        _shutDown = true;

        _timer.Stop();
        _tracker.EndAll(DateTimeOffset.Now);
        SaveAll();

        SystemEvents.SessionEnding -= OnSessionEnding;
        SystemEvents.PowerModeChanged -= OnPowerModeChanged;
        _dashboard?.Close();
        _trayIcon.Visible = false;
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            Shutdown();
            _exitWait.Unregister(null);
            _showWait.Unregister(null);
            _timer.Dispose();
            _trayIcon.Dispose();
        }
        base.Dispose(disposing);
    }

    private static DateTime SafeWriteTime(string path)
    {
        try
        {
            return File.Exists(path) ? File.GetLastWriteTimeUtc(path) : default;
        }
        catch
        {
            return default;
        }
    }

    private static Icon LoadIcon()
    {
        using var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("GameSessionTracker.app.ico");
        return stream is null ? (Icon)SystemIcons.Application.Clone() : new Icon(stream, SystemInformation.SmallIconSize);
    }
}

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

    // Set when a protected file couldn't be opened for an unexpected reason (e.g. locked by another program). The file
    // on disk may still hold good data, so it isn't overwritten during this run.
    private bool _dataReadOnly;
    private bool _settingsReadOnly;

    // ---- Updates (GitHub releases) ----
    private static readonly TimeSpan FirstUpdateCheckDelay = TimeSpan.FromMinutes(1);
    private static readonly TimeSpan UpdateCheckInterval = TimeSpan.FromHours(6);
    private readonly Updater _updater = new();
    private readonly ToolStripMenuItem _updateItem;
    private DateTimeOffset _nextUpdateCheck = DateTimeOffset.Now + FirstUpdateCheckDelay;
    private string? _notifiedUpdate;
    private bool _autoInstallPending;
    private bool _balloonOpensSettings;
    private bool _shutDown;

    public TrayApp(bool launchedAtStartup, WaitHandle exitRequested, WaitHandle showRequested)
    {
        _dataFolder = DataFolderMigration.Resolve(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments));
        Directory.CreateDirectory(_dataFolder);
        // Protected (DPAPI) files, so playtimes and settings can't be edited by hand; see ProtectedStore.
        _settingsPath = Path.Combine(_dataFolder, "settings.dat");
        _dataPath = Path.Combine(_dataFolder, "sessions.dat");
        _statsPath = Path.Combine(_dataFolder, "Game Stats.txt");
        _csvPath = Path.Combine(_dataFolder, "Sessions.csv");
        ErrorLog.Initialize(Path.Combine(_dataFolder, "errors.log"));

        _settings = LoadSettingsOrDefaults(out var settingsError);
        _settingsWriteTime = SafeWriteTime(_settingsPath);
        ConfigureStartupOnFirstRun();
        var updatedFrom = NoteVersionRun();

        string? dataWarning;
        try
        {
            _data = TrackerData.Load(_dataPath, Path.Combine(_dataFolder, "sessions.json"), out dataWarning);
        }
        catch (Exception ex)
        {
            // Couldn't read or protect the history (e.g. the folder is read-only). Start with an empty history in
            // memory; the files on disk are left as they are.
            ErrorLog.Write("Could not load the play history", ex);
            _data = new TrackerData();
            _dataReadOnly = true;
            dataWarning = "Your play history couldn't be opened, so this session won't be saved (your existing history is untouched). " +
                          "Restart Playtime Tracker to try again; details are in errors.log.";
        }
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
            (_updateItem = new ToolStripMenuItem("Install update...", null, (_, _) => ShowDashboard(DashboardForm.SettingsTab)) { Visible = false }),
            new ToolStripMenuItem("Exit", null, (_, _) => ExitApp()),
        });
        // Same glass menu styling as the dashboard, following the current light/dark setting.
        menu.Opening += (_, _) => ThemedMenu.Apply(menu, Ui.Theme.Resolve(_settings), SystemFonts.MenuFont ?? Control.DefaultFont);

        _trayIcon = new NotifyIcon
        {
            Icon = LoadIcon(),
            Text = "Playtime Tracker",
            ContextMenuStrip = menu,
            Visible = true,
        };
        _trayIcon.MouseClick += (_, e) => { if (e.Button == MouseButtons.Left) ShowDashboard(); };
        _trayIcon.BalloonTipClicked += (_, _) =>
        {
            if (_balloonOpensSettings)
                ShowDashboard(DashboardForm.SettingsTab);
            else
                ShowDashboard();
        };
        _updater.StateChanged += OnUpdaterStateChanged;

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

        if (updatedFrom is not null)
            Notify("Playtime Tracker updated", $"You're now on version {Updater.CurrentVersion.ToString(3)} (was {updatedFrom}).", ToolTipIcon.Info);
        if (dataWarning is not null)
            Notify("Playtime Tracker history", dataWarning, ToolTipIcon.Warning);
        if (settingsError is not null)
            Notify("Playtime Tracker settings", settingsError, ToolTipIcon.Warning);
        else if (dataWarning is null && !launchedAtStartup)
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
            MaybeCheckForUpdates(now);
            MaybeAutoInstall();
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
            if (_dataReadOnly)
                return; // never replace a history we couldn't read with an empty one
            _data.Save(_dataPath);
            // The readable reports are regenerated on every save; they're read-only so it's clear editing them changes nothing.
            ReportWriter.WriteStats(_statsPath, _data.Sessions, _tracker.Active, now, readOnly: true);
            ReportWriter.WriteCsv(_csvPath, _data.Sessions, readOnly: true);
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
        var tooltip = "Playtime Tracker\n" + status;
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
        try
        {
            return Settings.Load(_settingsPath, Path.Combine(_dataFolder, "settings.json"), out error);
        }
        catch (Exception ex)
        {
            ErrorLog.Write("Could not load settings", ex);
            error = "Your settings couldn't be opened, so defaults are in use until the next start. Details are in errors.log.";
            _settingsReadOnly = true;
            return new Settings();
        }
    }

    /// <summary>
    /// The settings file only changes through the app. If it changed on disk anyway, accept it only if it verifies (e.g.
    /// a restored copy); otherwise set it aside and write the current settings back.
    /// </summary>
    private void ReloadSettingsIfChanged()
    {
        var writeTime = SafeWriteTime(_settingsPath);
        if (writeTime == _settingsWriteTime)
            return;

        try
        {
            _settings = Settings.LoadVerified(_settingsPath);
            _settingsWriteTime = writeTime;
        }
        catch (Exception ex)
        {
            ErrorLog.Write("The settings file was changed outside the app and didn't verify", ex);
            try
            {
                if (File.Exists(_settingsPath))
                    ProtectedStore.SetAside(_settingsPath);
                SaveSettings();
            }
            catch (Exception saveEx)
            {
                ErrorLog.Write("Could not restore the settings file", saveEx);
            }
            _settingsWriteTime = SafeWriteTime(_settingsPath);
            Notify("Settings restored", "The settings file was changed outside Playtime Tracker, so your settings were written back.", ToolTipIcon.Warning);
            return;
        }

        ApplySettings(rebuildCatalog: true);
        _dashboard?.ReloadSettings();
    }

    private void SaveSettings()
    {
        if (!_settingsReadOnly)
            _settings.Save(_settingsPath);
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
            SaveSettings();
            _settingsWriteTime = SafeWriteTime(_settingsPath);
        }
        catch (Exception ex)
        {
            ErrorLog.Write("Could not save settings", ex);
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
                MessageBox.Show($"Couldn't change the startup setting:\n{ex.Message}", "Playtime Tracker", MessageBoxButtons.OK, MessageBoxIcon.Warning);
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

    public Updater Updater => _updater;

    public async Task<bool> InstallUpdateAsync()
    {
        if (_updater.Available is not { } update)
            return false;
        if (!await _updater.DownloadAndStartInstallerAsync(update))
            return false;
        // The installer replaces the app and starts the new version; exit now so any game in progress is logged first.
        ExitApp();
        return true;
    }

    // ---------- Updates ----------

    /// <summary>Remembers which version ran; returns the previous version if this start follows an update.</summary>
    private string? NoteVersionRun()
    {
        var current = Updater.CurrentVersion.ToString(3);
        var previous = _settings.LastRunVersion;
        if (previous == current)
            return null;
        _settings.LastRunVersion = current;
        try
        {
            SaveSettings();
            _settingsWriteTime = SafeWriteTime(_settingsPath);
        }
        catch (Exception ex)
        {
            ErrorLog.Write("Could not save settings", ex);
        }
        return string.IsNullOrEmpty(previous) ? null : previous;
    }

    private async void MaybeCheckForUpdates(DateTimeOffset now)
    {
        if (!_settings.CheckForUpdates || _updater.Busy || now < _nextUpdateCheck)
            return;
        _nextUpdateCheck = now + UpdateCheckInterval;
        var update = await _updater.CheckAsync(); // never throws
        if (update is null || _shutDown)
            return;

        if (_settings.InstallUpdatesAutomatically && update.CanInstall && Updater.IsInstalledCopy)
        {
            _autoInstallPending = true; // installs as soon as no game is running (see MaybeAutoInstall)
        }
        else if (_notifiedUpdate != update.Tag)
        {
            _notifiedUpdate = update.Tag;
            Notify("Update available", $"Playtime Tracker {update.Version.ToString(3)} is out. Click to see it in Settings.", ToolTipIcon.Info, opensSettings: true);
        }
    }

    /// <summary>Installs a pending update, but never while a game is being tracked (the install restarts the app).</summary>
    private async void MaybeAutoInstall()
    {
        if (!_autoInstallPending || _shutDown || _updater.Busy || _tracker.Active.Count > 0 ||
            !_settings.CheckForUpdates || !_settings.InstallUpdatesAutomatically)
            return;
        _autoInstallPending = false;
        if (!await InstallUpdateAsync() && _updater.Available is { } update && _notifiedUpdate != update.Tag)
        {
            _notifiedUpdate = update.Tag;
            Notify("Update available", $"Playtime Tracker {update.Version.ToString(3)} couldn't be installed automatically. Click to try from Settings.", ToolTipIcon.Warning, opensSettings: true);
        }
    }

    private void OnUpdaterStateChanged()
    {
        if (_updater.Available is { } update)
        {
            _updateItem.Text = $"Update to {update.Version.ToString(3)}...";
            _updateItem.Visible = true;
        }
        else
        {
            _updateItem.Visible = false;
        }
    }

    private void ConfigureStartupOnFirstRun()
    {
        try
        {
            StartupManager.MigrateLegacyEntry(); // "GameSessionTracker" → "PlaytimeTracker", keeping on/off as it was
            if (!_settings.StartupConfigured)
            {
                StartupManager.SetEnabled(true);
                _settings.StartupConfigured = true;
                SaveSettings();
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

    /// <param name="opensSettings">Clicking the notification opens Settings (updates) rather than the Overview.</param>
    private void Notify(string title, string text, ToolTipIcon icon, bool opensSettings = false)
    {
        _balloonOpensSettings = opensSettings;
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
            _updater.Dispose();
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

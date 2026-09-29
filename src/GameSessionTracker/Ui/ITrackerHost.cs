namespace GameSessionTracker.Ui;

/// <summary>What the dashboard can ask of the running tracker (implemented by the tray app).</summary>
internal interface ITrackerHost
{
    DashboardModel GetModel();

    /// <summary>The live settings. Change them only through <see cref="UpdateSettings"/>.</summary>
    Settings Settings { get; }

    /// <summary>Applies a change, saves the (protected) settings file and puts it into effect immediately.</summary>
    void UpdateSettings(Action<Settings> change, bool affectsGameDetection = false);

    bool StartWithWindows { get; set; }

    /// <summary>Deletes the given finished sessions from the history. Returns how many were removed.</summary>
    int DeleteSessions(IReadOnlyCollection<SessionRecord> sessions);

    /// <summary>Re-reads installed games; returns a short summary for the user.</summary>
    string Rescan();

    void ExportCsv(string path);
    void OpenDataFolder();
    void OpenTextReport();
    string DataFolder { get; }

    /// <summary>Update checks against GitHub releases.</summary>
    Updater Updater { get; }

    /// <summary>Downloads, verifies and starts the available update; the app then exits and the installer restarts it.
    /// Returns false (see <see cref="Updater.LastError"/>) if it couldn't.</summary>
    Task<bool> InstallUpdateAsync();
}

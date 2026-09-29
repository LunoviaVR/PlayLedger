namespace GameSessionTracker;

internal static class Program
{
    // These names predate the rename to Playtime Tracker. Keeping them means an older copy that is still running
    // is detected (and can be closed by the installer) instead of both tracking at once.
    private const string SingleInstanceMutexName = @"Local\GameSessionTracker.SingleInstance";
    private const string ExitEventName = @"Local\GameSessionTracker.Exit";
    private const string ShowEventName = @"Local\GameSessionTracker.Show";

    [STAThread]
    private static void Main(string[] args)
    {
        // Used by the installer/uninstaller to close a running tracker cleanly (so the current session is logged).
        if (args.Contains("--exit", StringComparer.OrdinalIgnoreCase))
        {
            RequestRunningInstanceExit();
            return;
        }

        using var mutex = new Mutex(initiallyOwned: true, SingleInstanceMutexName, out var isFirstInstance);
        if (!isFirstInstance)
        {
            // Already running: ask that copy to show its dashboard.
            if (EventWaitHandle.TryOpenExisting(ShowEventName, out var show))
            {
                using (show)
                    show.Set();
                return;
            }
            MessageBox.Show(
                "Playtime Tracker is already running.\n\nLook for the controller icon in the system tray (click the ^ arrow next to the clock if you don't see it).",
                "Playtime Tracker", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }

        using var exitEvent = new EventWaitHandle(false, EventResetMode.AutoReset, ExitEventName);
        using var showEvent = new EventWaitHandle(false, EventResetMode.AutoReset, ShowEventName);

        ApplicationConfiguration.Initialize();
        Application.SetUnhandledExceptionMode(UnhandledExceptionMode.CatchException);
        Application.ThreadException += (_, e) => ErrorLog.Write("Unexpected error", e.Exception);

        // --updated: started by the installer after an update; stays in the tray like a sign-in start.
        var launchedAtStartup = args.Contains("--startup", StringComparer.OrdinalIgnoreCase) ||
                                args.Contains("--updated", StringComparer.OrdinalIgnoreCase);
        using var app = new TrayApp(launchedAtStartup, exitEvent, showEvent);
        Application.Run(app);
    }

    /// <summary>Signals a running tracker to exit and waits (up to 15 s) for it to finish saving.</summary>
    private static void RequestRunningInstanceExit()
    {
        if (!EventWaitHandle.TryOpenExisting(ExitEventName, out var exitEvent))
            return; // not running
        using (exitEvent)
            exitEvent.Set();

        var deadline = DateTime.UtcNow.AddSeconds(15);
        while (DateTime.UtcNow < deadline)
        {
            if (!Mutex.TryOpenExisting(SingleInstanceMutexName, out var mutex))
                return; // the tracker has exited
            mutex.Dispose();
            Thread.Sleep(250);
        }
    }
}

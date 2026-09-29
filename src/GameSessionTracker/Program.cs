namespace GameSessionTracker;

internal static class Program
{
    [STAThread]
    private static void Main(string[] args)
    {
        using var mutex = new Mutex(initiallyOwned: true, @"Local\GameSessionTracker.SingleInstance", out var isFirstInstance);
        if (!isFirstInstance)
        {
            MessageBox.Show(
                "Game Session Tracker is already running.\n\nLook for the controller icon in the system tray (click the ^ arrow next to the clock if you don't see it).",
                "Game Session Tracker", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }

        ApplicationConfiguration.Initialize();
        Application.SetUnhandledExceptionMode(UnhandledExceptionMode.CatchException);
        Application.ThreadException += (_, e) => ErrorLog.Write("Unexpected error", e.Exception);

        var launchedAtStartup = args.Contains("--startup", StringComparer.OrdinalIgnoreCase);
        using var app = new TrayApp(launchedAtStartup);
        Application.Run(app);
    }
}

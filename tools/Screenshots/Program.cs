using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using GameSessionTracker.Ui;

namespace GameSessionTracker.Screenshots;

/// <summary>Opens the dashboard with sample data and saves a PNG of each page for the README.</summary>
internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        var outDir = Path.GetFullPath(args.Length > 0 ? args[0] : "docs");
        Directory.CreateDirectory(outDir);
        Application.SetHighDpiMode(HighDpiMode.PerMonitorV2);
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);

        // Park the pointer in the bottom-right corner, away from every window, so no hover state (row highlight, chart
        // tooltip) ends up in the pictures.
        var screen = SystemInformation.VirtualScreen;
        Cursor.Position = new Point(screen.Right - 1, screen.Bottom - 1);

        try
        {
            var dark = new DemoHost("dark");
            CaptureDashboard(dark, outDir, (DashboardForm.OverviewTab, "dashboard.png"), (DashboardForm.GamesTab, "games.png"),
                (DashboardForm.HistoryTab, "history.png"), (DashboardForm.SettingsTab, "settings.png"));
            CaptureDashboard(new DemoHost("light"), outDir, (DashboardForm.OverviewTab, "dashboard-light.png"));
            CaptureSessionDetails(dark, Path.Combine(outDir, "session-details.png"));
            return 0;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine(ex);
            return 1;
        }
    }

    private static void CaptureDashboard(DemoHost host, string outDir, params (int Tab, string File)[] pages)
    {
        using var form = new DashboardForm(host) { StartPosition = FormStartPosition.Manual, Location = new Point(0, 0) };
        form.Show();
        // Stay inside the screen: parts of a window beyond its edge aren't rendered, so they'd come out blank.
        var area = Screen.PrimaryScreen?.WorkingArea ?? new Rectangle(0, 0, 1024, 728);
        form.ClientSize = new Size(Math.Min(1000, area.Width - 24), Math.Min(680, area.Height - 48));
        foreach (var (tab, file) in pages)
        {
            form.ShowTab(tab);
            Save(form, Path.Combine(outDir, file));
        }
        form.Close();
    }

    private static void CaptureSessionDetails(DemoHost host, string path)
    {
        var model = host.GetModel();
        var session = model.Sessions.First(s => !s.IsLive && s.Game == "Elden Ring");
        using var fonts = new Fonts(1f);
        using var dialog = new SessionDetailsDialog(host, Theme.Resolve(host.Settings), fonts, session)
        {
            StartPosition = FormStartPosition.Manual,
            Location = new Point(0, 0),
        };
        dialog.Show();
        Save(dialog, path);
        dialog.Close();
    }

    /// <summary>Lets the window finish painting, then copies its client area (what the app draws, no title bar).</summary>
    private static void Save(Form form, string path)
    {
        for (var i = 0; i < 20; i++)
        {
            Application.DoEvents();
            Thread.Sleep(25);
        }
        form.Refresh();
        Application.DoEvents();

        var size = form.ClientSize;
        using var bitmap = new Bitmap(size.Width, size.Height, PixelFormat.Format32bppArgb);
        using (var g = Graphics.FromImage(bitmap))
        {
            var hdc = g.GetHdc();
            try
            {
                if (!PrintWindow(form.Handle, hdc, PwClientOnly | PwRenderFullContent))
                    throw new InvalidOperationException($"PrintWindow failed for {Path.GetFileName(path)}");
            }
            finally
            {
                g.ReleaseHdc(hdc);
            }
        }
        bitmap.Save(path, ImageFormat.Png);
        Console.WriteLine($"{Path.GetFileName(path)}: {size.Width}x{size.Height}");
    }

    private const uint PwClientOnly = 0x1;
    private const uint PwRenderFullContent = 0x2;

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
}

/// <summary>A stand-in for the tray app: fixed sample history, one game running, nothing is saved.</summary>
internal sealed class DemoHost : ITrackerHost
{
    private readonly List<SessionRecord> _sessions;
    private readonly List<ActiveSession> _active;

    public DemoHost(string themeMode)
    {
        Settings = new Settings { ThemeMode = themeMode, AccentColor = "blue", StartupConfigured = true };
        Settings.CustomGames.Add(new CustomGame { Name = "Minecraft", Executable = "javaw.exe" });
        Settings.ExtraGameFolders.Add(@"D:\Games");
        (_sessions, _active) = SampleData(DateTimeOffset.Now);
    }

    public Settings Settings { get; }
    public string DataFolder => @"C:\Users\you\Documents\Playtime Tracker";
    public bool StartWithWindows { get; set; } = true;

    public DashboardModel GetModel() => new(_sessions, _active, DateTimeOffset.Now);
    public void UpdateSettings(Action<Settings> change, bool affectsGameDetection = false) => change(Settings);
    public int DeleteSessions(IReadOnlyCollection<SessionRecord> sessions) => 0;
    public string Rescan() => "Found 6 installed games.";
    public void ExportCsv(string path) { }
    public void OpenDataFolder() { }
    public void OpenTextReport() { }
    public Updater Updater { get; } = new(); // never asked to check in the screenshots
    public Task<bool> InstallUpdateAsync() => Task.FromResult(false);

    /// <summary>A believable month of play (fixed seed, so every run looks the same relative to today).</summary>
    private static (List<SessionRecord>, List<ActiveSession>) SampleData(DateTimeOffset now)
    {
        var games = new (string Name, string Exe, double Weight, int MinMinutes, int MaxMinutes)[]
        {
            ("Elden Ring", @"D:\SteamLibrary\steamapps\common\ELDEN RING\Game\eldenring.exe", 5, 45, 170),
            ("Baldur's Gate 3", @"D:\SteamLibrary\steamapps\common\Baldurs Gate 3\bin\bg3.exe", 4, 60, 200),
            ("Hades II", @"C:\Program Files (x86)\Steam\steamapps\common\Hades II\Ship\Hades2.exe", 3, 25, 90),
            ("Stardew Valley", @"C:\Program Files (x86)\Steam\steamapps\common\Stardew Valley\Stardew Valley.exe", 2, 30, 120),
            ("Rocket League", @"C:\Program Files\Epic Games\rocketleague\Binaries\Win64\RocketLeague.exe", 2, 15, 60),
            ("Beat Saber", @"C:\Program Files (x86)\Steam\steamapps\common\Beat Saber\Beat Saber.exe", 1, 20, 50),
        };
        var random = new Random(20260929);
        var totalWeight = games.Sum(g => g.Weight);
        var sessions = new List<SessionRecord>();
        var today = now.ToLocalTime().Date;
        for (var daysAgo = 44; daysAgo >= 0; daysAgo--)
        {
            // Some days off; more play at weekends.
            var day = today.AddDays(-daysAgo);
            var weekend = day.DayOfWeek is DayOfWeek.Saturday or DayOfWeek.Sunday;
            if (random.NextDouble() < (weekend ? 0.1 : 0.35))
                continue;
            var count = weekend ? random.Next(2, 4) : random.Next(1, 3);
            var start = day.AddHours(weekend ? 13 + random.Next(0, 3) : 18 + random.NextDouble() * 2);
            for (var i = 0; i < count; i++)
            {
                var pick = random.NextDouble() * totalWeight;
                var game = games.First(g => (pick -= g.Weight) <= 0);
                var minutes = random.Next(game.MinMinutes, game.MaxMinutes);
                var end = start.AddMinutes(minutes).AddSeconds(random.Next(0, 60));
                if (end > now.LocalDateTime.AddHours(-1))
                    break; // leave today's evening free for the game that's running now
                sessions.Add(new SessionRecord { Game = game.Name, Start = new DateTimeOffset(start), End = new DateTimeOffset(end), Executable = game.Exe });
                start = end.AddMinutes(random.Next(10, 90));
            }
        }
        var active = new List<ActiveSession>
        {
            new() { Game = "Hades II", Start = now.AddMinutes(-47).AddSeconds(-12), LastSeen = now, Executable = games[2].Exe },
        };
        return (sessions, active);
    }
}

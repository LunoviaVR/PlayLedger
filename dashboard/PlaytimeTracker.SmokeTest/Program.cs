using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using PlaytimeTracker.Dashboard.Core;

// End-to-end smoke test of an installed Playtime Tracker on a real Windows machine:
//   PlaytimeTracker.SmokeTest <install folder> [screenshot folder]
// Starts the tracker, talks to it over its pipe exactly as the dashboard does, makes it track a real process,
// checks the protected history on disk, restarts the tracker to check the history survives, and (optionally)
// screenshots each dashboard page. Exits non-zero on the first failure.

if (args.Length < 1)
{
    Console.Error.WriteLine("usage: PlaytimeTracker.SmokeTest <install folder> [screenshot folder]");
    return 2;
}
var install = Path.GetFullPath(args[0]);
var screenshots = args.Length > 1 ? Path.GetFullPath(args[1]) : null;
var tracker = Path.Combine(install, TrackerClient.TrackerExeName);
var dashboard = Path.Combine(install, "Dashboard", "PlaytimeTracker.Dashboard.exe");
var dataFolder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "Playtime Tracker");
const string GameName = "Smoke Game";

var failed = false;
async Task Step(string name, Func<Task> action)
{
    if (failed)
        return;
    var clock = Stopwatch.StartNew();
    try
    {
        await action();
        Console.WriteLine($"PASS  {name} ({clock.Elapsed.TotalSeconds:0.0}s)");
    }
    catch (Exception ex)
    {
        failed = true;
        Console.WriteLine($"FAIL  {name}: {ex.GetType().Name}: {ex.Message}");
    }
}

static void Check(bool condition, string message)
{
    if (!condition)
        throw new InvalidOperationException(message);
}

static async Task<T> WaitFor<T>(Func<Task<T?>> probe, TimeSpan timeout, string what) where T : class
{
    var deadline = DateTime.UtcNow + timeout;
    while (true)
    {
        try
        {
            if (await probe() is { } value)
                return value;
        }
        catch (TrackerUnavailableException) when (DateTime.UtcNow < deadline)
        {
        }
        if (DateTime.UtcNow > deadline)
            throw new TimeoutException($"timed out waiting for {what}");
        await Task.Delay(500);
    }
}

Process? trackerProcess = null;
TrackerClient? client = null;
var events = new List<TrackerEvent>();
var eventsConnected = new TaskCompletionSource();

async Task StartTracker()
{
    Check(File.Exists(tracker), $"{tracker} isn't installed");
    trackerProcess = Process.Start(new ProcessStartInfo(tracker, "--startup") { UseShellExecute = false });
    Check(trackerProcess is not null, "the tracker didn't start");
    client = new TrackerClient();
    client.EventReceived += e => { lock (events) events.Add(e); };
    client.ConnectionChanged += connected => { if (connected) eventsConnected.TrySetResult(); };
    await WaitFor<object>(async () => { await client.ConnectAsync(); return new object(); }, TimeSpan.FromSeconds(30), "the tracker's pipe");
}

async Task StopTracker()
{
    if (client is not null)
        await client.DisposeAsync();
    client = null;
    using var exit = Process.Start(new ProcessStartInfo(tracker, "--exit") { UseShellExecute = false });
    Check(exit is not null, "couldn't run --exit");
    await exit!.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(30));
    await trackerProcess!.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(30));
    Check(trackerProcess.HasExited, "the tracker didn't exit");
}

var gamesRoot = Path.Combine(Path.GetTempPath(), "PlaytimeTracker-SmokeGames");
Process? game = null;

await Step("the tracker exe carries its name and version (Task Manager shows these)", () =>
{
    var info = FileVersionInfo.GetVersionInfo(tracker);
    Check(info.FileDescription == "Playtime Tracker", $"FileDescription is '{info.FileDescription}'");
    Check(info.ProductName == "Playtime Tracker", $"ProductName is '{info.ProductName}'");
    Check(!string.IsNullOrEmpty(info.ProductVersion), "no ProductVersion");
    using var icon = Icon.ExtractAssociatedIcon(tracker);
    Check(icon is not null, "no icon");
    return Task.CompletedTask;
});

await Step("tracker starts and answers over its pipe", async () =>
{
    await StartTracker();
    Check(client!.TrackerVersion is not null, "no version in hello");
    Console.WriteLine($"      tracker {client.TrackerVersion}");
});

await Step("settings round-trip (fast polling, a game folder)", async () =>
{
    // The folder must exist when it's added: discovery skips folders that don't (as the C# app does).
    Directory.CreateDirectory(Path.Combine(gamesRoot, GameName));
    var response = await client!.GetSettingsAsync();
    Check(response.IsInstalledCopy, "the installed tracker doesn't recognise itself as installed");
    var settings = response.Settings;
    settings.PollIntervalSeconds = 1;
    settings.GracePeriodSeconds = 0;
    settings.MinimumSessionSeconds = 0;
    settings.ExtraGameFolders = settings.ExtraGameFolders.Append(gamesRoot).Distinct().ToList();
    await client.UpdateSettingsAsync(settings);
    var saved = (await client.GetSettingsAsync()).Settings;
    Check(saved.PollIntervalSeconds == 1 && saved.ExtraGameFolders.Contains(gamesRoot), "settings weren't saved");
});

await Step("event stream connects", async () =>
{
    await eventsConnected.Task.WaitAsync(TimeSpan.FromSeconds(30));
});

await Step("a running game is tracked live", async () =>
{
    // Any real program in a game folder counts; a copy of ping.exe waits quietly for a minute.
    var folder = Path.Combine(gamesRoot, GameName);
    Directory.CreateDirectory(folder);
    var exe = Path.Combine(folder, "smoke-game.exe");
    File.Copy(Path.Combine(Environment.SystemDirectory, "PING.EXE"), exe, overwrite: true);
    game = Process.Start(new ProcessStartInfo(exe, "-n 120 127.0.0.1") { UseShellExecute = false, CreateNoWindow = true });
    Check(game is not null, "couldn't start the test game");
    // The catalog is rebuilt when game folders change; the poll interval switches to 1 s on the next tick.
    var live = await WaitFor(async () =>
    {
        var s = await client!.GetDashboardAsync();
        return s.Live.FirstOrDefault(l => l.Game == GameName);
    }, TimeSpan.FromSeconds(40), "the game to show as playing");
    Check(live.IsLive, "not live");
});

await Step("closing the game records a session", async () =>
{
    game!.Kill();
    await game.WaitForExitAsync();
    var session = await WaitFor(async () =>
    {
        var s = await client!.GetDashboardAsync();
        return s.Sessions.FirstOrDefault(x => x.Game == GameName && !x.IsLive);
    }, TimeSpan.FromSeconds(30), "the finished session");
    Check(session.Executable?.EndsWith("smoke-game.exe", StringComparison.OrdinalIgnoreCase) == true, "wrong executable recorded");
    // Events come over their own connection, so they can arrive just after the snapshot shows the session.
    TrackerEvent? Seen<T>(Func<T, bool> match) where T : TrackerEvent
    {
        lock (events) return events.OfType<T>().FirstOrDefault(match);
    }
    await WaitFor(() => Task.FromResult(Seen<SessionStartedEvent>(e => e.Game == GameName)), TimeSpan.FromSeconds(10), "the sessionStarted event");
    await WaitFor(() => Task.FromResult(Seen<SessionEndedEvent>(e => e.Session.Game == GameName)), TimeSpan.FromSeconds(10), "the sessionEnded event");
});

await Step("history is protected on disk and reports are read-only", async () =>
{
    var sessions = Path.Combine(dataFolder, "sessions.dat");
    Check(File.Exists(sessions), "sessions.dat missing");
    var header = new byte[8];
    await using (var file = new FileStream(sessions, FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
        await file.ReadExactlyAsync(header);
    Check(System.Text.Encoding.ASCII.GetString(header) == "PTDATA2\n", "sessions.dat isn't in the protected format");
    var stats = Path.Combine(dataFolder, "Game Stats.txt");
    Check(File.ReadAllText(stats).Contains(GameName), "Game Stats.txt doesn't list the game");
    Check(new FileInfo(stats).IsReadOnly, "Game Stats.txt isn't read-only");
    Check(File.ReadAllText(Path.Combine(dataFolder, "Sessions.csv")).Contains(GameName), "Sessions.csv doesn't list the game");
    // While the tracker runs, other programs can't change the history.
    var locked = false;
    try
    {
        await using var write = new FileStream(sessions, FileMode.Open, FileAccess.Write, FileShare.ReadWrite);
    }
    catch (IOException)
    {
        locked = true;
    }
    Check(locked, "sessions.dat could be opened for writing while the tracker runs");
});

await Step("the tracker exits cleanly with --exit", StopTracker);

await Step("the history survives a restart", async () =>
{
    await StartTracker();
    var s = await client!.GetDashboardAsync();
    Check(s.Sessions.Any(x => x.Game == GameName), "the session was lost");
    Check(s.Games.Any(g => g.Name == GameName), "the game was lost");
});

if (screenshots is not null)
{
    await Step("dashboard pages render (screenshots)", async () =>
    {
        Check(File.Exists(dashboard), $"{dashboard} isn't installed");
        Directory.CreateDirectory(screenshots);
        foreach (var page in new[] { "overview", "games", "history", "statistics", "settings" })
        {
            using var window = Process.Start(new ProcessStartInfo(dashboard, $"--page {page}") { UseShellExecute = false });
            Check(window is not null, "the dashboard didn't start");
            var hwnd = await WaitFor(async () =>
            {
                await Task.Delay(250);
                window!.Refresh();
                if (window.HasExited)
                    throw new InvalidOperationException($"the dashboard exited with code 0x{window.ExitCode:X8} before showing its window");
                return window.MainWindowHandle != IntPtr.Zero ? (object)window.MainWindowHandle : null;
            }, TimeSpan.FromSeconds(45), $"the {page} window");
            var handle = (IntPtr)hwnd;
            Native.Fit(handle);
            await Task.Delay(TimeSpan.FromSeconds(6)); // connect, load data and artwork, settle animations
            Check(!window!.HasExited, $"the dashboard closed on the {page} page");
            Native.Capture(handle, Path.Combine(screenshots, $"next-{page}.png"));
            if (page == "overview")
            {
                // Make the same window much wider, as maximising does: the page must follow and fill it.
                Native.Resize(handle, 1800, 900);
                await Task.Delay(TimeSpan.FromSeconds(3));
                Native.Capture(handle, Path.Combine(screenshots, "next-overview-wide.png"));
            }
            window.Kill();
            await window.WaitForExitAsync();
        }
    });

    await Step("the accent colour setting changes the dashboard's colours", async () =>
    {
        // Rose is far from any Windows default accent, so its shades on screen can only come from the setting.
        var settings = (await client!.GetSettingsAsync()).Settings;
        var before = settings.AccentColor;
        settings.AccentColor = "rose";
        await client.UpdateSettingsAsync(settings);
        try
        {
            using var window = Process.Start(new ProcessStartInfo(dashboard, "--page settings") { UseShellExecute = false });
            Check(window is not null, "the dashboard didn't start");
            var hwnd = await WaitFor(async () =>
            {
                await Task.Delay(250);
                window!.Refresh();
                return window.MainWindowHandle != IntPtr.Zero ? (object)window.MainWindowHandle : null;
            }, TimeSpan.FromSeconds(45), "the settings window");
            Native.Fit((IntPtr)hwnd);
            await Task.Delay(TimeSpan.FromSeconds(6));
            var path = Path.Combine(screenshots, "next-accent.png");
            Native.Capture((IntPtr)hwnd, path);
            window!.Kill();
            await window.WaitForExitAsync();
            // Accent buttons use the Dark1 shade in the light theme and Light2 in the dark theme.
            var rose = Accent.Shades(Accent.Resolve("rose")!.Value);
            var pixels = Native.CountNear(path, new[] { rose.Dark1, rose.Light2 }, tolerance: 12);
            Console.WriteLine($"      {pixels} pixels in the rose accent");
            if (pixels < 200)
                Console.WriteLine($"      most common strong colours: {Native.TopColors(path, 6)}");
            Check(pixels >= 200, "the dashboard doesn't show the chosen accent colour");
        }
        finally
        {
            settings.AccentColor = before;
            await client.UpdateSettingsAsync(settings);
        }
    });
}

await Step("a session can be deleted", async () =>
{
    var s = await client!.GetDashboardAsync();
    var session = s.Sessions.First(x => x.Game == GameName && !x.IsLive);
    await client.DeleteSessionAsync(session.Game, session.Start);
    var after = await client.GetDashboardAsync();
    Check(!after.Sessions.Any(x => x.Game == GameName && x.Start == session.Start), "the session is still there");
});

await Step("the tracker exits cleanly again", StopTracker);

if (game is { HasExited: false })
    game.Kill();
if (failed)
{
    var dashboardLog = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Playtime Tracker", "dashboard-errors.log");
    foreach (var path in new[] { Path.Combine(dataFolder, "errors.log"), Path.Combine(dataFolder, "migration.log"), dashboardLog })
    {
        var log = Path.GetFileName(path);
        if (File.Exists(path))
            Console.WriteLine($"\n--- {log} ---\n{File.ReadAllText(path)}");
    }
}
Console.WriteLine(failed ? "\nSMOKE TEST FAILED" : "\nSMOKE TEST PASSED");
return failed ? 1 : 0;

static class Native
{
    [StructLayout(LayoutKind.Sequential)]
    private struct Rect
    {
        public int Left, Top, Right, Bottom;
    }

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    private static extern int GetSystemMetrics(int index);

    private const uint PwRenderFullContent = 2;
    private const uint SwpNoZOrder = 0x0004;

    /// <summary>Fits the window on the (small) CI screen so every part of it is actually rendered.</summary>
    public static void Fit(IntPtr hwnd)
    {
        var width = Math.Min(1180, GetSystemMetrics(0));
        var height = Math.Min(800, GetSystemMetrics(1) - 48);
        SetWindowPos(hwnd, IntPtr.Zero, 0, 0, width, height, SwpNoZOrder);
        SetForegroundWindow(hwnd);
    }

    /// <summary>Sets the window's size, even beyond the (small) CI screen; the capture still renders all of it.</summary>
    public static void Resize(IntPtr hwnd, int width, int height) =>
        SetWindowPos(hwnd, IntPtr.Zero, 0, 0, width, height, SwpNoZOrder);

    public static void Capture(IntPtr hwnd, string path)
    {
        GetWindowRect(hwnd, out var rect);
        using var bitmap = new Bitmap(Math.Max(1, rect.Right - rect.Left), Math.Max(1, rect.Bottom - rect.Top));
        using (var graphics = Graphics.FromImage(bitmap))
        {
            var hdc = graphics.GetHdc();
            try
            {
                PrintWindow(hwnd, hdc, PwRenderFullContent);
            }
            finally
            {
                graphics.ReleaseHdc(hdc);
            }
        }
        bitmap.Save(path, ImageFormat.Png);
    }

    /// <summary>The most common clearly coloured (not grey) pixels, as "#rrggbb×count", for diagnosing a failure.</summary>
    public static string TopColors(string path, int top)
    {
        using var bitmap = new Bitmap(path);
        var counts = new Dictionary<int, int>();
        for (var y = 0; y < bitmap.Height; y++)
        {
            for (var x = 0; x < bitmap.Width; x++)
            {
                var p = bitmap.GetPixel(x, y);
                if (Math.Max(p.R, Math.Max(p.G, p.B)) - Math.Min(p.R, Math.Min(p.G, p.B)) < 40)
                    continue;
                var rgb = (p.R << 16) | (p.G << 8) | p.B;
                counts[rgb] = counts.GetValueOrDefault(rgb) + 1;
            }
        }
        return string.Join(", ", counts.OrderByDescending(c => c.Value).Take(top).Select(c => $"#{c.Key:x6}×{c.Value}"));
    }

    /// <summary>How many pixels of an image are within <paramref name="tolerance"/> of any of the colours.</summary>
    public static int CountNear(string path, IReadOnlyList<Rgb> colors, int tolerance)
    {
        using var bitmap = new Bitmap(path);
        var count = 0;
        for (var y = 0; y < bitmap.Height; y++)
        {
            for (var x = 0; x < bitmap.Width; x++)
            {
                var p = bitmap.GetPixel(x, y);
                if (colors.Any(c => Math.Abs(p.R - c.R) <= tolerance && Math.Abs(p.G - c.G) <= tolerance && Math.Abs(p.B - c.B) <= tolerance))
                    count++;
            }
        }
        return count;
    }
}

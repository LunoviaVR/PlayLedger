using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;

namespace PlaytimeTracker.Dashboard;

public partial class App : Application
{
    // One dashboard window: a second launch (tray icon, Start menu) signals the first to come forward instead.
    private const string InstanceMutexName = @"Local\PlaytimeTracker.Dashboard";
    private const string ShowEventName = @"Local\PlaytimeTracker.Dashboard.Show";
    private const string ShowSettingsEventName = @"Local\PlaytimeTracker.Dashboard.ShowSettings";

    private MainWindow? _window;
    private Mutex? _instance;

    /// <summary>The dashboard window (file pickers need its handle).</summary>
    public static Window? CurrentWindow { get; private set; }

    /// <summary>The dashboard's shared state (connection and latest data).</summary>
    public static AppState State { get; } = new();

    static App()
    {
        // Registered before anything else so even a failure while starting up is recorded.
        AppDomain.CurrentDomain.UnhandledException += (_, e) =>
        {
            if (e.ExceptionObject is Exception ex)
                CrashLog.Write("Unhandled", ex);
        };
        TaskScheduler.UnobservedTaskException += (_, e) => CrashLog.Write("Unobserved task", e.Exception);
    }

    public App()
    {
        InitializeComponent();
        // Missing resources and broken bindings are otherwise silent (or an unexplained parse error).
        DebugSettings.IsXamlResourceReferenceTracingEnabled = true;
        DebugSettings.XamlResourceReferenceFailed += (_, e) => CrashLog.Write("XAML resource", e.Message);
        DebugSettings.IsBindingTracingEnabled = true;
        DebugSettings.BindingFailed += (_, e) => CrashLog.Write("Binding", e.Message);
        UnhandledException += (_, e) =>
        {
            // Keep the window alive on unexpected UI errors; the tracker (and the data) are unaffected.
            e.Handled = true;
            CrashLog.Write("UI", e.Exception);
            State.ReportError(e.Exception.Message);
        };
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        // `--page settings` (from the tray menu's Settings item) opens that page.
        var arguments = Environment.GetCommandLineArgs();
        var pageIndex = Array.FindIndex(arguments, a => a.Equals("--page", StringComparison.OrdinalIgnoreCase));
        var page = pageIndex >= 0 && pageIndex + 1 < arguments.Length ? arguments[pageIndex + 1] : null;

        _instance = new Mutex(true, InstanceMutexName, out var createdNew);
        if (!createdNew)
        {
            AllowSetForegroundWindow(AsfwAny);
            var name = string.Equals(page, "settings", StringComparison.OrdinalIgnoreCase) ? ShowSettingsEventName : ShowEventName;
            if (EventWaitHandle.TryOpenExisting(name, out var show))
            {
                using (show)
                    show.Set();
            }
            Exit();
            return;
        }

        try
        {
            _window = new MainWindow(page);
            CurrentWindow = _window;
            _window.Activate();
        }
        catch (Exception ex)
        {
            CrashLog.Write("Starting the window", ex);
            throw;
        }
        ListenForOtherLaunches(_window);
    }

    private static void ListenForOtherLaunches(MainWindow window)
    {
        var show = new EventWaitHandle(false, EventResetMode.AutoReset, ShowEventName);
        var showSettings = new EventWaitHandle(false, EventResetMode.AutoReset, ShowSettingsEventName);
        var queue = window.DispatcherQueue;
        var thread = new Thread(() =>
        {
            while (true)
            {
                var index = WaitHandle.WaitAny(new WaitHandle[] { show, showSettings });
                var page = index == 1 ? "settings" : null;
                queue.TryEnqueue(() => window.BringToFront(page));
            }
        })
        { IsBackground = true, Name = "dashboard-instance" };
        thread.Start();
    }

    private const int AsfwAny = -1;

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool AllowSetForegroundWindow(int processId);
}

namespace PlaytimeTracker.Dashboard;

/// <summary>
/// Records unexpected errors in <c>%LocalAppData%\Playtime Tracker\dashboard-errors.log</c> (capped at 1 MB), so a
/// dashboard that fails before its window can show anything still leaves a trace. Holds no personal data: only the
/// error and where it happened.
/// </summary>
public static class CrashLog
{
    private const long MaxBytes = 1024 * 1024;
    private static readonly object Gate = new();

    public static string Path => System.IO.Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Playtime Tracker", "dashboard-errors.log");

    public static void Write(string context, string message) => Write(context, new Exception(message));

    public static void Write(string context, Exception exception)
    {
        try
        {
            lock (Gate)
            {
                Directory.CreateDirectory(System.IO.Path.GetDirectoryName(Path)!);
                if (File.Exists(Path) && new FileInfo(Path).Length > MaxBytes)
                    File.Move(Path, Path + ".old", overwrite: true);
                // WinRT errors keep their detailed description (e.g. which XAML line failed) in Data.
                var details = string.Concat(exception.Data.Keys.Cast<object>()
                    .Select(key => $"{Environment.NewLine}    {key}: {exception.Data[key]}"));
                File.AppendAllText(Path, $"{DateTime.Now:yyyy-MM-dd HH:mm:ss}  {context}: {exception}{details}{Environment.NewLine}");
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // Nowhere to write; nothing more we can do.
        }
    }
}

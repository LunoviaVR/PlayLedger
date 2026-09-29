using System.Text;

namespace GameSessionTracker;

internal static class FileUtil
{
    /// <summary>Writes to a temp file then swaps it in, so a crash or power cut never leaves a half-written file.</summary>
    public static void WriteAllTextAtomic(string path, string contents)
    {
        var tempPath = path + ".tmp";
        File.WriteAllText(tempPath, contents, new UTF8Encoding(encoderShouldEmitUTF8Identifier: false));
        File.Move(tempPath, path, overwrite: true);
    }

    /// <summary>Writes with a UTF-8 BOM so Excel and Notepad pick the right encoding for game names.</summary>
    public static void WriteAllTextAtomicWithBom(string path, string contents)
    {
        var tempPath = path + ".tmp";
        File.WriteAllText(tempPath, contents, new UTF8Encoding(encoderShouldEmitUTF8Identifier: true));
        File.Move(tempPath, path, overwrite: true);
    }
}

/// <summary>Tiny append-only error log in the data folder, capped so it can't grow forever.</summary>
internal static class ErrorLog
{
    private const long MaxBytes = 1024 * 1024;
    private static string? _path;

    public static void Initialize(string path) => _path = path;

    public static void Write(string message, Exception? ex = null)
    {
        if (_path is null) return;
        try
        {
            if (File.Exists(_path) && new FileInfo(_path).Length > MaxBytes)
                File.Move(_path, _path + ".old", overwrite: true);
            File.AppendAllText(_path, $"[{DateTimeOffset.Now:yyyy-MM-dd HH:mm:ss}] {message}{(ex is null ? "" : Environment.NewLine + ex)}{Environment.NewLine}");
        }
        catch
        {
            // Logging must never take the app down.
        }
    }
}

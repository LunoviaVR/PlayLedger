using System.Text;

namespace GameSessionTracker;

internal static class FileUtil
{
    /// <summary>
    /// Writes to a temp file then swaps it in, so a crash or power cut never leaves a half-written file. With a
    /// <paramref name="backupPath"/>, the previous version is kept there.
    /// </summary>
    public static void WriteAllBytesAtomic(string path, byte[] contents, string? backupPath = null)
    {
        var tempPath = path + ".tmp";
        File.WriteAllBytes(tempPath, contents);
        if (backupPath is not null && File.Exists(path))
        {
            try
            {
                File.Replace(tempPath, path, backupPath, ignoreMetadataErrors: true);
                return;
            }
            catch (IOException)
            {
                // Some file systems don't support ReplaceFile; fall back to copy-then-move.
                File.Copy(path, backupPath, overwrite: true);
            }
        }
        File.Move(tempPath, path, overwrite: true);
    }

    /// <summary>
    /// Writes with a UTF-8 BOM so Excel and Notepad pick the right encoding for game names. <paramref name="readOnly"/>
    /// marks the file read-only afterwards, for reports the app regenerates (editing them would change nothing).
    /// </summary>
    public static void WriteAllTextAtomicWithBom(string path, string contents, bool readOnly = false)
    {
        var tempPath = path + ".tmp";
        File.WriteAllText(tempPath, contents, new UTF8Encoding(encoderShouldEmitUTF8Identifier: true));
        if (!readOnly)
        {
            File.Move(tempPath, path, overwrite: true);
            return;
        }
        // The app's own reports: read-only, and held open read-only while the app runs so they can't be changed.
        FileLocks.Release(path);
        try
        {
            ClearReadOnly(path); // our own earlier copy; replacing a read-only file would fail
            File.Move(tempPath, path, overwrite: true);
            File.SetAttributes(path, File.GetAttributes(path) | FileAttributes.ReadOnly);
        }
        finally
        {
            FileLocks.Hold(path);
        }
    }

    private static void ClearReadOnly(string path)
    {
        if (File.Exists(path) && File.GetAttributes(path).HasFlag(FileAttributes.ReadOnly))
            File.SetAttributes(path, File.GetAttributes(path) & ~FileAttributes.ReadOnly);
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

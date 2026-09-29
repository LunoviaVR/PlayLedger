using System.Security.Cryptography;
using System.Text;

namespace GameSessionTracker;

/// <summary>A protected data file exists but failed verification: it was edited, is damaged, or belongs to another
/// Windows account or PC.</summary>
internal sealed class UnverifiedDataException : Exception
{
    public UnverifiedDataException(string message, Exception? inner = null) : base(message, inner) { }
}

/// <summary>
/// Keeps the tracker's data files (play history and settings) from being changed by hand. Contents are encrypted and
/// integrity-protected with Windows DPAPI for the current user (<see cref="ProtectedData"/>), so an edited file fails
/// verification instead of being trusted. This stops manual edits; it can't stop a program running as the same
/// Windows user, which can use DPAPI too.
/// </summary>
internal static class ProtectedStore
{
    /// <summary>Format marker at the start of every protected file.</summary>
    private static readonly byte[] Header = "PTDATA1\n"u8.ToArray();

    /// <summary>Upper bound on what we'll read (years of sessions are a few MB); guards against absurd files.</summary>
    private const long MaxFileBytes = 64L * 1024 * 1024;

    public static void Write(string path, string purpose, string contents)
    {
        var blob = ProtectedData.Protect(Encoding.UTF8.GetBytes(contents), Entropy(purpose), DataProtectionScope.CurrentUser);
        var bytes = new byte[Header.Length + blob.Length];
        Header.CopyTo(bytes, 0);
        blob.CopyTo(bytes, Header.Length);
        FileUtil.WriteAllBytesAtomic(path, bytes, backupPath: path + ".bak");
    }

    /// <summary>Reads and verifies a protected file. Throws <see cref="UnverifiedDataException"/> if it doesn't verify.</summary>
    public static string Read(string path, string purpose)
    {
        var info = new FileInfo(path);
        if (info.Length > MaxFileBytes)
            throw new UnverifiedDataException($"{info.Name} is larger than expected.");
        var bytes = File.ReadAllBytes(path);
        if (!bytes.AsSpan().StartsWith(Header))
            throw new UnverifiedDataException($"{info.Name} isn't in Playtime Tracker's protected format.");
        try
        {
            var plain = ProtectedData.Unprotect(bytes[Header.Length..], Entropy(purpose), DataProtectionScope.CurrentUser);
            return new UTF8Encoding(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true).GetString(plain);
        }
        catch (Exception ex) when (ex is CryptographicException or DecoderFallbackException)
        {
            throw new UnverifiedDataException($"{info.Name} failed verification.", ex);
        }
    }

    /// <summary>
    /// Loads <paramref name="path"/>; if it doesn't verify (or <paramref name="parse"/> rejects it), sets it aside
    /// (renamed, never deleted) and falls back to the previous good copy. Returns null if there's nothing usable.
    /// </summary>
    public static T? LoadWithRecovery<T>(string path, string purpose, Func<string, T> parse, out string? warning) where T : class
    {
        warning = null;
        if (!File.Exists(path))
            return null;
        try
        {
            return parse(Read(path, purpose));
        }
        catch (Exception ex) when (ex is UnverifiedDataException or System.Text.Json.JsonException)
        {
            var setAside = SetAside(path);
            ErrorLog.Write($"{Path.GetFileName(path)} could not be verified; moved it to {Path.GetFileName(setAside)}.", ex);
            var backup = path + ".bak";
            if (File.Exists(backup))
            {
                try
                {
                    var restored = parse(Read(backup, purpose));
                    File.Copy(backup, path, overwrite: true);
                    warning = $"{Path.GetFileName(path)} was changed outside Playtime Tracker, so the last saved copy was restored. " +
                              $"The changed file was kept as {Path.GetFileName(setAside)}.";
                    return restored;
                }
                catch (Exception backupEx) when (backupEx is UnverifiedDataException or System.Text.Json.JsonException or IOException)
                {
                    ErrorLog.Write($"The backup of {Path.GetFileName(path)} could not be verified either.", backupEx);
                }
            }
            warning = $"{Path.GetFileName(path)} couldn't be verified (it was changed outside Playtime Tracker, or comes from another " +
                      $"Windows account or PC). It was kept as {Path.GetFileName(setAside)} and a new one was started.";
            return null;
        }
    }

    /// <summary>Renames a file that failed verification so it's kept for reference but never read again.</summary>
    public static string SetAside(string path)
    {
        var destination = $"{path}.unverified-{DateTime.Now:yyyyMMdd-HHmmss}";
        File.Move(path, destination, overwrite: true);
        return destination;
    }

    /// <summary>Per-file "purpose" mixed into DPAPI (optional entropy). Not a secret: it only stops one protected file
    /// from being passed off as another (e.g. settings copied over the play history).</summary>
    private static byte[] Entropy(string purpose) => Encoding.UTF8.GetBytes($"PlaytimeTracker/{purpose}/v1");
}

using System.Security.Cryptography;
using System.Text;
using Microsoft.Win32;

namespace GameSessionTracker;

/// <summary>A protected data file exists but failed verification: it was edited, is damaged, or belongs to another
/// Windows account or PC.</summary>
internal sealed class UnverifiedDataException : Exception
{
    public UnverifiedDataException(string message, Exception? inner = null) : base(message, inner) { }
}

/// <summary>
/// Keeps the tracker's data files (play history and settings) from being changed by hand:
/// <list type="bullet">
/// <item>Contents are encrypted and integrity-protected with Windows DPAPI for the current user
/// (<see cref="ProtectedData"/>), so an edited file fails verification instead of being trusted.</item>
/// <item>Every save carries a generation number, also recorded (DPAPI-protected) in the user's registry, so putting back an
/// older genuine copy (to undo playtime) is detected and the newest verified copy is used.</item>
/// <item>While the app runs, the files are held open read-only (<see cref="FileLocks"/>) so nothing else can modify,
/// replace or delete them.</item>
/// </list>
/// This stops manual edits; it can't stop a program running as the same Windows user, which can use DPAPI too.
/// </summary>
internal static class ProtectedStore
{
    /// <summary>Format markers. v1 files (before generations) are still read and are upgraded on the next save.</summary>
    private static readonly byte[] HeaderV1 = "PTDATA1\n"u8.ToArray();
    private static readonly byte[] HeaderV2 = "PTDATA2\n"u8.ToArray();

    /// <summary>Upper bound on what we'll read (years of sessions are a few MB); guards against absurd files.</summary>
    private const long MaxFileBytes = 64L * 1024 * 1024;

    /// <summary>Highest generation seen or written this run, per purpose, so the counter only ever goes up.</summary>
    private static readonly Dictionary<string, long> Generations = new(StringComparer.Ordinal);

    public static void Write(string path, string purpose, string contents)
    {
        long generation;
        lock (Generations)
        {
            generation = Math.Max(Generations.GetValueOrDefault(purpose), IntegrityRegistry.Read(purpose) ?? 0) + 1;
            Generations[purpose] = generation;
        }
        var plain = Encoding.UTF8.GetBytes($"{generation}\n{contents}");
        var blob = ProtectedData.Protect(plain, Entropy(purpose), DataProtectionScope.CurrentUser);
        var bytes = new byte[HeaderV2.Length + blob.Length];
        HeaderV2.CopyTo(bytes, 0);
        blob.CopyTo(bytes, HeaderV2.Length);

        var backup = path + ".bak";
        FileLocks.Release(path);
        FileLocks.Release(backup);
        try
        {
            FileUtil.WriteAllBytesAtomic(path, bytes, backupPath: backup);
        }
        finally
        {
            FileLocks.Hold(path);
            FileLocks.Hold(backup);
        }
        IntegrityRegistry.Write(purpose, generation);
    }

    /// <summary>Reads and verifies a protected file. Throws <see cref="UnverifiedDataException"/> if it doesn't verify.</summary>
    public static string Read(string path, string purpose) => ReadWithGeneration(path, purpose).Contents;

    private static (string Contents, long Generation) ReadWithGeneration(string path, string purpose)
    {
        var info = new FileInfo(path);
        if (info.Length > MaxFileBytes)
            throw new UnverifiedDataException($"{info.Name} is larger than expected.");
        var bytes = File.ReadAllBytes(path);
        var v2 = bytes.AsSpan().StartsWith(HeaderV2);
        if (!v2 && !bytes.AsSpan().StartsWith(HeaderV1))
            throw new UnverifiedDataException($"{info.Name} isn't in Playtime Tracker's protected format.");
        string text;
        try
        {
            var plain = ProtectedData.Unprotect(bytes[HeaderV2.Length..], Entropy(purpose), DataProtectionScope.CurrentUser);
            text = new UTF8Encoding(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true).GetString(plain);
        }
        catch (Exception ex) when (ex is CryptographicException or DecoderFallbackException)
        {
            throw new UnverifiedDataException($"{info.Name} failed verification.", ex);
        }
        if (!v2)
            return (text, 0);
        var newline = text.IndexOf('\n');
        if (newline <= 0 || !long.TryParse(text.AsSpan(0, newline), System.Globalization.NumberStyles.None, null, out var generation))
            throw new UnverifiedDataException($"{info.Name} has an invalid header.");
        return (text[(newline + 1)..], generation);
    }

    /// <summary>
    /// Loads the newest verified copy among <paramref name="path"/> and its backup. A file that doesn't verify (or that
    /// <paramref name="parse"/> rejects) is set aside, renamed and never deleted. An older genuine copy put back over a
    /// newer one is detected via the generation number. Returns null if there's nothing usable.
    /// </summary>
    public static T? LoadWithRecovery<T>(string path, string purpose, Func<string, T> parse, out string? warning) where T : class
    {
        warning = null;
        var backup = path + ".bak";
        if (!File.Exists(path) && !File.Exists(backup))
            return null;

        var main = TryLoad(path, purpose, parse, out var mainError);
        var bak = TryLoad(backup, purpose, parse, out _);
        var recorded = IntegrityRegistry.Read(purpose);
        var name = Path.GetFileName(path);

        if (mainError is not null && File.Exists(path))
        {
            var setAside = SetAside(path, "unverified");
            ErrorLog.Write($"{name} could not be verified; moved it to {Path.GetFileName(setAside)}.", mainError);
            warning = $"{name} was changed outside Playtime Tracker. The changed file was kept as {Path.GetFileName(setAside)}";
        }

        // Use the newest verified copy.
        var useBackup = bak is not null && (main is null || bak.Value.Generation > main.Value.Generation);
        var chosen = useBackup ? bak : main;
        if (chosen is null)
        {
            if (warning is not null)
                warning += " and a new one was started.";
            else
                warning = $"{name} couldn't be verified (it comes from another Windows account or PC), so a new one was started.";
            RememberGeneration(purpose, recorded ?? 0);
            return null;
        }

        if (useBackup)
        {
            if (main is not null)
            {
                var older = SetAside(path, "older");
                ErrorLog.Write($"{name} was older than its backup (an older copy was put back); moved it to {Path.GetFileName(older)}.");
                warning ??= $"An older copy of {name} was put back; the newer saved copy was restored.";
            }
            FileLocks.Release(path);
            File.Copy(backup, path, overwrite: true);
            warning = warning is null || warning.EndsWith('.') ? warning : warning + ", and the last saved copy was restored.";
        }
        else if (warning is not null && !warning.EndsWith('.'))
        {
            warning += ".";
        }

        // Restoring the backup already explains a one-save gap, so only report missing data when nothing else happened.
        if (recorded is { } expected && chosen.Value.Generation < expected && warning is null)
        {
            ErrorLog.Write($"{name} is generation {chosen.Value.Generation} but generation {expected} was saved last; newer data is missing.");
            warning = $"An older copy of {name} was put back, and the newer data couldn't be found. The older copy is in use.";
        }

        RememberGeneration(purpose, Math.Max(chosen.Value.Generation, recorded ?? 0));
        FileLocks.Hold(path);
        FileLocks.Hold(backup);
        return chosen.Value.Value;
    }

    private static (T Value, long Generation)? TryLoad<T>(string path, string purpose, Func<string, T> parse, out Exception? error)
    {
        error = null;
        if (!File.Exists(path))
            return null;
        try
        {
            var (contents, generation) = ReadWithGeneration(path, purpose);
            return (parse(contents), generation);
        }
        catch (Exception ex) when (ex is UnverifiedDataException or System.Text.Json.JsonException)
        {
            error = ex;
            return null;
        }
    }

    private static void RememberGeneration(string purpose, long generation)
    {
        lock (Generations)
            Generations[purpose] = Math.Max(Generations.GetValueOrDefault(purpose), generation);
    }

    /// <summary>Renames a file so it's kept for reference but never read again.</summary>
    public static string SetAside(string path, string reason = "unverified")
    {
        FileLocks.Release(path);
        var destination = $"{path}.{reason}-{DateTime.Now:yyyyMMdd-HHmmss}";
        File.Move(path, destination, overwrite: true);
        return destination;
    }

    /// <summary>Per-file "purpose" mixed into DPAPI (optional entropy). Not a secret: it only stops one protected file
    /// from being passed off as another (e.g. settings copied over the play history).</summary>
    private static byte[] Entropy(string purpose) => Encoding.UTF8.GetBytes($"PlaytimeTracker/{purpose}/v1");

    /// <summary>
    /// The last saved generation of each protected file, kept DPAPI-protected under HKCU so it can't be lowered by hand.
    /// Deleting it only loses the rollback check until the next save; it never loses data.
    /// </summary>
    private static class IntegrityRegistry
    {
        private const string KeyPath = @"Software\Playtime Tracker\Integrity";

        public static long? Read(string purpose)
        {
            try
            {
                using var key = Registry.CurrentUser.OpenSubKey(KeyPath);
                if (key?.GetValue(purpose) is not byte[] blob)
                    return null;
                var plain = ProtectedData.Unprotect(blob, Entropy(purpose), DataProtectionScope.CurrentUser);
                return long.TryParse(Encoding.ASCII.GetString(plain), System.Globalization.NumberStyles.None, null, out var value) ? value : null;
            }
            catch (Exception ex)
            {
                ErrorLog.Write($"The saved generation for {purpose} couldn't be read", ex);
                return null;
            }
        }

        public static void Write(string purpose, long generation)
        {
            try
            {
                using var key = Registry.CurrentUser.CreateSubKey(KeyPath, writable: true);
                var blob = ProtectedData.Protect(Encoding.ASCII.GetBytes(generation.ToString(System.Globalization.CultureInfo.InvariantCulture)),
                    Entropy(purpose), DataProtectionScope.CurrentUser);
                key.SetValue(purpose, blob, RegistryValueKind.Binary);
            }
            catch (Exception ex)
            {
                ErrorLog.Write($"The saved generation for {purpose} couldn't be recorded", ex);
            }
        }

        private static byte[] Entropy(string purpose) => Encoding.UTF8.GetBytes($"PlaytimeTracker/{purpose}/generation/v1");
    }
}

/// <summary>
/// Holds the app's data files open read-only (other programs may read them, but not change, replace or delete them)
/// while the app runs. Released briefly around the app's own saves.
/// </summary>
internal static class FileLocks
{
    private static readonly Dictionary<string, FileStream> Held = new(StringComparer.OrdinalIgnoreCase);

    public static void Hold(string path)
    {
        lock (Held)
        {
            if (Held.ContainsKey(path) || !File.Exists(path))
                return;
            try
            {
                Held[path] = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                // Someone else has it open for writing right now; the next save tries again.
            }
        }
    }

    public static void Release(string path)
    {
        lock (Held)
        {
            if (Held.Remove(path, out var stream))
                stream.Dispose();
        }
    }
}

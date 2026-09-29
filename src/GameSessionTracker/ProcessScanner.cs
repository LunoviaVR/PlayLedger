using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;

namespace GameSessionTracker;

/// <summary>Lists running processes and reports which games are currently running.</summary>
internal sealed class ProcessScanner
{
    private readonly Dictionary<(int Pid, string Name), (string? Path, string? Game)> _cache = new();

    /// <summary>Returns running games as game name → executable path.</summary>
    public Dictionary<string, string> Scan(GameCatalog catalog)
    {
        var running = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var seen = new HashSet<(int, string)>();

        foreach (var process in Process.GetProcesses())
        {
            using (process)
            {
                if (process.Id <= 4) // System Idle / System
                    continue;

                var key = (process.Id, process.ProcessName);
                seen.Add(key);

                if (!_cache.TryGetValue(key, out var entry))
                {
                    var path = GetExecutablePath(process.Id);
                    entry = (path, path is null ? null : catalog.Match(path));
                    _cache[key] = entry;
                }

                if (entry.Game is not null && !running.ContainsKey(entry.Game))
                    running[entry.Game] = entry.Path!;
            }
        }

        foreach (var stale in _cache.Keys.Where(k => !seen.Contains(k)).ToList())
            _cache.Remove(stale);

        return running;
    }

    /// <summary>Forget cached matches, e.g. after the game catalog or settings change.</summary>
    public void Invalidate() => _cache.Clear();

    private static string? GetExecutablePath(int pid)
    {
        // PROCESS_QUERY_LIMITED_INFORMATION works for most processes, including elevated ones,
        // where Process.MainModule would throw "Access is denied".
        var handle = OpenProcess(ProcessQueryLimitedInformation, false, pid);
        if (handle == IntPtr.Zero)
            return null;
        try
        {
            var buffer = new StringBuilder(1024);
            var size = buffer.Capacity;
            return QueryFullProcessImageName(handle, 0, buffer, ref size) ? buffer.ToString(0, size) : null;
        }
        finally
        {
            CloseHandle(handle);
        }
    }

    private const uint ProcessQueryLimitedInformation = 0x1000;

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr OpenProcess(uint desiredAccess, bool inheritHandle, int processId);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode, EntryPoint = "QueryFullProcessImageNameW")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool QueryFullProcessImageName(IntPtr process, uint flags, StringBuilder exeName, ref int size);
}

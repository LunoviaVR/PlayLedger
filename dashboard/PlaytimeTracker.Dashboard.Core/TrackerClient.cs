using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Security.Principal;

namespace PlaytimeTracker.Dashboard.Core;

/// <summary>
/// The dashboard's connection to the background tracker (<c>playtime-tracker.exe</c>). Requests go over one pipe
/// connection; change notifications arrive over a second, subscribed one and are raised as <see cref="EventReceived"/>
/// (on a background thread). If the tracker restarts, the event stream reconnects by itself.
/// </summary>
[SupportedOSPlatform("windows")]
public sealed class TrackerClient : IAsyncDisposable
{
    public const string TrackerExeName = "playtime-tracker.exe";
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromSeconds(3);

    private readonly CancellationTokenSource _stop = new();
    private TrackerConnection? _requests;
    private Task? _eventLoop;

    public event Action<TrackerEvent>? EventReceived;
    /// <summary>Raised when the connection is lost (false) or re-established (true).</summary>
    public event Action<bool>? ConnectionChanged;

    public string? TrackerVersion { get; private set; }

    /// <summary>The tracker next to this dashboard.</summary>
    public static string TrackerPath => Path.Combine(AppContext.BaseDirectory, TrackerExeName);

    private static async Task<TrackerConnection> OpenAsync(CancellationToken cancellationToken)
    {
        var sid = WindowsIdentity.GetCurrent().User?.Value
                  ?? throw new TrackerUnavailableException("Couldn't identify the current Windows user.");
        // CurrentUserOnly: .NET refuses a pipe whose server isn't running as this user.
        var pipe = new NamedPipeClientStream(".", Protocol.PipeName(sid), PipeDirection.InOut,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        try
        {
            await pipe.ConnectAsync(ConnectTimeout, cancellationToken).ConfigureAwait(false);
            VerifyServer(pipe);
            return new TrackerConnection(pipe);
        }
        catch (Exception ex) when (ex is TimeoutException or IOException or UnauthorizedAccessException)
        {
            await pipe.DisposeAsync().ConfigureAwait(false);
            throw new TrackerUnavailableException("Playtime Tracker isn't running.", ex);
        }
        catch
        {
            await pipe.DisposeAsync().ConfigureAwait(false);
            throw;
        }
    }

    /// <summary>Only talk to the real tracker: the pipe's server process must be playtime-tracker.exe, and when one is
    /// installed next to the dashboard, that exact file.</summary>
    private static void VerifyServer(NamedPipeClientStream pipe)
    {
        if (!GetNamedPipeServerProcessId(pipe.SafePipeHandle, out var pid))
            throw new TrackerUnavailableException("Couldn't check who is serving the Playtime Tracker connection.");
        string? path;
        try
        {
            using var process = Process.GetProcessById((int)pid);
            path = process.MainModule?.FileName;
        }
        catch (Exception ex) when (ex is ArgumentException or InvalidOperationException or System.ComponentModel.Win32Exception)
        {
            throw new TrackerUnavailableException("Couldn't check who is serving the Playtime Tracker connection.", ex);
        }
        var expected = File.Exists(TrackerPath) ? TrackerPath : null;
        var ok = path is not null && (expected is not null
            ? string.Equals(Path.GetFullPath(path), Path.GetFullPath(expected), StringComparison.OrdinalIgnoreCase)
            : string.Equals(Path.GetFileName(path), TrackerExeName, StringComparison.OrdinalIgnoreCase));
        if (!ok)
            throw new TrackerUnavailableException("Another program is using Playtime Tracker's connection name.");
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeServerProcessId(Microsoft.Win32.SafeHandles.SafePipeHandle pipe, out uint serverProcessId);

    /// <summary>Connects and checks the protocol version. Throws <see cref="TrackerUnavailableException"/>.</summary>
    public async Task ConnectAsync(CancellationToken cancellationToken = default)
    {
        if (_requests is not null)
            await _requests.DisposeAsync().ConfigureAwait(false);
        _requests = await OpenAsync(cancellationToken).ConfigureAwait(false);
        if (await _requests.SendAsync(Protocol.Hello(), cancellationToken).ConfigureAwait(false) is not HelloResponse hello)
            throw new TrackerUnavailableException("Playtime Tracker didn't answer as expected.");
        if (hello.Protocol != Protocol.Version)
            throw new TrackerUnavailableException(
                $"This dashboard and Playtime Tracker {hello.Version} don't match. Reinstall Playtime Tracker to fix this.");
        TrackerVersion = hello.Version;
        _eventLoop ??= Task.Run(() => EventLoopAsync(_stop.Token));
    }

    /// <summary>Starts the tracker if it isn't running (it's installed next to the dashboard).</summary>
    public static bool StartTracker()
    {
        if (!File.Exists(TrackerPath))
            return false;
        Process.Start(new ProcessStartInfo(TrackerPath, "--startup") { UseShellExecute = false })?.Dispose();
        return true;
    }

    private async Task<T> SendAsync<T>(System.Text.Json.Nodes.JsonObject request, CancellationToken cancellationToken)
        where T : TrackerResponse
    {
        for (var attempt = 0; ; attempt++)
        {
            try
            {
                if (_requests is null)
                    await ConnectAsync(cancellationToken).ConfigureAwait(false);
                var response = await _requests!.SendAsync(request, cancellationToken).ConfigureAwait(false);
                return response as T ?? throw new TrackerUnavailableException("Playtime Tracker didn't answer as expected.");
            }
            catch (TrackerUnavailableException) when (attempt == 0)
            {
                // The tracker may have restarted: reconnect once.
                if (_requests is not null)
                    await _requests.DisposeAsync().ConfigureAwait(false);
                _requests = null;
            }
        }
    }

    public async Task<DashboardSnapshot> GetDashboardAsync(CancellationToken ct = default) =>
        (await SendAsync<DashboardResponse>(Protocol.GetDashboard(), ct).ConfigureAwait(false)).Snapshot;

    public Task<SettingsResponse> GetSettingsAsync(CancellationToken ct = default) =>
        SendAsync<SettingsResponse>(Protocol.GetSettings(), ct);

    public Task UpdateSettingsAsync(TrackerSettings settings, CancellationToken ct = default) =>
        SendAsync<OkResponse>(Protocol.UpdateSettings(settings), ct);

    public Task DeleteSessionAsync(string game, DateTimeOffset start, CancellationToken ct = default) =>
        SendAsync<OkResponse>(Protocol.DeleteSession(game, start), ct);

    public Task DeleteGameHistoryAsync(string game, CancellationToken ct = default) =>
        SendAsync<OkResponse>(Protocol.DeleteGameHistory(game), ct);

    public Task SetGameIgnoredAsync(string game, bool ignored, CancellationToken ct = default) =>
        SendAsync<OkResponse>(Protocol.SetGameIgnored(game, ignored), ct);

    public Task RescanGamesAsync(CancellationToken ct = default) => SendAsync<OkResponse>(Protocol.RescanGames(), ct);

    /// <summary>A local image path, or null (an <see cref="ArtworkReadyEvent"/> follows if it's found later).</summary>
    public async Task<string?> GetArtworkAsync(string game, string kind, CancellationToken ct = default) =>
        (await SendAsync<ArtworkResponse>(Protocol.GetArtwork(game, kind), ct).ConfigureAwait(false)).Path;

    public async Task<string> ExportCsvAsync(CancellationToken ct = default) =>
        (await SendAsync<CsvResponse>(Protocol.ExportCsv(), ct).ConfigureAwait(false)).Text;

    public Task SetSteamGridDbKeyAsync(string? key, CancellationToken ct = default) =>
        SendAsync<OkResponse>(Protocol.SetSteamGridDbKey(key), ct);

    /// <summary>Starts a check (the answer is the status right now; a new version is announced by an event).</summary>
    public Task<UpdateStatusResponse> CheckForUpdatesAsync(CancellationToken ct = default) =>
        SendAsync<UpdateStatusResponse>(Protocol.CheckForUpdates(), ct);

    public Task<UpdateStatusResponse> GetUpdateStatusAsync(CancellationToken ct = default) =>
        SendAsync<UpdateStatusResponse>(Protocol.GetUpdateStatus(), ct);

    public Task<UpdateStatusResponse> InstallUpdateAsync(CancellationToken ct = default) =>
        SendAsync<UpdateStatusResponse>(Protocol.InstallUpdate(), ct);

    public Task SetStartWithWindowsAsync(bool enabled, CancellationToken ct = default) =>
        SendAsync<OkResponse>(Protocol.SetStartWithWindows(enabled), ct);

    private async Task EventLoopAsync(CancellationToken cancellationToken)
    {
        var delay = TimeSpan.FromSeconds(1);
        while (!cancellationToken.IsCancellationRequested)
        {
            try
            {
                await using var connection = await OpenAsync(cancellationToken).ConfigureAwait(false);
                ConnectionChanged?.Invoke(true);
                delay = TimeSpan.FromSeconds(1);
                await foreach (var e in connection.SubscribeAsync(cancellationToken).ConfigureAwait(false))
                    EventReceived?.Invoke(e);
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                return;
            }
            catch (Exception ex) when (ex is TrackerUnavailableException or TrackerErrorException or IOException)
            {
                // Fall through to reconnect.
            }
            ConnectionChanged?.Invoke(false);
            try
            {
                await Task.Delay(delay, cancellationToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                return;
            }
            delay = TimeSpan.FromSeconds(Math.Min(delay.TotalSeconds * 2, 30));
        }
    }

    public async ValueTask DisposeAsync()
    {
        _stop.Cancel();
        if (_eventLoop is not null)
        {
            try
            {
                await _eventLoop.ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
            }
        }
        if (_requests is not null)
            await _requests.DisposeAsync().ConfigureAwait(false);
        _stop.Dispose();
    }
}

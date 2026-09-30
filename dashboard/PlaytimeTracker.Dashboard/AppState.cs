using Microsoft.UI.Dispatching;
using PlaytimeTracker.Dashboard.Core;

namespace PlaytimeTracker.Dashboard;

/// <summary>
/// The dashboard's shared state: the tracker connection, the latest data, and change notifications for the pages.
/// Everything here is raised on the UI thread.
/// </summary>
public sealed class AppState
{
    private readonly TrackerClient _client = new();
    private readonly Dictionary<(string Game, string Kind), string?> _artwork = new();
    private DispatcherQueue? _ui;
    private int _refreshing;

    public DashboardSnapshot? Snapshot { get; private set; }
    public bool Connected { get; private set; }
    public string? ConnectionProblem { get; private set; }

    /// <summary>New data arrived.</summary>
    public event Action? SnapshotChanged;
    /// <summary>Connected or disconnected (see <see cref="ConnectionProblem"/>).</summary>
    public event Action? ConnectionStateChanged;
    /// <summary>Artwork for (game, kind) is now available.</summary>
    public event Action<string, string>? ArtworkChanged;
    /// <summary>Something the user should know about went wrong.</summary>
    public event Action<string>? ErrorReported;

    public TrackerClient Client => _client;

    public void Attach(DispatcherQueue ui)
    {
        _ui = ui;
        _client.EventReceived += e => Post(() => OnEvent(e));
        _client.ConnectionChanged += connected => Post(() =>
        {
            // Only the change stream dropped (it reconnects by itself); whether the tracker is actually gone is decided
            // by a real request, which shows the banner if it fails.
            if (!connected || !Connected)
                _ = RefreshAsync();
        });
    }

    private void Post(Action action)
    {
        if (_ui is null || !_ui.TryEnqueue(() => action()))
            action();
    }

    public void ReportError(string message) => Post(() => ErrorReported?.Invoke(message));

    private void SetConnected(bool connected, string? problem)
    {
        if (Connected == connected && ConnectionProblem == problem)
            return;
        Connected = connected;
        ConnectionProblem = problem;
        ConnectionStateChanged?.Invoke();
    }

    /// <summary>Connects if needed and reloads everything.</summary>
    public async Task RefreshAsync()
    {
        if (Interlocked.Exchange(ref _refreshing, 1) == 1)
            return;
        try
        {
            var snapshot = await _client.GetDashboardAsync();
            Snapshot = snapshot;
            SetConnected(true, null);
            SnapshotChanged?.Invoke();
        }
        catch (TrackerUnavailableException ex)
        {
            SetConnected(false, ex.Message);
        }
        catch (TrackerErrorException ex)
        {
            ErrorReported?.Invoke(ex.Message);
        }
        finally
        {
            Interlocked.Exchange(ref _refreshing, 0);
        }
    }

    private void OnEvent(TrackerEvent e)
    {
        switch (e)
        {
            case ArtworkReadyEvent artwork:
                _artwork.Remove((artwork.Game.ToLowerInvariant(), artwork.Kind));
                ArtworkChanged?.Invoke(artwork.Game, artwork.Kind);
                break;
            case SessionStartedEvent or SessionEndedEvent or DataChangedEvent:
                _ = RefreshAsync();
                break;
        }
    }

    /// <summary>A local artwork file for the game, or null for now (<see cref="ArtworkChanged"/> fires if it arrives).</summary>
    public async Task<string?> ArtworkAsync(string game, string kind)
    {
        var key = (game.ToLowerInvariant(), kind);
        if (_artwork.TryGetValue(key, out var known) && known is not null)
            return known;
        try
        {
            var path = await _client.GetArtworkAsync(game, kind);
            _artwork[key] = path;
            return path;
        }
        catch (Exception ex) when (ex is TrackerUnavailableException or TrackerErrorException)
        {
            return null;
        }
    }

    /// <summary>Runs a tracker command, reporting failures instead of throwing. Returns whether it worked.</summary>
    public async Task<bool> RunAsync(Func<TrackerClient, Task> command)
    {
        try
        {
            await command(_client);
            return true;
        }
        catch (TrackerErrorException ex)
        {
            ErrorReported?.Invoke(ex.Message);
        }
        catch (TrackerUnavailableException ex)
        {
            SetConnected(false, ex.Message);
        }
        return false;
    }
}

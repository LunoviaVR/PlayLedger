namespace GameSessionTracker;

/// <summary>Turns "which games are running right now" snapshots into start/end play sessions.</summary>
internal sealed class SessionTracker
{
    private readonly TrackerData _data;
    private readonly Dictionary<string, ActiveSession> _active = new(StringComparer.OrdinalIgnoreCase);

    public event Action<SessionRecord>? SessionEnded;

    public SessionTracker(TrackerData data, Settings settings)
    {
        _data = data;
        Settings = settings;

        // Sessions still marked active were interrupted (crash, power cut, killed process).
        // Close them at the last moment the game was seen running.
        foreach (var interrupted in data.Active)
            Record(interrupted, interrupted.LastSeen, notify: false);
        data.Active.Clear();
    }

    public Settings Settings { get; set; }

    public IReadOnlyCollection<ActiveSession> Active => _active.Values;

    /// <summary>Applies a snapshot of running games. Returns true if a session started or ended.</summary>
    public bool Update(DateTimeOffset now, IReadOnlyDictionary<string, string> runningGames)
    {
        var changed = false;

        foreach (var (game, exe) in runningGames)
        {
            if (_active.TryGetValue(game, out var session))
            {
                session.LastSeen = now;
            }
            else
            {
                _active[game] = new ActiveSession { Game = game, Start = now, LastSeen = now, Executable = exe };
                changed = true;
            }
        }

        foreach (var session in _active.Values.ToList())
        {
            if (runningGames.ContainsKey(session.Game))
                continue;
            if ((now - session.LastSeen).TotalSeconds >= Settings.GracePeriodSeconds)
            {
                End(session, session.LastSeen);
                changed = true;
            }
        }

        SyncActive();
        return changed;
    }

    /// <summary>
    /// Ends every running session. With no time given, each ends when its game was last seen
    /// (used after the PC slept); otherwise at the given time (used when the tracker exits).
    /// </summary>
    public bool EndAll(DateTimeOffset? at = null)
    {
        if (_active.Count == 0)
            return false;
        foreach (var session in _active.Values.ToList())
            End(session, at ?? session.LastSeen);
        SyncActive();
        return true;
    }

    private void End(ActiveSession session, DateTimeOffset end)
    {
        _active.Remove(session.Game);
        Record(session, end, notify: true);
    }

    private void Record(ActiveSession session, DateTimeOffset end, bool notify)
    {
        if (end < session.Start || (end - session.Start).TotalSeconds < Settings.MinimumSessionSeconds)
            return;

        var record = new SessionRecord { Game = session.Game, Start = session.Start, End = end, Executable = session.Executable };
        _data.Sessions.Add(record);
        if (notify)
            SessionEnded?.Invoke(record);
    }

    private void SyncActive()
    {
        _data.Active.Clear();
        _data.Active.AddRange(_active.Values);
    }
}

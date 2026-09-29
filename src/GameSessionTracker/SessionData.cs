using System.Text.Json;

namespace GameSessionTracker;

/// <summary>One finished play session.</summary>
public sealed class SessionRecord
{
    public string Game { get; set; } = "";
    public DateTimeOffset Start { get; set; }
    public DateTimeOffset End { get; set; }
    public string? Executable { get; set; }

    public TimeSpan Duration => End - Start;
}

/// <summary>A game that is running right now. Saved regularly so a crash or power cut loses at most a minute.</summary>
public sealed class ActiveSession
{
    public string Game { get; set; } = "";
    public DateTimeOffset Start { get; set; }
    public DateTimeOffset LastSeen { get; set; }
    public string? Executable { get; set; }
}

/// <summary>Everything the tracker stores, persisted as sessions.json.</summary>
public sealed class TrackerData
{
    public int Version { get; set; } = 1;
    public List<SessionRecord> Sessions { get; set; } = new();
    public List<ActiveSession> Active { get; set; } = new();

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
    };

    public static TrackerData Load(string path)
    {
        if (!File.Exists(path))
            return new TrackerData();

        try
        {
            var data = JsonSerializer.Deserialize<TrackerData>(File.ReadAllText(path), JsonOptions) ?? new TrackerData();
            data.Sessions ??= new();
            data.Active ??= new();
            return data;
        }
        catch (Exception ex)
        {
            // Never silently throw away someone's history: keep the unreadable file next to the new one.
            var backup = Path.ChangeExtension(path, $".unreadable-{DateTime.Now:yyyyMMdd-HHmmss}.json");
            ErrorLog.Write($"sessions.json could not be read; moved it to {backup} and started fresh.", ex);
            File.Move(path, backup);
            return new TrackerData();
        }
    }

    public void Save(string path) => FileUtil.WriteAllTextAtomic(path, JsonSerializer.Serialize(this, JsonOptions));
}

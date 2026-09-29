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

    private const string Purpose = "sessions";

    /// <summary>
    /// Loads the protected play history (<paramref name="path"/>). The first time, imports the old plain-text
    /// <paramref name="legacyJsonPath"/> and removes it once the protected copy is saved and verified, so there's no
    /// editable copy of the history left behind. <paramref name="warning"/> explains any recovery to the user.
    /// </summary>
    public static TrackerData Load(string path, string legacyJsonPath, out string? warning)
    {
        var data = ProtectedStore.LoadWithRecovery(path, Purpose, Parse, out warning);
        if (data is not null || File.Exists(path))
            return data ?? new TrackerData();
        if (File.Exists(legacyJsonPath))
            return ImportLegacy(path, legacyJsonPath);
        return new TrackerData();
    }

    private static TrackerData Parse(string json)
    {
        var data = JsonSerializer.Deserialize<TrackerData>(json, JsonOptions) ?? new TrackerData();
        data.Sessions ??= new();
        data.Active ??= new();
        return data;
    }

    private static TrackerData ImportLegacy(string path, string legacyJsonPath)
    {
        TrackerData data;
        try
        {
            data = Parse(File.ReadAllText(legacyJsonPath));
        }
        catch (Exception ex)
        {
            // Never silently throw away someone's history: keep the unreadable file next to the new one.
            var backup = Path.ChangeExtension(legacyJsonPath, $".unreadable-{DateTime.Now:yyyyMMdd-HHmmss}.json");
            ErrorLog.Write($"sessions.json could not be read; moved it to {backup} and started fresh.", ex);
            File.Move(legacyJsonPath, backup);
            return new TrackerData();
        }
        data.Save(path);
        // Only remove the plain-text copy once the protected one reads back with the same history.
        if (Parse(ProtectedStore.Read(path, Purpose)).Sessions.Count == data.Sessions.Count)
            File.Delete(legacyJsonPath);
        return data;
    }

    public void Save(string path) => ProtectedStore.Write(path, Purpose, JsonSerializer.Serialize(this, JsonOptions));
}

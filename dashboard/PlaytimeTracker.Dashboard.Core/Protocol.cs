using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace PlaytimeTracker.Dashboard.Core;

// The dashboard side of the tracker's pipe protocol (crates/playtime-core/src/ipc.rs): one JSON object per line,
// tagged by "type", camelCase. dashboard/fixtures/responses.jsonl is written by the Rust tests and read by ours.

public sealed record SessionView(string Game, DateTimeOffset Start, DateTimeOffset End, bool IsLive, string? Executable, long Seconds)
{
    public TimeSpan Duration => TimeSpan.FromSeconds(Seconds);
}

public sealed record SessionRecord(string Game, DateTimeOffset Start, DateTimeOffset End, string? Executable)
{
    public TimeSpan Duration => End - Start;
}

public sealed record GameView(
    string Name,
    int SessionCount,
    long TotalSeconds,
    long AverageSeconds,
    long LongestSeconds,
    DateTimeOffset FirstPlayed,
    DateTimeOffset LastPlayed,
    bool IsLive);

public sealed record DayTotal(DateOnly Day, long Seconds);

public sealed record GameTime(string Game, long Seconds);

public sealed record DayHistory(DateOnly Day, long TotalSeconds, IReadOnlyList<GameTime> Games, int SessionCount);

public sealed record GameIdentity(string Game, string Source, string? ArtworkKey);

public sealed record DashboardSnapshot(
    long Revision,
    DateTimeOffset Now,
    IReadOnlyList<SessionView> Sessions,
    IReadOnlyList<GameView> Games,
    IReadOnlyList<SessionView> Live,
    IReadOnlyList<DayTotal> Daily,
    IReadOnlyList<DayHistory> History,
    long PastWeekSeconds,
    IReadOnlyList<GameIdentity> Identities)
{
    public GameIdentity? IdentityOf(string game) =>
        Identities.FirstOrDefault(i => string.Equals(i.Game, game, StringComparison.OrdinalIgnoreCase));
}

public abstract record TrackerEvent;
public sealed record SessionStartedEvent(string Game) : TrackerEvent;
public sealed record SessionEndedEvent(SessionRecord Session) : TrackerEvent;
public sealed record DataChangedEvent(long Revision) : TrackerEvent;
public sealed record ArtworkReadyEvent(string Game, string Kind) : TrackerEvent;
public sealed record UpdateAvailableEvent(string Version) : TrackerEvent;
public sealed record HeartbeatEvent : TrackerEvent;
/// <summary>An event type this dashboard doesn't know (a newer tracker); ignored.</summary>
public sealed record UnknownEvent(string Type) : TrackerEvent;

public abstract record TrackerResponse;
public sealed record HelloResponse(int Protocol, string Version) : TrackerResponse;
public sealed record DashboardResponse(DashboardSnapshot Snapshot) : TrackerResponse;
public sealed record SettingsResponse(TrackerSettings Settings, bool HasSteamGridDbKey) : TrackerResponse;
public sealed record OkResponse : TrackerResponse;
public sealed record CsvResponse(string Text) : TrackerResponse;
public sealed record ArtworkResponse(string? Path) : TrackerResponse;
public sealed record ErrorResponse(string Message) : TrackerResponse;
public sealed record EventResponse(TrackerEvent Event) : TrackerResponse;

/// <summary>Kinds of artwork the tracker can supply.</summary>
public static class ArtworkKinds
{
    public const string Cover = "cover";
    public const string Header = "header";
    public const string Hero = "hero";
    public const string Logo = "logo";
    public const string Icon = "icon";
}

public static class Protocol
{
    public const int Version = 1;
    public const int MaxMessageBytes = 16 * 1024 * 1024;

    public static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web)
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.Never,
    };

    public static string PipeName(string userSid) => $"PlaytimeTracker.{userSid}";

    private static JsonObject Request(string type) => new() { ["type"] = type };

    public static JsonObject Hello() => new() { ["type"] = "hello", ["protocol"] = Version };
    public static JsonObject GetDashboard() => Request("getDashboard");
    public static JsonObject GetSettings() => Request("getSettings");
    public static JsonObject UpdateSettings(TrackerSettings settings) =>
        new() { ["type"] = "updateSettings", ["settings"] = settings.ToJson() };
    public static JsonObject DeleteSession(string game, DateTimeOffset start) =>
        new() { ["type"] = "deleteSession", ["game"] = game, ["start"] = start.ToString("o") };
    public static JsonObject DeleteGameHistory(string game) => new() { ["type"] = "deleteGameHistory", ["game"] = game };
    public static JsonObject SetGameIgnored(string game, bool ignored) =>
        new() { ["type"] = "setGameIgnored", ["game"] = game, ["ignored"] = ignored };
    public static JsonObject RescanGames() => Request("rescanGames");
    public static JsonObject GetArtwork(string game, string kind) => new() { ["type"] = "getArtwork", ["game"] = game, ["kind"] = kind };
    public static JsonObject ExportCsv() => Request("exportCsv");
    public static JsonObject SetSteamGridDbKey(string? key) => new() { ["type"] = "setSteamGridDbKey", ["key"] = key };
    public static JsonObject CheckForUpdates() => Request("checkForUpdates");
    public static JsonObject Subscribe() => Request("subscribe");

    /// <summary>Parses one response line. Throws <see cref="JsonException"/> on malformed input.</summary>
    public static TrackerResponse ParseResponse(string line)
    {
        var node = JsonNode.Parse(line)?.AsObject() ?? throw new JsonException("empty message");
        var type = node["type"]?.GetValue<string>() ?? throw new JsonException("message without a type");
        return type switch
        {
            "hello" => new HelloResponse(node["protocol"]!.GetValue<int>(), node["version"]!.GetValue<string>()),
            "dashboard" => new DashboardResponse(Deserialize<DashboardSnapshot>(node["snapshot"])),
            "settings" => new SettingsResponse(
                new TrackerSettings(node["settings"]?.AsObject() ?? throw new JsonException("settings missing")),
                node["hasSteamGridDbKey"]?.GetValue<bool>() ?? false),
            "ok" => new OkResponse(),
            "csv" => new CsvResponse(node["text"]!.GetValue<string>()),
            "artwork" => new ArtworkResponse(node["path"]?.GetValue<string>()),
            "error" => new ErrorResponse(node["message"]?.GetValue<string>() ?? "Unknown error"),
            "event" => new EventResponse(ParseEvent(node["event"]?.AsObject() ?? throw new JsonException("event missing"))),
            _ => throw new JsonException($"unknown response type '{type}'"),
        };
    }

    private static TrackerEvent ParseEvent(JsonObject node)
    {
        var type = node["type"]?.GetValue<string>() ?? "";
        return type switch
        {
            "sessionStarted" => new SessionStartedEvent(node["game"]!.GetValue<string>()),
            "sessionEnded" => new SessionEndedEvent(Deserialize<SessionRecord>(node["session"])),
            "dataChanged" => new DataChangedEvent(node["revision"]!.GetValue<long>()),
            "artworkReady" => new ArtworkReadyEvent(node["game"]!.GetValue<string>(), node["kind"]!.GetValue<string>()),
            "updateAvailable" => new UpdateAvailableEvent(node["version"]!.GetValue<string>()),
            "heartbeat" => new HeartbeatEvent(),
            _ => new UnknownEvent(type),
        };
    }

    private static T Deserialize<T>(JsonNode? node) =>
        node.Deserialize<T>(Json) ?? throw new JsonException($"{typeof(T).Name} missing");
}

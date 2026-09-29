using System.Text.Json.Nodes;

namespace PlaytimeTracker.Dashboard.Core;

/// <summary>A game identified by its executable, from Settings → Custom games.</summary>
public sealed record CustomGame(string Name, string Executable);

/// <summary>
/// The tracker's settings as the dashboard edits them. Wraps the JSON the tracker sent, so fields this dashboard
/// doesn't know about (from a newer tracker) are sent back unchanged.
/// </summary>
public sealed class TrackerSettings
{
    private readonly JsonObject _json;

    public TrackerSettings(JsonObject json) => _json = json;

    public TrackerSettings Clone() => new((JsonObject)_json.DeepClone());

    public JsonObject ToJson() => (JsonObject)_json.DeepClone();

    private T Get<T>(string name, T fallback)
    {
        try
        {
            return _json[name] is { } node ? node.GetValue<T>() : fallback;
        }
        catch (InvalidOperationException)
        {
            return fallback;
        }
        catch (FormatException)
        {
            return fallback;
        }
    }

    private List<string> GetList(string name) =>
        _json[name] is JsonArray array
            ? array.Select(n => n?.GetValue<string>()).Where(s => !string.IsNullOrWhiteSpace(s)).Select(s => s!).ToList()
            : new List<string>();

    private void SetList(string name, IEnumerable<string> values) =>
        _json[name] = new JsonArray(values.Select(v => (JsonNode?)JsonValue.Create(v)).ToArray());

    public int PollIntervalSeconds { get => Get("pollIntervalSeconds", 5); set => _json["pollIntervalSeconds"] = Math.Clamp(value, 1, 300); }
    public int MinimumSessionSeconds { get => Get("minimumSessionSeconds", 0); set => _json["minimumSessionSeconds"] = Math.Max(0, value); }
    public int GracePeriodSeconds { get => Get("gracePeriodSeconds", 20); set => _json["gracePeriodSeconds"] = Math.Max(0, value); }
    public bool ShowNotifications { get => Get("showNotifications", true); set => _json["showNotifications"] = value; }
    public bool UseWindowsGameList { get => Get("useWindowsGameList", true); set => _json["useWindowsGameList"] = value; }
    public bool CheckForUpdates { get => Get("checkForUpdates", true); set => _json["checkForUpdates"] = value; }
    public bool InstallUpdatesAutomatically { get => Get("installUpdatesAutomatically", true); set => _json["installUpdatesAutomatically"] = value; }
    public bool OnlineArtwork { get => Get("onlineArtwork", false); set => _json["onlineArtwork"] = value; }

    /// <summary>"system", "dark" or "light".</summary>
    public string ThemeMode
    {
        get => Get("themeMode", "system");
        set => _json["themeMode"] = value is "dark" or "light" ? value : "system";
    }

    /// <summary>A preset name ("blue", "violet", …) or a colour like "#ff8800".</summary>
    public string AccentColor { get => Get("accentColor", "blue"); set => _json["accentColor"] = value.Trim().ToLowerInvariant(); }

    public List<string> ExtraGameFolders { get => GetList("extraGameFolders"); set => SetList("extraGameFolders", value); }
    public List<string> IgnoredGames { get => GetList("ignoredGames"); set => SetList("ignoredGames", value); }
    public List<string> IgnoredExecutables { get => GetList("ignoredExecutables"); set => SetList("ignoredExecutables", value); }

    public List<CustomGame> CustomGames
    {
        get => _json["customGames"] is JsonArray array
            ? array.OfType<JsonObject>()
                .Select(o => new CustomGame(o["name"]?.GetValue<string>() ?? "", o["executable"]?.GetValue<string>() ?? ""))
                .Where(g => g.Name.Length > 0 && g.Executable.Length > 0)
                .ToList()
            : new List<CustomGame>();
        set => _json["customGames"] = new JsonArray(value
            .Select(g => (JsonNode?)new JsonObject { ["name"] = g.Name.Trim(), ["executable"] = g.Executable.Trim() })
            .ToArray());
    }
}

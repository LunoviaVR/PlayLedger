using System.Text.Json;
using System.Text.Json.Serialization;

namespace GameSessionTracker;

/// <summary>A game the tracker can't find on its own, identified by its executable.</summary>
public sealed class CustomGame
{
    /// <summary>Name shown in the stats file, e.g. "Minecraft".</summary>
    public string Name { get; set; } = "";

    /// <summary>Executable file name ("javaw.exe") or a full path ("D:\Games\Foo\Foo.exe").</summary>
    public string Executable { get; set; } = "";
}

/// <summary>User-editable settings, stored as settings.json in the data folder.</summary>
public sealed class Settings
{
    /// <summary>How often running programs are checked.</summary>
    public int PollIntervalSeconds { get; set; } = 5;

    /// <summary>Sessions shorter than this are not recorded (e.g. a game that opened and immediately closed to update).</summary>
    public int MinimumSessionSeconds { get; set; } = 30;

    /// <summary>A game that closes and reopens within this many seconds continues the same session.</summary>
    public int GracePeriodSeconds { get; set; } = 20;

    /// <summary>Show a Windows notification when a session is logged.</summary>
    public bool ShowNotifications { get; set; } = true;

    /// <summary>Also treat programs that Windows (Xbox Game Bar) recognises as games as games.</summary>
    public bool UseWindowsGameList { get; set; } = true;

    /// <summary>Extra folders where every sub-folder is a game, e.g. "D:\\Games".</summary>
    public List<string> ExtraGameFolders { get; set; } = new();

    /// <summary>Games identified by executable name, for games outside any known game folder.</summary>
    public List<CustomGame> CustomGames { get; set; } = new();

    /// <summary>Games (by name or install-folder name) that should never be tracked.</summary>
    public List<string> IgnoredGames { get; set; } = new()
    {
        "Steamworks Common Redistributables",
        "Steamworks Shared",
        "Steam Controller Configs",
        "SteamVR",
        "Wallpaper Engine",
        "Riot Client",
        "Launcher",
        "Epic Online Services",
        "_CommonRedist",
        "Redist",
        "DirectX",
    };

    /// <summary>Executables that should never count as playing (launchers, crash reporters, anti-cheat, installers).</summary>
    public List<string> IgnoredExecutables { get; set; } = new()
    {
        "UnityCrashHandler64.exe",
        "UnityCrashHandler32.exe",
        "CrashReportClient.exe",
        "CrashReporter.exe",
        "UnrealCEFSubProcess.exe",
        "EpicWebHelper.exe",
        "EOSOverlayRenderer-Win64-Shipping.exe",
        "EOSOverlayRenderer-Win32-Shipping.exe",
        "EasyAntiCheat.exe",
        "EasyAntiCheat_EOS.exe",
        "EasyAntiCheat_Setup.exe",
        "EasyAntiCheat_EOS_Setup.exe",
        "BEService.exe",
        "BEService_x64.exe",
        "steamerrorreporter.exe",
        "steamerrorreporter64.exe",
        "steamwebhelper.exe",
        "CefSharp.BrowserSubProcess.exe",
        "QtWebEngineProcess.exe",
        "vc_redist.x64.exe",
        "vc_redist.x86.exe",
        "DXSETUP.exe",
        "unins000.exe",
        "EpicGamesLauncher.exe",
        "RiotClientServices.exe",
        "RiotClientUx.exe",
        "RiotClientUxRender.exe",
        "RiotClientCrashHandler.exe",
        "GalaxyClient.exe",
        "upc.exe",
        "UbisoftConnect.exe",
        "UplayWebCore.exe",
        "EADesktop.exe",
        "EABackgroundService.exe",
        "Battle.net.exe",
        "UnrealEditor.exe",
        "UE4Editor.exe",
        "Unity.exe",
        "Unity Hub.exe",
    };

    /// <summary>Set once the app has turned on "Start with Windows" for the first time. Not meant to be edited.</summary>
    public bool StartupConfigured { get; set; }

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        ReadCommentHandling = JsonCommentHandling.Skip,
        AllowTrailingCommas = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.Never,
    };

    /// <summary>Loads settings, creating the file with defaults if it doesn't exist. Throws if the file is malformed.</summary>
    public static Settings Load(string path)
    {
        if (!File.Exists(path))
        {
            var defaults = new Settings();
            defaults.Save(path);
            return defaults;
        }

        var settings = JsonSerializer.Deserialize<Settings>(File.ReadAllText(path), JsonOptions)
                       ?? throw new JsonException("settings.json is empty.");
        settings.Normalize();
        return settings;
    }

    public void Save(string path) => FileUtil.WriteAllTextAtomic(path, JsonSerializer.Serialize(this, JsonOptions));

    /// <summary>Clamps values into sensible ranges and replaces missing lists.</summary>
    internal void Normalize()
    {
        PollIntervalSeconds = Math.Clamp(PollIntervalSeconds, 1, 300);
        MinimumSessionSeconds = Math.Max(0, MinimumSessionSeconds);
        GracePeriodSeconds = Math.Max(0, GracePeriodSeconds);
        ExtraGameFolders ??= new();
        CustomGames ??= new();
        IgnoredGames ??= new();
        IgnoredExecutables ??= new();
    }
}

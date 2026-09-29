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

    /// <summary>Sessions shorter than this are not recorded. Off (0) by default: every time a game is open counts.</summary>
    public int MinimumSessionSeconds { get; set; } = 0;

    /// <summary>A game that closes and reopens within this many seconds continues the same session.</summary>
    public int GracePeriodSeconds { get; set; } = 20;

    /// <summary>Show a Windows notification when a session is logged.</summary>
    public bool ShowNotifications { get; set; } = true;

    /// <summary>Also treat programs that Windows (Xbox Game Bar) recognises as games as games.</summary>
    public bool UseWindowsGameList { get; set; } = true;

    /// <summary>"system" (follow the Windows light/dark setting), "dark" or "light".</summary>
    public string ThemeMode { get; set; } = "system";

    /// <summary>Accent colour: a preset name ("blue", "violet", "teal", "green", "amber", "rose") or a colour like "#ff8800".</summary>
    public string AccentColor { get; set; } = "blue";

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

    /// <summary>Check GitHub for new versions of the app.</summary>
    public bool CheckForUpdates { get; set; } = true;

    /// <summary>Install new versions by itself (only while no game is running). Otherwise just offer them.</summary>
    public bool InstallUpdatesAutomatically { get; set; } = true;

    /// <summary>The version that last ran, to say "Updated to x.y.z" once after an update. Not meant to be edited.</summary>
    public string LastRunVersion { get; set; } = "";

    /// <summary>Format of this file, for one-time migrations. Not meant to be edited.</summary>
    public int SettingsVersion { get; set; }

    private const int CurrentSettingsVersion = 2;

    /// <summary>Set once the app has turned on "Start with Windows" for the first time. Not meant to be edited.</summary>
    public bool StartupConfigured { get; set; }

    /// <summary>Fetch missing game artwork online. Used by the upcoming Rust version; kept here so saving settings
    /// from this version doesn't lose it. Off by default.</summary>
    public bool OnlineArtwork { get; set; }

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        ReadCommentHandling = JsonCommentHandling.Skip,
        AllowTrailingCommas = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.Never,
    };

    private const string Purpose = "settings";

    /// <summary>
    /// Loads the protected settings file, creating it with defaults if it doesn't exist. The first time, imports the old
    /// plain-text <paramref name="legacyJsonPath"/> (then removes it). If the file fails verification it is set aside
    /// and the last saved copy (or the defaults) is used; <paramref name="warning"/> says so.
    /// </summary>
    public static Settings Load(string path, string legacyJsonPath, out string? warning)
    {
        var settings = ProtectedStore.LoadWithRecovery(path, Purpose, Parse, out warning);
        if (settings is not null)
            return settings;

        if (!File.Exists(path) && File.Exists(legacyJsonPath))
        {
            try
            {
                settings = Parse(File.ReadAllText(legacyJsonPath));
                settings.Save(path);
                Parse(ProtectedStore.Read(path, Purpose)); // verify before removing the editable copy
                File.Delete(legacyJsonPath);
                return settings;
            }
            catch (Exception ex) when (ex is JsonException or IOException or UnverifiedDataException)
            {
                ErrorLog.Write("Could not import settings.json; using defaults.", ex);
                warning = "Your old settings.json couldn't be read, so default settings are in use.";
            }
        }

        settings = new Settings();
        settings.Save(path);
        return settings;
    }

    /// <summary>Reads settings from a protected file that changed on disk. Throws if it doesn't verify.</summary>
    public static Settings LoadVerified(string path) => Parse(ProtectedStore.Read(path, Purpose));

    private static Settings Parse(string json)
    {
        var settings = JsonSerializer.Deserialize<Settings>(json, JsonOptions) ?? throw new JsonException("The settings file is empty.");
        settings.Normalize();
        return settings;
    }

    public void Save(string path) => ProtectedStore.Write(path, Purpose, JsonSerializer.Serialize(this, JsonOptions));

    /// <summary>Clamps values into sensible ranges and replaces missing lists.</summary>
    internal void Normalize()
    {
        PollIntervalSeconds = Math.Clamp(PollIntervalSeconds, 1, 300);
        if (SettingsVersion < 2 && MinimumSessionSeconds == 30)
            MinimumSessionSeconds = 0; // the old default silently dropped real short plays; keep any value the user chose
        SettingsVersion = Math.Max(SettingsVersion, CurrentSettingsVersion);
        MinimumSessionSeconds = Math.Max(0, MinimumSessionSeconds);
        LastRunVersion ??= "";
        GracePeriodSeconds = Math.Max(0, GracePeriodSeconds);
        ThemeMode = ThemeMode?.Trim().ToLowerInvariant() is "dark" or "light" ? ThemeMode.Trim().ToLowerInvariant() : "system";
        AccentColor = string.IsNullOrWhiteSpace(AccentColor) ? "blue" : AccentColor.Trim().ToLowerInvariant();
        ExtraGameFolders ??= new();
        CustomGames ??= new();
        IgnoredGames ??= new();
        IgnoredExecutables ??= new();
    }
}

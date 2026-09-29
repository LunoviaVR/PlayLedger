using System.Diagnostics;
using System.Text.Json;
using System.Text.RegularExpressions;
using Microsoft.Win32;

namespace GameSessionTracker;

/// <summary>
/// Knows where games are installed on this PC and decides whether a running executable is a game.
/// Sources: Steam, Epic, GOG, Ubisoft, EA, Xbox/Game Pass, Riot, Windows' own game list,
/// plus the user's extra folders and custom games from settings.json.
/// </summary>
internal sealed partial class GameCatalog
{
    private readonly List<(string Dir, string Name)> _locations = new();          // a specific game's install folder
    private readonly List<string> _roots = new();                                   // folders where each sub-folder is a game
    private readonly Dictionary<string, string> _exactExes = new(StringComparer.OrdinalIgnoreCase);
    // Windows' game list by (exe file name, folder two levels up), for games that update into a new versioned folder.
    private readonly Dictionary<(string File, string Parent), string> _versionedExes = new();
    private readonly List<CustomGame> _customGames;
    private readonly HashSet<string> _ignoredExes;
    private readonly HashSet<string> _ignoredGames;

    public int LocationCount => _locations.Count;
    public int RootCount => _roots.Count;

    private GameCatalog(Settings settings)
    {
        _customGames = settings.CustomGames
            .Where(g => !string.IsNullOrWhiteSpace(g.Name) && !string.IsNullOrWhiteSpace(g.Executable))
            .ToList();
        _ignoredExes = new HashSet<string>(settings.IgnoredExecutables.Select(NormalizeExeName), StringComparer.OrdinalIgnoreCase);
        _ignoredGames = new HashSet<string>(settings.IgnoredGames.Select(n => n.Trim()), StringComparer.OrdinalIgnoreCase);
    }

    public static GameCatalog Build(Settings settings)
    {
        var catalog = new GameCatalog(settings);

        catalog.Try("Steam", catalog.AddSteam);
        catalog.Try("Epic Games", catalog.AddEpic);
        catalog.Try("GOG", catalog.AddGog);
        catalog.Try("Ubisoft", catalog.AddUbisoft);
        catalog.Try("well-known game folders", catalog.AddWellKnownRoots);
        if (settings.UseWindowsGameList)
            catalog.Try("Windows game list", catalog.AddWindowsGameList);

        foreach (var folder in settings.ExtraGameFolders)
            catalog.AddRoot(Environment.ExpandEnvironmentVariables(folder));

        // Longest path first so nested install folders win over their parents.
        catalog._locations.Sort((a, b) => b.Dir.Length.CompareTo(a.Dir.Length));
        return catalog;
    }

    /// <summary>Returns the game name for a running executable, or null if it isn't a game.</summary>
    public string? Match(string exePath)
    {
        var fileName = Path.GetFileName(exePath);

        foreach (var custom in _customGames)
        {
            var target = custom.Executable.Trim();
            var isFullPath = target.Contains('\\') || target.Contains('/');
            if (isFullPath ? PathsEqual(exePath, target) : string.Equals(fileName, NormalizeExeName(target), StringComparison.OrdinalIgnoreCase))
                return custom.Name.Trim();
        }

        if (_ignoredExes.Contains(fileName))
            return null;

        foreach (var (dir, name) in _locations)
        {
            if (TryGetRelative(exePath, dir, out var relative))
                return IsIgnored(name, relative) ? null : name;
        }

        foreach (var root in _roots)
        {
            if (!TryGetRelative(exePath, root, out var relative))
                continue;
            var parts = relative.Split('\\', StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length < 2)
                return null; // an exe sitting directly in the root folder isn't inside a game folder
            return IsIgnored(parts[0], relative) ? null : parts[0];
        }

        if (MatchKnownGame(exePath) is { } known)
            return IsIgnored(known, "") ? null : known;

        if (_exactExes.TryGetValue(NormalizePath(exePath), out var exactName))
            return IsIgnored(exactName, "") ? null : exactName;

        // A game Windows recognised in an older version folder (e.g. ...\Versions\version-abc\Game.exe after an update).
        if (VersionedKey(exePath) is { } key && _versionedExes.TryGetValue(key, out var versionedName))
            return IsIgnored(versionedName, "") ? null : versionedName;

        return null;
    }

    /// <summary>
    /// Popular games that install outside any store launcher, so none of the sources below would find them. Matched by
    /// the game's own executable name, which only the game uses. Their launchers, crash handlers and editors are
    /// deliberately left out (e.g. Roblox Studio isn't "playing Roblox").
    /// </summary>
    private static readonly Dictionary<string, string> KnownGameExes = new(StringComparer.OrdinalIgnoreCase)
    {
        ["RobloxPlayerBeta.exe"] = "Roblox",           // %LocalAppData%\Roblox\Versions\version-*\
        ["Minecraft.Windows.exe"] = "Minecraft",       // Minecraft for Windows (Bedrock), Microsoft Store
        ["GenshinImpact.exe"] = "Genshin Impact",
        ["StarRail.exe"] = "Honkai: Star Rail",
        ["ZenlessZoneZero.exe"] = "Zenless Zone Zero",
        ["osu!.exe"] = "osu!",
        ["League of Legends.exe"] = "League of Legends",
        ["VALORANT-Win64-Shipping.exe"] = "VALORANT",
        ["FortniteClient-Win64-Shipping.exe"] = "Fortnite",
    };

    private static string? MatchKnownGame(string exePath)
    {
        var fileName = exePath[(exePath.LastIndexOfAny(new[] { '\\', '/' }) + 1)..];
        if (KnownGameExes.TryGetValue(fileName, out var name))
            return name;

        // Roblox from the Microsoft Store runs as a generic "Windows10Universal.exe" inside its package folder.
        if (fileName.Equals("Windows10Universal.exe", StringComparison.OrdinalIgnoreCase) &&
            exePath.Contains("ROBLOXCORPORATION.ROBLOX", StringComparison.OrdinalIgnoreCase))
            return "Roblox";

        // Minecraft: Java Edition runs as javaw.exe from the Minecraft Launcher's own Java runtime.
        if (fileName.Equals("javaw.exe", StringComparison.OrdinalIgnoreCase) &&
            (exePath.Contains(@"\Minecraft Launcher\runtime\", StringComparison.OrdinalIgnoreCase) ||
             exePath.Contains(@"\Microsoft.4297127D64EC6_", StringComparison.OrdinalIgnoreCase) ||
             exePath.Contains(@"\.minecraft\runtime\", StringComparison.OrdinalIgnoreCase)))
            return "Minecraft";

        return null;
    }

    /// <summary>(file name, grandparent folder) of an exe: stays the same when a game moves into a new version folder.</summary>
    private static (string File, string Parent)? VersionedKey(string exePath)
    {
        var dir = Path.GetDirectoryName(NormalizePath(exePath));
        var grandparent = dir is null ? null : Path.GetDirectoryName(dir);
        if (string.IsNullOrEmpty(grandparent) || grandparent.Length <= 3)
            return null; // too close to a drive root to say anything
        return (Path.GetFileName(exePath).ToUpperInvariant(), grandparent.ToUpperInvariant());
    }

    private bool IsIgnored(string gameName, string relativePath)
    {
        if (_ignoredGames.Contains(gameName))
            return true;
        // Catches redistributable installers etc. that live in a sub-folder of a game.
        foreach (var part in relativePath.Split('\\', StringSplitOptions.RemoveEmptyEntries))
        {
            if (_ignoredGames.Contains(part))
                return true;
        }
        return false;
    }

    // ---------- Sources ----------

    private void AddSteam()
    {
        var steamPaths = new List<string>();
        if (Registry.GetValue(@"HKEY_CURRENT_USER\Software\Valve\Steam", "SteamPath", null) is string userPath)
            steamPaths.Add(userPath);
        if (Registry.GetValue(@"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath", null) is string machinePath)
            steamPaths.Add(machinePath);

        var libraries = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var steamPath in steamPaths)
        {
            var steam = NormalizePath(steamPath);
            libraries.Add(steam);
            var vdf = Path.Combine(steam, "steamapps", "libraryfolders.vdf");
            if (!File.Exists(vdf))
                continue;
            foreach (Match m in VdfPathRegex().Matches(File.ReadAllText(vdf)))
                libraries.Add(NormalizePath(UnescapeVdf(m.Groups[1].Value)));
        }

        foreach (var library in libraries)
        {
            var steamapps = Path.Combine(library, "steamapps");
            var common = Path.Combine(steamapps, "common");
            if (!Directory.Exists(common))
                continue;

            AddRoot(common);
            foreach (var manifest in Directory.EnumerateFiles(steamapps, "appmanifest_*.acf"))
            {
                try
                {
                    var text = File.ReadAllText(manifest);
                    var name = VdfNameRegex().Match(text);
                    var installDir = VdfInstallDirRegex().Match(text);
                    if (name.Success && installDir.Success)
                        AddLocation(Path.Combine(common, UnescapeVdf(installDir.Groups[1].Value)), UnescapeVdf(name.Groups[1].Value));
                }
                catch (Exception ex)
                {
                    ErrorLog.Write($"Could not read Steam manifest {manifest}", ex);
                }
            }
        }
    }

    private void AddEpic()
    {
        var manifests = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
            "Epic", "EpicGamesLauncher", "Data", "Manifests");
        if (!Directory.Exists(manifests))
            return;

        foreach (var file in Directory.EnumerateFiles(manifests, "*.item"))
        {
            try
            {
                using var doc = JsonDocument.Parse(File.ReadAllText(file));
                var root = doc.RootElement;
                var name = root.TryGetProperty("DisplayName", out var n) ? n.GetString() : null;
                var location = root.TryGetProperty("InstallLocation", out var l) ? l.GetString() : null;
                if (string.IsNullOrWhiteSpace(name) || string.IsNullOrWhiteSpace(location))
                    continue;

                // Skip Unreal Engine installs and plugins; only real games/apps count.
                if (root.TryGetProperty("AppCategories", out var categories) && categories.ValueKind == JsonValueKind.Array)
                {
                    var cats = categories.EnumerateArray().Select(c => c.GetString() ?? "").ToList();
                    if (cats.Any(c => c.Equals("engines", StringComparison.OrdinalIgnoreCase) || c.Equals("plugins", StringComparison.OrdinalIgnoreCase)))
                        continue;
                }

                AddLocation(location, name);
            }
            catch (Exception ex)
            {
                ErrorLog.Write($"Could not read Epic manifest {file}", ex);
            }
        }
    }

    private void AddGog()
    {
        using var hklm = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry32);
        using var games = hklm.OpenSubKey(@"SOFTWARE\GOG.com\Games");
        if (games is null)
            return;
        foreach (var id in games.GetSubKeyNames())
        {
            using var game = games.OpenSubKey(id);
            if (game?.GetValue("path") is string path && game.GetValue("gameName") is string name)
                AddLocation(path, name);
        }
    }

    private void AddUbisoft()
    {
        using var hklm = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry32);
        using var installs = hklm.OpenSubKey(@"SOFTWARE\Ubisoft\Launcher\Installs");
        if (installs is null)
            return;
        foreach (var id in installs.GetSubKeyNames())
        {
            using var game = installs.OpenSubKey(id);
            if (game?.GetValue("InstallDir") is string dir && !string.IsNullOrWhiteSpace(dir))
                AddLocation(dir, Path.GetFileName(NormalizePath(dir)));
        }
    }

    private void AddWellKnownRoots()
    {
        var programFiles = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
        var programFilesX86 = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86);
        AddRoot(Path.Combine(programFiles, "EA Games"));
        AddRoot(Path.Combine(programFilesX86, "Origin Games"));
        AddRoot(Path.Combine(programFiles, "ModifiableWindowsApps"));

        foreach (var drive in DriveInfo.GetDrives())
        {
            try
            {
                if (drive.DriveType != DriveType.Fixed || !drive.IsReady)
                    continue;
                AddRoot(Path.Combine(drive.RootDirectory.FullName, "XboxGames"));   // Xbox app / Game Pass
                AddRoot(Path.Combine(drive.RootDirectory.FullName, "Riot Games"));
                AddRoot(Path.Combine(drive.RootDirectory.FullName, "Games"));
            }
            catch
            {
                // Drive disappeared or isn't accessible; skip it.
            }
        }
    }

    private void AddWindowsGameList()
    {
        // Xbox Game Bar keeps a list of executables it has identified as games.
        using var children = Registry.CurrentUser.OpenSubKey(@"System\GameConfigStore\Children");
        if (children is null)
            return;
        foreach (var id in children.GetSubKeyNames())
        {
            using var child = children.OpenSubKey(id);
            if (child?.GetValue("MatchedExeFullPath") is not string exe || string.IsNullOrWhiteSpace(exe))
                continue;
            var path = NormalizePath(exe);
            if (_exactExes.ContainsKey(path) || _ignoredExes.Contains(Path.GetFileName(path)))
                continue;
            if (File.Exists(path))
            {
                _exactExes[path] = FriendlyNameFromExe(path);
            }
            else if (IsVersionFolder(Path.GetFileName(Path.GetDirectoryName(path) ?? "")) && VersionedKey(path) is { } key)
            {
                // The game has since updated into a new version folder; keep matching it there.
                _versionedExes.TryAdd(key, Path.GetFileNameWithoutExtension(path));
            }
        }
    }

    // ---------- Helpers ----------

    /// <summary>Folder names like "version-3f2c9a…", "app-1.2.3" or "1.2.3" that launchers replace on every update.</summary>
    private static bool IsVersionFolder(string folder) =>
        VersionFolderRegex().IsMatch(folder);

    private void Try(string source, Action action)
    {
        try
        {
            action();
        }
        catch (Exception ex)
        {
            ErrorLog.Write($"Could not read games from {source}", ex);
        }
    }

    private void AddLocation(string dir, string name)
    {
        if (string.IsNullOrWhiteSpace(dir) || string.IsNullOrWhiteSpace(name))
            return;
        var normalized = NormalizePath(dir);
        if (_locations.Any(l => PathsEqual(l.Dir, normalized)))
            return;
        _locations.Add((normalized, name.Trim()));
    }

    private void AddRoot(string dir)
    {
        if (string.IsNullOrWhiteSpace(dir))
            return;
        var normalized = NormalizePath(dir);
        if (!Directory.Exists(normalized) || _roots.Any(r => PathsEqual(r, normalized)))
            return;
        _roots.Add(normalized);
    }

    private static string FriendlyNameFromExe(string exe)
    {
        try
        {
            var info = FileVersionInfo.GetVersionInfo(exe);
            foreach (var candidate in new[] { info.ProductName, info.FileDescription })
            {
                if (string.IsNullOrWhiteSpace(candidate))
                    continue;
                var trimmed = candidate.Trim();
                // Engine-generic names say nothing about which game it is.
                if (trimmed is "Unity" or "UnrealGame" or "Unreal Engine" or "BootstrapPackagedGame")
                    continue;
                return trimmed;
            }
        }
        catch
        {
            // Fall back to the file name.
        }
        return Path.GetFileNameWithoutExtension(exe);
    }

    private static bool TryGetRelative(string path, string dir, out string relative)
    {
        relative = "";
        if (path.Length <= dir.Length + 1 || !path.StartsWith(dir, StringComparison.OrdinalIgnoreCase) || path[dir.Length] != '\\')
            return false;
        relative = path[(dir.Length + 1)..];
        return true;
    }

    internal static string NormalizePath(string path)
    {
        var p = path.Trim().Trim('"').Replace('/', '\\');
        try
        {
            p = Path.GetFullPath(p);
        }
        catch
        {
            // Keep the raw value if it isn't a valid path.
        }
        return p.Length > 3 ? p.TrimEnd('\\') : p;
    }

    private static bool PathsEqual(string a, string b) =>
        string.Equals(NormalizePath(a), NormalizePath(b), StringComparison.OrdinalIgnoreCase);

    private static string NormalizeExeName(string name)
    {
        var trimmed = name.Trim();
        return trimmed.EndsWith(".exe", StringComparison.OrdinalIgnoreCase) ? trimmed : trimmed + ".exe";
    }

    private static string UnescapeVdf(string value) => value.Replace(@"\\", @"\").Replace("\\\"", "\"");

    [GeneratedRegex(@"^(version-[0-9a-f]{6,}|app-\d+(\.\d+)+|v?\d+(\.\d+){1,3})$", RegexOptions.IgnoreCase)]
    private static partial Regex VersionFolderRegex();

    [GeneratedRegex("\"path\"\\s+\"((?:[^\"\\\\]|\\\\.)*)\"", RegexOptions.IgnoreCase)]
    private static partial Regex VdfPathRegex();

    [GeneratedRegex("\"name\"\\s+\"((?:[^\"\\\\]|\\\\.)*)\"", RegexOptions.IgnoreCase)]
    private static partial Regex VdfNameRegex();

    [GeneratedRegex("\"installdir\"\\s+\"((?:[^\"\\\\]|\\\\.)*)\"", RegexOptions.IgnoreCase)]
    private static partial Regex VdfInstallDirRegex();
}

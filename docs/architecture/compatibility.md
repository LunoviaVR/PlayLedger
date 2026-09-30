# What stays the same across versions

The names, files and formats every version of PlayLedger shares, so an upgrade (or a downgrade to an earlier
version) keeps working with the same data and the same Windows entries. Changing any of these needs a migration
(see [`overview.md`](overview.md#migrations)).

The app was renamed from Playtime Tracker to PlayLedger in 3.1.0. Only what people see changed (window and tray
text, notifications, exe version info, the installer, the Start menu shortcut and the Apps entry's display name).
Every name below keeps its Playtime Tracker form, including the DPAPI entropy, which a test pins.

## Processes and lifecycle

- Single instance: mutex `Local\GameSessionTracker.SingleInstance`. A second launch signals
  `Local\GameSessionTracker.Show` (show the dashboard); `--exit` signals `Local\GameSessionTracker.Exit` and waits
  up to 15 s for the running copy to save and quit (used by the installer). These names predate the rename and
  are kept so old and new copies detect each other.
- Arguments: `--startup` and `--updated` start quietly in the tray; otherwise the dashboard opens.
- Start with Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `PlaytimeTracker` =
  `"<exe>" --startup`; Task Manager's enabled/disabled state (`...\Explorer\StartupApproved\Run`) is respected.
  The old value name `GameSessionTracker` is migrated once.

## Tracking

- Every `pollIntervalSeconds` (default 5) the tracker lists processes, gets each exe path with
  `QueryFullProcessImageName` (works for elevated processes) and asks the catalog whether it's a game. Matches
  are cached per (pid, process name).
- A session starts when a game first appears and ends when it has been absent for `gracePeriodSeconds`
  (default 20); the end time is the last time it was seen. Sessions shorter than `minimumSessionSeconds`
  (default 0 = off, since 2.1.0) are dropped.
- Sleep: a gap between polls longer than max(90 s, 6 × poll interval), or a suspend event, ends every session
  at its last-seen time, so sleep never counts. Shutdown/exit ends sessions at the exit time.
- Crash recovery: running sessions are saved every 60 s in `active`; on the next start they are recorded as
  ending at their last-seen time.
- The game catalog is rebuilt every 10 minutes and when detection settings change.

## Game detection (in priority order)

1. Custom games from Settings (exe file name, or full path).
2. Ignored executables → not a game.
3. Specific install folders, longest first: Steam (`appmanifest_*.acf` name + installdir), Epic
   (`%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item`, skipping engines/plugins), GOG
   (`HKLM\SOFTWARE\GOG.com\Games`, 32-bit view), Ubisoft (`HKLM\SOFTWARE\Ubisoft\Launcher\Installs`).
4. Roots where every sub-folder is a game: Steam `steamapps\common` of every library
   (`libraryfolders.vdf`), `EA Games`, `Origin Games`, `ModifiableWindowsApps`, and `X:\XboxGames`,
   `X:\Riot Games`, `X:\Games` on every fixed drive, plus the user's extra folders.
5. Built-in games that install outside launchers: Roblox (incl. Store package), Minecraft (Bedrock, and Java via
   the launcher's `javaw.exe`), Genshin Impact, Honkai: Star Rail, Zenless Zone Zero, osu!, League of Legends,
   VALORANT, Fortnite.
6. Windows' Xbox Game Bar list (`HKCU\System\GameConfigStore\Children\*\MatchedExeFullPath`), by exact path.
7. The same list after a game moved into a new version folder (`version-<hex>`, `app-x.y.z`, `x.y.z`): matched
   by (exe name, folder two levels up).

Ignored games (by name, or any path component under the game folder) are never tracked. History is keyed by the
game's display name.

## Data (`Documents\Playtime Tracker\`)

| File | Contents |
| --- | --- |
| `sessions.dat` (+ `.bak`) | Protected JSON: `{"version":1,"sessions":[{"game","start","end","executable"}],"active":[{"game","start","lastSeen","executable"}]}` |
| `settings.dat` (+ `.bak`) | Protected JSON of every setting (camelCase, all fields written; unknown fields are ignored and missing ones take their defaults, so versions read each other's files) |
| `Game Stats.txt`, `Sessions.csv` | Readable reports, regenerated on every save, read-only, UTF-8 with a byte-order mark |
| `errors.log` | Capped at 1 MB, rotated to `.old` |
| `migration.log`, `Backups\` | What the migrations did, and the copies taken before a version change |
| `*.unverified-<stamp>`, `*.older-<stamp>` | Files that failed verification or were rolled back; kept, never read |

Timestamps are ISO 8601 with a UTC offset and up to 7 fractional digits (100 ns precision).

### Protected file format

- `PTDATA1\n` + DPAPI(CurrentUser) blob of the JSON (2.0.x), or
- `PTDATA2\n` + DPAPI blob of `<generation>\n<json>` (2.2.0 and later).
- DPAPI optional entropy: `PlaytimeTracker/<purpose>/v1`, purpose = `sessions` | `settings`.
- The last saved generation is stored in `HKCU\Software\Playtime Tracker\Integrity`, value `<purpose>`,
  REG_BINARY = DPAPI(ASCII decimal) with entropy `PlaytimeTracker/<purpose>/generation/v1`.
- Saves: generation = max(known, recorded) + 1; temp file, then replace keeping `.bak`.
- Loads: the newest verified copy of main/`.bak` wins; unverifiable files are renamed aside; a copy older than the
  recorded generation is reported. While running, the files are held open with read-only sharing.
- First start after 1.x: `sessions.json` / `settings.json` are imported, verified, then deleted; the old
  `Documents\Game Session Tracker` folder is moved.

## Updates

GitHub API `repos/LunoviaVR/PlaytimeTracker/releases/latest`, 1 min after start then every 6 h (if enabled). Only
non-draft, non-prerelease plain `vX.Y.Z` tags newer than the running version. The release's single asset,
`Setup.exe`, must come from this repository's release download URL over HTTPS (redirects only to `github.com` /
`*.githubusercontent.com`) and match GitHub's recorded size and SHA-256 digest. It runs only for installed copies
(the Apps entry's `InstallLocation` is the exe's folder), silently with `/S /relaunch`; the installer then starts
the tracker with `--updated`. Automatic installs wait until no game is running.

## Installer

Per-user install to `%LocalAppData%\Programs\Playtime Tracker`, Start menu shortcut, optional desktop shortcut,
Run entry, Apps entry (`HKCU\...\Uninstall\PlaytimeTracker`); closes a running copy with `--exit`; removes a
legacy Game Session Tracker install; supports `/S` (silent) and `/relaunch`.

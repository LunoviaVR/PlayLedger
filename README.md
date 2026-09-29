# Game Session Tracker

A small Windows 11 app that runs in the system tray, notices when you open a game, and keeps a file with
how many times you've played each game and how long every session lasted.

- **Runs in the background** in the system tray (the controller icon). Right-click it for the menu, or left-click to open your stats.
- **Starts with Windows** automatically. You can turn this off with **Start with Windows** in the tray menu.
- **Close it any time** with **Exit** in the tray menu. A game that's running when you exit is logged up to that moment.
- **Finds games on its own** from Steam, Epic Games, GOG, Ubisoft Connect, EA app/Origin, Xbox app/Game Pass (`XboxGames`),
  Riot Games, any `X:\Games` folder, and every program Windows' Xbox Game Bar recognises as a game.
  You can add anything else yourself (see [Settings](#settings)).

## Install

1. Get `GameSessionTracker.exe`, either from the **Build** workflow's artifacts / a release on GitHub, or by building it yourself (below).
2. Move it somewhere permanent, e.g. `C:\Users\<you>\AppData\Local\Programs\GameSessionTracker\` (it registers its current location to run at startup).
3. Double-click it. A notification confirms it's running.

Windows 11 hides new tray icons behind the **^** arrow next to the clock. To keep it visible: *Settings → Personalization → Taskbar → Other system tray icons* → turn on **Game Session Tracker**.

The first time you run it, Windows SmartScreen may warn about an unrecognised app because the exe isn't code-signed.
Click **More info → Run anyway**.

## Your files

Everything lives in `Documents\Game Session Tracker\` (tray menu → **Open data folder**):

| File | What it is |
| --- | --- |
| `Game Stats.txt` | The readable report: times played, total and average time per game, and every session with start/end time and length. |
| `Sessions.csv` | Every session, one per row. Opens in Excel / Google Sheets. |
| `sessions.json` | The tracker's own data. Don't edit it while the app is running. |
| `settings.json` | Your settings (see below). |
| `errors.log` | Only written if something goes wrong. |

Example `Game Stats.txt`:

```
SUMMARY  (3 games, 4 sessions, 4h 37m total)
  Game                   Times played    Total time     Average  Last played
  ---------------------  ------------  ------------  ----------  ----------------------
  Beat Saber                        2        2h 01m      1h 00m  Tue Sep 29, 2026  12:01 AM
  Elden Ring                        1        1h 35m      1h 35m  Sun Sep 27, 2026  9:35 PM

SESSIONS BY GAME  (newest first)

Beat Saber
  Played 2 times, 2h 01m total
  #2    Mon Sep 28, 2026  11:00 PM  ->  Tue Sep 29, 2026  12:01 AM   1h 01m
  #1    Mon Sep 28, 2026  8:00 PM  ->  9:00 PM   1h 00m
```

## How sessions are counted

- The app checks running programs every 5 seconds. A session starts when a game's process appears and ends when it's gone.
- If a game closes and reopens within 20 seconds (e.g. a launcher handing off to the game) it stays one session.
- Sessions under 30 seconds aren't recorded (updaters, crashes on launch).
- Time the PC spends asleep isn't counted. A game still open when the PC wakes starts a new session.
- If the PC crashes or loses power mid-game, the session is recovered on next start, ending at the last time the game was seen (at most about a minute lost).

## Settings

Tray menu → **Edit settings...** opens `settings.json` in Notepad. Save the file and changes apply within a few seconds.

| Setting | Default | Meaning |
| --- | --- | --- |
| `pollIntervalSeconds` | `5` | How often to check for running games. |
| `minimumSessionSeconds` | `30` | Shorter sessions are ignored. |
| `gracePeriodSeconds` | `20` | Close and reopen within this time = same session. |
| `showNotifications` | `true` | Show a notification when a session is logged. |
| `useWindowsGameList` | `true` | Also count programs Xbox Game Bar has flagged as games. |
| `extraGameFolders` | `[]` | Folders where every sub-folder is a game. |
| `customGames` | `[]` | Games identified by exe name or full path. |
| `ignoredGames` | launchers, redistributables, SteamVR, Wallpaper Engine... | Game or folder names never to track. |
| `ignoredExecutables` | launchers, crash reporters, anti-cheat, Unity/Unreal editors... | Exe names never to track. |

Example: track Minecraft and everything in `D:\MyGames`, and stop ignoring SteamVR:

```json
{
  "extraGameFolders": [ "D:\\MyGames" ],
  "customGames": [
    { "name": "Minecraft", "executable": "javaw.exe" },
    { "name": "Some Indie Game", "executable": "C:\\Stuff\\Indie\\game.exe" }
  ],
  "ignoredGames": [ "Steamworks Common Redistributables", "Wallpaper Engine", "Launcher" ]
}
```

(Remember to double the backslashes `\\` in paths.) If a game isn't being picked up, add it to `customGames`.
If something is counted that shouldn't be, add its exe to `ignoredExecutables` or its name to `ignoredGames`.

## Uninstall

1. Tray menu → uncheck **Start with Windows**, then **Exit**.
2. Delete `GameSessionTracker.exe`. Your history stays in `Documents\Game Session Tracker\` until you delete it.

## Build it yourself

Install the [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0), then in PowerShell from this folder:

```powershell
./build.ps1
```

The exe is written to `publish\GameSessionTracker.exe`. It's self-contained, so it runs without installing .NET.

# Migrating Playtime Tracker to Rust

Status of the incremental rewrite. The C# app in `src/` stays the shipping product until the Rust version reaches
feature parity (phase 12); every phase keeps both building.

## Architecture decisions

**1. A Rust background service plus a thin native dashboard (hybrid).**
Rust has no supported way to host WinUI 3 / XAML (the `windows` crate doesn't project the Windows App SDK UI
framework), and the target is an app that could pass for a Microsoft one: real Mica and Acrylic, WinUI controls,
UI Automation (screen readers), virtualised lists and Windows motion. So:

- `playtime-tracker.exe` (**Rust**) is the always-running part: tray icon, process polling, session tracking,
  storage and data protection, launcher discovery, artwork cache, update checks. It has no UI framework, so idle
  CPU and memory stay minimal and tracking never depends on the dashboard.
- The dashboard (**WinUI 3**, the thinnest possible presentation layer) only runs while it's open. It holds no
  business logic: it asks the tracker for view models and sends commands.
- They talk over a local named pipe (`\\.\pipe\PlaytimeTracker.<user SID>`, ACL'd to the current user) with
  small JSON messages, event-driven (the tracker pushes "session started/ended", "artwork ready").

**2. Workspace of small crates** (`crates/`), core logic separate from platform code:

| Crate | Contents | Status |
| --- | --- | --- |
| `playtime-core` | Domain model + JSON compatible with C#, `Timestamp` (.NET format), session tracker, the tracker engine (polling rules, sleep, saves), game catalog matching and discovery, launcher metadata parsers (Steam KeyValues, Epic manifests), settings (+ migration), protected-file format and recovery rules, reports (`Game Stats.txt`, CSV), dashboard view models, the IPC protocol, update-release parsing | **Done** (phases 2–7) |
| `playtime-windows` | DPAPI `DataProtector`, HKCU generation store, read-only file locks, data folders, the data store, process list, discovery host, WinHTTP client, Credential Manager, exe icon provider | **Done** (phases 3–7) |
| `playtime-tracker` | The background service binary: single instance, tray icon and menu, poll timer, power/shutdown handling, notifications, named-pipe server | **Done** (phase 7), not installed yet |
| `playtime-artwork` | `ArtworkProvider` trait, image validation, on-disk cache with remembered misses, Steam local library art, Steam store CDN and SteamGridDB (online, opt-in), PNG encoder | **Done** (phase 6) |
| `dashboard/` (C#, WinUI 3) | Presentation only: `PlaytimeTracker.Dashboard` (the app) and `PlaytimeTracker.Dashboard.Core` (pipe client, protocol, formatting; plain .NET, unit-tested) | **Built** (phase 8), not installed yet |

The core never calls Windows APIs, so it builds and is tested on any OS; Windows specifics sit behind traits
(`DataProtector`, `GenerationStore`, later process enumeration and discovery).

**3. Storage stays the protected JSON files for now.** They already give atomic saves, backups, tamper detection
and rollback detection, and the C# and Rust versions must share them during the migration. SQLite
(`games`, `sessions`, `artwork`, …) is planned once the dashboard needs indexed queries over very large histories;
it will be imported from `sessions.dat` with the same verify-then-switch rules, and must keep the same guarantees
(tamper and rollback detection, protection at rest); how is decided in that phase.

**4. Stable game identities.** Matches now carry a `GameId` (Steam App ID, Epic namespace + catalog item, package
family name, GOG product ID, normalised path, or built-in key). History stays keyed by display name for
compatibility; the ID keys artwork and future per-game settings.

## Artwork

Covers, headers, heroes, logos and icons come from, in order:

1. **Steam's local library cache** (`<Steam>\appcache\librarycache`), both the old flat and the current per-app layouts.
2. **The game's own exe icon** (Windows), saved as PNG.
3. Only if **Settings → Online artwork** is on (off by default):
   - the **Steam store CDN**, for Steam games; the request is just the app ID in the URL;
   - **SteamGridDB**, only if the user entered their own API key. It gets the Steam app ID, or for other games the
     game's name as a search term. The key is stored in Windows Credential Manager (never in the repo, the data
     files or logs), is sent only to the API host, and never to the image CDN.

Online requests go through WinHTTP with Windows' certificate validation, TLS 1.2+ only, no automatic redirects, and an
allow-list of hosts that's checked again for every redirect and for every image URL an API returns. Everything that
comes back is sniffed as PNG/JPEG/WebP from its bytes, dimension- and size-checked, then stored in
`%LocalAppData%\Playtime Tracker\Cache\Artwork\<game key>\`. A lookup that found nothing is remembered (a day for local
sources, a week online) so nothing is re-requested on every start; a lookup that failed because the PC is offline isn't.
Play history is never sent anywhere.

## The tracker service (phase 7)

`playtime-tracker.exe` does what the C# `TrayApp` does, with the same rules and files:

- **Single instance** with the C# app's kernel object names (`Local\GameSessionTracker.SingleInstance`, `.Show`,
  `.Exit`), so the C# and Rust trackers never run at the same time, a second launch shows the dashboard, and the
  installer's `--exit` works for both. `--startup` / `--updated` start quietly.
- **Tracking** (`playtime_core::engine`): polls every `pollIntervalSeconds`, caches matches per (pid, image name),
  ends sessions after the grace period at their last-seen time, treats a gap longer than max(90 s, 6 × poll) or a
  suspend message as sleep, saves running sessions every 60 s for crash recovery, rebuilds the catalog every
  10 minutes, and ends everything at exit or Windows shutdown.
- **Storage** (`playtime_windows::store`): the protected `sessions.dat` / `settings.dat` with recovery, read-only
  `Game Stats.txt` / `Sessions.csv` (byte-compatible layout, tested), file locks, and `errors.log`. If the data
  folder can't be opened, tracking continues in memory and nothing is written, so an unreadable history is never
  replaced.
- **Tray**: icon, tooltip with what's playing, menu (status, Open dashboard, Settings, Exit), "Session logged"
  notifications, and it comes back after Explorer restarts.
- **Dashboard API**: `\\.\pipe\PlaytimeTracker.<user SID>`. The DACL allows only the current user, remote clients
  are rejected, the first instance is created exclusively (no name squatting), and at most 8 connections are served.
  Messages are one JSON object per line (≤ 16 MB), defined in `playtime_core::ipc`:
  `hello`, `getDashboard` (sessions, games, 30-day chart and history, past week, game sources and artwork keys),
  `getSettings` / `updateSettings`, `deleteSession`, `deleteGameHistory`, `setGameIgnored`, `rescanGames`,
  `getArtwork` (a cached file path, else fetched in the background followed by an `artworkReady` event),
  `exportCsv`, `setSteamGridDbKey` (write-only), and `subscribe`, which turns the connection into an event
  stream (`sessionStarted`, `sessionEnded`, `dataChanged`, `artworkReady`, `heartbeat`).

**Start with Windows and updates** (phase 10) behave as in the C# app, and only for the *installed* copy (the Apps
entry's `InstallLocation` is the exe's folder), so a copy run from anywhere else never takes over the Run key or
updates itself:

- the `PlaytimeTracker` Run value (`"<exe>" --startup`), on by default the first time, respecting Task Manager's
  on/off switch, moving the pre-rename `GameSessionTracker` entry over, and re-pointed at this exe if it moved;
- updates from GitHub releases: checked a minute after start and every 6 hours (if enabled), announced once, and
  installed automatically only when no game is running (or from the dashboard / tray menu). `Setup.exe` must come
  from this repository's release URL over HTTPS, with every redirect limited to GitHub's hosts, and match GitHub's
  recorded size and SHA-256 (streamed to disk, hashed with `sha2`); it runs silently with `/S /relaunch`;
- after an update, "Playtime Tracker updated: you're now on version X (was Y)".

The dashboard's Settings page gained Startup, Updates (check now, install, automatic) and the accent colour
(Windows accent, the six presets, or any colour; applied to WinUI's accent shades).

## The dashboard (phase 8)

`dashboard/PlaytimeTracker.Dashboard` is an unpackaged, self-contained WinUI 3 app (Windows App SDK 1.8, .NET 8):
Mica backdrop, a custom title bar, and a Task Manager-style left navigation with **Overview** (totals, playing
now, 30-day chart, recent sessions), **Games** (cover art or icon tiles, search and sort, details with Stop
tracking and Delete history), **History** (the last 30 days, each expandable into its sessions), **Statistics**
(days played, longest session, most played, by weekday and time of day) and **Settings** (theme, notifications,
detection, tracking timings, online artwork and the SteamGridDB key, custom games, game folders, ignored games and
programs, rescan, CSV export, open the report). Every control has an accessible name; deletions ask for
confirmation. Everything the C# dashboard does is kept: choosing a game on the Overview shows only that game across
the page (tiles with its longest session, chart, sessions); a chart bar opens that day's sessions; the Overview lists
every session (fifty at a time); session details show the session number (#3 of 12), the game's and the day's totals,
*Show program*, and count up live for a running game.

It holds no data of its own. Everything comes from the tracker over the pipe through
`PlaytimeTracker.Dashboard.Core`, which checks that the pipe's server runs as the current user
(`PipeOptions.CurrentUserOnly`) and is `playtime-tracker.exe` (the copy next to the dashboard when installed),
reconnects its event stream if the tracker restarts, and keeps settings fields it doesn't know about.
`dashboard/fixtures/responses.jsonl` is written by the Rust tests and parsed by the C# tests, so both sides
must agree on every message. One dashboard window at a time: a second launch brings the first forward.

## Migrations (phase 9)

`playtime_core::migration` runs before the tracker opens the data folder. Every step is *verify, then switch*:
nothing old is removed until its replacement has been written and read back, anything unreadable is kept under a
new name, and every step is appended to `migration.log` in the data folder.

- `Documents\Game Session Tracker` (1.x) is moved to `Documents\Playtime Tracker`; if it can't be moved yet (a file
  is open, OneDrive is syncing), the old folder is used as is and the move is retried next start.
- 1.x plain-text `sessions.json` / `settings.json` are imported into the protected files, read back, compared, and
  only then deleted. An unreadable one is kept as `sessions.unreadable-<date>.json`. Existing protected data always
  wins; a stray JSON next to it is left alone, never merged over newer data.
- Before the first run of the Rust tracker and before every version change, `sessions.dat` / `settings.dat` (and
  their `.bak`) are copied to `Backups\<date> before <version>\` and compared byte for byte. The newest ten
  automatic backups are kept; other folders in `Backups` are never touched.
- The running version is recorded in the settings (`lastRunVersion`, the same field the C# app uses).

## Installer and the switch-over (phases 10–11)

`build-next.ps1` builds `playtime-tracker.exe` (release) and the self-contained dashboard, then
`installer/PlaytimeTracker-Next.nsi` packs them into `PlaytimeTrackerSetup-Preview.exe`. The Build workflow's
`preview` job produces it on every push as the `PlaytimeTracker-Preview` artifact. It installs per user to the same
place with the same Apps entry, Start menu shortcut and Run entry as the C# app, so it upgrades an existing install in
place: it closes whichever tracker is running (both answer `--exit`), removes `PlaytimeTracker.exe`, and puts the
dashboard in its own `Dashboard\` folder (the only folder it ever removes recursively). The data folder isn't
touched by setup; the tracker's migrations back it up on first run. Uninstalling removes the artwork cache and,
if "Delete my play history" is ticked, the saved SteamGridDB key too.

**Releases still ship the C# app.** Switching them over (having the release job attach the preview installer as
`Setup.exe`) is a one-line change, but it updates every existing user automatically, so it waits until the preview
has been installed over a real 2.x install and checked: tracking, the tray, the dashboard pages, settings, updates
and uninstall. [`testing-the-preview.md`](../testing-the-preview.md) is the checklist for that.

## Phases

| # | Phase | Status |
| --- | --- | --- |
| 1 | Document current behaviour | Done: [`current-behavior.md`](current-behavior.md) |
| 2 | Rust workspace, domain models | Done |
| 3 | Storage and session model | Done: format, recovery, tracker (tested against C#-written JSON) |
| 4 | Game detection | Done: matching rules |
| 5 | Launcher integrations (discovery on disk/registry) | Done: `playtime_core::discovery` (Steam, Epic, GOG, Ubisoft, EA/Origin, Xbox, Riot, `X:\Games`, extra folders, Windows game list) over a `DiscoveryHost` trait; `playtime_windows::discovery::WindowsHost` |
| 6 | Artwork service | Done: see *Artwork* below |
| 7 | Tray/background tracker in Rust | Done: see *The tracker service* above; installed by the preview installer and smoke-tested in CI |
| 8 | WinUI 3 dashboard | Done: see *The dashboard* above; installed by the preview installer and smoke-tested in CI |
| 9 | Migrations (verify-then-switch, logs, backups) | Done: see *Migrations* above |
| 10 | Replace the C# entry point | Done in code: start with Windows, updates, `--exit`/`--startup`/`--updated`, accent colour; the installed layout below. Switching releases over waits for real-PC testing: [`testing-the-preview.md`](../testing-the-preview.md) |
| 11 | Installer and CI for the Rust build | Done: `installer/PlaytimeTracker-Next.nsi`, `build-next.ps1`, the Build workflow's `preview` job (artifact `PlaytimeTracker-Preview`), plus the Rust and Dashboard workflows |
| 12 | Remove C# after parity is verified | Planned |
| 13 | Game context menu: change artwork, stop tracking, delete history | Planned, not started: see *Game context menu* below |
| 14 | Tip or donate section in Settings | Planned, not started: see *Tip or donate* below |

## Game context menu (phase 13, planned)

Not started. On the **Games** page, right-clicking a game tile opens a menu. The same menu opens with the keyboard
(the Menu key or Shift+F10) and with press-and-hold on touch screens:

- **Change artwork ▸** a cascading submenu:
  - **Choose an image file…** picks a PNG, JPEG or WebP from the PC. It goes through the same checks as downloaded
    artwork (format sniffed from the bytes, size and dimension limits), is copied into the artwork cache, and is
    never uploaded.
  - **Pick from SteamGridDB…** shows the images SteamGridDB has for the game, to choose one. It's only there when
    *Online artwork* is on and the user has entered their own API key, and sends only what artwork lookups already
    send: the Steam app ID, or the game's name.
  - **Use automatic artwork** removes the override and goes back to the sources under *Artwork* above.
- **Stop tracking**, as in the game's details now: it moves the game to *Settings → Ignored games* and keeps its
  history.
- **Delete history…**, as in the game's details now, after the same confirmation.

The tracker would keep each game's chosen artwork locally, as an override the artwork service checks before its
automatic sources. The dashboard would reach it through new pipe requests (setting and resetting a game's
artwork, and listing SteamGridDB candidates), added to `dashboard/fixtures/responses.jsonl` so both sides agree.
It would come with Rust unit tests for the override and the file checks, and a smoke-test step that sets a
game's artwork from a file and resets it.

## Tip or donate (phase 14, planned)

Not started. A small **Support Playtime Tracker** section in Settings, directly below *About*: one line of text and
a button with the Cash App logo, labelled **Tip or Donate**. It opens `https://cash.app/$LunoviaVR` in the default
browser.

- The logo is Cash App's trademark, so it would be the official asset from Cash App's brand resources, used as their
  guidelines allow (unaltered, with clear space), not a redrawn copy. It is bundled with the dashboard, not loaded
  from the web. The owner of this project has agreed to use it under Cash App's terms.
- Nothing is sent anywhere until the user clicks. The link is a fixed address in the code, opened only after a
  check that it's that exact `https://cash.app/` address, and the app neither sees nor handles any payment.
- The button has an accessible name ("Tip or donate with Cash App, opens in your browser") and works with the
  keyboard like every other control.

## Checks

`cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and `cargo audit` run on
`windows-latest` for every change to the Rust code. Locally, `cargo check --target x86_64-pc-windows-msvc` also
works from Linux/macOS (the toolchain file installs the target).

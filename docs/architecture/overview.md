# How Playtime Tracker is built

Playtime Tracker is two Rust programs and a few shared libraries. [`compatibility.md`](compatibility.md) lists the
files, names and formats that stay the same across versions.

## Two programs

- **`playtime-tracker.exe`** is the always-running part: tray icon, process polling, session tracking, storage and
  data protection, launcher discovery, artwork cache and update checks. It has no UI framework, so idle CPU and memory
  stay minimal and tracking never depends on the dashboard.
- **The dashboard** (`Dashboard\PlaytimeTracker.Dashboard.exe`) only runs while its window is open. It holds no data
  and no business logic: it asks the tracker for view models and sends commands.
- They talk over a local named pipe (`\\.\pipe\PlaytimeTracker.<user SID>`, allowed only to the current user) with
  small JSON messages, event-driven (the tracker pushes "session started/ended", "artwork ready").

## Crates (`crates/`)

| Crate | Contents |
| --- | --- |
| `playtime-core` | Domain model and its JSON, `Timestamp`, session tracker, the tracker engine (polling rules, sleep, saves), game catalog matching and discovery, launcher metadata parsers (Steam KeyValues, Epic manifests), settings (+ migration), protected-file format and recovery rules, reports (`Game Stats.txt`, CSV), dashboard view models, the IPC protocol, update-release parsing, data-folder migrations |
| `playtime-windows` | DPAPI `DataProtector`, HKCU generation store, read-only file locks, data folders, the data store, process list, discovery host, WinHTTP client, Credential Manager, exe icon provider, the named-pipe client checks, the Open/Save As dialogs |
| `playtime-artwork` | `ArtworkProvider` trait, image validation, on-disk cache with remembered misses, the user's own choices, Steam local library art, Steam store CDN and SteamGridDB (online, opt-in), PNG encoder |
| `playtime-tracker` | The background service: single instance, tray icon and menu, poll timer, power/shutdown handling, notifications, the named-pipe server, start with Windows, updates |
| `playtime-dashboard` | The dashboard window (Slint, Fluent style) |
| `playtime-smoke-test` | The end-to-end test CI runs against an installed copy |

`playtime-core` never calls Windows APIs, so it builds and is tested on any OS; Windows specifics sit behind traits
(`DataProtector`, `GenerationStore`, `DiscoveryHost`, …) implemented in `playtime-windows`.

**Storage** is the protected JSON files (atomic saves, backups, tamper and rollback detection). **Game identities**:
matches carry a `GameId` (Steam App ID, Epic namespace + catalog item, package family name, GOG product ID,
normalised path, or built-in key); history stays keyed by display name, and the ID keys artwork.

## The tracker

- **Single instance** (`Local\GameSessionTracker.SingleInstance`, `.Show`, `.Exit`): a second launch shows the
  dashboard, and `--exit` asks the running copy to save and quit (the installer uses it). `--startup` / `--updated`
  start quietly.
- **Tracking** (`playtime_core::engine`): polls every `pollIntervalSeconds`, caches matches per (pid, image name),
  ends sessions after the grace period at their last-seen time, treats a gap longer than max(90 s, 6 × poll) or a
  suspend message as sleep, saves running sessions every 60 s for crash recovery, rebuilds the catalog every
  10 minutes, and ends everything at exit or Windows shutdown.
- **Storage** (`playtime_windows::store`): the protected `sessions.dat` / `settings.dat` with recovery, read-only
  `Game Stats.txt` / `Sessions.csv`, file locks, and `errors.log`. If the data folder can't be opened, tracking
  continues in memory and nothing is written, so an unreadable history is never replaced.
- **Tray**: icon, tooltip with what's playing, menu (status, Open dashboard, Settings, Exit), "Session logged"
  notifications, and it comes back after Explorer restarts.
- **Dashboard API**: the pipe's DACL allows only the current user (who also owns it), remote clients are rejected,
  the first instance is created exclusively (no name squatting), and at most 8 connections are served. Messages are
  one JSON object per line (≤ 16 MB), defined in `playtime_core::ipc`: `hello`, `getDashboard`, `getSettings` /
  `updateSettings`, `deleteSession`, `deleteGameHistory`, `setGameIgnored`, `rescanGames`, `getArtwork` (a cached
  file, else fetched in the background followed by an `artworkReady` event), `setArtworkFromFile`,
  `listArtworkChoices` / `applyArtworkChoice`, `resetArtwork`, `exportCsv`, `setSteamGridDbKey` (write-only),
  `checkForUpdates` / `getUpdateStatus` / `installUpdate`, `setStartWithWindows`, and `subscribe`, which turns the
  connection into an event stream (`sessionStarted`, `sessionEnded`, `dataChanged`, `artworkReady`,
  `updateAvailable`, `heartbeat`). `crates/playtime-dashboard/fixtures/responses.jsonl` holds sample messages
  written by a test, so a protocol change shows up as a failing test.
- **Start with Windows and updates** apply only to the *installed* copy (the Apps entry's `InstallLocation` is the
  exe's folder), so a copy run from anywhere else never takes over the Run key or updates itself:
  - the `PlaytimeTracker` Run value (`"<exe>" --startup`), on by default the first time, respecting Task Manager's
    on/off switch, moving the pre-rename `GameSessionTracker` entry over, and re-pointed at this exe if it moved;
  - updates from GitHub releases: checked a minute after start and every 6 hours (if enabled), announced once, and
    installed automatically only when no game is running (or from the dashboard / tray menu). `Setup.exe` must come
    from this repository's release URL over HTTPS, with every redirect limited to GitHub's hosts, and match GitHub's
    recorded size and SHA-256 (streamed to disk, hashed with `sha2`); it runs silently with `/S /relaunch`;
  - after an update, "Playtime Tracker updated: you're now on version X (was Y)".

## Artwork

Covers, headers, heroes, logos and icons come from, in order:

1. **The user's own choice** for that game (*Change artwork* in a game's menu): a picture file from the PC, or one
   picked from SteamGridDB. It's copied into `%LocalAppData%\Playtime Tracker\Cache\Artwork Choices`, checked like any
   other artwork, never uploaded, and *Use automatic artwork* removes it.
2. **Steam's local library cache** (`<Steam>\appcache\librarycache`), both the old flat and the current per-app layouts.
3. **The game's own exe icon**, saved as PNG.
4. Only if **Settings → Download missing artwork** is on (off by default):
   - the **Steam store CDN**, for Steam games; the request is just the app ID in the URL;
   - **SteamGridDB**, only if the user entered their own API key. It gets the Steam app ID, or for other games the
     game's name as a search term. The key is stored in Windows Credential Manager (never in the repository, the
     data files or logs), is sent only to the API host, and never to the image CDN. *Pick from SteamGridDB…* offers
     up to 12 pictures; only the one chosen is downloaded in full.

Online requests go through WinHTTP with Windows' certificate validation, TLS 1.2+ only, no automatic redirects, and an
allow-list of hosts that's checked again for every redirect and for every image URL an API returns. Everything that
comes back is sniffed as PNG/JPEG/WebP from its bytes, dimension- and size-checked (at most 8192 pixels a side and
16 MB), then stored in `%LocalAppData%\Playtime Tracker\Cache\Artwork\<game key>\`. A lookup that found nothing is
remembered (a day for local sources, a week online) so nothing is re-requested on every start; a lookup that failed
because the PC is offline isn't. Play history is never sent anywhere.

**Recommended size for your own artwork:** 600 × 900 pixels, portrait (2:3), the size of Steam's library covers and
SteamGridDB's standard grids. Game tiles are 160 × 240, so this stays sharp at up to 375% scaling; 320 × 480 is the
smallest that stays sharp at 200%. PNG transparency is kept (the file is checked, not re-compressed). Other shapes are
scaled to fill the tile and cropped at the edges. Tiles have rounded corners (about 30 pixels at 600 × 900), and while
the game runs a *Playing* badge covers roughly the top-left 250 × 100 pixels.

## The dashboard

Written with [Slint](https://slint.dev) in its Fluent style (used under the GPL-3.0, credited in *About*), with
screen-reader support through AccessKit. It follows light and dark mode, the accent colour setting (its own switches,
buttons, selection and charts use it), and the Windows 11 layout of Task Manager: a navigation pane that folds to
icons, cards, and a minimum window size of 500 × 500.

- **Pages**: Overview (tiles that reflow four / two / one per row, playing now, the 30-day chart, games, sessions
  fifty at a time; choosing a game shows only that game across the page), Games (tiles with box art or icons,
  search, sort, the game menu), History (the last 30 days, each expandable into its sessions; in a narrow window the
  games summary moves under the day and never wraps letter by letter), Statistics, Settings.
- **Game menu** (right-click, the Menu key, Shift+F10, or press and hold): *Change artwork ▸* (choose a file, pick
  from SteamGridDB, use automatic artwork), *Stop tracking*, *Delete history…*.
- **Dialogs** in the window: session details (to the second, *#3 of 12*, the game's and the day's totals, *Show
  program*; a running session counts up live), a day's sessions, game details, and a confirmation before anything is
  deleted.
- **Settings**, in this order: Appearance, Startup, General, Tracking, Artwork, Custom Games, Game Folders, Your Data,
  Ignored Games, Ignored Programs, Updates, About, Support Playtime Tracker. Headings are in title case; setting names
  and descriptions in sentence case. The ignored lists have a search box (case-insensitive, with a count like "3 of
  41", a clear button and "No matches"). *Get API key* shows only while no SteamGridDB key is saved.
- **Links**: *Get API key* opens `https://www.steamgriddb.com/profile/preferences/api` and *Tip* opens
  `https://cash.app/$LunoviaVR`, each only after checking it's exactly that address. The app sends nothing itself.
- **AMOLED mode** (off by default): in the dark theme the window and page are `#000000`, cards `#0D0D0D` with a
  brighter border, and dialogs `#161616`; the title bar matches (`DWMWA_CAPTION_COLOR`, black). It has no effect in
  the light theme. The title bar also follows light or dark (`DWMWA_USE_IMMERSIVE_DARK_MODE`). Every surface is
  opaque: the 3.0.1 glass look was removed in 3.0.2, because some graphics drivers drop an OpenGL window's
  transparency and drew it black.
- **Hardware acceleration** (on by default): Skia on OpenGL draws with the GPU (smooth, subpixel-positioned text;
  femtovg, which hints glyphs to the pixel grid, is the fallback if Skia can't start); off, the software
  renderer draws on the CPU. The choice applies when the dashboard reopens (*Reopen now*). If the GPU renderer fails,
  the dashboard falls back to the software renderer on its own and notes it in the crash log.
- **One window at a time** (`Local\PlaytimeTracker.Dashboard`, with `.Show` / `.ShowSettings` events): the tray's
  *Open dashboard* and *Settings* bring the open window forward. A launch that finds a window still closing waits a
  moment and takes over.
- **Pipe client**: it checks that the pipe belongs to the current user and is served by `playtime-tracker.exe`, and
  reconnects its event stream if the tracker restarts. Slow requests (downloading artwork) use their own connection.
- **Crash log**: `%LocalAppData%\Playtime Tracker\dashboard-errors.log` (under 1 MB), holding only errors.
- `playtime-dashboard --render <page> <W>x<H> <out.png> <messages file> [--dark] [--dialog …]` draws a page offscreen
  with the software renderer; the Screenshots workflow uses it for the README images.

## Migrations

`playtime_core::migration` runs before the tracker opens the data folder. Every step is *verify, then switch*:
nothing old is removed until its replacement has been written and read back, anything unreadable is kept under a
new name, and every step is appended to `migration.log` in the data folder.

- `Documents\Game Session Tracker` (1.x) is moved to `Documents\Playtime Tracker`; if it can't be moved yet (a file
  is open, OneDrive is syncing), the old folder is used as is and the move is retried next start.
- 1.x plain-text `sessions.json` / `settings.json` are imported into the protected files, read back, compared, and
  only then deleted. An unreadable one is kept as `sessions.unreadable-<date>.json`. Existing protected data always
  wins; a stray JSON next to it is left alone, never merged over newer data.
- Before every version change, `sessions.dat` / `settings.dat` (and their `.bak`) are copied to
  `Backups\<date> before <version>\` and compared byte for byte. The newest ten automatic backups are kept; other
  folders in `Backups` are never touched.
- The running version is recorded in the settings (`lastRunVersion`).

## Installer, CI and releases

- `build.ps1` builds both programs (release, `--locked`) and `installer/PlaytimeTracker.nsi` packs them into
  `publish\PlaytimeTrackerSetup.exe`. The version comes from `Cargo.toml`.
- The installer installs per user to `%LocalAppData%\Programs\Playtime Tracker` with the Apps entry, Start menu
  shortcut and Run entry, upgrading any earlier version in place: it closes whichever tracker is running (`--exit`),
  removes version 2.x's `PlaytimeTracker.exe`, and puts the dashboard in its own `Dashboard\` folder (the only folder
  it ever removes recursively). The data folder isn't touched by setup; the tracker's migrations back it up on first
  run. Uninstalling removes the artwork cache and, if "Delete my play history" is ticked, the data folder and the
  saved SteamGridDB key.
- **Build** workflow (every push): builds the installer on Windows, installs it silently, runs `playtime-smoke-test`
  (a real process tracked in a game folder, protected storage and read-only reports, a restart, screenshots of every
  page at several sizes, and the accent colour check), and uninstalls it, checking the history is kept.
- **Releases**: pushing a `vX.Y.Z` tag, or running the Build workflow by hand with a version, publishes a release with
  a single asset, `Setup.exe` (the name the updater looks for), and the notes in `docs/releases/vX.Y.Z.md`. The
  version must match `Cargo.toml`; a manual run creates the tag on the commit it built and never reuses an existing
  one. Only the release job can write to the repository.
- **Rust** workflow: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and `cargo audit`
  on `windows-latest` for every change to the code. Locally, `cargo check --target x86_64-pc-windows-msvc` also works
  from Linux or macOS (the toolchain file installs the target).
- **Screenshots** workflows: README images from `--render`, and the smoke test's screenshots committed to
  `docs/smoke-test/`.
- Every GitHub Action is pinned to a full commit SHA.

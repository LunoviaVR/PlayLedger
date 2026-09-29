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
| `playtime-core` | Domain model + JSON compatible with C#, `Timestamp` (.NET format), session tracker, game catalog matching, launcher metadata parsers (Steam KeyValues, Epic manifests), settings (+ migration), protected-file format and recovery rules, reports (durations, CSV), update-release parsing | **Done** (phases 2–4), 54 tests |
| `playtime-windows` | DPAPI `DataProtector`, HKCU generation store, read-only file locks, data folders | DPAPI, registry generations, file locks, discovery host, WinHTTP client, Credential Manager, exe icon provider |
| `playtime-tracker` | Background service: tray, polling loop, IPC server | Phase 7 |
| `playtime-artwork` | `ArtworkProvider` trait, image validation, on-disk cache with remembered misses, Steam local library art, Steam store CDN and SteamGridDB (online, opt-in), PNG encoder | **Done** (phase 6) |
| dashboard (WinUI 3) | Presentation only | Phase 8 |

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

## Phases

| # | Phase | Status |
| --- | --- | --- |
| 1 | Document current behaviour | Done: [`current-behavior.md`](current-behavior.md) |
| 2 | Rust workspace, domain models | Done |
| 3 | Storage and session model | Done: format, recovery, tracker (tested against C#-written JSON) |
| 4 | Game detection | Done: matching rules |
| 5 | Launcher integrations (discovery on disk/registry) | Done: `playtime_core::discovery` (Steam, Epic, GOG, Ubisoft, EA/Origin, Xbox, Riot, `X:\Games`, extra folders, Windows game list) over a `DiscoveryHost` trait; `playtime_windows::discovery::WindowsHost` |
| 6 | Artwork service | Done: see *Artwork* below |
| 7 | Tray/background tracker in Rust | Next |
| 8 | WinUI 3 dashboard | Planned |
| 9 | Migrations (verify-then-switch, logs, backups) | Planned |
| 10 | Replace the C# entry point | Planned |
| 11 | Installer and CI for the Rust build | CI checks added (`.github/workflows/rust.yml`) |
| 12 | Remove C# after parity is verified | Planned |

## Checks

`cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and `cargo audit` run on
`windows-latest` for every change to the Rust code. Locally, `cargo check --target x86_64-pc-windows-msvc` also
works from Linux/macOS (the toolchain file installs the target).

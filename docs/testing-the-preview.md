# Testing the preview on your PC

The new Playtime Tracker (a Rust tracker and a Windows 11 dashboard) is built on every push as a preview installer.
Releases keep shipping the current app until the preview has been checked on a real PC. This is that check. It
takes about 20 minutes, most of it playing a game.

The preview upgrades your existing install in place and keeps your play history. It also backs your history up
before it first runs. Even so, take the manual backup in step 1: it's your way back if anything goes wrong.

## 1. Before you install

1. Open the current app's dashboard and note, for two or three games, the **total time** and **number of
   sessions**. You'll compare these after the upgrade.
2. Choose **Exit** from the tray icon's menu.
3. Copy the whole `Documents\Playtime Tracker` folder somewhere safe (for example to your desktop). If you've used
   version 1.x, the folder may be called `Documents\Game Session Tracker` instead.

## 2. Get the preview installer

1. On GitHub, open **Actions → Build**, then the latest run on `main` with a green tick.
2. Under **Artifacts**, download **PlaytimeTracker-Preview** (you need to be signed in) and unzip it.
3. Run `PlaytimeTrackerSetup-Preview.exe`. Windows SmartScreen may warn that it's unrecognised, because the
   preview isn't code-signed. Choose **More info → Run anyway** only if you downloaded it from your own
   repository's Actions page.

## 3. Checklist

Tick each item. Anything that fails is worth a note with what you saw.

**Your data came across**

- [ ] The tray icon is back. Hovering over it shows *Playtime Tracker* and *Not playing anything*.
- [ ] **Open dashboard** opens the new dashboard. The games you noted in step 1 show the same totals and session
      counts.
- [ ] `Documents\Playtime Tracker\Backups` has a new folder named `<date> before 2.2.0`, containing
      `sessions.dat` and `settings.dat`.
- [ ] `Documents\Playtime Tracker\migration.log` exists, and its last lines say what was backed up (or
      imported, if you came from 1.x).
- [ ] Your settings (theme, ignored games, custom games, extra game folders) are the same as before.

**Tracking**

- [ ] Start a game. Within a few seconds it shows under *Playing now* on the Overview, and the tray menu's first
      line reads *Playing: \<game\> (\<time\>)*.
- [ ] If notifications are on, quitting the game shows a *Session logged* notification with its name and length.
- [ ] Play for at least a couple of minutes, then quit. The session appears in the Overview's *Sessions* list with
      the right length.
- [ ] Select the session: its details show when it started and ended, *Session #n of m*, the game's and the day's
      totals, and *Show program*, which opens the game's folder with its exe selected.
- [ ] Put the PC to sleep while a game is running, then wake it. The session doesn't count the time asleep.

**Dashboard**

- [ ] **Overview:** the chart has a bar for today. Selecting a bar lists that day's sessions. Selecting a game in
      *Games* shows only that game across the page, and *All games* goes back.
- [ ] **Games:** cover art or icons appear. Search and sort work. A game's details offer *Show on Overview*,
      *Stop tracking* and *Delete history*. Don't use *Delete history* on a game you care about.
- [ ] **History** and **Statistics** show sensible numbers for your past play.
- [ ] **Settings:** change a setting, close the dashboard and reopen it; the change is kept. *Export sessions…*
      writes a CSV, and *Open report* opens `Game Stats.txt`.
- [ ] Light and dark mode both look right (Settings → Theme, or your Windows setting).

**Windows integration**

- [ ] Restart the PC. The tracker starts on its own, unless you'd turned off *Start Playtime Tracker when I sign
      in to Windows* in Settings.
- [ ] Task Manager lists the tracker as *Playtime Tracker*, with the app icon.
- [ ] **Settings → Check now** (under updates) says you're up to date and doesn't offer anything. Releases still
      carry the current app.

**Uninstall** (optional; afterwards reinstall the preview, or go back as described below)

- [ ] **Settings → Apps → Playtime Tracker → Uninstall** removes the app, with *Delete my play history and
      settings* left unticked. `Documents\Playtime Tracker` is still there, untouched.

## Going back to the current app

1. **Exit** the preview from its tray menu.
2. Install `Setup.exe` from the latest release on GitHub. It installs over the preview.
3. If the current app doesn't show your history as it was, **Exit** it, then replace
   `Documents\Playtime Tracker` with the copy you made in step 1.

## When you're done

Tell Claude which items passed and what failed. Once everything passes, the next steps are to have releases ship
the new app, and then to remove the C# app (phase 12 in
[`architecture/rust-migration.md`](architecture/rust-migration.md)). Neither happens until you say so.

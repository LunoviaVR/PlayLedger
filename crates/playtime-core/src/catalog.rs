//! Decides whether a running executable is a game, and which one. The same matching as earlier versions
//! with the same priority order, plus a source and (where known) a stable [`GameId`] for each match.
//!
//! Discovery (reading launchers' files and the registry) fills a [`CatalogBuilder`]; this module only matches.

use crate::launchers::{GameId, GameSource};
use crate::paths;
use crate::settings::{CustomGame, Settings};
use std::collections::{HashMap, HashSet};

/// A running executable identified as a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMatch {
    pub name: String,
    pub source: GameSource,
    pub id: Option<GameId>,
}

#[derive(Debug, Clone)]
struct Location {
    dir: String,
    name: String,
    source: GameSource,
    id: Option<GameId>,
}

/// Collects what discovery found, then builds an immutable [`GameCatalog`].
#[derive(Debug, Default)]
pub struct CatalogBuilder {
    locations: Vec<Location>,
    roots: Vec<(String, GameSource)>,
    exact_exes: HashMap<String, String>,
    versioned_exes: HashMap<(String, String), String>,
}

impl CatalogBuilder {
    /// A specific game's install folder.
    pub fn add_location(&mut self, dir: &str, name: &str, source: GameSource, id: Option<GameId>) {
        let dir = paths::normalize(dir);
        let name = name.trim();
        if dir.is_empty()
            || name.is_empty()
            || self.locations.iter().any(|l| paths::eq(&l.dir, &dir))
        {
            return;
        }
        self.locations.push(Location {
            dir,
            name: name.to_string(),
            source,
            id,
        });
    }

    /// A folder where every sub-folder is a game (Steam's `common`, `X:\Games`, …).
    pub fn add_root(&mut self, dir: &str, source: GameSource) {
        let dir = paths::normalize(dir);
        if dir.is_empty() || self.roots.iter().any(|(r, _)| paths::eq(r, &dir)) {
            return;
        }
        self.roots.push((dir, source));
    }

    /// An exe Windows' game list recognised. If `still_exists` is false and the exe sat in a version folder,
    /// it's remembered by (file name, folder two levels up) so the updated game still matches.
    pub fn add_windows_game(&mut self, exe: &str, friendly_name: &str, still_exists: bool) {
        let path = paths::normalize(exe);
        let key = paths::key(&path);
        if self.exact_exes.contains_key(&key) {
            return;
        }
        if still_exists {
            self.exact_exes.insert(key, friendly_name.to_string());
        } else if paths::parent(&path)
            .map(paths::file_name)
            .is_some_and(is_version_folder)
        {
            if let Some(versioned) = versioned_key(&path) {
                self.versioned_exes
                    .entry(versioned)
                    .or_insert_with(|| paths::file_stem(&path).to_string());
            }
        }
    }

    pub fn build(mut self, settings: &Settings) -> GameCatalog {
        // Longest path first so nested install folders win over their parents.
        self.locations.sort_by(|a, b| b.dir.len().cmp(&a.dir.len()));
        GameCatalog {
            custom_games: settings
                .custom_games
                .iter()
                .filter(|g| !g.name.trim().is_empty() && !g.executable.trim().is_empty())
                .cloned()
                .collect(),
            ignored_exes: settings
                .ignored_executables
                .iter()
                .map(|e| paths::key(&normalize_exe_name(e)))
                .collect(),
            ignored_games: settings
                .ignored_games
                .iter()
                .map(|g| paths::key(g.trim()))
                .collect(),
            locations: self.locations,
            roots: self.roots,
            exact_exes: self.exact_exes,
            versioned_exes: self.versioned_exes,
        }
    }
}

#[derive(Debug)]
pub struct GameCatalog {
    custom_games: Vec<CustomGame>,
    ignored_exes: HashSet<String>,
    ignored_games: HashSet<String>,
    locations: Vec<Location>,
    roots: Vec<(String, GameSource)>,
    exact_exes: HashMap<String, String>,
    versioned_exes: HashMap<(String, String), String>,
}

impl GameCatalog {
    pub fn location_count(&self) -> usize {
        self.locations.len()
    }

    pub fn root_count(&self) -> usize {
        self.roots.len()
    }

    /// Games found per source, for the Game Sources settings page.
    pub fn count_by_source(&self) -> HashMap<GameSource, usize> {
        let mut counts = HashMap::new();
        for location in &self.locations {
            *counts.entry(location.source).or_default() += 1;
        }
        counts
    }

    /// Returns the game for a running executable, or `None` if it isn't one.
    pub fn match_exe(&self, exe_path: &str) -> Option<GameMatch> {
        let exe_path = paths::normalize(exe_path);
        let file_name = paths::file_name(&exe_path);

        for custom in &self.custom_games {
            let target = custom.executable.trim();
            let hit = if target.contains(['\\', '/']) {
                paths::eq(&exe_path, target)
            } else {
                file_name.eq_ignore_ascii_case(&normalize_exe_name(target))
            };
            if hit {
                return Some(GameMatch {
                    name: custom.name.trim().to_string(),
                    source: GameSource::Custom,
                    id: Some(GameId::Path {
                        normalized: paths::key(&exe_path),
                    }),
                });
            }
        }

        if self.ignored_exes.contains(&paths::key(file_name)) {
            return None;
        }

        for location in &self.locations {
            if let Some(relative) = paths::relative_to(&exe_path, &location.dir) {
                return (!self.is_ignored(&location.name, relative)).then(|| GameMatch {
                    name: location.name.clone(),
                    source: location.source,
                    id: location.id.clone().or_else(|| {
                        Some(GameId::Path {
                            normalized: paths::key(&location.dir),
                        })
                    }),
                });
            }
        }

        for (root, source) in &self.roots {
            let Some(relative) = paths::relative_to(&exe_path, root) else {
                continue;
            };
            let parts: Vec<&str> = relative.split('\\').filter(|p| !p.is_empty()).collect();
            if parts.len() < 2 {
                return None; // an exe directly in the root isn't inside a game's folder
            }
            let game = parts[0];
            return (!self.is_ignored(game, relative)).then(|| GameMatch {
                name: game.to_string(),
                source: *source,
                id: Some(GameId::Path {
                    normalized: paths::key(&format!("{root}\\{game}")),
                }),
            });
        }

        if let Some((key, name)) = match_built_in(&exe_path) {
            return (!self.is_ignored(name, "")).then(|| GameMatch {
                name: name.to_string(),
                source: GameSource::BuiltIn,
                id: Some(GameId::BuiltIn { key }),
            });
        }

        if let Some(name) = self.exact_exes.get(&paths::key(&exe_path)) {
            return (!self.is_ignored(name, "")).then(|| GameMatch {
                name: name.clone(),
                source: GameSource::WindowsGameList,
                id: Some(GameId::Path {
                    normalized: paths::key(&exe_path),
                }),
            });
        }

        let versioned = versioned_key(&exe_path)?;
        let name = self.versioned_exes.get(&versioned)?;
        (!self.is_ignored(name, "")).then(|| GameMatch {
            name: name.clone(),
            source: GameSource::WindowsGameList,
            id: Some(GameId::Path {
                normalized: format!("{}\\{}", versioned.1, versioned.0),
            }),
        })
    }

    fn is_ignored(&self, game: &str, relative: &str) -> bool {
        // Also catches redistributable installers etc. that live in a sub-folder of a game.
        self.ignored_games.contains(&paths::key(game))
            || relative
                .split('\\')
                .filter(|p| !p.is_empty())
                .any(|part| self.ignored_games.contains(&paths::key(part)))
    }
}

/// Games that install outside any store launcher, matched by the game's own executable only
/// (their launchers, crash handlers and editors are deliberately not counted).
const BUILT_IN_EXES: &[(&str, &str, &str)] = &[
    ("robloxplayerbeta.exe", "roblox", "Roblox"),
    ("minecraft.windows.exe", "minecraft", "Minecraft"),
    ("genshinimpact.exe", "genshin-impact", "Genshin Impact"),
    ("starrail.exe", "honkai-star-rail", "Honkai: Star Rail"),
    (
        "zenlesszonezero.exe",
        "zenless-zone-zero",
        "Zenless Zone Zero",
    ),
    ("osu!.exe", "osu", "osu!"),
    (
        "league of legends.exe",
        "league-of-legends",
        "League of Legends",
    ),
    ("valorant-win64-shipping.exe", "valorant", "VALORANT"),
    ("fortniteclient-win64-shipping.exe", "fortnite", "Fortnite"),
];

fn match_built_in(exe_path: &str) -> Option<(&'static str, &'static str)> {
    let file = paths::key(paths::file_name(exe_path));
    if let Some((_, key, name)) = BUILT_IN_EXES.iter().find(|(exe, _, _)| *exe == file) {
        return Some((key, name));
    }
    let path = paths::key(exe_path);
    // Roblox from the Microsoft Store runs as a generic "Windows10Universal.exe" inside its package folder.
    if file == "windows10universal.exe" && path.contains("robloxcorporation.roblox") {
        return Some(("roblox", "Roblox"));
    }
    // Minecraft: Java Edition runs as javaw.exe from the Minecraft Launcher's own Java runtime.
    if file == "javaw.exe"
        && (path.contains(r"\minecraft launcher\runtime\")
            || path.contains(r"\microsoft.4297127d64ec6_")
            || path.contains(r"\.minecraft\runtime\"))
    {
        return Some(("minecraft", "Minecraft"));
    }
    None
}

/// Folder names that launchers replace on every update: `version-3f2c9a…`, `app-1.2.3`, `1.2.3`, `v1.2`.
pub fn is_version_folder(folder: &str) -> bool {
    let lower = folder.to_ascii_lowercase();
    if let Some(hash) = lower.strip_prefix("version-") {
        return hash.len() >= 6 && hash.chars().all(|c| c.is_ascii_hexdigit());
    }
    if let Some(version) = lower.strip_prefix("app-") {
        return dotted_numbers(version, 2, usize::MAX);
    }
    dotted_numbers(lower.strip_prefix('v').unwrap_or(&lower), 2, 4)
}

fn dotted_numbers(text: &str, min_parts: usize, max_parts: usize) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() >= min_parts
        && parts.len() <= max_parts
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// (file name, grandparent folder), lower-cased: stays the same when a game moves into a new version folder.
fn versioned_key(exe_path: &str) -> Option<(String, String)> {
    let dir = paths::parent(exe_path)?;
    let grandparent = paths::parent(dir)?;
    if grandparent.len() <= 3 {
        return None; // too close to a drive root to say anything
    }
    Some((
        paths::key(paths::file_name(exe_path)),
        paths::key(grandparent),
    ))
}

fn normalize_exe_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.to_ascii_lowercase().ends_with(".exe") {
        trimmed.to_string()
    } else {
        format!("{trimmed}.exe")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(configure: impl FnOnce(&mut CatalogBuilder), settings: &Settings) -> GameCatalog {
        let mut builder = CatalogBuilder::default();
        configure(&mut builder);
        builder.build(settings)
    }

    fn name(c: &GameCatalog, exe: &str) -> Option<String> {
        c.match_exe(exe).map(|m| m.name)
    }

    #[test]
    fn steam_location_with_app_id() {
        let c = catalog(
            |b| {
                b.add_root(r"D:\SteamLibrary\steamapps\common", GameSource::Steam);
                b.add_location(
                    r"D:\SteamLibrary\steamapps\common\ELDEN RING",
                    "ELDEN RING",
                    GameSource::Steam,
                    Some(GameId::Steam { app_id: 1_245_620 }),
                );
            },
            &Settings::default(),
        );
        let m = c
            .match_exe(r"d:\steamlibrary\steamapps\common\ELDEN RING\Game\eldenring.exe")
            .expect("match");
        assert_eq!(m.name, "ELDEN RING");
        assert_eq!(m.id, Some(GameId::Steam { app_id: 1_245_620 }));
        // Unknown folder under the root: the folder name is the game.
        assert_eq!(
            name(&c, r"D:\SteamLibrary\steamapps\common\Hades\x64\Hades.exe").as_deref(),
            Some("Hades")
        );
        // An exe right in the root isn't a game.
        assert_eq!(
            name(&c, r"D:\SteamLibrary\steamapps\common\setup.exe"),
            None
        );
    }

    #[test]
    fn nested_location_wins_over_parent() {
        let c = catalog(
            |b| {
                b.add_location(r"C:\Games\Suite", "Suite", GameSource::Gog, None);
                b.add_location(
                    r"C:\Games\Suite\Episode 2",
                    "Episode 2",
                    GameSource::Gog,
                    None,
                );
            },
            &Settings::default(),
        );
        assert_eq!(
            name(&c, r"C:\Games\Suite\Episode 2\ep2.exe").as_deref(),
            Some("Episode 2")
        );
    }

    #[test]
    fn ignore_lists_and_redist_subfolders() {
        let settings = Settings::default();
        let c = catalog(
            |b| b.add_root(r"C:\Games", GameSource::CustomFolder),
            &settings,
        );
        assert_eq!(
            name(&c, r"C:\Games\Foo\UnityCrashHandler64.exe"),
            None,
            "ignored exe"
        );
        assert_eq!(
            name(&c, r"C:\Games\Foo\_CommonRedist\vc\setup.exe"),
            None,
            "ignored sub-folder"
        );
        assert_eq!(
            name(&c, r"C:\Games\Wallpaper Engine\wallpaper64.exe"),
            None,
            "ignored game"
        );
    }

    #[test]
    fn custom_games_win() {
        let mut settings = Settings::default();
        settings.custom_games.push(CustomGame {
            name: "Minecraft (modded)".into(),
            executable: "javaw".into(),
        });
        let c = catalog(|_| {}, &settings);
        let m = c
            .match_exe(r"C:\Program Files\Java\bin\javaw.exe")
            .expect("custom");
        assert_eq!(
            (m.name.as_str(), m.source),
            ("Minecraft (modded)", GameSource::Custom)
        );
    }

    #[test]
    fn built_in_games() {
        let c = catalog(|_| {}, &Settings::default());
        assert_eq!(
            name(
                &c,
                r"C:\Users\a\AppData\Local\Roblox\Versions\version-0123abcd\RobloxPlayerBeta.exe"
            )
            .as_deref(),
            Some("Roblox")
        );
        assert_eq!(
            name(
                &c,
                r"C:\Users\a\AppData\Local\Roblox\Versions\version-0123abcd\RobloxStudioBeta.exe"
            ),
            None
        );
        assert_eq!(name(&c, r"C:\Program Files\WindowsApps\ROBLOXCORPORATION.ROBLOX_2.6_x64__55nm5eh3cm0pr\Windows10Universal.exe").as_deref(), Some("Roblox"));
        assert_eq!(name(&c, r"C:\Program Files (x86)\Minecraft Launcher\runtime\java-runtime-gamma\windows-x64\java-runtime-gamma\bin\javaw.exe").as_deref(), Some("Minecraft"));
        assert_eq!(name(&c, r"C:\Program Files\Java\bin\javaw.exe"), None);
    }

    #[test]
    fn windows_game_list_survives_version_folder_updates() {
        let c = catalog(
            |b| {
                b.add_windows_game(r"C:\Games\Tool\app.exe", "Tool Game", true);
                b.add_windows_game(
                    r"C:\Users\a\AppData\Local\Foo\Versions\version-aaaaaa\Foo.exe",
                    "Foo",
                    false,
                );
                b.add_windows_game(r"C:\Users\a\AppData\Local\Bar\bin\Bar.exe", "Bar", false);
                // gone, not versioned
            },
            &Settings::default(),
        );
        assert_eq!(
            name(&c, r"c:\games\tool\APP.exe").as_deref(),
            Some("Tool Game")
        );
        assert_eq!(
            name(
                &c,
                r"C:\Users\a\AppData\Local\Foo\Versions\version-bbbbbb\Foo.exe"
            )
            .as_deref(),
            Some("Foo")
        );
        assert_eq!(name(&c, r"C:\Users\a\AppData\Local\Bar\bin2\Bar.exe"), None);
    }

    #[test]
    fn version_folders() {
        for yes in [
            "version-0123abcdef",
            "app-1.2.3",
            "1.20.81",
            "v2.1",
            "VERSION-ABCDEF",
        ] {
            assert!(is_version_folder(yes), "{yes}");
        }
        for no in [
            "Binaries",
            "Win64",
            "2",
            "version-xyz",
            "1.2.3.4.5",
            "app-",
            "v",
        ] {
            assert!(!is_version_folder(no), "{no}");
        }
    }
}

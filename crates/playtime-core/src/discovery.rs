//! Finds installed games (the C# `GameCatalog.Build`): Steam, Epic Games, GOG, Ubisoft Connect, EA/Origin, Xbox app,
//! Riot, `X:\Games`, the user's extra folders and Windows' own game list.
//!
//! All file-system and registry access goes through [`DiscoveryHost`], so the rules are tested here with a fake
//! host and the Windows crate only supplies the real reads. A source that fails is logged and skipped; it never
//! stops the others.

use crate::catalog::CatalogBuilder;
use crate::launchers::{epic, steam, GameId, GameSource};
use crate::paths;
use crate::settings::Settings;

/// Registry roots discovery reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

/// Which registry view to read. `Wow32` is the 32-bit view (`WOW6432Node`), where 32-bit launchers such as GOG
/// Galaxy and Ubisoft Connect write their install lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RegView {
    Default,
    Wow32,
}

/// Well-known folders discovery starts from.
#[derive(Debug, Clone, Default)]
pub struct KnownFolders {
    pub program_files: Option<String>,
    pub program_files_x86: Option<String>,
    pub program_data: Option<String>,
    /// Roots of fixed drives, e.g. `C:\`.
    pub fixed_drives: Vec<String>,
}

/// Read-only access to the machine. Every method is infallible: anything unreadable is simply absent.
pub trait DiscoveryHost {
    fn known_folders(&self) -> KnownFolders;
    fn dir_exists(&self, path: &str) -> bool;
    fn file_exists(&self, path: &str) -> bool;
    /// Reads a small text file (launcher metadata). `None` if missing, unreadable or unreasonably large.
    fn read_text(&self, path: &str) -> Option<String>;
    /// File names (not paths) directly in `dir`.
    fn list_files(&self, dir: &str) -> Vec<String>;
    fn reg_string(&self, hive: Hive, view: RegView, key: &str, value: &str) -> Option<String>;
    fn reg_subkeys(&self, hive: Hive, view: RegView, key: &str) -> Vec<String>;
    /// A display name from the exe's version resource (product name or description), if it has one.
    fn exe_product_name(&self, exe: &str) -> Option<String>;
    /// Expands `%VARIABLES%` in a user-entered folder.
    fn expand_env(&self, text: &str) -> String;
}

/// Something that went wrong reading one source or file; reported, never fatal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryIssue {
    pub source: &'static str,
    pub message: String,
}

/// What discovery found: the builder to turn into a catalog, and anything worth logging.
#[derive(Debug, Default)]
pub struct Discovered {
    pub builder: CatalogBuilder,
    pub issues: Vec<DiscoveryIssue>,
}

/// Runs every source in the C# app's order (earlier sources win for the same folder).
pub fn discover(host: &dyn DiscoveryHost, settings: &Settings) -> Discovered {
    let mut out = Discovered::default();
    let folders = host.known_folders();
    add_steam(host, &mut out);
    add_epic(host, &folders, &mut out);
    add_gog(host, &mut out);
    add_ubisoft(host, &mut out);
    add_well_known_roots(host, &folders, &mut out);
    if settings.use_windows_game_list {
        add_windows_game_list(host, settings, &mut out);
    }
    for folder in &settings.extra_game_folders {
        add_root(
            host,
            &mut out,
            &host.expand_env(folder),
            GameSource::CustomFolder,
        );
    }
    out
}

fn add_root(host: &dyn DiscoveryHost, out: &mut Discovered, dir: &str, source: GameSource) {
    if !dir.trim().is_empty() && host.dir_exists(&paths::normalize(dir)) {
        out.builder.add_root(dir, source);
    }
}

fn join(dir: &str, name: &str) -> String {
    format!("{}\\{}", dir.trim_end_matches(['\\', '/']), name)
}

// ---------- Steam ----------

fn add_steam(host: &dyn DiscoveryHost, out: &mut Discovered) {
    let installs = [
        host.reg_string(
            Hive::CurrentUser,
            RegView::Default,
            r"Software\Valve\Steam",
            "SteamPath",
        ),
        host.reg_string(
            Hive::LocalMachine,
            RegView::Default,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
    ];
    let mut libraries: Vec<String> = Vec::new();
    let mut push = |path: String| {
        let path = paths::normalize(&path);
        if !path.is_empty() && !libraries.iter().any(|l| paths::eq(l, &path)) {
            libraries.push(path);
        }
    };
    for steam in installs.into_iter().flatten() {
        let steam = paths::normalize(&steam);
        push(steam.clone());
        let vdf = join(&join(&steam, "steamapps"), "libraryfolders.vdf");
        let Some(text) = host.read_text(&vdf) else {
            continue;
        };
        match steam::parse_library_folders(&text) {
            Ok(found) => found.into_iter().for_each(&mut push),
            Err(e) => out.issues.push(DiscoveryIssue {
                source: "Steam",
                message: format!("{vdf}: {e}"),
            }),
        }
    }

    for library in libraries {
        let steamapps = join(&library, "steamapps");
        let common = join(&steamapps, "common");
        if !host.dir_exists(&common) {
            continue;
        }
        out.builder.add_root(&common, GameSource::Steam);
        let mut manifests: Vec<String> = host
            .list_files(&steamapps)
            .into_iter()
            .filter(|f| {
                let f = f.to_ascii_lowercase();
                f.starts_with("appmanifest_") && f.ends_with(".acf")
            })
            .collect();
        manifests.sort();
        for file in manifests {
            let path = join(&steamapps, &file);
            let Some(text) = host.read_text(&path) else {
                continue;
            };
            match steam::parse_app_manifest(&text) {
                Ok(Some(app)) => out.builder.add_location(
                    &join(&common, &app.install_dir),
                    &app.name,
                    GameSource::Steam,
                    Some(GameId::Steam { app_id: app.app_id }),
                ),
                Ok(None) => {}
                Err(e) => out.issues.push(DiscoveryIssue {
                    source: "Steam",
                    message: format!("{path}: {e}"),
                }),
            }
        }
    }
}

/// Steam's install folder (for its local artwork cache), if Steam is installed.
pub fn steam_install_dir(host: &dyn DiscoveryHost) -> Option<String> {
    [
        host.reg_string(
            Hive::CurrentUser,
            RegView::Default,
            r"Software\Valve\Steam",
            "SteamPath",
        ),
        host.reg_string(
            Hive::LocalMachine,
            RegView::Default,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
    ]
    .into_iter()
    .flatten()
    .map(|p| paths::normalize(&p))
    .find(|p| host.dir_exists(p))
}

// ---------- Epic Games ----------

fn add_epic(host: &dyn DiscoveryHost, folders: &KnownFolders, out: &mut Discovered) {
    let Some(program_data) = &folders.program_data else {
        return;
    };
    let manifests = join(program_data, r"Epic\EpicGamesLauncher\Data\Manifests");
    if !host.dir_exists(&manifests) {
        return;
    }
    let mut files: Vec<String> = host
        .list_files(&manifests)
        .into_iter()
        .filter(|f| f.to_ascii_lowercase().ends_with(".item"))
        .collect();
    files.sort();
    for file in files {
        let path = join(&manifests, &file);
        let Some(text) = host.read_text(&path) else {
            continue;
        };
        match epic::parse_manifest(&text) {
            Ok(Some(game)) => {
                let id = match (game.catalog_namespace, game.catalog_item_id) {
                    (Some(namespace), Some(catalog_item_id)) => Some(GameId::Epic {
                        namespace,
                        catalog_item_id,
                    }),
                    _ => None,
                };
                out.builder.add_location(
                    &game.install_location,
                    &game.display_name,
                    GameSource::Epic,
                    id,
                );
            }
            Ok(None) => {}
            Err(e) => out.issues.push(DiscoveryIssue {
                source: "Epic Games",
                message: format!("{path}: {e}"),
            }),
        }
    }
}

// ---------- GOG ----------

fn add_gog(host: &dyn DiscoveryHost, out: &mut Discovered) {
    const KEY: &str = r"SOFTWARE\GOG.com\Games";
    for id in host.reg_subkeys(Hive::LocalMachine, RegView::Wow32, KEY) {
        let key = format!("{KEY}\\{id}");
        let path = host.reg_string(Hive::LocalMachine, RegView::Wow32, &key, "path");
        let name = host.reg_string(Hive::LocalMachine, RegView::Wow32, &key, "gameName");
        if let (Some(path), Some(name)) = (path, name) {
            let product_id = id.trim();
            let game_id = (!product_id.is_empty()).then(|| GameId::Gog {
                product_id: product_id.to_string(),
            });
            out.builder
                .add_location(&path, &name, GameSource::Gog, game_id);
        }
    }
}

// ---------- Ubisoft Connect ----------

fn add_ubisoft(host: &dyn DiscoveryHost, out: &mut Discovered) {
    const KEY: &str = r"SOFTWARE\Ubisoft\Launcher\Installs";
    for id in host.reg_subkeys(Hive::LocalMachine, RegView::Wow32, KEY) {
        let key = format!("{KEY}\\{id}");
        let Some(dir) = host.reg_string(Hive::LocalMachine, RegView::Wow32, &key, "InstallDir")
        else {
            continue;
        };
        let dir = paths::normalize(&dir);
        let name = paths::file_name(&dir).to_string();
        if !name.is_empty() {
            out.builder
                .add_location(&dir, &name, GameSource::Ubisoft, None);
        }
    }
}

// ---------- Launcher folders ----------

fn add_well_known_roots(host: &dyn DiscoveryHost, folders: &KnownFolders, out: &mut Discovered) {
    if let Some(pf) = &folders.program_files {
        add_root(host, out, &join(pf, "EA Games"), GameSource::Ea);
        add_root(
            host,
            out,
            &join(pf, "ModifiableWindowsApps"),
            GameSource::Xbox,
        );
    }
    if let Some(pf86) = &folders.program_files_x86 {
        add_root(host, out, &join(pf86, "Origin Games"), GameSource::Ea);
    }
    for drive in &folders.fixed_drives {
        add_root(host, out, &join(drive, "XboxGames"), GameSource::Xbox);
        add_root(host, out, &join(drive, "Riot Games"), GameSource::Riot);
        add_root(host, out, &join(drive, "Games"), GameSource::CustomFolder);
    }
}

// ---------- Windows' game list ----------

/// Engine-generic product names that say nothing about which game it is.
const GENERIC_PRODUCT_NAMES: &[&str] = &[
    "Unity",
    "UnrealGame",
    "Unreal Engine",
    "BootstrapPackagedGame",
];

/// The name to show for an exe Windows recognised: its product name unless that's generic, else its file name.
pub fn friendly_name(exe: &str, product_names: &[Option<String>]) -> String {
    product_names
        .iter()
        .flatten()
        .map(|n| n.trim())
        .find(|n| !n.is_empty() && !GENERIC_PRODUCT_NAMES.contains(n))
        .map(str::to_string)
        .unwrap_or_else(|| paths::file_stem(exe).to_string())
}

fn add_windows_game_list(host: &dyn DiscoveryHost, settings: &Settings, out: &mut Discovered) {
    // Xbox Game Bar keeps a list of executables it has identified as games.
    const KEY: &str = r"System\GameConfigStore\Children";
    let ignored: Vec<String> = settings
        .ignored_executables
        .iter()
        .map(|e| {
            let e = e.trim().to_lowercase();
            if e.ends_with(".exe") {
                e
            } else {
                format!("{e}.exe")
            }
        })
        .collect();
    for id in host.reg_subkeys(Hive::CurrentUser, RegView::Default, KEY) {
        let Some(exe) = host.reg_string(
            Hive::CurrentUser,
            RegView::Default,
            &format!("{KEY}\\{id}"),
            "MatchedExeFullPath",
        ) else {
            continue;
        };
        if exe.trim().is_empty() {
            continue;
        }
        let path = paths::normalize(&exe);
        if ignored.contains(&paths::key(paths::file_name(&path))) {
            continue;
        }
        let exists = host.file_exists(&path);
        let name = if exists {
            friendly_name(&path, &[host.exe_product_name(&path)])
        } else {
            String::new()
        };
        out.builder.add_windows_game(&path, &name, exists);
    }
}

#[cfg(test)]
pub mod fake {
    //! An in-memory machine for discovery tests.
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(Default)]
    pub struct FakeHost {
        pub folders: KnownFolders,
        pub dirs: BTreeSet<String>,
        pub files: BTreeMap<String, String>,
        pub registry: BTreeMap<(Hive, RegView, String), BTreeMap<String, String>>,
        pub product_names: BTreeMap<String, String>,
    }

    impl FakeHost {
        pub fn dir(&mut self, path: &str) -> &mut Self {
            self.dirs.insert(paths::key(&paths::normalize(path)));
            self
        }

        pub fn file(&mut self, path: &str, contents: &str) -> &mut Self {
            let path = paths::normalize(path);
            if let Some(parent) = paths::parent(&path) {
                self.dirs.insert(paths::key(parent));
            }
            self.files.insert(paths::key(&path), contents.to_string());
            self
        }

        pub fn reg(
            &mut self,
            hive: Hive,
            view: RegView,
            key: &str,
            value: &str,
            data: &str,
        ) -> &mut Self {
            self.registry
                .entry((hive, view, key.to_lowercase()))
                .or_default()
                .insert(value.to_lowercase(), data.to_string());
            self
        }
    }

    impl DiscoveryHost for FakeHost {
        fn known_folders(&self) -> KnownFolders {
            self.folders.clone()
        }
        fn dir_exists(&self, path: &str) -> bool {
            self.dirs.contains(&paths::key(&paths::normalize(path)))
        }
        fn file_exists(&self, path: &str) -> bool {
            self.files
                .contains_key(&paths::key(&paths::normalize(path)))
        }
        fn read_text(&self, path: &str) -> Option<String> {
            self.files
                .get(&paths::key(&paths::normalize(path)))
                .cloned()
        }
        fn list_files(&self, dir: &str) -> Vec<String> {
            let dir = paths::key(&paths::normalize(dir));
            self.files
                .keys()
                .filter(|f| paths::parent(f) == Some(dir.as_str()))
                .map(|f| paths::file_name(f).to_string())
                .collect()
        }
        fn reg_string(&self, hive: Hive, view: RegView, key: &str, value: &str) -> Option<String> {
            self.registry
                .get(&(hive, view, key.to_lowercase()))?
                .get(&value.to_lowercase())
                .cloned()
        }
        fn reg_subkeys(&self, hive: Hive, view: RegView, key: &str) -> Vec<String> {
            let prefix = format!("{}\\", key.to_lowercase());
            let mut names: Vec<String> = self
                .registry
                .keys()
                .filter(|(h, v, k)| *h == hive && *v == view && k.starts_with(&prefix))
                .filter_map(|(_, _, k)| k[prefix.len()..].split('\\').next().map(str::to_string))
                .collect();
            names.dedup();
            names
        }
        fn exe_product_name(&self, exe: &str) -> Option<String> {
            self.product_names
                .get(&paths::key(&paths::normalize(exe)))
                .cloned()
        }
        fn expand_env(&self, text: &str) -> String {
            text.replace("%USERPROFILE%", r"C:\Users\me")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeHost;
    use super::*;

    fn settings() -> Settings {
        Settings::default()
    }

    fn matched(
        host: &FakeHost,
        settings: &Settings,
        exe: &str,
    ) -> Option<(String, GameSource, Option<GameId>)> {
        let found = discover(host, settings);
        found
            .builder
            .build(settings)
            .match_exe(exe)
            .map(|m| (m.name, m.source, m.id))
    }

    #[test]
    fn steam_libraries_and_manifests() {
        let mut host = FakeHost::default();
        host.reg(Hive::CurrentUser, RegView::Default, r"Software\Valve\Steam", "SteamPath", "c:/program files (x86)/steam")
            .file(
                r"C:\Program Files (x86)\Steam\steamapps\libraryfolders.vdf",
                r#""libraryfolders" { "0" { "path" "C:\\Program Files (x86)\\Steam" } "1" { "path" "E:\\SteamLibrary" } }"#,
            )
            .dir(r"C:\Program Files (x86)\Steam\steamapps\common")
            .dir(r"E:\SteamLibrary\steamapps\common")
            .file(
                r"E:\SteamLibrary\steamapps\appmanifest_620.acf",
                r#""AppState" { "appid" "620" "name" "Portal 2" "installdir" "Portal 2" }"#,
            )
            .file(r"E:\SteamLibrary\steamapps\appmanifest_1.acf", "not { valid");

        let found = discover(&host, &settings());
        assert_eq!(found.issues.len(), 1, "the broken manifest is reported");
        let catalog = found.builder.build(&settings());
        let hit = catalog
            .match_exe(r"E:\SteamLibrary\steamapps\common\Portal 2\portal2.exe")
            .expect("matched");
        assert_eq!(hit.name, "Portal 2");
        assert_eq!(hit.id, Some(GameId::Steam { app_id: 620 }));
        // A game folder without a manifest still counts through the library's `common` root.
        let root = catalog
            .match_exe(r"C:\Program Files (x86)\Steam\steamapps\common\Other Game\bin\game.exe")
            .expect("matched");
        assert_eq!(
            (root.name.as_str(), root.source),
            ("Other Game", GameSource::Steam)
        );
    }

    #[test]
    fn epic_manifests_skip_engines() {
        let mut host = FakeHost::default();
        host.folders.program_data = Some(r"C:\ProgramData".into());
        host.file(
            r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests\A.item",
            r#"{"DisplayName":"Fortnite","InstallLocation":"D:\\Epic\\Fortnite","CatalogNamespace":"fn","CatalogItemId":"4fe7"}"#,
        )
        .file(
            r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests\B.item",
            r#"{"DisplayName":"Unreal Engine","InstallLocation":"D:\\UE","AppCategories":["engines"]}"#,
        );
        let s = settings();
        assert_eq!(
            matched(
                &host,
                &s,
                r"D:\Epic\Fortnite\FortniteGame\Binaries\Win64\x.exe"
            ),
            Some((
                "Fortnite".into(),
                GameSource::Epic,
                Some(GameId::Epic {
                    namespace: "fn".into(),
                    catalog_item_id: "4fe7".into()
                })
            ))
        );
        assert_eq!(
            matched(&host, &s, r"D:\UE\Engine\Binaries\UnrealEditor.exe"),
            None
        );
    }

    #[test]
    fn gog_and_ubisoft_from_the_32_bit_registry() {
        let mut host = FakeHost::default();
        host.reg(
            Hive::LocalMachine,
            RegView::Wow32,
            r"SOFTWARE\GOG.com\Games\1207658924",
            "path",
            r"C:\GOG Games\Witcher 3",
        )
        .reg(
            Hive::LocalMachine,
            RegView::Wow32,
            r"SOFTWARE\GOG.com\Games\1207658924",
            "gameName",
            "The Witcher 3",
        )
        .reg(
            Hive::LocalMachine,
            RegView::Wow32,
            r"SOFTWARE\Ubisoft\Launcher\Installs\635",
            "InstallDir",
            "C:/Ubisoft/Assassin's Creed Origins/",
        );
        let s = settings();
        let (name, source, id) =
            matched(&host, &s, r"C:\GOG Games\Witcher 3\bin\x64\witcher3.exe").expect("gog");
        assert_eq!((name.as_str(), source), ("The Witcher 3", GameSource::Gog));
        assert_eq!(
            id,
            Some(GameId::Gog {
                product_id: "1207658924".into()
            })
        );
        let (name, source, _) = matched(
            &host,
            &s,
            r"C:\Ubisoft\Assassin's Creed Origins\ACOrigins.exe",
        )
        .expect("ubi");
        assert_eq!(
            (name.as_str(), source),
            ("Assassin's Creed Origins", GameSource::Ubisoft)
        );
    }

    #[test]
    fn well_known_roots_only_when_they_exist() {
        let mut host = FakeHost::default();
        host.folders.fixed_drives = vec![r"C:\".into(), r"D:\".into()];
        host.dir(r"D:\XboxGames").dir(r"C:\Games");
        let s = settings();
        assert_eq!(
            matched(
                &host,
                &s,
                r"D:\XboxGames\Forza Horizon 5\Content\ForzaHorizon5.exe"
            )
            .map(|m| (m.0, m.1)),
            Some(("Forza Horizon 5".into(), GameSource::Xbox))
        );
        assert_eq!(
            matched(&host, &s, r"C:\Games\Celeste\Celeste.exe").map(|m| m.0),
            Some("Celeste".into())
        );
        assert_eq!(
            matched(&host, &s, r"C:\Riot Games\VALORANT\live\x.exe"),
            None
        );
    }

    #[test]
    fn extra_folders_expand_variables() {
        let mut host = FakeHost::default();
        host.dir(r"C:\Users\me\MyGames");
        let mut s = settings();
        s.extra_game_folders = vec![r"%USERPROFILE%\MyGames".into()];
        assert_eq!(
            matched(&host, &s, r"C:\Users\me\MyGames\Hades\Hades.exe").map(|m| (m.0, m.1)),
            Some(("Hades".into(), GameSource::CustomFolder))
        );
    }

    #[test]
    fn windows_game_list_names_and_version_folders() {
        let mut host = FakeHost::default();
        let key = r"System\GameConfigStore\Children";
        host.reg(
            Hive::CurrentUser,
            RegView::Default,
            &format!(r"{key}\a"),
            "MatchedExeFullPath",
            r"C:\Apps\Tunic\Tunic.exe",
        )
        .file(r"C:\Apps\Tunic\Tunic.exe", "")
        .reg(
            Hive::CurrentUser,
            RegView::Default,
            &format!(r"{key}\b"),
            "MatchedExeFullPath",
            r"C:\Apps\Engine\Game.exe",
        )
        .file(r"C:\Apps\Engine\Game.exe", "")
        .reg(
            Hive::CurrentUser,
            RegView::Default,
            &format!(r"{key}\c"),
            "MatchedExeFullPath",
            r"C:\Users\me\AppData\Local\Thing\app-1.2.3\Thing.exe",
        )
        .reg(
            Hive::CurrentUser,
            RegView::Default,
            &format!(r"{key}\d"),
            "MatchedExeFullPath",
            r"C:\Tools\steamwebhelper.exe",
        )
        .file(r"C:\Tools\steamwebhelper.exe", "");
        host.product_names
            .insert(paths::key(r"C:\Apps\Tunic\Tunic.exe"), "TUNIC".into());
        host.product_names
            .insert(paths::key(r"C:\Apps\Engine\Game.exe"), "Unity".into());

        let mut s = settings();
        s.ignored_executables = vec!["steamwebhelper".into()];
        assert_eq!(
            matched(&host, &s, r"C:\Apps\Tunic\Tunic.exe").map(|m| m.0),
            Some("TUNIC".into())
        );
        assert_eq!(
            matched(&host, &s, r"C:\Apps\Engine\Game.exe").map(|m| m.0),
            Some("Game".into())
        );
        assert_eq!(
            matched(
                &host,
                &s,
                r"C:\Users\me\AppData\Local\Thing\app-1.3.0\Thing.exe"
            )
            .map(|m| m.0),
            Some("Thing".into())
        );
        assert_eq!(matched(&host, &s, r"C:\Tools\steamwebhelper.exe"), None);

        s.use_windows_game_list = false;
        assert_eq!(matched(&host, &s, r"C:\Apps\Tunic\Tunic.exe"), None);
    }

    #[test]
    fn friendly_names() {
        assert_eq!(
            friendly_name(r"C:\a\Game.exe", &[Some("  ".into()), None]),
            "Game"
        );
        assert_eq!(
            friendly_name(
                r"C:\a\Game.exe",
                &[Some("UnrealGame".into()), Some("Real".into())]
            ),
            "Real"
        );
    }
}

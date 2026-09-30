//! User settings, in the same JSON shape earlier versions store inside `settings.dat` (camelCase, all fields written).
//! Unknown fields are ignored and missing ones take their defaults, so both versions read each other's files.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A game the tracker can't find on its own, identified by its executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomGame {
    /// Name shown for the game, e.g. "Minecraft".
    pub name: String,
    /// Executable file name ("javaw.exe") or a full path.
    pub executable: String,
}

/// Light/dark appearance. Stored as "system" / "dark" / "light"; anything else reads as `System`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

impl Serialize for ThemeMode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ThemeMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Option::<String>::deserialize(deserializer)?.unwrap_or_default();
        Ok(match text.trim().to_ascii_lowercase().as_str() {
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::System,
        })
    }
}

/// Current format of the settings file, for one-time migrations.
pub const CURRENT_SETTINGS_VERSION: i32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// How often running programs are checked.
    pub poll_interval_seconds: i32,
    /// Sessions shorter than this are not recorded. Off (0) by default: every time a game is open counts.
    pub minimum_session_seconds: i32,
    /// A game that closes and reopens within this many seconds continues the same session.
    pub grace_period_seconds: i32,
    pub show_notifications: bool,
    /// Also treat programs Windows' Xbox Game Bar recognises as games.
    pub use_windows_game_list: bool,
    pub theme_mode: ThemeMode,
    /// A preset name ("blue", "violet", …) or a custom colour like "#ff8800".
    pub accent_color: String,
    pub extra_game_folders: Vec<String>,
    pub custom_games: Vec<CustomGame>,
    pub ignored_games: Vec<String>,
    pub ignored_executables: Vec<String>,
    pub check_for_updates: bool,
    pub install_updates_automatically: bool,
    /// The version that last ran, to say "Updated to x.y.z" once after an update.
    pub last_run_version: String,
    pub settings_version: i32,
    /// Set once the app has turned on "Start with Windows" for the first time.
    pub startup_configured: bool,
    /// Fetch missing game artwork online (Steam store images, and SteamGridDB if the user added an API key).
    /// Off by default: until the user turns it on, nothing about their games leaves the PC.
    pub online_artwork: bool,
    /// AMOLED mode: the dashboard's dark theme uses true black. Older files' `glassEffects` (the removed glass look)
    /// is ignored.
    pub amoled_black: bool,
    /// The dashboard draws with the GPU; off draws on the CPU only (for driver trouble or remote desktop).
    pub hardware_acceleration: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            poll_interval_seconds: 5,
            minimum_session_seconds: 0,
            grace_period_seconds: 20,
            show_notifications: true,
            use_windows_game_list: true,
            theme_mode: ThemeMode::System,
            accent_color: "blue".into(),
            extra_game_folders: Vec::new(),
            custom_games: Vec::new(),
            ignored_games: DEFAULT_IGNORED_GAMES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            ignored_executables: DEFAULT_IGNORED_EXECUTABLES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            check_for_updates: true,
            install_updates_automatically: true,
            last_run_version: String::new(),
            // A file without the field predates versioning (see `normalize`).
            settings_version: 0,
            startup_configured: false,
            online_artwork: false,
            amoled_black: false,
            hardware_acceleration: true,
        }
    }
}

impl Settings {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let mut value: serde_json::Value = serde_json::from_str(json)?;
        // Earlier versions may write null for strings/lists; treat null as "use the default".
        if let Some(object) = value.as_object_mut() {
            object.retain(|_, v| !v.is_null());
        }
        let mut settings: Settings = serde_json::from_value(value)?;
        settings.normalize();
        Ok(settings)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Clamps values into sensible ranges and applies one-time migrations (the same rules as earlier versions).
    pub fn normalize(&mut self) {
        self.poll_interval_seconds = self.poll_interval_seconds.clamp(1, 300);
        if self.settings_version < 2 && self.minimum_session_seconds == 30 {
            // The old default silently dropped real short plays; keep any value the user chose.
            self.minimum_session_seconds = 0;
        }
        self.settings_version = self.settings_version.max(CURRENT_SETTINGS_VERSION);
        self.minimum_session_seconds = self.minimum_session_seconds.max(0);
        self.grace_period_seconds = self.grace_period_seconds.max(0);
        let accent = self.accent_color.trim().to_lowercase();
        self.accent_color = if accent.is_empty() {
            "blue".into()
        } else {
            accent
        };
    }

    pub fn tracking_rules(&self) -> crate::tracking::TrackingRules {
        crate::tracking::TrackingRules {
            minimum_session_seconds: i64::from(self.minimum_session_seconds),
            grace_period_seconds: i64::from(self.grace_period_seconds),
        }
    }
}

pub const DEFAULT_IGNORED_GAMES: &[&str] = &[
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
];

pub const DEFAULT_IGNORED_EXECUTABLES: &[&str] = &[
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
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_earlier_versions_settings() {
        let json = r##"{"pollIntervalSeconds":5,"minimumSessionSeconds":0,"gracePeriodSeconds":20,"showNotifications":true,
            "useWindowsGameList":true,"themeMode":"dark","accentColor":"#FF8800","extraGameFolders":["D:\\Games"],
            "customGames":[{"name":"Minecraft","executable":"javaw.exe"}],"ignoredGames":[],"ignoredExecutables":["x.exe"],
            "checkForUpdates":false,"installUpdatesAutomatically":true,"lastRunVersion":"2.1.0","settingsVersion":2,"startupConfigured":true}"##;
        let s = Settings::from_json(json).expect("parses");
        assert_eq!(s.theme_mode, ThemeMode::Dark);
        assert_eq!(s.accent_color, "#ff8800");
        assert_eq!(s.custom_games[0].executable, "javaw.exe");
        assert!(!s.check_for_updates && s.startup_configured);
        assert!(
            s.ignored_games.is_empty(),
            "an empty list the user chose stays empty"
        );
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s = Settings::from_json("{}").expect("parses");
        assert_eq!(s.poll_interval_seconds, 5);
        assert!(s
            .ignored_executables
            .iter()
            .any(|e| e == "steamwebhelper.exe"));
        assert_eq!(s.settings_version, CURRENT_SETTINGS_VERSION);
    }

    #[test]
    fn amoled_mode_replaces_glass_effects() {
        // 3.0.x wrote glassEffects; it's ignored, and AMOLED mode starts off.
        let old = Settings::from_json(r#"{"glassEffects":true}"#).expect("parses");
        assert!(!old.amoled_black);
        let json = Settings::from_json(r#"{"amoledBlack":true}"#)
            .expect("parses")
            .to_json()
            .expect("serializes");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["amoledBlack"], true);
        assert!(value.get("glassEffects").is_none());
    }

    #[test]
    fn migration_and_clamping() {
        let old = Settings::from_json(
            r#"{"minimumSessionSeconds":30,"pollIntervalSeconds":0,"themeMode":"purple"}"#,
        )
        .expect("parses");
        assert_eq!(old.minimum_session_seconds, 0, "old 30 s default migrated");
        assert_eq!(old.poll_interval_seconds, 1);
        assert_eq!(old.theme_mode, ThemeMode::System);
        let chosen = Settings::from_json(r#"{"minimumSessionSeconds":30,"settingsVersion":2}"#)
            .expect("parses");
        assert_eq!(
            chosen.minimum_session_seconds, 30,
            "a value chosen after the migration is kept"
        );
        let nulls =
            Settings::from_json(r#"{"accentColor":null,"customGames":null}"#).expect("parses");
        assert_eq!(nulls.accent_color, "blue");
    }

    #[test]
    fn round_trips() {
        let s = Settings::from_json(r#"{"themeMode":"light","extraGameFolders":["E:\\G"]}"#)
            .expect("parses");
        assert_eq!(
            Settings::from_json(&s.to_json().expect("serializes")).expect("parses"),
            s
        );
    }
}

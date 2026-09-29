//! Epic Games Launcher: one JSON `*.item` manifest per installed app under
//! `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests`.

use serde::Deserialize;

/// An installed Epic game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicGame {
    pub display_name: String,
    pub install_location: String,
    pub catalog_namespace: Option<String>,
    pub catalog_item_id: Option<String>,
    pub app_name: Option<String>,
    /// Path of the game's exe relative to the install folder, when the manifest has it.
    pub launch_executable: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Manifest {
    display_name: Option<String>,
    install_location: Option<String>,
    catalog_namespace: Option<String>,
    catalog_item_id: Option<String>,
    app_name: Option<String>,
    launch_executable: Option<String>,
    #[serde(default)]
    app_categories: Vec<String>,
}

/// Parses a manifest. `Ok(None)` for entries that aren't games (Unreal Engine, plugins) or lack a name/location.
pub fn parse_manifest(json: &str) -> Result<Option<EpicGame>, serde_json::Error> {
    let m: Manifest = serde_json::from_str(json)?;
    if m.app_categories
        .iter()
        .any(|c| c.eq_ignore_ascii_case("engines") || c.eq_ignore_ascii_case("plugins"))
    {
        return Ok(None);
    }
    let non_empty = |s: Option<String>| s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let (Some(display_name), Some(install_location)) =
        (non_empty(m.display_name), non_empty(m.install_location))
    else {
        return Ok(None);
    };
    Ok(Some(EpicGame {
        display_name,
        install_location,
        catalog_namespace: non_empty(m.catalog_namespace),
        catalog_item_id: non_empty(m.catalog_item_id),
        app_name: non_empty(m.app_name),
        launch_executable: non_empty(m.launch_executable),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_manifest() {
        let json = r#"{"FormatVersion":0,"DisplayName":"Fortnite","InstallLocation":"C:\\Program Files\\Epic Games\\Fortnite",
            "CatalogNamespace":"fn","CatalogItemId":"4fe75bbc5a674f4f9b356b5c90567da5","AppName":"Fortnite",
            "LaunchExecutable":"FortniteGame/Binaries/Win64/FortniteLauncher.exe","AppCategories":["public","games","applications"]}"#;
        let game = parse_manifest(json).expect("ok").expect("a game");
        assert_eq!(game.display_name, "Fortnite");
        assert_eq!(game.catalog_namespace.as_deref(), Some("fn"));
    }

    #[test]
    fn engines_plugins_and_incomplete_are_skipped() {
        assert_eq!(parse_manifest(r#"{"DisplayName":"Unreal Engine","InstallLocation":"C:\\UE","AppCategories":["engines"]}"#).expect("ok"), None);
        assert_eq!(
            parse_manifest(r#"{"DisplayName":"","InstallLocation":"C:\\X"}"#).expect("ok"),
            None
        );
        assert!(parse_manifest("not json").is_err());
    }
}

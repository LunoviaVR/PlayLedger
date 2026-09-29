//! Steam: `steamapps/libraryfolders.vdf` lists the library folders; each library's
//! `steamapps/appmanifest_<appid>.acf` names an installed game and its folder under `steamapps/common`.

use super::keyvalues::{self, ParseError, Value};

/// An installed Steam game from an app manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamApp {
    pub app_id: u32,
    pub name: String,
    /// Folder name under `steamapps\common`.
    pub install_dir: String,
}

/// Library folder paths from `libraryfolders.vdf` (both the current nested format and the old flat one).
pub fn parse_library_folders(text: &str) -> Result<Vec<String>, ParseError> {
    let doc = keyvalues::parse(text)?;
    let Some(root) = doc
        .block("libraryfolders")
        .or_else(|| doc.block("LibraryFolders"))
    else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    for (key, value) in root.iter() {
        match value {
            // Current format: "0" { "path" "D:\\SteamLibrary" ... }
            Value::Block(entry) => {
                if let Some(path) = entry.text("path") {
                    paths.push(path.to_string());
                }
            }
            // Old format: "1" "D:\\SteamLibrary"
            Value::Text(path) if key.chars().all(|c| c.is_ascii_digit()) => {
                paths.push(path.clone())
            }
            Value::Text(_) => {}
        }
    }
    Ok(paths)
}

/// Parses an `appmanifest_*.acf`. Returns `Ok(None)` if it lacks the fields needed to identify a game.
pub fn parse_app_manifest(text: &str) -> Result<Option<SteamApp>, ParseError> {
    let doc = keyvalues::parse(text)?;
    let Some(state) = doc.block("AppState") else {
        return Ok(None);
    };
    let app_id = state
        .text("appid")
        .and_then(|id| id.trim().parse::<u32>().ok());
    let name = state.text("name").map(str::trim).filter(|n| !n.is_empty());
    let install_dir = state
        .text("installdir")
        .map(str::trim)
        .filter(|d| !d.is_empty());
    Ok(match (app_id, name, install_dir) {
        (Some(app_id), Some(name), Some(install_dir)) => Some(SteamApp {
            app_id,
            name: name.to_string(),
            install_dir: install_dir.to_string(),
        }),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_folders_both_formats() {
        let current = r#""libraryfolders" { "0" { "path" "C:\\Program Files (x86)\\Steam" "apps" { "228980" "0" } } "1" { "path" "E:\\Games\\Steam" } }"#;
        assert_eq!(
            parse_library_folders(current).expect("ok"),
            [r"C:\Program Files (x86)\Steam", r"E:\Games\Steam"]
        );
        let old = r#""LibraryFolders" { "TimeNextStatsReport" "123" "ContentStatsID" "-1" "1" "D:\\SteamLibrary" }"#;
        assert_eq!(
            parse_library_folders(old).expect("ok"),
            [r"D:\SteamLibrary"]
        );
    }

    #[test]
    fn app_manifest() {
        let acf = r#""AppState" { "appid" "1245620" "Universe" "1" "name" "ELDEN RING" "StateFlags" "4" "installdir" "ELDEN RING" }"#;
        assert_eq!(
            parse_app_manifest(acf).expect("ok"),
            Some(SteamApp {
                app_id: 1_245_620,
                name: "ELDEN RING".into(),
                install_dir: "ELDEN RING".into()
            })
        );
    }

    #[test]
    fn incomplete_or_broken_manifests_do_not_panic() {
        assert_eq!(
            parse_app_manifest(r#""AppState" { "appid" "x" "name" "A" "installdir" "A" }"#)
                .expect("ok"),
            None
        );
        assert_eq!(parse_app_manifest(r#""Other" { }"#).expect("ok"), None);
        assert!(parse_app_manifest(r#""AppState" { "appid" "#).is_err());
    }
}

//! Parsers for launcher metadata files. Pure functions over file contents, so malformed files
//! are just errors (never panics) and everything is testable without the launchers installed.
//! Finding the files on disk and in the registry is the Windows crate's job.

pub mod epic;
pub mod keyvalues;
pub mod steam;

use std::fmt;

/// Where a game was found. Also used for the Games page's platform filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GameSource {
    Steam,
    Epic,
    Gog,
    Ubisoft,
    Ea,
    Xbox,
    Riot,
    /// A folder the user added in Settings (each sub-folder is a game).
    CustomFolder,
    /// A game the user picked by its .exe in Settings.
    Custom,
    /// Recognised by Windows' Xbox Game Bar.
    WindowsGameList,
    /// The built-in list of games that install outside any launcher (Roblox, Minecraft, …).
    BuiltIn,
}

impl GameSource {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Epic => "Epic Games",
            Self::Gog => "GOG",
            Self::Ubisoft => "Ubisoft Connect",
            Self::Ea => "EA",
            Self::Xbox => "Xbox",
            Self::Riot => "Riot Games",
            Self::CustomFolder => "Custom folder",
            Self::Custom => "Custom",
            Self::WindowsGameList => "Windows",
            Self::BuiltIn => "Other",
        }
    }
}

impl fmt::Display for GameSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// A stable identity for a game, used for artwork lookups and to survive renames/moves.
/// Session history stays keyed by display name, as in earlier versions, for compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GameId {
    Steam {
        app_id: u32,
    },
    Epic {
        namespace: String,
        catalog_item_id: String,
    },
    /// Windows package family name (Xbox / Microsoft Store).
    Package {
        family_name: String,
    },
    Gog {
        product_id: String,
    },
    /// Anything else: a normalized, lower-cased executable path or install folder.
    Path {
        normalized: String,
    },
    /// One of the built-in games, by a fixed key (e.g. "roblox").
    BuiltIn {
        key: &'static str,
    },
}

impl GameId {
    /// A file-system-safe key, e.g. `steam_438100`, used for the artwork cache folder.
    pub fn cache_key(&self) -> String {
        fn slug(text: &str) -> String {
            let mut out: String = text
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect();
            out.truncate(96);
            out
        }
        match self {
            Self::Steam { app_id } => format!("steam_{app_id}"),
            Self::Epic {
                namespace,
                catalog_item_id,
            } => format!("epic_{}_{}", slug(namespace), slug(catalog_item_id)),
            Self::Package { family_name } => format!("pkg_{}", slug(family_name)),
            Self::Gog { product_id } => format!("gog_{}", slug(product_id)),
            Self::Path { normalized } => format!("path_{:016x}", fnv1a(normalized.as_bytes())),
            Self::BuiltIn { key } => format!("builtin_{}", slug(key)),
        }
    }
}

/// Stable 64-bit FNV-1a hash (std's hasher isn't stable across releases, and the key names cache folders).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_keys_are_stable_and_safe() {
        assert_eq!(
            GameId::Steam { app_id: 438_100 }.cache_key(),
            "steam_438100"
        );
        assert_eq!(
            GameId::Epic {
                namespace: "fn".into(),
                catalog_item_id: "4fe75bbc/5a0a".into()
            }
            .cache_key(),
            "epic_fn_4fe75bbc_5a0a"
        );
        let a = GameId::Path {
            normalized: r"c:\games\x\x.exe".into(),
        }
        .cache_key();
        assert_eq!(
            a,
            GameId::Path {
                normalized: r"c:\games\x\x.exe".into()
            }
            .cache_key()
        );
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    }
}

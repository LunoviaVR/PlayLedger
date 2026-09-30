//! The Games page: a tile per game with its box art (or its icon), search and sort, and what's playing first.
//! Artwork is read and scaled on a worker thread; tiles are updated in place as each picture arrives.

use crate::format;
use crate::ui::{AppWindow, GameTile};
use playtime_core::dashboard::GameView;
use playtime_core::ipc::DashboardSnapshot;
use playtime_core::paths;
use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

/// Tiles are 160 × 240; pictures are kept at twice that for high-DPI screens.
pub const COVER_SIZE: (u32, u32) = (320, 480);
/// Exe icons are drawn at 64 × 64.
pub const ICON_SIZE: (u32, u32) = (128, 128);
/// Choices offered from SteamGridDB are previewed at 100 × 150.
pub const PREVIEW_SIZE: (u32, u32) = (200, 300);

/// The largest picture file read, as the tracker also allows.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// The largest picture side decoded, as the tracker also allows.
const MAX_SIDE: u32 = 8192;

/// A game's picture: box art, or its exe's icon when there is none.
#[derive(Clone)]
pub struct Art {
    pub image: Image,
    pub icon: bool,
}

#[derive(Default)]
pub struct GamesState {
    pub query: String,
    /// 0 most played, 1 recently played, 2 name.
    pub sort: i32,
    /// The games the two lists show, in order, so a click can be mapped back.
    pub live: Vec<String>,
    pub played: Vec<String>,
    /// Loaded pictures by game key; a game with an entry of `None` is being loaded or has none.
    pub art: HashMap<String, Option<Art>>,
}

fn s(text: impl Into<SharedString>) -> SharedString {
    text.into()
}

fn tile(game: &GameView, now: playtime_core::Timestamp, art: Option<&Art>) -> GameTile {
    let sessions = if game.session_count == 1 {
        "1 session".to_string()
    } else {
        format!("{} sessions", game.session_count)
    };
    let detail = if game.is_live {
        format!("{} · Playing now", format::duration(game.total_seconds))
    } else {
        format!(
            "{} · {sessions} · {}",
            format::duration(game.total_seconds),
            format::day_of(game.last_played, now)
        )
    };
    GameTile {
        name: s(&game.name),
        detail: s(detail),
        initial: s(game
            .name
            .chars()
            .next()
            .map_or("?".into(), |c| c.to_uppercase().to_string())),
        live: game.is_live,
        art: art.map(|a| a.image.clone()).unwrap_or_default(),
        has_art: art.is_some(),
        art_is_icon: art.is_some_and(|a| a.icon),
        accessible: s(format!(
            "{}, {} played, {sessions}{}",
            game.name,
            format::spoken_duration(game.total_seconds),
            if game.is_live { ", playing now" } else { "" }
        )),
    }
}

/// The games matching the search, in the chosen order.
pub fn visible<'a>(games: &'a [GameView], query: &str, sort: i32) -> Vec<&'a GameView> {
    let query = query.trim().to_lowercase();
    let mut list: Vec<&GameView> = games
        .iter()
        .filter(|g| query.is_empty() || g.name.to_lowercase().contains(&query))
        .collect();
    match sort {
        1 => list.sort_by(|a, b| b.last_played.cmp(&a.last_played)),
        2 => list.sort_by_key(|g| g.name.to_lowercase()),
        _ => list.sort_by(|a, b| b.total_seconds.cmp(&a.total_seconds)),
    }
    list
}

/// Shows the page. Returns the games whose pictures haven't been asked for yet.
pub fn render(
    window: &AppWindow,
    snapshot: &DashboardSnapshot,
    state: &mut GamesState,
) -> Vec<String> {
    let model = &snapshot.model;
    let list = visible(&model.games, &state.query, state.sort);
    let mut wanted = Vec::new();
    let mut make = |games: Vec<&GameView>| {
        let tiles: Vec<GameTile> = games
            .iter()
            .map(|g| {
                let key = paths::key(&g.name);
                if !state.art.contains_key(&key) {
                    state.art.insert(key.clone(), None);
                    wanted.push(g.name.clone());
                }
                tile(g, model.now, state.art.get(&key).and_then(Option::as_ref))
            })
            .collect();
        let names = games.iter().map(|g| g.name.clone()).collect::<Vec<_>>();
        (ModelRc::new(VecModel::from(tiles)), names)
    };
    let (live, live_names) = make(list.iter().copied().filter(|g| g.is_live).collect());
    let (played, played_names) = make(list.iter().copied().filter(|g| !g.is_live).collect());
    window.set_games_live(live);
    window.set_games_played(played);
    window.set_games_empty(model.games.is_empty());
    state.live = live_names;
    state.played = played_names;
    wanted
}

/// Puts a game's newly loaded picture on its tile, wherever it's shown.
pub fn art_arrived(window: &AppWindow, state: &mut GamesState, game: &str, art: Option<Art>) {
    let key = paths::key(game);
    for (names, model) in [
        (&state.live, window.get_games_live()),
        (&state.played, window.get_games_played()),
    ] {
        if let Some(i) = names.iter().position(|n| paths::key(n) == key) {
            if let Some(mut row) = model.row_data(i) {
                row.art = art.as_ref().map(|a| a.image.clone()).unwrap_or_default();
                row.has_art = art.is_some();
                row.art_is_icon = art.as_ref().is_some_and(|a| a.icon);
                model.set_row_data(i, row);
            }
        }
    }
    state.art.insert(key, art);
}

/// The game a tile stands for (0 playing now, 1 played before).
pub fn game_at(state: &GamesState, section: i32, index: i32) -> Option<String> {
    let names = if section == 0 {
        &state.live
    } else {
        &state.played
    };
    names.get(usize::try_from(index).ok()?).cloned()
}

/// Reads a PNG or JPEG and scales it down to fit `max` (never up). Runs off the UI thread; the pixels are handed
/// to the UI thread, which makes the [`Image`].
pub fn decode(path: &Path, max: (u32, u32)) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let size = std::fs::metadata(path).ok()?.len();
    if size == 0 || size > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    reader.limits(limits);
    let mut picture = reader.decode().ok()?;
    if picture.width() > max.0 || picture.height() > max.1 {
        picture = picture.thumbnail(max.0, max.1);
    }
    let rgba = picture.to_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use playtime_core::Timestamp;

    fn at(hour: u32) -> Timestamp {
        Timestamp::parse(&format!("2026-09-01T{hour:02}:00:00+00:00"))
            .unwrap_or_else(|_| Timestamp::now())
    }

    fn game(name: &str, total: i64, last: u32) -> GameView {
        GameView {
            name: name.into(),
            session_count: 1,
            total_seconds: total,
            average_seconds: total,
            longest_seconds: total,
            first_played: at(0),
            last_played: at(last),
            is_live: false,
        }
    }

    #[test]
    fn search_and_sort() {
        let games = vec![
            game("Hades", 100, 3),
            game("Elden Ring", 300, 1),
            game("hades II", 50, 2),
        ];
        let names = |list: Vec<&GameView>| list.iter().map(|g| g.name.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(visible(&games, "", 0)),
            ["Elden Ring", "Hades", "hades II"]
        );
        assert_eq!(
            names(visible(&games, "", 1)),
            ["Hades", "hades II", "Elden Ring"]
        );
        assert_eq!(
            names(visible(&games, "", 2)),
            ["Elden Ring", "Hades", "hades II"]
        );
        assert_eq!(names(visible(&games, " HADES ", 2)), ["Hades", "hades II"]);
        assert!(visible(&games, "portal", 0).is_empty());
    }

    #[test]
    fn pictures_are_scaled_down_and_bad_files_refused() {
        let dir = std::env::temp_dir().join(format!("pt-dash-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let png = dir.join("cover.png");
        let pixels = vec![200u8; 600 * 900 * 4];
        let encoded = playtime_artwork::png::encode_rgba(600, 900, &pixels).unwrap_or_default();
        assert!(std::fs::write(&png, encoded).is_ok());
        let scaled = decode(&png, COVER_SIZE);
        assert_eq!(scaled.map(|b| (b.width(), b.height())), Some((320, 480)));
        let junk = dir.join("junk.png");
        assert!(std::fs::write(&junk, b"not a picture").is_ok());
        assert!(decode(&junk, COVER_SIZE).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

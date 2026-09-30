//! Renders a page to a PNG without a screen or a tracker: `playtime-dashboard --render <page> <W>x<H> <out.png>
//! <snapshot> [--dark] [--amoled]`, where `<snapshot>` is a file whose first line is a `dashboard` response (the
//! protocol fixture has one). Used to check layouts at several sizes on any platform, including CI.

use crate::ui::{AppWindow, Palette};
use playtime_core::ipc::{DashboardSnapshot, Response};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, PhysicalSize, PlatformError};
use std::rc::Rc;

struct Offscreen {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for Offscreen {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
}

pub struct RenderArgs {
    pub page: i32,
    pub width: u32,
    pub height: u32,
    pub out: String,
    pub snapshot: String,
    pub dark: bool,
    /// `--amoled`: AMOLED mode (true black with `--dark`).
    pub amoled: bool,
    /// `--dialog session|day|game|name|choices`: open that dialog (the newest session, today, the most played game,
    /// naming a custom game, or the SteamGridDB picker with sample covers).
    pub dialog: Option<String>,
}

impl RenderArgs {
    /// Parses the arguments after `--render`.
    pub fn parse(args: &[String], pages: &[&str]) -> Result<Self, String> {
        let usage = "usage: --render <page> <width>x<height> <out.png> <snapshot file> [--dark] [--amoled] [--dialog session|day|game|name|choices]";
        let [page, size, out, snapshot, rest @ ..] = args else {
            return Err(usage.into());
        };
        let page = pages.iter().position(|p| p == page).ok_or(usage)? as i32;
        let (w, h) = size.split_once('x').ok_or(usage)?;
        Ok(Self {
            page,
            width: w.parse().map_err(|_| usage)?,
            height: h.parse().map_err(|_| usage)?,
            out: out.clone(),
            snapshot: snapshot.clone(),
            dark: rest.iter().any(|a| a == "--dark"),
            amoled: rest.iter().any(|a| a == "--amoled"),
            dialog: rest
                .iter()
                .position(|a| a == "--dialog")
                .and_then(|i| rest.get(i + 1))
                .cloned(),
        })
    }
}

fn read_snapshot(path: &str) -> Result<DashboardSnapshot, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let line = text.lines().next().unwrap_or_default();
    match serde_json::from_str(line).map_err(|e| format!("{path}: {e}"))? {
        Response::Dashboard { snapshot } => Ok(*snapshot),
        _ => Err(format!("{path}: the first line isn't a dashboard response")),
    }
}

/// Renders the page as it looks once the snapshot has arrived, and writes it as a PNG.
pub fn render(
    args: &RenderArgs,
    fill: impl FnOnce(&AppWindow, DashboardSnapshot),
) -> Result<(), String> {
    let screen = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    screen.set_size(PhysicalSize::new(args.width, args.height));
    slint::platform::set_platform(Box::new(Offscreen {
        window: screen.clone(),
    }))
    .map_err(|e| format!("{e:?}"))?;
    let window = AppWindow::new().map_err(|e| e.to_string())?;
    // The settings too, when the file has them (the fixture's second line).
    let text = std::fs::read_to_string(&args.snapshot).unwrap_or_default();
    for line in text.lines() {
        if let Ok(Response::Settings {
            settings,
            has_steam_grid_db_key,
            start_with_windows,
            is_installed_copy,
        }) = serde_json::from_str(line)
        {
            crate::APP.with_borrow_mut(|app| {
                app.settings.drawn_with_gpu = settings.hardware_acceleration;
                crate::settings::fill(
                    &window,
                    &mut app.settings,
                    *settings,
                    has_steam_grid_db_key,
                    start_with_windows,
                    is_installed_copy,
                )
            });
            let version = env!("CARGO_PKG_VERSION");
            window.global::<crate::ui::SettingsData>().set_about(
                format!(
                    "Playtime Tracker {version} (dashboard {version}). Your play history stays on this PC."
                )
                .into(),
            );
        }
    }
    if args.dark {
        window
            .global::<Palette>()
            .set_color_scheme(slint::language::ColorScheme::Dark);
    }
    window.global::<crate::ui::Theme>().set_amoled(args.amoled);
    window.set_page(args.page);
    fill(&window, read_snapshot(&args.snapshot)?);
    window.show().map_err(|e| e.to_string())?;
    slint::platform::update_timers_and_animations();

    let (w, h) = (args.width as usize, args.height as usize);
    let mut pixels = vec![PremultipliedRgbaColor::default(); w * h];
    screen.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, w);
    });
    let rgba: Vec<u8> = pixels
        .iter()
        .flat_map(|p| {
            // Un-premultiply (the window background is opaque, so alpha is 255 almost everywhere).
            let a = p.alpha.max(1) as u32;
            let c = |v: u8| ((v as u32 * 255) / a).min(255) as u8;
            [c(p.red), c(p.green), c(p.blue), p.alpha]
        })
        .collect();
    let png = playtime_artwork::png::encode_rgba(args.width, args.height, &rgba)
        .ok_or("PNG encoding failed")?;
    std::fs::write(&args.out, png).map_err(|e| format!("{}: {e}", args.out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_amoled() {
        let args: Vec<String> = [
            "overview", "800x600", "o.png", "s.jsonl", "--dark", "--amoled",
        ]
        .map(String::from)
        .to_vec();
        let parsed = RenderArgs::parse(&args, &["overview"]).expect("parses");
        assert!(parsed.dark && parsed.amoled);
    }
}

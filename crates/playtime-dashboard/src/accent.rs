//! The accent setting shared with the tracker: a preset name ("blue", "violet", …), a custom "#rrggbb", or
//! "windows" for Windows' own accent colour. Shades follow WinUI: a darker one on light surfaces, a lighter one on
//! dark surfaces, so accent text and fills stay readable in both.

pub const WINDOWS: &str = "windows";

/// The presets, with the same main colours as before.
pub const PRESETS: [(&str, &str, (u8, u8, u8)); 6] = [
    ("blue", "Blue", (0x3b, 0x82, 0xf6)),
    ("violet", "Violet", (0x8b, 0x5c, 0xf6)),
    ("teal", "Teal", (0x14, 0xb8, 0xa6)),
    ("green", "Green", (0x10, 0xb9, 0x81)),
    ("amber", "Amber", (0xf5, 0x9e, 0x0b)),
    ("rose", "Rose", (0xf4, 0x3f, 0x5e)),
];

pub type Rgb = (u8, u8, u8);

pub fn parse_hex(text: &str) -> Option<Rgb> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(((value >> 16) as u8, (value >> 8) as u8, value as u8))
}

pub fn to_hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The colour for a setting value; `None` means "use the Windows accent". Unreadable values are blue.
pub fn resolve(key: &str) -> Option<Rgb> {
    let value = key.trim().to_ascii_lowercase();
    if value == WINDOWS {
        return None;
    }
    PRESETS
        .iter()
        .find(|p| p.0 == value)
        .map(|p| p.2)
        .or_else(|| parse_hex(&value))
        .or(Some(PRESETS[0].2))
}

fn mix((r, g, b): Rgb, (tr, tg, tb): Rgb, amount: f32) -> Rgb {
    let channel =
        |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * amount).round() as u8;
    (channel(r, tr), channel(g, tg), channel(b, tb))
}

/// The accent as used on light surfaces (WinUI's "dark 1" shade) and on dark ones ("light 2").
pub fn shades(color: Rgb) -> (Rgb, Rgb) {
    (
        mix(color, (0, 0, 0), 0.2),
        mix(color, (255, 255, 255), 0.45),
    )
}

/// Which entry of the Settings list a setting value is: 0 Windows, 1–6 the presets, 7 custom.
pub fn choice_index(key: &str) -> usize {
    let value = key.trim().to_ascii_lowercase();
    if value == WINDOWS {
        return 0;
    }
    PRESETS
        .iter()
        .position(|p| p.0 == value)
        .map_or(PRESETS.len() + 1, |i| i + 1)
}

/// The names in the Settings list, in [`choice_index`] order.
pub fn choice_names() -> Vec<&'static str> {
    std::iter::once("Windows accent")
        .chain(PRESETS.iter().map(|p| p.1))
        .chain(std::iter::once("Custom…"))
        .collect()
}

/// Windows' accent colour (from the Personalisation settings), if it can be read.
#[cfg(windows)]
pub fn windows_accent() -> Option<Rgb> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: `value` and `size` outlive the call and `size` is the size of `value`.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\DWM"),
            w!("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    // Stored as 0xAABBGGRR.
    result
        .is_ok()
        .then_some((value as u8, (value >> 8) as u8, (value >> 16) as u8))
}

#[cfg(not(windows))]
pub fn windows_accent() -> Option<Rgb> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_values_resolve() {
        assert_eq!(resolve("rose"), Some((0xf4, 0x3f, 0x5e)));
        assert_eq!(resolve(" #FF8800 "), Some((0xff, 0x88, 0x00)));
        assert_eq!(resolve("windows"), None);
        assert_eq!(resolve("nonsense"), Some(PRESETS[0].2));
        assert_eq!(choice_index("windows"), 0);
        assert_eq!(choice_index("teal"), 3);
        assert_eq!(choice_index("#123456"), 7);
        assert_eq!(choice_names().len(), 8);
        assert_eq!(to_hex((0xff, 0x88, 0)), "#ff8800");
        assert!(parse_hex("#12345").is_none());
        let (light, dark) = shades((100, 100, 100));
        assert_eq!(light, (80, 80, 80));
        assert_eq!(dark, (170, 170, 170));
    }
}

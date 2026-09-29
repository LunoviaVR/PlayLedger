//! The game's own icon, extracted from its executable and saved as PNG: artwork for every game, offline.

use playtime_artwork::{
    png, ArtworkError, ArtworkKind, ArtworkProvider, ArtworkRequest, FetchedImage,
};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::UI::Shell::SHDefExtractIconW;
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

/// Size requested from the shell; it scales smaller icons up.
const ICON_SIZE: u32 = 256;

#[derive(Debug, Default, Clone, Copy)]
pub struct ExeIconProvider;

impl ArtworkProvider for ExeIconProvider {
    fn name(&self) -> &'static str {
        "exe-icon"
    }

    fn is_online(&self) -> bool {
        false
    }

    fn fetch(
        &self,
        request: &ArtworkRequest,
        kind: ArtworkKind,
    ) -> Result<Option<FetchedImage>, ArtworkError> {
        if kind != ArtworkKind::Icon {
            return Ok(None);
        }
        let Some(exe) = request.exe_path.as_deref() else {
            return Ok(None);
        };
        if !exe.to_ascii_lowercase().ends_with(".exe") || !std::path::Path::new(exe).is_file() {
            return Ok(None);
        }
        Ok(extract_png(exe).map(|bytes| FetchedImage {
            bytes,
            content_type: Some("image/png".into()),
        }))
    }
}

/// Deletes a GDI object on drop.
struct GdiObject(HGDIOBJ);

impl Drop for GdiObject {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: we own the object (from GetIconInfo) and delete it once.
            unsafe {
                let _ = DeleteObject(self.0);
            }
        }
    }
}

fn extract_png(exe: &str) -> Option<Vec<u8>> {
    let path = HSTRING::from(exe);
    let mut icon = HICON::default();
    // SAFETY: `path` outlives the call; only the large icon is requested, destroyed below.
    unsafe {
        SHDefExtractIconW(
            PCWSTR(path.as_ptr()),
            0,
            0,
            Some(&mut icon),
            None,
            ICON_SIZE,
        )
    }
    .ok()
    .ok()?;
    if icon.is_invalid() {
        return None;
    }
    let pixels = icon_rgba(icon);
    // SAFETY: we own the icon returned above.
    unsafe {
        let _ = DestroyIcon(icon);
    }
    let (width, height, rgba) = pixels?;
    png::encode_rgba(width, height, &rgba)
}

fn icon_rgba(icon: HICON) -> Option<(u32, u32, Vec<u8>)> {
    let mut info = ICONINFO::default();
    // SAFETY: valid icon; GetIconInfo creates two bitmaps we must delete (GdiObject does).
    unsafe { GetIconInfo(icon, &mut info) }.ok()?;
    let color = GdiObject(HGDIOBJ(info.hbmColor.0));
    let _mask = GdiObject(HGDIOBJ(info.hbmMask.0));
    if info.hbmColor.is_invalid() {
        return None; // monochrome icon; not worth showing
    }
    let mut bitmap = BITMAP::default();
    // SAFETY: `bitmap` is a BITMAP-sized buffer.
    let got = unsafe {
        GetObjectW(
            color.0,
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut bitmap as *mut BITMAP).cast()),
        )
    };
    if got == 0 {
        return None;
    }
    let (width, height) = (
        u32::try_from(bitmap.bmWidth).ok()?,
        u32::try_from(bitmap.bmHeight).ok()?,
    );
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return None;
    }
    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32), // top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0u8; width as usize * height as usize * 4];
    // SAFETY: a memory DC for GetDIBits, deleted below; `bgra` holds exactly width×height 32-bit pixels.
    let lines = unsafe {
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(
            dc,
            HBITMAP(color.0 .0),
            0,
            height,
            Some(bgra.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        lines
    };
    if lines != height as i32 {
        return None;
    }
    // Old icons have no alpha channel (all zero): treat them as opaque.
    let has_alpha = bgra.chunks_exact(4).any(|p| p[3] != 0);
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2); // BGRA → RGBA
        if !has_alpha {
            pixel[3] = 0xFF;
        }
    }
    Some((width, height, bgra))
}

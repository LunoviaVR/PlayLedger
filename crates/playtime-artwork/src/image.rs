//! Checks that downloaded or extracted bytes really are a reasonable image before they're cached or shown:
//! the format is sniffed from the bytes (never trusted from a file name or header), dimensions are read from the
//! image header, and size limits stop oversized or decompression-bomb-shaped files early.

/// Largest image file we accept.
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// Largest side we accept (Steam heroes are 3840 wide).
pub const MAX_SIDE: u32 = 8192;
/// Smallest side worth keeping (icons are at least 16×16).
pub const MIN_SIDE: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    WebP,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::WebP => "webp",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "webp" => Some(Self::WebP),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImageError {
    #[error("the file is empty")]
    Empty,
    #[error("the file is too large ({0} bytes)")]
    TooLarge(usize),
    #[error("not a PNG, JPEG or WebP image")]
    UnknownFormat,
    #[error("the image header is damaged")]
    BadHeader,
    #[error("unreasonable dimensions {0}×{1}")]
    BadDimensions(u32, u32),
    #[error("the source said {claimed} but sent {actual}")]
    ContentTypeMismatch {
        claimed: String,
        actual: &'static str,
    },
}

/// Validates `bytes` as an image; `claimed_type` is the Content-Type a server sent, if any.
pub fn validate(bytes: &[u8], claimed_type: Option<&str>) -> Result<ImageInfo, ImageError> {
    if bytes.is_empty() {
        return Err(ImageError::Empty);
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageError::TooLarge(bytes.len()));
    }
    let format = sniff(bytes).ok_or(ImageError::UnknownFormat)?;
    if let Some(claimed) = claimed_type {
        let claimed = claimed
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        // Servers sometimes send a generic type; anything that claims to be a *different* image or not an image
        // (HTML error pages, scripts) is refused.
        let generic = claimed.is_empty()
            || claimed == "application/octet-stream"
            || claimed == "binary/octet-stream";
        let same =
            claimed == format.mime() || (format == ImageFormat::Jpeg && claimed == "image/jpg");
        if !generic && !same {
            return Err(ImageError::ContentTypeMismatch {
                claimed,
                actual: format.mime(),
            });
        }
    }
    let (width, height) = match format {
        ImageFormat::Png => png_size(bytes),
        ImageFormat::Jpeg => jpeg_size(bytes),
        ImageFormat::WebP => webp_size(bytes),
    }
    .ok_or(ImageError::BadHeader)?;
    let ok = |side: u32| (MIN_SIDE..=MAX_SIDE).contains(&side);
    if !ok(width) || !ok(height) {
        return Err(ImageError::BadDimensions(width, height));
    }
    Ok(ImageInfo {
        format,
        width,
        height,
    })
}

pub fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::WebP)
    } else {
        None
    }
}

fn be_u16(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
    ])))
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le_u16(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
    ])))
}

fn le_u24(b: &[u8], at: usize) -> Option<u32> {
    Some(
        u32::from(*b.get(at)?) | u32::from(*b.get(at + 1)?) << 8 | u32::from(*b.get(at + 2)?) << 16,
    )
}

fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    // Signature, then the IHDR chunk: length(4) "IHDR"(4) width(4) height(4).
    if b.get(12..16)? != b"IHDR" {
        return None;
    }
    Some((be_u32(b, 16)?, be_u32(b, 20)?))
}

fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    // Walk the marker segments to the first start-of-frame; bounded by the data length.
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1; // fill byte
            continue;
        }
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2; // markers without a length
            continue;
        }
        let length = be_u16(b, i + 2)? as usize;
        if length < 2 {
            return None;
        }
        let is_sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            // length(2) precision(1) height(2) width(2)
            return Some((be_u16(b, i + 7)?, be_u16(b, i + 5)?));
        }
        if marker == 0xD9 || marker == 0xDA {
            return None; // end of image / start of scan before any frame header
        }
        i += 2 + length;
    }
    None
}

fn webp_size(b: &[u8]) -> Option<(u32, u32)> {
    match b.get(12..16)? {
        b"VP8X" => Some((le_u24(b, 24)? + 1, le_u24(b, 27)? + 1)),
        b"VP8L" => {
            if *b.get(20)? != 0x2F {
                return None;
            }
            let bits = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        b"VP8 " => {
            // Frame tag(3) then the start code 9d 01 2a, then 14-bit width and height.
            if b.get(23..26)? != [0x9D, 0x01, 0x2A] {
                return None;
            }
            Some((le_u16(b, 26)? & 0x3FFF, le_u16(b, 28)? & 0x3FFF))
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod samples {
    /// A minimal PNG header of the given size (enough for validation; not decodable).
    pub fn png(width: u32, height: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(width.to_be_bytes());
        b.extend(height.to_be_bytes());
        b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    pub fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x4A, 0x46];
        b.extend([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        b.extend(height.to_be_bytes());
        b.extend(width.to_be_bytes());
        b.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::samples::*;
    use super::*;

    #[test]
    fn accepts_real_headers() {
        assert_eq!(
            validate(&png(600, 900), Some("image/png")),
            Ok(ImageInfo {
                format: ImageFormat::Png,
                width: 600,
                height: 900
            })
        );
        assert_eq!(
            validate(&jpeg(460, 215), Some("image/jpeg; charset=binary")),
            Ok(ImageInfo {
                format: ImageFormat::Jpeg,
                width: 460,
                height: 215
            })
        );
        assert_eq!(
            validate(&jpeg(460, 215), Some("application/octet-stream")).map(|i| i.width),
            Ok(460)
        );

        let mut vp8x = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
        vp8x.extend([0x57, 0x02, 0x00, 0x83, 0x03, 0x00]); // 600×900 stored minus one
        assert_eq!(
            validate(&vp8x, Some("image/webp")).map(|i| (i.width, i.height)),
            Ok((600, 900))
        );
    }

    #[test]
    fn refuses_everything_else() {
        assert_eq!(validate(b"", None), Err(ImageError::Empty));
        assert_eq!(
            validate(b"<html>not found</html>", Some("text/html")),
            Err(ImageError::UnknownFormat)
        );
        assert!(matches!(
            validate(&png(64, 64), Some("text/html")),
            Err(ImageError::ContentTypeMismatch { .. })
        ));
        assert!(validate(&png(64, 64), Some("image/jpeg")).is_err());
        assert_eq!(
            validate(&png(100_000, 64), None),
            Err(ImageError::BadDimensions(100_000, 64))
        );
        assert_eq!(
            validate(&png(8, 8), None),
            Err(ImageError::BadDimensions(8, 8))
        );
        assert_eq!(
            validate(&png(64, 64)[..14], None),
            Err(ImageError::BadHeader)
        );
        assert_eq!(
            validate(&[0xFF, 0xD8, 0xFF, 0xD9], None),
            Err(ImageError::BadHeader)
        );
        let huge = vec![0u8; MAX_IMAGE_BYTES + 1];
        assert_eq!(
            validate(&huge, None),
            Err(ImageError::TooLarge(MAX_IMAGE_BYTES + 1))
        );
    }
}

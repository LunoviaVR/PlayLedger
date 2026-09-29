//! A minimal PNG encoder for RGBA pixels (used for icons extracted from game executables). Uses uncompressed
//! ("stored") deflate blocks: icons are at most 256×256, so files stay small, and no compression library is needed.

/// Encodes `rgba` (row-major, 4 bytes per pixel, top row first) as a PNG. `None` if the sizes don't agree.
pub fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let row = (width as usize).checked_mul(4)?;
    if width == 0 || height == 0 || rgba.len() != row.checked_mul(height as usize)? {
        return None;
    }
    // Each scanline is prefixed with filter type 0 (none).
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgba.chunks_exact(row) {
        raw.push(0);
        raw.extend_from_slice(line);
    }

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, adaptive filtering, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    Some(out)
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut blocks = data.chunks(65_535).peekable();
    if blocks.peek().is_none() {
        out.extend([1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        out.extend(len.to_le_bytes());
        out.extend((!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend(adler32(data).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= 65_521;
        b %= 65_521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{validate, ImageFormat};

    #[test]
    fn checksums() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn produces_a_valid_png() {
        let pixels = vec![0x80u8; 256 * 256 * 4];
        let png = encode_rgba(256, 256, &pixels).expect("encoded");
        let info = validate(&png, Some("image/png")).expect("valid");
        assert_eq!(
            (info.format, info.width, info.height),
            (ImageFormat::Png, 256, 256)
        );
        assert!(png.ends_with(&[0xAE, 0x42, 0x60, 0x82]));
        assert_eq!(encode_rgba(2, 2, &[0; 3]), None);
    }
}

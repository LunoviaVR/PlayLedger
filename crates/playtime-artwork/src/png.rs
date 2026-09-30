//! A minimal PNG encoder for RGBA pixels (icons extracted from game executables, and page screenshots). Rows use
//! the "up" filter and are deflate-compressed, which keeps flat UI areas small.

/// Encodes `rgba` (row-major, 4 bytes per pixel, top row first) as a PNG. `None` if the sizes don't agree.
pub fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let row = (width as usize).checked_mul(4)?;
    if width == 0 || height == 0 || rgba.len() != row.checked_mul(height as usize)? {
        return None;
    }
    // Each scanline is prefixed with filter type 2 ("up": the difference from the row above).
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    let mut above: &[u8] = &[];
    for line in rgba.chunks_exact(row) {
        raw.push(2);
        if above.is_empty() {
            raw.extend_from_slice(line);
        } else {
            raw.extend(line.iter().zip(above).map(|(v, up)| v.wrapping_sub(*up)));
        }
        above = line;
    }

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, adaptive filtering, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(
        &mut out,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{validate, ImageFormat};

    #[test]
    fn checksum() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
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
        assert!(png.len() < 10_000, "flat pixels compress well");
        assert_eq!(encode_rgba(2, 2, &[0; 3]), None);
    }

    #[test]
    fn pixels_round_trip() {
        let (w, h) = (37u32, 23u32);
        let pixels: Vec<u8> = (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect();
        let png = encode_rgba(w, h, &pixels).expect("encoded");
        // IHDR ends at 8 + 25; IDAT's length and type follow.
        let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&png[41..41 + idat_len])
            .expect("inflates");
        let row = w as usize * 4;
        let mut decoded: Vec<u8> = Vec::new();
        for line in raw.chunks_exact(row + 1) {
            assert_eq!(line[0], 2);
            let start = decoded.len();
            for (i, v) in line[1..].iter().enumerate() {
                let up = if start == 0 {
                    0
                } else {
                    decoded[start - row + i]
                };
                decoded.push(v.wrapping_add(up));
            }
        }
        assert_eq!(decoded, pixels);
    }
}

//! PNG encoding (8-bit RGB, or 8-bit palette indices), from the published
//! PNG specification.

use super::Image;
use crate::codec::{zlib, Level};
use crate::dwg::crc::crc32;

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(0, &out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = (
        (p - a as i16).abs(),
        (p - b as i16).abs(),
        (p - c as i16).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Filter one row (3 bytes per pixel) with each PNG filter and keep the
/// one with the smallest sum of magnitudes, the usual heuristic.
fn filter_row(row: &[u8], prior: &[u8], out: &mut Vec<u8>) {
    let bpp = 3;
    let mut best: Option<(u64, u8, Vec<u8>)> = None;
    for f in 0u8..5 {
        let mut v = Vec::with_capacity(row.len());
        for i in 0..row.len() {
            let a = if i >= bpp { row[i - bpp] } else { 0 };
            let b = prior[i];
            let c = if i >= bpp { prior[i - bpp] } else { 0 };
            let x = row[i];
            v.push(match f {
                0 => x,
                1 => x.wrapping_sub(a),
                2 => x.wrapping_sub(b),
                3 => x.wrapping_sub(((a as u16 + b as u16) / 2) as u8),
                _ => x.wrapping_sub(paeth(a, b, c)),
            });
        }
        let cost: u64 = v.iter().map(|&x| (x as i8).unsigned_abs() as u64).sum();
        if best.as_ref().is_none_or(|(c, _, _)| cost < *c) {
            best = Some((cost, f, v));
        }
    }
    if let Some((_, f, v)) = best {
        out.push(f);
        out.extend_from_slice(&v);
    }
}

/// Encode an image as PNG (RGB, the alpha channel dropped). PNG has no
/// empty images, so a zero width or height is written as one black pixel
/// across; pixels missing from a short `rgba` buffer are black too.
pub fn encode(img: &Image) -> Vec<u8> {
    let (src_w, src_h) = (img.width as usize, img.height as usize);
    let (w, h) = (src_w.max(1), src_h.max(1));
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    let mut prior = vec![0u8; w * 3];
    let mut row = vec![0u8; w * 3];
    for y in 0..h {
        for x in 0..w {
            let i = (y * src_w + x) * 4;
            let p = if x < src_w && y < src_h {
                img.rgba.get(i..i + 3)
            } else {
                None
            };
            row[x * 3..x * 3 + 3].copy_from_slice(p.unwrap_or(&[0, 0, 0]));
        }
        filter_row(&row, &prior, &mut raw);
        std::mem::swap(&mut prior, &mut row);
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, RGB, deflate, adaptive filters, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib(&raw, Level::Default));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Encode 8-bit palette indices as an indexed-colour PNG (colour type 3)
/// with the given palette (at most 256 entries). Rows are unfiltered, as
/// the specification recommends for palette images. `pixels` holds
/// `width * height` indices, row by row; missing ones are index 0.
pub fn encode_indexed(width: u32, height: u32, palette: &[(u8, u8, u8)], pixels: &[u8]) -> Vec<u8> {
    let (w, h) = (width.max(1) as usize, height.max(1) as usize);
    let mut raw = Vec::with_capacity((w + 1) * h);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let i = y * width as usize + x;
            let inside = x < width as usize && y < height as usize;
            raw.push(if inside { pixels.get(i).copied().unwrap_or(0) } else { 0 });
        }
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 3, 0, 0, 0]); // 8-bit, palette, deflate, no filtering, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    let plte: Vec<u8> = palette.iter().take(256).flat_map(|&(r, g, b)| [r, g, b]).collect();
    chunk(&mut out, b"PLTE", if plte.is_empty() { &[0, 0, 0] } else { &plte });
    chunk(&mut out, b"IDAT", &zlib(&raw, Level::Default));
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::unzlib;

    #[test]
    fn indexed_png_round_trips() {
        let (w, h) = (5u32, 3u32);
        let pal = [(1, 2, 3), (250, 251, 252), (9, 9, 9)];
        let px: Vec<u8> = (0..15).map(|i| (i % 3) as u8).collect();
        let png = encode_indexed(w, h, &pal, &px);
        let mut p = 8;
        let (mut idat, mut plte, mut ihdr) = (Vec::new(), Vec::new(), Vec::new());
        while p < png.len() {
            let len = u32::from_be_bytes(png[p..p + 4].try_into().unwrap()) as usize;
            let data = png[p + 8..p + 8 + len].to_vec();
            let crc = u32::from_be_bytes(png[p + 8 + len..p + 12 + len].try_into().unwrap());
            assert_eq!(crc, crc32(0, &png[p + 4..p + 8 + len]));
            match &png[p + 4..p + 8] {
                b"IHDR" => ihdr = data,
                b"PLTE" => plte = data,
                b"IDAT" => idat.extend(data),
                _ => {}
            }
            p += 12 + len;
        }
        assert_eq!(&ihdr[8..10], &[8, 3]);
        assert_eq!(plte, [1, 2, 3, 250, 251, 252, 9, 9, 9]);
        let raw = unzlib(&idat, 1 << 20).unwrap();
        for y in 0..h as usize {
            assert_eq!(raw[y * 6], 0);
            assert_eq!(&raw[y * 6 + 1..y * 6 + 6], &px[y * 5..y * 5 + 5]);
        }
    }

    /// Undo the filters of decoded scanlines.
    fn unfilter(data: &[u8], w: usize, h: usize) -> Vec<u8> {
        let stride = w * 3;
        let mut out = vec![0u8; stride * h];
        for y in 0..h {
            let f = data[y * (stride + 1)];
            for i in 0..stride {
                let x = data[y * (stride + 1) + 1 + i];
                let a = if i >= 3 { out[y * stride + i - 3] } else { 0 };
                let b = if y > 0 { out[(y - 1) * stride + i] } else { 0 };
                let c = if y > 0 && i >= 3 {
                    out[(y - 1) * stride + i - 3]
                } else {
                    0
                };
                out[y * stride + i] = match f {
                    0 => x,
                    1 => x.wrapping_add(a),
                    2 => x.wrapping_add(b),
                    3 => x.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                    _ => x.wrapping_add(paeth(a, b, c)),
                };
            }
        }
        out
    }

    #[test]
    fn png_structure_and_pixels_round_trip() {
        let (w, h) = (37u32, 23u32);
        let mut img = Image::new(w, h, (13, 16, 19));
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                img.rgba[i] = (x * 7) as u8;
                img.rgba[i + 1] = (y * 11) as u8;
                img.rgba[i + 2] = ((x * y) % 256) as u8;
            }
        }
        let png = encode(&img);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        // Walk the chunks, checking each CRC.
        let mut p = 8;
        let mut idat = Vec::new();
        let mut kinds = Vec::new();
        while p < png.len() {
            let len = u32::from_be_bytes(png[p..p + 4].try_into().unwrap()) as usize;
            let kind = &png[p + 4..p + 8];
            let data = &png[p + 8..p + 8 + len];
            let crc = u32::from_be_bytes(png[p + 8 + len..p + 12 + len].try_into().unwrap());
            assert_eq!(crc, crc32(0, &png[p + 4..p + 8 + len]));
            kinds.push(String::from_utf8_lossy(kind).to_string());
            if kind == b"IDAT" {
                idat.extend_from_slice(data);
            }
            p += 12 + len;
        }
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
        let raw = unzlib(&idat, 1 << 24).unwrap();
        let px = unfilter(&raw, w as usize, h as usize);
        for i in 0..(w * h) as usize {
            assert_eq!(&px[i * 3..i * 3 + 3], &img.rgba[i * 4..i * 4 + 3]);
        }
    }

    #[test]
    fn empty_and_short_images_still_make_valid_files() {
        for img in [
            Image::new(0, 0, (255, 255, 255)),
            Image::new(5, 0, (255, 255, 255)),
            Image {
                width: 3,
                height: 2,
                rgba: vec![9; 7],
            },
        ] {
            let png = encode(&img);
            let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
            let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
            assert!(w >= 1 && h >= 1);
            let len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
            let raw = unzlib(&png[41..41 + len], 1 << 20).unwrap();
            assert_eq!(raw.len(), (w as usize * 3 + 1) * h as usize);
        }
    }
}

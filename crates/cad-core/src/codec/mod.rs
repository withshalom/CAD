//! Compression shared by the native format and the PNG writer: DEFLATE
//! (RFC 1951) both ways, and the zlib wrapper (RFC 1950) PNG uses.
//!
//! The native format also reads and writes DWG's own LZ77 (`dwg::lz77`);
//! DEFLATE is its stronger codec from format 2.0 on.

pub mod deflate;
pub mod inflate;
pub mod sha256;

pub use deflate::{deflate, Level};
pub use inflate::{inflate, InflateError};
pub use sha256::{sha256, Sha256};

/// Adler-32 checksum (RFC 1950).
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// A zlib stream: header (deflate, 32 KiB window), DEFLATE data, Adler-32.
pub fn zlib(data: &[u8], level: Level) -> Vec<u8> {
    // CMF 0x78; FLG 0xDA (level "maximum", FCHECK so the pair is a
    // multiple of 31) or 0x01.
    let flg = match level {
        Level::Fast => 0x01,
        _ => 0xDA,
    };
    let mut out = vec![0x78, flg];
    out.extend(deflate(data, level));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Decode a zlib stream, producing at most `max_out` bytes.
pub fn unzlib(d: &[u8], max_out: usize) -> Result<Vec<u8>, InflateError> {
    if d.len() < 6
        || d[0] & 0x0F != 8
        || !u16::from_be_bytes([d[0], d[1]]).is_multiple_of(31)
        || d[1] & 0x20 != 0
    {
        return Err(InflateError("not a zlib stream".into()));
    }
    let (body, used) = inflate(&d[2..], max_out)?;
    let at = 2 + used;
    let want = d
        .get(at..at + 4)
        .ok_or_else(|| InflateError("no Adler-32 after the data".into()))?;
    if adler32(&body).to_be_bytes() != want {
        return Err(InflateError("Adler-32 does not match".into()));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zlib_round_trip_and_checks() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        for level in [Level::Fast, Level::Default] {
            let z = zlib(b"hello hello hello\n", level);
            assert!(u16::from_be_bytes([z[0], z[1]]).is_multiple_of(31));
            assert_eq!(unzlib(&z, 100).unwrap(), b"hello hello hello\n");
            let mut bad = z.clone();
            let n = bad.len();
            bad[n - 1] ^= 1;
            assert!(unzlib(&bad, 100).is_err());
        }
    }
}

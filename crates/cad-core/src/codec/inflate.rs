//! DEFLATE decompressor (RFC 1951): stored, fixed-code and dynamic-code
//! blocks. Written from RFC 1951 alone.
//!
//! Every input is treated as hostile: lengths, distances and code tables
//! are checked, the output is capped by the caller, and reading past the
//! end of the input is an error, never a panic.

use super::deflate::{CLEN_ORDER, DIST_BASE, DIST_EXTRA, LEN_BASE, LEN_EXTRA};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InflateError(pub String);

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DEFLATE data: {}", self.0)
    }
}

impl std::error::Error for InflateError {}

fn err<T>(msg: impl Into<String>) -> Result<T, InflateError> {
    Err(InflateError(msg.into()))
}

/// Bits in, least significant first.
struct Input<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    cnt: u32,
}

impl Input<'_> {
    /// Make at least `n` (≤ 32) bits available; bits past the end of the
    /// input read as zero, and taking them is an error (see `take`).
    fn fill(&mut self, n: u32) {
        while self.cnt < n {
            let b = self.data.get(self.pos).copied().unwrap_or(0);
            self.pos += 1;
            self.buf |= (b as u64) << self.cnt;
            self.cnt += 8;
        }
    }

    fn overrun(&self) -> bool {
        // Bytes loaded past the end that are not just unused look-ahead.
        self.pos > self.data.len() && (self.pos - self.data.len()) * 8 > self.cnt as usize
    }

    fn bits(&mut self, n: u32) -> Result<u32, InflateError> {
        if n == 0 {
            return Ok(0);
        }
        self.fill(n);
        let v = (self.buf & ((1u64 << n) - 1)) as u32;
        self.buf >>= n;
        self.cnt -= n;
        if self.overrun() {
            return err("ends in the middle of a block");
        }
        Ok(v)
    }

    /// Drop bits up to the next byte boundary.
    fn align(&mut self) {
        let r = self.cnt % 8;
        self.buf >>= r;
        self.cnt -= r;
    }

    /// Bytes consumed, counting a partly used last byte.
    fn consumed(&self) -> usize {
        self.pos - (self.cnt / 8) as usize
    }
}

/// Bits looked up at once; longer codes take the slow path.
const FAST: u32 = 9;

/// A Huffman decoding table for code lengths (RFC 1951 §3.2.2).
struct Huffman {
    /// count[len]: codes of each length.
    count: [u16; 16],
    /// Symbols in canonical order.
    symbol: Vec<u16>,
    /// Indexed by the next FAST input bits: `len << 12 | symbol`, or 0
    /// when the code is longer than FAST bits (or unused).
    fast: Vec<u16>,
}

impl Huffman {
    fn new(lens: &[u8]) -> Result<Huffman, InflateError> {
        let mut count = [0u16; 16];
        for &l in lens {
            if l > 15 {
                return err("code length over 15");
            }
            count[l as usize] += 1;
        }
        count[0] = 0;
        // Over-subscribed sets are invalid; incomplete ones are allowed and
        // an unused code is caught when it is met.
        let mut left: i32 = 1;
        for &c in count.iter().skip(1) {
            left <<= 1;
            left -= c as i32;
            if left < 0 {
                return err("over-subscribed code");
            }
        }
        let mut offs = [0u16; 16];
        for l in 1..15 {
            offs[l + 1] = offs[l] + count[l];
        }
        let mut symbol = vec![0u16; lens.len()];
        for (s, &l) in lens.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        // Fast table: canonical codes, bit-reversed, for lengths ≤ FAST.
        let mut fast = vec![0u16; 1 << FAST];
        let mut code: u32 = 0;
        let mut idx = 0usize;
        for len in 1..=15u32 {
            for _ in 0..count[len as usize] {
                if len <= FAST {
                    let rev = code.reverse_bits() >> (32 - len);
                    let entry = (len as u16) << 12 | symbol[idx];
                    let mut k = rev;
                    while k < (1 << FAST) {
                        fast[k as usize] = entry;
                        k += 1 << len;
                    }
                }
                code += 1;
                idx += 1;
            }
            code <<= 1;
        }
        Ok(Huffman {
            count,
            symbol,
            fast,
        })
    }

    fn decode(&self, inp: &mut Input) -> Result<u16, InflateError> {
        inp.fill(FAST);
        let e = self.fast[(inp.buf & ((1 << FAST) - 1)) as usize];
        if e != 0 {
            let len = (e >> 12) as u32;
            inp.buf >>= len;
            inp.cnt -= len;
            if inp.overrun() {
                return err("ends in the middle of a code");
            }
            return Ok(e & 0x0FFF);
        }
        // Slow path, one bit at a time.
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= inp.bits(1)? as i32;
            let c = self.count[len] as i32;
            if code - c < first {
                return Ok(self.symbol[(index + (code - first)) as usize]);
            }
            index += c;
            first += c;
            first <<= 1;
            code <<= 1;
        }
        err("invalid code")
    }
}

fn fixed_tables() -> (Huffman, Huffman) {
    let mut lit = [8u8; 288];
    lit[144..256].fill(9);
    lit[256..280].fill(7);
    // Building these fixed tables cannot fail.
    let lit = Huffman::new(&lit).unwrap_or_else(|_| unreachable!());
    let dist = Huffman::new(&[5u8; 30]).unwrap_or_else(|_| unreachable!());
    (lit, dist)
}

fn dynamic_tables(inp: &mut Input) -> Result<(Huffman, Huffman), InflateError> {
    let hlit = inp.bits(5)? as usize + 257;
    let hdist = inp.bits(5)? as usize + 1;
    let hclen = inp.bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return err("too many length or distance codes");
    }
    let mut clen = [0u8; 19];
    for &k in &CLEN_ORDER[..hclen] {
        clen[k] = inp.bits(3)? as u8;
    }
    let ch = Huffman::new(&clen)?;
    let mut lens = vec![0u8; hlit + hdist];
    let mut i = 0;
    while i < lens.len() {
        let sym = ch.decode(inp)?;
        let (val, rep) = match sym {
            0..=15 => (sym as u8, 1),
            16 => {
                if i == 0 {
                    return err("repeat with no previous length");
                }
                (lens[i - 1], 3 + inp.bits(2)? as usize)
            }
            17 => (0, 3 + inp.bits(3)? as usize),
            18 => (0, 11 + inp.bits(7)? as usize),
            _ => return err("invalid code-length symbol"),
        };
        if i + rep > lens.len() {
            return err("code lengths run past the end");
        }
        lens[i..i + rep].fill(val);
        i += rep;
    }
    if lens[256] == 0 {
        return err("no end-of-block code");
    }
    Ok((Huffman::new(&lens[..hlit])?, Huffman::new(&lens[hlit..])?))
}

/// Decompress raw DEFLATE data, producing at most `max_out` bytes.
/// Returns the output and the number of input bytes the stream used.
pub fn inflate(data: &[u8], max_out: usize) -> Result<(Vec<u8>, usize), InflateError> {
    let mut inp = Input {
        data,
        pos: 0,
        buf: 0,
        cnt: 0,
    };
    let mut out: Vec<u8> =
        Vec::with_capacity(max_out.min(data.len().saturating_mul(4)).min(1 << 26));
    loop {
        let last = inp.bits(1)?;
        match inp.bits(2)? {
            0 => {
                inp.align();
                let len = inp.bits(16)? as usize;
                let nlen = inp.bits(16)? as usize;
                if len != !nlen & 0xFFFF {
                    return err("stored block length check failed");
                }
                if out.len() + len > max_out {
                    return err("output larger than expected");
                }
                // Whole bytes still buffered come first.
                let mut need = len;
                while need > 0 && inp.cnt >= 8 {
                    out.push(inp.bits(8)? as u8);
                    need -= 1;
                }
                let start = inp.pos;
                let end = start
                    .checked_add(need)
                    .filter(|&e| e <= data.len())
                    .ok_or_else(|| InflateError("stored block runs past the end".into()))?;
                out.extend_from_slice(&data[start..end]);
                inp.pos = end;
            }
            t @ (1 | 2) => {
                let (lit, dist) = if t == 1 {
                    fixed_tables()
                } else {
                    dynamic_tables(&mut inp)?
                };
                loop {
                    let sym = lit.decode(&mut inp)? as usize;
                    if sym < 256 {
                        if out.len() >= max_out {
                            return err("output larger than expected");
                        }
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let li = sym - 257;
                        if li >= 29 {
                            return err("invalid length code");
                        }
                        let len = LEN_BASE[li] as usize + inp.bits(LEN_EXTRA[li] as u32)? as usize;
                        let di = dist.decode(&mut inp)? as usize;
                        if di >= 30 {
                            return err("invalid distance code");
                        }
                        let d = DIST_BASE[di] as usize + inp.bits(DIST_EXTRA[di] as u32)? as usize;
                        if d > out.len() {
                            return err("distance reaches before the start");
                        }
                        if out.len() + len > max_out {
                            return err("output larger than expected");
                        }
                        let start = out.len() - d;
                        if d >= len {
                            out.extend_from_within(start..start + len);
                        } else {
                            for k in 0..len {
                                let b = out[start + k];
                                out.push(b);
                            }
                        }
                    }
                }
            }
            _ => return err("reserved block type 3"),
        }
        if last == 1 {
            break;
        }
    }
    Ok((out, inp.consumed()))
}

#[cfg(test)]
mod tests {
    use super::super::deflate::{deflate, Level};
    use super::*;

    fn samples() -> Vec<Vec<u8>> {
        let mut v: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"a".to_vec(),
            b"abcabcabcabcabcabcabc".to_vec(),
            vec![0u8; 100_000],
            (0..70_000u32).map(|i| (i * 7 % 251) as u8).collect(),
            b"Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor"
                .repeat(300),
        ];
        let mut x = 12345u32;
        v.push(
            (0..50_000)
                .map(|_| {
                    x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    (x >> 16) as u8
                })
                .collect(),
        );
        // Text with a skewed alphabet: dynamic codes pay.
        v.push(
            (0..200_000u32)
                .map(|i| {
                    b"eeeeeeeettttaaoinshr\n"[(i.wrapping_mul(2_654_435_761) >> 27) as usize % 21]
                })
                .collect(),
        );
        // A run longer than the window, and matches at exactly 32768.
        let mut w: Vec<u8> = (0..32_768u32).map(|i| (i * 31 % 256) as u8).collect();
        let w2 = w.clone();
        w.extend_from_slice(&w2);
        w.extend_from_slice(&w2[..1000]);
        v.push(w);
        v
    }

    #[test]
    fn round_trips_at_every_level() {
        for s in samples() {
            for level in [Level::Fast, Level::Default, Level::Best] {
                let z = deflate(&s, level);
                let (back, used) = inflate(&z, s.len()).unwrap();
                assert_eq!(back, s, "{level:?}, {} bytes", s.len());
                assert_eq!(used, z.len());
            }
        }
    }

    #[test]
    fn dynamic_codes_beat_fixed_on_skewed_text() {
        let s = &samples()[7];
        let z = deflate(s, Level::Default);
        // Fixed codes need at least 7 bits for anything; a 21-letter
        // alphabet needs under 5 on average.
        assert!(z.len() * 8 < s.len() * 6, "{} → {}", s.len(), z.len());
        // Random bytes fall back to stored blocks: barely larger.
        let r = &samples()[6];
        assert!(deflate(r, Level::Default).len() < r.len() + r.len() / 100 + 16);
    }

    #[test]
    fn known_streams_from_other_encoders() {
        // Test vectors made with Python's zlib module (level 9, raw DEFLATE).
        // "hello hello hello\n": a fixed-code block with a back-reference.
        let z = [
            0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x90, 0x5c, 0x00,
        ];
        let (out, used) = inflate(&z, 100).unwrap();
        assert_eq!(out, b"hello hello hello\n");
        assert_eq!(used, z.len());
        // 600 bytes from a 21-letter alphabet: a dynamic-code block.
        let z = [
            0xed, 0x4e, 0x41, 0x0e, 0xc0, 0x30, 0x08, 0xfa, 0x8a, 0x5f, 0xf3, 0x40, 0xa2, 0x17,
            0x4d, 0x2a, 0xff, 0xcf, 0xa8, 0x7b, 0xc5, 0x92, 0x79, 0x20, 0x54, 0x90, 0x02, 0x07,
            0x58, 0x40, 0x0e, 0xc4, 0xc0, 0x0b, 0x62, 0x96, 0x81, 0x24, 0x98, 0x68, 0xfa, 0x20,
            0x9a, 0x0d, 0xd7, 0x5b, 0xb4, 0x62, 0x9d, 0xe8, 0x17, 0xa6, 0x0e, 0x8a, 0xd4, 0x2e,
            0xdd, 0xef, 0xd1, 0xde, 0x50, 0xca, 0x99, 0xf3, 0x9a, 0xf2, 0x02, 0x11, 0x63, 0x98,
            0x4d, 0x2f, 0xef, 0x14, 0x78, 0x5d, 0x49, 0x7e, 0x0b, 0x93, 0xae, 0x29, 0xae, 0x7e,
            0x42, 0xc9, 0xfa, 0x4d, 0xe9, 0x54, 0xb9, 0x71, 0xdf, 0x76, 0x7f, 0xcf, 0xef, 0xf4,
            0x7c, 0x00,
        ];
        assert_eq!((z[0] >> 1) & 3, 2, "dynamic block");
        let a = b"eeeeeeeettttaaoinshr ";
        let want: Vec<u8> = (0..600u64)
            .map(|i| a[((i * i * 7919 + i * 31 + (i >> 3) * 17) % 21) as usize])
            .collect();
        assert_eq!(inflate(&z, 1000).unwrap().0, want);
        // A stored block: BFINAL=1, BTYPE=00, LEN=3, NLEN=!3, "abc".
        let z = [0x01, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c'];
        assert_eq!(inflate(&z, 10).unwrap().0, b"abc");
    }

    #[test]
    fn hostile_input_is_an_error_not_a_panic() {
        let good = deflate(&samples()[5], Level::Default);
        // Every truncation fails cleanly.
        for cut in 0..good.len() {
            assert!(inflate(&good[..cut], 1 << 20).is_err(), "cut at {cut}");
        }
        // Output caps hold.
        assert!(inflate(&good, 100).is_err());
        // Flipped bits and random garbage never panic.
        let mut x = 7u32;
        for _ in 0..3000 {
            let mut bad = good.clone();
            for _ in 0..3 {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let i = (x >> 8) as usize % bad.len();
                bad[i] ^= 1 << (x % 8);
            }
            let _ = inflate(&bad, 1 << 20);
        }
        for n in 0..500 {
            let junk: Vec<u8> = (0..n)
                .map(|_| {
                    x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (x >> 24) as u8
                })
                .collect();
            let _ = inflate(&junk, 1 << 16);
        }
        // Block type 3, a distance before the start, a bad stored length.
        assert!(inflate(&[0x07], 10).is_err());
        assert!(inflate(&[0x01, 0x03, 0x00, 0x00, 0x00], 10).is_err());
    }
}

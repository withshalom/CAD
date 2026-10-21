//! DEFLATE compressor (RFC 1951): LZ77 with hash chains and lazy matching,
//! then per block whichever of stored, fixed-code or dynamic-code
//! (Huffman codes built for the block) is smallest.
//!
//! Written from RFC 1951 alone. The output is checked in the tests by this
//! crate's own decoder (`codec::inflate`).

/// How hard the compressor looks for matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Short hash chains, no lazy matching: quick saves.
    Fast,
    /// Longer chains and lazy matching: the default for files.
    Default,
    /// Long chains: archiving.
    Best,
}

impl Level {
    /// (chain length, lazy-match threshold, "good enough" match length)
    fn params(self) -> (usize, usize, usize) {
        match self {
            Level::Fast => (8, 0, 32),
            Level::Default => (64, 32, 128),
            Level::Best => (1024, 258, 258),
        }
    }
}

// ---------------------------------------------------------------------------
// Bit output, least significant bit first (RFC 1951 §3.1.1)
// ---------------------------------------------------------------------------

struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, value: u32, count: u32) {
        debug_assert!(count <= 32 && (count == 32 || value >> count == 0));
        self.acc |= (value as u64) << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// Pad to a byte boundary with zero bits.
    fn align(&mut self) {
        if self.n > 0 {
            self.out.push(self.acc as u8);
            self.acc = 0;
            self.n = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Length and distance codes (RFC 1951 §3.2.5)
// ---------------------------------------------------------------------------

pub(crate) const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
pub(crate) const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
pub(crate) const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
pub(crate) const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// Order in which code-length code lengths are sent (§3.2.7).
pub(crate) const CLEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Length code index (0–28) for a match length 3–258.
fn len_index(len: usize) -> usize {
    // 258 has its own code; otherwise the last base not above len.
    if len == 258 {
        28
    } else {
        LEN_BASE[..28].partition_point(|&b| b as usize <= len) - 1
    }
}

/// Distance code (0–29) for a distance 1–32768.
fn dist_index(dist: usize) -> usize {
    DIST_BASE.partition_point(|&b| b as usize <= dist) - 1
}

// ---------------------------------------------------------------------------
// LZ77
// ---------------------------------------------------------------------------

const WINDOW: usize = 32 * 1024;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const HASH_BITS: u32 = 15;
const NONE: u32 = u32::MAX;

/// A literal byte, or a match packed as `MATCH | (len - 3) << 15 | (dist - 1)`.
type Token = u32;
const MATCH: u32 = 1 << 31;

fn hash3(d: &[u8], i: usize) -> usize {
    let v = (d[i] as u32) | (d[i + 1] as u32) << 8 | (d[i + 2] as u32) << 16;
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

struct Matcher<'a> {
    data: &'a [u8],
    head: Vec<u32>,
    prev: Vec<u32>,
    chain: usize,
    nice: usize,
}

impl Matcher<'_> {
    fn insert(&mut self, at: usize) {
        if at + MIN_MATCH <= self.data.len() {
            let h = hash3(self.data, at);
            self.prev[at % WINDOW] = self.head[h];
            self.head[h] = at as u32;
        }
    }

    /// Longest match for position `i` that beats `min_len`: (length, distance).
    fn longest(&self, i: usize, min_len: usize) -> (usize, usize) {
        let d = self.data;
        let n = d.len();
        if i + MIN_MATCH > n {
            return (0, 0);
        }
        let max = (n - i).min(MAX_MATCH);
        let mut best = (min_len.max(MIN_MATCH - 1), 0usize);
        if best.0 >= max {
            return (0, 0);
        }
        let mut cand = self.head[hash3(d, i)];
        let mut chain = self.chain;
        while cand != NONE && chain > 0 {
            let c = cand as usize;
            if c >= i || i - c > WINDOW {
                break;
            }
            // Cheap reject: the byte that would extend the best match.
            if d[c + best.0] == d[i + best.0] && d[c] == d[i] {
                let mut l = 0;
                while l < max && d[c + l] == d[i + l] {
                    l += 1;
                }
                if l > best.0 {
                    best = (l, i - c);
                    if l >= self.nice || l == max {
                        break;
                    }
                }
            }
            let next = self.prev[c % WINDOW];
            if next == NONE || next as usize >= c {
                break;
            }
            cand = next;
            chain -= 1;
        }
        if best.1 == 0 {
            (0, 0)
        } else {
            best
        }
    }
}

fn tokenize(data: &[u8], level: Level) -> Vec<Token> {
    let (chain, lazy, nice) = level.params();
    let mut m = Matcher {
        data,
        head: vec![NONE; 1 << HASH_BITS],
        prev: vec![NONE; WINDOW],
        chain,
        nice,
    };
    let mut out = Vec::with_capacity(data.len() / 3 + 16);
    let n = data.len();
    let mut i = 0;
    let push_match = |out: &mut Vec<Token>, len: usize, dist: usize| {
        out.push(MATCH | ((len - MIN_MATCH) as u32) << 15 | (dist - 1) as u32);
    };
    while i < n {
        let (len, dist) = m.longest(i, 0);
        if len < MIN_MATCH {
            out.push(data[i] as u32);
            m.insert(i);
            i += 1;
            continue;
        }
        // Lazy matching: a longer match one byte on wins over this one.
        if len < lazy && i + 1 < n {
            m.insert(i);
            let (len2, dist2) = m.longest(i + 1, len);
            if len2 > len {
                out.push(data[i] as u32);
                i += 1;
                push_match(&mut out, len2, dist2);
                for k in 0..len2 {
                    m.insert(i + k);
                }
                i += len2;
                continue;
            }
            push_match(&mut out, len, dist);
            for k in 1..len {
                m.insert(i + k);
            }
            i += len;
            continue;
        }
        push_match(&mut out, len, dist);
        for k in 0..len {
            m.insert(i + k);
        }
        i += len;
    }
    out
}

// ---------------------------------------------------------------------------
// Huffman codes
// ---------------------------------------------------------------------------

/// Code lengths for `freq`, no longer than `limit` bits. Symbols with zero
/// frequency get length 0. At least two symbols get a code, so the code is
/// always complete.
fn code_lengths(freq: &[u32], limit: u32) -> Vec<u8> {
    let mut f: Vec<u32> = freq.to_vec();
    let used = f.iter().filter(|&&x| x > 0).count();
    if used < 2 {
        // Give two symbols a one-bit code each.
        for s in 0..f.len() {
            if f.iter().filter(|&&x| x > 0).count() >= 2 {
                break;
            }
            if f[s] == 0 {
                f[s] = 1;
            }
        }
    }
    // Huffman's algorithm on (weight, node); leaves are 0..n, inner nodes
    // after. Two sorted queues avoid a heap.
    let mut leaves: Vec<(u64, usize)> = f
        .iter()
        .enumerate()
        .filter(|&(_, &x)| x > 0)
        .map(|(s, &x)| (x as u64, s))
        .collect();
    leaves.sort();
    let nl = leaves.len();
    let mut parent = vec![0usize; 2 * nl];
    let mut inner: Vec<(u64, usize)> = Vec::with_capacity(nl);
    let (mut li, mut ii) = (0, 0);
    let take = |leaves: &[(u64, usize)], inner: &[(u64, usize)], li: &mut usize, ii: &mut usize| {
        if *li < leaves.len() && (*ii >= inner.len() || leaves[*li].0 <= inner[*ii].0) {
            *li += 1;
            (leaves[*li - 1].0, *li - 1)
        } else {
            *ii += 1;
            (inner[*ii - 1].0, nl + *ii - 1)
        }
    };
    for _ in 0..nl - 1 {
        let a = take(&leaves, &inner, &mut li, &mut ii);
        let b = take(&leaves, &inner, &mut li, &mut ii);
        let id = nl + inner.len();
        parent[a.1] = id;
        parent[b.1] = id;
        inner.push((a.0 + b.0, id));
    }
    // Depths: the root is the last inner node.
    let root = 2 * nl - 2;
    let mut depth = vec![0u32; 2 * nl - 1];
    for node in (0..root).rev() {
        depth[node] = depth[parent[node]] + 1;
    }
    let mut lens = vec![0u8; f.len()];
    // Count codes per length, capped at the limit, then fix the Kraft sum
    // by lengthening the deepest codes that are still under the limit.
    let mut count = vec![0u32; 64];
    for (k, &(_, s)) in leaves.iter().enumerate() {
        let _ = s;
        count[depth[k].min(limit) as usize] += 1;
    }
    let full = 1u64 << limit;
    let mut kraft: u64 = (1..=limit as usize)
        .map(|l| count[l] as u64 * (1u64 << (limit as usize - l)))
        .sum();
    // Each step turns a leaf at depth l (below the limit) into a node whose
    // two children are that leaf and one leaf from the limit depth: the sum
    // drops by exactly one unit, so the code ends complete.
    let lim = limit as usize;
    while kraft > full {
        let mut l = lim - 1;
        while l > 1 && count[l] == 0 {
            l -= 1;
        }
        if count[l] == 0 || count[lim] == 0 {
            break; // cannot happen for a tree that was complete
        }
        count[l] -= 1;
        count[l + 1] += 2;
        count[lim] -= 1;
        kraft -= 1;
    }
    // Shortest lengths to the most frequent symbols.
    let mut by_freq: Vec<usize> = leaves.iter().map(|&(_, s)| s).collect();
    by_freq.sort_by(|&a, &b| f[b].cmp(&f[a]).then(a.cmp(&b)));
    let mut it = by_freq.into_iter();
    for (l, &n) in count.iter().enumerate().take(limit as usize + 1).skip(1) {
        for _ in 0..n {
            if let Some(s) = it.next() {
                lens[s] = l as u8;
            }
        }
    }
    lens
}

/// Canonical codes for code lengths (RFC 1951 §3.2.2), bit-reversed so they
/// can be written least significant bit first.
fn canonical(lens: &[u8]) -> Vec<u32> {
    let mut bl_count = [0u32; 16];
    for &l in lens {
        bl_count[l as usize] += 1;
    }
    bl_count[0] = 0;
    let mut next = [0u32; 16];
    let mut code = 0u32;
    for bits in 1..16 {
        code = (code + bl_count[bits - 1]) << 1;
        next[bits] = code;
    }
    lens.iter()
        .map(|&l| {
            if l == 0 {
                return 0;
            }
            let c = next[l as usize];
            next[l as usize] += 1;
            c.reverse_bits() >> (32 - l as u32)
        })
        .collect()
}

fn fixed_lens() -> (Vec<u8>, Vec<u8>) {
    let mut lit = vec![8u8; 288];
    lit[144..256].fill(9);
    lit[256..280].fill(7);
    (lit, vec![5u8; 30])
}

// ---------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------

/// Tokens per block: small enough to adapt codes to changing data.
const BLOCK_TOKENS: usize = 1 << 15;

struct Block<'a> {
    tokens: &'a [Token],
    /// The input bytes the tokens cover (for a stored block).
    raw: &'a [u8],
}

fn freqs(tokens: &[Token]) -> (Vec<u32>, Vec<u32>) {
    let mut lit = vec![0u32; 286];
    let mut dist = vec![0u32; 30];
    for &t in tokens {
        if t & MATCH != 0 {
            let len = ((t >> 15) & 0xFF) as usize + MIN_MATCH;
            let d = (t & 0x7FFF) as usize + 1;
            lit[257 + len_index(len)] += 1;
            dist[dist_index(d)] += 1;
        } else {
            lit[t as usize] += 1;
        }
    }
    lit[256] += 1;
    (lit, dist)
}

/// Bits the tokens take with these code lengths (data only, no header).
fn data_bits(lit_f: &[u32], dist_f: &[u32], lit_l: &[u8], dist_l: &[u8]) -> u64 {
    let mut bits = 0u64;
    for (s, &f) in lit_f.iter().enumerate() {
        if f > 0 {
            let extra = if s >= 257 {
                LEN_EXTRA[s - 257] as u64
            } else {
                0
            };
            bits += f as u64 * (lit_l[s] as u64 + extra);
        }
    }
    for (s, &f) in dist_f.iter().enumerate() {
        if f > 0 {
            bits += f as u64 * (dist_l[s] as u64 + DIST_EXTRA[s] as u64);
        }
    }
    bits
}

/// The code-length sequence run-length coded with symbols 16, 17 and 18:
/// (symbol, extra bits value).
fn rle_lengths(lens: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lens.len() {
        let l = lens[i];
        let mut run = 1;
        while i + run < lens.len() && lens[i + run] == l {
            run += 1;
        }
        i += run;
        if l == 0 {
            let mut r = run;
            while r >= 11 {
                let k = r.min(138);
                out.push((18, (k - 11) as u8));
                r -= k;
            }
            if r >= 3 {
                out.push((17, (r - 3) as u8));
                r = 0;
            }
            for _ in 0..r {
                out.push((0, 0));
            }
        } else {
            out.push((l, 0));
            let mut r = run - 1;
            while r >= 3 {
                let k = r.min(6);
                out.push((16, (k - 3) as u8));
                r -= k;
            }
            for _ in 0..r {
                out.push((l, 0));
            }
        }
    }
    out
}

struct Dynamic {
    lit_l: Vec<u8>,
    dist_l: Vec<u8>,
    hlit: usize,
    hdist: usize,
    rle: Vec<(u8, u8)>,
    clen_l: Vec<u8>,
    hclen: usize,
}

impl Dynamic {
    fn new(lit_f: &[u32], dist_f: &[u32]) -> Dynamic {
        let lit_l = code_lengths(lit_f, 15);
        let dist_l = code_lengths(dist_f, 15);
        let hlit = (257..=286)
            .rev()
            .find(|&n| lit_l[n - 1] != 0)
            .unwrap_or(257);
        let hdist = (1..=30).rev().find(|&n| dist_l[n - 1] != 0).unwrap_or(1);
        let mut all = lit_l[..hlit].to_vec();
        all.extend_from_slice(&dist_l[..hdist]);
        let rle = rle_lengths(&all);
        let mut cf = vec![0u32; 19];
        for &(s, _) in &rle {
            cf[s as usize] += 1;
        }
        let clen_l = code_lengths(&cf, 7);
        let hclen = (4..=19)
            .rev()
            .find(|&n| clen_l[CLEN_ORDER[n - 1]] != 0)
            .unwrap_or(4);
        Dynamic {
            lit_l,
            dist_l,
            hlit,
            hdist,
            rle,
            clen_l,
            hclen,
        }
    }

    fn header_bits(&self) -> u64 {
        let mut bits = 5 + 5 + 4 + 3 * self.hclen as u64;
        for &(s, _) in &self.rle {
            bits += self.clen_l[s as usize] as u64
                + match s {
                    16 => 2,
                    17 => 3,
                    18 => 7,
                    _ => 0,
                };
        }
        bits
    }

    fn write_header(&self, b: &mut Bits) {
        b.put((self.hlit - 257) as u32, 5);
        b.put((self.hdist - 1) as u32, 5);
        b.put((self.hclen - 4) as u32, 4);
        for &k in &CLEN_ORDER[..self.hclen] {
            b.put(self.clen_l[k] as u32, 3);
        }
        let codes = canonical(&self.clen_l);
        for &(s, x) in &self.rle {
            b.put(codes[s as usize], self.clen_l[s as usize] as u32);
            match s {
                16 => b.put(x as u32, 2),
                17 => b.put(x as u32, 3),
                18 => b.put(x as u32, 7),
                _ => {}
            }
        }
    }
}

fn write_tokens(b: &mut Bits, tokens: &[Token], lit_l: &[u8], dist_l: &[u8]) {
    let lit_c = canonical(lit_l);
    let dist_c = canonical(dist_l);
    for &t in tokens {
        if t & MATCH != 0 {
            let len = ((t >> 15) & 0xFF) as usize + MIN_MATCH;
            let d = (t & 0x7FFF) as usize + 1;
            let li = len_index(len);
            b.put(lit_c[257 + li], lit_l[257 + li] as u32);
            if LEN_EXTRA[li] > 0 {
                b.put((len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
            }
            let di = dist_index(d);
            b.put(dist_c[di], dist_l[di] as u32);
            if DIST_EXTRA[di] > 0 {
                b.put((d - DIST_BASE[di] as usize) as u32, DIST_EXTRA[di] as u32);
            }
        } else {
            b.put(lit_c[t as usize], lit_l[t as usize] as u32);
        }
    }
    b.put(lit_c[256], lit_l[256] as u32);
}

fn write_block(b: &mut Bits, blk: &Block, last: bool) {
    let (lit_f, dist_f) = freqs(blk.tokens);
    let dynamic = Dynamic::new(&lit_f, &dist_f);
    let dyn_bits =
        3 + dynamic.header_bits() + data_bits(&lit_f, &dist_f, &dynamic.lit_l, &dynamic.dist_l);
    let (fl, fd) = fixed_lens();
    let fixed_bits = 3 + data_bits(&lit_f, &dist_f, &fl, &fd);
    // Stored: header, padding to a byte (at most 7 bits), then LEN, NLEN
    // and the bytes, per 65535-byte chunk.
    let chunks = blk.raw.len().div_ceil(65535).max(1) as u64;
    let stored_bits = chunks * (3 + 7 + 32) + 8 * blk.raw.len() as u64;
    if stored_bits < dyn_bits.min(fixed_bits) {
        let mut chunks = blk.raw.chunks(65535).peekable();
        if blk.raw.is_empty() {
            b.put(last as u32, 1);
            b.put(0, 2);
            b.align();
            b.put(0, 16);
            b.put(0xFFFF, 16);
            return;
        }
        while let Some(c) = chunks.next() {
            let final_chunk = last && chunks.peek().is_none();
            b.put(final_chunk as u32, 1);
            b.put(0, 2);
            b.align();
            b.put(c.len() as u32, 16);
            b.put(!(c.len() as u32) & 0xFFFF, 16);
            b.out.extend_from_slice(c);
        }
    } else if fixed_bits <= dyn_bits {
        b.put(last as u32, 1);
        b.put(1, 2);
        write_tokens(b, blk.tokens, &fl, &fd);
    } else {
        b.put(last as u32, 1);
        b.put(2, 2);
        dynamic.write_header(b);
        write_tokens(b, blk.tokens, &dynamic.lit_l, &dynamic.dist_l);
    }
}

/// Raw DEFLATE data (no zlib or gzip wrapper).
pub fn deflate(data: &[u8], level: Level) -> Vec<u8> {
    let tokens = tokenize(data, level);
    let mut b = Bits {
        out: Vec::with_capacity(data.len() / 3 + 64),
        acc: 0,
        n: 0,
    };
    if tokens.is_empty() {
        write_block(
            &mut b,
            &Block {
                tokens: &[],
                raw: &[],
            },
            true,
        );
        b.align();
        return b.out;
    }
    // Walk the tokens in blocks, tracking which input bytes each covers.
    let mut pos = 0usize;
    let mut chunks = tokens.chunks(BLOCK_TOKENS).peekable();
    while let Some(ts) = chunks.next() {
        let covered: usize = ts
            .iter()
            .map(|&t| {
                if t & MATCH != 0 {
                    ((t >> 15) & 0xFF) as usize + MIN_MATCH
                } else {
                    1
                }
            })
            .sum();
        let blk = Block {
            tokens: ts,
            raw: &data[pos..pos + covered],
        };
        pos += covered;
        write_block(&mut b, &blk, chunks.peek().is_none());
    }
    b.align();
    b.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_and_distance_codes() {
        assert_eq!(len_index(3), 0);
        assert_eq!(len_index(10), 7);
        assert_eq!(len_index(11), 8);
        assert_eq!(len_index(12), 8);
        assert_eq!(len_index(257), 27);
        assert_eq!(len_index(258), 28);
        assert_eq!(dist_index(1), 0);
        assert_eq!(dist_index(4), 3);
        assert_eq!(dist_index(5), 4);
        assert_eq!(dist_index(32768), 29);
    }

    #[test]
    fn code_lengths_are_complete_and_limited() {
        // Fibonacci frequencies force a deep tree.
        let mut f = vec![1u32, 1];
        while f.len() < 30 {
            let n = f.len();
            f.push(f[n - 1] + f[n - 2]);
        }
        let l = code_lengths(&f, 15);
        assert!(l.iter().all(|&x| (1..=15).contains(&x)));
        let kraft: f64 = l.iter().map(|&x| 0.5f64.powi(x as i32)).sum();
        assert!((kraft - 1.0).abs() < 1e-12, "complete: {kraft}");
        // A single used symbol still gets a complete two-symbol code.
        let l = code_lengths(&[0, 5, 0], 15);
        assert_eq!(l.iter().filter(|&&x| x == 1).count(), 2);
    }

    #[test]
    fn run_length_coding_of_lengths() {
        let lens = [0u8; 150];
        let r = rle_lengths(&lens);
        assert_eq!(r, vec![(18, 127), (18, 1)]);
        let r = rle_lengths(&[5, 5, 5, 5, 5, 5, 5, 5, 0, 0]);
        assert_eq!(r, vec![(5, 0), (16, 3), (5, 0), (0, 0), (0, 0)]);
    }
}

//! Recovery data (format 2.0): parity that rebuilds damaged bytes of the
//! snapshot.
//!
//! The section `Recovery` covers the snapshot's sections as stored: the
//! bytes from the end of the file header to the end of the last section
//! before it. (The first copy of the directory sits between those
//! sections and `Recovery`, the second copy after it.) The covered bytes
//! are cut into blocks of `B` bytes (the last one shorter), each with a
//! CRC-32 so a damaged block can be found. Parity block `r` is the XOR of
//! blocks `r`, `r + K`, `r + 2K`, … (each padded with zeros to `B`), so
//! any damage that leaves at most one bad block in each of those `K`
//! stripes is repaired; in particular any run of damage inside the covered
//! bytes up to `(K − 1) × B + 1` bytes long, wherever it starts. `K` is
//! chosen so the parity is about a sixteenth of the covered bytes.
//!
//! ```text
//! varint  scheme (1: XOR stripes as above)
//! varint  start of the covered bytes (file offset)
//! varint  length of the covered bytes
//! varint  block size B, a power of two from 64 to 65536
//! varint  stripe count K
//! u32     CRC-32 of each block, ceil(length / B) of them
//! u32     CRC-32 of everything above
//! K × B   parity blocks
//! ```
//!
//! The layout and the block CRCs check themselves, so the section is
//! still used when damage reaches its parity: a block rebuilt from a
//! damaged parity block fails its CRC and is left alone. The directory is
//! not covered (it lists this section); it has two copies instead, as does
//! the file header. Change sets appended later are not covered either: an
//! unreadable change set falls back to the state before it, and a full
//! save covers everything again.

use super::container::DirEntry;
use super::tags::{get_varint, put_varint};
use crate::dwg::crc::crc32;
use crate::error::{Error, Result};

pub const SECTION: &str = "Recovery";
const SCHEME_XOR: u64 = 1;
/// Parity is about 1/STRIPES_PER of the covered bytes.
const STRIPES_PER: usize = 16;

/// Block size for `len` covered bytes: about 256 blocks, 64 to 4096 bytes.
fn block_size(len: usize) -> usize {
    (len / 256).next_power_of_two().clamp(64, 4096)
}

/// The `Recovery` section for a file whose sections (all of `entries`)
/// end at `buf.len()`. Empty when nothing is covered.
pub fn build(buf: &[u8], entries: &[DirEntry]) -> Option<Vec<u8>> {
    let start = entries.iter().map(|e| e.offset).min().unwrap_or(0) as usize;
    let covered = &buf[start.min(buf.len())..];
    (!covered.is_empty()).then(|| encode(start as u64, covered))
}

fn encode(start: u64, covered: &[u8]) -> Vec<u8> {
    let b = block_size(covered.len());
    let blocks = covered.len().div_ceil(b);
    let k = blocks.div_ceil(STRIPES_PER).max(1);
    let mut out = Vec::with_capacity(36 + blocks * 4 + k * b);
    for v in [SCHEME_XOR, start, covered.len() as u64, b as u64, k as u64] {
        put_varint(&mut out, v);
    }
    for blk in covered.chunks(b) {
        out.extend_from_slice(&crc32(0, blk).to_le_bytes());
    }
    let own = crc32(0, &out);
    out.extend_from_slice(&own.to_le_bytes());
    let mut parity = vec![0u8; k * b];
    for (i, blk) in covered.chunks(b).enumerate() {
        let p = &mut parity[(i % k) * b..][..b];
        for (x, y) in p.iter_mut().zip(blk) {
            *x ^= y;
        }
    }
    out.extend_from_slice(&parity);
    out
}

/// A decoded `Recovery` section.
#[derive(Debug, Clone)]
pub struct Recovery<'a> {
    pub start: usize,
    pub len: usize,
    pub block: usize,
    pub stripes: usize,
    crcs: &'a [u8],
    parity: &'a [u8],
}

pub fn decode(data: &[u8]) -> Result<Recovery<'_>> {
    let bad = |m: &str| Error::format(format!("recovery section: {m}"));
    let mut p = 0usize;
    let scheme = get_varint(data, &mut p)?;
    if scheme != SCHEME_XOR {
        return Err(Error::unsupported(format!(
            "recovery scheme {scheme} from a newer revision"
        )));
    }
    let start = get_varint(data, &mut p)?;
    let len = get_varint(data, &mut p)?;
    let block = get_varint(data, &mut p)?;
    let stripes = get_varint(data, &mut p)?;
    if !block.is_power_of_two()
        || !(64..=1 << 16).contains(&block)
        || stripes == 0
        || len > 1 << 40
        || start > 1 << 40
    {
        return Err(bad("implausible layout"));
    }
    // Everything must fit the address space (32-bit targets too).
    let fit = |v: u64| usize::try_from(v).map_err(|_| bad("larger than memory"));
    let (start, len, block, stripes) = (fit(start)?, fit(len)?, fit(block)?, fit(stripes)?);
    if start.checked_add(len).is_none() {
        return Err(bad("larger than memory"));
    }
    let blocks = len.div_ceil(block);
    if stripes > blocks.max(1) {
        return Err(bad("more stripes than blocks"));
    }
    let need = blocks
        .checked_mul(4)
        .and_then(|c| c.checked_add(4))
        .and_then(|c| stripes.checked_mul(block).and_then(|q| c.checked_add(q)))
        .ok_or_else(|| bad("implausible layout"))?;
    if data.len() - p != need {
        return Err(bad("length does not match its layout"));
    }
    let (crcs, rest) = data[p..].split_at(blocks * 4);
    let (own, parity) = rest.split_at(4);
    if crc32(0, &data[..p + blocks * 4]).to_le_bytes() != own {
        return Err(bad("its layout or block CRCs are damaged"));
    }
    Ok(Recovery {
        start,
        len,
        block,
        stripes,
        crcs,
        parity,
    })
}

/// What the recovery data says about the covered bytes of `bytes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub blocks: usize,
    /// Indexes of blocks whose CRC does not match.
    pub damaged: Vec<usize>,
    /// Damaged blocks that cannot be rebuilt (another block of the same
    /// stripe is damaged too).
    pub unrepairable: Vec<usize>,
}

impl Recovery<'_> {
    /// The longest run of damaged bytes always rebuilt, wherever it lies.
    pub fn longest_run(&self) -> usize {
        (self.stripes - 1) * self.block + 1
    }

    fn block_range(&self, i: usize) -> std::ops::Range<usize> {
        let a = self.start + i * self.block;
        a..a.saturating_add(self.block).min(self.start + self.len)
    }

    fn crc(&self, i: usize) -> u32 {
        u32::from_le_bytes(self.crcs[i * 4..i * 4 + 4].try_into().unwrap_or([0; 4]))
    }

    /// Damaged blocks per stripe.
    fn per_stripe(&self, damaged: &[usize]) -> Vec<usize> {
        let mut n = vec![0usize; self.stripes];
        for &i in damaged {
            n[i % self.stripes] += 1;
        }
        n
    }

    pub fn scan(&self, bytes: &[u8]) -> Scan {
        let blocks = self.len.div_ceil(self.block);
        let mut damaged = Vec::new();
        for i in 0..blocks {
            let ok = bytes
                .get(self.block_range(i))
                .is_some_and(|b| crc32(0, b) == self.crc(i));
            if !ok {
                damaged.push(i);
            }
        }
        let per_stripe = self.per_stripe(&damaged);
        let unrepairable = damaged
            .iter()
            .copied()
            .filter(|&i| per_stripe[i % self.stripes] > 1)
            .collect();
        Scan {
            blocks,
            damaged,
            unrepairable,
        }
    }

    /// A copy of `bytes` with every repairable damaged block rebuilt, and
    /// the number rebuilt. `None` when nothing could be rebuilt.
    pub fn repair(&self, bytes: &[u8]) -> Option<(Vec<u8>, usize)> {
        let scan = self.scan(bytes);
        let per_stripe = self.per_stripe(&scan.damaged);
        let fixable: Vec<usize> = scan
            .damaged
            .iter()
            .copied()
            .filter(|&i| per_stripe[i % self.stripes] == 1)
            .collect();
        // The section lies after the bytes it covers, so they are all
        // there in any file it was read from.
        if fixable.is_empty() || self.start + self.len > bytes.len() {
            return None;
        }
        let mut out = bytes.to_vec();
        let blocks = scan.blocks;
        let mut fixed = 0;
        for &i in &fixable {
            let r = i % self.stripes;
            let mut acc = self.parity[r * self.block..][..self.block].to_vec();
            let mut j = r;
            while j < blocks {
                if j != i {
                    for (x, y) in acc.iter_mut().zip(&out[self.block_range(j)]) {
                        *x ^= y;
                    }
                }
                j += self.stripes;
            }
            let range = self.block_range(i);
            let n = range.len();
            if crc32(0, &acc[..n]) == self.crc(i) {
                out[range].copy_from_slice(&acc[..n]);
                fixed += 1;
            }
        }
        (fixed > 0).then_some((out, fixed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(n: usize) -> Vec<u8> {
        let mut x = 7u32;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (x >> 16) as u8
            })
            .collect()
    }

    #[test]
    fn a_run_of_damage_up_to_one_block_per_stripe_is_rebuilt() {
        for n in [1usize, 63, 64, 65, 2000, 100_000, 1_000_003] {
            let mut file = vec![0u8; 48];
            file.extend(data(n));
            let sec = encode(48, &file[48..]);
            let rec = decode(&sec).unwrap();
            assert!(rec.scan(&file).damaged.is_empty());
            // Overhead about 1/16 plus the CRCs (small inputs pay the
            // minimum of one block).
            if n >= 100_000 {
                assert!(sec.len() < n / 12, "{n}: {}", sec.len());
            }
            // Damage the longest run the stripes always cover, starting
            // anywhere.
            let run = (rec.stripes - 1) * rec.block + 1;
            let mut bad = file.clone();
            let at = 48 + n / 3;
            for b in bad.iter_mut().skip(at).take(run.min(n - n / 3)) {
                *b ^= 0x5A;
            }
            let (fixed, count) = rec.repair(&bad).unwrap();
            assert!(count >= 1);
            assert_eq!(fixed, file, "{n}");
            // Two damaged blocks in one stripe: reported, not rebuilt.
            if rec.len.div_ceil(rec.block) > rec.stripes {
                let mut bad = file.clone();
                bad[48] ^= 1;
                bad[48 + rec.stripes * rec.block] ^= 1;
                let scan = rec.scan(&bad);
                assert_eq!(scan.unrepairable.len(), 2);
                assert!(rec.repair(&bad).is_none());
            }
        }
    }

    #[test]
    fn hostile_recovery_sections_are_refused() {
        let file: Vec<u8> = data(5000);
        let sec = encode(0, &file);
        for cut in 0..sec.len().min(40) {
            assert!(decode(&sec[..cut]).is_err());
        }
        let mut x = 1u32;
        for _ in 0..500 {
            let mut b = sec.clone();
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let i = (x >> 8) as usize % 16.min(b.len());
            b[i] ^= 1 << (x % 8);
            if let Ok(r) = decode(&b) {
                let _ = r.scan(&file);
                let _ = r.repair(&file);
            }
        }
    }
}

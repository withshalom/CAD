//! Splitting a section of records into streams by kind of value, before
//! compression (format 2.0, directory flag `SPLIT`).
//!
//! DWG keeps each object's data, strings and handles in separate streams
//! from R2007 on, so that similar values sit together. This does the same
//! for a whole page of tagged records, as a reversible transform on bytes:
//!
//! ```text
//! varint  record count
//! varint  length of each of the 4 streams
//! stream 0  structure: per record a varint tag count, per tag its u16 group
//!           code and u8 value type
//! stream 1  floating point: F64 values and the coordinates of P2 and P3
//! stream 2  integers: I16, I32, I64 and BOOL bytes, and handles as zigzag
//!           varints of the difference from the previous handle with the
//!           same group code in the section
//! stream 3  text and bytes: STR, BYTES and newer types, length-prefixed
//! ```
//!
//! Handles are predicted per group code because neighbouring records
//! usually share owners and layers, and keys (group 5) ascend: most
//! differences are 0 or small. Only a section that rebuilds exactly is
//! split (records are checked by joining them again); any other is stored
//! as it is, without the flag.

use super::tags::{get_len, get_varint, put_varint, ty};
use crate::error::{Error, Result};
use std::collections::HashMap;

const STREAMS: usize = 4;

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

/// Fixed value sizes of the types that have one.
fn fixed_size(t: u8) -> Option<usize> {
    Some(match t {
        ty::F64 => 8,
        ty::I16 => 2,
        ty::I32 => 4,
        ty::I64 => 8,
        ty::BOOL => 1,
        ty::P2 => 16,
        ty::P3 => 24,
        _ => return None,
    })
}

/// Split a sequence of length-prefixed records. `None` when the data is not
/// a clean sequence of tags (damaged records kept as found), or does not
/// rebuild byte for byte.
pub fn split(data: &[u8]) -> Option<Vec<u8>> {
    let mut s: [Vec<u8>; STREAMS] = Default::default();
    let mut prev: HashMap<u16, u64> = HashMap::new();
    let mut p = 0usize;
    let mut records = 0u64;
    while p < data.len() {
        let n = get_len(data, &mut p, "record").ok()?;
        let rec = &data[p..p + n];
        p += n;
        records += 1;
        // Tag count first: walk the record once to count, then encode.
        let mut tags: Vec<(u16, u8, usize, usize)> = Vec::new();
        let mut q = 0usize;
        while q < rec.len() {
            let code = u16::from_le_bytes([*rec.get(q)?, *rec.get(q + 1)?]);
            let t = *rec.get(q + 2)?;
            q += 3;
            let start = q;
            match t {
                0 => return None,
                ty::HANDLE => {
                    get_varint(rec, &mut q).ok()?;
                }
                t if fixed_size(t).is_some() => {
                    q += fixed_size(t)?;
                    if q > rec.len() {
                        return None;
                    }
                }
                _ => {
                    let len = get_len(rec, &mut q, "value").ok()?;
                    q += len;
                }
            }
            tags.push((code, t, start, q));
        }
        put_varint(&mut s[0], tags.len() as u64);
        for (code, t, a, b) in tags {
            s[0].extend_from_slice(&code.to_le_bytes());
            s[0].push(t);
            let v = &rec[a..b];
            match t {
                ty::F64 | ty::P2 | ty::P3 => s[1].extend_from_slice(v),
                ty::I16 | ty::I32 | ty::I64 | ty::BOOL => s[2].extend_from_slice(v),
                ty::HANDLE => {
                    let mut k = 0;
                    let h = get_varint(v, &mut k).ok()?;
                    let last = prev.insert(code, h).unwrap_or(0);
                    put_varint(&mut s[2], zigzag(h.wrapping_sub(last) as i64));
                }
                _ => s[3].extend_from_slice(v),
            }
        }
    }
    let mut out = Vec::with_capacity(data.len() + 32);
    put_varint(&mut out, records);
    for st in &s {
        put_varint(&mut out, st.len() as u64);
    }
    for st in &s {
        out.extend_from_slice(st);
    }
    // Only exact transforms are used (a record kept as found may hold a
    // varint that is not in its shortest form, which would not rebuild).
    (join(&out, data.len()).ok()? == data).then_some(out)
}

/// Rebuild the records from their streams. `max` bounds the output.
pub fn join(data: &[u8], max: usize) -> Result<Vec<u8>> {
    let bad = |m: &str| Error::format(format!("split section: {m}"));
    let mut p = 0usize;
    let records = get_varint(data, &mut p)?;
    let mut lens = [0usize; STREAMS];
    for l in &mut lens {
        *l = usize::try_from(get_varint(data, &mut p)?)
            .map_err(|_| bad("stream longer than memory"))?;
    }
    let mut starts = [0usize; STREAMS];
    let mut at = p;
    for (i, l) in lens.iter().enumerate() {
        starts[i] = at;
        at = at
            .checked_add(*l)
            .filter(|&e| e <= data.len())
            .ok_or_else(|| bad("streams run past the end"))?;
    }
    if at != data.len() {
        return Err(bad("bytes after the streams"));
    }
    let stream = |i: usize| &data[starts[i]..starts[i] + lens[i]];
    let (s0, s1, s2, s3) = (stream(0), stream(1), stream(2), stream(3));
    let (mut p0, mut p1, mut p2, mut p3) = (0usize, 0usize, 0usize, 0usize);
    let mut prev: HashMap<u16, u64> = HashMap::new();
    let mut out = Vec::with_capacity(max.min(1 << 26));
    let mut rec = Vec::new();
    for _ in 0..records {
        let n = get_varint(s0, &mut p0)?;
        if n > (s0.len() - p0) as u64 / 3 {
            return Err(bad("tag count too large"));
        }
        rec.clear();
        for _ in 0..n {
            let head = s0
                .get(p0..p0 + 3)
                .ok_or_else(|| bad("structure ends early"))?;
            let code = u16::from_le_bytes([head[0], head[1]]);
            let t = head[2];
            p0 += 3;
            rec.extend_from_slice(head);
            match t {
                0 => return Err(bad("value type 0")),
                ty::HANDLE => {
                    let d = unzigzag(get_varint(s2, &mut p2)?);
                    let last = prev.get(&code).copied().unwrap_or(0);
                    let h = last.wrapping_add(d as u64);
                    prev.insert(code, h);
                    put_varint(&mut rec, h);
                }
                ty::F64 | ty::P2 | ty::P3 => {
                    let k = fixed_size(t).unwrap_or(0);
                    let v = s1.get(p1..p1 + k).ok_or_else(|| bad("numbers end early"))?;
                    rec.extend_from_slice(v);
                    p1 += k;
                }
                ty::I16 | ty::I32 | ty::I64 | ty::BOOL => {
                    let k = fixed_size(t).unwrap_or(0);
                    let v = s2
                        .get(p2..p2 + k)
                        .ok_or_else(|| bad("integers end early"))?;
                    rec.extend_from_slice(v);
                    p2 += k;
                }
                _ => {
                    let a = p3;
                    let len = get_len(s3, &mut p3, "value")?;
                    p3 += len;
                    rec.extend_from_slice(&s3[a..p3]);
                }
            }
            if out.len() + rec.len() > max {
                return Err(bad("larger than the directory says"));
            }
        }
        put_varint(&mut out, rec.len() as u64);
        out.extend_from_slice(&rec);
        if out.len() > max {
            return Err(bad("larger than the directory says"));
        }
    }
    if p0 != s0.len() || p1 != s1.len() || p2 != s2.len() || p3 != s3.len() {
        return Err(bad("streams not used up"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::tags::TagWriter;
    use super::*;
    use crate::geom::{Vec2, Vec3};
    use crate::model::Handle;

    fn page() -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..50u64 {
            let mut w = TagWriter::new();
            w.handle(5, Handle(0x100 + i * 3));
            w.i16(2000, 0x13);
            w.handle(330, Handle(0x1F));
            w.i16(2005, 2);
            w.handle(2006, Handle(0x10));
            w.p3(10, Vec3::new(i as f64, 2.0, 0.0));
            w.p2(11, Vec2::new(1.5, -2.5));
            w.f64(40, 0.25);
            w.str(1, "text é");
            w.bytes(310, &[1, 2, 3]);
            w.bool(290, true);
            w.i64(160, -5);
            w.i32(90, 7);
            put_varint(&mut out, w.buf.len() as u64);
            out.extend_from_slice(&w.buf);
        }
        out
    }

    #[test]
    fn split_and_join_round_trip() {
        let data = page();
        let s = split(&data).unwrap();
        assert_eq!(join(&s, data.len()).unwrap(), data);
        // Handles predicted: owner and layer repeat, keys step by 3.
        assert!(s.len() < data.len());
        // Empty input.
        let e = split(&[]).unwrap();
        assert_eq!(join(&e, 0).unwrap(), Vec::<u8>::new());
        // A record holding a damaged tag (type 0) is not split.
        let mut bad = data.clone();
        bad[3] = 0;
        assert!(split(&bad).is_none());
    }

    #[test]
    fn join_refuses_hostile_input_without_panicking() {
        let data = page();
        let s = split(&data).unwrap();
        for cut in 0..s.len() {
            assert!(join(&s[..cut], data.len())
                .map(|v| v != data)
                .unwrap_or(true));
        }
        assert!(join(&s, data.len() - 1).is_err(), "cap");
        let mut x = 3u32;
        for _ in 0..2000 {
            let mut b = s.clone();
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let i = (x >> 8) as usize % b.len();
            b[i] ^= 1 << (x % 8);
            let _ = join(&b, 1 << 20);
        }
    }
}

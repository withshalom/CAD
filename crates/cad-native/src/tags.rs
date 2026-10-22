//! Tagged values: the record encoding of the native format.
//!
//! A record is a sequence of tags. Each tag is a group code (u16, little
//! endian) with the meaning the published DXF reference gives it, a type
//! byte, and a value. Binary DXF infers the value type from the group-code
//! range; this format writes the type explicitly so a reader can step over
//! tags it does not know (and keep them), and adds point, handle and
//! length-prefixed types.
//! Codes 2000 and up are this format's own. See `docs/FORMAT.md`.

use crate::error::{Error, Result};
use crate::geom::{Vec2, Vec3};
use crate::model::Handle;

pub(crate) mod ty {
    pub const STR: u8 = 0x01;
    pub const F64: u8 = 0x02;
    pub const I16: u8 = 0x03;
    pub const I32: u8 = 0x04;
    pub const I64: u8 = 0x05;
    pub const BOOL: u8 = 0x06;
    pub const HANDLE: u8 = 0x07;
    pub const BYTES: u8 = 0x08;
    pub const P2: u8 = 0x09;
    pub const P3: u8 = 0x0A;
    // Types above P3 are for later revisions. Their values are always a
    // varint length and that many bytes, so any reader can step over them.
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    F64(f64),
    I16(i16),
    I32(i32),
    I64(i64),
    Bool(bool),
    Handle(u64),
    Bytes(Vec<u8>),
    P2(Vec2),
    P3(Vec3),
    /// A value type from a later revision (0x0B and up), undecoded.
    Unknown {
        ty: u8,
        bytes: Vec<u8>,
    },
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Str(_) => "string",
            Value::F64(_) => "double",
            Value::I16(_) => "int16",
            Value::I32(_) => "int32",
            Value::I64(_) => "int64",
            Value::Bool(_) => "bool",
            Value::Handle(_) => "handle",
            Value::Bytes(_) => "bytes",
            Value::P2(_) => "point2",
            Value::P3(_) => "point3",
            Value::Unknown { .. } => "newer type",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tag {
    pub code: u16,
    pub value: Value,
}

// ---------------------------------------------------------------------------
// Varints (unsigned LEB128)
// ---------------------------------------------------------------------------

pub fn put_varint(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            buf.push(byte);
            return;
        }
        buf.push(byte | 0x80);
    }
}

pub fn get_varint(data: &[u8], pos: &mut usize) -> Result<u64> {
    let mut v: u64 = 0;
    let mut shift = 0u32;
    loop {
        let b = *data
            .get(*pos)
            .ok_or_else(|| Error::truncated(format!("varint at byte {pos}")))?;
        *pos += 1;
        if shift == 63 && b > 1 {
            return Err(Error::format("varint overflows 64 bits"));
        }
        v |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            return Ok(v);
        }
        shift += 7;
        if shift > 63 {
            return Err(Error::format("varint longer than 10 bytes"));
        }
    }
}

/// Read a varint length and check it fits in what is left of `data`.
pub fn get_len(data: &[u8], pos: &mut usize, what: &str) -> Result<usize> {
    let n = get_varint(data, pos)?;
    let left = data.len().saturating_sub(*pos) as u64;
    if n > left {
        return Err(Error::truncated(format!(
            "{what} of {n} bytes at byte {pos} runs past the data ({left} left)"
        )));
    }
    Ok(n as usize)
}

fn fixed<const N: usize>(data: &[u8], pos: &mut usize) -> Result<[u8; N]> {
    let end = pos
        .checked_add(N)
        .ok_or_else(|| Error::format("offset overflow"))?;
    let s = data
        .get(*pos..end)
        .ok_or_else(|| Error::truncated(format!("{N}-byte value at byte {pos}")))?;
    *pos = end;
    let mut a = [0u8; N];
    a.copy_from_slice(s);
    Ok(a)
}

fn f64_at(data: &[u8], pos: &mut usize) -> Result<f64> {
    Ok(f64::from_le_bytes(fixed::<8>(data, pos)?))
}

/// Append a record to a records section: its length, then its tags.
pub fn put_record(out: &mut Vec<u8>, w: TagWriter) {
    put_varint(out, w.buf.len() as u64);
    out.extend_from_slice(&w.buf);
}

/// Split a records section into its length-prefixed records.
pub fn records(data: &[u8]) -> Result<Vec<&[u8]>> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < data.len() {
        let n = get_len(data, &mut p, "record")?;
        out.push(&data[p..p + n]);
        p += n;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct TagWriter {
    pub buf: Vec<u8>,
    /// Write every field, defaults included, as format 1.x does (for
    /// change sets appended to 1.x files).
    pub full: bool,
}

impl TagWriter {
    pub fn new() -> Self {
        TagWriter::default()
    }

    /// A writer for records of the given major version.
    pub fn for_major(major: u16) -> Self {
        TagWriter {
            buf: Vec::new(),
            full: major < 2,
        }
    }

    fn head(&mut self, code: u16, t: u8) {
        self.buf.extend_from_slice(&code.to_le_bytes());
        self.buf.push(t);
    }

    pub fn str(&mut self, code: u16, s: &str) {
        self.head(code, ty::STR);
        put_varint(&mut self.buf, s.len() as u64);
        self.buf.extend_from_slice(s.as_bytes());
    }
    pub fn f64(&mut self, code: u16, v: f64) {
        self.head(code, ty::F64);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i16(&mut self, code: u16, v: i16) {
        self.head(code, ty::I16);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, code: u16, v: i32) {
        self.head(code, ty::I32);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i64(&mut self, code: u16, v: i64) {
        self.head(code, ty::I64);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bool(&mut self, code: u16, v: bool) {
        self.head(code, ty::BOOL);
        self.buf.push(v as u8);
    }
    pub fn handle(&mut self, code: u16, h: Handle) {
        self.head(code, ty::HANDLE);
        put_varint(&mut self.buf, h.0);
    }
    pub fn bytes(&mut self, code: u16, b: &[u8]) {
        self.head(code, ty::BYTES);
        put_varint(&mut self.buf, b.len() as u64);
        self.buf.extend_from_slice(b);
    }
    pub fn p2(&mut self, code: u16, p: Vec2) {
        self.head(code, ty::P2);
        self.buf.extend_from_slice(&p.x.to_le_bytes());
        self.buf.extend_from_slice(&p.y.to_le_bytes());
    }
    pub fn p3(&mut self, code: u16, p: Vec3) {
        self.head(code, ty::P3);
        self.buf.extend_from_slice(&p.x.to_le_bytes());
        self.buf.extend_from_slice(&p.y.to_le_bytes());
        self.buf.extend_from_slice(&p.z.to_le_bytes());
    }

    /// Append tags that are already encoded (kept from a newer writer).
    pub fn encoded(&mut self, tags: &[u8]) {
        self.buf.extend_from_slice(tags);
    }

    // Unsigned model fields are stored bit-for-bit in the signed type of
    // the same width, as DXF does for flag words.
    pub fn u8(&mut self, code: u16, v: u8) {
        self.i16(code, v as i16);
    }
    pub fn u16(&mut self, code: u16, v: u16) {
        self.i16(code, v as i16);
    }
    pub fn u32(&mut self, code: u16, v: u32) {
        self.i32(code, v as i32);
    }
    pub fn u64(&mut self, code: u16, v: u64) {
        self.i64(code, v as i64);
    }
    pub fn count(&mut self, code: u16, n: usize) {
        self.i32(code, n.min(i32::MAX as usize) as i32);
    }

    /// Optional handle: written only when not null.
    pub fn opt_handle(&mut self, code: u16, h: Handle) {
        if !h.is_null() {
            self.handle(code, h);
        }
    }

    // Fields with a default (format 2.0): left out when equal to it. Floats
    // compare bit for bit, so -0.0 and NaN payloads round-trip exactly.
    pub fn f64_d(&mut self, code: u16, v: f64, d: f64) {
        if self.full || v.to_bits() != d.to_bits() {
            self.f64(code, v);
        }
    }
    pub fn i16_d(&mut self, code: u16, v: i16, d: i16) {
        if self.full || v != d {
            self.i16(code, v);
        }
    }
    pub fn u16_d(&mut self, code: u16, v: u16, d: i16) {
        self.i16_d(code, v as i16, d);
    }
    pub fn u8_d(&mut self, code: u16, v: u8, d: i16) {
        self.i16_d(code, v as i16, d);
    }
    pub fn i32_d(&mut self, code: u16, v: i32, d: i32) {
        if self.full || v != d {
            self.i32(code, v);
        }
    }
    pub fn u32_d(&mut self, code: u16, v: u32, d: i32) {
        self.i32_d(code, v as i32, d);
    }
    pub fn bool_d(&mut self, code: u16, v: bool, d: bool) {
        if self.full || v != d {
            self.bool(code, v);
        }
    }
    pub fn str_d(&mut self, code: u16, v: &str, d: &str) {
        if self.full || v != d {
            self.str(code, v);
        }
    }
    pub fn p2_d(&mut self, code: u16, v: Vec2, d: [f64; 2]) {
        if self.full || v.x.to_bits() != d[0].to_bits() || v.y.to_bits() != d[1].to_bits() {
            self.p2(code, v);
        }
    }
    pub fn p3_d(&mut self, code: u16, v: Vec3, d: [f64; 3]) {
        if self.full
            || v.x.to_bits() != d[0].to_bits()
            || v.y.to_bits() != d[1].to_bits()
            || v.z.to_bits() != d[2].to_bits()
        {
            self.p3(code, v);
        }
    }
    /// A list count, left out when the list is empty.
    pub fn count_d(&mut self, code: u16, n: usize) {
        if self.full || n != 0 {
            self.count(code, n);
        }
    }
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// Decode one tag at `pos`.
pub fn read_tag(data: &[u8], pos: &mut usize) -> Result<Tag> {
    let code = u16::from_le_bytes(fixed::<2>(data, pos)?);
    let t = fixed::<1>(data, pos)?[0];
    let value = match t {
        ty::STR => {
            let n = get_len(data, pos, "string")?;
            let s = std::str::from_utf8(&data[*pos..*pos + n])
                .map_err(|_| Error::format(format!("group {code}: string is not UTF-8")))?;
            *pos += n;
            Value::Str(s.to_string())
        }
        ty::F64 => Value::F64(f64_at(data, pos)?),
        ty::I16 => Value::I16(i16::from_le_bytes(fixed::<2>(data, pos)?)),
        ty::I32 => Value::I32(i32::from_le_bytes(fixed::<4>(data, pos)?)),
        ty::I64 => Value::I64(i64::from_le_bytes(fixed::<8>(data, pos)?)),
        ty::BOOL => Value::Bool(fixed::<1>(data, pos)?[0] != 0),
        ty::HANDLE => Value::Handle(get_varint(data, pos)?),
        ty::BYTES => {
            let n = get_len(data, pos, "byte string")?;
            let v = data[*pos..*pos + n].to_vec();
            *pos += n;
            Value::Bytes(v)
        }
        ty::P2 => Value::P2(Vec2::new(f64_at(data, pos)?, f64_at(data, pos)?)),
        ty::P3 => Value::P3(Vec3::new(
            f64_at(data, pos)?,
            f64_at(data, pos)?,
            f64_at(data, pos)?,
        )),
        0 => {
            return Err(Error::format(format!(
                "group {code}: value type 0 is never used"
            )))
        }
        other => {
            let n = get_len(data, pos, "value of a newer type")?;
            let bytes = data[*pos..*pos + n].to_vec();
            *pos += n;
            Value::Unknown { ty: other, bytes }
        }
    };
    Ok(Tag { code, value })
}

/// Sequential reader over one record's tags.
///
/// Records are read in the order they were written. Tags a newer writer
/// appends after the fields this reader knows are left for the caller,
/// which finds them from [`TagReader::position`].
pub struct TagReader<'a> {
    data: &'a [u8],
    pos: usize,
    peeked: Option<Tag>,
    /// Where the peeked tag starts.
    peeked_at: usize,
}

macro_rules! typed {
    ($name:ident, $opt:ident, $variant:ident, $t:ty) => {
        pub fn $name(&mut self, code: u16) -> Result<$t> {
            match self.expect(code)? {
                Value::$variant(v) => Ok(v),
                other => Err(mismatch(code, stringify!($variant), &other)),
            }
        }
        #[allow(dead_code)]
        pub fn $opt(&mut self, code: u16) -> Result<Option<$t>> {
            match self.opt(code)? {
                None => Ok(None),
                Some(Value::$variant(v)) => Ok(Some(v)),
                Some(other) => Err(mismatch(code, stringify!($variant), &other)),
            }
        }
    };
}

fn mismatch(code: u16, want: &str, got: &Value) -> Error {
    Error::format(format!(
        "group {code}: expected {want}, found {}",
        got.type_name()
    ))
}

impl<'a> TagReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        TagReader {
            data,
            pos: 0,
            peeked: None,
            peeked_at: 0,
        }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Byte offset of the first tag not yet taken (a peeked tag counts as
    /// not taken).
    pub fn position(&self) -> usize {
        if self.peeked.is_some() {
            self.peeked_at
        } else {
            self.pos
        }
    }

    fn fill(&mut self) -> Result<()> {
        if self.peeked.is_none() && self.pos < self.data.len() {
            // Read into a copy so a failed read leaves the position at the
            // start of the bad tag.
            let mut p = self.pos;
            let t = read_tag(self.data, &mut p)?;
            self.peeked_at = self.pos;
            self.pos = p;
            self.peeked = Some(t);
        }
        Ok(())
    }

    /// Group code of the next tag, read without decoding its value, so a
    /// tag with a value type this version does not know (from a newer
    /// writer) does not stop a reader that only checks for an optional
    /// field. `None` at the end of the data.
    pub fn peek_code(&mut self) -> Result<Option<u16>> {
        if let Some(t) = &self.peeked {
            return Ok(Some(t.code));
        }
        Ok(self
            .data
            .get(self.pos..self.pos.saturating_add(2))
            .map(|b| u16::from_le_bytes([b[0], b[1]])))
    }

    pub fn next_tag(&mut self) -> Result<Option<Tag>> {
        self.fill()?;
        Ok(self.peeked.take())
    }

    #[allow(dead_code)]
    pub fn at_end(&mut self) -> Result<bool> {
        Ok(self.position() >= self.data.len())
    }

    pub fn expect(&mut self, code: u16) -> Result<Value> {
        match self.next_tag()? {
            Some(t) if t.code == code => Ok(t.value),
            Some(t) => Err(Error::format(format!(
                "expected group {code}, found group {}",
                t.code
            ))),
            None => Err(Error::truncated(format!("record ends before group {code}"))),
        }
    }

    pub fn opt(&mut self, code: u16) -> Result<Option<Value>> {
        if self.peek_code()? == Some(code) {
            Ok(self.next_tag()?.map(|t| t.value))
        } else {
            Ok(None)
        }
    }

    typed!(str, opt_str, Str, String);
    typed!(f64, opt_f64, F64, f64);
    typed!(i16, opt_i16, I16, i16);
    typed!(i32, opt_i32, I32, i32);
    typed!(i64, opt_i64, I64, i64);
    typed!(bool, opt_bool, Bool, bool);
    typed!(bytes, opt_bytes, Bytes, Vec<u8>);
    typed!(p2, opt_p2, P2, Vec2);
    typed!(p3, opt_p3, P3, Vec3);

    pub fn handle(&mut self, code: u16) -> Result<Handle> {
        match self.expect(code)? {
            Value::Handle(v) => Ok(Handle(v)),
            other => Err(mismatch(code, "Handle", &other)),
        }
    }
    pub fn opt_handle(&mut self, code: u16) -> Result<Handle> {
        match self.opt(code)? {
            None => Ok(Handle::NULL),
            Some(Value::Handle(v)) => Ok(Handle(v)),
            Some(other) => Err(mismatch(code, "Handle", &other)),
        }
    }

    pub fn u8(&mut self, code: u16) -> Result<u8> {
        let v = self.i16(code)?;
        u8::try_from(v as u16).map_err(|_| Error::format(format!("group {code}: {v} out of range")))
    }
    pub fn u16(&mut self, code: u16) -> Result<u16> {
        Ok(self.i16(code)? as u16)
    }
    pub fn u32(&mut self, code: u16) -> Result<u32> {
        Ok(self.i32(code)? as u32)
    }
    pub fn u64(&mut self, code: u16) -> Result<u64> {
        Ok(self.i64(code)? as u64)
    }

    // Fields with a default: the default when the next tag is another code.
    pub fn f64_or(&mut self, code: u16, d: f64) -> Result<f64> {
        Ok(self.opt_f64(code)?.unwrap_or(d))
    }
    pub fn i16_or(&mut self, code: u16, d: i16) -> Result<i16> {
        Ok(self.opt_i16(code)?.unwrap_or(d))
    }
    pub fn u16_or(&mut self, code: u16, d: i16) -> Result<u16> {
        Ok(self.i16_or(code, d)? as u16)
    }
    pub fn u8_or(&mut self, code: u16, d: i16) -> Result<u8> {
        let v = self.i16_or(code, d)?;
        u8::try_from(v as u16).map_err(|_| Error::format(format!("group {code}: {v} out of range")))
    }
    pub fn i32_or(&mut self, code: u16, d: i32) -> Result<i32> {
        Ok(self.opt_i32(code)?.unwrap_or(d))
    }
    pub fn u32_or(&mut self, code: u16, d: i32) -> Result<u32> {
        Ok(self.i32_or(code, d)? as u32)
    }
    pub fn bool_or(&mut self, code: u16, d: bool) -> Result<bool> {
        Ok(self.opt_bool(code)?.unwrap_or(d))
    }
    pub fn str_or(&mut self, code: u16, d: &str) -> Result<String> {
        Ok(self.opt_str(code)?.unwrap_or_else(|| d.to_string()))
    }
    pub fn p2_or(&mut self, code: u16, d: [f64; 2]) -> Result<Vec2> {
        Ok(self.opt_p2(code)?.unwrap_or(Vec2::new(d[0], d[1])))
    }
    pub fn p3_or(&mut self, code: u16, d: [f64; 3]) -> Result<Vec3> {
        Ok(self.opt_p3(code)?.unwrap_or(Vec3::new(d[0], d[1], d[2])))
    }
    /// A list length that may be left out (an empty list).
    pub fn count_or(&mut self, code: u16, min_item: usize) -> Result<usize> {
        if self.peek_code()? == Some(code) {
            self.count(code, min_item)
        } else {
            Ok(0)
        }
    }

    /// A list length. Each element takes at least `min_item` bytes, which
    /// bounds the count by what is left of the record.
    pub fn count(&mut self, code: u16, min_item: usize) -> Result<usize> {
        let n = self.i32(code)?;
        if n < 0 {
            return Err(Error::format(format!("group {code}: negative count {n}")));
        }
        let n = n as usize;
        if n.saturating_mul(min_item.max(1)) > self.remaining() {
            return Err(Error::truncated(format!(
                "group {code}: {n} items cannot fit in {} bytes",
                self.remaining()
            )));
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip() {
        for v in [
            0u64,
            1,
            127,
            128,
            300,
            16_383,
            16_384,
            u32::MAX as u64,
            u64::MAX,
        ] {
            let mut b = Vec::new();
            put_varint(&mut b, v);
            let mut p = 0;
            assert_eq!(get_varint(&b, &mut p).unwrap(), v);
            assert_eq!(p, b.len());
        }
        let mut p = 0;
        assert!(get_varint(&[0xFF; 11], &mut p).is_err());
    }

    #[test]
    fn tags_roundtrip_and_skip() {
        let mut w = TagWriter::new();
        w.str(1, "héllo");
        w.f64(40, 2.5);
        w.u16(70, 0xFFFF);
        w.handle(330, Handle(0x1F));
        w.p3(10, Vec3::new(1.0, 2.0, 3.0));
        w.bytes(310, &[1, 2, 3]);
        w.bool(2999, true); // a tag this reader does not know
        let mut r = TagReader::new(&w.buf);
        assert_eq!(r.str(1).unwrap(), "héllo");
        assert_eq!(r.opt_f64(41).unwrap(), None);
        assert_eq!(r.f64(40).unwrap(), 2.5);
        assert_eq!(r.u16(70).unwrap(), 0xFFFF);
        assert_eq!(r.handle(330).unwrap(), Handle(0x1F));
        assert_eq!(r.p3(10).unwrap(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(r.bytes(310).unwrap(), vec![1, 2, 3]);
        assert!(!r.at_end().unwrap());
        assert!(r.f64(40).is_err());
    }

    #[test]
    fn corrupt_input_is_error() {
        let mut w = TagWriter::new();
        w.str(1, "abcdef");
        for cut in 0..w.buf.len() {
            let mut p = 0;
            assert!(read_tag(&w.buf[..cut], &mut p).is_err());
        }
        let mut bad = w.buf.clone();
        bad[2] = 0;
        let mut p = 0;
        assert!(read_tag(&bad, &mut p).is_err());
    }

    #[test]
    fn newer_value_types_are_length_prefixed_and_skipped() {
        // Group 2999, type 0x7E, 3 bytes; then an ordinary tag.
        let mut data = vec![0xB7, 0x0B, 0x7E, 3, 1, 2, 3];
        let mut w = TagWriter::new();
        w.f64(40, 2.5);
        data.extend_from_slice(&w.buf);
        let mut r = TagReader::new(&data);
        assert_eq!(r.opt_f64(40).unwrap(), None);
        assert_eq!(
            r.next_tag().unwrap().unwrap().value,
            Value::Unknown {
                ty: 0x7E,
                bytes: vec![1, 2, 3]
            }
        );
        assert_eq!(r.f64(40).unwrap(), 2.5);
        // A length running past the data is damage.
        let mut p = 0;
        assert!(read_tag(&[0xB7, 0x0B, 0x7E, 9, 1], &mut p).is_err());
    }
}

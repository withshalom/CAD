//! The three streams an object (or the R2007+ header) is split into: main
//! data, strings (R2007+) and handles.
//!
//! Written from knowledge of the spec (chapter 20, "common object format",
//! R2007+ string stream). VERIFY against the spec PDF and real files.

use super::bits::{BitReader, HandleRef};
use super::version::DwgVersion;
use crate::error::{Error, Result};
use crate::model::Color;
use crate::text;

pub(crate) struct Streams<'a> {
    pub ver: DwgVersion,
    pub codepage: u16,
    pub dat: BitReader<'a>,
    /// R2007+ string stream; `None` when the object stores no strings.
    pub strs: Option<BitReader<'a>>,
    /// Separate handle stream; `None` means handles are inline in `dat`
    /// (the R13–R2004 header section).
    pub hdl: Option<BitReader<'a>>,
}

impl<'a> Streams<'a> {
    /// TV: 8-bit text in the main stream up to R2004, UTF-16 text in the
    /// string stream from R2007.
    pub fn tv(&mut self) -> Result<String> {
        if self.ver.has_string_stream() {
            match self.strs.as_mut() {
                Some(s) => s.text16(),
                None => Ok(String::new()),
            }
        } else {
            let raw = self.dat.text8()?;
            Ok(text::decode_ansi(&raw, self.codepage))
        }
    }

    /// Next handle reference from the handle stream (or inline).
    pub fn h(&mut self) -> Result<HandleRef> {
        match self.hdl.as_mut() {
            Some(h) => h.handle(),
            None => self.dat.handle(),
        }
    }

    /// Handle resolved against the owning object's handle.
    pub fn h_abs(&mut self, own: u64) -> Result<u64> {
        Ok(self.h()?.absolute(own))
    }

    /// Bits left in the handle stream (or main stream when inline).
    pub fn handle_bits_left(&self) -> usize {
        match &self.hdl {
            Some(h) => h.remaining(),
            None => self.dat.remaining(),
        }
    }

    /// CMC (§2.11): BS index up to R2000; from R2004 a BS index, a BL
    /// method+RGB word and a flag byte announcing optional names.
    pub fn cmc(&mut self) -> Result<(Color, i16)> {
        let index = self.dat.bs()? as i16;
        if self.ver < DwgVersion::R2004 {
            return Ok((Color::from_aci(index), index));
        }
        let word = self.dat.bl()?;
        let flags = self.dat.rc()?;
        if flags & 1 != 0 {
            let _name = self.tv()?;
        }
        if flags & 2 != 0 {
            let _book = self.tv()?;
        }
        Ok((Color::from_method_word(word, index), index))
    }

    /// ENC entity color (§2.11, R2004+). Returns the color, the raw
    /// transparency word if stored, and whether a color-book handle follows
    /// in the handle stream.
    pub fn enc(&mut self) -> Result<(Color, Option<u32>, bool)> {
        let raw = self.dat.bs()?;
        if self.ver < DwgVersion::R2004 {
            return Ok((Color::from_aci(raw as i16), None, false));
        }
        let index = (raw & 0x01FF) as i16;
        let mut color = Color::from_aci(index);
        if raw & 0x8000 != 0 {
            let word = self.dat.bl()?;
            color = Color::from_method_word(word, index);
        }
        let transparency = if raw & 0x2000 != 0 {
            Some(self.dat.bl()?)
        } else {
            None
        };
        Ok((color, transparency, raw & 0x4000 != 0))
    }

    /// Guard against absurd counts read from corrupt data: `n` items, each
    /// needing at least `min_bits` in the main stream.
    pub fn check_count(&self, n: u32, min_bits: usize, what: &str) -> Result<usize> {
        let n = n as usize;
        if n.saturating_mul(min_bits) > self.dat.remaining() {
            return Err(Error::format(format!(
                "{what} count {n} exceeds remaining data ({} bits)",
                self.dat.remaining()
            )));
        }
        Ok(n)
    }

    /// Same guard for handle lists.
    pub fn check_handle_count(&self, n: u32, what: &str) -> Result<usize> {
        let n = n as usize;
        if n.saturating_mul(8) > self.handle_bits_left() {
            return Err(Error::format(format!(
                "{what} handle count {n} exceeds remaining handle data"
            )));
        }
        Ok(n)
    }
}

/// Locate the R2007+ string stream that ends just before bit `endbit` of
/// `data`. The bit at `endbit - 1` says whether strings are present; the 16
/// bits before it hold the stream size in bits, with 0x8000 flagging a
/// further 16-bit high part before that. Returns the string reader and the
/// bit where the strings begin.
pub(crate) fn string_stream(data: &[u8], endbit: usize) -> Result<Option<(BitReader<'_>, usize)>> {
    if endbit == 0 || endbit > data.len() * 8 {
        return Err(Error::format(format!(
            "string stream end bit {endbit} out of range"
        )));
    }
    let mut r = BitReader::with_bounds(data, 0, endbit);
    r.set_position(endbit - 1)?;
    if !r.b()? {
        return Ok(None);
    }
    let mut p = endbit
        .checked_sub(17)
        .ok_or_else(|| Error::format("string stream flag without size"))?;
    r.set_position(p)?;
    let mut size = r.rs()? as usize;
    if size & 0x8000 != 0 {
        p = p
            .checked_sub(16)
            .ok_or_else(|| Error::format("string stream size underflow"))?;
        r.set_position(p)?;
        let hi = r.rs()? as usize;
        size = (size & 0x7FFF) | (hi << 15);
    }
    let start = p
        .checked_sub(size)
        .ok_or_else(|| Error::format(format!("string stream of {size} bits does not fit")))?;
    Ok(Some((BitReader::with_bounds(data, start, p), start)))
}

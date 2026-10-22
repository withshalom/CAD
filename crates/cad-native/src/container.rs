//! The file container: a fixed header, named sections, a section
//! directory, and a trailing copy of the header.
//!
//! Modelled on the DWG R2004 file structure (named, individually
//! compressed and checksummed sections found through a map), simplified to
//! one stream per section. Two copies of the header, each with its own CRC
//! and a generation counter, let an interrupted incremental save fall back
//! to the last complete state; from format 2.0 each copy points at its own
//! copy of the directory, so neither is a single point of failure. See
//! `docs/FORMAT.md`.

use super::split;
use super::tags::{get_len, get_varint, put_varint};
use crate::codec::{self, Level};
use crate::dwg::{crc::crc32, lz77};
use crate::error::{Error, ErrorKind, Result};
use std::borrow::Cow;

/// `\x89CAD\r\n\x1a\n`: a high-bit byte catches 7-bit transfers, CR LF and
/// LF catch newline conversion, and 0x1A stops text-mode readers.
pub const MAGIC: [u8; 8] = [0x89, b'C', b'A', b'D', b'\r', b'\n', 0x1A, b'\n'];
pub const FORMAT_MAJOR: u16 = 2;
pub const FORMAT_MINOR: u16 = 9;
/// The oldest major version this build reads.
pub const OLDEST_MAJOR: u16 = 1;
pub const HEADER_LEN: usize = 48;

/// File header flags.
pub const FLAG_HAS_JOURNAL: u32 = 1;
/// Reserved (2.0): the file carries a signature over its hash chain. No
/// writer sets it yet; readers that do not check signatures ignore it.
pub const FLAG_SIGNED: u32 = 2;
/// Reserved (2.0): some sections are encrypted (their directory entries
/// say which). No writer sets it yet.
pub const FLAG_ENCRYPTED: u32 = 4;

/// Directory entry flags (2.0). Bits 0 to 31 change how the stored bytes
/// decode, so a reader that does not know one cannot read that section;
/// bits 32 to 63 are informational and may be ignored.
pub mod section_flag {
    /// The records are split into streams before compression
    /// (`native::split`).
    pub const SPLIT: u64 = 1;
    /// Reserved: the stored bytes are encrypted. This build reads no
    /// encrypted section and reports it as unreadable.
    pub const ENCRYPTED: u64 = 2;
    pub const CRITICAL: u64 = 0xFFFF_FFFF;
    pub const KNOWN: u64 = SPLIT | ENCRYPTED;
}

/// Sections at or below this size are stored uncompressed.
const MIN_COMPRESS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Raw,
    /// DWG R2004 page compression (spec §4.7), see `dwg::lz77`. Written by
    /// format 1.x.
    Lz77,
    /// DEFLATE (RFC 1951), raw, without a zlib wrapper; see `codec`.
    /// Written from format 2.0 on.
    Deflate,
    /// A codec from a newer revision: the section cannot be read, the rest
    /// of the file can.
    Other(u8),
}

impl Codec {
    pub fn byte(self) -> u8 {
        match self {
            Codec::Raw => 0,
            Codec::Lz77 => 1,
            Codec::Deflate => 2,
            Codec::Other(b) => b,
        }
    }
    pub fn from_byte(b: u8) -> Codec {
        match b {
            0 => Codec::Raw,
            1 => Codec::Lz77,
            2 => Codec::Deflate,
            b => Codec::Other(b),
        }
    }
    pub fn name(self) -> String {
        match self {
            Codec::Raw => "stored".into(),
            Codec::Lz77 => "DWG LZ77".into(),
            Codec::Deflate => "DEFLATE".into(),
            Codec::Other(b) => format!("codec {b}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileHeader {
    pub major: u16,
    pub minor: u16,
    pub flags: u32,
    pub generation: u64,
    pub dir_offset: u64,
    pub dir_len: u32,
    pub dir_crc: u32,
}

impl FileHeader {
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..8].copy_from_slice(&MAGIC);
        b[8..10].copy_from_slice(&self.major.to_le_bytes());
        b[10..12].copy_from_slice(&self.minor.to_le_bytes());
        b[12..16].copy_from_slice(&self.flags.to_le_bytes());
        b[16..24].copy_from_slice(&self.generation.to_le_bytes());
        b[24..32].copy_from_slice(&self.dir_offset.to_le_bytes());
        b[32..36].copy_from_slice(&self.dir_len.to_le_bytes());
        b[36..40].copy_from_slice(&self.dir_crc.to_le_bytes());
        // 40..44 reserved, zero.
        let crc = crc32(0, &b[..44]);
        b[44..48].copy_from_slice(&crc.to_le_bytes());
        b
    }

    /// Decode and check one header copy. `None` when it is damaged.
    pub fn decode(b: &[u8]) -> Option<FileHeader> {
        if b.len() < HEADER_LEN || b[..8] != MAGIC {
            return None;
        }
        let crc = u32::from_le_bytes(b[44..48].try_into().ok()?);
        if crc32(0, &b[..44]) != crc {
            return None;
        }
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap_or([0; 4]));
        let u64_at = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap_or([0; 8]));
        Some(FileHeader {
            major: u16_at(8),
            minor: u16_at(10),
            flags: u32_at(12),
            generation: u64_at(16),
            dir_offset: u64_at(24),
            dir_len: u32_at(32),
            dir_crc: u32_at(36),
        })
    }

    /// True when the two copies describe the same state: everything but
    /// the place of the directory (each copy may point at its own copy).
    pub fn same_state(&self, other: &FileHeader) -> bool {
        FileHeader {
            dir_offset: 0,
            ..*self
        } == FileHeader {
            dir_offset: 0,
            ..*other
        }
    }
}

/// True when `bytes` starts with the native magic.
pub fn is_native(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..8] == MAGIC
}

/// True when either copy of the file header identifies a native file, so
/// a file whose first bytes were damaged is still recognised.
pub fn looks_native(bytes: &[u8]) -> bool {
    is_native(bytes)
        || (bytes.len() >= 2 * HEADER_LEN
            && FileHeader::decode(&bytes[bytes.len() - HEADER_LEN..]).is_some())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub codec: Codec,
    pub offset: u64,
    pub stored_len: u64,
    /// Length of the section's records (after decompressing and joining
    /// split streams).
    pub raw_len: u64,
    /// CRC-32 of the section's records.
    pub crc: u32,
    /// `section_flag` bits (0 in format 1.x).
    pub flags: u64,
}

pub fn encode_directory(entries: &[DirEntry]) -> Vec<u8> {
    let mut b = Vec::new();
    put_varint(&mut b, entries.len() as u64);
    for e in entries {
        put_varint(&mut b, e.name.len() as u64);
        b.extend_from_slice(e.name.as_bytes());
        b.push(e.codec.byte());
        put_varint(&mut b, e.offset);
        put_varint(&mut b, e.stored_len);
        put_varint(&mut b, e.raw_len);
        b.extend_from_slice(&e.crc.to_le_bytes());
        put_varint(&mut b, e.flags);
    }
    b
}

pub fn decode_directory(b: &[u8]) -> Result<Vec<DirEntry>> {
    let mut p = 0usize;
    let n = get_varint(b, &mut p)?;
    if n > b.len() as u64 {
        return Err(Error::format(format!("directory claims {n} sections")));
    }
    let mut out = Vec::with_capacity((n as usize).min(256));
    for _ in 0..n {
        let len = get_len(b, &mut p, "section name")?;
        let name = std::str::from_utf8(&b[p..p + len])
            .map_err(|_| Error::format("section name is not UTF-8"))?
            .to_string();
        p += len;
        let codec = Codec::from_byte(*b.get(p).ok_or_else(|| Error::truncated("directory"))?);
        p += 1;
        let offset = get_varint(b, &mut p)?;
        let stored_len = get_varint(b, &mut p)?;
        let raw_len = get_varint(b, &mut p)?;
        let crc_bytes = b
            .get(p..p + 4)
            .ok_or_else(|| Error::truncated("directory CRC"))?;
        let crc = u32::from_le_bytes(crc_bytes.try_into().unwrap_or([0; 4]));
        p += 4;
        let flags = get_varint(b, &mut p)?;
        out.push(DirEntry {
            name,
            codec,
            offset,
            stored_len,
            raw_len,
            crc,
            flags,
        });
    }
    Ok(out)
}

/// Directories larger than this are compressed (2.0).
const MIN_COMPRESS_DIRECTORY: usize = 256;
/// Largest directory a reader inflates.
const MAX_DIRECTORY: u64 = 64 << 20;

/// The directory as a file of the given major version stores it: 1.x as
/// it is; from 2.0 a codec byte first (0 stored, 2 DEFLATE, then the
/// length as a varint), since a directory of many pages is mostly
/// repeated names and small numbers and is written again by every
/// incremental save.
pub fn encode_directory_for(major: u16, entries: &[DirEntry]) -> Vec<u8> {
    let raw = encode_directory(entries);
    if major < 2 {
        return raw;
    }
    if raw.len() > MIN_COMPRESS_DIRECTORY {
        let z = codec::deflate(&raw, Level::Default);
        if z.len() + 10 < raw.len() {
            let mut out = vec![Codec::Deflate.byte()];
            put_varint(&mut out, raw.len() as u64);
            out.extend_from_slice(&z);
            return out;
        }
    }
    let mut out = Vec::with_capacity(raw.len() + 1);
    out.push(Codec::Raw.byte());
    out.extend_from_slice(&raw);
    out
}

pub fn decode_directory_for(major: u16, b: &[u8]) -> Result<Vec<DirEntry>> {
    if major < 2 {
        return decode_directory(b);
    }
    let (&c, rest) = b
        .split_first()
        .ok_or_else(|| Error::truncated("directory"))?;
    match Codec::from_byte(c) {
        Codec::Raw => decode_directory(rest),
        Codec::Deflate => {
            let mut p = 0usize;
            let n = get_varint(rest, &mut p)?;
            if n > MAX_DIRECTORY || n > max_inflated(Codec::Deflate, rest.len() as u64) {
                return Err(Error::format(format!("directory claims {n} bytes")));
            }
            let (raw, used) = codec::inflate(&rest[p..], n as usize)
                .map_err(|e| Error::format(format!("directory: {}", e.0)))?;
            if raw.len() as u64 != n || p + used != rest.len() {
                return Err(Error::format("directory length does not match"));
            }
            decode_directory(&raw)
        }
        other => Err(Error::unsupported(format!(
            "directory stored with {}",
            other.name()
        ))),
    }
}

/// How sections are encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encoding {
    /// `Raw` stores every section as it is.
    pub codec: Codec,
    pub level: Level,
    /// Split record sections into streams (2.0).
    pub split: bool,
}

impl Encoding {
    /// What this version writes.
    pub fn current(compress: bool, level: Level) -> Encoding {
        Encoding {
            codec: if compress { Codec::Deflate } else { Codec::Raw },
            level,
            split: compress,
        }
    }
    /// What a file of the given major version may hold (a change set
    /// appended to a 1.x file must stay readable by 1.x readers).
    pub fn for_major(major: u16) -> Encoding {
        if major >= 2 {
            Encoding::current(true, Level::Default)
        } else {
            Encoding {
                codec: Codec::Lz77,
                level: Level::Default,
                split: false,
            }
        }
    }
}

/// One encoded section.
#[derive(Debug, Clone)]
pub struct Encoded {
    pub codec: Codec,
    pub flags: u64,
    pub stored: Vec<u8>,
    pub crc: u32,
}

/// Encode one section's data. `records` marks a section of length-prefixed
/// records, which may be split. Compressed only when that makes it smaller.
pub fn encode_section(raw: &[u8], records: bool, enc: Encoding) -> Encoded {
    let crc = crc32(0, raw);
    if enc.codec != Codec::Raw && raw.len() > MIN_COMPRESS {
        let (body, flags) = match (records && enc.split).then(|| split::split(raw)).flatten() {
            Some(s) => (Cow::Owned(s), section_flag::SPLIT),
            None => (Cow::Borrowed(raw), 0),
        };
        let packed = match enc.codec {
            Codec::Deflate => Some(codec::deflate(&body, enc.level)),
            Codec::Lz77 => lz77::compress(&body),
            _ => None,
        };
        // Worth it only when it saves more than a few bytes.
        if let Some(c) = packed.filter(|c| c.len() + 8 <= raw.len()) {
            return Encoded {
                codec: enc.codec,
                flags,
                stored: c,
                crc,
            };
        }
    }
    Encoded {
        codec: Codec::Raw,
        flags: 0,
        stored: raw.to_vec(),
        crc,
    }
}

/// A parsed container: the chosen header and its directory.
#[derive(Debug, Clone)]
pub struct Container<'a> {
    pub bytes: Cow<'a, [u8]>,
    pub header: FileHeader,
    pub entries: Vec<DirEntry>,
    /// Which copy was used: "header" or "trailer".
    pub source: &'static str,
    pub notes: Vec<String>,
}

/// Largest section we will inflate, as a guard against corrupt lengths.
const MAX_SECTION: u64 = 1 << 32;

/// Most output one stored byte can produce: a long count of the §4.7
/// stream adds 0xFF per byte; DEFLATE's best case is a 258-byte match in
/// two one-bit codes, 1032 bytes per byte. Plus slack for the smallest
/// streams.
fn max_inflated(codec: Codec, stored: u64) -> u64 {
    let ratio = match codec {
        Codec::Lz77 => 0x100,
        Codec::Deflate => 1032,
        _ => 1,
    };
    stored.saturating_mul(ratio).saturating_add(64)
}

/// The largest split form of `raw` bytes of records (a handle difference
/// can take more bytes than the handle; see `split`).
fn max_split(raw: u64) -> u64 {
    raw.saturating_mul(4).saturating_add(64)
}

/// Read and decode one section of `bytes`. The second value is false
/// when the CRC does not match (the data is still returned).
pub fn read_entry(bytes: &[u8], e: &DirEntry) -> Result<(Vec<u8>, bool)> {
    let unknown = e.flags & section_flag::CRITICAL & !section_flag::KNOWN;
    if unknown != 0 {
        return Err(Error::unsupported(format!(
            "section {}: flags {unknown:#x} from a newer revision",
            e.name
        )));
    }
    if e.flags & section_flag::ENCRYPTED != 0 {
        return Err(Error::unsupported(format!(
            "section {} is encrypted; this build cannot decrypt",
            e.name
        )));
    }
    let split = e.flags & section_flag::SPLIT != 0;
    // Length of the decompressed body: the records, or their split form.
    let body_max = if split {
        max_split(e.raw_len)
    } else {
        e.raw_len
    };
    let plausible = match e.codec {
        Codec::Raw | Codec::Lz77 if split => {
            return Err(Error::format(format!(
                "section {}: split streams are only written with DEFLATE",
                e.name
            )))
        }
        Codec::Raw => e.raw_len == e.stored_len,
        Codec::Lz77 => e.raw_len <= max_inflated(e.codec, e.stored_len),
        Codec::Deflate => {
            // A split body rebuilds to at most 4 times its size.
            let most = max_inflated(e.codec, e.stored_len);
            e.raw_len <= if split { most.saturating_mul(4) } else { most }
        }
        Codec::Other(c) => {
            return Err(Error::unsupported(format!(
                "section {}: codec {c} from a newer revision",
                e.name
            )))
        }
    };
    if e.raw_len > MAX_SECTION || !plausible {
        return Err(Error::format(format!(
            "section {}: {} bytes stored cannot hold {} bytes",
            e.name, e.stored_len, e.raw_len
        )));
    }
    // Lengths must fit the address space (32-bit targets too).
    let fit = |v: u64| usize::try_from(v).map_err(|_| Error::format("section larger than memory"));
    let (raw_len, body_max) = (fit(e.raw_len)?, fit(body_max)?);
    let start = fit(e.offset)?;
    let end = start
        .checked_add(fit(e.stored_len)?)
        .ok_or_else(|| Error::format("section offset overflow"))?;
    let stored = bytes
        .get(start..end)
        .ok_or_else(|| Error::truncated(format!("section {} runs past end of file", e.name)))?;
    let ctx = |err: Error| err.context(format!("section {}", e.name));
    let body = match e.codec {
        Codec::Raw => Cow::Borrowed(stored),
        Codec::Lz77 => Cow::Owned(lz77::decompress(stored, raw_len).map_err(ctx)?),
        Codec::Deflate => {
            let (out, used) = codec::inflate(stored, body_max)
                .map_err(|x| Error::format(format!("section {}: {}", e.name, x.0)))?;
            if used != stored.len() {
                return Err(Error::format(format!(
                    "section {}: {} bytes after the compressed data",
                    e.name,
                    stored.len() - used
                )));
            }
            Cow::Owned(out)
        }
        // Refused above; an error rather than a panic if that ever changes.
        Codec::Other(c) => {
            return Err(Error::format(format!("section {}: unknown compression {c}", e.name)));
        }
    };
    let raw = if split {
        split::join(&body, raw_len).map_err(ctx)?
    } else {
        body.into_owned()
    };
    if raw.len() as u64 != e.raw_len {
        return Err(Error::format(format!(
            "section {}: {} bytes, directory says {}",
            e.name,
            raw.len(),
            e.raw_len
        )));
    }
    let ok = crc32(0, &raw) == e.crc;
    Ok((raw, ok))
}

impl<'a> Container<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Container<'a>> {
        if !looks_native(bytes) {
            return Err(Error::new(ErrorKind::Format, "not a native .cad file"));
        }
        let front = FileHeader::decode(bytes);
        let back = if bytes.len() >= 2 * HEADER_LEN {
            FileHeader::decode(&bytes[bytes.len() - HEADER_LEN..])
        } else {
            None
        };
        let mut notes = Vec::new();
        if front.is_none() {
            notes.push("file header is damaged; using the trailer copy".to_string());
        }
        // Prefer the newer generation; fall back to the other copy if its
        // directory does not check out.
        let mut candidates: Vec<(FileHeader, &'static str)> = Vec::new();
        match (front, back) {
            (Some(f), Some(b)) if b.generation > f.generation => {
                candidates.push((b, "trailer"));
                candidates.push((f, "header"));
            }
            (Some(f), Some(b)) => {
                candidates.push((f, "header"));
                if b != f {
                    candidates.push((b, "trailer"));
                }
            }
            (Some(f), None) => candidates.push((f, "header")),
            (None, Some(b)) => candidates.push((b, "trailer")),
            (None, None) => {
                return Err(Error::format("both copies of the file header are damaged"))
            }
        }
        for (h, _) in &candidates {
            if !(OLDEST_MAJOR..=FORMAT_MAJOR).contains(&h.major) {
                return Err(Error::new(
                    ErrorKind::UnsupportedVersion,
                    format!(
                        "native format {}.{}; this build reads {OLDEST_MAJOR}.x to {FORMAT_MAJOR}.x",
                        h.major, h.minor
                    ),
                ));
            }
        }
        // First pass: a copy counts only if its directory and the change set
        // it was written for are intact. Second pass (every copy failed that):
        // accept an intact directory; the damaged change set is then reported
        // when read, and everything before it still loads.
        let mut last_err: Option<Error> = None;
        let mut dir_damaged = false;
        for strict in [true, false] {
            for (h, source) in &candidates {
                let entries = Self::directory(bytes, h);
                if entries.is_err() && strict {
                    dir_damaged = true;
                }
                let checked = entries.and_then(|entries| {
                    if strict {
                        Self::check_latest_journal(bytes, h, &entries)?;
                    }
                    Ok(entries)
                });
                match checked {
                    Ok(entries) => {
                        let newer = h.major == FORMAT_MAJOR && h.minor > FORMAT_MINOR;
                        if newer {
                            notes.push(format!(
                                "written by a newer format revision {}.{}; unknown data is skipped",
                                h.major, h.minor
                            ));
                        }
                        if let Some(e) = last_err.take() {
                            let same_state = candidates.iter().any(|(o, s)| {
                                s != source && o.same_state(h) && o.dir_offset != h.dir_offset
                            });
                            notes.push(if dir_damaged && same_state {
                                format!(
                                    "one copy of the directory is damaged ({e}); used the other"
                                )
                            } else if strict {
                                format!(
                                    "newer save was incomplete ({e}); opened the previous state"
                                )
                            } else {
                                format!("latest change set is damaged ({e})")
                            });
                        }
                        return Ok(Container {
                            bytes: Cow::Borrowed(bytes),
                            header: *h,
                            entries,
                            source,
                            notes,
                        });
                    }
                    Err(e) => {
                        if last_err.is_none() {
                            last_err = Some(e);
                        }
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| Error::format("no usable directory")))
    }

    /// Read and check the directory a header copy points at.
    pub fn directory(bytes: &[u8], h: &FileHeader) -> Result<Vec<DirEntry>> {
        let start = h.dir_offset as usize;
        let end = start
            .checked_add(h.dir_len as usize)
            .ok_or_else(|| Error::format("directory offset overflow"))?;
        let dir = bytes
            .get(start..end)
            .filter(|_| h.dir_offset <= usize::MAX as u64)
            .ok_or_else(|| Error::truncated("directory runs past end of file"))?;
        if crc32(0, dir) != h.dir_crc {
            return Err(Error::format("directory CRC mismatch"));
        }
        decode_directory_for(h.major, dir)
    }

    /// An incremental save is only complete if the change set it wrote is
    /// intact; otherwise this copy of the header must not be trusted.
    fn check_latest_journal(bytes: &[u8], h: &FileHeader, entries: &[DirEntry]) -> Result<()> {
        let name = format!("Journal.{}", h.generation);
        if let Some(e) = entries.iter().rev().find(|e| e.name == name) {
            match read_entry(bytes, e) {
                Ok((_, true)) => {}
                Ok((_, false)) => return Err(Error::format(format!("{name} fails its CRC"))),
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    /// The container of an owned buffer (`NativeFile::open`).
    pub fn parse_owned(bytes: Vec<u8>) -> Result<Container<'static>> {
        let Container {
            header,
            entries,
            source,
            notes,
            ..
        } = Container::parse(&bytes)?;
        Ok(Container {
            bytes: Cow::Owned(bytes),
            header,
            entries,
            source,
            notes,
        })
    }

    pub fn entry(&self, name: &str) -> Option<&DirEntry> {
        // Later entries win, so a journal can shadow a section by name.
        self.entries.iter().rev().find(|e| e.name == name)
    }

    /// Read and decompress a section (see [`read_entry`]).
    pub fn read(&self, e: &DirEntry) -> Result<(Vec<u8>, bool)> {
        read_entry(&self.bytes, e)
    }
}

/// One section waiting to be encoded.
struct Pending {
    name: String,
    raw: Vec<u8>,
    records: bool,
}

/// Builds a complete file in memory. Sections are encoded in parallel
/// when `finish` runs.
pub struct Builder {
    pending: Vec<Pending>,
    enc: Encoding,
    recovery: bool,
}

impl Builder {
    pub fn new(enc: Encoding, recovery: bool) -> Builder {
        Builder {
            pending: Vec::new(),
            enc,
            recovery,
        }
    }

    /// A section of opaque bytes.
    pub fn section(&mut self, name: &str, raw: Vec<u8>) {
        self.pending.push(Pending {
            name: name.to_string(),
            raw,
            records: false,
        });
    }

    /// A section of length-prefixed tagged records (may be split).
    pub fn records(&mut self, name: &str, raw: Vec<u8>) {
        self.pending.push(Pending {
            name: name.to_string(),
            raw,
            records: true,
        });
    }

    /// The records of every section so far, in order (to hash them).
    pub fn raw_sections(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.pending
            .iter()
            .map(|p| (p.name.as_str(), p.raw.as_slice()))
    }

    /// Encode the sections, append both copies of the directory and the
    /// trailer, and fill in the front header.
    pub fn finish(self, generation: u64, flags: u32) -> Vec<u8> {
        let encoded = encode_all(&self.pending, self.enc);
        let mut buf = vec![0u8; HEADER_LEN];
        let mut entries = Vec::with_capacity(encoded.len() + 1);
        for (p, x) in self.pending.iter().zip(encoded) {
            entries.push(DirEntry {
                name: p.name.clone(),
                codec: x.codec,
                offset: buf.len() as u64,
                stored_len: x.stored.len() as u64,
                raw_len: p.raw.len() as u64,
                crc: x.crc,
                flags: x.flags,
            });
            buf.extend_from_slice(&x.stored);
        }
        let recovery = if self.recovery {
            super::recovery::build(&buf, &entries)
        } else {
            None
        };
        let Some(data) = recovery else {
            return finish_file(buf, &entries, generation, flags);
        };
        // The recovery data goes between the two copies of the directory,
        // so damage at the end of the file (a copy cut short) leaves the
        // first copy, and damage before it leaves the second. The first
        // copy lists the recovery section, which follows it, so its place
        // depends on the directory's own length: settle that first.
        entries.push(DirEntry {
            name: super::recovery::SECTION.to_string(),
            codec: Codec::Raw,
            offset: 0,
            stored_len: data.len() as u64,
            raw_len: data.len() as u64,
            crc: crc32(0, &data),
            flags: 0,
        });
        let dir = place_before(buf.len() as u64, &mut entries);
        let h = FileHeader {
            major: FORMAT_MAJOR,
            minor: FORMAT_MINOR,
            flags,
            generation,
            dir_offset: buf.len() as u64,
            dir_len: dir.len() as u32,
            dir_crc: crc32(0, &dir),
        };
        buf.extend_from_slice(&dir);
        buf.extend_from_slice(&data);
        let trailer = FileHeader {
            dir_offset: buf.len() as u64,
            ..h
        };
        buf.extend_from_slice(&dir);
        buf.extend_from_slice(&trailer.encode());
        buf[..HEADER_LEN].copy_from_slice(&h.encode());
        buf
    }
}

/// The directory to write at `at`, when the last entry is a section that
/// directly follows it: that entry's offset is set to the end of the
/// directory. A compressed directory's length need not settle (its length
/// moves the offset, which changes what it compresses to), so after a few
/// tries the directory is stored instead, whose length only grows.
fn place_before(at: u64, entries: &mut [DirEntry]) -> Vec<u8> {
    let Some(last) = entries.len().checked_sub(1) else {
        return encode_directory_for(FORMAT_MAJOR, entries);
    };
    let mut offset = at;
    let mut tries = 0u32;
    loop {
        entries[last].offset = offset;
        let dir = if tries < 8 {
            encode_directory_for(FORMAT_MAJOR, entries)
        } else {
            let mut d = vec![Codec::Raw.byte()];
            d.extend_from_slice(&encode_directory(entries));
            d
        };
        let end = at + dir.len() as u64;
        if end == offset {
            return dir;
        }
        offset = end;
        // The stored directory's length only grows with its offset, and is
        // bounded, so this settles.
        tries = tries.saturating_add(1);
    }
}

/// Append the directory twice and the trailer to `buf` (which starts with
/// room for the front header), and fill in the front header. The front
/// header points at the first copy of the directory, the trailer at the
/// second.
pub fn finish_file(mut buf: Vec<u8>, entries: &[DirEntry], generation: u64, flags: u32) -> Vec<u8> {
    let h = FileHeader {
        major: FORMAT_MAJOR,
        minor: FORMAT_MINOR,
        flags,
        generation,
        ..Default::default()
    };
    let tail = directory_and_trailer(buf.len() as u64, entries, h);
    buf.extend_from_slice(&tail.bytes);
    buf[..HEADER_LEN].copy_from_slice(&tail.front);
    buf
}

/// The end of a file whose sections end at `base`: the directory, then the
/// trailer; and the front header to go with it.
pub struct Tail {
    pub bytes: Vec<u8>,
    pub front: [u8; HEADER_LEN],
}

/// `h` gives the version, flags and generation (its directory fields are
/// filled in). From format 2.0 the directory is written twice, the front
/// header pointing at the first copy and the trailer at the second; a 1.x
/// file keeps one copy, as 1.x readers expect.
pub fn directory_and_trailer(base: u64, entries: &[DirEntry], h: FileHeader) -> Tail {
    let dir = encode_directory_for(h.major, entries);
    let h = FileHeader {
        dir_offset: base,
        dir_len: dir.len() as u32,
        dir_crc: crc32(0, &dir),
        ..h
    };
    let mirrored = h.major >= 2;
    let trailer = FileHeader {
        dir_offset: if mirrored {
            base + dir.len() as u64
        } else {
            base
        },
        ..h
    };
    let mut bytes = Vec::with_capacity(2 * dir.len() + HEADER_LEN);
    bytes.extend_from_slice(&dir);
    if mirrored {
        bytes.extend_from_slice(&dir);
    }
    bytes.extend_from_slice(&trailer.encode());
    Tail {
        bytes,
        front: h.encode(),
    }
}

/// Encode sections on all available cores, keeping their order.
fn encode_all(pending: &[Pending], enc: Encoding) -> Vec<Encoded> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let one = |p: &Pending| encode_section(&p.raw, p.records, enc);
    let big = pending.iter().filter(|p| p.raw.len() > 16 * 1024).count();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(big);
    if threads < 2 || enc.codec == Codec::Raw {
        return pending.iter().map(one).collect();
    }
    // Largest first, so one big section does not finish last on its own.
    let mut order: Vec<usize> = (0..pending.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(pending[i].raw.len()));
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<Encoded>>> = Mutex::new(vec![None; pending.len()]);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&i) = order.get(k) else { break };
                let x = one(&pending[i]);
                if let Ok(mut v) = out.lock() {
                    v[i] = Some(x);
                }
            });
        }
    });
    let done = out.into_inner().unwrap_or_else(|e| e.into_inner());
    done.into_iter()
        .zip(pending)
        // A worker that failed to store its result: encode it here.
        .map(|(x, p)| x.unwrap_or_else(|| one(p)))
        .collect()
}

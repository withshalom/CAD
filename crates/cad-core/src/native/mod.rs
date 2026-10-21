//! The native file format (`.cad`).
//!
//! A drawing is stored as tagged records in the style of binary DXF (group
//! codes from the published DXF reference, typed values), inside a
//! container modelled on DWG R2004: named sections, each compressed and
//! checked by CRC-32, a section directory, an object map for random
//! access, and two copies of the file header. On top of that it adds what
//! neither DWG nor DXF has: an append-only change journal (incremental
//! saves that survive a crash), preserved bytes for DWG objects the core
//! does not decode yet, a preview, and from format 2.0 a schema that
//! describes every record, a SHA-256 hash chain over the snapshot and the
//! change sets, and recovery data that rebuilds damaged bytes.
//!
//! The byte-level specification is `docs/FORMAT.md`.
//!
//! ```no_run
//! let d = cad_core::open("part.dwg")?;
//! cad_core::native::write_file(&d, "part.cad", &Default::default())?;
//! let again = cad_core::native::read_file("part.cad")?;
//! // Or open once and look things up without loading everything:
//! let f = cad_core::native::NativeFile::open("part.cad")?;
//! let props = f.properties()?;
//! # Ok::<(), cad_core::Error>(())
//! ```

mod container;
mod objects;
mod records;
mod recovery;
pub mod schema;
mod split;
mod tags;

pub mod audit;
pub mod lock;

pub use crate::codec::Level;
pub use container::{
    is_native, looks_native, FLAG_ENCRYPTED, FLAG_HAS_JOURNAL, FLAG_SIGNED, FORMAT_MAJOR,
    FORMAT_MINOR, MAGIC, OLDEST_MAJOR,
};

use container::{Builder, Codec, Container, DirEntry, Encoding, FileHeader};
use records as rec;
use tags::{get_len, get_varint, put_varint, TagReader, TagWriter};

use crate::codec::sha256::{hex, Sha256};
use crate::dwg::DwgVersion;
use crate::error::{Error, ErrorKind, Result, ResultExt};
use crate::model::*;
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// File extension of native drawings.
pub const EXTENSION: &str = "cad";

/// Target uncompressed size of one object page. Records are never split,
/// so a single large record can make a page bigger.
const PAGE_TARGET: usize = 64 * 1024;
/// Previews larger than this are left out.
const PREVIEW_LIMIT: usize = 4 * 1024 * 1024;

const SEC_META: &str = "Meta";
const SEC_HEADER: &str = "Header";
const SEC_CLASSES: &str = "Classes";
const SEC_SCHEMA: &str = "Schema";
const SEC_OBJECT_MAP: &str = "ObjectMap";
const SEC_OPAQUE: &str = "DwgPreserved";
const SEC_PREVIEW: &str = "Preview";
const SEC_DIGEST: &str = "Digest";
const SEC_RECOVERY: &str = recovery::SECTION;
const OBJECTS_PREFIX: &str = "Objects.";
const JOURNAL_PREFIX: &str = "Journal.";

#[derive(Debug, Clone)]
pub struct WriteOptions {
    /// Compress sections. Off is only useful for looking at the format
    /// with a hex viewer.
    pub compress: bool,
    /// DEFLATE effort: `Fast`, `Default` or `Best`.
    pub level: Level,
    /// Store an SVG preview of model space.
    pub preview: bool,
    /// Store the original DWG bytes of objects the core cannot decode.
    pub keep_dwg_bytes: bool,
    /// Store recovery data (about 1/16 of the file) that rebuilds damaged
    /// bytes when the file is read.
    pub recovery: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            compress: true,
            level: Level::Default,
            preview: true,
            keep_dwg_bytes: true,
            recovery: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Framing helpers
// ---------------------------------------------------------------------------

fn put_record(out: &mut Vec<u8>, w: TagWriter) {
    put_varint(out, w.buf.len() as u64);
    out.extend_from_slice(&w.buf);
}

/// Split a section into its length-prefixed records.
fn records(data: &[u8]) -> Result<Vec<&[u8]>> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < data.len() {
        let n = get_len(data, &mut p, "record")?;
        out.push(&data[p..p + n]);
        p += n;
    }
    Ok(out)
}

fn now_unix() -> u64 {
    crate::now_unix().max(0) as u64
}

fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::new(ErrorKind::Io, format!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Hash chain
// ---------------------------------------------------------------------------

/// SHA-256 of a snapshot: every section before `Digest`, in directory
/// order, each as varint name length, name, varint data length, data (its
/// records, as decoded).
fn snapshot_hash<'x>(sections: impl Iterator<Item = (&'x str, &'x [u8])>) -> [u8; 32] {
    let mut s = Sha256::new();
    let mut head = Vec::with_capacity(32);
    for (name, data) in sections {
        head.clear();
        put_varint(&mut head, name.len() as u64);
        head.extend_from_slice(name.as_bytes());
        put_varint(&mut head, data.len() as u64);
        s.update(&head);
        s.update(data);
    }
    s.finish()
}

/// The next link of the chain: SHA-256 of the previous value followed by a
/// change set's records.
fn chain_next(previous: &[u8; 32], journal: &[u8]) -> [u8; 32] {
    let mut s = Sha256::new();
    s.update(previous);
    s.update(journal);
    s.finish()
}

fn digest_section(snapshot: &[u8; 32]) -> Vec<u8> {
    let mut w = TagWriter::new();
    w.str(9, "$CAD_HASH");
    w.str(1, "SHA-256");
    w.str(9, "$CAD_SNAPSHOT");
    w.bytes(310, snapshot);
    w.buf
}

fn read_digest(data: &[u8]) -> Result<[u8; 32]> {
    let mut r = TagReader::new(data);
    let mut algorithm = String::new();
    let mut snapshot = None;
    while let Some(t) = r.next()? {
        if let (9, tags::Value::Str(n)) = (t.code, &t.value) {
            match n.as_str() {
                "$CAD_HASH" => algorithm = r.str(1)?,
                "$CAD_SNAPSHOT" => snapshot = Some(r.bytes(310)?),
                _ => {}
            }
        }
    }
    if algorithm != "SHA-256" {
        return Err(Error::unsupported(format!(
            "digest algorithm {algorithm:?}"
        )));
    }
    snapshot
        .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
        .ok_or_else(|| Error::format("digest without a 32-byte snapshot hash"))
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

fn meta_section(d: &Drawing, objects: usize, pages: usize) -> Vec<u8> {
    let mut w = TagWriter::new();
    let var = |w: &mut TagWriter, n: &str| w.str(9, n);
    var(&mut w, "$CAD_FORMAT");
    w.str(1, &format!("{FORMAT_MAJOR}.{FORMAT_MINOR}"));
    var(&mut w, "$CAD_WRITER");
    w.str(1, &format!("cad-core {}", crate::VERSION));
    var(&mut w, "$CAD_SOURCE_VERSION");
    w.str(1, d.version.magic());
    var(&mut w, "$CAD_MAINTVER");
    w.u8(70, d.maintenance_version);
    var(&mut w, "$CAD_CODEPAGE");
    w.u16(70, d.codepage);
    var(&mut w, "$CAD_SAVED");
    w.i64(160, now_unix() as i64);
    var(&mut w, "$CAD_OBJECTS");
    w.u64(160, objects as u64);
    var(&mut w, "$CAD_PAGES");
    w.u64(160, pages as u64);
    w.buf
}

#[derive(Debug, Default, Clone)]
struct Meta {
    source: Option<DwgVersion>,
    maintenance: u8,
    codepage: u16,
    writer: String,
    saved: Option<u64>,
    objects: Option<u64>,
}

fn read_meta(data: &[u8]) -> Result<Meta> {
    let mut r = TagReader::new(data);
    let mut m = Meta::default();
    while let Some(t) = r.next()? {
        let tags::Value::Str(name) = t.value else {
            continue;
        };
        if t.code != 9 {
            continue;
        }
        match name.as_str() {
            "$CAD_WRITER" => m.writer = r.str(1)?,
            "$CAD_SOURCE_VERSION" => {
                let tag = r.str(1)?;
                m.source = DwgVersion::from_tag(&tag);
            }
            "$CAD_MAINTVER" => m.maintenance = r.u8(70)?,
            "$CAD_CODEPAGE" => m.codepage = r.u16(70)?,
            "$CAD_SAVED" => m.saved = Some(r.i64(160)? as u64),
            "$CAD_OBJECTS" => m.objects = Some(r.u64(160)?),
            _ => {}
        }
    }
    Ok(m)
}

/// Serialize a drawing as a complete file with the given generation.
fn build(d: &Drawing, opts: &WriteOptions, generation: u64) -> Vec<u8> {
    // Object pages and the map (handle → page, offset in page).
    let mut pages: Vec<Vec<u8>> = vec![Vec::new()];
    let mut map: Vec<(u64, usize, usize)> = Vec::with_capacity(d.objects.len());
    let mut opaque = Vec::new();
    for (key, o) in &d.objects {
        let mut w = TagWriter::new();
        rec::write_object(&mut w, *key, o);
        if pages
            .last()
            .map(|p| !p.is_empty() && p.len() + w.buf.len() > PAGE_TARGET)
            == Some(true)
        {
            pages.push(Vec::new());
        }
        let page_no = pages.len() - 1;
        let page = pages.last_mut().expect("page");
        map.push((key.0, page_no, page.len()));
        put_record(page, w);
        if opts.keep_dwg_bytes {
            if let Some(raw) = &o.raw {
                let mut w = TagWriter::new();
                rec::write_raw(&mut w, *key, raw);
                put_record(&mut opaque, w);
            }
        }
    }

    let mut header = TagWriter::new();
    rec::write_header(&mut header, &d.header);

    let mut classes = Vec::new();
    for k in &d.classes {
        let mut w = TagWriter::new();
        rec::write_class(&mut w, k);
        put_record(&mut classes, w);
    }

    let mut schema = Vec::new();
    for r in schema::write_section() {
        put_varint(&mut schema, r.len() as u64);
        schema.extend_from_slice(&r);
    }

    let mut objmap = Vec::new();
    put_varint(&mut objmap, map.len() as u64);
    let mut prev = 0u64;
    for (h, page, off) in &map {
        put_varint(&mut objmap, h.wrapping_sub(prev));
        put_varint(&mut objmap, *page as u64);
        put_varint(&mut objmap, *off as u64);
        prev = *h;
    }

    let enc = Encoding::current(opts.compress, opts.level);
    let mut b = Builder::new(enc, opts.recovery);
    b.section(SEC_META, meta_section(d, d.objects.len(), pages.len()));
    b.section(SEC_HEADER, header.buf);
    b.records(SEC_CLASSES, classes);
    b.records(SEC_SCHEMA, schema);
    let page_count = pages.len();
    for (i, p) in pages.into_iter().enumerate() {
        b.records(&format!("{OBJECTS_PREFIX}{i}"), p);
    }
    debug_assert!(page_count >= 1);
    b.section(SEC_OBJECT_MAP, objmap);
    if !opaque.is_empty() {
        b.records(SEC_OPAQUE, opaque);
    }
    if opts.preview {
        let scene = crate::render::model_space(d);
        if !scene.items.is_empty() {
            let svg = crate::export::svg::to_svg(
                &scene,
                &crate::export::svg::SvgOptions {
                    dark_background: false,
                    width_px: 512.0,
                    ..Default::default()
                },
            );
            if svg.len() <= PREVIEW_LIMIT {
                b.section(SEC_PREVIEW, svg.into_bytes());
            }
        }
    }
    let snapshot = snapshot_hash(b.raw_sections());
    b.section(SEC_DIGEST, digest_section(&snapshot));
    b.finish(generation, 0)
}

/// Serialize a drawing to bytes (generation 1).
pub fn to_bytes(d: &Drawing, opts: &WriteOptions) -> Vec<u8> {
    build(d, opts, 1)
}

/// Generation of the native file at `path`, if it is one.
fn existing_generation(path: &Path) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; container::HEADER_LEN];
    f.read_exact(&mut head).ok()?;
    let front = FileHeader::decode(&head);
    let len = f.seek(SeekFrom::End(0)).ok()?;
    let back = if len >= 2 * container::HEADER_LEN as u64 {
        f.seek(SeekFrom::End(-(container::HEADER_LEN as i64)))
            .ok()?;
        f.read_exact(&mut head).ok()?;
        FileHeader::decode(&head)
    } else {
        None
    };
    [front, back].iter().flatten().map(|h| h.generation).max()
}

/// Flush the directory entry after a rename, where the platform allows
/// opening a directory (Unix); elsewhere this is a no-op.
fn sync_parent(path: &Path) {
    if let Some(dir) = path.parent() {
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        if let Ok(f) = std::fs::File::open(dir) {
            let _ = f.sync_all();
        }
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".saving");
    path.with_file_name(name)
}

/// Write a full snapshot, merging any journal. The file is written beside
/// the target and renamed over it, so a failed save never damages the
/// previous file. Returns the new generation.
pub fn write_file(d: &Drawing, path: impl AsRef<Path>, opts: &WriteOptions) -> Result<u64> {
    let path = path.as_ref();
    let generation = existing_generation(path).unwrap_or(0).saturating_add(1);
    let bytes = build(d, opts, generation);
    let tmp = temp_path(path);
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        // Closed before the rename. (On WebAssembly, which has no files, `File`
        // is an empty stand-in with nothing to close.)
        #[allow(clippy::drop_non_drop)]
        drop(f);
        std::fs::rename(&tmp, path)?;
        sync_parent(path);
        Ok(())
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io_err(path, e)
    })?;
    Ok(generation)
}

// ---------------------------------------------------------------------------
// Journal (incremental save)
// ---------------------------------------------------------------------------

/// One change in a change set.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Add or replace an object (by its handle).
    Put(Box<Object>),
    /// Remove an object.
    Delete(Handle),
    /// Replace the header variables.
    Header(Box<HeaderVars>),
}

/// A group of changes saved together.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChangeSet {
    pub author: String,
    /// Unix seconds; 0 means "now" when appending.
    pub time: u64,
    pub changes: Vec<Change>,
}

mod op {
    pub const PUT: i16 = 1;
    pub const DELETE: i16 = 2;
    pub const HEADER: i16 = 3;
    pub const RAW: i16 = 4;
}

/// A change set as the given major version writes it. `previous` is the
/// hash chain's value before it (2.0 files).
fn encode_journal(
    set: &ChangeSet,
    generation: u64,
    major: u16,
    previous: Option<&[u8; 32]>,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut w = TagWriter::for_major(major);
    w.str(9, "$CAD_AUTHOR");
    w.str(1, &set.author);
    w.str(9, "$CAD_TIME");
    w.i64(
        160,
        if set.time == 0 { now_unix() } else { set.time } as i64,
    );
    w.str(9, "$CAD_GENERATION");
    w.u64(160, generation);
    if let Some(p) = previous {
        w.str(9, "$CAD_PREVIOUS");
        w.bytes(310, p);
    }
    put_record(&mut out, w);
    for c in &set.changes {
        let mut w = TagWriter::for_major(major);
        match c {
            Change::Put(o) => {
                w.i16(rec::code::OP, op::PUT);
                rec::write_object(&mut w, o.handle, o);
                put_record(&mut out, w);
                if let Some(raw) = &o.raw {
                    let mut w = TagWriter::for_major(major);
                    w.i16(rec::code::OP, op::RAW);
                    rec::write_raw(&mut w, o.handle, raw);
                    put_record(&mut out, w);
                }
            }
            Change::Delete(h) => {
                w.i16(rec::code::OP, op::DELETE);
                w.handle(5, *h);
                put_record(&mut out, w);
            }
            Change::Header(hv) => {
                w.i16(rec::code::OP, op::HEADER);
                rec::write_header(&mut w, hv);
                put_record(&mut out, w);
            }
        }
    }
    out
}

fn decode_journal(data: &[u8]) -> Result<ChangeSet> {
    decode_journal_checked(data).map(|(set, _)| set)
}

/// The hash chain value a change set says came before it, if it has one.
fn journal_previous(data: &[u8]) -> Option<[u8; 32]> {
    let first = records(data).ok()?.into_iter().next()?;
    let mut r = TagReader::new(first);
    while let Ok(Some(t)) = r.next() {
        if let (9, tags::Value::Str(n)) = (t.code, &t.value) {
            if n == "$CAD_PREVIOUS" {
                return r.bytes(310).ok()?.as_slice().try_into().ok();
            }
        }
    }
    None
}

/// [`decode_journal`], also listing object records and header streams in
/// it that only partly decoded (their undecoded parts are kept).
fn decode_journal_checked(data: &[u8]) -> Result<(ChangeSet, Vec<String>)> {
    let recs = records(data)?;
    let mut set = ChangeSet::default();
    let mut partial = Vec::new();
    let Some((first, rest)) = recs.split_first() else {
        return Ok((set, partial));
    };
    let mut r = TagReader::new(first);
    while let Some(t) = r.next()? {
        if let (9, tags::Value::Str(n)) = (t.code, &t.value) {
            match n.as_str() {
                "$CAD_AUTHOR" => set.author = r.str(1)?,
                "$CAD_TIME" => set.time = r.i64(160)? as u64,
                _ => {}
            }
        }
    }
    for data in rest {
        // Each op record starts with its op tag (code, type, int16 = 5 bytes).
        let mut r = TagReader::new(data);
        let kind = r.i16(rec::code::OP)?;
        let body = &data[5..];
        match kind {
            op::PUT => {
                let (key, o, problem) = rec::read_object_checked(body)?;
                if let Some(m) = problem {
                    partial.push(format!("{key}: {m}"));
                }
                set.changes.push(Change::Put(Box::new(o)));
            }
            op::DELETE => set.changes.push(Change::Delete(r.handle(5)?)),
            op::HEADER => {
                let (h, problem) = rec::read_header_checked(body)?;
                if let Some(m) = problem {
                    partial.push(format!("header: {m}"));
                }
                set.changes.push(Change::Header(Box::new(h)));
            }
            op::RAW => {
                let (h, raw) = rec::read_raw(body)?;
                for c in set.changes.iter_mut().rev() {
                    if let Change::Put(o) = c {
                        if o.handle == h {
                            o.raw = Some(raw);
                            break;
                        }
                    }
                }
            }
            _ => {} // op from a newer revision
        }
    }
    Ok((set, partial))
}

/// Apply a change set to a drawing in memory.
pub fn apply(d: &mut Drawing, set: &ChangeSet) {
    apply_without_index(d, set);
    d.rebuild_index();
}

/// [`apply`] without rebuilding the lookup index; the reader applies every
/// change set first and rebuilds once.
fn apply_without_index(d: &mut Drawing, set: &ChangeSet) {
    for c in &set.changes {
        match c {
            Change::Put(o) => {
                d.objects.insert(o.handle, (**o).clone());
            }
            Change::Delete(h) => {
                d.objects.remove(h);
            }
            Change::Header(h) => d.header = (**h).clone(),
        }
    }
}

/// Append a change set to an existing file without rewriting it (an
/// incremental save). The new data, directory and trailer are appended
/// and flushed before the front header is updated, so a crash at any
/// point leaves either the old or the new state readable. The change set
/// is written in the file's own format version, so a 1.x file stays
/// readable by 1.x readers. Returns the new generation.
pub fn append_changes(path: impl AsRef<Path>, set: &ChangeSet) -> Result<u64> {
    let path = path.as_ref();
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| io_err(path, e))?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(|e| io_err(path, e))?;
    let c = Container::parse(&bytes).ctx(path.display())?;
    // Appending after a damaged change set would hide the new one too:
    // readers stop at the first change set they cannot read. On the way,
    // follow the hash chain to its current end.
    if c.header.major == 1 && c.header.minor == 0 {
        // 1.0 gives lineweight 0 a meaning of its own (see
        // `upgrade_1_0_lineweights`), which new records would inherit.
        return Err(Error::format(format!(
            "{}: a format 1.0 file; save a full copy (compact) before saving changes",
            path.display()
        )));
    }
    let mut chain = c
        .entry(SEC_DIGEST)
        .and_then(|e| c.read(e).ok())
        .filter(|(_, ok)| *ok)
        .and_then(|(d, _)| read_digest(&d).ok());
    if c.header.major >= 2 && chain.is_none() {
        return Err(Error::format(format!(
            "{}: the snapshot's digest is missing or damaged; save a full copy (compact) \
             before saving changes",
            path.display()
        )));
    }
    for (g, e) in journal_sections(&c) {
        let data = match c.read(e) {
            Ok((data, true)) if decode_journal(&data).is_ok() => data,
            _ => {
                return Err(Error::format(format!(
                "{}: change set {g} is damaged; save a full copy (compact) before saving changes",
                path.display()
            )))
            }
        };
        chain = chain.map(|prev| chain_next(&prev, &data));
    }
    let generation = c
        .header
        .generation
        .checked_add(1)
        .ok_or_else(|| Error::format("generation counter exhausted; save a full copy"))?;
    let raw = encode_journal(set, generation, c.header.major, chain.as_ref());
    let name = format!("{JOURNAL_PREFIX}{generation}");
    let x = container::encode_section(&raw, true, Encoding::for_major(c.header.major));
    let mut entries = c.entries.clone();
    let base = bytes.len() as u64;
    entries.push(DirEntry {
        name,
        codec: x.codec,
        offset: base,
        stored_len: x.stored.len() as u64,
        raw_len: raw.len() as u64,
        crc: x.crc,
        flags: x.flags,
    });
    let tail = container::directory_and_trailer(
        base + x.stored.len() as u64,
        &entries,
        FileHeader {
            major: c.header.major,
            minor: c.header.minor,
            flags: c.header.flags | container::FLAG_HAS_JOURNAL,
            generation,
            ..Default::default()
        },
    );
    let mut run = || -> std::io::Result<()> {
        // The change set must be on disk before a header that points at it.
        f.seek(SeekFrom::End(0))?;
        f.write_all(&x.stored)?;
        f.sync_data()?;
        f.write_all(&tail.bytes)?;
        f.sync_all()?;
        f.seek(SeekFrom::Start(0))?;
        f.write_all(&tail.front)?;
        f.sync_all()
    };
    run().map_err(|e| io_err(path, e))?;
    Ok(generation)
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Numbered sections with a prefix, in number order. When a name appears
/// twice in the directory, the later entry wins.
fn numbered<'c>(c: &'c Container, prefix: &str) -> Vec<(u64, &'c DirEntry)> {
    let mut by_number: BTreeMap<u64, &DirEntry> = BTreeMap::new();
    for e in &c.entries {
        if let Some(n) = e
            .name
            .strip_prefix(prefix)
            .and_then(|n| n.parse::<u64>().ok())
        {
            by_number.insert(n, e);
        }
    }
    by_number.into_iter().collect()
}

fn page_sections<'c>(c: &'c Container) -> Vec<(u64, &'c DirEntry)> {
    numbered(c, OBJECTS_PREFIX)
}

fn journal_sections<'c>(c: &'c Container) -> Vec<(u64, &'c DirEntry)> {
    numbered(c, JOURNAL_PREFIX)
}

/// Collects diagnostics while a file is read, and whether anything was
/// damaged (so the reader knows to try the recovery data).
#[derive(Default)]
struct Diag {
    list: Vec<Diagnostic>,
    damaged: bool,
}

impl Diag {
    fn push(&mut self, severity: Severity, message: String) {
        if severity != Severity::Info {
            self.damaged = true;
        }
        self.list.push(Diagnostic { severity, message });
    }
}

fn read_section(c: &Container, e: &DirEntry, diag: &mut Diag) -> Result<Vec<u8>> {
    let r = c.read(e);
    match &r {
        Ok((_, false)) => diag.push(
            Severity::Warning,
            format!("section {}: CRC mismatch, data may be damaged", e.name),
        ),
        Err(_) => diag.damaged = true,
        _ => {}
    }
    r.map(|(d, _)| d)
}

/// Turn a section failure into a diagnostic.
fn soft<T>(what: &str, r: Result<T>, diag: &mut Diag) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            diag.push(Severity::Error, format!("section {what} unreadable: {e}"));
            None
        }
    }
}

/// [`records`], keeping the records before any framing damage.
fn records_lossy<'d>(data: &'d [u8], what: &str, diag: &mut Diag) -> Vec<&'d [u8]> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < data.len() {
        match get_len(data, &mut p, "record") {
            Ok(n) => {
                out.push(&data[p..p + n]);
                p += n;
            }
            Err(e) => {
                diag.push(
                    Severity::Error,
                    format!("section {what}: {e}; rest skipped"),
                );
                break;
            }
        }
    }
    out
}

fn required<'c>(c: &'c Container, name: &str) -> Result<&'c DirEntry> {
    c.entry(name)
        .ok_or_else(|| Error::format(format!("missing section {name}")))
}

fn read_objects_page(
    data: &[u8],
    page: u64,
    objects: &mut BTreeMap<Handle, Object>,
    diag: &mut Diag,
) {
    let mut p = 0usize;
    let mut bad = 0usize;
    let mut partial: Vec<String> = Vec::new();
    while p < data.len() {
        let n = match get_len(data, &mut p, "record") {
            Ok(n) => n,
            Err(e) => {
                diag.push(
                    Severity::Error,
                    format!("object page {page}: {e}; rest of page skipped"),
                );
                return;
            }
        };
        match rec::read_object_checked(&data[p..p + n]) {
            Ok((key, o, problem)) => {
                if let Some(m) = problem {
                    partial.push(format!("{key}: {m}"));
                }
                objects.insert(key, o);
            }
            Err(_) => bad += 1,
        }
        p += n;
    }
    if bad > 0 {
        diag.push(
            Severity::Warning,
            format!("object page {page}: {bad} unreadable records skipped"),
        );
    }
    if !partial.is_empty() {
        diag.push(
            Severity::Warning,
            format!(
                "object page {page}: {} records only partly decoded, the rest kept as is \
                 (first: {})",
                partial.len(),
                partial[0]
            ),
        );
    }
}

/// Load a whole drawing from a parsed container. The flag says whether
/// any part was damaged.
fn load(c: &Container) -> Result<(Drawing, bool)> {
    let mut diag = Diag::default();
    for n in &c.notes {
        diag.push(Severity::Warning, n.clone());
    }

    // Damage in one section is reported and the rest still loads.
    let meta = soft(
        SEC_META,
        required(c, SEC_META)
            .and_then(|e| read_section(c, e, &mut diag))
            .and_then(|d| read_meta(&d)),
        &mut diag,
    )
    .unwrap_or_default();
    let version = meta.source.unwrap_or_else(|| {
        diag.push(
            Severity::Warning,
            "source DWG version unknown; assuming AC1032".into(),
        );
        DwgVersion::R2018
    });
    let header = soft(
        SEC_HEADER,
        required(c, SEC_HEADER)
            .and_then(|e| read_section(c, e, &mut diag))
            .and_then(|d| rec::read_header_checked(&d)),
        &mut diag,
    )
    .map(|(h, problem)| {
        if let Some(m) = problem {
            diag.push(Severity::Warning, format!("section {SEC_HEADER}: {m}"));
        }
        h
    })
    .unwrap_or_else(|| HeaderVars {
        note: "header section unreadable".into(),
        ..Default::default()
    });

    let mut classes = Vec::new();
    let class_data = c.entry(SEC_CLASSES).and_then(|e| {
        let r = read_section(c, e, &mut diag);
        soft(SEC_CLASSES, r, &mut diag)
    });
    if let Some(data) = class_data {
        for r in records_lossy(&data, SEC_CLASSES, &mut diag) {
            match rec::read_class(r) {
                Ok(k) => classes.push(k),
                Err(e) => diag.push(Severity::Warning, format!("class record skipped: {e}")),
            }
        }
    }

    let mut objects = BTreeMap::new();
    let pages = page_sections(c);
    if pages.is_empty() {
        return Err(Error::format("no object pages"));
    }
    for (n, e) in &pages {
        match read_section(c, e, &mut diag) {
            Ok(data) => read_objects_page(&data, *n, &mut objects, &mut diag),
            Err(err) => diag.push(
                Severity::Error,
                format!("object page {n} unreadable: {err}"),
            ),
        }
    }
    if let Some(expected) = meta.objects {
        if expected != objects.len() as u64 {
            diag.push(
                Severity::Warning,
                format!(
                    "snapshot lists {expected} objects, {} were read",
                    objects.len()
                ),
            );
        }
    }

    let opaque = c.entry(SEC_OPAQUE).and_then(|e| {
        let r = read_section(c, e, &mut diag);
        soft(SEC_OPAQUE, r, &mut diag)
    });
    if let Some(data) = opaque {
        for r in records_lossy(&data, SEC_OPAQUE, &mut diag) {
            if let Ok((h, raw)) = rec::read_raw(r) {
                if let Some(o) = objects.get_mut(&h) {
                    o.raw = Some(raw);
                }
            }
        }
    }

    let mut d = Drawing {
        version,
        maintenance_version: meta.maintenance,
        codepage: meta.codepage,
        header,
        classes,
        objects: objects.into(),
        sections: Vec::new(),
        diagnostics: Vec::new(),
        index: DrawingIndex::default(),
    };

    let journals = journal_sections(c);
    for (g, e) in &journals {
        let set = read_section(c, e, &mut diag).and_then(|data| decode_journal_checked(&data));
        match set {
            Ok((set, partial)) => {
                if let Some(first) = partial.first() {
                    diag.push(
                        Severity::Warning,
                        format!(
                            "change set {g}: {} records only partly decoded, the rest kept as is \
                             (first: {first})",
                            partial.len()
                        ),
                    );
                }
                apply_without_index(&mut d, &set)
            }
            Err(err) => {
                diag.push(
                    Severity::Error,
                    format!("change set {g} unreadable, later changes skipped: {err}"),
                );
                break;
            }
        }
    }

    if is_1_0(c) && version <= DwgVersion::R14 {
        for o in d.objects.values_mut() {
            upgrade_1_0_lineweights(o);
        }
    }

    d.sections = c
        .entries
        .iter()
        .map(|e| SectionInfo {
            name: e.name.clone(),
            size: e.raw_len,
            pages: 1,
            compressed: e.codec != Codec::Raw,
            encrypted: e.flags & container::section_flag::ENCRYPTED != 0,
        })
        .collect();
    let damaged = diag.damaged;
    let mut diagnostics = diag.list;
    diagnostics.insert(
        0,
        Diagnostic {
            severity: Severity::Info,
            message: format!(
                "native .cad {}.{}, generation {} ({} copy), {} object pages, {} change sets; source {}",
                c.header.major,
                c.header.minor,
                c.header.generation,
                c.source,
                pages.len(),
                journals.len(),
                version.magic()
            ),
        },
    );
    d.diagnostics = diagnostics;
    // Files before 2.2 keep these objects as DWG bytes.
    crate::dwg::nongraphic::promote(&mut d);
    d.rebuild_index();
    Ok((d, damaged))
}

/// Format 1.0 wrote lineweight 0 for drawings from R13/R14 files, which
/// have no lineweights; 1.1 reads 0 as an explicit 0.00 mm. In such a file
/// it means what 1.1 makes of those drawings: by layer for entities, the
/// default weight for layers (FORMAT.md, revisions).
fn upgrade_1_0_lineweights(o: &mut Object) {
    if let Some(e) = o.entity.as_mut() {
        if e.lineweight == 0 {
            e.lineweight = Lineweight::CODE_BY_LAYER;
        }
    }
    if let ObjectKind::Layer(l) = &mut o.kind {
        if l.lineweight == 0 {
            l.lineweight = Lineweight::CODE_DEFAULT;
        }
    }
}

fn is_1_0(c: &Container) -> bool {
    c.header.major == 1 && c.header.minor == 0
}

/// True for a 1.0 file made from an R13/R14 drawing (see
/// [`upgrade_1_0_lineweights`]).
fn is_1_0_from_r14(c: &Container) -> bool {
    is_1_0(c)
        && required(c, SEC_META)
            .and_then(|e| c.read(e))
            .and_then(|(d, _)| read_meta(&d))
            .ok()
            .and_then(|m| m.source)
            .is_some_and(|v| v <= DwgVersion::R14)
}

/// The object map: (handle, page, offset of the record in the page), in
/// handle order as written. Found by binary search, or by a scan when a
/// damaged map is out of order.
struct ObjectMap {
    entries: Vec<(u64, u64, u64)>,
    sorted: bool,
}

impl ObjectMap {
    fn read(map: &[u8]) -> Result<ObjectMap> {
        let mut p = 0usize;
        let n = get_varint(map, &mut p)?;
        if n > map.len() as u64 {
            return Err(Error::format(format!("object map claims {n} entries")));
        }
        let mut entries = Vec::with_capacity(n as usize);
        let mut h = 0u64;
        let mut sorted = true;
        for i in 0..n {
            let delta = get_varint(map, &mut p)?;
            sorted &= i == 0 || (delta != 0 && h.checked_add(delta).is_some());
            h = h.wrapping_add(delta);
            let page = get_varint(map, &mut p)?;
            let off = get_varint(map, &mut p)?;
            entries.push((h, page, off));
        }
        Ok(ObjectMap { entries, sorted })
    }

    fn find(&self, h: u64) -> Option<(u64, u64)> {
        let at = if self.sorted {
            self.entries.binary_search_by_key(&h, |e| e.0).ok()
        } else {
            self.entries.iter().position(|e| e.0 == h)
        };
        at.map(|i| (self.entries[i].1, self.entries[i].2))
    }
}

/// A native file opened once, for looking things up without decoding the
/// whole drawing: one object, the properties, the preview, the file
/// structure. The object map is read the first time it is needed and kept.
pub struct NativeFile<'a> {
    c: Container<'a>,
    map: OnceLock<std::result::Result<ObjectMap, Error>>,
}

impl NativeFile<'static> {
    /// Read a file from disk and parse its header and directory.
    pub fn open(path: impl AsRef<Path>) -> Result<NativeFile<'static>> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| io_err(path, e))?;
        let c = Container::parse_owned(bytes).ctx(path.display())?;
        Ok(NativeFile {
            c,
            map: OnceLock::new(),
        })
    }
}

impl<'a> NativeFile<'a> {
    /// Parse the header and directory of a file held in memory.
    pub fn from_bytes(bytes: &'a [u8]) -> Result<NativeFile<'a>> {
        Ok(NativeFile {
            c: Container::parse(bytes)?,
            map: OnceLock::new(),
        })
    }

    /// Format version (major, minor).
    pub fn format(&self) -> (u16, u16) {
        (self.c.header.major, self.c.header.minor)
    }

    pub fn generation(&self) -> u64 {
        self.c.header.generation
    }

    /// Decode the whole drawing, every change set applied. Damage is
    /// reported in the drawing's diagnostics; when the file has recovery
    /// data that rebuilds the damaged bytes, the rebuilt file is read
    /// instead (and a diagnostic says so).
    pub fn drawing(&self) -> Result<Drawing> {
        let first = load(&self.c);
        let damaged = first.as_ref().map(|(_, d)| *d).unwrap_or(true);
        if damaged {
            if let Some(d) = self.load_repaired() {
                return Ok(d);
            }
        }
        first.map(|(d, _)| d)
    }

    /// The bytes with damaged blocks rebuilt from the recovery data, and
    /// how many blocks were rebuilt.
    fn repaired(&self) -> Option<(Vec<u8>, usize)> {
        let (data, r) = recovery_of(&self.c)?;
        let r = recovery::decode(&data)
            .ok()
            .filter(|x| x.start == r.0 && x.start + x.len == r.1)?;
        r.repair(&self.c.bytes)
    }

    fn load_repaired(&self) -> Option<Drawing> {
        let (fixed, blocks) = self.repaired()?;
        let c = Container::parse(&fixed).ok()?;
        let (mut d, still) = load(&c).ok()?;
        d.diagnostics.insert(
            1,
            Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "{blocks} damaged blocks were rebuilt from the recovery data{}; \
                     save a full copy to repair the file on disk",
                    if still { " (some damage remains)" } else { "" }
                ),
            },
        );
        Some(d)
    }

    /// Read a section; when it is damaged, read it from the bytes the
    /// recovery data rebuilds instead, if that works.
    fn read(&self, e: &DirEntry) -> Result<Vec<u8>> {
        match self.c.read(e) {
            Ok((d, true)) => Ok(d),
            first => {
                let fixed = self.repaired().and_then(|(fixed, _)| {
                    let c = Container::parse(&fixed).ok()?;
                    let e2 = c
                        .entries
                        .iter()
                        .find(|x| x.name == e.name && x.offset == e.offset)?;
                    c.read(e2).ok().filter(|(_, ok)| *ok).map(|(d, _)| d)
                });
                match fixed {
                    Some(d) => Ok(d),
                    None => first.map(|(d, _)| d),
                }
            }
        }
    }

    fn map(&self) -> Result<&ObjectMap> {
        self.map
            .get_or_init(|| ObjectMap::read(&self.read(required(&self.c, SEC_OBJECT_MAP)?)?))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Decode a single object: the object map locates its page, and only
    /// that page (plus any change sets) is decompressed.
    pub fn object(&self, handle: Handle) -> Result<Option<Object>> {
        let c = &self.c;
        let mut result = None;
        if let Some((page, off)) = self.map()?.find(handle.0) {
            let e = required(c, &format!("{OBJECTS_PREFIX}{page}"))?;
            let data = self.read(e)?;
            let mut q = usize::try_from(off).map_err(|_| Error::format("offset overflow"))?;
            if q >= data.len() {
                return Err(Error::format(format!(
                    "object map points {handle} past the end of page {page}"
                )));
            }
            let len = get_len(&data, &mut q, "record")?;
            let (key, o) = rec::read_object(&data[q..q + len])?;
            if key != handle {
                return Err(Error::format(format!(
                    "object map points {handle} at object {key}"
                )));
            }
            result = Some(o);
        }
        for (_, e) in journal_sections(c) {
            let set = decode_journal(&self.read(e)?)?;
            for ch in set.changes {
                match ch {
                    Change::Put(o) if o.handle == handle => result = Some(*o),
                    Change::Delete(h) if h == handle => result = None,
                    _ => {}
                }
            }
        }
        if let Some(o) = result.as_mut() {
            if is_1_0_from_r14(c) {
                upgrade_1_0_lineweights(o);
            }
        }
        Ok(result)
    }

    /// The header variables as of the latest change set.
    pub fn header(&self) -> Result<HeaderVars> {
        let c = &self.c;
        let mut h = rec::read_header(&self.read(required(c, SEC_HEADER)?)?)?;
        for (_, e) in journal_sections(c) {
            for ch in decode_journal(&self.read(e)?)?.changes {
                if let Change::Header(x) = ch {
                    h = *x;
                }
            }
        }
        Ok(h)
    }

    /// The drawing properties as of the latest change set.
    pub fn properties(&self) -> Result<Properties> {
        self.header().map(|h| h.properties)
    }

    /// The stored SVG preview, if the file has one.
    pub fn preview_svg(&self) -> Result<Option<String>> {
        let Some(e) = self.c.entry(SEC_PREVIEW) else {
            return Ok(None);
        };
        let data = self.read(e)?;
        Ok(Some(String::from_utf8_lossy(&data).into_owned()))
    }

    /// The record schema the file describes itself with (empty for 1.x
    /// files, which have none).
    pub fn schema(&self) -> Result<Vec<schema::FileKind>> {
        let Some(e) = self.c.entry(SEC_SCHEMA) else {
            return Ok(Vec::new());
        };
        let data = self.read(e)?;
        records(&data)?
            .into_iter()
            .map(schema::read_record)
            .collect()
    }

    /// Recompute the hash chain and compare it with what the file says.
    pub fn verify_chain(&self) -> Chain {
        chain(&self.c)
    }

    /// What the file contains, without decoding objects.
    pub fn info(&self) -> FileInfo {
        let c = &self.c;
        let mut sections = Vec::new();
        for e in &c.entries {
            let crc_ok = c.read(e).map(|(_, ok)| ok).unwrap_or(false);
            sections.push(SectionSummary {
                name: e.name.clone(),
                stored: e.stored_len,
                raw: e.raw_len,
                compressed: e.codec != Codec::Raw,
                codec: e.codec.name(),
                split: e.flags & container::section_flag::SPLIT != 0,
                crc_ok,
            });
        }
        let meta = c
            .entry(SEC_META)
            .and_then(|e| c.read(e).ok())
            .and_then(|(d, _)| read_meta(&d).ok())
            .unwrap_or_default();
        let recovery = recovery_of(c).and_then(|(d, _)| {
            recovery::decode(&d).ok().map(|r| {
                let scan = r.scan(&c.bytes);
                RecoveryInfo {
                    covered: r.len as u64,
                    block: r.block as u64,
                    stripes: r.stripes as u64,
                    damaged: scan.damaged.len(),
                    unrepairable: scan.unrepairable.len(),
                }
            })
        });
        FileInfo {
            format: (c.header.major, c.header.minor),
            generation: c.header.generation,
            header_copy: c.source,
            writer: meta.writer,
            saved_unix: meta.saved,
            source_version: meta.source,
            change_sets: journal_sections(c).len(),
            sections,
            notes: c.notes.clone(),
            schema_kinds: self.schema().map(|k| k.len()).unwrap_or(0),
            chain_head: chain_head(c).map(|h| hex(&h)),
            recovery,
        }
    }
}

/// The data of the `Recovery` section, whether or not it passes its CRC
/// (it checks its own layout, and rebuilt blocks are checked one by one),
/// and the bytes it must cover: from the end of the file header to the end
/// of the sections listed before it.
fn recovery_of(c: &Container) -> Option<(Vec<u8>, (usize, usize))> {
    let at = c.entries.iter().rposition(|e| e.name == SEC_RECOVERY)?;
    let end = c.entries[..at]
        .iter()
        .map(|e| e.offset.saturating_add(e.stored_len))
        .max()?;
    let (data, _) = c.read(&c.entries[at]).ok()?;
    Some((data, (container::HEADER_LEN, usize::try_from(end).ok()?)))
}

/// Whether a section belongs to the snapshot the `Digest` covers: every
/// section but `Digest`, `Recovery` and change sets, wherever it is.
fn in_snapshot(name: &str) -> bool {
    name != SEC_DIGEST && name != SEC_RECOVERY && !name.starts_with(JOURNAL_PREFIX)
}

/// The hash chain's latest value as the file states it (the snapshot hash
/// from `Digest` followed through every change set), without checking it.
fn chain_head(c: &Container) -> Option<[u8; 32]> {
    let mut v = read_digest(&c.read(c.entry(SEC_DIGEST)?).ok()?.0).ok()?;
    for (_, e) in journal_sections(c) {
        v = chain_next(&v, &c.read(e).ok()?.0);
    }
    Some(v)
}

/// The hash chain, recomputed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    /// False when the file has no `Digest` section (format 1.x).
    pub present: bool,
    /// The snapshot hash matches the sections.
    pub snapshot_ok: bool,
    /// Per change set (generation): its link to the one before matches.
    pub links: Vec<(u64, bool)>,
    /// The chain's latest value (hex).
    pub head: String,
    pub problems: Vec<String>,
}

impl Chain {
    pub fn ok(&self) -> bool {
        self.present && self.snapshot_ok && self.links.iter().all(|(_, ok)| *ok)
    }
}

fn chain(c: &Container) -> Chain {
    let mut out = Chain {
        present: false,
        snapshot_ok: false,
        links: Vec::new(),
        head: String::new(),
        problems: Vec::new(),
    };
    let Some(digest) = c.entry(SEC_DIGEST) else {
        return out;
    };
    out.present = true;
    let stated = match c.read(digest).and_then(|(d, _)| read_digest(&d)) {
        Ok(s) => s,
        Err(e) => {
            out.problems.push(format!("Digest: {e}"));
            return out;
        }
    };
    let mut data = Vec::new();
    for e in c.entries.iter().filter(|e| in_snapshot(&e.name)) {
        match c.read(e) {
            Ok((d, _)) => data.push((e.name.as_str(), d)),
            Err(err) => out.problems.push(format!("{}: {err}", e.name)),
        }
    }
    let actual = snapshot_hash(data.iter().map(|(n, d)| (*n, d.as_slice())));
    out.snapshot_ok = out.problems.is_empty() && actual == stated;
    if !out.snapshot_ok && out.problems.is_empty() {
        out.problems.push(format!(
            "snapshot hashes to {}, Digest says {}",
            hex(&actual),
            hex(&stated)
        ));
    }
    let mut v = stated;
    for (g, e) in journal_sections(c) {
        match c.read(e) {
            Ok((d, _)) => {
                let ok = journal_previous(&d) == Some(v);
                if !ok {
                    out.problems.push(format!(
                        "change set {g} does not follow from the one before"
                    ));
                }
                out.links.push((g, ok));
                v = chain_next(&v, &d);
            }
            Err(err) => {
                out.problems.push(format!("change set {g}: {err}"));
                out.links.push((g, false));
                break;
            }
        }
    }
    out.head = hex(&v);
    out
}

/// Read a native file from memory.
pub fn read_bytes(bytes: &[u8]) -> Result<Drawing> {
    NativeFile::from_bytes(bytes)?.drawing()
}

pub fn read_file(path: impl AsRef<Path>) -> Result<Drawing> {
    let path = path.as_ref();
    NativeFile::open(path)?.drawing().ctx(path.display())
}

/// Decode a single object without loading the rest of the drawing (see
/// [`NativeFile::object`], which keeps the object map between lookups).
pub fn read_object(bytes: &[u8], handle: Handle) -> Result<Option<Object>> {
    NativeFile::from_bytes(bytes)?.object(handle)
}

/// The stored SVG preview, if the file has one.
pub fn preview_svg(bytes: &[u8]) -> Result<Option<String>> {
    NativeFile::from_bytes(bytes)?.preview_svg()
}

/// What a native file contains, without decoding objects.
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub format: (u16, u16),
    pub generation: u64,
    /// "header" or "trailer": which copy of the file header was used.
    pub header_copy: &'static str,
    pub writer: String,
    pub saved_unix: Option<u64>,
    pub source_version: Option<DwgVersion>,
    pub sections: Vec<SectionSummary>,
    pub change_sets: usize,
    pub notes: Vec<String>,
    /// Kinds the file's own schema describes (0 for 1.x files).
    pub schema_kinds: usize,
    /// Latest value of the hash chain, as the file states it (2.0).
    pub chain_head: Option<String>,
    pub recovery: Option<RecoveryInfo>,
}

#[derive(Debug, Clone)]
pub struct SectionSummary {
    pub name: String,
    pub stored: u64,
    pub raw: u64,
    pub compressed: bool,
    /// "stored", "DEFLATE", "DWG LZ77".
    pub codec: String,
    /// Records split into streams before compression.
    pub split: bool,
    pub crc_ok: bool,
}

/// The recovery data of a file and the state of the bytes it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryInfo {
    pub covered: u64,
    pub block: u64,
    pub stripes: u64,
    pub damaged: usize,
    pub unrepairable: usize,
}

pub fn inspect(bytes: &[u8]) -> Result<FileInfo> {
    Ok(NativeFile::from_bytes(bytes)?.info())
}

#[cfg(test)]
mod tests;

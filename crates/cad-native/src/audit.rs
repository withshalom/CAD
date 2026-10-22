//! Structural audit of a native file: where every byte range is, and a list
//! of checks that the file obeys `docs/FORMAT.md` (the `cad inspect`
//! command). It reads every section, record, map entry and change set, so
//! it is also the regression tool for the format itself.
//!
//! The audit never changes the file and never panics on bad input.

use super::container::{section_flag, Codec, Container, DirEntry, FileHeader, HEADER_LEN};
use super::records as rec;
use super::tags::{get_len, get_varint, read_tag, Value};
use super::{
    chain, decode_journal_checked, numbered, read_meta, recovery, schema, JOURNAL_PREFIX,
    OBJECTS_PREFIX, SEC_CLASSES, SEC_DIGEST, SEC_HEADER, SEC_META, SEC_OBJECT_MAP, SEC_OPAQUE,
    SEC_PREVIEW, SEC_RECOVERY, SEC_SCHEMA,
};
use crate::error::{Error, Result};
use crate::model::*;
use std::collections::{BTreeMap, BTreeSet};

/// One copy of the file header, as decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderCopy {
    pub major: u16,
    pub minor: u16,
    pub flags: u32,
    pub generation: u64,
    pub dir_offset: u64,
    pub dir_len: u32,
}

impl From<FileHeader> for HeaderCopy {
    fn from(h: FileHeader) -> Self {
        HeaderCopy {
            major: h.major,
            minor: h.minor,
            flags: h.flags,
            generation: h.generation,
            dir_offset: h.dir_offset,
            dir_len: h.dir_len,
        }
    }
}

/// A byte range of the file and what occupies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub len: u64,
    pub label: String,
    /// Bytes left behind by earlier saves (reclaimed by a full save).
    pub superseded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub ok: bool,
    pub name: &'static str,
    pub detail: String,
}

/// Size of the object records in this format, and what the same tags
/// would take in binary DXF (2-byte group code; points as two or three
/// tags; handles as hexadecimal strings; strings NUL-terminated; binary
/// data in chunks of 127 bytes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagStats {
    pub records: u64,
    pub tags: u64,
    pub cad_bytes: u64,
    pub binary_dxf_bytes: u64,
    /// Tag count by value type.
    pub by_type: BTreeMap<&'static str, u64>,
}

#[derive(Debug, Clone, Default)]
pub struct AuditReport {
    pub file_len: u64,
    pub front: Option<HeaderCopy>,
    pub trailer: Option<HeaderCopy>,
    /// Which header copy the reader uses ("header" or "trailer").
    pub used: Option<&'static str>,
    /// The whole file, in order, with no gaps.
    pub regions: Vec<Region>,
    pub checks: Vec<Check>,
    pub objects: usize,
    pub change_sets: usize,
    pub tags: TagStats,
}

impl AuditReport {
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }

    pub fn failed(&self) -> impl Iterator<Item = &Check> {
        self.checks.iter().filter(|c| !c.ok)
    }

    /// Bytes a full save (compaction) would drop.
    pub fn superseded_bytes(&self) -> u64 {
        self.regions
            .iter()
            .filter(|r| r.superseded)
            .map(|r| r.len)
            .sum()
    }
}

fn push(r: &mut AuditReport, ok: bool, name: &'static str, detail: impl Into<String>) {
    r.checks.push(Check {
        ok,
        name,
        detail: detail.into(),
    });
}

fn add_tag_stats(stats: &mut TagStats, record: &[u8]) {
    stats.records += 1;
    let mut p = 0usize;
    while p < record.len() {
        let start = p;
        let Ok(t) = read_tag(record, &mut p) else {
            return;
        };
        stats.tags += 1;
        stats.cad_bytes += (p - start) as u64;
        // Binary DXF: 2-byte group code, then the value.
        let dxf_value: u64 = match &t.value {
            Value::Str(s) => s.len() as u64 + 1,
            Value::F64(_) | Value::I64(_) => 8,
            Value::I16(_) => 2,
            Value::I32(_) => 4,
            Value::Bool(_) => 1,
            Value::Handle(h) => format!("{h:X}").len() as u64 + 1,
            Value::Bytes(b) => {
                let chunks = (b.len() as u64).div_ceil(127).max(1);
                // Every chunk after the first repeats the group code.
                chunks + (chunks - 1) * 2 + b.len() as u64
            }
            Value::P2(_) => 8 + 2 + 8,
            Value::P3(_) => 8 + 2 * (2 + 8),
            // No DXF form; counted as its bytes.
            Value::Unknown { bytes, .. } => bytes.len() as u64,
        };
        stats.binary_dxf_bytes += 2 + dxf_value;
        *stats.by_type.entry(t.value.type_name()).or_default() += 1;
    }
}

fn section_label(e: &DirEntry) -> String {
    let split = if e.flags & section_flag::SPLIT != 0 {
        ", split into streams"
    } else {
        ""
    };
    match e.codec {
        Codec::Raw => format!("section {} (stored{split}, {} bytes)", e.name, e.raw_len),
        c => format!(
            "section {} ({}{split}, from {} bytes)",
            e.name,
            c.name(),
            e.raw_len
        ),
    }
}

/// Every handle an object refers to.
pub fn references(o: &Object) -> Vec<Handle> {
    let mut v = vec![o.owner, o.xdictionary];
    v.extend(&o.reactors);
    if let Some(e) = &o.entity {
        v.extend([e.layer, e.plotstyle, e.material]);
        if let LinetypeRef::Handle(h) = e.linetype {
            v.push(h);
        }
    }
    use ObjectKind as K;
    match &o.kind {
        K::Polyline(p) => {
            v.extend(&p.vertices);
            v.extend([p.first_vertex, p.last_vertex, p.seqend]);
        }
        K::Text(t) => v.push(t.style),
        K::Attribute(a) => v.push(a.text.style),
        K::MText(m) => v.push(m.style),
        K::Insert(i) => {
            v.extend(&i.attribs);
            v.extend([i.block_header, i.first_attrib, i.last_attrib, i.seqend]);
        }
        K::Dimension(d) => v.extend([d.dimstyle, d.block]),
        K::BlockHeader(b) => {
            v.extend(&b.entities);
            v.extend([
                b.block_entity,
                b.first_entity,
                b.last_entity,
                b.endblk,
                b.layout,
            ]);
        }
        K::Layer(l) => v.push(l.linetype),
        K::Linetype(l) => v.extend(l.elements.iter().map(|e| e.style)),
        K::DimStyle(d) => v.push(d.text_style),
        K::Control(c) => {
            v.extend(&c.entries);
            v.extend([c.model_space, c.paper_space]);
        }
        _ => {}
    }
    for x in &o.xdata {
        if let XDataApp::Handle(h) = x.app {
            v.push(h);
        }
        for it in &x.items {
            if let XValue::Layer(h) | XValue::Handle(h) = it {
                v.push(*h);
            }
        }
    }
    v.retain(|h| !h.is_null());
    v
}

/// Which applications' extended data the drawing carries, and anything a
/// DXF or DWG writer would refuse (braces that do not balance).
fn xdata_summary(d: &Drawing) -> String {
    let mut apps: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut carriers, mut blocks, mut by_handle, mut undecoded, mut unbalanced) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for o in d.objects.values() {
        if o.xdata.is_empty() {
            continue;
        }
        carriers += 1;
        for x in &o.xdata {
            blocks += 1;
            match &x.app {
                XDataApp::Name(n) => *apps.entry(n.as_str()).or_default() += 1,
                XDataApp::Handle(_) => by_handle += 1,
            }
            let mut depth = 0i64;
            let mut negative = false;
            let mut opaque = false;
            for it in &x.items {
                match it {
                    XValue::Open => depth += 1,
                    XValue::Close => {
                        depth -= 1;
                        negative |= depth < 0;
                    }
                    XValue::Undecoded { .. } => opaque = true,
                    _ => {}
                }
            }
            if opaque {
                undecoded += 1;
            } else if negative || depth != 0 {
                unbalanced += 1;
            }
        }
    }
    if blocks == 0 {
        return "none".into();
    }
    let mut list: Vec<(&str, usize)> = apps.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let mut s = format!("{blocks} application blocks on {carriers} objects");
    if !list.is_empty() {
        let names: Vec<String> = list
            .iter()
            .take(6)
            .map(|(n, k)| format!("{n} ×{k}"))
            .collect();
        s.push_str(&format!(
            ": {}{}",
            names.join(", "),
            if list.len() > 6 { ", …" } else { "" }
        ));
    }
    if by_handle > 0 {
        s.push_str(&format!(
            "; {by_handle} refer to a missing application record (kept by handle)"
        ));
    }
    if undecoded > 0 {
        s.push_str(&format!("; {undecoded} hold DWG bytes that did not decode"));
    }
    if unbalanced > 0 {
        s.push_str(&format!(
            "; {unbalanced} with unbalanced braces, which AutoCAD rejects"
        ));
    }
    s
}

/// Audit a native file held in memory.
pub fn audit(bytes: &[u8]) -> AuditReport {
    let len = bytes.len() as u64;
    let mut r = AuditReport {
        file_len: len,
        front: FileHeader::decode(bytes).map(Into::into),
        trailer: if bytes.len() >= 2 * HEADER_LEN {
            FileHeader::decode(&bytes[bytes.len() - HEADER_LEN..]).map(Into::into)
        } else {
            None
        },
        ..Default::default()
    };

    // --- File header copies -------------------------------------------
    let front_ok = r.front.is_some();
    push(
        &mut r,
        front_ok,
        "file header",
        if front_ok {
            "magic and CRC correct"
        } else {
            "damaged or missing (magic or CRC wrong)"
        },
    );
    let trailer_ok = r.trailer.is_some();
    push(
        &mut r,
        trailer_ok,
        "trailer",
        if trailer_ok {
            "magic and CRC correct"
        } else {
            "damaged or missing (magic or CRC wrong)"
        },
    );
    let front_h = FileHeader::decode(bytes);
    let trailer_h = if bytes.len() >= 2 * HEADER_LEN {
        FileHeader::decode(&bytes[bytes.len() - HEADER_LEN..])
    } else {
        None
    };
    if let (Some(f), Some(t)) = (front_h, trailer_h) {
        let same = f.same_state(&t);
        // From 2.0 each copy has a directory of its own.
        let shared = f.major >= 2 && same && f.dir_offset == t.dir_offset;
        push(
            &mut r,
            same && !shared,
            "header copies agree",
            if shared {
                "both copies point at one directory; format 2.0 keeps one for each".to_string()
            } else if same && f.dir_offset != t.dir_offset {
                format!(
                    "both at generation {}, each pointing at its own copy of the directory",
                    f.generation
                )
            } else if same {
                format!("both at generation {}", f.generation)
            } else {
                format!(
                    "front generation {}, trailer generation {}: a save did not finish",
                    f.generation, t.generation
                )
            },
        );
    }

    let c = match Container::parse(bytes) {
        Ok(c) => c,
        Err(e) => {
            push(&mut r, false, "directory", e.to_string());
            r.regions.push(Region {
                start: 0,
                len,
                label: "unreadable".into(),
                superseded: false,
            });
            return r;
        }
    };
    r.used = Some(c.source);
    let h = c.header;
    push(
        &mut r,
        c.notes.is_empty(),
        "directory",
        if c.notes.is_empty() {
            format!(
                "{} entries, CRC correct (read through the {} copy)",
                c.entries.len(),
                c.source
            )
        } else {
            c.notes.join("; ")
        },
    );

    // Format 2.0 keeps a second copy of the directory, which the reader
    // only looks at when the first is damaged.
    let other_copy = if c.source == "header" {
        trailer_h
    } else {
        front_h
    };
    if let Some(o) = other_copy.filter(|o| o.same_state(&h) && o.dir_offset != h.dir_offset) {
        let theirs = Container::directory(bytes, &o);
        let ok = theirs.as_ref().is_ok_and(|e| *e == c.entries);
        push(
            &mut r,
            ok,
            "directory copies",
            match theirs {
                Ok(_) if ok => "the second copy matches the first".to_string(),
                Ok(_) => "the two copies list different sections".to_string(),
                Err(e) => format!("the other copy is damaged ({e}); the file still opens"),
            },
        );
    }

    // --- Byte map ----------------------------------------------------------
    let mut regions: Vec<Region> = vec![Region {
        start: 0,
        len: HEADER_LEN as u64,
        label: if front_ok {
            "file header".into()
        } else {
            "file header (damaged)".into()
        },
        superseded: false,
    }];
    for e in &c.entries {
        regions.push(Region {
            start: e.offset,
            len: e.stored_len,
            label: section_label(e),
            superseded: false,
        });
    }
    regions.push(Region {
        start: h.dir_offset,
        len: h.dir_len as u64,
        label: format!("directory ({} entries)", c.entries.len()),
        superseded: false,
    });
    // The other header copy of the same state points at the other copy of
    // the directory (format 2.0).
    let other = if c.source == "header" {
        trailer_h
    } else {
        front_h
    };
    if let Some(o) = other.filter(|o| o.same_state(&h) && o.dir_offset != h.dir_offset) {
        regions.push(Region {
            start: o.dir_offset,
            len: o.dir_len as u64,
            label: "directory, second copy".into(),
            superseded: false,
        });
    }
    let trailer_start = len.saturating_sub(HEADER_LEN as u64);
    let tail_is_trailer = trailer_h.is_some_and(|t| t.same_state(&h));
    if tail_is_trailer {
        regions.push(Region {
            start: trailer_start,
            len: HEADER_LEN as u64,
            label: "trailer (copy of the file header)".into(),
            superseded: false,
        });
    }
    regions.sort_by_key(|g| (g.start, g.len));
    let mut overlaps = Vec::new();
    let mut out_of_bounds = Vec::new();
    // Sections lie before the last copy of the directory (in 2.0 the
    // recovery data sits between the two copies).
    let last_dir = other
        .filter(|o| o.same_state(&h))
        .map_or(h.dir_offset, |o| o.dir_offset.max(h.dir_offset));
    for e in &c.entries {
        let end = e.offset.saturating_add(e.stored_len);
        if e.offset < HEADER_LEN as u64 || end > last_dir {
            out_of_bounds.push(e.name.clone());
        }
    }
    let mut filled: Vec<Region> = Vec::new();
    let mut at = 0u64;
    for g in regions {
        if g.len == 0 {
            filled.push(g);
            continue;
        }
        if g.start < at {
            overlaps.push(g.label.clone());
        } else if g.start > at {
            filled.push(Region {
                start: at,
                len: g.start - at,
                label: "superseded: an earlier directory and trailer, or an unfinished save".into(),
                superseded: true,
            });
        }
        at = at.max(g.start.saturating_add(g.len));
        filled.push(g);
    }
    if at < len {
        filled.push(Region {
            start: at,
            len: len - at,
            label: "after the directory: an unfinished save or a damaged trailer".into(),
            superseded: true,
        });
    }
    r.regions = filled;
    push(
        &mut r,
        overlaps.is_empty() && out_of_bounds.is_empty(),
        "section placement",
        if overlaps.is_empty() && out_of_bounds.is_empty() {
            "every section lies between the file header and the last copy of its directory, \
             no overlaps"
                .to_string()
        } else {
            format!(
                "overlapping: {:?}; outside header..directory: {:?}",
                overlaps, out_of_bounds
            )
        },
    );

    // --- Section data ------------------------------------------------------
    let mut data: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut problems = Vec::new();
    let mut compressed = 0;
    for e in &c.entries {
        match c.read(e) {
            Ok((d, true)) => {
                if e.codec != Codec::Raw {
                    compressed += 1;
                }
                data.insert(e.name.clone(), d);
            }
            Ok((d, false)) => {
                problems.push(format!("{}: CRC mismatch", e.name));
                data.insert(e.name.clone(), d);
            }
            Err(err) => problems.push(format!("{}: {err}", e.name)),
        }
    }
    push(
        &mut r,
        problems.is_empty(),
        "section data",
        if problems.is_empty() {
            format!(
                "{} sections read ({compressed} decompressed), every CRC-32 matches",
                c.entries.len()
            )
        } else {
            problems.join("; ")
        },
    );
    let names: Vec<&str> = c.entries.iter().map(|e| e.name.as_str()).collect();
    let unique: BTreeSet<&str> = names.iter().copied().collect();
    push(
        &mut r,
        unique.len() == names.len(),
        "section names unique",
        if unique.len() == names.len() {
            "no name appears twice".to_string()
        } else {
            "a name appears twice (the later entry is used)".to_string()
        },
    );

    let pages = numbered(&c, OBJECTS_PREFIX);
    let v2 = h.major >= 2;
    let required: &[&str] = if v2 {
        &[
            SEC_META,
            SEC_HEADER,
            SEC_CLASSES,
            SEC_SCHEMA,
            SEC_OBJECT_MAP,
            SEC_DIGEST,
        ]
    } else {
        &[SEC_META, SEC_HEADER, SEC_CLASSES, SEC_OBJECT_MAP]
    };
    let mut missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|n| c.entry(n).is_none())
        .collect();
    if pages.is_empty() {
        missing.push("Objects.0");
    }
    push(
        &mut r,
        missing.is_empty(),
        "required sections",
        if missing.is_empty() {
            format!("{} and {} object pages", required.join(", "), pages.len())
        } else {
            format!("missing: {}", missing.join(", "))
        },
    );
    let known = |n: &str| {
        [
            SEC_META,
            SEC_HEADER,
            SEC_CLASSES,
            SEC_OBJECT_MAP,
            SEC_OPAQUE,
            SEC_PREVIEW,
            SEC_SCHEMA,
            SEC_DIGEST,
            SEC_RECOVERY,
        ]
        .contains(&n)
            || n.strip_prefix(OBJECTS_PREFIX)
                .map(|x| x.parse::<u64>().is_ok())
                .unwrap_or(false)
            || n.strip_prefix(JOURNAL_PREFIX)
                .map(|x| x.parse::<u64>().is_ok())
                .unwrap_or(false)
    };
    let unknown: Vec<&str> = unique.iter().copied().filter(|n| !known(n)).collect();
    if !unknown.is_empty() {
        push(
            &mut r,
            true,
            "unknown sections",
            format!(
                "{} (from a newer revision; skipped by this reader)",
                unknown.join(", ")
            ),
        );
    }

    // --- Section encoding ----------------------------------------------------
    let mut odd = Vec::new();
    let mut codecs: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for e in &c.entries {
        let unknown = e.flags & section_flag::CRITICAL & !section_flag::KNOWN;
        if unknown != 0 {
            odd.push(format!(
                "{}: flags {unknown:#x} from a newer revision",
                e.name
            ));
        } else if e.flags & section_flag::ENCRYPTED != 0 {
            odd.push(format!(
                "{}: encrypted, which this build cannot read",
                e.name
            ));
        } else if let Codec::Other(b) = e.codec {
            odd.push(format!("{}: codec {b} from a newer revision", e.name));
        } else if !v2 && (e.flags != 0 || e.codec == Codec::Deflate) {
            odd.push(format!(
                "{}: format 2.0 encoding in a {}.{} file",
                e.name, h.major, h.minor
            ));
        }
        let n = codecs.entry(e.codec.name()).or_default();
        n.0 += 1;
        if e.flags & section_flag::SPLIT != 0 {
            n.1 += 1;
        }
    }
    let summary: Vec<String> = codecs
        .iter()
        .map(|(k, (n, split))| {
            if *split > 0 {
                format!("{k} ×{n} ({split} split into streams)")
            } else {
                format!("{k} ×{n}")
            }
        })
        .collect();
    push(
        &mut r,
        odd.is_empty(),
        "section encoding",
        if odd.is_empty() {
            summary.join(", ")
        } else {
            odd.join("; ")
        },
    );
    if let Some(d) = data.get(SEC_RECOVERY) {
        // It covers the sections listed before it, from the file header on.
        let covered_ok = |rc: &recovery::Recovery| {
            let at = c.entries.iter().rposition(|e| e.name == SEC_RECOVERY);
            let end = at.and_then(|at| {
                c.entries[..at]
                    .iter()
                    .map(|e| e.offset.saturating_add(e.stored_len))
                    .max()
            });
            rc.start == HEADER_LEN && Some((rc.start + rc.len) as u64) == end
        };
        match recovery::decode(d) {
            Ok(rc) => {
                let scan = rc.scan(bytes);
                let whole = covered_ok(&rc);
                push(
                    &mut r,
                    whole && scan.damaged.is_empty(),
                    "recovery data",
                    if !whole {
                        format!(
                            "covers bytes {}..{}, not every snapshot section",
                            rc.start,
                            rc.start + rc.len
                        )
                    } else if scan.damaged.is_empty() {
                        format!(
                            "covers the {} bytes of the snapshot in {} blocks of {}; {} stripes \
                             rebuild any run of damage up to {} bytes; every block intact",
                            rc.len,
                            scan.blocks,
                            rc.block,
                            rc.stripes,
                            rc.longest_run()
                        )
                    } else {
                        format!(
                            "{} of {} blocks damaged, {} of them beyond repair; readers rebuild \
                             the rest, `cad compact` writes a sound copy",
                            scan.damaged.len(),
                            scan.blocks,
                            scan.unrepairable.len()
                        )
                    },
                );
            }
            Err(e) => push(&mut r, false, "recovery data", e.to_string()),
        }
    }

    // --- Meta and header ---------------------------------------------------
    let mut meta_objects = None;
    if let Some(d) = data.get(SEC_META) {
        match read_meta(d) {
            Ok(m) => {
                meta_objects = m.objects;
                let ok = m.source.is_some() && !m.writer.is_empty();
                push(
                    &mut r,
                    ok,
                    "Meta",
                    format!(
                        "written by {:?}, source {}",
                        m.writer,
                        m.source.map(|v| v.magic()).unwrap_or("unknown")
                    ),
                );
            }
            Err(e) => push(&mut r, false, "Meta", e.to_string()),
        }
    }
    if let Some(d) = data.get(SEC_HEADER) {
        // The reader keeps a tail it cannot parse instead of failing, so
        // check separately that every tag parses.
        let mut p = 0usize;
        let mut bad_tag = None;
        while p < d.len() {
            let at = p;
            if let Err(e) = read_tag(d, &mut p) {
                bad_tag = Some(format!("tag at byte {at}: {e}"));
                break;
            }
        }
        match (rec::read_header(d), bad_tag) {
            (Ok(hv), None) => push(
                &mut r,
                true,
                "Header",
                format!(
                    "parsed; {} source variables kept{}{}",
                    hv.raw.len(),
                    if hv.verified {
                        ", verified on import"
                    } else {
                        ""
                    },
                    if hv.unknown_tags.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "; {} bytes of variables from a newer writer kept as is",
                            hv.unknown_tags.len()
                        )
                    }
                ),
            ),
            (Ok(_), Some(e)) => push(&mut r, false, "Header", e),
            (Err(e), _) => push(&mut r, false, "Header", e.to_string()),
        }
    }
    if let Some(d) = data.get(SEC_CLASSES) {
        let res: Result<usize> = (|| {
            let mut p = 0;
            let mut n = 0;
            while p < d.len() {
                let l = get_len(d, &mut p, "record")?;
                rec::read_class(&d[p..p + l])?;
                p += l;
                n += 1;
            }
            Ok(n)
        })();
        match res {
            Ok(n) => push(&mut r, true, "Classes", format!("{n} class records parse")),
            Err(e) => push(&mut r, false, "Classes", e.to_string()),
        }
    }
    if let Some(d) = data.get(SEC_SCHEMA) {
        let res: Result<Vec<schema::FileKind>> = (|| {
            let mut p = 0;
            let mut v = Vec::new();
            while p < d.len() {
                let l = get_len(d, &mut p, "record")?;
                v.push(schema::read_record(&d[p..p + l])?);
                p += l;
            }
            Ok(v)
        })();
        match res {
            Ok(kinds) => {
                let ours: Vec<schema::FileKind> =
                    schema::KINDS.iter().map(schema::as_file_kind).collect();
                let mut differ = Vec::new();
                let mut newer = Vec::new();
                for k in &kinds {
                    match ours.iter().find(|o| o.kind == k.kind) {
                        Some(o) if o == k => {}
                        Some(_) => differ.push(k.name.clone()),
                        None => newer.push(k.name.clone()),
                    }
                }
                let mut detail = format!("{} kinds described", kinds.len());
                if differ.is_empty() && newer.is_empty() {
                    detail.push_str(", the same as this build's");
                }
                if !differ.is_empty() {
                    detail.push_str(&format!(
                        "; described differently from this build: {}",
                        differ.join(", ")
                    ));
                }
                if !newer.is_empty() {
                    detail.push_str(&format!(
                        "; kinds this build does not know: {}",
                        newer.join(", ")
                    ));
                }
                push(&mut r, true, "Schema", detail);
            }
            Err(e) => push(&mut r, false, "Schema", e.to_string()),
        }
    }

    // --- Object pages ------------------------------------------------------
    let mut located: BTreeMap<Handle, (u64, u64)> = BTreeMap::new();
    let mut page_problems = Vec::new();
    let mut order_ok = true;
    let mut last_key: Option<Handle> = None;
    let (mut newer_kinds, mut newer_fields) = (0usize, 0usize);
    let contiguous = pages.iter().enumerate().all(|(i, (n, _))| *n == i as u64);
    for (n, e) in &pages {
        let Some(d) = data.get(&e.name) else {
            page_problems.push(format!("page {n} unreadable"));
            continue;
        };
        let mut p = 0usize;
        while p < d.len() {
            let at = p as u64;
            let l = match get_len(d, &mut p, "record") {
                Ok(l) => l,
                Err(err) => {
                    page_problems.push(format!("page {n} at byte {at}: {err}"));
                    break;
                }
            };
            let body = &d[p..p + l];
            add_tag_stats(&mut r.tags, body);
            match rec::read_object_checked(body) {
                Ok((key, o, problem)) => {
                    if let Some(m) = problem {
                        page_problems.push(format!("page {n}, object {key}: {m}"));
                    }
                    if matches!(o.kind, ObjectKind::NewerKind(_)) {
                        newer_kinds += 1;
                    } else if !o.unknown_tags.is_empty() {
                        newer_fields += 1;
                    }
                    if last_key.map(|k| k >= key) == Some(true) {
                        order_ok = false;
                    }
                    last_key = Some(key);
                    located.insert(key, (*n, at));
                }
                Err(err) => page_problems.push(format!("page {n} at byte {at}: {err}")),
            }
            p += l;
        }
    }
    r.objects = located.len();
    push(
        &mut r,
        page_problems.is_empty() && contiguous,
        "object records",
        if page_problems.is_empty() && contiguous {
            let mut s = format!(
                "{} records in {} pages, every tag and field decodes",
                located.len(),
                pages.len()
            );
            if newer_kinds + newer_fields > 0 {
                s.push_str(&format!(
                    "; kept as is from a newer writer: {newer_kinds} objects of new kinds, \
                     {newer_fields} objects with new fields"
                ));
            }
            s
        } else {
            let mut v = page_problems.clone();
            if !contiguous {
                v.insert(0, "page numbers are not 0, 1, 2, …".into());
            }
            v.truncate(5);
            v.join("; ")
        },
    );
    push(
        &mut r,
        order_ok,
        "object order",
        if order_ok {
            "keys strictly ascending across pages (no duplicates)"
        } else {
            "keys out of order or repeated"
        },
    );
    if let Some(n) = meta_objects {
        push(
            &mut r,
            n == located.len() as u64,
            "object count",
            format!("Meta says {n}, pages hold {}", located.len()),
        );
    }

    // --- Object map --------------------------------------------------------
    if let Some(d) = data.get(SEC_OBJECT_MAP) {
        let res: Result<(u64, Vec<String>)> = (|| {
            let mut p = 0usize;
            let n = get_varint(d, &mut p)?;
            let mut key = 0u64;
            let mut bad = Vec::new();
            let mut seen = 0u64;
            for i in 0..n {
                let delta = get_varint(d, &mut p)?;
                if i > 0 && delta == 0 {
                    bad.push(format!("entry {i} repeats key {key:X}"));
                }
                key = key.wrapping_add(delta);
                let page = get_varint(d, &mut p)?;
                let off = get_varint(d, &mut p)?;
                match located.get(&Handle(key)) {
                    Some(&(pg, o)) if pg == page && o == off => seen += 1,
                    Some(&(pg, o)) => bad.push(format!(
                        "{key:X} mapped to page {page}+{off}, record is at page {pg}+{o}"
                    )),
                    None => bad.push(format!("{key:X} mapped but no such record")),
                }
            }
            if p != d.len() {
                bad.push(format!("{} trailing bytes", d.len() - p));
            }
            if seen != located.len() as u64 {
                bad.push(format!(
                    "{} records have no map entry",
                    located.len() as u64 - seen.min(located.len() as u64)
                ));
            }
            Ok((n, bad))
        })();
        match res {
            Ok((n, bad)) if bad.is_empty() => push(
                &mut r,
                true,
                "object map",
                format!("{n} entries, each points at the start of its record"),
            ),
            Ok((_, mut bad)) => {
                bad.truncate(5);
                push(&mut r, false, "object map", bad.join("; "))
            }
            Err(e) => push(&mut r, false, "object map", e.to_string()),
        }
    }

    // --- Preserved DWG bytes -----------------------------------------------
    if let Some(d) = data.get(SEC_OPAQUE) {
        let res: Result<(usize, usize)> = (|| {
            let mut p = 0;
            let (mut n, mut orphans) = (0, 0);
            while p < d.len() {
                let l = get_len(d, &mut p, "record")?;
                let (h, _) = rec::read_raw(&d[p..p + l])?;
                if !located.contains_key(&h) {
                    orphans += 1;
                }
                n += 1;
                p += l;
            }
            Ok((n, orphans))
        })();
        match res {
            Ok((n, 0)) => push(
                &mut r,
                true,
                "preserved DWG bytes",
                format!("{n} records, each belongs to an object in the file"),
            ),
            Ok((n, o)) => push(
                &mut r,
                false,
                "preserved DWG bytes",
                format!("{o} of {n} records belong to no object"),
            ),
            Err(e) => push(&mut r, false, "preserved DWG bytes", e.to_string()),
        }
    }

    // --- Change sets -------------------------------------------------------
    let journals = numbered(&c, JOURNAL_PREFIX);
    r.change_sets = journals.len();
    let flag = h.flags & super::container::FLAG_HAS_JOURNAL != 0;
    if journals.is_empty() {
        push(
            &mut r,
            !flag,
            "change sets",
            if flag {
                "journal flag set but no change sets"
            } else {
                "none (a full snapshot)"
            },
        );
    } else {
        let mut bad = Vec::new();
        let mut ops = 0usize;
        for (g, e) in &journals {
            if *g > h.generation {
                bad.push(format!("Journal.{g} is newer than the file's generation"));
            }
            match data.get(&e.name).map(|d| decode_journal_checked(d)) {
                Some(Ok((set, partial))) => {
                    ops += set.changes.len();
                    if let Some(first) = partial.first() {
                        bad.push(format!(
                            "Journal.{g}: {} records only partly decoded (first: {first})",
                            partial.len()
                        ));
                    }
                }
                Some(Err(err)) => bad.push(format!("Journal.{g}: {err}")),
                None => bad.push(format!("Journal.{g} unreadable")),
            }
        }
        let newest = journals.last().map(|(g, _)| *g).unwrap_or(0);
        if newest != h.generation {
            bad.push(format!(
                "newest change set is {newest}, file generation is {}",
                h.generation
            ));
        }
        if !flag {
            bad.push("change sets present but journal flag clear".into());
        }
        push(
            &mut r,
            bad.is_empty(),
            "change sets",
            if bad.is_empty() {
                format!(
                    "{} change sets ({ops} operations), generations ascending to {}",
                    journals.len(),
                    h.generation
                )
            } else {
                bad.join("; ")
            },
        );
    }

    // --- Hash chain ----------------------------------------------------------
    let ch = chain(&c);
    if ch.present {
        let ok = ch.ok();
        push(
            &mut r,
            ok,
            "hash chain",
            if ok {
                format!(
                    "SHA-256 of the snapshot matches Digest{}; head {}",
                    if ch.links.is_empty() {
                        String::new()
                    } else {
                        format!(
                            ", and each of {} change sets follows from the one before",
                            ch.links.len()
                        )
                    },
                    ch.head
                )
            } else {
                ch.problems.join("; ")
            },
        );
    }

    // --- Preview -----------------------------------------------------------
    if let Some(d) = data.get(SEC_PREVIEW) {
        let ok = std::str::from_utf8(d)
            .map(|s| s.starts_with("<svg") || s.starts_with("<?xml"))
            .unwrap_or(false);
        push(
            &mut r,
            ok,
            "preview",
            if ok {
                format!("SVG, {} bytes", d.len())
            } else {
                "not an SVG document".into()
            },
        );
    }

    // --- Extended data and references (informational) ----------------------
    if let Ok(d) = super::read_bytes(bytes) {
        push(&mut r, true, "extended data", xdata_summary(&d));
        let mut dangling = 0usize;
        let mut objs = 0usize;
        for o in d.objects.values() {
            let n = references(o)
                .iter()
                .filter(|h| !d.objects.contains_key(h))
                .count();
            if n > 0 {
                dangling += n;
                objs += 1;
            }
        }
        push(
            &mut r,
            true,
            "references",
            if dangling == 0 {
                "every handle an object refers to exists".to_string()
            } else {
                format!(
                    "{dangling} handles in {objs} objects point at objects not in the file \
                     (expected for partial drawings; in a DWG import it points at decode gaps)"
                )
            },
        );
    }
    r
}

/// Tags of the records in one section, one formatted line per tag, for
/// looking at a file by eye (`cad inspect --tags`). `Meta` and `Header`
/// are one tag stream; record sections are split into records.
pub fn dump_section(bytes: &[u8], name: &str, max_records: usize) -> Result<Vec<Vec<String>>> {
    let c = Container::parse(bytes)?;
    let e = c
        .entry(name)
        .ok_or_else(|| Error::format(format!("no section {name}")))?;
    let (d, _) = c.read(e)?;
    // Object records are annotated with the field names of the file's own
    // schema (this build's for 1.x files, which have none).
    let named = name.starts_with(OBJECTS_PREFIX) || name.starts_with(JOURNAL_PREFIX);
    let kinds: Vec<schema::FileKind> = match c.entry(SEC_SCHEMA).map(|e| c.read(e)) {
        Some(Ok((s, _))) => {
            let mut v = Vec::new();
            let mut p = 0;
            while p < s.len() {
                let Ok(l) = get_len(&s, &mut p, "record") else {
                    break;
                };
                if let Ok(k) = schema::read_record(&s[p..p + l]) {
                    v.push(k);
                }
                p += l;
            }
            v
        }
        _ => schema::KINDS.iter().map(schema::as_file_kind).collect(),
    };
    let streams: Vec<&[u8]> = if name == SEC_META || name == SEC_HEADER || name == SEC_DIGEST {
        vec![&d[..]]
    } else if name == SEC_OBJECT_MAP || name == SEC_PREVIEW || name == SEC_RECOVERY {
        return Err(Error::format(format!("{name} is not made of tags")));
    } else {
        let mut v = Vec::new();
        let mut p = 0;
        while p < d.len() && v.len() < max_records {
            let l = get_len(&d, &mut p, "record")?;
            v.push(&d[p..p + l]);
            p += l;
        }
        v
    };
    let mut out = Vec::new();
    for s in streams.into_iter().take(max_records.max(1)) {
        let mut lines = Vec::new();
        let mut p = 0;
        let mut kind = None;
        while p < s.len() {
            let t = read_tag(s, &mut p)?;
            if let (rec::code::KIND, Value::I16(k)) = (t.code, &t.value) {
                kind = Some(*k);
            }
            let field = if named && t.code == rec::code::KIND {
                kinds
                    .iter()
                    .find(|k| Some(k.kind) == kind)
                    .map(|k| format!("  [kind: {}]", k.name))
                    .unwrap_or_default()
            } else if named {
                // The name up to its explanation.
                schema::field_name(&kinds, kind, t.code)
                    .map(|n| {
                        let head = n.split(':').next().unwrap_or(n);
                        let short: String = head.chars().take(40).collect();
                        format!("  [{short}]")
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };
            lines.push(format!(
                "{:>5}  {:<6} {}{field}",
                t.code,
                t.value.type_name(),
                fmt_value(&t.value)
            ));
        }
        out.push(lines);
    }
    Ok(out)
}

fn fmt_value(v: &Value) -> String {
    match v {
        Value::Str(s) => {
            let mut t: String = s.chars().take(60).collect();
            if s.chars().count() > 60 {
                t.push('…');
            }
            format!("{t:?}")
        }
        Value::F64(x) => format!("{x}"),
        Value::I16(x) => format!("{x}"),
        Value::I32(x) => format!("{x}"),
        Value::I64(x) => format!("{x}"),
        Value::Bool(b) => format!("{b}"),
        Value::Handle(h) => format!("#{h:X}"),
        Value::Bytes(b) => {
            let head: Vec<String> = b.iter().take(8).map(|x| format!("{x:02x}")).collect();
            format!(
                "{} bytes: {}{}",
                b.len(),
                head.join(" "),
                if b.len() > 8 { " …" } else { "" }
            )
        }
        Value::P2(p) => format!("({}, {})", p.x, p.y),
        Value::P3(p) => format!("({}, {}, {})", p.x, p.y, p.z),
        Value::Unknown { ty, bytes } => format!("type {ty:#04x}, {} bytes", bytes.len()),
    }
}

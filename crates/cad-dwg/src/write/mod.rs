//! DWG writer: the drawing as an AC1032 (AutoCAD 2018) file.
//!
//! Objects the model holds whole are encoded from it (`objects`); objects
//! kept as their original DWG records of the same version are written as
//! they were. The header, classes, object map and the small sections are
//! built from the drawing (`sections`), and everything is packed into the
//! R2004-family paged container (`container`).
//!
//! Every layout choice follows what AutoCAD 2027 writes for AC1032, checked
//! against its MI-201 file byte for byte where the content is the same
//! (`tests`).

pub(crate) mod container;
pub(crate) mod objects;
pub(crate) mod sections;

#[cfg(test)]
mod tests;

use super::nongraphic::{self, DimVars, NonGraphic, Vport};
use super::version::DwgVersion;
use crate::error::{Error, Result};
use crate::model::*;
use container::{Section, MAX_PAGE};
use objects::Ctx;
use std::collections::BTreeMap;

/// A DWG file and what the writer had to leave out.
#[derive(Debug, Clone)]
pub struct DwgOutput {
    pub bytes: Vec<u8>,
    pub notes: Vec<String>,
}

/// What a DWG file records about the program that wrote it, and the parts
/// of the file the writer takes from that program when it has them.
///
/// [`WriteOptions::default`] names this crate as the writer, stores a
/// one-pixel preview, estimates text widths, and writes a pattern hatch's
/// line families only when the drawing holds them.
#[derive(Clone, Copy)]
pub struct WriteOptions<'a> {
    /// The program's name, recorded in the file's application information
    /// ("Saved by {name} {version}.") and its save history. The file never
    /// claims to come from another program.
    pub app_name: &'a str,
    /// The program's version, `major.minor.patch`.
    pub app_version: &'a str,
    /// The preview image (a PNG) that file browsers and AutoCAD's open
    /// dialog show. `None` stores a one-pixel image.
    pub preview_png: Option<&'a [u8]>,
    /// The width of one line of text at height 1, in the program's own
    /// font. A DWG's MTEXT records its extents, which AutoCAD measures
    /// again when it regenerates the text. `None` estimates from the
    /// number of characters ([`estimated_text_width`]).
    pub text_width: Option<&'a dyn Fn(&str) -> f64>,
    /// The line families of a pattern hatch that holds none, so that
    /// other programs draw the pattern: given the drawing and the hatch,
    /// the lines to store. `None` writes such hatches with no lines.
    pub hatch_lines: Option<&'a HatchLibrary<'a>>,
}

impl Default for WriteOptions<'_> {
    fn default() -> Self {
        WriteOptions {
            app_name: "cad-dwg",
            app_version: env!("CARGO_PKG_VERSION"),
            preview_png: None,
            text_width: None,
            hatch_lines: None,
        }
    }
}

impl std::fmt::Debug for WriteOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteOptions")
            .field("app_name", &self.app_name)
            .field("app_version", &self.app_version)
            .field("preview_png", &self.preview_png.map(<[u8]>::len))
            .field("text_width", &self.text_width.is_some())
            .field("hatch_lines", &self.hatch_lines.is_some())
            .finish()
    }
}

/// A line of text's width at height 1 when the program gives no font:
/// 0.8 of the height per character, about the average advance of a
/// single-stroke drafting font.
pub fn estimated_text_width(line: &str) -> f64 {
    line.chars().count() as f64 * 0.8
}

/// The preview stored when the program gives none: a one-pixel PNG in the
/// colour of AutoCAD's model-space background (a test checks its CRCs).
const BLANK_PREVIEW: &[u8] = &[
    // signature
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
    // IHDR: 1 × 1, 8-bit palette indices
    0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01,
    0x00, 0x00, 0x00, 0x01, 0x08, 0x03, 0x00, 0x00, 0x00, 0x28, 0xCB, 0x34,
    0xBB,
    // PLTE: one colour, #212830
    0x00, 0x00, 0x00, 0x03, 0x50, 0x4C, 0x54, 0x45, 0x21, 0x28, 0x30, 0xE5,
    0x71, 0x8F, 0x0B,
    // IDAT: one row, no filter, index 0
    0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE5, 0x27, 0xDE, 0xFC,
    // IEND
    0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// The objects the writer would encode and the ones it leaves out, by
/// handle, for a drawing.
struct Encoded {
    /// (handle, framed object bytes), in handle order.
    objects: Vec<(u64, Vec<u8>)>,
    notes: Vec<String>,
}

fn named_entry(d: &Drawing, dict: Handle, name: &str) -> Option<Handle> {
    let o = d.objects.get(&dict)?;
    match nongraphic::decode(o, &d.classes, d.codepage)? {
        Ok(NonGraphic::Dictionary(dc)) => dc
            .entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, h)| *h)
            .or(dc.default),
        _ => None,
    }
}

fn default_entry(d: &Drawing, dict: Handle, name: &str) -> Option<Handle> {
    let o = d.objects.get(&dict)?;
    match nongraphic::decode(o, &d.classes, d.codepage)? {
        Ok(NonGraphic::Dictionary(dc)) => dc.default.or_else(|| {
            dc.entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, h)| *h)
        }),
        _ => None,
    }
}

fn encode_objects(d: &Drawing, ver: DwgVersion, opts: &WriteOptions<'_>) -> Encoded {
    let hv = super::hvars::Hv::new(d);
    let appids: BTreeMap<String, Handle> = d
        .objects
        .values()
        .filter_map(|o| match &o.kind {
            ObjectKind::NamedRecord(r) if o.type_code == 0x43 => {
                Some((r.name.to_ascii_uppercase(), o.handle))
            }
            _ => None,
        })
        .collect();
    let header_dims = nongraphic::dim_vars_from_header(&hv);
    let dv = |o: &Object| -> DimVars {
        match nongraphic::dimstyle(o, &d.classes, d.codepage) {
            Some(Ok((_, v))) => v,
            _ => {
                let mut v = header_dims.clone();
                if let ObjectKind::DimStyle(s) = &o.kind {
                    v.apply_model(s);
                }
                v
            }
        }
    };
    let extents = (hv.p3("EXTMIN", Default::default()), hv.p3("EXTMAX", Default::default()));
    let vp = |o: &Object| -> Vport {
        match nongraphic::vport(o, &d.classes, d.codepage) {
            Some(Ok(v)) => v,
            _ => Vport::looking_at(
                crate::geom::Vec2::new(extents.0.x, extents.0.y),
                crate::geom::Vec2::new(extents.1.x, extents.1.y),
            ),
        }
    };
    let plot_style = default_entry(d, hv.handle("DICTIONARY_PLOTSTYLES"), "Normal").unwrap_or(Handle::NULL);
    let material = named_entry(d, hv.handle("DICTIONARY_MATERIALS"), "Global").unwrap_or(Handle::NULL);
    let ctx = Ctx {
        d,
        appids: &appids,
        plot_style,
        material,
        dimstyle_vars: &dv,
        vport: &vp,
        text_width: opts.text_width.unwrap_or(&estimated_text_width),
        hatch_lines: opts.hatch_lines,
    };
    let mut objects = Vec::with_capacity(d.objects.len());
    let mut notes = Vec::new();
    let dxf_only = dxf_only_objects(d);
    let mut patched: Option<Object> = None;
    for o in d.objects.values() {
        if dxf_only.contains(&o.handle) {
            continue;
        }
        let o = match without_dxf_only(o, &dxf_only) {
            Some(p) => patched.insert(p),
            None => o,
        };
        let from_raw = || -> Option<Vec<u8>> {
            let raw = o.raw.as_ref()?;
            (raw.version == ver).then(|| objects::frame(ver, &raw.data, raw.handle_stream_bits))
        };
        let bytes = match objects::encode(o, &ctx, ver) {
            Ok(Some(b)) => Some(b),
            Ok(None) => from_raw(),
            Err(e) => {
                let b = from_raw();
                if b.is_none() {
                    notes.push(format!("{:X} {}: {}", o.handle.0, o.type_name, e.message));
                }
                b
            }
        };
        match bytes {
            Some(b) => objects.push((o.handle.0, b)),
            None => {
                if !notes.iter().any(|n| n.starts_with(&format!("{:X} ", o.handle.0))) {
                    let why = match &o.raw {
                        Some(r) => format!("kept only as a {} record", r.version.magic()),
                        None => "not decoded".into(),
                    };
                    notes.push(format!("{:X} {}: left out, {why}", o.handle.0, o.type_name));
                }
            }
        }
    }
    Encoded { objects, notes }
}

/// Objects a DXF holds that the DWG of the same drawing does not: the
/// cell style maps AutoCAD adds to each table style in DXF (and the
/// dictionaries that hold them), and the recomposition record it adds to
/// the named object dictionary. AutoCAD writes them when it saves a DXF
/// and folds them back in when it reads one.
fn dxf_only_objects(d: &Drawing) -> std::collections::HashSet<Handle> {
    let mut out = std::collections::HashSet::new();
    for o in d.objects.values() {
        match &o.kind {
            ObjectKind::NonGraphic(NonGraphic::Groups(g)) if g.kind == "CELLSTYLEMAP" => {
                out.insert(o.handle);
            }
            ObjectKind::NonGraphic(NonGraphic::Dictionary(dc))
                if !dc.entries.is_empty()
                    && dc.entries.iter().all(|(k, _)| k == "ACAD_ROUNDTRIP_2008_TABLESTYLE_CELLSTYLEMAP") =>
            {
                out.insert(o.handle);
            }
            _ => {}
        }
    }
    if let Some(root) = d.objects.get(&d.header.named_objects_dict) {
        if let ObjectKind::NonGraphic(NonGraphic::Dictionary(dc)) = &root.kind {
            for (k, h) in &dc.entries {
                if k == "ACDB_RECOMPOSE_DATA" {
                    out.insert(*h);
                }
            }
        }
    }
    out
}

/// The object without references to DXF-only objects (an extension
/// dictionary, dictionary entries), or `None` when it has none.
fn without_dxf_only(o: &Object, dxf_only: &std::collections::HashSet<Handle>) -> Option<Object> {
    let xdic = dxf_only.contains(&o.xdictionary);
    let entries = match &o.kind {
        ObjectKind::NonGraphic(NonGraphic::Dictionary(dc)) => dc.entries.iter().any(|(_, h)| dxf_only.contains(h)),
        _ => false,
    };
    if !xdic && !entries {
        return None;
    }
    let mut p = o.clone();
    if xdic {
        p.xdictionary = Handle::NULL;
    }
    if let ObjectKind::NonGraphic(NonGraphic::Dictionary(dc)) = &mut p.kind {
        dc.entries.retain(|(_, h)| !dxf_only.contains(h));
    }
    Some(p)
}

/// The section map order AutoCAD 2027 uses (ids descending from the first).
const MAP_ORDER: [&str; 13] = [
    "AcDb:XrefManifest",
    "AcDb:AppInfoHistory",
    "AcDb:AppInfo",
    "AcDb:Preview",
    "AcDb:SummaryInfo",
    "AcDb:RevHistory",
    "AcDb:AcDbObjects",
    "AcDb:ObjFreeSpace",
    "AcDb:Template",
    "AcDb:Handles",
    "AcDb:Classes",
    "AcDb:AuxHeader",
    "AcDb:Header",
];

fn round_up(n: usize, to: usize) -> usize {
    n.max(1).div_ceil(to) * to
}

/// The drawing as an AutoCAD 2018 (AC1032) DWG file, with
/// [`WriteOptions::default`].
pub fn to_bytes(d: &Drawing) -> Result<DwgOutput> {
    to_bytes_with(d, &WriteOptions::default())
}

/// The drawing as an AutoCAD 2018 (AC1032) DWG file.
///
/// Objects are written as the drawing holds them. A dimension whose
/// graphics block is missing is written without one; a program that draws
/// dimensions adds the blocks first, so that other programs show them.
pub fn to_bytes_with(d: &Drawing, opts: &WriteOptions<'_>) -> Result<DwgOutput> {
    let (stream, mut notes) = stream_sections(d, opts)?;
    let bytes = container::pack(&file_info(d), &stream, &MAP_ORDER)?;
    if !notes.is_empty() {
        notes.insert(
            0,
            format!("{} objects could not be written to AC1032", notes.len()),
        );
    }
    Ok(DwgOutput { bytes, notes })
}

fn file_info(d: &Drawing) -> container::FileInfo {
    container::FileInfo {
        version: *b"AC1032",
        maintenance: 0,
        app_version: 33,
        app_maintenance: 0,
        codepage: d.codepage,
        acad_maintenance: sections::ACAD_MAINTENANCE,
    }
}

/// Every section of the file, in stream order, and notes on what was left
/// out.
fn stream_sections(d: &Drawing, opts: &WriteOptions<'_>) -> Result<(Vec<Section>, Vec<String>)> {
    let ver = DwgVersion::R2018;
    let Encoded { objects, mut notes } = encode_objects(d, ver, opts);
    let _ = &mut notes;
    if objects.is_empty() {
        return Err(Error::format("the drawing has no objects to write"));
    }

    // AcDb:AcDbObjects: RL 0x0DCA, then the objects; the object map
    // records where each starts.
    let mut obj_section = 0x0DCAu32.to_le_bytes().to_vec();
    let mut map = Vec::with_capacity(objects.len());
    for (h, b) in &objects {
        map.push((*h, obj_section.len() as u64));
        obj_section.extend_from_slice(b);
    }
    // The map is in handle order. Objects are encoded in key order, and an
    // object read from a damaged file can carry a handle other than its
    // key; two with the same handle keep the first.
    map.sort_by_key(|e| e.0);
    map.dedup_by_key(|e| e.0);
    let max_handle = objects.iter().map(|(h, _)| *h).max().unwrap_or(0);
    let handseed = d.header.handseed.unwrap_or(0).max(max_handle.saturating_add(1));

    let summary = sections::summary_info(d);
    let summary_page = round_up(summary.len(), 0x80);
    let png = opts.preview_png.unwrap_or(BLANK_PREVIEW);
    let preview_len = sections::preview(png, 0).len();
    let preview_page = round_up(preview_len, 0x400);
    let preview_address = 0x100 + container::uncompressed_page_extent(summary_page) + 0x20;
    let preview = sections::preview(png, preview_address as u64);
    let app_info = sections::app_info(opts.app_name, opts.app_version);
    let app_info_page = round_up(app_info.len(), 0x80);
    // Every write is the drawing's first save as far as the file records:
    // the model keeps no count of earlier saves.
    let saves = 1;
    let history = sections::app_info_history(d, saves == 1, opts.app_name, opts.app_version);
    let history_page = round_up(history.len(), 0x80);

    let compressed = |name, data| Section {
        name,
        data,
        compressed: true,
        page_size: MAX_PAGE,
    };
    let stream = vec![
        Section {
            name: "AcDb:SummaryInfo",
            data: summary,
            compressed: false,
            page_size: summary_page,
        },
        Section {
            name: "AcDb:Preview",
            data: preview,
            compressed: false,
            page_size: preview_page,
        },
        Section {
            name: "AcDb:AppInfo",
            data: app_info,
            compressed: false,
            page_size: app_info_page,
        },
        Section {
            name: "AcDb:AppInfoHistory",
            data: history,
            compressed: false,
            page_size: history_page,
        },
        compressed("AcDb:XrefManifest", sections::xref_manifest(d)),
        compressed("AcDb:RevHistory", sections::rev_history()),
        compressed("AcDb:AcDbObjects", obj_section),
        compressed("AcDb:ObjFreeSpace", sections::obj_free_space(d, map.len())),
        compressed("AcDb:Template", sections::template(d)),
        compressed("AcDb:Handles", sections::handles_section(&map)),
        compressed("AcDb:Classes", sections::classes_section(d, ver)),
        compressed("AcDb:AuxHeader", sections::aux_header(d, handseed, saves)),
        compressed("AcDb:Header", sections::header_section(d, ver, handseed)),
    ];
    Ok((stream, std::mem::take(&mut notes)))
}

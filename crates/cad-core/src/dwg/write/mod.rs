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

fn encode_objects(d: &Drawing, ver: DwgVersion) -> Encoded {
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
    let header_dims = DimVars::from_header(&hv);
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

/// AutoCAD's model-space background, the canvas of its previews.
const PREVIEW_BACKGROUND: (u8, u8, u8) = (0x21, 0x28, 0x30);

/// The drawing's `*Active` viewport, which AutoCAD draws the preview from.
fn active_viewport(d: &Drawing) -> Option<Vport> {
    d.objects
        .values()
        .filter(|o| o.type_name == "VPORT")
        .filter_map(|o| nongraphic::vport(o, &d.classes, d.codepage)?.ok())
        .find(|v| v.name.eq_ignore_ascii_case("*Active"))
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

/// The preview image as AutoCAD writes it: 256 pixels wide, the shape and
/// view of the active viewport, model space on AutoCAD's background, as an
/// indexed-colour PNG (the background, then the AutoCAD colour index).
fn preview_png(d: &Drawing) -> Vec<u8> {
    let scene = crate::render::model_space(d);
    let b = scene.bounds;
    let width = 256u32;
    let (height, view) = match active_viewport(d) {
        Some(v) if v.view_height > 0.0 && v.aspect > 0.0 => {
            let h = ((width as f64 / v.aspect).round() as u32).clamp(16, 256);
            let view = crate::raster::View {
                center: v.center,
                scale: h as f64 / v.view_height,
            };
            (h, Some(view))
        }
        _ => {
            let h = if b.is_empty() || b.width() <= 0.0 || b.height() <= 0.0 {
                192
            } else {
                ((width as f64 * b.height() / b.width()).round() as u32).clamp(64, 256)
            };
            (h, None)
        }
    };
    let opts = crate::raster::RasterOptions {
        width,
        height,
        view,
        grid: false,
        axis_icon: false,
        threads: 1,
        ..Default::default()
    };
    let img = crate::raster::render(&scene, &opts);
    let mut palette = vec![PREVIEW_BACKGROUND];
    palette.extend((1..=255u8).map(crate::model::color::aci_rgb));
    let canvas = crate::standards::canvas::BACKGROUND;
    let dist = |a: (u8, u8, u8), b: (u8, u8, u8)| {
        let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
        d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
    };
    let mut cache = std::collections::HashMap::new();
    let pixels: Vec<u8> = img
        .rgba
        .chunks(4)
        .map(|p| {
            let c = (p[0], p[1], p[2]);
            *cache.entry(c).or_insert_with(|| {
                if dist(c, canvas) < 3 * 24 * 24 {
                    return 0;
                }
                (1..palette.len()).min_by_key(|&i| dist(c, palette[i])).unwrap_or(0) as u8
            })
        })
        .collect();
    crate::raster::png::encode_indexed(img.width, img.height, &palette, &pixels)
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

/// The drawing as an AutoCAD 2018 (AC1032) DWG file.
pub fn to_bytes(d: &Drawing) -> Result<DwgOutput> {
    // Dimensions drawn from their definition points get graphics blocks,
    // which other programs draw.
    let d = &*crate::export::dimblocks::with_dimension_blocks(d);
    let (stream, mut notes) = stream_sections(d)?;
    let bytes = container::pack(&file_info(d), &stream, &MAP_ORDER);
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
fn stream_sections(d: &Drawing) -> Result<(Vec<Section>, Vec<String>)> {
    let ver = DwgVersion::R2018;
    let Encoded { objects, mut notes } = encode_objects(d, ver);
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
    let png = preview_png(d);
    let preview_len = sections::preview(&png, 0).len();
    let preview_page = round_up(preview_len, 0x400);
    let preview_address = 0x100 + container::uncompressed_page_extent(summary_page) + 0x20;
    let preview = sections::preview(&png, preview_address as u64);
    let app_info = sections::app_info();
    let app_info_page = round_up(app_info.len(), 0x80);
    // Every write is the drawing's first save as far as the file records:
    // the model keeps no count of earlier saves.
    let saves = 1;
    let history = sections::app_info_history(d, saves == 1);
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

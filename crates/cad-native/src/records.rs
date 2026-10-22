//! Mapping between the drawing database and tagged records.
//!
//! Group codes carry the meaning the published DXF reference gives them
//! wherever the model has a DXF equivalent (10 = first point, 40 = radius,
//! 62 = color index, 330 = owner handle, …). Where the model keeps more
//! than DXF expresses, or keeps it differently (a layer by handle instead
//! of by name), a code of 2000 or more is used. Deliberate differences
//! from DXF: angles are radians, handles are binary, points are single
//! tags. `docs/FORMAT.md` lists every record layout.
//!
//! An object record ends like a DXF entity: its own fields, then extended
//! data (XDATA, groups 1000–1071). Between the two sit any fields a newer
//! revision of the format added; this version keeps them as encoded bytes
//! and writes them back unchanged, so older software never strips newer
//! data.

use super::schema::{
    COLOR_WORD_BY_BLOCK, LW_BY_BLOCK, LW_BY_LAYER, LW_DEFAULT, ONES, ORIGIN2, ORIGIN3, X, Z,
};
use super::tags::{read_tag, TagReader, TagWriter, Value};
use crate::dwg::DwgVersion;
use crate::error::{Error, Result};
use crate::geom::{Vec2, Vec3};
use crate::model::*;
use std::f64::consts::TAU;

// Private group codes (2000+). Keep in sync with docs/FORMAT.md.
pub mod code {
    pub const TYPE_CODE: u16 = 2000;
    pub const KIND: u16 = 2001;
    pub const SRC_OFFSET: u16 = 2002;
    pub const SRC_SIZE: u16 = 2003;
    pub const NOTE: u16 = 2004;
    pub const SPACE: u16 = 2005;
    pub const LAYER: u16 = 2006;
    pub const LTYPE_KIND: u16 = 2007;
    pub const LTYPE: u16 = 2008;
    pub const COLOR_NONE: u16 = 2009;

    pub const SUBKIND: u16 = 2100;
    pub const STYLE: u16 = 2101;
    pub const IS_DEFINITION: u16 = 2102;
    pub const BLOCK_HEADER: u16 = 2103;
    pub const BLOCK: u16 = 2104;
    pub const DIMSTYLE: u16 = 2105;
    pub const FIRST: u16 = 2106;
    pub const LAST: u16 = 2107;
    pub const SEQEND: u16 = 2108;
    pub const ENDBLK: u16 = 2109;
    pub const BLOCK_ENTITY: u16 = 2110;
    pub const LIST_COUNT: u16 = 2111;
    pub const LIST_ITEM: u16 = 2112;
    pub const BULGE_COUNT: u16 = 2113;
    pub const WIDTH_COUNT: u16 = 2114;
    pub const ID_COUNT: u16 = 2115;
    pub const WEIGHT_COUNT: u16 = 2116;
    pub const IS_TRACE: u16 = 2117;
    pub const INSERT_SCALE: u16 = 2118;
    pub const CLONE_INSERTION: u16 = 2119;
    pub const INSERT_UNITS: u16 = 2120;
    pub const LAYER_STATE: u16 = 2121;
    pub const LAYER_LTYPE: u16 = 2122;
    pub const MODEL_SPACE: u16 = 2123;
    pub const PAPER_SPACE: u16 = 2124;
    pub const MESSAGE: u16 = 2125;
    pub const INSERT_ROTATION: u16 = 2126;
    pub const KNOT_COUNT: u16 = 2127;
    pub const CTRL_COUNT: u16 = 2128;
    pub const FIT_COUNT: u16 = 2129;
    pub const PATH_COUNT: u16 = 2130;
    pub const EDGE_COUNT: u16 = 2131;
    /// A flag word kept as DWG stores it, where its bits differ from the
    /// DXF group 70 of the same entity (LWPOLYLINE, LAYER, DIMENSION).
    pub const DWG_FLAGS: u16 = 2132;
    /// The object's own handle, when it differs from the key it is stored
    /// under (possible in DWG files whose object map disagrees).
    pub const OWN_HANDLE: u16 = 2133;
    /// Dimension style colours (DIMCLRD, DIMCLRE, DIMCLRT) as color method
    /// words (`Color::to_method_word`): DXF's groups 176–178 hold only an
    /// index, these keep true colours too.
    pub const DIM_LINE_COLOR: u16 = 2134;
    pub const DIM_EXT_COLOR: u16 = 2135;
    pub const DIM_TEXT_COLOR: u16 = 2136;
    /// Part data of a block record (Proposed).
    pub const PART_MATERIAL: u16 = 2140;
    pub const PART_GRADE: u16 = 2141;
    pub const PART_THICKNESS: u16 = 2142;
    pub const PART_QUANTITY: u16 = 2143;

    pub const HDR_RAW_KEY: u16 = 2200;
    pub const HDR_RAW_VALUE: u16 = 2201;

    pub const CLASS_NUMBER: u16 = 2300;
    pub const CLASS_ITEM_ID: u16 = 2301;
    pub const CLASS_DWG_VERSION: u16 = 2302;
    pub const CLASS_MAINT_VERSION: u16 = 2303;

    pub const RAW_HANDLE_BITS: u16 = 2400;

    pub const OP: u16 = 2500;

    /// Inside extended data: DWG version tag of the undecoded bytes that
    /// follow in [`XDATA_RAW`].
    pub const XDATA_RAW_VERSION: u16 = 2600;
    /// Inside extended data: DWG extended-data bytes that could not be
    /// decoded, exactly as stored.
    pub const XDATA_RAW: u16 = 2601;
}

/// Extended data group codes, as DXF numbers them.
pub mod xcode {
    pub const STR: u16 = 1000;
    pub const APP: u16 = 1001;
    pub const BRACE: u16 = 1002;
    pub const LAYER: u16 = 1003;
    pub const BYTES: u16 = 1004;
    pub const HANDLE: u16 = 1005;
    pub const POINT: u16 = 1010;
    pub const POSITION: u16 = 1011;
    pub const DISPLACEMENT: u16 = 1012;
    pub const DIRECTION: u16 = 1013;
    pub const REAL: u16 = 1040;
    pub const DISTANCE: u16 = 1041;
    pub const SCALE: u16 = 1042;
    pub const INT16: u16 = 1070;
    pub const INT32: u16 = 1071;
}

use code as c;

// Kind identifiers (group 2001).
pub(crate) mod kind {
    pub const UNSUPPORTED: i16 = 0;
    pub const FAILED: i16 = 1;
    pub const LINE: i16 = 2;
    pub const CIRCLE: i16 = 3;
    pub const ARC: i16 = 4;
    pub const POINT: i16 = 5;
    pub const ELLIPSE: i16 = 6;
    pub const LWPOLYLINE: i16 = 7;
    pub const POLYLINE: i16 = 8;
    pub const VERTEX: i16 = 9;
    pub const SEQEND: i16 = 10;
    pub const TEXT: i16 = 11;
    pub const ATTRIBUTE: i16 = 12;
    pub const MTEXT: i16 = 13;
    pub const INSERT: i16 = 14;
    pub const SOLID: i16 = 15;
    pub const FACE3D: i16 = 16;
    pub const SPLINE: i16 = 17;
    pub const DIMENSION: i16 = 18;
    pub const HATCH: i16 = 19;
    pub const BLOCK_BEGIN: i16 = 20;
    pub const BLOCK_END: i16 = 21;
    pub const BLOCK_HEADER: i16 = 22;
    pub const LAYER: i16 = 23;
    pub const CONTROL: i16 = 24;
    pub const NAMED_RECORD: i16 = 25;
    // Format 1.1.
    pub const LINETYPE: i16 = 26;
    pub const TEXT_STYLE: i16 = 27;
    pub const DIM_STYLE: i16 = 28;
    // Format 2.2.
    pub const VPORT: i16 = 29;
    pub const DIMSTYLE_RECORD: i16 = 30;
    pub const DICTIONARY: i16 = 31;
    pub const XRECORD: i16 = 32;
    pub const PLACEHOLDER: i16 = 33;
    pub const DICTIONARY_VAR: i16 = 34;
    pub const SCALE: i16 = 35;
    pub const LAYOUT: i16 = 36;
    pub const MLINESTYLE: i16 = 37;
    // Format 2.3.
    pub const GROUPS: i16 = 38;
    // Format 2.4.
    pub const XLINE: i16 = 39;
    pub const LEADER: i16 = 40;
    pub const VIEWPORT: i16 = 41;
    /// The highest kind this version reads.
    pub const LAST: i16 = VIEWPORT;
}

const P2_BYTES: usize = 3 + 16;
const P3_BYTES: usize = 3 + 24;
const F64_BYTES: usize = 3 + 8;
const H_BYTES: usize = 3 + 1;

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// A color: 62 index (0 by block), 420 true color or 2009 none; by layer,
/// the default, writes nothing (format 2.0; 1.x wrote 62 = 256).
fn put_color(w: &mut TagWriter, color: Color) {
    match color {
        // 1.x always writes a color.
        Color::ByLayer if w.full => w.i16(62, 256),
        Color::ByLayer => {}
        Color::ByBlock => w.i16(62, 0),
        Color::Index(i) => w.i16(62, i as i16),
        Color::Rgb(r, g, b) => w.i32(420, ((r as i32) << 16) | ((g as i32) << 8) | b as i32),
        Color::None => w.bool(c::COLOR_NONE, true),
    }
}

fn get_color(r: &mut TagReader) -> Result<Color> {
    if let Some(v) = r.opt_i16(62)? {
        return Ok(match v {
            256 => Color::ByLayer,
            0 => Color::ByBlock,
            1..=255 => Color::Index(v as u8),
            _ => return Err(Error::format(format!("group 62: color index {v}"))),
        });
    }
    if let Some(v) = r.opt_i32(420)? {
        return Ok(Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
    }
    if r.opt_bool(c::COLOR_NONE)?.is_some() {
        return Ok(Color::None);
    }
    Ok(Color::ByLayer)
}

fn put_f64s(w: &mut TagWriter, count_code: u16, item: u16, v: &[f64]) {
    w.count(count_code, v.len());
    for x in v {
        w.f64(item, *x);
    }
}

fn get_f64s(r: &mut TagReader, count_code: u16, item: u16) -> Result<Vec<f64>> {
    let n = r.count(count_code, F64_BYTES)?;
    (0..n).map(|_| r.f64(item)).collect()
}

fn put_p2s(w: &mut TagWriter, count_code: u16, item: u16, v: &[Vec2]) {
    w.count(count_code, v.len());
    for p in v {
        w.p2(item, *p);
    }
}

fn get_p2s(r: &mut TagReader, count_code: u16, item: u16) -> Result<Vec<Vec2>> {
    let n = r.count(count_code, P2_BYTES)?;
    (0..n).map(|_| r.p2(item)).collect()
}

fn put_handles_d(w: &mut TagWriter, list: &[Handle]) {
    w.count_d(c::LIST_COUNT, list.len());
    for h in list {
        w.handle(c::LIST_ITEM, *h);
    }
}

fn get_handles_or(r: &mut TagReader) -> Result<Vec<Handle>> {
    let n = r.count_or(c::LIST_COUNT, H_BYTES)?;
    (0..n).map(|_| r.handle(c::LIST_ITEM)).collect()
}

fn put_f64s_d(w: &mut TagWriter, count_code: u16, item: u16, v: &[f64]) {
    w.count_d(count_code, v.len());
    for x in v {
        w.f64(item, *x);
    }
}

fn get_f64s_or(r: &mut TagReader, count_code: u16, item: u16) -> Result<Vec<f64>> {
    let n = r.count_or(count_code, F64_BYTES)?;
    (0..n).map(|_| r.f64(item)).collect()
}

fn put_p3s_d(w: &mut TagWriter, count_code: u16, item: u16, v: &[Vec3]) {
    w.count_d(count_code, v.len());
    for p in v {
        w.p3(item, *p);
    }
}

fn get_p3s_or(r: &mut TagReader, count_code: u16, item: u16) -> Result<Vec<Vec3>> {
    let n = r.count_or(count_code, P3_BYTES)?;
    (0..n).map(|_| r.p3(item)).collect()
}

fn bits(flags: &[bool]) -> i16 {
    flags
        .iter()
        .enumerate()
        .fold(0i16, |acc, (i, &b)| acc | ((b as i16) << i))
}

fn bit(v: i16, i: u32) -> bool {
    v & (1 << i) != 0
}

// ---------------------------------------------------------------------------
// Objects
// ---------------------------------------------------------------------------

/// Encode one object as a record (without its length prefix), stored
/// under `key` (its key in the drawing, normally its own handle).
///
/// Format 2.0 records leave out what a reader can restore: the type name
/// of DWG's fixed types (it follows from the type number), and every field
/// equal to its default (`native::schema`).
pub fn write_object(w: &mut TagWriter, key: Handle, o: &Object) {
    if w.full || crate::dwg::fixed_type_name(o.type_code) != Some(o.type_name.as_str()) {
        w.str(0, &o.type_name);
    }
    w.handle(5, key);
    w.u16(c::TYPE_CODE, o.type_code);
    if o.handle != key {
        w.handle(c::OWN_HANDLE, o.handle);
    }
    w.opt_handle(330, o.owner);
    if !o.reactors.is_empty() {
        w.str(102, "{ACAD_REACTORS");
        for h in &o.reactors {
            w.handle(330, *h);
        }
        w.str(102, "}");
    }
    if !o.xdictionary.is_null() {
        w.str(102, "{ACAD_XDICTIONARY");
        w.handle(360, o.xdictionary);
        w.str(102, "}");
    }
    if o.offset != 0 {
        w.u64(c::SRC_OFFSET, o.offset as u64);
    }
    if o.size != 0 {
        w.u64(c::SRC_SIZE, o.size as u64);
    }
    if let Some(n) = &o.note {
        w.str(c::NOTE, n);
    }
    if let Some(e) = &o.entity {
        // The space tag marks the entity part; 1.x writes 100 "AcDbEntity"
        // before it.
        if w.full {
            w.str(100, "AcDbEntity");
        }
        w.i16(
            c::SPACE,
            match e.space {
                Space::Owned => 0,
                Space::Paper => 1,
                Space::Model => 2,
            },
        );
        w.opt_handle(c::LAYER, e.layer);
        match e.linetype {
            LinetypeRef::ByLayer => w.i16_d(c::LTYPE_KIND, 0, 0),
            LinetypeRef::ByBlock => w.i16(c::LTYPE_KIND, 1),
            LinetypeRef::Continuous => w.i16(c::LTYPE_KIND, 2),
            LinetypeRef::Handle(h) => {
                w.i16(c::LTYPE_KIND, 3);
                w.handle(c::LTYPE, h);
            }
        }
        put_color(w, e.color);
        if let Some(t) = e.transparency {
            w.u32(440, t);
        }
        w.f64_d(48, e.linetype_scale, 1.0);
        w.u8_d(370, e.lineweight, LW_BY_LAYER);
        if e.invisible {
            w.i16(60, 1);
        }
        w.opt_handle(390, e.plotstyle);
        w.opt_handle(347, e.material);
    }
    // A damaged kind has no kind tag of its own: its unknown tags hold the
    // record from where that tag should be, written back as found.
    if !matches!(o.kind, ObjectKind::Damaged(_)) {
        write_kind(w, &o.kind);
    }
    w.encoded(&o.unknown_tags);
    for x in &o.xdata {
        write_xdata(w, x);
    }
}

/// One application's extended data: `1001` with the application name (or
/// the APPID handle when the name is unknown), then one tag per value.
pub fn write_xdata(w: &mut TagWriter, x: &XData) {
    use xcode as x_;
    match &x.app {
        XDataApp::Name(n) => w.str(x_::APP, n),
        XDataApp::Handle(h) => w.handle(x_::APP, *h),
    }
    for v in &x.items {
        match v {
            XValue::Str(s) => w.str(x_::STR, s),
            XValue::Open => w.str(x_::BRACE, "{"),
            XValue::Close => w.str(x_::BRACE, "}"),
            XValue::Layer(h) => w.handle(x_::LAYER, *h),
            XValue::Bytes(b) => w.bytes(x_::BYTES, b),
            XValue::Handle(h) => w.handle(x_::HANDLE, *h),
            XValue::Point(p) => w.p3(x_::POINT, *p),
            XValue::Position(p) => w.p3(x_::POSITION, *p),
            XValue::Displacement(p) => w.p3(x_::DISPLACEMENT, *p),
            XValue::Direction(p) => w.p3(x_::DIRECTION, *p),
            XValue::Real(f) => w.f64(x_::REAL, *f),
            XValue::Distance(f) => w.f64(x_::DISTANCE, *f),
            XValue::ScaleFactor(f) => w.f64(x_::SCALE, *f),
            XValue::Int16(i) => w.i16(x_::INT16, *i),
            XValue::Int32(i) => w.i32(x_::INT32, *i),
            XValue::Undecoded { version, data } => {
                w.str(c::XDATA_RAW_VERSION, version.magic());
                w.bytes(c::XDATA_RAW, data);
            }
        }
    }
}

/// Extended data blocks, from the first `1001` to the end of the record.
fn read_xdata(data: &[u8]) -> Result<Vec<XData>> {
    use xcode as x_;
    let mut r = TagReader::new(data);
    let mut out: Vec<XData> = Vec::new();
    while let Some(t) = r.next()? {
        if t.code == x_::APP {
            let app = match t.value {
                Value::Str(s) => XDataApp::Name(s),
                Value::Handle(h) => XDataApp::Handle(Handle(h)),
                other => {
                    return Err(Error::format(format!(
                        "group 1001: expected a name or handle, found {}",
                        other.type_name()
                    )))
                }
            };
            out.push(XData {
                app,
                items: Vec::new(),
            });
            continue;
        }
        let Some(block) = out.last_mut() else {
            return Err(Error::format(
                "extended data does not start with group 1001",
            ));
        };
        let v = match (t.code, t.value) {
            (x_::STR, Value::Str(s)) => XValue::Str(s),
            (x_::BRACE, Value::Str(s)) => match s.as_str() {
                "{" => XValue::Open,
                "}" => XValue::Close,
                _ => return Err(Error::format(format!("group 1002: {s:?}"))),
            },
            (x_::LAYER, Value::Handle(h)) => XValue::Layer(Handle(h)),
            (x_::BYTES, Value::Bytes(b)) => XValue::Bytes(b),
            (x_::HANDLE, Value::Handle(h)) => XValue::Handle(Handle(h)),
            (x_::POINT, Value::P3(p)) => XValue::Point(p),
            (x_::POSITION, Value::P3(p)) => XValue::Position(p),
            (x_::DISPLACEMENT, Value::P3(p)) => XValue::Displacement(p),
            (x_::DIRECTION, Value::P3(p)) => XValue::Direction(p),
            (x_::REAL, Value::F64(f)) => XValue::Real(f),
            (x_::DISTANCE, Value::F64(f)) => XValue::Distance(f),
            (x_::SCALE, Value::F64(f)) => XValue::ScaleFactor(f),
            (x_::INT16, Value::I16(i)) => XValue::Int16(i),
            (x_::INT32, Value::I32(i)) => XValue::Int32(i),
            (c::XDATA_RAW_VERSION, Value::Str(tag)) => {
                let version = DwgVersion::from_tag(&tag)
                    .ok_or_else(|| Error::format(format!("unknown DWG version {tag:?}")))?;
                XValue::Undecoded {
                    version,
                    data: r.bytes(c::XDATA_RAW)?,
                }
            }
            (code, v) => {
                return Err(Error::format(format!(
                    "group {code} ({}) is not extended data",
                    v.type_name()
                )))
            }
        };
        block.items.push(v);
    }
    Ok(out)
}

/// Split what follows an object's own fields (from byte `at`): tags a
/// newer writer added, kept as bytes, then extended data from the first
/// top-level `1001`. Extended data that does not decode is kept as bytes
/// too, whole, so the record still round-trips; the message says why.
/// A tag that cannot be parsed at all is damage (every value type can be
/// stepped over): the rest of the record is kept as bytes and reported.
fn split_tail(data: &[u8], at: usize) -> (Vec<u8>, Vec<XData>, Option<String>) {
    let mut p = at;
    let mut xstart = None;
    while p < data.len() {
        let here = p;
        match read_tag(data, &mut p) {
            Ok(t) if t.code == xcode::APP => {
                xstart = Some(here);
                break;
            }
            Ok(_) => {}
            Err(e) => {
                return (
                    data[at..].to_vec(),
                    Vec::new(),
                    Some(format!("tags from byte {here} kept unread: {e}")),
                )
            }
        }
    }
    let Some(x) = xstart else {
        return (data[at..].to_vec(), Vec::new(), None);
    };
    match read_xdata(&data[x..]) {
        Ok(xdata) => (data[at..x].to_vec(), xdata, None),
        Err(e) => (
            data[at..].to_vec(),
            Vec::new(),
            Some(format!("extended data kept undecoded: {e}")),
        ),
    }
}

/// `v_code`: 73 for TEXT, 74 for ATTRIB/ATTDEF (as in DXF, where 73 is an
/// attribute's field length).
fn write_text(w: &mut TagWriter, t: &Text, v_code: u16) {
    w.p2(10, t.insertion);
    w.p2(11, t.alignment);
    w.f64_d(38, t.elevation, 0.0);
    w.p3_d(210, t.extrusion, Z);
    w.f64_d(39, t.thickness, 0.0);
    w.f64_d(51, t.oblique, 0.0);
    w.f64_d(50, t.rotation, 0.0);
    w.f64(40, t.height);
    w.f64_d(41, t.width_factor, 1.0);
    w.str(1, &t.value);
    w.u16_d(71, t.generation, 0);
    w.u16_d(72, t.h_align, 0);
    w.u16_d(v_code, t.v_align, 0);
    w.opt_handle(c::STYLE, t.style);
}

fn read_text(r: &mut TagReader, v_code: u16) -> Result<Text> {
    Ok(Text {
        insertion: r.p2(10)?,
        alignment: r.p2(11)?,
        elevation: r.f64_or(38, 0.0)?,
        extrusion: r.p3_or(210, Z)?,
        thickness: r.f64_or(39, 0.0)?,
        oblique: r.f64_or(51, 0.0)?,
        rotation: r.f64_or(50, 0.0)?,
        height: r.f64(40)?,
        width_factor: r.f64_or(41, 1.0)?,
        value: r.str(1)?,
        generation: r.u16_or(71, 0)?,
        h_align: r.u16_or(72, 0)?,
        v_align: r.u16_or(v_code, 0)?,
        style: r.opt_handle(c::STYLE)?,
    })
}

fn write_kind(w: &mut TagWriter, k: &ObjectKind) {
    use ObjectKind as K;
    match k {
        K::Unsupported => w.i16(c::KIND, kind::UNSUPPORTED),
        K::Failed(m) => {
            w.i16(c::KIND, kind::FAILED);
            w.str(c::MESSAGE, m);
        }
        // Written by `write_object` from the unknown tags.
        K::Damaged(_) => {}
        // Its fields follow as the object's unknown tags.
        K::NewerKind(n) => {
            debug_assert!(
                !(kind::UNSUPPORTED..=kind::LAST).contains(n),
                "NewerKind({n}) is a kind this version knows"
            );
            w.i16(c::KIND, *n)
        }
        K::Line(l) => {
            w.i16(c::KIND, kind::LINE);
            w.p3(10, l.start);
            w.p3(11, l.end);
            w.f64_d(39, l.thickness, 0.0);
            w.p3_d(210, l.extrusion, Z);
        }
        K::Circle(ci) => {
            w.i16(c::KIND, kind::CIRCLE);
            w.p3(10, ci.center);
            w.f64(40, ci.radius);
            w.f64_d(39, ci.thickness, 0.0);
            w.p3_d(210, ci.extrusion, Z);
        }
        K::Arc(a) => {
            w.i16(c::KIND, kind::ARC);
            w.p3(10, a.center);
            w.f64(40, a.radius);
            w.f64_d(39, a.thickness, 0.0);
            w.p3_d(210, a.extrusion, Z);
            w.f64_d(50, a.start_angle, 0.0);
            w.f64(51, a.end_angle);
        }
        K::Point(p) => {
            w.i16(c::KIND, kind::POINT);
            w.p3(10, p.position);
            w.f64_d(39, p.thickness, 0.0);
            w.p3_d(210, p.extrusion, Z);
            w.f64_d(50, p.x_axis_angle, 0.0);
        }
        K::Ellipse(e) => {
            w.i16(c::KIND, kind::ELLIPSE);
            w.p3(10, e.center);
            w.p3(11, e.major_axis);
            w.p3_d(210, e.extrusion, Z);
            w.f64(40, e.axis_ratio);
            w.f64_d(41, e.start_param, 0.0);
            w.f64_d(42, e.end_param, TAU);
        }
        K::LwPolyline(p) => {
            w.i16(c::KIND, kind::LWPOLYLINE);
            w.u16_d(c::DWG_FLAGS, p.flags, 0);
            w.f64_d(43, p.const_width, 0.0);
            w.f64_d(38, p.elevation, 0.0);
            w.f64_d(39, p.thickness, 0.0);
            w.p3_d(210, p.extrusion, Z);
            put_p2s(w, 90, 10, &p.points);
            w.count_d(c::BULGE_COUNT, p.bulges.len());
            for b in &p.bulges {
                w.f64(42, *b);
            }
            w.count_d(c::WIDTH_COUNT, p.widths.len());
            for (s, e) in &p.widths {
                w.f64(40, *s);
                w.f64(41, *e);
            }
            w.count_d(c::ID_COUNT, p.vertex_ids.len());
            for id in &p.vertex_ids {
                w.u32(91, *id);
            }
        }
        K::Polyline(p) => {
            w.i16(c::KIND, kind::POLYLINE);
            w.i16(
                c::SUBKIND,
                match p.kind {
                    PolylineKind::Polyline2d => 0,
                    PolylineKind::Polyline3d => 1,
                    PolylineKind::PolyfaceMesh => 2,
                    PolylineKind::PolygonMesh => 3,
                },
            );
            w.u16_d(70, p.flags, 0);
            w.u16_d(75, p.curve_type, 0);
            w.f64_d(40, p.start_width, 0.0);
            w.f64_d(41, p.end_width, 0.0);
            w.f64_d(39, p.thickness, 0.0);
            w.f64_d(38, p.elevation, 0.0);
            w.p3_d(210, p.extrusion, Z);
            put_handles_d(w, &p.vertices);
            w.opt_handle(c::FIRST, p.first_vertex);
            w.opt_handle(c::LAST, p.last_vertex);
            w.opt_handle(c::SEQEND, p.seqend);
        }
        K::Vertex(v) => {
            w.i16(c::KIND, kind::VERTEX);
            w.i16(
                c::SUBKIND,
                match v.kind {
                    VertexKind::Vertex2d => 0,
                    VertexKind::Vertex3d => 1,
                    VertexKind::Mesh => 2,
                    VertexKind::PfaceVertex => 3,
                    VertexKind::PfaceFace => 4,
                },
            );
            w.u8_d(70, v.flags, 0);
            w.p3(10, v.point);
            w.f64_d(40, v.start_width, 0.0);
            w.f64_d(41, v.end_width, 0.0);
            w.f64_d(42, v.bulge, 0.0);
            w.f64_d(50, v.tangent_dir, 0.0);
            w.u32_d(91, v.id, 0);
            for (i, f) in v.face.iter().enumerate() {
                w.i16_d(71 + i as u16, *f, 0);
            }
        }
        K::SeqEnd => w.i16(c::KIND, kind::SEQEND),
        K::Text(t) => {
            w.i16(c::KIND, kind::TEXT);
            write_text(w, t, 73);
        }
        K::Attribute(a) => {
            w.i16(c::KIND, kind::ATTRIBUTE);
            write_text(w, &a.text, 74);
            w.str(2, &a.tag);
            w.str_d(3, &a.prompt, "");
            w.u8_d(70, a.flags, 0);
            w.u16_d(73, a.field_length, 0);
            w.bool_d(c::IS_DEFINITION, a.is_definition, false);
        }
        K::MText(m) => {
            w.i16(c::KIND, kind::MTEXT);
            w.p3(10, m.insertion);
            w.p3_d(210, m.extrusion, Z);
            w.p3_d(11, m.x_axis, X);
            w.f64_d(41, m.rect_width, 0.0);
            w.f64_d(46, m.rect_height, 0.0);
            w.f64(40, m.text_height);
            w.u16_d(71, m.attachment, 1);
            w.u16_d(72, m.drawing_dir, 1);
            w.str(1, &m.contents);
            w.f64_d(44, m.linespacing_factor, 1.0);
            w.opt_handle(c::STYLE, m.style);
        }
        K::Insert(i) => {
            w.i16(c::KIND, kind::INSERT);
            w.p3(10, i.insertion);
            w.f64_d(41, i.scale.x, 1.0);
            w.f64_d(42, i.scale.y, 1.0);
            w.f64_d(43, i.scale.z, 1.0);
            w.f64_d(50, i.rotation, 0.0);
            w.p3_d(210, i.extrusion, Z);
            w.bool_d(66, i.has_attribs, false);
            w.opt_handle(c::BLOCK_HEADER, i.block_header);
            put_handles_d(w, &i.attribs);
            w.opt_handle(c::FIRST, i.first_attrib);
            w.opt_handle(c::LAST, i.last_attrib);
            w.opt_handle(c::SEQEND, i.seqend);
            if let Some((cols, rows, cs, rs)) = i.array {
                w.u16(70, cols);
                w.u16(71, rows);
                w.f64(44, cs);
                w.f64(45, rs);
            }
        }
        K::Solid(s) => {
            w.i16(c::KIND, kind::SOLID);
            w.f64_d(39, s.thickness, 0.0);
            w.f64_d(38, s.elevation, 0.0);
            for (i, p) in s.corners.iter().enumerate() {
                w.p2(10 + i as u16, *p);
            }
            w.p3_d(210, s.extrusion, Z);
            w.bool_d(c::IS_TRACE, s.is_trace, false);
        }
        K::Face3d(f) => {
            w.i16(c::KIND, kind::FACE3D);
            for (i, p) in f.corners.iter().enumerate() {
                w.p3(10 + i as u16, *p);
            }
            w.u16_d(70, f.invisible_edges, 0);
        }
        K::Spline(s) => {
            w.i16(c::KIND, kind::SPLINE);
            // DXF spline flags: 1 closed, 2 periodic, 4 rational.
            w.i16_d(70, bits(&[s.closed, s.periodic, s.rational]), 0);
            w.u32(71, s.degree);
            put_f64s_d(w, c::KNOT_COUNT, 40, &s.knots);
            put_p3s_d(w, c::CTRL_COUNT, 10, &s.control_points);
            put_f64s_d(w, c::WEIGHT_COUNT, 41, &s.weights);
            put_p3s_d(w, c::FIT_COUNT, 11, &s.fit_points);
            w.f64_d(44, s.fit_tolerance, 0.0);
            w.p3_d(12, s.start_tangent, ORIGIN3);
            w.p3_d(13, s.end_tangent, ORIGIN3);
        }
        K::Dimension(d) => {
            w.i16(c::KIND, kind::DIMENSION);
            w.i16(
                c::SUBKIND,
                match d.kind {
                    DimensionKind::Linear => 0,
                    DimensionKind::Aligned => 1,
                    DimensionKind::Ordinate => 2,
                    DimensionKind::Angular3Pt => 3,
                    DimensionKind::Angular2Line => 4,
                    DimensionKind::Radius => 5,
                    DimensionKind::Diameter => 6,
                },
            );
            w.p3_d(210, d.extrusion, Z);
            w.p2(11, d.text_midpoint);
            w.f64_d(38, d.elevation, 0.0);
            w.u8_d(c::DWG_FLAGS, d.flags, 0);
            w.str_d(1, &d.user_text, "");
            w.f64_d(53, d.text_rotation, 0.0);
            w.f64_d(51, d.horizontal_direction, 0.0);
            w.p3_d(c::INSERT_SCALE, d.insert_scale, ONES);
            w.f64_d(c::INSERT_ROTATION, d.insert_rotation, 0.0);
            w.u16_d(71, d.attachment, 0);
            w.f64(42, d.measurement);
            w.p2_d(c::CLONE_INSERTION, d.clone_insertion, ORIGIN2);
            w.p3(10, d.pt10);
            w.p3_d(13, d.pt13, ORIGIN3);
            w.p3_d(14, d.pt14, ORIGIN3);
            w.p3_d(15, d.pt15, ORIGIN3);
            w.p2_d(16, d.pt16, ORIGIN2);
            w.f64_d(52, d.ext_line_rotation, 0.0);
            w.f64_d(50, d.dim_rotation, 0.0);
            w.f64_d(40, d.leader_length, 0.0);
            w.opt_handle(c::DIMSTYLE, d.dimstyle);
            w.opt_handle(c::BLOCK, d.block);
        }
        K::Hatch(h) => {
            w.i16(c::KIND, kind::HATCH);
            w.f64_d(38, h.elevation, 0.0);
            w.p3_d(210, h.extrusion, Z);
            w.str(2, &h.pattern_name);
            w.bool_d(70, h.solid_fill, false);
            w.bool_d(71, h.associative, false);
            w.bool_d(450, h.is_gradient, false);
            w.u16_d(75, h.style, 0);
            w.u16_d(76, h.pattern_type, 1);
            w.f64_d(52, h.pattern_angle, 0.0);
            w.f64_d(41, h.pattern_scale, 1.0);
            w.count(c::PATH_COUNT, h.paths.len());
            for p in &h.paths {
                write_hatch_path(w, p);
            }
            // Fields new in 2.4: left out at their defaults in every
            // form, so the 1.x form stays as 1.x wrote it.
            if h.pattern_double {
                w.bool(77, true);
            }
            if !h.pattern_lines.is_empty() {
                w.count(78, h.pattern_lines.len());
                for l in &h.pattern_lines {
                    w.f64(53, l.angle);
                    w.p2(43, l.base);
                    w.p2(45, l.offset);
                    w.count(79, l.dashes.len());
                    for d in &l.dashes {
                        w.f64(49, *d);
                    }
                }
            }
            if !h.seed_points.is_empty() {
                w.count(98, h.seed_points.len());
                for p in &h.seed_points {
                    w.p2(super::objects::code::HATCH_SEED, *p);
                }
            }
        }
        K::Xline(x) | K::Ray(x) => {
            w.i16(c::KIND, kind::XLINE);
            w.p3(10, x.base);
            w.p3(11, x.direction);
            w.bool_d(super::objects::code::IS_RAY, matches!(k, K::Ray(_)), false);
        }
        K::Leader(l) => {
            w.i16(c::KIND, kind::LEADER);
            super::objects::write_leader(w, l);
        }
        K::PaperViewport(v) => {
            w.i16(c::KIND, kind::VIEWPORT);
            super::objects::write_paper_viewport(w, v);
        }
        K::BlockBegin { name } => {
            w.i16(c::KIND, kind::BLOCK_BEGIN);
            w.str(2, name);
        }
        K::BlockEnd => w.i16(c::KIND, kind::BLOCK_END),
        K::BlockHeader(b) => {
            w.i16(c::KIND, kind::BLOCK_HEADER);
            w.str(2, &b.name);
            // DXF block flags: 1 anonymous, 2 has attributes, 4 xref, 8 overlay.
            w.i16_d(
                70,
                bits(&[b.anonymous, b.has_attribs, b.is_xref, b.is_overlaid]),
                0,
            );
            w.p3_d(10, b.base_point, ORIGIN3);
            w.str_d(1, &b.xref_path, "");
            w.str_d(4, &b.description, "");
            w.u16_d(c::INSERT_UNITS, b.insert_units, 0);
            w.opt_handle(c::BLOCK_ENTITY, b.block_entity);
            put_handles_d(w, &b.entities);
            w.opt_handle(c::FIRST, b.first_entity);
            w.opt_handle(c::LAST, b.last_entity);
            w.opt_handle(c::ENDBLK, b.endblk);
            w.opt_handle(340, b.layout);
            // Fields new in 2.0: left out at their defaults in the 1.x
            // form too, which never had them.
            let p = &b.part;
            if !p.material.is_empty() {
                w.str(c::PART_MATERIAL, &p.material);
            }
            if !p.grade.is_empty() {
                w.str(c::PART_GRADE, &p.grade);
            }
            if p.thickness.to_bits() != 0f64.to_bits() {
                w.f64(c::PART_THICKNESS, p.thickness);
            }
            if p.quantity != 0 {
                w.u32(c::PART_QUANTITY, p.quantity);
            }
        }
        K::Layer(l) => {
            w.i16(c::KIND, kind::LAYER);
            w.str(2, &l.name);
            w.u16_d(c::DWG_FLAGS, l.flags, 0);
            w.i16(c::LAYER_STATE, bits(&[l.frozen, l.off, l.locked, l.plot]));
            put_color(w, l.color);
            w.opt_handle(c::LAYER_LTYPE, l.linetype);
            w.u8_d(370, l.lineweight, LW_DEFAULT);
        }
        K::Control(ct) => {
            w.i16(c::KIND, kind::CONTROL);
            put_handles_d(w, &ct.entries);
            w.opt_handle(c::MODEL_SPACE, ct.model_space);
            w.opt_handle(c::PAPER_SPACE, ct.paper_space);
        }
        K::NamedRecord(n) => {
            w.i16(c::KIND, kind::NAMED_RECORD);
            w.str(2, &n.name);
        }
        K::Linetype(l) => {
            w.i16(c::KIND, kind::LINETYPE);
            w.str(2, &l.name);
            w.u16_d(70, l.flags, 0);
            w.str_d(3, &l.description, "");
            w.f64_d(40, l.pattern_length, 0.0);
            w.count_d(73, l.elements.len());
            for e in &l.elements {
                w.f64(49, e.length);
                w.u16_d(74, e.complex, 0);
                w.u16_d(75, e.shape, 0);
                w.opt_handle(340, e.style);
                w.f64_d(46, e.scale, 1.0);
                w.f64_d(50, e.rotation, 0.0);
                w.p2_d(44, e.offset, ORIGIN2);
                w.str_d(9, &e.text, "");
            }
        }
        K::TextStyle(t) => {
            w.i16(c::KIND, kind::TEXT_STYLE);
            w.str(2, &t.name);
            w.u16_d(70, t.flags, 0);
            w.f64_d(40, t.height, 0.0);
            w.f64_d(41, t.width_factor, 1.0);
            w.f64_d(50, t.oblique, 0.0);
            w.u16_d(71, t.generation, 0);
            w.f64_d(42, t.last_height, 0.0);
            w.str_d(3, &t.font_file, "");
            w.str_d(4, &t.bigfont_file, "");
        }
        K::DimStyle(d) => {
            w.i16(c::KIND, kind::DIM_STYLE);
            w.str(2, &d.name);
            w.u16_d(70, d.flags, 0);
            w.str_d(3, &d.post_text, "");
            w.f64_d(40, d.scale, 1.0);
            w.f64_d(41, d.arrow_size, 0.0);
            w.f64_d(42, d.ext_offset, 0.0);
            w.f64_d(43, d.baseline_spacing, 0.0);
            w.f64_d(44, d.ext_extension, 0.0);
            w.f64_d(45, d.rounding, 0.0);
            w.f64_d(46, d.dim_line_extension, 0.0);
            w.f64_d(140, d.text_height, 0.0);
            w.f64_d(141, d.center_mark, 0.0);
            w.f64_d(142, d.tick_size, 0.0);
            w.f64_d(143, d.alt_factor, 25.4);
            w.f64_d(144, d.linear_factor, 1.0);
            w.f64_d(147, d.text_gap, 0.0);
            w.bool_d(73, d.text_inside_horizontal, true);
            w.bool_d(74, d.text_outside_horizontal, true);
            w.u16_d(77, d.text_above, 0);
            w.u16_d(78, d.zero_suppression, 0);
            w.bool_d(170, d.alt_units, false);
            w.u16_d(171, d.alt_decimals, 2);
            w.u32_d(
                c::DIM_LINE_COLOR,
                d.line_color.to_method_word(),
                COLOR_WORD_BY_BLOCK,
            );
            w.u32_d(
                c::DIM_EXT_COLOR,
                d.ext_color.to_method_word(),
                COLOR_WORD_BY_BLOCK,
            );
            w.u32_d(
                c::DIM_TEXT_COLOR,
                d.text_color.to_method_word(),
                COLOR_WORD_BY_BLOCK,
            );
            w.u16_d(271, d.decimals, 2);
            w.u16_d(276, d.fraction_format, 0);
            w.u16_d(277, d.unit_format, 2);
            w.u32_d(278, d.decimal_separator as u32, '.' as i32);
            w.u16_d(280, d.text_justify, 0);
            w.opt_handle(340, d.text_style);
            w.u8_d(371, d.line_weight, LW_BY_BLOCK);
            w.u8_d(372, d.ext_weight, LW_BY_BLOCK);
        }
        K::Viewport(v) => {
            w.i16(c::KIND, kind::VPORT);
            super::objects::write_vport(w, v);
        }
        K::DimStyleRecord(d) => {
            w.i16(c::KIND, kind::DIMSTYLE_RECORD);
            super::objects::write_dimstyle(w, d);
        }
        K::NonGraphic(n) => {
            use super::objects as ob;
            match n {
                NonGraphic::Dictionary(d) => {
                    w.i16(c::KIND, kind::DICTIONARY);
                    ob::write_dictionary(w, d);
                }
                NonGraphic::XRecord(x) => {
                    w.i16(c::KIND, kind::XRECORD);
                    ob::write_xrecord(w, x);
                }
                NonGraphic::Placeholder => w.i16(c::KIND, kind::PLACEHOLDER),
                NonGraphic::DictionaryVar { schema, value } => {
                    w.i16(c::KIND, kind::DICTIONARY_VAR);
                    ob::write_dictionary_var(w, *schema, value);
                }
                NonGraphic::Scale(sc) => {
                    w.i16(c::KIND, kind::SCALE);
                    ob::write_scale(w, sc);
                }
                NonGraphic::Layout(l) => {
                    w.i16(c::KIND, kind::LAYOUT);
                    ob::write_layout(w, l);
                }
                NonGraphic::MLineStyle(m) => {
                    w.i16(c::KIND, kind::MLINESTYLE);
                    ob::write_mlinestyle(w, m);
                }
                NonGraphic::Groups(g) => {
                    w.i16(c::KIND, kind::GROUPS);
                    ob::write_groups(w, g);
                }
            }
        }
    }
}

/// A color stored as a method word (dimension style colours); by block
/// when left out.
fn get_method_color(r: &mut TagReader, code: u16) -> Result<Color> {
    let word = r.u32_or(code, COLOR_WORD_BY_BLOCK)?;
    match word >> 24 {
        0xC0 | 0xC1 | 0xC2 | 0xC8 => Ok(Color::from_method_word(word, 7)),
        0xC3 if (1..=255).contains(&(word & 0xFF)) => Ok(Color::from_method_word(word, 7)),
        _ => Err(Error::format(format!(
            "group {code}: color word {word:#010X}"
        ))),
    }
}

fn write_hatch_path(w: &mut TagWriter, p: &HatchPath) {
    match p {
        HatchPath::Edges { flags, edges } => {
            w.i16(c::SUBKIND, 0);
            w.u32_d(92, *flags, 0);
            w.count(c::EDGE_COUNT, edges.len());
            for e in edges {
                match e {
                    HatchEdge::Line { a, b } => {
                        w.i16(72, 1);
                        w.p2(10, *a);
                        w.p2(11, *b);
                    }
                    HatchEdge::Arc {
                        center,
                        radius,
                        start,
                        end,
                        ccw,
                    } => {
                        w.i16(72, 2);
                        w.p2(10, *center);
                        w.f64(40, *radius);
                        w.f64(50, *start);
                        w.f64(51, *end);
                        w.bool(73, *ccw);
                    }
                    HatchEdge::Ellipse {
                        center,
                        major,
                        ratio,
                        start,
                        end,
                        ccw,
                    } => {
                        w.i16(72, 3);
                        w.p2(10, *center);
                        w.p2(11, *major);
                        w.f64(40, *ratio);
                        w.f64(50, *start);
                        w.f64(51, *end);
                        w.bool(73, *ccw);
                    }
                    HatchEdge::Spline {
                        degree,
                        knots,
                        control_points,
                        weights,
                    } => {
                        w.i16(72, 4);
                        w.u32(94, *degree);
                        put_f64s(w, c::KNOT_COUNT, 40, knots);
                        put_p2s(w, c::CTRL_COUNT, 10, control_points);
                        put_f64s(w, c::WEIGHT_COUNT, 42, weights);
                    }
                }
            }
        }
        HatchPath::Polyline {
            flags,
            closed,
            vertices,
        } => {
            w.i16(c::SUBKIND, 1);
            w.u32_d(92, *flags, 0);
            w.bool(73, *closed);
            w.count(93, vertices.len());
            for (p, bulge) in vertices {
                w.p2(10, *p);
                w.f64_d(42, *bulge, 0.0);
            }
        }
    }
}

fn read_hatch_path(r: &mut TagReader) -> Result<HatchPath> {
    match r.i16(c::SUBKIND)? {
        0 => {
            let flags = r.u32_or(92, 0)?;
            let n = r.count(c::EDGE_COUNT, 5)?;
            let mut edges = Vec::with_capacity(n);
            for _ in 0..n {
                edges.push(match r.i16(72)? {
                    1 => HatchEdge::Line {
                        a: r.p2(10)?,
                        b: r.p2(11)?,
                    },
                    2 => HatchEdge::Arc {
                        center: r.p2(10)?,
                        radius: r.f64(40)?,
                        start: r.f64(50)?,
                        end: r.f64(51)?,
                        ccw: r.bool(73)?,
                    },
                    3 => HatchEdge::Ellipse {
                        center: r.p2(10)?,
                        major: r.p2(11)?,
                        ratio: r.f64(40)?,
                        start: r.f64(50)?,
                        end: r.f64(51)?,
                        ccw: r.bool(73)?,
                    },
                    4 => HatchEdge::Spline {
                        degree: r.u32(94)?,
                        knots: get_f64s(r, c::KNOT_COUNT, 40)?,
                        control_points: get_p2s(r, c::CTRL_COUNT, 10)?,
                        weights: get_f64s(r, c::WEIGHT_COUNT, 42)?,
                    },
                    t => return Err(Error::format(format!("hatch edge type {t}"))),
                });
            }
            Ok(HatchPath::Edges { flags, edges })
        }
        1 => {
            let flags = r.u32_or(92, 0)?;
            let closed = r.bool(73)?;
            let n = r.count(93, P2_BYTES)?;
            let mut vertices = Vec::with_capacity(n);
            for _ in 0..n {
                vertices.push((r.p2(10)?, r.f64_or(42, 0.0)?));
            }
            Ok(HatchPath::Polyline {
                flags,
                closed,
                vertices,
            })
        }
        t => Err(Error::format(format!("hatch path kind {t}"))),
    }
}

/// Decode a record into its key and object. Header fields that fail are
/// an error for the whole record; a failure in the type-specific part
/// yields an object whose kind is `Failed`, so ownership and handles
/// survive, and the fields that failed are kept as unknown tags.
pub fn read_object(data: &[u8]) -> Result<(Handle, Object)> {
    read_object_checked(data).map(|(key, o, _)| (key, o))
}

/// [`read_object`], also saying what did not decode (a kind's fields, or
/// the extended data), if anything. Whatever did not decode is kept in
/// `unknown_tags`, so writing the object back loses nothing.
pub fn read_object_checked(data: &[u8]) -> Result<(Handle, Object, Option<String>)> {
    let mut r = TagReader::new(data);
    // 2.0 leaves out the type name of DWG's fixed types.
    let named = r.opt_str(0)?;
    let key = r.handle(5)?;
    let type_code = r.u16(c::TYPE_CODE)?;
    let type_name = match named {
        Some(n) => n,
        None => crate::dwg::fixed_type_name(type_code)
            .map(str::to_string)
            .unwrap_or_else(|| format!("TYPE_{type_code:#X}")),
    };
    let own = r.opt_handle(c::OWN_HANDLE)?;
    let handle = if own.is_null() { key } else { own };
    let owner = r.opt_handle(330)?;
    let mut reactors = Vec::new();
    let mut xdictionary = Handle::NULL;
    while r.peek_code()? == Some(102) {
        let open = r.str(102)?;
        match open.as_str() {
            "{ACAD_REACTORS" => {
                while r.peek_code()? == Some(330) {
                    reactors.push(r.handle(330)?);
                }
            }
            "{ACAD_XDICTIONARY" => xdictionary = r.handle(360)?,
            other => return Err(Error::format(format!("unknown group 102 {other:?}"))),
        }
        let close = r.str(102)?;
        if close != "}" {
            return Err(Error::format(format!(
                "group 102 group not closed: {close:?}"
            )));
        }
    }
    let offset = r.opt_i64(c::SRC_OFFSET)?.unwrap_or(0) as u64 as usize;
    let size = r.opt_i64(c::SRC_SIZE)?.unwrap_or(0) as u64 as usize;
    let note = r.opt_str(c::NOTE)?;
    // 1.x marks the entity part with 100 "AcDbEntity"; 2.0 with the space
    // tag alone.
    let marked = r.opt_str(100)?.is_some();
    let entity = if marked || r.peek_code()? == Some(c::SPACE) {
        let space = match r.i16(c::SPACE)? {
            0 => Space::Owned,
            1 => Space::Paper,
            2 => Space::Model,
            v => return Err(Error::format(format!("space {v}"))),
        };
        let layer = r.opt_handle(c::LAYER)?;
        let linetype = match r.i16_or(c::LTYPE_KIND, 0)? {
            0 => LinetypeRef::ByLayer,
            1 => LinetypeRef::ByBlock,
            2 => LinetypeRef::Continuous,
            3 => LinetypeRef::Handle(r.handle(c::LTYPE)?),
            v => return Err(Error::format(format!("linetype kind {v}"))),
        };
        let color = get_color(&mut r)?;
        let transparency = r.opt_i32(440)?.map(|v| v as u32);
        let linetype_scale = r.f64_or(48, 1.0)?;
        let lineweight = r.u8_or(370, LW_BY_LAYER)?;
        let invisible = r.opt_i16(60)?.map(|v| v != 0).unwrap_or(false);
        let plotstyle = r.opt_handle(390)?;
        let material = r.opt_handle(347)?;
        Some(EntityCommon {
            space,
            layer,
            linetype,
            color,
            transparency,
            linetype_scale,
            lineweight,
            invisible,
            plotstyle,
            material,
        })
    } else {
        None
    };
    // The kind part is read on its own, so positions in an error message
    // count from the kind tag and stay the same after a save.
    let kind_at = r.position();
    let mut kr = TagReader::new(&data[kind_at..]);
    let (kind, tail_at, kind_problem) = match read_kind(&mut kr) {
        Ok(k) => (k, kind_at + kr.position(), None),
        Err(e) => {
            let m = format!("native record, kind fields: {e}");
            (ObjectKind::Damaged(m.clone()), kind_at, Some(m))
        }
    };
    let (unknown_tags, xdata, xdata_problem) = split_tail(data, tail_at);
    let o = Object {
        handle,
        type_code,
        type_name,
        owner,
        reactors,
        xdictionary,
        entity,
        kind,
        offset,
        size,
        note,
        raw: None,
        xdata,
        unknown_tags,
    };
    Ok((key, o, kind_problem.or(xdata_problem)))
}

fn read_kind(r: &mut TagReader) -> Result<ObjectKind> {
    use ObjectKind as K;
    Ok(match r.i16(c::KIND)? {
        kind::UNSUPPORTED => K::Unsupported,
        kind::FAILED => K::Failed(r.str(c::MESSAGE)?),
        kind::LINE => K::Line(Line {
            start: r.p3(10)?,
            end: r.p3(11)?,
            thickness: r.f64_or(39, 0.0)?,
            extrusion: r.p3_or(210, Z)?,
        }),
        kind::CIRCLE => K::Circle(Circle {
            center: r.p3(10)?,
            radius: r.f64(40)?,
            thickness: r.f64_or(39, 0.0)?,
            extrusion: r.p3_or(210, Z)?,
        }),
        kind::ARC => K::Arc(Arc {
            center: r.p3(10)?,
            radius: r.f64(40)?,
            thickness: r.f64_or(39, 0.0)?,
            extrusion: r.p3_or(210, Z)?,
            start_angle: r.f64_or(50, 0.0)?,
            end_angle: r.f64(51)?,
        }),
        kind::POINT => K::Point(Point {
            position: r.p3(10)?,
            thickness: r.f64_or(39, 0.0)?,
            extrusion: r.p3_or(210, Z)?,
            x_axis_angle: r.f64_or(50, 0.0)?,
        }),
        kind::ELLIPSE => K::Ellipse(Ellipse {
            center: r.p3(10)?,
            major_axis: r.p3(11)?,
            extrusion: r.p3_or(210, Z)?,
            axis_ratio: r.f64(40)?,
            start_param: r.f64_or(41, 0.0)?,
            end_param: r.f64_or(42, TAU)?,
        }),
        kind::LWPOLYLINE => {
            let flags = r.u16_or(c::DWG_FLAGS, 0)?;
            let const_width = r.f64_or(43, 0.0)?;
            let elevation = r.f64_or(38, 0.0)?;
            let thickness = r.f64_or(39, 0.0)?;
            let extrusion = r.p3_or(210, Z)?;
            let points = get_p2s(r, 90, 10)?;
            let bulges = get_f64s_or(r, c::BULGE_COUNT, 42)?;
            let n = r.count_or(c::WIDTH_COUNT, 2 * F64_BYTES)?;
            let mut widths = Vec::with_capacity(n);
            for _ in 0..n {
                widths.push((r.f64(40)?, r.f64(41)?));
            }
            let n = r.count_or(c::ID_COUNT, 7)?;
            let mut vertex_ids = Vec::with_capacity(n);
            for _ in 0..n {
                vertex_ids.push(r.u32(91)?);
            }
            K::LwPolyline(LwPolyline {
                flags,
                const_width,
                elevation,
                thickness,
                extrusion,
                points,
                bulges,
                widths,
                vertex_ids,
            })
        }
        kind::POLYLINE => K::Polyline(Polyline {
            kind: match r.i16(c::SUBKIND)? {
                0 => PolylineKind::Polyline2d,
                1 => PolylineKind::Polyline3d,
                2 => PolylineKind::PolyfaceMesh,
                3 => PolylineKind::PolygonMesh,
                v => return Err(Error::format(format!("polyline kind {v}"))),
            },
            flags: r.u16_or(70, 0)?,
            curve_type: r.u16_or(75, 0)?,
            start_width: r.f64_or(40, 0.0)?,
            end_width: r.f64_or(41, 0.0)?,
            thickness: r.f64_or(39, 0.0)?,
            elevation: r.f64_or(38, 0.0)?,
            extrusion: r.p3_or(210, Z)?,
            vertices: get_handles_or(r)?,
            first_vertex: r.opt_handle(c::FIRST)?,
            last_vertex: r.opt_handle(c::LAST)?,
            seqend: r.opt_handle(c::SEQEND)?,
        }),
        kind::VERTEX => K::Vertex(Vertex {
            kind: match r.i16(c::SUBKIND)? {
                0 => VertexKind::Vertex2d,
                1 => VertexKind::Vertex3d,
                2 => VertexKind::Mesh,
                3 => VertexKind::PfaceVertex,
                4 => VertexKind::PfaceFace,
                v => return Err(Error::format(format!("vertex kind {v}"))),
            },
            flags: r.u8_or(70, 0)?,
            point: r.p3(10)?,
            start_width: r.f64_or(40, 0.0)?,
            end_width: r.f64_or(41, 0.0)?,
            bulge: r.f64_or(42, 0.0)?,
            tangent_dir: r.f64_or(50, 0.0)?,
            id: r.u32_or(91, 0)?,
            face: [
                r.i16_or(71, 0)?,
                r.i16_or(72, 0)?,
                r.i16_or(73, 0)?,
                r.i16_or(74, 0)?,
            ],
        }),
        kind::SEQEND => K::SeqEnd,
        kind::TEXT => K::Text(read_text(r, 73)?),
        kind::ATTRIBUTE => K::Attribute(Attribute {
            text: read_text(r, 74)?,
            tag: r.str(2)?,
            prompt: r.str_or(3, "")?,
            flags: r.u8_or(70, 0)?,
            field_length: r.u16_or(73, 0)?,
            is_definition: r.bool_or(c::IS_DEFINITION, false)?,
        }),
        kind::MTEXT => K::MText(MText {
            insertion: r.p3(10)?,
            extrusion: r.p3_or(210, Z)?,
            x_axis: r.p3_or(11, X)?,
            rect_width: r.f64_or(41, 0.0)?,
            rect_height: r.f64_or(46, 0.0)?,
            text_height: r.f64(40)?,
            attachment: r.u16_or(71, 1)?,
            drawing_dir: r.u16_or(72, 1)?,
            contents: r.str(1)?,
            linespacing_factor: r.f64_or(44, 1.0)?,
            style: r.opt_handle(c::STYLE)?,
        }),
        kind::INSERT => {
            let insertion = r.p3(10)?;
            let scale = Vec3::new(r.f64_or(41, 1.0)?, r.f64_or(42, 1.0)?, r.f64_or(43, 1.0)?);
            let rotation = r.f64_or(50, 0.0)?;
            let extrusion = r.p3_or(210, Z)?;
            let has_attribs = r.bool_or(66, false)?;
            let block_header = r.opt_handle(c::BLOCK_HEADER)?;
            let attribs = get_handles_or(r)?;
            let first_attrib = r.opt_handle(c::FIRST)?;
            let last_attrib = r.opt_handle(c::LAST)?;
            let seqend = r.opt_handle(c::SEQEND)?;
            let array = match r.opt_i16(70)? {
                Some(cols) => Some((cols as u16, r.u16(71)?, r.f64(44)?, r.f64(45)?)),
                None => None,
            };
            K::Insert(Insert {
                insertion,
                scale,
                rotation,
                extrusion,
                has_attribs,
                block_header,
                attribs,
                first_attrib,
                last_attrib,
                seqend,
                array,
            })
        }
        kind::SOLID => K::Solid(Solid {
            thickness: r.f64_or(39, 0.0)?,
            elevation: r.f64_or(38, 0.0)?,
            corners: [r.p2(10)?, r.p2(11)?, r.p2(12)?, r.p2(13)?],
            extrusion: r.p3_or(210, Z)?,
            is_trace: r.bool_or(c::IS_TRACE, false)?,
        }),
        kind::FACE3D => K::Face3d(Face3d {
            corners: [r.p3(10)?, r.p3(11)?, r.p3(12)?, r.p3(13)?],
            invisible_edges: r.u16_or(70, 0)?,
        }),
        kind::SPLINE => {
            let flags = r.i16_or(70, 0)?;
            K::Spline(Spline {
                closed: bit(flags, 0),
                periodic: bit(flags, 1),
                rational: bit(flags, 2),
                degree: r.u32(71)?,
                knots: get_f64s_or(r, c::KNOT_COUNT, 40)?,
                control_points: get_p3s_or(r, c::CTRL_COUNT, 10)?,
                weights: get_f64s_or(r, c::WEIGHT_COUNT, 41)?,
                fit_points: get_p3s_or(r, c::FIT_COUNT, 11)?,
                fit_tolerance: r.f64_or(44, 0.0)?,
                start_tangent: r.p3_or(12, ORIGIN3)?,
                end_tangent: r.p3_or(13, ORIGIN3)?,
            })
        }
        kind::DIMENSION => K::Dimension(Dimension {
            kind: match r.i16(c::SUBKIND)? {
                0 => DimensionKind::Linear,
                1 => DimensionKind::Aligned,
                2 => DimensionKind::Ordinate,
                3 => DimensionKind::Angular3Pt,
                4 => DimensionKind::Angular2Line,
                5 => DimensionKind::Radius,
                6 => DimensionKind::Diameter,
                v => return Err(Error::format(format!("dimension kind {v}"))),
            },
            extrusion: r.p3_or(210, Z)?,
            text_midpoint: r.p2(11)?,
            elevation: r.f64_or(38, 0.0)?,
            flags: r.u8_or(c::DWG_FLAGS, 0)?,
            user_text: r.str_or(1, "")?,
            text_rotation: r.f64_or(53, 0.0)?,
            horizontal_direction: r.f64_or(51, 0.0)?,
            insert_scale: r.p3_or(c::INSERT_SCALE, ONES)?,
            insert_rotation: r.f64_or(c::INSERT_ROTATION, 0.0)?,
            attachment: r.u16_or(71, 0)?,
            measurement: r.f64(42)?,
            clone_insertion: r.p2_or(c::CLONE_INSERTION, ORIGIN2)?,
            pt10: r.p3(10)?,
            pt13: r.p3_or(13, ORIGIN3)?,
            pt14: r.p3_or(14, ORIGIN3)?,
            pt15: r.p3_or(15, ORIGIN3)?,
            pt16: r.p2_or(16, ORIGIN2)?,
            ext_line_rotation: r.f64_or(52, 0.0)?,
            dim_rotation: r.f64_or(50, 0.0)?,
            leader_length: r.f64_or(40, 0.0)?,
            dimstyle: r.opt_handle(c::DIMSTYLE)?,
            block: r.opt_handle(c::BLOCK)?,
        }),
        kind::HATCH => {
            let mut h = Hatch {
                elevation: r.f64_or(38, 0.0)?,
                extrusion: r.p3_or(210, Z)?,
                pattern_name: r.str(2)?,
                solid_fill: r.bool_or(70, false)?,
                associative: r.bool_or(71, false)?,
                is_gradient: r.bool_or(450, false)?,
                style: r.u16_or(75, 0)?,
                pattern_type: r.u16_or(76, 1)?,
                pattern_angle: r.f64_or(52, 0.0)?,
                pattern_scale: r.f64_or(41, 1.0)?,
                ..Default::default()
            };
            let n = r.count(c::PATH_COUNT, 10)?;
            for _ in 0..n {
                h.paths.push(read_hatch_path(r)?);
            }
            // Format 2.4.
            h.pattern_double = r.bool_or(77, false)?;
            let n = r.count_or(78, F64_BYTES + 2 * P2_BYTES + 7)?;
            for _ in 0..n {
                let angle = r.f64(53)?;
                let base = r.p2(43)?;
                let offset = r.p2(45)?;
                let k = r.count(79, F64_BYTES)?;
                let mut dashes = Vec::with_capacity(k);
                for _ in 0..k {
                    dashes.push(r.f64(49)?);
                }
                h.pattern_lines.push(HatchPatternLine { angle, base, offset, dashes });
            }
            let n = r.count_or(98, P2_BYTES)?;
            for _ in 0..n {
                h.seed_points.push(r.p2(super::objects::code::HATCH_SEED)?);
            }
            K::Hatch(h)
        }
        kind::XLINE => {
            let x = ConstructionLine {
                base: r.p3(10)?,
                direction: r.p3(11)?,
            };
            if r.bool_or(super::objects::code::IS_RAY, false)? {
                K::Ray(x)
            } else {
                K::Xline(x)
            }
        }
        kind::LEADER => K::Leader(super::objects::read_leader(r)?),
        kind::VIEWPORT => K::PaperViewport(Box::new(super::objects::read_paper_viewport(r)?)),
        kind::BLOCK_BEGIN => K::BlockBegin { name: r.str(2)? },
        kind::BLOCK_END => K::BlockEnd,
        kind::BLOCK_HEADER => {
            let name = r.str(2)?;
            let flags = r.i16_or(70, 0)?;
            K::BlockHeader(BlockHeader {
                name,
                anonymous: bit(flags, 0),
                has_attribs: bit(flags, 1),
                is_xref: bit(flags, 2),
                is_overlaid: bit(flags, 3),
                base_point: r.p3_or(10, ORIGIN3)?,
                xref_path: r.str_or(1, "")?,
                description: r.str_or(4, "")?,
                insert_units: r.u16_or(c::INSERT_UNITS, 0)?,
                block_entity: r.opt_handle(c::BLOCK_ENTITY)?,
                entities: get_handles_or(r)?,
                first_entity: r.opt_handle(c::FIRST)?,
                last_entity: r.opt_handle(c::LAST)?,
                endblk: r.opt_handle(c::ENDBLK)?,
                layout: r.opt_handle(340)?,
                part: Part {
                    material: r.str_or(c::PART_MATERIAL, "")?,
                    grade: r.str_or(c::PART_GRADE, "")?,
                    thickness: r.f64_or(c::PART_THICKNESS, 0.0)?,
                    quantity: r.u32_or(c::PART_QUANTITY, 0)?,
                },
            })
        }
        kind::LAYER => {
            let name = r.str(2)?;
            let flags = r.u16_or(c::DWG_FLAGS, 0)?;
            let state = r.i16(c::LAYER_STATE)?;
            K::Layer(Layer {
                name,
                flags,
                frozen: bit(state, 0),
                off: bit(state, 1),
                locked: bit(state, 2),
                plot: bit(state, 3),
                color: get_color(r)?,
                linetype: r.opt_handle(c::LAYER_LTYPE)?,
                lineweight: r.u8_or(370, LW_DEFAULT)?,
            })
        }
        kind::CONTROL => K::Control(Control {
            entries: get_handles_or(r)?,
            model_space: r.opt_handle(c::MODEL_SPACE)?,
            paper_space: r.opt_handle(c::PAPER_SPACE)?,
        }),
        kind::NAMED_RECORD => K::NamedRecord(NamedRecord { name: r.str(2)? }),
        kind::LINETYPE => {
            let name = r.str(2)?;
            let flags = r.u16_or(70, 0)?;
            let description = r.str_or(3, "")?;
            let pattern_length = r.f64_or(40, 0.0)?;
            // An element is at least its 49 length (the rest may be left
            // out at their defaults).
            let n = r.count_or(73, F64_BYTES)?;
            let mut elements = Vec::with_capacity(n);
            for _ in 0..n {
                elements.push(LinetypeElement {
                    length: r.f64(49)?,
                    complex: r.u16_or(74, 0)?,
                    shape: r.u16_or(75, 0)?,
                    style: r.opt_handle(340)?,
                    scale: r.f64_or(46, 1.0)?,
                    rotation: r.f64_or(50, 0.0)?,
                    offset: r.p2_or(44, ORIGIN2)?,
                    text: r.str_or(9, "")?,
                });
            }
            K::Linetype(Linetype {
                name,
                description,
                flags,
                pattern_length,
                elements,
            })
        }
        kind::TEXT_STYLE => K::TextStyle(TextStyle {
            name: r.str(2)?,
            flags: r.u16_or(70, 0)?,
            height: r.f64_or(40, 0.0)?,
            width_factor: r.f64_or(41, 1.0)?,
            oblique: r.f64_or(50, 0.0)?,
            generation: r.u16_or(71, 0)?,
            last_height: r.f64_or(42, 0.0)?,
            font_file: r.str_or(3, "")?,
            bigfont_file: r.str_or(4, "")?,
        }),
        kind::DIM_STYLE => K::DimStyle(DimStyle {
            name: r.str(2)?,
            flags: r.u16_or(70, 0)?,
            post_text: r.str_or(3, "")?,
            scale: r.f64_or(40, 1.0)?,
            arrow_size: r.f64_or(41, 0.0)?,
            ext_offset: r.f64_or(42, 0.0)?,
            baseline_spacing: r.f64_or(43, 0.0)?,
            ext_extension: r.f64_or(44, 0.0)?,
            rounding: r.f64_or(45, 0.0)?,
            dim_line_extension: r.f64_or(46, 0.0)?,
            text_height: r.f64_or(140, 0.0)?,
            center_mark: r.f64_or(141, 0.0)?,
            tick_size: r.f64_or(142, 0.0)?,
            alt_factor: r.f64_or(143, 25.4)?,
            linear_factor: r.f64_or(144, 1.0)?,
            text_gap: r.f64_or(147, 0.0)?,
            text_inside_horizontal: r.bool_or(73, true)?,
            text_outside_horizontal: r.bool_or(74, true)?,
            text_above: r.u16_or(77, 0)?,
            zero_suppression: r.u16_or(78, 0)?,
            alt_units: r.bool_or(170, false)?,
            alt_decimals: r.u16_or(171, 2)?,
            line_color: get_method_color(r, c::DIM_LINE_COLOR)?,
            ext_color: get_method_color(r, c::DIM_EXT_COLOR)?,
            text_color: get_method_color(r, c::DIM_TEXT_COLOR)?,
            decimals: r.u16_or(271, 2)?,
            fraction_format: r.u16_or(276, 0)?,
            unit_format: r.u16_or(277, 2)?,
            decimal_separator: {
                let v = r.u32_or(278, '.' as i32)?;
                char::from_u32(v)
                    .ok_or_else(|| Error::format(format!("group 278: {v:#X} is not a character")))?
            },
            text_justify: r.u16_or(280, 0)?,
            text_style: r.opt_handle(340)?,
            line_weight: r.u8_or(371, LW_BY_BLOCK)?,
            ext_weight: r.u8_or(372, LW_BY_BLOCK)?,
        }),
        kind::VPORT => K::Viewport(Box::new(super::objects::read_vport(r)?)),
        kind::DIMSTYLE_RECORD => K::DimStyleRecord(Box::new(super::objects::read_dimstyle(r)?)),
        kind::DICTIONARY => K::NonGraphic(NonGraphic::Dictionary(super::objects::read_dictionary(r)?)),
        kind::XRECORD => K::NonGraphic(NonGraphic::XRecord(super::objects::read_xrecord(r)?)),
        kind::PLACEHOLDER => K::NonGraphic(NonGraphic::Placeholder),
        kind::DICTIONARY_VAR => K::NonGraphic(super::objects::read_dictionary_var(r)?),
        kind::SCALE => K::NonGraphic(NonGraphic::Scale(super::objects::read_scale(r)?)),
        kind::LAYOUT => K::NonGraphic(NonGraphic::Layout(Box::new(super::objects::read_layout(r)?))),
        kind::MLINESTYLE => K::NonGraphic(NonGraphic::MLineStyle(super::objects::read_mlinestyle(r)?)),
        kind::GROUPS => K::NonGraphic(NonGraphic::Groups(super::objects::read_groups(r)?)),
        // A kind from a newer revision: its fields stay undecoded.
        k => K::NewerKind(k),
    })
}

// ---------------------------------------------------------------------------
// Header variables: DXF-style "9 $NAME" followed by the value tag
// ---------------------------------------------------------------------------

pub fn write_header(w: &mut TagWriter, h: &HeaderVars) {
    let var = |w: &mut TagWriter, name: &str| w.str(9, name);
    var(w, "$CAD_VERIFIED");
    w.bool(290, h.verified);
    if !h.note.is_empty() {
        var(w, "$CAD_NOTE");
        w.str(1, &h.note);
    }
    if let Some(v) = h.insunits {
        var(w, "$INSUNITS");
        w.u16(70, v);
    }
    if let Some(v) = h.measurement {
        var(w, "$MEASUREMENT");
        w.u16(70, v);
    }
    for (name, v) in [
        ("$EXTMIN", h.extmin),
        ("$EXTMAX", h.extmax),
        ("$INSBASE", h.insbase),
    ] {
        if let Some(p) = v {
            var(w, name);
            w.p3(10, p);
        }
    }
    for (name, v) in [("$LIMMIN", h.limmin), ("$LIMMAX", h.limmax)] {
        if let Some(p) = v {
            var(w, name);
            w.p2(10, p);
        }
    }
    for (name, v) in [
        ("$LTSCALE", h.ltscale),
        ("$TEXTSIZE", h.textsize),
        ("$CELTSCALE", h.celtscale),
    ] {
        if let Some(x) = v {
            var(w, name);
            w.f64(40, x);
        }
    }
    for (name, v) in [
        ("$LUNITS", h.lunits),
        ("$LUPREC", h.luprec),
        ("$AUNITS", h.aunits),
        ("$AUPREC", h.auprec),
        ("$ANGDIR", h.angdir),
    ] {
        if let Some(x) = v {
            var(w, name);
            w.u16(70, x);
        }
    }
    if let Some(a) = h.angbase {
        var(w, "$ANGBASE");
        w.f64(50, a);
    }
    if let Some(v) = h.handseed {
        var(w, "$HANDSEED");
        w.handle(5, Handle(v));
    }
    for (name, v) in [
        ("$CLAYER", h.clayer),
        ("$TEXTSTYLE", h.textstyle),
        ("$CELTYPE", h.celtype),
        ("$DIMSTYLE", h.dimstyle),
        ("$CAD_BLOCK_CONTROL", h.block_control),
        ("$CAD_LAYER_CONTROL", h.layer_control),
        ("$CAD_STYLE_CONTROL", h.style_control),
        ("$CAD_LTYPE_CONTROL", h.linetype_control),
        ("$CAD_DIMSTYLE_CONTROL", h.dimstyle_control),
        ("$CAD_MODEL_SPACE", h.model_space),
        ("$CAD_PAPER_SPACE", h.paper_space),
        ("$CAD_NOD", h.named_objects_dict),
    ] {
        if !v.is_null() {
            var(w, name);
            w.handle(330, v);
        }
    }
    let pr = &h.properties;
    for (name, v) in [
        ("$CAD_TITLE", &pr.title),
        ("$CAD_SUBJECT", &pr.subject),
        ("$CAD_AUTHOR", &pr.author),
        ("$CAD_KEYWORDS", &pr.keywords),
        ("$CAD_COMMENTS", &pr.comments),
        ("$CAD_LAST_SAVED_BY", &pr.last_saved_by),
        ("$CAD_REVISION", &pr.revision),
        ("$CAD_HYPERLINK_BASE", &pr.hyperlink_base),
    ] {
        if !v.is_empty() {
            var(w, name);
            w.str(1, v);
        }
    }
    for (name, t) in [("$CAD_CREATED", pr.created), ("$CAD_MODIFIED", pr.modified)] {
        if t != 0 {
            var(w, name);
            w.i64(160, t);
        }
    }
    for (k, v) in &pr.custom {
        var(w, "$CAD_PROPERTY");
        w.str(1, k);
        w.str(2, v);
    }
    for (k, v) in &h.raw {
        var(w, "$CAD_SOURCE_VAR");
        w.str(c::HDR_RAW_KEY, k);
        w.str(c::HDR_RAW_VALUE, v);
    }
    w.encoded(&h.unknown_tags);
}

/// Header variables. Variables this version does not know (a newer
/// writer's) are kept as encoded tags, each from its `9` tag to the next
/// one, and written back at the end.
pub fn read_header(data: &[u8]) -> Result<HeaderVars> {
    read_header_checked(data).map(|(h, _)| h)
}

/// [`read_header`], also saying whether part of the stream could not be
/// parsed (damage). That part is kept as it is, like an unknown variable.
pub fn read_header_checked(data: &[u8]) -> Result<(HeaderVars, Option<String>)> {
    let mut r = TagReader::new(data);
    let mut h = HeaderVars::default();
    let mut unknown_from: Option<usize> = None;
    let mut problem: Option<String> = None;
    loop {
        let at = r.position();
        let t = match r.next() {
            Ok(Some(t)) => t,
            Ok(None) => break,
            Err(e) => {
                let from = unknown_from.take().unwrap_or(at);
                h.unknown_tags.extend_from_slice(&data[from..]);
                let m = format!("tags from byte {at} kept unread: {e}");
                return Ok((h, Some(problem.unwrap_or(m))));
            }
        };
        if t.code != 9 {
            continue; // value of a variable this reader does not know
        }
        if let Some(from) = unknown_from.take() {
            h.unknown_tags.extend_from_slice(&data[from..at]);
        }
        let Value::Str(name) = t.value else {
            // Not a variable name: keep it like an unknown variable.
            problem
                .get_or_insert_with(|| format!("group 9 at byte {at} is not a name; kept as is"));
            unknown_from = Some(at);
            continue;
        };
        match read_known_var(&name, &mut r, &mut h) {
            Ok(true) => {}
            Ok(false) => unknown_from = Some(at),
            // A known variable whose value does not decode is kept as it
            // is, like an unknown one; the field stays unset.
            Err(e) => {
                problem.get_or_insert_with(|| format!("{name}: {e}; kept as is"));
                unknown_from = Some(at);
            }
        }
    }
    if let Some(from) = unknown_from {
        h.unknown_tags.extend_from_slice(&data[from..]);
    }
    Ok((h, problem))
}

/// Read the value of a variable this version knows; `false` for others.
fn read_known_var(name: &str, r: &mut TagReader, h: &mut HeaderVars) -> Result<bool> {
    match name {
        "$CAD_VERIFIED" => h.verified = r.bool(290)?,
        "$CAD_NOTE" => h.note = r.str(1)?,
        "$INSUNITS" => h.insunits = Some(r.u16(70)?),
        "$MEASUREMENT" => h.measurement = Some(r.u16(70)?),
        "$EXTMIN" => h.extmin = Some(r.p3(10)?),
        "$EXTMAX" => h.extmax = Some(r.p3(10)?),
        "$INSBASE" => h.insbase = Some(r.p3(10)?),
        "$LIMMIN" => h.limmin = Some(r.p2(10)?),
        "$LIMMAX" => h.limmax = Some(r.p2(10)?),
        "$LTSCALE" => h.ltscale = Some(r.f64(40)?),
        "$TEXTSIZE" => h.textsize = Some(r.f64(40)?),
        "$CELTSCALE" => h.celtscale = Some(r.f64(40)?),
        "$LUNITS" => h.lunits = Some(r.u16(70)?),
        "$LUPREC" => h.luprec = Some(r.u16(70)?),
        "$AUNITS" => h.aunits = Some(r.u16(70)?),
        "$AUPREC" => h.auprec = Some(r.u16(70)?),
        "$ANGDIR" => h.angdir = Some(r.u16(70)?),
        "$ANGBASE" => h.angbase = Some(r.f64(50)?),
        "$TEXTSTYLE" => h.textstyle = r.handle(330)?,
        "$CELTYPE" => h.celtype = r.handle(330)?,
        "$DIMSTYLE" => h.dimstyle = r.handle(330)?,
        "$CAD_DIMSTYLE_CONTROL" => h.dimstyle_control = r.handle(330)?,
        "$HANDSEED" => h.handseed = Some(r.handle(5)?.0),
        "$CLAYER" => h.clayer = r.handle(330)?,
        "$CAD_BLOCK_CONTROL" => h.block_control = r.handle(330)?,
        "$CAD_LAYER_CONTROL" => h.layer_control = r.handle(330)?,
        "$CAD_STYLE_CONTROL" => h.style_control = r.handle(330)?,
        "$CAD_LTYPE_CONTROL" => h.linetype_control = r.handle(330)?,
        "$CAD_MODEL_SPACE" => h.model_space = r.handle(330)?,
        "$CAD_PAPER_SPACE" => h.paper_space = r.handle(330)?,
        "$CAD_NOD" => h.named_objects_dict = r.handle(330)?,
        "$CAD_TITLE" => h.properties.title = r.str(1)?,
        "$CAD_SUBJECT" => h.properties.subject = r.str(1)?,
        "$CAD_AUTHOR" => h.properties.author = r.str(1)?,
        "$CAD_KEYWORDS" => h.properties.keywords = r.str(1)?,
        "$CAD_COMMENTS" => h.properties.comments = r.str(1)?,
        "$CAD_LAST_SAVED_BY" => h.properties.last_saved_by = r.str(1)?,
        "$CAD_REVISION" => h.properties.revision = r.str(1)?,
        "$CAD_HYPERLINK_BASE" => h.properties.hyperlink_base = r.str(1)?,
        "$CAD_CREATED" => h.properties.created = r.i64(160)?,
        "$CAD_MODIFIED" => h.properties.modified = r.i64(160)?,
        "$CAD_PROPERTY" => {
            let k = r.str(1)?;
            let v = r.str(2)?;
            h.properties.custom.push((k, v));
        }
        "$CAD_SOURCE_VAR" => {
            let k = r.str(c::HDR_RAW_KEY)?;
            let v = r.str(c::HDR_RAW_VALUE)?;
            h.raw.push((k, v));
        }
        _ => return Ok(false),
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Classes: one record per class, DXF CLASS section codes
// ---------------------------------------------------------------------------

pub fn write_class(w: &mut TagWriter, k: &ClassDef) {
    w.str(0, "CLASS");
    w.str(1, &k.dxf_name);
    w.str(2, &k.cpp_name);
    w.str(3, &k.app_name);
    w.u32(90, k.proxy_flags as u32);
    w.bool(280, k.was_zombie);
    w.bool(281, k.is_entity());
    w.u16(c::CLASS_NUMBER, k.number);
    w.u16(c::CLASS_ITEM_ID, k.item_class_id);
    // 2.1: what DWG class records add.
    w.u32(91, k.instance_count);
    w.u32(c::CLASS_DWG_VERSION, k.dwg_version);
    w.u32(c::CLASS_MAINT_VERSION, k.maintenance_version);
}

pub fn read_class(data: &[u8]) -> Result<ClassDef> {
    let mut r = TagReader::new(data);
    let marker = r.str(0)?;
    if marker != "CLASS" {
        return Err(Error::format(format!(
            "class record starts with {marker:?}"
        )));
    }
    let dxf_name = r.str(1)?;
    let cpp_name = r.str(2)?;
    let app_name = r.str(3)?;
    let proxy_flags = r.u32(90)? as u16;
    let was_zombie = r.bool(280)?;
    let _is_entity = r.bool(281)?;
    Ok(ClassDef {
        number: r.u16(c::CLASS_NUMBER)?,
        item_class_id: r.u16(c::CLASS_ITEM_ID)?,
        instance_count: r.u32_or(91, 0)?,
        dwg_version: r.u32_or(c::CLASS_DWG_VERSION, 0)?,
        maintenance_version: r.u32_or(c::CLASS_MAINT_VERSION, 0)?,
        proxy_flags,
        app_name,
        cpp_name,
        dxf_name,
        was_zombie,
    })
}

// ---------------------------------------------------------------------------
// Preserved DWG bytes
// ---------------------------------------------------------------------------

pub fn write_raw(w: &mut TagWriter, h: Handle, raw: &RawDwgObject) {
    w.handle(5, h);
    w.str(1, raw.version.magic());
    w.u64(c::RAW_HANDLE_BITS, raw.handle_stream_bits);
    w.bytes(310, &raw.data);
}

pub fn read_raw(data: &[u8]) -> Result<(Handle, RawDwgObject)> {
    let mut r = TagReader::new(data);
    let h = r.handle(5)?;
    let tag = r.str(1)?;
    let version = DwgVersion::from_tag(&tag)
        .ok_or_else(|| Error::format(format!("unknown DWG version {tag:?}")))?;
    let handle_stream_bits = r.u64(c::RAW_HANDLE_BITS)?;
    let data = r.bytes(310)?;
    Ok((
        h,
        RawDwgObject {
            version,
            handle_stream_bits,
            data,
        },
    ))
}

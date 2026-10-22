//! Records of format 2.2: the VPORT record, the DIMSTYLE record with every
//! variable, and the non-graphical objects (dictionaries, extension
//! records, placeholders, dictionary variables, scales, layouts, multiline
//! styles). `docs/FORMAT.md` lists the layouts; `schema` describes them.
//!
//! Where a field has a DXF group code of its own, the record uses it;
//! points are single tags at the first DXF code, colours are method words
//! (`Color::to_method_word`), and what DXF does not express has a code from
//! 2144–2199.

use super::records::code as c;
use super::tags::{TagReader, TagWriter};
use crate::error::{Error, Result};
use crate::model::*;

pub(crate) mod code {
    /// Ambient colour of a VPORT; DIMTFILLCLR of a DIMSTYLE; the fill or
    /// element colour of a multiline style: a color method word.
    pub const COLOR_WORD: u16 = 2144;
    pub const VPORT_UNKNOWN_BIT: u16 = 2145;
    pub const DIMALTMZF: u16 = 2146;
    pub const DIMALTMZS: u16 = 2147;
    pub const DIMMZF: u16 = 2148;
    pub const DIMMZS: u16 = 2149;
    pub const DIM_FLAG_BIT: u16 = 2150;
    /// XRECORD item: its DXF group code, then the value under one of the
    /// codes below by type.
    pub const XR_CODE: u16 = 2151;
    pub const XR_STR: u16 = 2152;
    pub const XR_POINT: u16 = 2153;
    pub const XR_REAL: u16 = 2154;
    pub const XR_INT8: u16 = 2155;
    pub const XR_INT16: u16 = 2156;
    pub const XR_INT32: u16 = 2157;
    pub const XR_INT64: u16 = 2158;
    pub const XR_BOOL: u16 = 2159;
    pub const XR_BYTES: u16 = 2160;
    pub const XR_HANDLE: u16 = 2161;
    /// A dictionary with a default entry (ACDBDICTIONARYWDFLT).
    pub const DICT_HAS_DEFAULT: u16 = 2162;
    pub const PLOT_VIEW: u16 = 2163;
    pub const PLOT_VISUAL_STYLE: u16 = 2164;
    pub const LAYOUT_NAME: u16 = 2165;
    pub const LAYOUT_FLAGS: u16 = 2166;
    pub const LAYOUT_TAB_ORDER: u16 = 2167;
    pub const LAYOUT_LIMMIN: u16 = 2168;
    pub const LAYOUT_LIMMAX: u16 = 2169;
    pub const LAYOUT_INSBASE: u16 = 2170;
    pub const LAYOUT_EXTMIN: u16 = 2171;
    pub const LAYOUT_EXTMAX: u16 = 2172;
    pub const LAYOUT_ELEVATION: u16 = 2173;
    pub const LAYOUT_UCS_ORIGIN: u16 = 2174;
    pub const LAYOUT_UCS_X: u16 = 2175;
    pub const LAYOUT_UCS_Y: u16 = 2176;
    pub const LAYOUT_ORTHO_TYPE: u16 = 2177;
    pub const LAYOUT_BLOCK: u16 = 2178;
    pub const LAYOUT_LAST_VIEWPORT: u16 = 2179;
    pub const LAYOUT_BASE_UCS: u16 = 2180;
    pub const LAYOUT_NAMED_UCS: u16 = 2181;
    pub const MLINE_LINETYPE_INDEX: u16 = 2182;
    /// A kept object's DXF type (format 2.3).
    pub const GROUPS_KIND: u16 = 2183;
    /// Format 2.4: an XLINE record that is a RAY.
    pub const IS_RAY: u16 = 2184;
    /// Format 2.4: LEADER fields DXF does not carry.
    pub const LEADER_ORIGIN: u16 = 2185;
    pub const LEADER_UNKNOWN_BITS: u16 = 2186;
    pub const LEADER_UNKNOWN_SHORT: u16 = 2187;
    /// Format 2.4: a hatch's seed points.
    pub const HATCH_SEED: u16 = 2188;
    /// HATCH (2.6): the number of boundary object lists, one per path.
    pub const HATCH_SOURCES: u16 = 2189;
    /// HATCH (2.6): a boundary object (the record's 330 is its owner).
    pub const HATCH_SOURCE: u16 = 2190;
}

use code as o;

const H_BYTES: usize = 4;

/// The colour method word of index 250, VPORT's default ambient colour.
pub(crate) const COLOR_WORD_250: i32 = 0xC300_00FAu32 as i32;
const BY_BLOCK: i32 = super::schema::COLOR_WORD_BY_BLOCK;

/// A colour stored as a method word, `d` when left out.
fn color_word(r: &mut TagReader, code: u16, d: i32) -> Result<Color> {
    let word = r.u32_or(code, d)?;
    match word >> 24 {
        0xC0 | 0xC1 | 0xC2 | 0xC8 => Ok(Color::from_method_word(word, 7)),
        0xC3 if (1..=255).contains(&(word & 0xFF)) => Ok(Color::from_method_word(word, 7)),
        _ => Err(Error::format(format!("group {code}: color word {word:#010X}"))),
    }
}

// ----- VPORT ------------------------------------------------------------------

pub(super) fn write_vport(w: &mut TagWriter, v: &Vport) {
    w.str(2, &v.name);
    w.f64(40, v.view_height);
    w.f64(41, v.aspect);
    w.p2(12, v.center);
    w.p3(17, v.target);
    w.p3(16, v.direction);
    w.f64(51, v.twist);
    w.f64(42, v.lens);
    w.f64(43, v.front_clip);
    w.f64(44, v.back_clip);
    w.u16(71, v.view_mode);
    w.u8(281, v.render_mode);
    w.bool(292, v.default_lights);
    w.u8(282, v.lighting_type);
    w.f64(141, v.brightness);
    w.f64(142, v.contrast);
    w.u32_d(o::COLOR_WORD, v.ambient.to_method_word(), COLOR_WORD_250);
    w.p2(10, v.lower_left);
    w.p2(11, v.upper_right);
    w.u16(72, v.circle_zoom);
    w.bool(73, v.fast_zoom);
    w.u8(74, v.ucsicon);
    w.bool(76, v.grid_on);
    w.p2(15, v.grid_spacing);
    w.bool(75, v.snap_on);
    w.bool(77, v.snap_style);
    w.u16(78, v.snap_isopair);
    w.f64(50, v.snap_rotation);
    w.p2(13, v.snap_base);
    w.p2(14, v.snap_spacing);
    w.bool(o::VPORT_UNKNOWN_BIT, v.unknown_bit);
    w.bool(65, v.ucs_per_viewport);
    w.p3(110, v.ucs_origin);
    w.p3(111, v.ucs_x);
    w.p3(112, v.ucs_y);
    w.f64(146, v.ucs_elevation);
    w.u16(79, v.ucs_ortho_type);
    w.u16(60, v.grid_flags);
    w.u16(61, v.grid_major);
    w.opt_handle(332, v.background);
    w.opt_handle(348, v.visual_style);
    w.opt_handle(361, v.sun);
    w.opt_handle(345, v.named_ucs);
    w.opt_handle(346, v.base_ucs);
}

pub(super) fn read_vport(r: &mut TagReader) -> Result<Vport> {
    Ok(Vport {
        name: r.str(2)?,
        view_height: r.f64(40)?,
        aspect: r.f64(41)?,
        center: r.p2(12)?,
        target: r.p3(17)?,
        direction: r.p3(16)?,
        twist: r.f64(51)?,
        lens: r.f64(42)?,
        front_clip: r.f64(43)?,
        back_clip: r.f64(44)?,
        view_mode: r.u16(71)?,
        render_mode: r.u8(281)?,
        default_lights: r.bool(292)?,
        lighting_type: r.u8(282)?,
        brightness: r.f64(141)?,
        contrast: r.f64(142)?,
        ambient: color_word(r, o::COLOR_WORD, COLOR_WORD_250)?,
        lower_left: r.p2(10)?,
        upper_right: r.p2(11)?,
        circle_zoom: r.u16(72)?,
        fast_zoom: r.bool(73)?,
        ucsicon: r.u8(74)?,
        grid_on: r.bool(76)?,
        grid_spacing: r.p2(15)?,
        snap_on: r.bool(75)?,
        snap_style: r.bool(77)?,
        snap_isopair: r.u16(78)?,
        snap_rotation: r.f64(50)?,
        snap_base: r.p2(13)?,
        snap_spacing: r.p2(14)?,
        unknown_bit: r.bool(o::VPORT_UNKNOWN_BIT)?,
        ucs_per_viewport: r.bool(65)?,
        ucs_origin: r.p3(110)?,
        ucs_x: r.p3(111)?,
        ucs_y: r.p3(112)?,
        ucs_elevation: r.f64(146)?,
        ucs_ortho_type: r.u16(79)?,
        grid_flags: r.u16(60)?,
        grid_major: r.u16(61)?,
        background: r.opt_handle(332)?,
        visual_style: r.opt_handle(348)?,
        sun: r.opt_handle(361)?,
        named_ucs: r.opt_handle(345)?,
        base_ucs: r.opt_handle(346)?,
    })
}

// ----- DIMSTYLE -------------------------------------------------------------

pub(super) fn write_dimstyle(w: &mut TagWriter, d: &DimStyleRecord) {
    let v = &d.vars;
    w.str(2, &d.name);
    w.str(3, &v.post);
    w.str(4, &v.apost);
    for (code, x) in [
        (40, v.scale), (41, v.asz), (42, v.exo), (43, v.dli), (44, v.exe), (45, v.rnd),
        (46, v.dle), (47, v.tp), (48, v.tm), (49, v.fxl), (50, v.jogang),
    ] {
        w.f64(code, x);
    }
    w.u16(69, v.tfill);
    w.u32_d(o::COLOR_WORD, v.tfillclr.to_method_word(), BY_BLOCK);
    for (code, x) in [(71, v.tol), (72, v.lim), (73, v.tih), (74, v.toh), (75, v.se1), (76, v.se2)] {
        w.bool(code, x);
    }
    w.u16(77, v.tad);
    w.u16(78, v.zin);
    w.u16(79, v.azin);
    w.u16(90, v.arcsym);
    for (code, x) in [
        (140, v.txt), (141, v.cen), (142, v.tsz), (143, v.altf), (144, v.lfac), (145, v.tvp),
        (146, v.tfac), (147, v.gap), (148, v.altrnd),
    ] {
        w.f64(code, x);
    }
    w.bool(170, v.alt);
    w.u16(171, v.altd);
    for (code, x) in [(172, v.tofl), (173, v.sah), (174, v.tix), (175, v.soxd)] {
        w.bool(code, x);
    }
    w.u32_d(c::DIM_LINE_COLOR, v.clrd.to_method_word(), BY_BLOCK);
    w.u32_d(c::DIM_EXT_COLOR, v.clre.to_method_word(), BY_BLOCK);
    w.u32_d(c::DIM_TEXT_COLOR, v.clrt.to_method_word(), BY_BLOCK);
    for (code, x) in [
        (179, v.adec), (271, v.dec), (272, v.tdec), (273, v.altu), (274, v.alttd), (275, v.aunit),
        (276, v.frac), (277, v.lunit), (278, v.dsep), (279, v.tmove), (280, v.just),
    ] {
        w.u16(code, x);
    }
    w.bool(281, v.sd1);
    w.bool(282, v.sd2);
    for (code, x) in [(283, v.tolj), (284, v.tzin), (285, v.altz), (286, v.alttz)] {
        w.u16(code, x);
    }
    w.bool(288, v.upt);
    w.u16(289, v.fit);
    w.bool(290, v.fxlon);
    w.bool(294, v.txtdirection);
    w.f64(o::DIMALTMZF, v.altmzf);
    w.str(o::DIMALTMZS, &v.altmzs);
    w.f64(o::DIMMZF, v.mzf);
    w.str(o::DIMMZS, &v.mzs);
    w.i16(371, v.lwd);
    w.i16(372, v.lwe);
    w.bool(o::DIM_FLAG_BIT, v.flag_bit);
    for (code, h) in [
        (340, v.txsty), (341, v.ldrblk), (342, v.blk), (343, v.blk1), (344, v.blk2),
        (345, v.ltype), (346, v.ltex1), (347, v.ltex2),
    ] {
        w.opt_handle(code, h);
    }
}

pub(super) fn read_dimstyle(r: &mut TagReader) -> Result<DimStyleRecord> {
    let name = r.str(2)?;
    let post = r.str(3)?;
    let apost = r.str(4)?;
    let mut f = [0.0; 11];
    for (i, code) in [40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50].into_iter().enumerate() {
        f[i] = r.f64(code)?;
    }
    let [scale, asz, exo, dli, exe, rnd, dle, tp, tm, fxl, jogang] = f;
    let tfill = r.u16(69)?;
    let tfillclr = color_word(r, o::COLOR_WORD, BY_BLOCK)?;
    let mut b = [false; 6];
    for (i, code) in [71, 72, 73, 74, 75, 76].into_iter().enumerate() {
        b[i] = r.bool(code)?;
    }
    let [tol, lim, tih, toh, se1, se2] = b;
    let tad = r.u16(77)?;
    let zin = r.u16(78)?;
    let azin = r.u16(79)?;
    let arcsym = r.u16(90)?;
    let mut g = [0.0; 9];
    for (i, code) in [140, 141, 142, 143, 144, 145, 146, 147, 148].into_iter().enumerate() {
        g[i] = r.f64(code)?;
    }
    let [txt, cen, tsz, altf, lfac, tvp, tfac, gap, altrnd] = g;
    let alt = r.bool(170)?;
    let altd = r.u16(171)?;
    let mut b = [false; 4];
    for (i, code) in [172, 173, 174, 175].into_iter().enumerate() {
        b[i] = r.bool(code)?;
    }
    let [tofl, sah, tix, soxd] = b;
    let clrd = color_word(r, c::DIM_LINE_COLOR, BY_BLOCK)?;
    let clre = color_word(r, c::DIM_EXT_COLOR, BY_BLOCK)?;
    let clrt = color_word(r, c::DIM_TEXT_COLOR, BY_BLOCK)?;
    let mut n = [0u16; 11];
    for (i, code) in [179, 271, 272, 273, 274, 275, 276, 277, 278, 279, 280].into_iter().enumerate() {
        n[i] = r.u16(code)?;
    }
    let [adec, dec, tdec, altu, alttd, aunit, frac, lunit, dsep, tmove, just] = n;
    let sd1 = r.bool(281)?;
    let sd2 = r.bool(282)?;
    let mut m = [0u16; 4];
    for (i, code) in [283, 284, 285, 286].into_iter().enumerate() {
        m[i] = r.u16(code)?;
    }
    let [tolj, tzin, altz, alttz] = m;
    let upt = r.bool(288)?;
    let fit = r.u16(289)?;
    let fxlon = r.bool(290)?;
    let txtdirection = r.bool(294)?;
    let altmzf = r.f64(o::DIMALTMZF)?;
    let altmzs = r.str(o::DIMALTMZS)?;
    let mzf = r.f64(o::DIMMZF)?;
    let mzs = r.str(o::DIMMZS)?;
    let lwd = r.i16(371)?;
    let lwe = r.i16(372)?;
    let flag_bit = r.bool(o::DIM_FLAG_BIT)?;
    let mut hs = [Handle::NULL; 8];
    for (i, code) in [340, 341, 342, 343, 344, 345, 346, 347].into_iter().enumerate() {
        hs[i] = r.opt_handle(code)?;
    }
    let [txsty, ldrblk, blk, blk1, blk2, ltype, ltex1, ltex2] = hs;
    Ok(DimStyleRecord {
        name,
        vars: DimVars {
            post, apost, scale, asz, exo, dli, exe, rnd, dle, tp, tm, fxl, jogang, tfill,
            tfillclr, tol, lim, tih, toh, se1, se2, tad, zin, azin, arcsym, txt, cen, tsz,
            altf, lfac, tvp, tfac, gap, altrnd, alt, altd, tofl, sah, tix, soxd, clrd, clre,
            clrt, adec, dec, tdec, altu, alttd, aunit, frac, lunit, dsep, tmove, just, sd1,
            sd2, tolj, tzin, altz, alttz, upt, fit, fxlon, txtdirection, altmzf, altmzs, mzf,
            mzs, lwd, lwe, flag_bit, txsty, ldrblk, blk, blk1, blk2, ltype, ltex1, ltex2,
        },
    })
}

// ----- non-graphical objects ------------------------------------------------

pub(super) fn write_dictionary(w: &mut TagWriter, d: &Dictionary) {
    w.u16(281, d.cloning);
    w.u8_d(280, d.hard_owner, 0);
    w.count_d(c::LIST_COUNT, d.entries.len());
    for (k, h) in &d.entries {
        w.str(3, k);
        w.handle(350, *h);
    }
    w.bool_d(o::DICT_HAS_DEFAULT, d.default.is_some(), false);
    if let Some(h) = d.default {
        w.handle(340, h);
    }
}

pub(super) fn read_dictionary(r: &mut TagReader) -> Result<Dictionary> {
    let cloning = r.u16(281)?;
    let hard_owner = r.u8_or(280, 0)?;
    let n = r.count_or(c::LIST_COUNT, 3 + H_BYTES)?;
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        let k = r.str(3)?;
        entries.push((k, r.handle(350)?));
    }
    let default = if r.bool_or(o::DICT_HAS_DEFAULT, false)? {
        Some(r.handle(340)?)
    } else {
        None
    };
    Ok(Dictionary {
        cloning,
        hard_owner,
        entries,
        default,
    })
}

pub(super) fn write_xrecord(w: &mut TagWriter, x: &XRecord) {
    w.u16(280, x.cloning);
    write_items(w, &x.items);
}

pub(super) fn read_xrecord(r: &mut TagReader) -> Result<XRecord> {
    let cloning = r.u16(280)?;
    let items = read_items(r)?;
    Ok(XRecord { cloning, items })
}

fn write_items(w: &mut TagWriter, items: &[(u16, XrValue)]) {
    w.count_d(c::LIST_COUNT, items.len());
    for (code, v) in items {
        w.u16(o::XR_CODE, *code);
        match v {
            XrValue::Str(s) => w.str(o::XR_STR, s),
            XrValue::Point(p) => w.p3(o::XR_POINT, *p),
            XrValue::Real(f) => w.f64(o::XR_REAL, *f),
            XrValue::Int8(i) => w.i16(o::XR_INT8, *i as i16),
            XrValue::Int16(i) => w.i16(o::XR_INT16, *i),
            XrValue::Int32(i) => w.i32(o::XR_INT32, *i),
            XrValue::Int64(i) => w.i64(o::XR_INT64, *i),
            XrValue::Bool(b) => w.bool(o::XR_BOOL, *b),
            XrValue::Bytes(b) => w.bytes(o::XR_BYTES, b),
            XrValue::Handle(h) => w.handle(o::XR_HANDLE, *h),
        }
    }
}

fn read_items(r: &mut TagReader) -> Result<Vec<(u16, XrValue)>> {
    let n = r.count_or(c::LIST_COUNT, 6)?;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        let code = r.u16(o::XR_CODE)?;
        let v = match r.peek_code()? {
            Some(o::XR_STR) => XrValue::Str(r.str(o::XR_STR)?),
            Some(o::XR_POINT) => XrValue::Point(r.p3(o::XR_POINT)?),
            Some(o::XR_REAL) => XrValue::Real(r.f64(o::XR_REAL)?),
            Some(o::XR_INT8) => XrValue::Int8(r.i16(o::XR_INT8)? as i8),
            Some(o::XR_INT16) => XrValue::Int16(r.i16(o::XR_INT16)?),
            Some(o::XR_INT32) => XrValue::Int32(r.i32(o::XR_INT32)?),
            Some(o::XR_INT64) => XrValue::Int64(r.i64(o::XR_INT64)?),
            Some(o::XR_BOOL) => XrValue::Bool(r.bool(o::XR_BOOL)?),
            Some(o::XR_BYTES) => XrValue::Bytes(r.bytes(o::XR_BYTES)?),
            Some(o::XR_HANDLE) => XrValue::Handle(r.handle(o::XR_HANDLE)?),
            other => {
                return Err(Error::format(format!(
                    "item {code}: no value (group {other:?})"
                )))
            }
        };
        items.push((code, v));
    }
    Ok(items)
}

pub(super) fn write_groups(w: &mut TagWriter, g: &Groups) {
    w.str(o::GROUPS_KIND, &g.kind);
    write_items(w, &g.items);
}

pub(super) fn read_groups(r: &mut TagReader) -> Result<Groups> {
    let kind = r.str(o::GROUPS_KIND)?;
    let items = read_items(r)?;
    Ok(Groups { kind, items })
}

pub(super) fn write_dictionary_var(w: &mut TagWriter, schema: u8, value: &str) {
    w.u8_d(280, schema, 0);
    w.str(1, value);
}

pub(super) fn read_dictionary_var(r: &mut TagReader) -> Result<NonGraphic> {
    Ok(NonGraphic::DictionaryVar {
        schema: r.u8_or(280, 0)?,
        value: r.str(1)?,
    })
}

pub(super) fn write_scale(w: &mut TagWriter, s: &Scale) {
    w.u16_d(70, s.flag, 0);
    w.str(300, &s.name);
    w.f64(140, s.paper_units);
    w.f64(141, s.drawing_units);
    w.bool_d(290, s.unit_scale, false);
}

pub(super) fn read_scale(r: &mut TagReader) -> Result<Scale> {
    Ok(Scale {
        flag: r.u16_or(70, 0)?,
        name: r.str(300)?,
        paper_units: r.f64(140)?,
        drawing_units: r.f64(141)?,
        unit_scale: r.bool_or(290, false)?,
    })
}

pub(super) fn write_layout(w: &mut TagWriter, l: &Layout) {
    let p = &l.plot;
    w.str(1, &p.page_setup_name);
    w.str(2, &p.printer);
    w.str(4, &p.paper_size);
    w.u16(70, p.flags);
    for (code, v) in [(40, p.margins[0]), (41, p.margins[1]), (42, p.margins[2]), (43, p.margins[3])] {
        w.f64(code, v);
    }
    w.f64(44, p.paper_width);
    w.f64(45, p.paper_height);
    w.p2(46, p.origin);
    w.u16(72, p.paper_units);
    w.u16(73, p.rotation);
    w.u16(74, p.plot_type);
    w.p2(48, p.window_min);
    w.p2(140, p.window_max);
    w.f64(142, p.real_world_units);
    w.f64(143, p.drawing_units);
    w.str(7, &p.style_sheet);
    w.u16(75, p.scale_type);
    w.f64(147, p.scale_factor);
    w.p2(148, p.image_origin);
    w.u16(76, p.shade_plot_mode);
    w.u16(77, p.shade_plot_res_level);
    w.u16(78, p.shade_plot_custom_dpi);
    w.opt_handle(o::PLOT_VIEW, p.plot_view);
    w.opt_handle(o::PLOT_VISUAL_STYLE, p.visual_style);
    w.str(o::LAYOUT_NAME, &l.name);
    w.u16(o::LAYOUT_FLAGS, l.flags);
    w.u32(o::LAYOUT_TAB_ORDER, l.tab_order);
    w.p2(o::LAYOUT_LIMMIN, l.limmin);
    w.p2(o::LAYOUT_LIMMAX, l.limmax);
    w.p3(o::LAYOUT_INSBASE, l.insbase);
    w.p3(o::LAYOUT_EXTMIN, l.extmin);
    w.p3(o::LAYOUT_EXTMAX, l.extmax);
    w.f64(o::LAYOUT_ELEVATION, l.elevation);
    w.p3(o::LAYOUT_UCS_ORIGIN, l.ucs_origin);
    w.p3(o::LAYOUT_UCS_X, l.ucs_x);
    w.p3(o::LAYOUT_UCS_Y, l.ucs_y);
    w.u16(o::LAYOUT_ORTHO_TYPE, l.ortho_type);
    w.opt_handle(o::LAYOUT_BLOCK, l.block);
    w.opt_handle(o::LAYOUT_LAST_VIEWPORT, l.last_viewport);
    w.opt_handle(o::LAYOUT_BASE_UCS, l.base_ucs);
    w.opt_handle(o::LAYOUT_NAMED_UCS, l.named_ucs);
    w.count_d(c::LIST_COUNT, l.viewports.len());
    for h in &l.viewports {
        w.handle(c::LIST_ITEM, *h);
    }
}

pub(super) fn read_layout(r: &mut TagReader) -> Result<Layout> {
    let page_setup_name = r.str(1)?;
    let printer = r.str(2)?;
    let paper_size = r.str(4)?;
    let flags = r.u16(70)?;
    let margins = [r.f64(40)?, r.f64(41)?, r.f64(42)?, r.f64(43)?];
    let paper_width = r.f64(44)?;
    let paper_height = r.f64(45)?;
    let origin = r.p2(46)?;
    let paper_units = r.u16(72)?;
    let rotation = r.u16(73)?;
    let plot_type = r.u16(74)?;
    let window_min = r.p2(48)?;
    let window_max = r.p2(140)?;
    let real_world_units = r.f64(142)?;
    let drawing_units = r.f64(143)?;
    let style_sheet = r.str(7)?;
    let scale_type = r.u16(75)?;
    let scale_factor = r.f64(147)?;
    let image_origin = r.p2(148)?;
    let shade_plot_mode = r.u16(76)?;
    let shade_plot_res_level = r.u16(77)?;
    let shade_plot_custom_dpi = r.u16(78)?;
    let plot_view = r.opt_handle(o::PLOT_VIEW)?;
    let visual_style = r.opt_handle(o::PLOT_VISUAL_STYLE)?;
    let plot = PlotSettings {
        page_setup_name,
        printer,
        flags,
        margins,
        paper_width,
        paper_height,
        paper_size,
        origin,
        paper_units,
        rotation,
        plot_type,
        window_min,
        window_max,
        real_world_units,
        drawing_units,
        style_sheet,
        scale_type,
        scale_factor,
        image_origin,
        shade_plot_mode,
        shade_plot_res_level,
        shade_plot_custom_dpi,
        plot_view,
        visual_style,
    };
    let name = r.str(o::LAYOUT_NAME)?;
    let flags = r.u16(o::LAYOUT_FLAGS)?;
    let tab_order = r.u32(o::LAYOUT_TAB_ORDER)?;
    let limmin = r.p2(o::LAYOUT_LIMMIN)?;
    let limmax = r.p2(o::LAYOUT_LIMMAX)?;
    let insbase = r.p3(o::LAYOUT_INSBASE)?;
    let extmin = r.p3(o::LAYOUT_EXTMIN)?;
    let extmax = r.p3(o::LAYOUT_EXTMAX)?;
    let elevation = r.f64(o::LAYOUT_ELEVATION)?;
    let ucs_origin = r.p3(o::LAYOUT_UCS_ORIGIN)?;
    let ucs_x = r.p3(o::LAYOUT_UCS_X)?;
    let ucs_y = r.p3(o::LAYOUT_UCS_Y)?;
    let ortho_type = r.u16(o::LAYOUT_ORTHO_TYPE)?;
    let block = r.opt_handle(o::LAYOUT_BLOCK)?;
    let last_viewport = r.opt_handle(o::LAYOUT_LAST_VIEWPORT)?;
    let base_ucs = r.opt_handle(o::LAYOUT_BASE_UCS)?;
    let named_ucs = r.opt_handle(o::LAYOUT_NAMED_UCS)?;
    let n = r.count_or(c::LIST_COUNT, H_BYTES)?;
    let viewports = (0..n).map(|_| r.handle(c::LIST_ITEM)).collect::<Result<_>>()?;
    Ok(Layout {
        plot,
        name,
        tab_order,
        flags,
        ucs_origin,
        limmin,
        limmax,
        insbase,
        ucs_x,
        ucs_y,
        elevation,
        ortho_type,
        extmin,
        extmax,
        block,
        last_viewport,
        base_ucs,
        named_ucs,
        viewports,
    })
}

pub(super) fn write_mlinestyle(w: &mut TagWriter, m: &MLineStyle) {
    w.str(2, &m.name);
    w.str(3, &m.description);
    w.u16(70, m.flags);
    w.u32_d(o::COLOR_WORD, m.fill_color.to_method_word(), BY_BLOCK);
    w.f64(51, m.start_angle);
    w.f64(52, m.end_angle);
    w.count_d(c::LIST_COUNT, m.elements.len());
    for e in &m.elements {
        w.f64(49, e.offset);
        w.u32(o::COLOR_WORD, e.color.to_method_word());
        w.handle(c::LIST_ITEM, e.linetype);
        w.i16(o::MLINE_LINETYPE_INDEX, e.linetype_index);
    }
}

pub(super) fn read_mlinestyle(r: &mut TagReader) -> Result<MLineStyle> {
    let name = r.str(2)?;
    let description = r.str(3)?;
    let flags = r.u16(70)?;
    let fill_color = color_word(r, o::COLOR_WORD, BY_BLOCK)?;
    let start_angle = r.f64(51)?;
    let end_angle = r.f64(52)?;
    let n = r.count_or(c::LIST_COUNT, 24)?;
    let mut elements = Vec::with_capacity(n);
    for _ in 0..n {
        elements.push(MLineElement {
            offset: r.f64(49)?,
            color: color_word(r, o::COLOR_WORD, BY_BLOCK)?,
            linetype: r.handle(c::LIST_ITEM)?,
            linetype_index: r.i16(o::MLINE_LINETYPE_INDEX)?,
        });
    }
    Ok(MLineStyle {
        name,
        description,
        flags,
        fill_color,
        start_angle,
        end_angle,
        elements,
    })
}

// ----- Format 2.4: LEADER and VIEWPORT ------------------------------------------

const P3_BYTES: usize = 3 + 24;
const ORIGIN3: [f64; 3] = [0.0, 0.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];
const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];

pub(super) fn write_leader(w: &mut TagWriter, l: &Leader) {
    w.bool_d(71, l.arrowhead, true);
    w.bool_d(72, l.spline, false);
    w.u16_d(73, l.annotation_type, 3);
    w.bool_d(74, l.hookline_on_x, true);
    w.bool_d(75, l.hookline, false);
    w.f64_d(40, l.text_height, 0.0);
    w.f64_d(41, l.text_width, 0.0);
    w.count_d(76, l.vertices.len());
    for v in &l.vertices {
        w.p3(10, *v);
    }
    w.p3_d(o::LEADER_ORIGIN, l.origin, ORIGIN3);
    w.p3_d(210, l.extrusion, Z);
    w.p3_d(211, l.x_direction, X);
    w.p3_d(212, l.block_offset, ORIGIN3);
    w.p3_d(213, l.annotation_offset, ORIGIN3);
    let (a, _, b, c) = l.unknown;
    w.i16_d(o::LEADER_UNKNOWN_BITS, (a as i16) | ((b as i16) << 1) | ((c as i16) << 2), 0);
    w.u16_d(o::LEADER_UNKNOWN_SHORT, l.unknown.1, 0);
    w.opt_handle(340, l.annotation);
    w.opt_handle(c::DIMSTYLE, l.dimstyle);
}

pub(super) fn read_leader(r: &mut TagReader) -> Result<Leader> {
    let arrowhead = r.bool_or(71, true)?;
    let spline = r.bool_or(72, false)?;
    let annotation_type = r.u16_or(73, 3)?;
    let hookline_on_x = r.bool_or(74, true)?;
    let hookline = r.bool_or(75, false)?;
    let text_height = r.f64_or(40, 0.0)?;
    let text_width = r.f64_or(41, 0.0)?;
    let n = r.count_or(76, P3_BYTES)?;
    let mut vertices = Vec::with_capacity(n);
    for _ in 0..n {
        vertices.push(r.p3(10)?);
    }
    let origin = r.p3_or(o::LEADER_ORIGIN, ORIGIN3)?;
    let extrusion = r.p3_or(210, Z)?;
    let x_direction = r.p3_or(211, X)?;
    let block_offset = r.p3_or(212, ORIGIN3)?;
    let annotation_offset = r.p3_or(213, ORIGIN3)?;
    let bits = r.i16_or(o::LEADER_UNKNOWN_BITS, 0)?;
    let short = r.u16_or(o::LEADER_UNKNOWN_SHORT, 0)?;
    Ok(Leader {
        vertices,
        arrowhead,
        spline,
        annotation_type,
        hookline_on_x,
        hookline,
        text_height,
        text_width,
        origin,
        extrusion,
        x_direction,
        block_offset,
        annotation_offset,
        annotation: r.opt_handle(340)?,
        dimstyle: r.opt_handle(c::DIMSTYLE)?,
        unknown: (bits & 1 != 0, short, bits & 2 != 0, bits & 4 != 0),
    })
}

pub(super) fn write_paper_viewport(w: &mut TagWriter, v: &PaperViewport) {
    w.p3(10, v.center);
    w.f64(40, v.width);
    w.f64(41, v.height);
    w.i16(69, v.id);
    w.p3_d(17, v.view_target, ORIGIN3);
    w.p3_d(16, v.view_direction, Z);
    w.f64_d(51, v.twist, 0.0);
    w.f64(45, v.view_height);
    w.f64_d(42, v.lens, 50.0);
    w.f64_d(43, v.front_clip, 0.0);
    w.f64_d(44, v.back_clip, 0.0);
    w.f64_d(50, v.snap_angle, 0.0);
    w.p2(12, v.view_center);
    w.p2_d(13, v.snap_base, [0.0, 0.0]);
    w.p2(14, v.snap_spacing);
    w.p2(15, v.grid_spacing);
    w.u16_d(72, v.circle_zoom, 1000);
    w.u16_d(61, v.grid_major, 5);
    w.u32(90, v.status);
    w.str_d(1, &v.style_sheet, "");
    w.u8_d(281, v.render_mode, 0);
    w.bool_d(74, v.ucs_at_origin, true);
    w.bool_d(71, v.ucs_per_viewport, true);
    w.p3_d(110, v.ucs_origin, ORIGIN3);
    w.p3_d(111, v.ucs_x, X);
    w.p3_d(112, v.ucs_y, Y);
    w.f64_d(146, v.ucs_elevation, 0.0);
    w.u16_d(79, v.ucs_ortho_type, 0);
    w.u16_d(170, v.shade_plot_mode, 0);
    w.bool_d(292, v.default_lights, true);
    w.u8_d(282, v.lighting_type, 1);
    w.f64_d(141, v.brightness, 0.0);
    w.f64_d(142, v.contrast, 0.0);
    w.u32_d(o::COLOR_WORD, v.ambient.to_method_word(), COLOR_WORD_250);
    w.count_d(c::LIST_COUNT, v.frozen_layers.len());
    for h in &v.frozen_layers {
        w.handle(331, *h);
    }
    w.opt_handle(340, v.clip_boundary);
    w.opt_handle(345, v.named_ucs);
    w.opt_handle(346, v.base_ucs);
    w.opt_handle(332, v.background);
    w.opt_handle(348, v.visual_style);
    w.opt_handle(333, v.shade_plot);
    w.opt_handle(361, v.sun);
}

pub(super) fn read_paper_viewport(r: &mut TagReader) -> Result<PaperViewport> {
    let center = r.p3(10)?;
    let width = r.f64(40)?;
    let height = r.f64(41)?;
    let id = r.i16(69)?;
    let view_target = r.p3_or(17, ORIGIN3)?;
    let view_direction = r.p3_or(16, Z)?;
    let twist = r.f64_or(51, 0.0)?;
    let view_height = r.f64(45)?;
    let lens = r.f64_or(42, 50.0)?;
    let front_clip = r.f64_or(43, 0.0)?;
    let back_clip = r.f64_or(44, 0.0)?;
    let snap_angle = r.f64_or(50, 0.0)?;
    let view_center = r.p2(12)?;
    let snap_base = r.p2_or(13, [0.0, 0.0])?;
    let snap_spacing = r.p2(14)?;
    let grid_spacing = r.p2(15)?;
    let circle_zoom = r.u16_or(72, 1000)?;
    let grid_major = r.u16_or(61, 5)?;
    let status = r.u32(90)?;
    let style_sheet = r.str_or(1, "")?;
    let render_mode = r.u8_or(281, 0)?;
    let ucs_at_origin = r.bool_or(74, true)?;
    let ucs_per_viewport = r.bool_or(71, true)?;
    let ucs_origin = r.p3_or(110, ORIGIN3)?;
    let ucs_x = r.p3_or(111, X)?;
    let ucs_y = r.p3_or(112, Y)?;
    let ucs_elevation = r.f64_or(146, 0.0)?;
    let ucs_ortho_type = r.u16_or(79, 0)?;
    let shade_plot_mode = r.u16_or(170, 0)?;
    let default_lights = r.bool_or(292, true)?;
    let lighting_type = r.u8_or(282, 1)?;
    let brightness = r.f64_or(141, 0.0)?;
    let contrast = r.f64_or(142, 0.0)?;
    let ambient = color_word(r, o::COLOR_WORD, COLOR_WORD_250)?;
    let n = r.count_or(c::LIST_COUNT, H_BYTES)?;
    let mut frozen_layers = Vec::with_capacity(n);
    for _ in 0..n {
        frozen_layers.push(r.handle(331)?);
    }
    Ok(PaperViewport {
        center,
        width,
        height,
        id,
        view_target,
        view_direction,
        twist,
        view_height,
        lens,
        front_clip,
        back_clip,
        snap_angle,
        view_center,
        snap_base,
        snap_spacing,
        grid_spacing,
        circle_zoom,
        grid_major,
        frozen_layers,
        status,
        style_sheet,
        render_mode,
        ucs_at_origin,
        ucs_per_viewport,
        ucs_origin,
        ucs_x,
        ucs_y,
        ucs_elevation,
        ucs_ortho_type,
        shade_plot_mode,
        default_lights,
        lighting_type,
        brightness,
        contrast,
        ambient,
        clip_boundary: r.opt_handle(340)?,
        named_ucs: r.opt_handle(345)?,
        base_ucs: r.opt_handle(346)?,
        background: r.opt_handle(332)?,
        visual_style: r.opt_handle(348)?,
        shade_plot: r.opt_handle(333)?,
        sun: r.opt_handle(361)?,
    })
}

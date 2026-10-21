//! The record schema: for every object kind, the fields a record may hold,
//! in order, with each field's group code, value type, meaning and default.
//!
//! The same table does three jobs:
//! - It is written into every file as the `Schema` section, so a file
//!   describes its own records: a reader can name the fields of a kind it
//!   does not know, and nobody has to guess what one of our 2000+ codes is.
//! - It fixes the default values. From format 2.0 a field equal to its
//!   default is left out of the record, and readers fill it back in.
//! - The tests check the record code against it: order, types and defaults
//!   (`native::tests::the_schema_matches_the_record_code`).
//!
//! Defaults are part of the format: changing one needs a new major
//! version, because older files leave the field out.

use super::tags::{ty, TagWriter};
use std::f64::consts::TAU;

/// How a field appears in a record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Def {
    /// Always present.
    Req,
    /// Present only when it has a value (a handle that is not null, an
    /// option that is set).
    Null,
    /// Present or not depending on other fields (one of several color
    /// codes; a handle that only follows a certain flag).
    Cond,
    /// Left out when equal to this value.
    F64(f64),
    I16(i16),
    I32(i32),
    Bool(bool),
    Str(&'static str),
    P2([f64; 2]),
    P3([f64; 3]),
}

/// Plain field, the count of a list, or a field repeated per list item
/// (the items of a list follow its count).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Plain,
    Count,
    Item,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    pub code: u16,
    /// Value type (`tags::ty`).
    pub ty: u8,
    pub def: Def,
    pub role: Role,
    pub name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindSpec {
    /// Group 2001 value; −1 and −2 describe the parts every object and every
    /// entity start with.
    pub kind: i16,
    pub name: &'static str,
    pub fields: &'static [Field],
}

const fn f(code: u16, ty: u8, def: Def, name: &'static str) -> Field {
    Field {
        code,
        ty,
        def,
        role: Role::Plain,
        name,
    }
}
/// A list count, left out when the list is empty.
const fn count(code: u16, name: &'static str) -> Field {
    Field {
        code,
        ty: ty::I32,
        def: Def::I32(0),
        role: Role::Count,
        name,
    }
}
/// A list count that is always present.
const fn count_req(code: u16, name: &'static str) -> Field {
    Field {
        code,
        ty: ty::I32,
        def: Def::Req,
        role: Role::Count,
        name,
    }
}
const fn item(code: u16, ty: u8, def: Def, name: &'static str) -> Field {
    Field {
        code,
        ty,
        def,
        role: Role::Item,
        name,
    }
}

use Def::*;

/// The default extrusion (normal) of planar entities.
pub const Z: [f64; 3] = [0.0, 0.0, 1.0];
/// Default x-axis direction of MTEXT.
pub const X: [f64; 3] = [1.0, 0.0, 0.0];
pub const ORIGIN2: [f64; 2] = [0.0, 0.0];
pub const ORIGIN3: [f64; 3] = [0.0, 0.0, 0.0];
pub const ONES: [f64; 3] = [1.0, 1.0, 1.0];
/// Default y-axis direction of a UCS.
pub const Y: [f64; 3] = [0.0, 1.0, 0.0];
/// Lineweight codes (see FORMAT.md): by layer, by block, default.
pub const LW_BY_LAYER: i16 = 29;
pub const LW_BY_BLOCK: i16 = 30;
pub const LW_DEFAULT: i16 = 31;
/// Color method word for "by block" (dimension style colours).
pub const COLOR_WORD_BY_BLOCK: i32 = 0xC100_0000u32 as i32;

const OBJECT: &[Field] = &[
    f(
        0,
        ty::STR,
        Cond,
        "type name (left out for DWG's fixed types, whose name follows from group 2000)",
    ),
    f(
        5,
        ty::HANDLE,
        Req,
        "key: the handle the object is stored under",
    ),
    f(2000, ty::I16, Req, "DWG object type number"),
    f(
        2133,
        ty::HANDLE,
        Null,
        "own handle, when it differs from the key",
    ),
    f(330, ty::HANDLE, Null, "owner"),
    f(
        102,
        ty::STR,
        Cond,
        "\"{ACAD_REACTORS\", then 330 per reactor, then \"}\"",
    ),
    f(
        102,
        ty::STR,
        Cond,
        "\"{ACAD_XDICTIONARY\", 360 extension dictionary, \"}\"",
    ),
    f(2002, ty::I64, Null, "offset in the source DWG"),
    f(2003, ty::I64, Null, "size in the source DWG"),
    f(2004, ty::STR, Null, "import note"),
];

const ENTITY: &[Field] = &[
    f(
        2005,
        ty::I16,
        Req,
        "space: 0 owned by a block, 1 paper, 2 model; marks the entity part",
    ),
    f(2006, ty::HANDLE, Null, "layer"),
    f(
        2007,
        ty::I16,
        I16(0),
        "linetype: 0 by layer, 1 by block, 2 continuous, 3 by handle",
    ),
    f(2008, ty::HANDLE, Cond, "linetype record (when 2007 is 3)"),
    f(
        62,
        ty::I16,
        Cond,
        "color index: 0 by block, 1-255; absent with 420 and 2009 means by layer",
    ),
    f(420, ty::I32, Cond, "true color 0x00RRGGBB"),
    f(2009, ty::BOOL, Cond, "no color"),
    f(440, ty::I32, Null, "transparency word as DWG stores it"),
    f(48, ty::F64, F64(1.0), "linetype scale"),
    f(370, ty::I16, I16(LW_BY_LAYER), "lineweight code"),
    f(60, ty::I16, Null, "1 when invisible"),
    f(390, ty::HANDLE, Null, "plot style"),
    f(347, ty::HANDLE, Null, "material"),
];

const TEXT_FIELDS: &[Field] = &[
    f(10, ty::P2, Req, "insertion point"),
    f(11, ty::P2, Req, "alignment point"),
    f(38, ty::F64, F64(0.0), "elevation"),
    f(210, ty::P3, P3(Z), "extrusion"),
    f(39, ty::F64, F64(0.0), "thickness"),
    f(51, ty::F64, F64(0.0), "oblique angle (radians)"),
    f(50, ty::F64, F64(0.0), "rotation (radians)"),
    f(40, ty::F64, Req, "height"),
    f(41, ty::F64, F64(1.0), "width factor"),
    f(1, ty::STR, Req, "text"),
    f(
        71,
        ty::I16,
        I16(0),
        "generation flags: 2 backwards, 4 upside down",
    ),
    f(72, ty::I16, I16(0), "horizontal alignment"),
];

macro_rules! fields {
    ($($part:expr),* $(,)?) => {{
        const N: usize = 0 $(+ $part.len())*;
        const ALL: [Field; N] = {
            let mut out = [f(0, 0, Req, ""); N];
            let mut n = 0;
            $(
                let p: &[Field] = $part;
                let mut i = 0;
                while i < p.len() {
                    out[n] = p[i];
                    n += 1;
                    i += 1;
                }
            )*
            out
        };
        &ALL
    }};
}

pub static KINDS: &[KindSpec] = &[
    KindSpec {
        kind: -1,
        name: "(every object)",
        fields: OBJECT,
    },
    KindSpec {
        kind: -2,
        name: "(every entity, after the object part)",
        fields: ENTITY,
    },
    KindSpec {
        kind: 0,
        name: "NOT_DECODED",
        fields: &[],
    },
    KindSpec {
        kind: 1,
        name: "FAILED",
        fields: &[f(2125, ty::STR, Req, "why the DWG object did not decode")],
    },
    KindSpec {
        kind: 2,
        name: "LINE",
        fields: &[
            f(10, ty::P3, Req, "start"),
            f(11, ty::P3, Req, "end"),
            f(39, ty::F64, F64(0.0), "thickness"),
            f(210, ty::P3, P3(Z), "extrusion"),
        ],
    },
    KindSpec {
        kind: 3,
        name: "CIRCLE",
        fields: &[
            f(10, ty::P3, Req, "center (OCS)"),
            f(40, ty::F64, Req, "radius"),
            f(39, ty::F64, F64(0.0), "thickness"),
            f(210, ty::P3, P3(Z), "extrusion"),
        ],
    },
    KindSpec {
        kind: 4,
        name: "ARC",
        fields: &[
            f(10, ty::P3, Req, "center (OCS)"),
            f(40, ty::F64, Req, "radius"),
            f(39, ty::F64, F64(0.0), "thickness"),
            f(210, ty::P3, P3(Z), "extrusion"),
            f(50, ty::F64, F64(0.0), "start angle (radians)"),
            f(51, ty::F64, Req, "end angle (radians)"),
        ],
    },
    KindSpec {
        kind: 5,
        name: "POINT",
        fields: &[
            f(10, ty::P3, Req, "position"),
            f(39, ty::F64, F64(0.0), "thickness"),
            f(210, ty::P3, P3(Z), "extrusion"),
            f(50, ty::F64, F64(0.0), "x-axis angle (radians)"),
        ],
    },
    KindSpec {
        kind: 6,
        name: "ELLIPSE",
        fields: &[
            f(10, ty::P3, Req, "center (WCS)"),
            f(11, ty::P3, Req, "major axis end, from the center"),
            f(210, ty::P3, P3(Z), "extrusion"),
            f(40, ty::F64, Req, "minor to major axis ratio"),
            f(41, ty::F64, F64(0.0), "start parameter"),
            f(42, ty::F64, F64(TAU), "end parameter"),
        ],
    },
    KindSpec {
        kind: 7,
        name: "LWPOLYLINE",
        fields: &[
            f(2132, ty::I16, I16(0), "flag word as DWG stores it (0x200 closed)"),
            f(43, ty::F64, F64(0.0), "constant width"),
            f(38, ty::F64, F64(0.0), "elevation"),
            f(39, ty::F64, F64(0.0), "thickness"),
            f(210, ty::P3, P3(Z), "extrusion"),
            count_req(90, "vertex count"),
            item(10, ty::P2, Req, "vertex"),
            count(2113, "bulge count"),
            item(42, ty::F64, Req, "bulge"),
            count(2114, "width pair count"),
            item(40, ty::F64, Req, "start width"),
            item(41, ty::F64, Req, "end width"),
            count(2115, "vertex id count"),
            item(91, ty::I32, Req, "vertex id"),
        ],
    },
    KindSpec {
        kind: 8,
        name: "POLYLINE",
        fields: &[
            f(2100, ty::I16, Req, "subkind: 0 2D, 1 3D, 2 polyface mesh, 3 polygon mesh"),
            f(70, ty::I16, I16(0), "flags"),
            f(75, ty::I16, I16(0), "curve type"),
            f(40, ty::F64, F64(0.0), "default start width"),
            f(41, ty::F64, F64(0.0), "default end width"),
            f(39, ty::F64, F64(0.0), "thickness"),
            f(38, ty::F64, F64(0.0), "elevation"),
            f(210, ty::P3, P3(Z), "extrusion"),
            count(2111, "vertex count"),
            item(2112, ty::HANDLE, Req, "vertex"),
            f(2106, ty::HANDLE, Null, "first vertex"),
            f(2107, ty::HANDLE, Null, "last vertex"),
            f(2108, ty::HANDLE, Null, "SEQEND"),
        ],
    },
    KindSpec {
        kind: 9,
        name: "VERTEX",
        fields: &[
            f(2100, ty::I16, Req, "subkind: 0 2D, 1 3D, 2 mesh, 3 polyface vertex, 4 polyface face"),
            f(70, ty::I16, I16(0), "flags"),
            f(10, ty::P3, Req, "point"),
            f(40, ty::F64, F64(0.0), "start width"),
            f(41, ty::F64, F64(0.0), "end width"),
            f(42, ty::F64, F64(0.0), "bulge"),
            f(50, ty::F64, F64(0.0), "tangent direction"),
            f(91, ty::I32, I32(0), "vertex id"),
            f(71, ty::I16, I16(0), "face vertex 1"),
            f(72, ty::I16, I16(0), "face vertex 2"),
            f(73, ty::I16, I16(0), "face vertex 3"),
            f(74, ty::I16, I16(0), "face vertex 4"),
        ],
    },
    KindSpec {
        kind: 10,
        name: "SEQEND",
        fields: &[],
    },
    KindSpec {
        kind: 11,
        name: "TEXT",
        fields: fields!(
            TEXT_FIELDS,
            &[
                f(73, ty::I16, I16(0), "vertical alignment"),
                f(2101, ty::HANDLE, Null, "text style"),
            ]
        ),
    },
    KindSpec {
        kind: 12,
        name: "ATTRIB / ATTDEF",
        fields: fields!(
            TEXT_FIELDS,
            &[
                f(74, ty::I16, I16(0), "vertical alignment"),
                f(2101, ty::HANDLE, Null, "text style"),
                f(2, ty::STR, Req, "tag"),
                f(3, ty::STR, Str(""), "prompt"),
                f(70, ty::I16, I16(0), "flags"),
                f(73, ty::I16, I16(0), "field length"),
                f(2102, ty::BOOL, Bool(false), "is an ATTDEF"),
            ]
        ),
    },
    KindSpec {
        kind: 13,
        name: "MTEXT",
        fields: &[
            f(10, ty::P3, Req, "insertion point"),
            f(210, ty::P3, P3(Z), "extrusion"),
            f(11, ty::P3, P3(X), "x-axis direction"),
            f(41, ty::F64, F64(0.0), "reference width"),
            f(46, ty::F64, F64(0.0), "reference height"),
            f(40, ty::F64, Req, "text height"),
            f(71, ty::I16, I16(1), "attachment: 1 top left … 9 bottom right"),
            f(72, ty::I16, I16(1), "drawing direction"),
            f(1, ty::STR, Req, "contents with formatting codes"),
            f(44, ty::F64, F64(1.0), "line-spacing factor"),
            f(2101, ty::HANDLE, Null, "text style"),
        ],
    },
    KindSpec {
        kind: 14,
        name: "INSERT / MINSERT",
        fields: &[
            f(10, ty::P3, Req, "insertion point"),
            f(41, ty::F64, F64(1.0), "x scale"),
            f(42, ty::F64, F64(1.0), "y scale"),
            f(43, ty::F64, F64(1.0), "z scale"),
            f(50, ty::F64, F64(0.0), "rotation (radians)"),
            f(210, ty::P3, P3(Z), "extrusion"),
            f(66, ty::BOOL, Bool(false), "has attributes"),
            f(2103, ty::HANDLE, Null, "block record"),
            count(2111, "attribute count"),
            item(2112, ty::HANDLE, Req, "attribute"),
            f(2106, ty::HANDLE, Null, "first attribute"),
            f(2107, ty::HANDLE, Null, "last attribute"),
            f(2108, ty::HANDLE, Null, "SEQEND"),
            f(70, ty::I16, Cond, "MINSERT columns (70, 71, 44, 45 appear together)"),
            f(71, ty::I16, Cond, "MINSERT rows"),
            f(44, ty::F64, Cond, "MINSERT column spacing"),
            f(45, ty::F64, Cond, "MINSERT row spacing"),
        ],
    },
    KindSpec {
        kind: 15,
        name: "SOLID / TRACE",
        fields: &[
            f(39, ty::F64, F64(0.0), "thickness"),
            f(38, ty::F64, F64(0.0), "elevation"),
            f(10, ty::P2, Req, "corner 1"),
            f(11, ty::P2, Req, "corner 2"),
            f(12, ty::P2, Req, "corner 3"),
            f(13, ty::P2, Req, "corner 4"),
            f(210, ty::P3, P3(Z), "extrusion"),
            f(2117, ty::BOOL, Bool(false), "is a TRACE"),
        ],
    },
    KindSpec {
        kind: 16,
        name: "3DFACE",
        fields: &[
            f(10, ty::P3, Req, "corner 1"),
            f(11, ty::P3, Req, "corner 2"),
            f(12, ty::P3, Req, "corner 3"),
            f(13, ty::P3, Req, "corner 4"),
            f(70, ty::I16, I16(0), "invisible-edge flags"),
        ],
    },
    KindSpec {
        kind: 17,
        name: "SPLINE",
        fields: &[
            f(70, ty::I16, I16(0), "flags: 1 closed, 2 periodic, 4 rational"),
            f(71, ty::I32, Req, "degree"),
            count(2127, "knot count"),
            item(40, ty::F64, Req, "knot"),
            count(2128, "control point count"),
            item(10, ty::P3, Req, "control point"),
            count(2116, "weight count"),
            item(41, ty::F64, Req, "weight"),
            count(2129, "fit point count"),
            item(11, ty::P3, Req, "fit point"),
            f(44, ty::F64, F64(0.0), "fit tolerance"),
            f(12, ty::P3, P3(ORIGIN3), "start tangent"),
            f(13, ty::P3, P3(ORIGIN3), "end tangent"),
        ],
    },
    KindSpec {
        kind: 18,
        name: "DIMENSION",
        fields: &[
            f(2100, ty::I16, Req, "subkind: 0 linear, 1 aligned, 2 ordinate, 3 angular 3-point, 4 angular 2-line, 5 radius, 6 diameter"),
            f(210, ty::P3, P3(Z), "extrusion"),
            f(11, ty::P2, Req, "text midpoint"),
            f(38, ty::F64, F64(0.0), "elevation"),
            f(2132, ty::I16, I16(0), "flags as DWG stores them"),
            f(1, ty::STR, Str(""), "user text (<> is the measurement)"),
            f(53, ty::F64, F64(0.0), "text rotation"),
            f(51, ty::F64, F64(0.0), "horizontal direction"),
            f(2118, ty::P3, P3(ONES), "insert scale"),
            f(2126, ty::F64, F64(0.0), "insert rotation"),
            f(71, ty::I16, I16(0), "text attachment"),
            f(42, ty::F64, Req, "measurement"),
            f(2119, ty::P2, P2(ORIGIN2), "clone insertion point"),
            f(10, ty::P3, Req, "definition point"),
            f(13, ty::P3, P3(ORIGIN3), "definition point 13"),
            f(14, ty::P3, P3(ORIGIN3), "definition point 14"),
            f(15, ty::P3, P3(ORIGIN3), "definition point 15"),
            f(16, ty::P2, P2(ORIGIN2), "definition point 16"),
            f(52, ty::F64, F64(0.0), "extension-line rotation"),
            f(50, ty::F64, F64(0.0), "dimension rotation"),
            f(40, ty::F64, F64(0.0), "leader length"),
            f(2105, ty::HANDLE, Null, "dimension style"),
            f(2104, ty::HANDLE, Null, "graphics block (*D)"),
        ],
    },
    KindSpec {
        kind: 19,
        name: "HATCH",
        fields: &[
            f(38, ty::F64, F64(0.0), "elevation"),
            f(210, ty::P3, P3(Z), "extrusion"),
            f(2, ty::STR, Req, "pattern name"),
            f(70, ty::BOOL, Bool(false), "solid fill"),
            f(71, ty::BOOL, Bool(false), "associative"),
            f(450, ty::BOOL, Bool(false), "gradient"),
            f(75, ty::I16, I16(0), "style"),
            f(76, ty::I16, I16(1), "pattern type"),
            f(52, ty::F64, F64(0.0), "pattern angle"),
            f(41, ty::F64, F64(1.0), "pattern scale"),
            count_req(2130, "boundary path count"),
            item(2100, ty::I16, Req, "path kind: 0 edges, 1 polyline"),
            item(92, ty::I32, I32(0), "path flags"),
            item(2131, ty::I32, Req, "edge count (edge paths)"),
            item(72, ty::I16, Req, "edge type: 1 line, 2 arc, 3 ellipse, 4 spline"),
            item(10, ty::P2, Req, "edge start, center or control point; polyline vertex"),
            item(11, ty::P2, Req, "edge end or major axis"),
            item(40, ty::F64, Req, "radius, axis ratio or knot"),
            item(50, ty::F64, Req, "start angle"),
            item(51, ty::F64, Req, "end angle"),
            item(73, ty::BOOL, Req, "counterclockwise (arcs, ellipses); closed (polyline paths)"),
            item(94, ty::I32, Req, "spline degree"),
            item(2127, ty::I32, Req, "spline knot count"),
            item(2128, ty::I32, Req, "spline control point count"),
            item(2116, ty::I32, Req, "spline weight count"),
            item(93, ty::I32, Req, "polyline vertex count"),
            item(42, ty::F64, F64(0.0), "bulge or spline weight"),
            f(77, ty::BOOL, Bool(false), "double hatch (2.4)"),
            count(78, "pattern line count (2.4)"),
            item(53, ty::F64, Req, "pattern line angle, radians"),
            item(43, ty::P2, Req, "pattern line base point"),
            item(45, ty::P2, Req, "pattern line offset to the next line"),
            item(79, ty::I32, Req, "pattern line dash count"),
            item(49, ty::F64, Req, "dash length: positive dash, negative gap, 0 dot"),
            count(98, "seed point count (2.4)"),
            item(2188, ty::P2, Req, "seed point"),
        ],
    },
    KindSpec {
        kind: 20,
        name: "BLOCK",
        fields: &[f(2, ty::STR, Req, "block name")],
    },
    KindSpec {
        kind: 21,
        name: "ENDBLK",
        fields: &[],
    },
    KindSpec {
        kind: 22,
        name: "BLOCK_HEADER",
        fields: &[
            f(2, ty::STR, Req, "name"),
            f(70, ty::I16, I16(0), "flags: 1 anonymous, 2 has attributes, 4 xref, 8 overlay"),
            f(10, ty::P3, P3(ORIGIN3), "base point"),
            f(1, ty::STR, Str(""), "xref path"),
            f(4, ty::STR, Str(""), "description"),
            f(2120, ty::I16, I16(0), "insert units"),
            f(2110, ty::HANDLE, Null, "BLOCK entity"),
            count(2111, "entity count"),
            item(2112, ty::HANDLE, Req, "entity, in drawing order"),
            f(2106, ty::HANDLE, Null, "first entity"),
            f(2107, ty::HANDLE, Null, "last entity"),
            f(2109, ty::HANDLE, Null, "ENDBLK"),
            f(340, ty::HANDLE, Null, "layout"),
            f(2140, ty::STR, Str(""), "part material (Proposed)"),
            f(2141, ty::STR, Str(""), "part grade (Proposed)"),
            f(2142, ty::F64, F64(0.0), "part thickness in drawing units (Proposed)"),
            f(2143, ty::I32, I32(0), "part quantity (Proposed)"),
        ],
    },
    KindSpec {
        kind: 23,
        name: "LAYER",
        fields: &[
            f(2, ty::STR, Req, "name"),
            f(2132, ty::I16, I16(0), "flag word as DWG stores it"),
            f(2121, ty::I16, Req, "state: 1 frozen, 2 off, 4 locked, 8 plots"),
            f(62, ty::I16, Cond, "color index (or 420, 2009, as for entities)"),
            f(420, ty::I32, Cond, "true color"),
            f(2009, ty::BOOL, Cond, "no color"),
            f(2122, ty::HANDLE, Null, "linetype"),
            f(370, ty::I16, I16(LW_DEFAULT), "lineweight code"),
        ],
    },
    KindSpec {
        kind: 24,
        name: "TABLE_CONTROL",
        fields: &[
            count(2111, "entry count"),
            item(2112, ty::HANDLE, Req, "entry"),
            f(2123, ty::HANDLE, Null, "model space block record"),
            f(2124, ty::HANDLE, Null, "paper space block record"),
        ],
    },
    KindSpec {
        kind: 25,
        name: "NAMED_RECORD",
        fields: &[f(2, ty::STR, Req, "name")],
    },
    KindSpec {
        kind: 26,
        name: "LTYPE",
        fields: &[
            f(2, ty::STR, Req, "name"),
            f(70, ty::I16, I16(0), "flags"),
            f(3, ty::STR, Str(""), "description"),
            f(40, ty::F64, F64(0.0), "total pattern length"),
            count(73, "element count"),
            item(49, ty::F64, Req, "length: positive dash, negative gap, 0 dot"),
            item(74, ty::I16, I16(0), "complex flags: 1 absolute rotation, 2 text, 4 shape"),
            item(75, ty::I16, I16(0), "shape number"),
            item(340, ty::HANDLE, Null, "text style or shape file"),
            item(46, ty::F64, F64(1.0), "scale"),
            item(50, ty::F64, F64(0.0), "rotation (radians)"),
            item(44, ty::P2, P2(ORIGIN2), "offset"),
            item(9, ty::STR, Str(""), "text"),
        ],
    },
    KindSpec {
        kind: 27,
        name: "STYLE",
        fields: &[
            f(2, ty::STR, Req, "name"),
            f(70, ty::I16, I16(0), "flags: 1 shape file, 4 vertical"),
            f(40, ty::F64, F64(0.0), "fixed height (0 none)"),
            f(41, ty::F64, F64(1.0), "width factor"),
            f(50, ty::F64, F64(0.0), "oblique angle (radians)"),
            f(71, ty::I16, I16(0), "generation flags"),
            f(42, ty::F64, F64(0.0), "last height used"),
            f(3, ty::STR, Str(""), "font file"),
            f(4, ty::STR, Str(""), "big-font file"),
        ],
    },
    KindSpec {
        kind: 28,
        name: "DIMSTYLE",
        fields: &[
            f(2, ty::STR, Req, "name"),
            f(70, ty::I16, I16(0), "flags"),
            f(3, ty::STR, Str(""), "DIMPOST"),
            f(40, ty::F64, F64(1.0), "DIMSCALE"),
            f(41, ty::F64, F64(0.0), "DIMASZ"),
            f(42, ty::F64, F64(0.0), "DIMEXO"),
            f(43, ty::F64, F64(0.0), "DIMDLI"),
            f(44, ty::F64, F64(0.0), "DIMEXE"),
            f(45, ty::F64, F64(0.0), "DIMRND"),
            f(46, ty::F64, F64(0.0), "DIMDLE"),
            f(140, ty::F64, F64(0.0), "DIMTXT"),
            f(141, ty::F64, F64(0.0), "DIMCEN"),
            f(142, ty::F64, F64(0.0), "DIMTSZ"),
            f(143, ty::F64, F64(25.4), "DIMALTF"),
            f(144, ty::F64, F64(1.0), "DIMLFAC"),
            f(147, ty::F64, F64(0.0), "DIMGAP"),
            f(73, ty::BOOL, Bool(true), "DIMTIH"),
            f(74, ty::BOOL, Bool(true), "DIMTOH"),
            f(77, ty::I16, I16(0), "DIMTAD"),
            f(78, ty::I16, I16(0), "DIMZIN"),
            f(170, ty::BOOL, Bool(false), "DIMALT"),
            f(171, ty::I16, I16(2), "DIMALTD"),
            f(2134, ty::I32, I32(COLOR_WORD_BY_BLOCK), "DIMCLRD as a color method word"),
            f(2135, ty::I32, I32(COLOR_WORD_BY_BLOCK), "DIMCLRE as a color method word"),
            f(2136, ty::I32, I32(COLOR_WORD_BY_BLOCK), "DIMCLRT as a color method word"),
            f(271, ty::I16, I16(2), "DIMDEC"),
            f(276, ty::I16, I16(0), "DIMFRAC"),
            f(277, ty::I16, I16(2), "DIMLUNIT"),
            f(278, ty::I32, I32('.' as i32), "DIMDSEP (a Unicode scalar value)"),
            f(280, ty::I16, I16(0), "DIMJUST"),
            f(340, ty::HANDLE, Null, "DIMTXSTY"),
            f(371, ty::I16, I16(LW_BY_BLOCK), "DIMLWD"),
            f(372, ty::I16, I16(LW_BY_BLOCK), "DIMLWE"),
        ],
    },    // Format 2.2.
    KindSpec {
        kind: 29,
        name: "VPORT",
        fields: &[
            f(2, ty::STR, Req, "name"),
            f(40, ty::F64, Req, "view height"),
            f(41, ty::F64, Req, "aspect ratio (width over height)"),
            f(12, ty::P2, Req, "view centre"),
            f(17, ty::P3, Req, "target"),
            f(16, ty::P3, Req, "view direction"),
            f(51, ty::F64, Req, "twist angle, radians"),
            f(42, ty::F64, Req, "lens length"),
            f(43, ty::F64, Req, "front clip"),
            f(44, ty::F64, Req, "back clip"),
            f(71, ty::I16, Req, "view mode (DXF 71)"),
            f(281, ty::I16, Req, "render mode"),
            f(292, ty::BOOL, Req, "default lighting"),
            f(282, ty::I16, Req, "lighting type"),
            f(141, ty::F64, Req, "brightness"),
            f(142, ty::F64, Req, "contrast"),
            f(2144, ty::I32, I32(super::objects::COLOR_WORD_250), "ambient colour as a color method word"),
            f(10, ty::P2, Req, "lower-left corner"),
            f(11, ty::P2, Req, "upper-right corner"),
            f(72, ty::I16, Req, "circle zoom percent"),
            f(73, ty::BOOL, Req, "fast zoom"),
            f(74, ty::I16, Req, "UCS icon setting"),
            f(76, ty::BOOL, Req, "grid on"),
            f(15, ty::P2, Req, "grid spacing"),
            f(75, ty::BOOL, Req, "snap on"),
            f(77, ty::BOOL, Req, "snap style (isometric)"),
            f(78, ty::I16, Req, "isometric snap plane"),
            f(50, ty::F64, Req, "snap rotation, radians"),
            f(13, ty::P2, Req, "snap base"),
            f(14, ty::P2, Req, "snap spacing"),
            f(2145, ty::BOOL, Req, "the DWG record's unnamed bit"),
            f(65, ty::BOOL, Req, "UCS per viewport"),
            f(110, ty::P3, Req, "UCS origin"),
            f(111, ty::P3, Req, "UCS X axis"),
            f(112, ty::P3, Req, "UCS Y axis"),
            f(146, ty::F64, Req, "UCS elevation"),
            f(79, ty::I16, Req, "UCS orthographic type"),
            f(60, ty::I16, Req, "grid flags"),
            f(61, ty::I16, Req, "major grid lines"),
            f(332, ty::HANDLE, Null, "background"),
            f(348, ty::HANDLE, Null, "visual style"),
            f(361, ty::HANDLE, Null, "sun"),
            f(345, ty::HANDLE, Null, "named UCS"),
            f(346, ty::HANDLE, Null, "base UCS"),
        ],
    },
    KindSpec {
        kind: 30,
        name: "DIMSTYLE_RECORD",
        fields: &[
            f(2, ty::STR, Req, "name"),
            f(3, ty::STR, Req, "DIMPOST"),
            f(4, ty::STR, Req, "DIMAPOST"),
            f(40, ty::F64, Req, "DIMSCALE"),
            f(41, ty::F64, Req, "DIMASZ"),
            f(42, ty::F64, Req, "DIMEXO"),
            f(43, ty::F64, Req, "DIMDLI"),
            f(44, ty::F64, Req, "DIMEXE"),
            f(45, ty::F64, Req, "DIMRND"),
            f(46, ty::F64, Req, "DIMDLE"),
            f(47, ty::F64, Req, "DIMTP"),
            f(48, ty::F64, Req, "DIMTM"),
            f(49, ty::F64, Req, "DIMFXL"),
            f(50, ty::F64, Req, "DIMJOGANG, radians"),
            f(69, ty::I16, Req, "DIMTFILL"),
            f(2144, ty::I32, I32(COLOR_WORD_BY_BLOCK), "DIMTFILLCLR as a color method word"),
            f(71, ty::BOOL, Req, "DIMTOL"),
            f(72, ty::BOOL, Req, "DIMLIM"),
            f(73, ty::BOOL, Req, "DIMTIH"),
            f(74, ty::BOOL, Req, "DIMTOH"),
            f(75, ty::BOOL, Req, "DIMSE1"),
            f(76, ty::BOOL, Req, "DIMSE2"),
            f(77, ty::I16, Req, "DIMTAD"),
            f(78, ty::I16, Req, "DIMZIN"),
            f(79, ty::I16, Req, "DIMAZIN"),
            f(90, ty::I16, Req, "DIMARCSYM"),
            f(140, ty::F64, Req, "DIMTXT"),
            f(141, ty::F64, Req, "DIMCEN"),
            f(142, ty::F64, Req, "DIMTSZ"),
            f(143, ty::F64, Req, "DIMALTF"),
            f(144, ty::F64, Req, "DIMLFAC"),
            f(145, ty::F64, Req, "DIMTVP"),
            f(146, ty::F64, Req, "DIMTFAC"),
            f(147, ty::F64, Req, "DIMGAP"),
            f(148, ty::F64, Req, "DIMALTRND"),
            f(170, ty::BOOL, Req, "DIMALT"),
            f(171, ty::I16, Req, "DIMALTD"),
            f(172, ty::BOOL, Req, "DIMTOFL"),
            f(173, ty::BOOL, Req, "DIMSAH"),
            f(174, ty::BOOL, Req, "DIMTIX"),
            f(175, ty::BOOL, Req, "DIMSOXD"),
            f(2134, ty::I32, I32(COLOR_WORD_BY_BLOCK), "DIMCLRD as a color method word"),
            f(2135, ty::I32, I32(COLOR_WORD_BY_BLOCK), "DIMCLRE as a color method word"),
            f(2136, ty::I32, I32(COLOR_WORD_BY_BLOCK), "DIMCLRT as a color method word"),
            f(179, ty::I16, Req, "DIMADEC"),
            f(271, ty::I16, Req, "DIMDEC"),
            f(272, ty::I16, Req, "DIMTDEC"),
            f(273, ty::I16, Req, "DIMALTU"),
            f(274, ty::I16, Req, "DIMALTTD"),
            f(275, ty::I16, Req, "DIMAUNIT"),
            f(276, ty::I16, Req, "DIMFRAC"),
            f(277, ty::I16, Req, "DIMLUNIT"),
            f(278, ty::I16, Req, "DIMDSEP"),
            f(279, ty::I16, Req, "DIMTMOVE"),
            f(280, ty::I16, Req, "DIMJUST"),
            f(281, ty::BOOL, Req, "DIMSD1"),
            f(282, ty::BOOL, Req, "DIMSD2"),
            f(283, ty::I16, Req, "DIMTOLJ"),
            f(284, ty::I16, Req, "DIMTZIN"),
            f(285, ty::I16, Req, "DIMALTZ"),
            f(286, ty::I16, Req, "DIMALTTZ"),
            f(288, ty::BOOL, Req, "DIMUPT"),
            f(289, ty::I16, Req, "DIMATFIT"),
            f(290, ty::BOOL, Req, "DIMFXLON"),
            f(294, ty::BOOL, Req, "DIMTXTDIRECTION"),
            f(2146, ty::F64, Req, "DIMALTMZF"),
            f(2147, ty::STR, Req, "DIMALTMZS"),
            f(2148, ty::F64, Req, "DIMMZF"),
            f(2149, ty::STR, Req, "DIMMZS"),
            f(371, ty::I16, Req, "DIMLWD (DXF lineweight)"),
            f(372, ty::I16, Req, "DIMLWE (DXF lineweight)"),
            f(2150, ty::BOOL, Req, "the DWG record's trailing bit"),
            f(340, ty::HANDLE, Null, "DIMTXSTY"),
            f(341, ty::HANDLE, Null, "DIMLDRBLK"),
            f(342, ty::HANDLE, Null, "DIMBLK"),
            f(343, ty::HANDLE, Null, "DIMBLK1"),
            f(344, ty::HANDLE, Null, "DIMBLK2"),
            f(345, ty::HANDLE, Null, "DIMLTYPE"),
            f(346, ty::HANDLE, Null, "DIMLTEX1"),
            f(347, ty::HANDLE, Null, "DIMLTEX2"),
        ],
    },
    KindSpec {
        kind: 31,
        name: "DICTIONARY",
        fields: &[
            f(281, ty::I16, Req, "duplicate record cloning"),
            f(280, ty::I16, I16(0), "hard owner"),
            count(2111, "entry count"),
            item(3, ty::STR, Req, "entry key"),
            item(350, ty::HANDLE, Req, "entry object"),
            f(2162, ty::BOOL, Bool(false), "has a default entry (ACDBDICTIONARYWDFLT)"),
            f(340, ty::HANDLE, Cond, "default entry, after 2162"),
        ],
    },
    KindSpec {
        kind: 32,
        name: "XRECORD",
        fields: &[
            f(280, ty::I16, Req, "duplicate record cloning"),
            count(2111, "item count"),
            item(2151, ty::I16, Req, "the item's DXF group code"),
            item(2152, ty::STR, Cond, "string value"),
            item(2153, ty::P3, Cond, "point value"),
            item(2154, ty::F64, Cond, "real value"),
            item(2155, ty::I16, Cond, "8-bit integer value"),
            item(2156, ty::I16, Cond, "16-bit integer value"),
            item(2157, ty::I32, Cond, "32-bit integer value"),
            item(2158, ty::I64, Cond, "64-bit integer value"),
            item(2159, ty::BOOL, Cond, "boolean value"),
            item(2160, ty::BYTES, Cond, "binary value"),
            item(2161, ty::HANDLE, Cond, "handle value"),
        ],
    },
    KindSpec {
        kind: 33,
        name: "ACDBPLACEHOLDER",
        fields: &[],
    },
    KindSpec {
        kind: 34,
        name: "DICTIONARYVAR",
        fields: &[
            f(280, ty::I16, I16(0), "schema number"),
            f(1, ty::STR, Req, "value"),
        ],
    },
    KindSpec {
        kind: 35,
        name: "SCALE",
        fields: &[
            f(70, ty::I16, I16(0), "flag"),
            f(300, ty::STR, Req, "name"),
            f(140, ty::F64, Req, "paper units"),
            f(141, ty::F64, Req, "drawing units"),
            f(290, ty::BOOL, Bool(false), "unit scale"),
        ],
    },
    KindSpec {
        kind: 36,
        name: "LAYOUT",
        fields: &[
            f(1, ty::STR, Req, "page setup name"),
            f(2, ty::STR, Req, "printer or plotter"),
            f(4, ty::STR, Req, "paper size"),
            f(70, ty::I16, Req, "plot layout flags"),
            f(40, ty::F64, Req, "left margin, mm"),
            f(41, ty::F64, Req, "bottom margin, mm"),
            f(42, ty::F64, Req, "right margin, mm"),
            f(43, ty::F64, Req, "top margin, mm"),
            f(44, ty::F64, Req, "paper width, mm"),
            f(45, ty::F64, Req, "paper height, mm"),
            f(46, ty::P2, Req, "plot origin"),
            f(72, ty::I16, Req, "paper units"),
            f(73, ty::I16, Req, "plot rotation"),
            f(74, ty::I16, Req, "plot type"),
            f(48, ty::P2, Req, "plot window lower left"),
            f(140, ty::P2, Req, "plot window upper right"),
            f(142, ty::F64, Req, "custom scale: real-world units"),
            f(143, ty::F64, Req, "custom scale: drawing units"),
            f(7, ty::STR, Req, "plot style sheet"),
            f(75, ty::I16, Req, "standard scale type"),
            f(147, ty::F64, Req, "scale factor"),
            f(148, ty::P2, Req, "paper image origin"),
            f(76, ty::I16, Req, "shade plot mode"),
            f(77, ty::I16, Req, "shade plot resolution level"),
            f(78, ty::I16, Req, "shade plot custom DPI"),
            f(2163, ty::HANDLE, Null, "plot view"),
            f(2164, ty::HANDLE, Null, "plot visual style"),
            f(2165, ty::STR, Req, "layout name"),
            f(2166, ty::I16, Req, "layout flags"),
            f(2167, ty::I32, Req, "tab order"),
            f(2168, ty::P2, Req, "limits min"),
            f(2169, ty::P2, Req, "limits max"),
            f(2170, ty::P3, Req, "insertion base"),
            f(2171, ty::P3, Req, "extents min"),
            f(2172, ty::P3, Req, "extents max"),
            f(2173, ty::F64, Req, "elevation"),
            f(2174, ty::P3, Req, "UCS origin"),
            f(2175, ty::P3, Req, "UCS X axis"),
            f(2176, ty::P3, Req, "UCS Y axis"),
            f(2177, ty::I16, Req, "UCS orthographic type"),
            f(2178, ty::HANDLE, Null, "paper-space block record"),
            f(2179, ty::HANDLE, Null, "last active viewport"),
            f(2180, ty::HANDLE, Null, "base UCS"),
            f(2181, ty::HANDLE, Null, "named UCS"),
            count(2111, "viewport count"),
            item(2112, ty::HANDLE, Req, "viewport"),
        ],
    },
    KindSpec {
        kind: 37,
        name: "MLINESTYLE",
        fields: &[
            f(2, ty::STR, Req, "name"),
            f(3, ty::STR, Req, "description"),
            f(70, ty::I16, Req, "flags (DXF)"),
            f(2144, ty::I32, I32(COLOR_WORD_BY_BLOCK), "fill colour as a color method word"),
            f(51, ty::F64, Req, "start angle, radians"),
            f(52, ty::F64, Req, "end angle, radians"),
            count(2111, "element count"),
            item(49, ty::F64, Req, "element offset"),
            item(2144, ty::I32, I32(COLOR_WORD_BY_BLOCK), "element colour as a color method word"),
            item(2112, ty::HANDLE, Req, "element linetype"),
            item(2182, ty::I16, Req, "element linetype index (before R2018)"),
        ],
    },
    KindSpec {
        kind: 38,
        name: "GROUPS",
        fields: &[
            f(2183, ty::STR, Req, "DXF object type"),
            count(2111, "item count"),
            item(2151, ty::I16, Req, "the item's DXF group code"),
            item(2152, ty::STR, Cond, "string value"),
            item(2153, ty::P3, Cond, "point value"),
            item(2154, ty::F64, Cond, "real value"),
            item(2155, ty::I16, Cond, "8-bit integer value"),
            item(2156, ty::I16, Cond, "16-bit integer value"),
            item(2157, ty::I32, Cond, "32-bit integer value"),
            item(2158, ty::I64, Cond, "64-bit integer value"),
            item(2159, ty::BOOL, Cond, "boolean value"),
            item(2160, ty::BYTES, Cond, "binary value"),
            item(2161, ty::HANDLE, Cond, "handle value"),
        ],
    },
    KindSpec {
        kind: 39,
        name: "XLINE / RAY",
        fields: &[
            f(10, ty::P3, Req, "base point (WCS)"),
            f(11, ty::P3, Req, "direction, a unit vector"),
            f(2184, ty::BOOL, Bool(false), "is a RAY: infinite from the base point on"),
        ],
    },
    KindSpec {
        kind: 40,
        name: "LEADER",
        fields: &[
            f(71, ty::BOOL, Bool(true), "arrowhead"),
            f(72, ty::BOOL, Bool(false), "spline path"),
            f(73, ty::I16, I16(3), "annotation type: 0 MTEXT, 1 tolerance, 2 block reference, 3 none"),
            f(74, ty::BOOL, Bool(true), "hook line along the horizontal direction"),
            f(75, ty::BOOL, Bool(false), "hook line"),
            f(40, ty::F64, F64(0.0), "annotation height"),
            f(41, ty::F64, F64(0.0), "annotation width"),
            count(76, "vertex count"),
            item(10, ty::P3, Req, "vertex (WCS), the arrow at the first"),
            f(2185, ty::P3, P3(ORIGIN3), "leader plane origin (DWG)"),
            f(210, ty::P3, P3(Z), "extrusion"),
            f(211, ty::P3, P3(X), "horizontal direction"),
            f(212, ty::P3, P3(ORIGIN3), "offset of the last vertex from a block annotation"),
            f(213, ty::P3, P3(ORIGIN3), "offset of the last vertex from the annotation"),
            f(2186, ty::I16, I16(0), "DWG bits of unknown meaning: 1 the first, 2 and 4 the last two"),
            f(2187, ty::I16, I16(0), "DWG short of unknown meaning"),
            f(340, ty::HANDLE, Null, "annotation"),
            f(2105, ty::HANDLE, Null, "dimension style"),
        ],
    },
    KindSpec {
        kind: 41,
        name: "VIEWPORT",
        fields: &[
            f(10, ty::P3, Req, "centre on the paper"),
            f(40, ty::F64, Req, "width on the paper"),
            f(41, ty::F64, Req, "height on the paper"),
            f(69, ty::I16, Req, "viewport id: 1 the layout's own view"),
            f(17, ty::P3, P3(ORIGIN3), "view target"),
            f(16, ty::P3, P3(Z), "view direction"),
            f(51, ty::F64, F64(0.0), "twist angle, radians"),
            f(45, ty::F64, Req, "view height in model units"),
            f(42, ty::F64, F64(50.0), "lens length"),
            f(43, ty::F64, F64(0.0), "front clip"),
            f(44, ty::F64, F64(0.0), "back clip"),
            f(50, ty::F64, F64(0.0), "snap angle, radians"),
            f(12, ty::P2, Req, "view centre (model)"),
            f(13, ty::P2, P2(ORIGIN2), "snap base"),
            f(14, ty::P2, Req, "snap spacing"),
            f(15, ty::P2, Req, "grid spacing"),
            f(72, ty::I16, I16(1000), "circle zoom percent"),
            f(61, ty::I16, I16(5), "major grid lines"),
            f(90, ty::I32, Req, "status flags (DXF 90; 0x20000 off)"),
            f(1, ty::STR, Str(""), "plot style sheet"),
            f(281, ty::I16, I16(0), "render mode"),
            f(74, ty::BOOL, Bool(true), "UCS icon at origin"),
            f(71, ty::BOOL, Bool(true), "UCS per viewport"),
            f(110, ty::P3, P3(ORIGIN3), "UCS origin"),
            f(111, ty::P3, P3(X), "UCS X axis"),
            f(112, ty::P3, P3(Y), "UCS Y axis"),
            f(146, ty::F64, F64(0.0), "UCS elevation"),
            f(79, ty::I16, I16(0), "UCS orthographic type"),
            f(170, ty::I16, I16(0), "shade plot mode"),
            f(292, ty::BOOL, Bool(true), "default lighting"),
            f(282, ty::I16, I16(1), "lighting type"),
            f(141, ty::F64, F64(0.0), "brightness"),
            f(142, ty::F64, F64(0.0), "contrast"),
            f(2144, ty::I32, I32(0xC300_00FAu32 as i32), "ambient colour as a color method word"),
            count(2111, "frozen layer count"),
            item(331, ty::HANDLE, Req, "layer frozen in this viewport"),
            f(340, ty::HANDLE, Null, "clip boundary"),
            f(345, ty::HANDLE, Null, "named UCS"),
            f(346, ty::HANDLE, Null, "base UCS"),
            f(332, ty::HANDLE, Null, "background"),
            f(348, ty::HANDLE, Null, "visual style"),
            f(333, ty::HANDLE, Null, "shade plot"),
            f(361, ty::HANDLE, Null, "sun"),
        ],
    },
];

/// The description of one kind.
pub fn kind(k: i16) -> Option<&'static KindSpec> {
    KINDS.iter().find(|s| s.kind == k)
}

/// Group codes of the `Schema` section's own records.
pub mod code {
    pub const KIND: u16 = 2001;
    pub const NAME: u16 = 2;
    pub const FIELD_CODE: u16 = 2800;
    pub const FIELD_TYPE: u16 = 2801;
    pub const FIELD_FLAGS: u16 = 2802;
    pub const FIELD_NAME: u16 = 2803;
    pub const FIELD_DEFAULT: u16 = 2804;
}

/// Field flags as stored: how the field appears.
pub mod flag {
    pub const REQUIRED: i16 = 1;
    pub const DEFAULT: i16 = 2;
    pub const NULL_ABSENT: i16 = 4;
    pub const CONDITIONAL: i16 = 8;
    pub const COUNT: i16 = 16;
    pub const ITEM: i16 = 32;
}

fn field_flags(fd: &Field) -> i16 {
    let mut fl = match fd.def {
        Req => flag::REQUIRED,
        Null => flag::NULL_ABSENT,
        Cond => flag::CONDITIONAL,
        _ => flag::DEFAULT,
    };
    fl |= match fd.role {
        Role::Plain => 0,
        Role::Count => flag::COUNT,
        Role::Item => flag::ITEM,
    };
    fl
}

/// One record per kind (the common parts first): 2001 kind, 2 name, then
/// per field 2800 code, 2801 type, 2802 flags, 2803 name and, for a field
/// with a default, 2804 the default in the field's own type.
pub fn write_section() -> Vec<Vec<u8>> {
    KINDS
        .iter()
        .map(|k| {
            let mut w = TagWriter::new();
            w.i16(code::KIND, k.kind);
            w.str(code::NAME, k.name);
            for fd in k.fields {
                w.u16(code::FIELD_CODE, fd.code);
                w.i16(code::FIELD_TYPE, fd.ty as i16);
                w.i16(code::FIELD_FLAGS, field_flags(fd));
                w.str(code::FIELD_NAME, fd.name);
                match fd.def {
                    F64(v) => w.f64(code::FIELD_DEFAULT, v),
                    I16(v) => w.i16(code::FIELD_DEFAULT, v),
                    I32(v) => w.i32(code::FIELD_DEFAULT, v),
                    Bool(v) => w.bool(code::FIELD_DEFAULT, v),
                    Str(v) => w.str(code::FIELD_DEFAULT, v),
                    P2(v) => w.p2(code::FIELD_DEFAULT, crate::geom::Vec2::new(v[0], v[1])),
                    P3(v) => w.p3(
                        code::FIELD_DEFAULT,
                        crate::geom::Vec3::new(v[0], v[1], v[2]),
                    ),
                    Req | Null | Cond => {}
                }
            }
            w.buf
        })
        .collect()
}

/// A field as a file describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct FileField {
    pub code: u16,
    pub ty: u8,
    pub flags: i16,
    pub name: String,
    pub default: Option<super::tags::Value>,
}

/// A kind as a file describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct FileKind {
    pub kind: i16,
    pub name: String,
    pub fields: Vec<FileField>,
}

/// Read one record of a `Schema` section.
pub fn read_record(data: &[u8]) -> crate::error::Result<FileKind> {
    use super::tags::TagReader;
    let mut r = TagReader::new(data);
    let kind = r.i16(code::KIND)?;
    let name = r.str(code::NAME)?;
    let mut fields = Vec::new();
    while r.peek_code()? == Some(code::FIELD_CODE) {
        let fcode = r.u16(code::FIELD_CODE)?;
        let fty = r.i16(code::FIELD_TYPE)?;
        let flags = r.i16(code::FIELD_FLAGS)?;
        let fname = r.str(code::FIELD_NAME)?;
        let default = r.opt(code::FIELD_DEFAULT)?;
        fields.push(FileField {
            code: fcode,
            ty: fty as u8,
            flags,
            name: fname,
            default,
        });
    }
    Ok(FileKind { kind, name, fields })
}

/// What this version would write for a kind, in the file's terms (to
/// compare a file's schema with ours).
pub fn as_file_kind(k: &KindSpec) -> FileKind {
    let rec = write_section()
        .into_iter()
        .zip(KINDS)
        .find(|(_, s)| s.kind == k.kind)
        .map(|(r, _)| r)
        .unwrap_or_default();
    read_record(&rec).unwrap_or(FileKind {
        kind: k.kind,
        name: k.name.to_string(),
        fields: Vec::new(),
    })
}

/// The name a schema gives a group code within a kind (the common parts
/// are searched too), for dumps.
pub fn field_name(kinds: &[FileKind], kind: Option<i16>, code: u16) -> Option<&str> {
    let order = [kind, Some(-2), Some(-1)];
    order.iter().flatten().find_map(|k| {
        kinds
            .iter()
            .find(|s| s.kind == *k)?
            .fields
            .iter()
            .find(|f| f.code == code)
            .map(|f| f.name.as_str())
    })
}

/// The name FORMAT.md uses for a value type.
fn type_label(t: u8) -> String {
    match t {
        ty::STR => "STR".into(),
        ty::F64 => "F64".into(),
        ty::I16 => "I16".into(),
        ty::I32 => "I32".into(),
        ty::I64 => "I64".into(),
        ty::BOOL => "BOOL".into(),
        ty::HANDLE => "HANDLE".into(),
        ty::BYTES => "BYTES".into(),
        ty::P2 => "P2".into(),
        ty::P3 => "P3".into(),
        other => format!("type {other:#04x}"),
    }
}

fn number(v: f64) -> String {
    if v == TAU {
        "2π".into()
    } else {
        format!("{v}")
    }
}

/// One field as FORMAT.md lists it: code, type, meaning, and how it may be
/// absent.
fn describe(fd: &Field) -> String {
    let mut s = String::new();
    if fd.role == Role::Item {
        s.push_str("each: ");
    }
    s.push_str(&format!("{} {} {}", fd.code, type_label(fd.ty), fd.name));
    let default = match fd.def {
        Req => None,
        Null => Some("absent when null".to_string()),
        Cond => Some("conditional".to_string()),
        F64(v) => Some(format!("default {}", number(v))),
        I16(v) => Some(format!("default {v}")),
        I32(v) if !(-0xFFFF..=0xFFFF).contains(&v) => Some(format!("default {:#X}", v as u32)),
        I32(v) => Some(format!("default {v}")),
        Bool(v) => Some(format!("default {v}")),
        Str("") => Some("default empty".to_string()),
        Str(v) => Some(format!("default {v:?}")),
        P2(v) => Some(format!("default ({}, {})", number(v[0]), number(v[1]))),
        P3(v) => Some(format!(
            "default ({}, {}, {})",
            number(v[0]),
            number(v[1]),
            number(v[2])
        )),
    };
    if let Some(d) = default {
        s.push_str(&format!(" *[{d}]*"));
    }
    s
}

/// The schema as the Markdown table in FORMAT.md §8.2 (`cad schema`
/// prints it; a test checks the document has it word for word).
pub fn markdown_table() -> String {
    let mut out = String::from("| Kind | Name | Fields, in order |\n|---|---|---|\n");
    for k in KINDS {
        let fields: Vec<String> = k.fields.iter().map(describe).collect();
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            k.kind,
            k.name,
            if fields.is_empty() {
                "—".to_string()
            } else {
                fields.join("; ").replace('|', "\\|")
            }
        ));
    }
    out
}

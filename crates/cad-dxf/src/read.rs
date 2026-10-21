//! Reading a DXF file (ASCII or binary, AC1015 and later) into the drawing
//! database: the mirror of [`super::write`].
//!
//! Every group the writer produces is read back into the model field it
//! came from, so that a DXF this program wrote reads back into a drawing
//! that writes the same DXF again, byte for byte (`tests/corpus.rs`).
//! Group meanings are those of the Autodesk DXF reference. Handles are
//! kept as the file gives them; names in entity records (layer, linetype,
//! text style, block, dimension style) are resolved to the table records
//! of the same name.
//!
//! What DXF does not carry is filled in the way the DWG reader would find
//! it in a drawing AutoCAD made: the DWG flag words, entity modes, control
//! object lists and the header's handle variables are rebuilt from the
//! records. Objects of types the model does not hold (visual styles,
//! materials and the like in AutoCAD's own DXF files) are left out, with a
//! note.

use super::encode::{self, Group, Value, BINARY_SENTINEL};
use super::write::{codepage_name, deg, Src, HEADER};
use crate::dwg::nongraphic::{xr_type, XrType};
use crate::dwg::DwgVersion;
use crate::error::{Error, Result};
use crate::geom::{Vec2, Vec3};
use crate::model::*;
use std::collections::{BTreeMap, HashMap};

/// True when the bytes look like a DXF file (binary, or ASCII starting with
/// a section or a comment).
pub fn looks_like_dxf(bytes: &[u8]) -> bool {
    if bytes.starts_with(BINARY_SENTINEL) {
        return true;
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]);
    let mut lines = head.trim_start_matches('\u{feff}').lines().map(str::trim);
    matches!(
        (lines.next(), lines.next()),
        (Some("0"), Some("SECTION")) | (Some("999"), _)
    )
}

/// Read a DXF file from memory.
pub fn read_bytes(bytes: &[u8]) -> Result<Drawing> {
    let groups = if bytes.starts_with(BINARY_SENTINEL) {
        encode::from_binary(bytes)?
    } else {
        encode::from_ascii(bytes)?
    };
    Reader {
        d: empty_drawing(),
        vars: Vec::new(),
        names: HashMap::new(),
        class_numbers: HashMap::new(),
        owned: BTreeMap::new(),
        table_records: BTreeMap::new(),
        notes: Vec::new(),
        skipped: BTreeMap::new(),
        fresh: 1,
        entities_seen: 0,
    }
    .read(&groups)
}

/// Read a DXF file from disk.
pub fn read_file(path: impl AsRef<std::path::Path>) -> Result<Drawing> {
    read_bytes(&std::fs::read(path.as_ref())?)
}

// ----- values ---------------------------------------------------------------

fn vs(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Handle(h) => format!("{h:X}"),
        Value::Int(i) => i.to_string(),
        Value::Bool(b) => (*b as i64).to_string(),
        Value::Real(r) => encode::real_text(*r),
        Value::Bytes(b) => b.iter().map(|x| format!("{x:02X}")).collect(),
    }
}
fn vf(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Int(i) => *i as f64,
        Value::Bool(b) => *b as i64 as f64,
        Value::Str(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}
fn vi(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        Value::Bool(b) => *b as i64,
        Value::Real(r) => *r as i64,
        Value::Str(s) => s.trim().parse().unwrap_or(0),
        Value::Handle(h) => *h as i64,
        Value::Bytes(_) => 0,
    }
}
fn vh(v: &Value) -> Handle {
    match v {
        Value::Handle(h) => Handle(*h),
        Value::Str(s) => Handle(u64::from_str_radix(s.trim(), 16).unwrap_or(0)),
        Value::Int(i) => Handle(*i as u64),
        _ => Handle::NULL,
    }
}

/// Radians for a DXF angle in degrees, chosen so that the writer gives
/// the same degrees back (the conversion can be a unit in the last place
/// off either way).
fn rad(d: f64) -> f64 {
    let r = d.to_radians();
    if deg(r) == d {
        return r;
    }
    let mut lo = r;
    let mut hi = r;
    for _ in 0..8 {
        lo = next_down(lo);
        hi = next_up(hi);
        if deg(hi) == d {
            return hi;
        }
        if deg(lo) == d {
            return lo;
        }
    }
    r
}
fn next_up(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x == 0.0 {
        return f64::from_bits(1);
    }
    let b = x.to_bits();
    f64::from_bits(if x > 0.0 { b + 1 } else { b - 1 })
}
fn next_down(x: f64) -> f64 {
    -next_up(-x)
}

/// The groups of one record (after its `0` group), with accessors.
#[derive(Clone, Copy)]
struct G<'a>(&'a [Group]);

impl<'a> G<'a> {
    fn get(&self, code: u16) -> Option<&'a Value> {
        self.0.iter().find(|g| g.code == code).map(|g| &g.value)
    }
    fn s(&self, code: u16) -> Option<String> {
        self.get(code).map(vs)
    }
    fn s_or(&self, code: u16, d: &str) -> String {
        self.s(code).unwrap_or_else(|| d.to_string())
    }
    fn f(&self, code: u16, d: f64) -> f64 {
        self.get(code).map(vf).unwrap_or(d)
    }
    fn i(&self, code: u16, d: i64) -> i64 {
        self.get(code).map(vi).unwrap_or(d)
    }
    fn h(&self, code: u16) -> Handle {
        self.get(code).map(vh).unwrap_or(Handle::NULL)
    }
    fn a(&self, code: u16, d: f64) -> f64 {
        self.get(code).map(|v| rad(vf(v))).unwrap_or(d)
    }
    fn p2(&self, code: u16, d: Vec2) -> Vec2 {
        if self.get(code).is_none() {
            return d;
        }
        Vec2::new(self.f(code, 0.0), self.f(code + 10, 0.0))
    }
    fn p3(&self, code: u16, d: Vec3) -> Vec3 {
        if self.get(code).is_none() {
            return d;
        }
        Vec3::new(self.f(code, 0.0), self.f(code + 10, 0.0), self.f(code + 20, 0.0))
    }
    /// The groups after `100 marker` up to the next 100.
    fn sub(&self, marker: &str) -> G<'a> {
        let g = self.0;
        // A record with no subclass markers at all (R12 style, as simple
        // exporters write) is one flat list of groups.
        if !g.iter().any(|x| x.code == 100) {
            return G(g);
        }
        match g.iter().position(|x| x.code == 100 && vs(&x.value) == marker) {
            Some(i) => {
                let rest = &g[i + 1..];
                let end = rest.iter().position(|x| x.code == 100).unwrap_or(rest.len());
                G(&rest[..end])
            }
            None => G(&[]),
        }
    }
    fn has_marker(&self, marker: &str) -> bool {
        self.0.iter().any(|x| x.code == 100 && vs(&x.value) == marker)
    }
}

/// A record split into the parts every record shares and its own groups.
struct Rec<'a> {
    kind: String,
    handle: Handle,
    owner: Handle,
    reactors: Vec<Handle>,
    xdictionary: Handle,
    body: Vec<Group>,
    xdata: &'a [Group],
}

/// Groups as typed items (the extension record and kept-object form): a
/// point's 10/20/30 (and the like) become one item.
fn typed_items(gs: &[Group]) -> Vec<(u16, XrValue)> {
    let mut items = Vec::new();
    let mut k = 0;
    while k < gs.len() {
        let x = &gs[k];
        let Some(t) = xr_type(x.code) else {
            k += 1;
            continue;
        };
        let v = match t {
            XrType::Str => XrValue::Str(vs(&x.value)),
            XrType::Point => {
                let y = gs.get(k + 1).filter(|n| n.code == x.code + 10).map(|n| vf(&n.value));
                let z = gs.get(k + 2).filter(|n| n.code == x.code + 20).map(|n| vf(&n.value));
                k += y.is_some() as usize + z.is_some() as usize;
                XrValue::Point(Vec3::new(vf(&x.value), y.unwrap_or(0.0), z.unwrap_or(0.0)))
            }
            XrType::Real => XrValue::Real(vf(&x.value)),
            XrType::Int8 => XrValue::Int8(vi(&x.value) as i8),
            XrType::Int16 => XrValue::Int16(vi(&x.value) as i16),
            XrType::Int32 => XrValue::Int32(vi(&x.value) as i32),
            XrType::Int64 => XrValue::Int64(vi(&x.value)),
            XrType::Bool => XrValue::Bool(vi(&x.value) != 0),
            XrType::Bytes => XrValue::Bytes(match &x.value {
                Value::Bytes(b) => b.clone(),
                _ => Vec::new(),
            }),
            XrType::Handle => XrValue::Handle(vh(&x.value)),
        };
        items.push((x.code, v));
        k += 1;
    }
    items
}

fn split_record<'a>(kind: &str, groups: &'a [Group]) -> Rec<'a> {
    let mut r = Rec {
        kind: kind.to_string(),
        handle: Handle::NULL,
        owner: Handle::NULL,
        reactors: Vec::new(),
        xdictionary: Handle::NULL,
        body: Vec::new(),
        xdata: &[],
    };
    let mut seen_marker = false;
    let mut owner_set = false;
    let mut i = 0;
    while i < groups.len() {
        let g = &groups[i];
        match g.code {
            1001 => {
                r.xdata = &groups[i..];
                break;
            }
            5 | 105 if r.handle.is_null() && !seen_marker => r.handle = vh(&g.value),
            102 if !seen_marker && vs(&g.value).starts_with('{') => {
                let tag = vs(&g.value);
                i += 1;
                while i < groups.len() && !(groups[i].code == 102 && vs(&groups[i].value) == "}") {
                    match (tag.as_str(), groups[i].code) {
                        ("{ACAD_XDICTIONARY", 360) => r.xdictionary = vh(&groups[i].value),
                        ("{ACAD_REACTORS", 330) => r.reactors.push(vh(&groups[i].value)),
                        _ => {}
                    }
                    i += 1;
                }
            }
            330 if !seen_marker && !owner_set => {
                r.owner = vh(&g.value);
                owner_set = true;
            }
            _ => {
                if g.code == 100 {
                    seen_marker = true;
                }
                r.body.push(g.clone());
            }
        }
        i += 1;
    }
    r
}

/// The records of a section: (type, groups after the `0` group).
fn records(groups: &[Group]) -> Vec<(String, &[Group])> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < groups.len() {
        if groups[i].code != 0 {
            i += 1;
            continue;
        }
        let kind = vs(&groups[i].value);
        let start = i + 1;
        let mut end = start;
        while end < groups.len() && groups[end].code != 0 {
            end += 1;
        }
        out.push((kind, &groups[start..end]));
        i = end;
    }
    out
}

/// DWG type numbers of the table controls and their records.
const TABLES: [(&str, u16, u16); 9] = [
    ("VPORT", 0x40, 0x41),
    ("LTYPE", 0x38, 0x39),
    ("LAYER", 0x32, 0x33),
    ("STYLE", 0x34, 0x35),
    ("VIEW", 0x3C, 0x3D),
    ("UCS", 0x3E, 0x3F),
    ("APPID", 0x42, 0x43),
    ("DIMSTYLE", 0x44, 0x45),
    ("BLOCK_RECORD", 0x30, 0x31),
];

/// The DWG header variable of each table's control object.
fn control_var(table: &str) -> &'static str {
    match table {
        "VPORT" => "VPORT_CONTROL",
        "LTYPE" => "LTYPE_CONTROL",
        "LAYER" => "LAYER_CONTROL",
        "STYLE" => "STYLE_CONTROL",
        "VIEW" => "VIEW_CONTROL",
        "UCS" => "UCS_CONTROL",
        "APPID" => "APPID_CONTROL",
        "DIMSTYLE" => "DIMSTYLE_CONTROL",
        _ => "BLOCK_CONTROL",
    }
}

/// The table a header variable given by name refers to.
fn name_table(var: &str) -> u16 {
    match var {
        "TEXTSTYLE" | "DIMTXSTY" => 0x35,
        "CLAYER" => 0x33,
        "CELTYPE" | "DIMLTYPE" | "DIMLTEX1" | "DIMLTEX2" => 0x39,
        "DIMSTYLE" => 0x45,
        "DIMBLK" | "DIMBLK1" | "DIMBLK2" | "DIMLDRBLK" => 0x31,
        _ => 0x3F,
    }
}

/// The DWG header variable of each entry of the named-object dictionary.
fn dictionary_var(key: &str) -> Option<&'static str> {
    Some(match key {
        "ACAD_GROUP" => "DICTIONARY_ACAD_GROUP",
        "ACAD_MLINESTYLE" => "DICTIONARY_ACAD_MLINESTYLE",
        "ACAD_LAYOUT" => "DICTIONARY_LAYOUTS",
        "ACAD_PLOTSETTINGS" => "DICTIONARY_PLOTSETTINGS",
        "ACAD_PLOTSTYLENAME" => "DICTIONARY_PLOTSTYLES",
        "ACAD_MATERIAL" => "DICTIONARY_MATERIALS",
        "ACAD_COLOR" => "DICTIONARY_COLORS",
        "ACAD_VISUALSTYLE" => "DICTIONARY_VISUALSTYLE",
        _ => return None,
    })
}

fn codepage_number(name: &str) -> u16 {
    if codepage_name(30).eq_ignore_ascii_case(name) {
        30
    } else {
        match name.to_ascii_uppercase().as_str() {
            "ANSI_874" => 26,
            "ANSI_932" => 27,
            "ANSI_936" => 28,
            "ANSI_949" => 29,
            "ANSI_950" => 30,
            "ANSI_1250" => 31,
            "ANSI_1251" => 32,
            _ => 30,
        }
    }
}

fn fmt_f(v: f64) -> String {
    format!("{v:?}")
}

fn empty_drawing() -> Drawing {
    Drawing {
        version: DwgVersion::R2018,
        maintenance_version: 0,
        codepage: 30,
        header: HeaderVars::default(),
        classes: Vec::new(),
        objects: ObjectMap::new(),
        sections: Vec::new(),
        diagnostics: Vec::new(),
        index: DrawingIndex::default(),
    }
}

struct Reader {
    d: Drawing,
    /// Header variables (DXF name → value groups), in file order.
    vars: Vec<(String, Vec<Group>)>,
    /// Table records by DWG type and upper-case name.
    names: HashMap<(u16, String), Handle>,
    /// Class numbers by DXF name.
    class_numbers: HashMap<String, u16>,
    /// Entity order per owner (block record, polyline, insert).
    owned: BTreeMap<Handle, Vec<Handle>>,
    /// Records of each table, in file order.
    table_records: BTreeMap<u16, Vec<Handle>>,
    notes: Vec<String>,
    skipped: BTreeMap<String, usize>,
    /// The next handle no record in the file has, for records the file
    /// gives none (simple exporters leave them out).
    fresh: u64,
    /// Entity records in the ENTITIES section.
    entities_seen: usize,
}

impl Reader {
    /// A record, with a handle of its own when the file gives it none.
    fn rec<'a>(&mut self, kind: &str, groups: &'a [Group]) -> Rec<'a> {
        let mut r = split_record(kind, groups);
        if r.handle.is_null() {
            r.handle = Handle(self.fresh);
            self.fresh += 1;
        }
        r
    }

    /// Run `f` on the drawing with new handles taken from above every
    /// handle in the file.
    fn making<T>(&mut self, f: impl FnOnce(&mut Drawing) -> T) -> T {
        let seed = self.d.header.handseed;
        self.d.header.handseed = Some(self.fresh.max(seed.unwrap_or(0)));
        let out = f(&mut self.d);
        self.fresh = self.fresh.max(self.d.header.handseed.unwrap_or(0));
        self.d.header.handseed = seed;
        out
    }

    /// What every drawing has, made when the file leaves it out (a DXF
    /// of entities alone, or without a block table): the layer table with
    /// layer `0`, and the block table with model and paper space.
    fn complete(&mut self) {
        let had_layer0 = self.d.layer_by_name("0").is_some();
        let had_ms = self.d.block_by_name("*Model_Space").is_some();
        let ms = self.making(|d| {
            d.ensure_layer_zero();
            d.ensure_model_space()
        });
        let made: Vec<&str> = [(had_layer0, "layer 0"), (had_ms, "model space")]
            .iter()
            .filter(|(had, _)| !had)
            .map(|(_, what)| *what)
            .collect();
        if !made.is_empty() {
            self.notes.push(format!("the file has no {}; made as a new drawing has it", made.join(" or ")));
        }
        // Names the entities can refer to.
        let found: Vec<((u16, String), Handle)> = self
            .d
            .objects
            .values()
            .filter_map(|o| match &o.kind {
                ObjectKind::Layer(l) => Some(((0x33, l.name.to_ascii_uppercase()), o.handle)),
                ObjectKind::BlockHeader(b) => Some(((0x31, b.name.to_ascii_uppercase()), o.handle)),
                _ => None,
            })
            .collect();
        for (k, h) in found {
            self.names.entry(k).or_insert(h);
        }
        self.d.header.model_space = ms;
    }

    /// The layer an entity names, made with default properties when the
    /// file does not define it, as CAD programs do.
    fn layer_for(&mut self, name: &str) -> Handle {
        let name = if name.is_empty() { "0" } else { name };
        let h = self.lookup(0x33, name);
        if !h.is_null() {
            return h;
        }
        let made = self.making(|d| d.add_layer(name, Color::Index(7), Handle::NULL, Lineweight::Default));
        match made {
            Ok(h) => {
                self.names.insert((0x33, name.to_ascii_uppercase()), h);
                self.notes.push(format!("layer {name} is named but not defined; made with default properties"));
                h
            }
            Err(_) => Handle::NULL,
        }
    }

    fn read(mut self, groups: &[Group]) -> Result<Drawing> {
        // Sections.
        let mut sections: Vec<(String, &[Group])> = Vec::new();
        let mut i = 0;
        while i < groups.len() {
            let g = &groups[i];
            if g.code == 0 && vs(&g.value) == "EOF" {
                break;
            }
            if g.code == 0 && vs(&g.value) == "SECTION" {
                let name = groups.get(i + 1).map(|x| vs(&x.value)).unwrap_or_default();
                // A file cut off just after SECTION has no name or body.
                let start = (i + 2).min(groups.len());
                let mut end = start;
                while end < groups.len() && !(groups[end].code == 0 && vs(&groups[end].value) == "ENDSEC") {
                    end += 1;
                }
                sections.push((name, &groups[start..end.min(groups.len())]));
                i = end + 1;
            } else {
                i += 1;
            }
        }
        if sections.is_empty() {
            return Err(Error::format("not a DXF file: no sections"));
        }
        // New handles go above every handle the file gives.
        self.fresh = groups
            .iter()
            .filter(|g| g.code == 5 || g.code == 105)
            .filter_map(|g| u64::from_str_radix(vs(&g.value).trim(), 16).ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(1);
        self.d.version = DwgVersion::R2018;
        self.d.codepage = 30;
        self.d.header.verified = true;
        let get = |n: &str| sections.iter().find(|(s, _)| s == n).map(|(_, g)| *g);
        if let Some(h) = get("HEADER") {
            self.header(h)?;
        }
        if let Some(c) = get("CLASSES") {
            self.classes(c);
        }
        if let Some(t) = get("TABLES") {
            self.tables(t)?;
        }
        self.complete();
        if let Some(b) = get("BLOCKS") {
            self.entities(b, true)?;
        }
        if let Some(e) = get("ENTITIES") {
            self.entities(e, false)?;
        }
        if let Some(o) = get("OBJECTS") {
            self.objects(o)?;
        }
        self.finish();
        // Entity records found, none placed: say so, rather than open an
        // empty drawing as though the file were empty.
        let placed = self
            .d
            .objects
            .values()
            .filter(|o| o.entity.as_ref().is_some_and(|e| e.space != Space::Owned))
            .filter(|o| !matches!(o.kind, ObjectKind::BlockBegin { .. } | ObjectKind::BlockEnd))
            .count();
        if self.entities_seen > 0 && placed == 0 {
            self.d.diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!(
                    "{} found in the ENTITIES section, none placed in model or paper space{}",
                    if self.entities_seen == 1 { "1 entity".to_string() } else { format!("{} entities", self.entities_seen) },
                    if self.skipped.is_empty() { String::new() } else { " (see the note on what was left out)".into() }
                ),
            });
        }
        if !self.skipped.is_empty() {
            let list: Vec<String> = self.skipped.iter().map(|(t, n)| format!("{t} ×{n}")).collect();
            self.notes.push(format!("left out (not held by the model): {}", list.join(", ")));
        }
        for n in std::mem::take(&mut self.notes) {
            self.d.diagnostics.push(Diagnostic {
                severity: Severity::Info,
                message: n,
            });
        }
        self.d.rebuild_index();
        Ok(self.d)
    }

    fn raw(&mut self, name: &str, value: String) {
        let raw = &mut self.d.header.raw;
        match raw.iter_mut().find(|(k, _)| k == name) {
            Some(e) => e.1 = value,
            None => raw.push((name.to_string(), value)),
        }
    }

    // ----- header -------------------------------------------------------------

    fn header(&mut self, groups: &[Group]) -> Result<()> {
        let mut cur: Option<(String, Vec<Group>)> = None;
        for g in groups {
            if g.code == 9 {
                if let Some(v) = cur.take() {
                    self.vars.push(v);
                }
                cur = Some((vs(&g.value), Vec::new()));
            } else if let Some((_, v)) = &mut cur {
                v.push(g.clone());
            }
        }
        if let Some(v) = cur.take() {
            self.vars.push(v);
        }
        let mut flags: i64 = 0x2A1D;
        let vars = std::mem::take(&mut self.vars);
        for (name, vals) in &vars {
            let g = G(vals);
            if name == "$ACADVER" {
                let v = g.s_or(1, "");
                if let Some(ver) = DwgVersion::from_magic(v.as_bytes()) {
                    self.d.version = ver;
                }
                continue;
            }
            let Some(&(_, code, n, src, _)) = HEADER.iter().find(|e| e.0 == name) else { continue };
            let first = || vals.first().map(|x| &x.value);
            match src {
                Src::Raw(dwg) => {
                    let text = match n {
                        2 => {
                            let p = g.p2(code, Vec2::ZERO);
                            format!("({}, {})", fmt_f(p.x), fmt_f(p.y))
                        }
                        3 => {
                            let p = g.p3(code, Vec3::ZERO);
                            format!("({}, {}, {})", fmt_f(p.x), fmt_f(p.y), fmt_f(p.z))
                        }
                        _ => match (encode::kind(code), first()) {
                            (_, None) => continue,
                            (encode::Kind::Real, Some(v)) => fmt_f(vf(v)),
                            (encode::Kind::Str, Some(v)) => format!("{:?}", vs(v)),
                            (_, Some(v)) => vi(v).to_string(),
                        },
                    };
                    self.raw(dwg, text);
                }
                Src::Name(dwg) => {
                    let v = first().map(vs).unwrap_or_default();
                    self.raw(&format!("@NAME:{dwg}"), v);
                }
                Src::Color(dwg, _) => {
                    let c = first().map(vi).unwrap_or(256);
                    let t = match c {
                        0 => "BYBLOCK".to_string(),
                        256 => "BYLAYER".to_string(),
                        257 => "NONE".to_string(),
                        v => v.to_string(),
                    };
                    self.raw(dwg, t);
                }
                Src::Time(dwg) => {
                    let v = first().map(vf).unwrap_or(0.0);
                    self.raw(dwg, fmt_f(v));
                }
                Src::Flags(bit) => {
                    let v = first().map(vi).unwrap_or(0);
                    let set = |flags: &mut i64, mask: i64, on: bool| {
                        if on {
                            *flags |= mask
                        } else {
                            *flags &= !mask
                        }
                    };
                    match bit {
                        0 => flags = (flags & !0x1F) | Lineweight::from_dxf(v as i16).code() as i64,
                        1 => flags = (flags & !(3 << 5)) | ((v & 3) << 5),
                        2 => flags = (flags & !(3 << 7)) | ((v & 3) << 7),
                        3 => set(&mut flags, 0x200, v == 0),
                        4 => set(&mut flags, 0x400, v == 0),
                        5 => set(&mut flags, 0x800, v != 0),
                        6 => set(&mut flags, 0x2000, v != 0),
                        _ => set(&mut flags, 0x4000, v != 0),
                    }
                }
                Src::Object(dwg) => {
                    let h = first().map(vh).unwrap_or(Handle::NULL);
                    self.raw(dwg, format!("#{:X}", h.0));
                }
                Src::Maint => self.d.maintenance_version = first().map(vi).unwrap_or(0) as u8,
                Src::Codepage => self.d.codepage = codepage_number(&first().map(vs).unwrap_or_default()),
                Src::LastSavedBy => self.d.header.properties.last_saved_by = first().map(vs).unwrap_or_default(),
                Src::Handseed => self.d.header.handseed = first().map(|v| vh(v).0),
                Src::MlineStyle => {
                    let v = first().map(vs).unwrap_or_default();
                    self.raw("@NAME:CMLSTYLE", v);
                }
            }
        }
        self.raw("FLAGS", flags.to_string());
        self.vars = vars;
        Ok(())
    }

    // ----- classes ------------------------------------------------------------

    fn classes(&mut self, groups: &[Group]) {
        for (i, (_, g)) in records(groups).into_iter().enumerate() {
            let g = G(g);
            let c = ClassDef {
                number: 500 + i as u16,
                dxf_name: g.s_or(1, ""),
                cpp_name: g.s_or(2, ""),
                app_name: g.s_or(3, ""),
                proxy_flags: g.i(90, 0) as u16,
                instance_count: g.i(91, 0) as u32,
                was_zombie: g.i(280, 0) != 0,
                item_class_id: if g.i(281, 0) != 0 { 0x1F2 } else { 0x1F3 },
                dwg_version: 0,
                maintenance_version: 0,
            };
            self.class_numbers.insert(c.dxf_name.to_ascii_uppercase(), c.number);
            self.d.classes.push(c);
        }
    }

    fn type_of(&self, name: &str) -> Option<(u16, String)> {
        let fixed = |code: u16| Some((code, crate::dwg::fixed_type_name(code)?.to_string()));
        match name {
            "DICTIONARY" => fixed(0x2A),
            "XRECORD" => fixed(0x4F),
            "LAYOUT" => fixed(0x52),
            "MLINESTYLE" => fixed(0x49),
            "LINE" => fixed(0x13),
            "CIRCLE" => fixed(0x12),
            "ARC" => fixed(0x11),
            "POINT" => fixed(0x1B),
            "ELLIPSE" => fixed(0x23),
            "LWPOLYLINE" => fixed(0x4D),
            "SEQEND" => fixed(0x06),
            "TEXT" => fixed(0x01),
            "ATTRIB" => fixed(0x02),
            "ATTDEF" => fixed(0x03),
            "MTEXT" => fixed(0x2C),
            "SOLID" => fixed(0x1F),
            "TRACE" => fixed(0x20),
            "3DFACE" => fixed(0x1C),
            "SPLINE" => fixed(0x24),
            "BLOCK" => fixed(0x04),
            "ENDBLK" => fixed(0x05),
            "HATCH" => fixed(0x4E),
            other => match self.class_numbers.get(&other.to_ascii_uppercase()) {
                Some(n) => Some((*n, other.to_string())),
                // Some types are classes in one release and fixed in
                // another (ACDBPLACEHOLDER).
                None => (0..500u16).find_map(|c| (crate::dwg::fixed_type_name(c)? == other).then(|| (c, other.to_string()))),
            },
        }
    }

    fn new_object(&self, r: &Rec, type_code: u16, type_name: &str, kind: ObjectKind) -> Object {
        Object {
            handle: r.handle,
            type_code,
            type_name: type_name.to_string(),
            owner: r.owner,
            reactors: r.reactors.clone(),
            xdictionary: r.xdictionary,
            entity: None,
            kind,
            offset: 0,
            size: 0,
            note: None,
            raw: None,
            xdata: self.xdata(r.xdata),
            unknown_tags: Vec::new(),
        }
    }

    fn insert(&mut self, o: Object) {
        if o.handle.is_null() {
            *self.skipped.entry(format!("{} without a handle", o.type_name)).or_default() += 1;
            return;
        }
        self.d.objects.insert(o.handle, o);
    }

    fn xdata(&self, groups: &[Group]) -> Vec<XData> {
        let mut out: Vec<XData> = Vec::new();
        let mut i = 0;
        while i < groups.len() {
            let g = &groups[i];
            let v = &g.value;
            let p3 = |i: usize| -> (Vec3, usize) {
                let mut p = Vec3::new(vf(v), 0.0, 0.0);
                let mut used = 0;
                for (k, axis) in [(1, 1), (2, 2)] {
                    if let Some(n) = groups.get(i + k) {
                        if n.code == g.code + 10 * axis {
                            if axis == 1 {
                                p.y = vf(&n.value);
                            } else {
                                p.z = vf(&n.value);
                            }
                            used = k;
                        }
                    }
                }
                (p, used)
            };
            let mut skip = 0;
            let item = match g.code {
                1001 => {
                    out.push(XData {
                        app: XDataApp::Name(vs(v)),
                        items: Vec::new(),
                    });
                    i += 1;
                    continue;
                }
                1000 => XValue::Str(vs(v)),
                1002 => {
                    if vs(v).trim() == "}" {
                        XValue::Close
                    } else {
                        XValue::Open
                    }
                }
                1003 => XValue::Layer(self.names.get(&(0x33, vs(v).to_ascii_uppercase())).copied().unwrap_or(Handle::NULL)),
                1004 => XValue::Bytes(match v {
                    Value::Bytes(b) => b.clone(),
                    _ => Vec::new(),
                }),
                1005 => XValue::Handle(vh(v)),
                1010..=1013 => {
                    let (p, used) = p3(i);
                    skip = used;
                    match g.code {
                        1010 => XValue::Point(p),
                        1011 => XValue::Position(p),
                        1012 => XValue::Displacement(p),
                        _ => XValue::Direction(p),
                    }
                }
                1040 => XValue::Real(vf(v)),
                1041 => XValue::Distance(vf(v)),
                1042 => XValue::ScaleFactor(vf(v)),
                1070 => XValue::Int16(vi(v) as i16),
                1071 => XValue::Int32(vi(v) as i32),
                _ => {
                    i += 1;
                    continue;
                }
            };
            if let Some(x) = out.last_mut() {
                x.items.push(item);
            }
            i += 1 + skip;
        }
        out
    }

    fn lookup(&self, table: u16, name: &str) -> Handle {
        self.names.get(&(table, name.to_ascii_uppercase())).copied().unwrap_or(Handle::NULL)
    }

    // ----- tables -------------------------------------------------------------

    fn tables(&mut self, groups: &[Group]) -> Result<()> {
        let recs = records(groups);
        let mut table: Option<(&str, u16, u16, Handle)> = None;
        for (kind, g) in recs {
            match kind.as_str() {
                "TABLE" => {
                    let r = self.rec(&kind, g);
                    let name = G(&r.body).s_or(2, "");
                    let Some(&(tname, ctl, rec)) = TABLES.iter().find(|t| t.0 == name) else {
                        table = None;
                        continue;
                    };
                    let o = self.new_object(
                        &r,
                        ctl,
                        crate::dwg::fixed_type_name(ctl).unwrap_or("CONTROL"),
                        ObjectKind::Control(Control::default()),
                    );
                    self.raw(control_var(tname), format!("#{:X}", r.handle.0));
                    table = Some((tname, ctl, rec, r.handle));
                    self.insert(o);
                }
                "ENDTAB" => table = None,
                _ => {
                    let Some((tname, _, rec, _)) = table else { continue };
                    if kind != tname {
                        continue;
                    }
                    let r = self.rec(&kind, g);
                    self.table_record(rec, &r)?;
                }
            }
        }
        // Control entries.
        let ctls: Vec<(u16, Handle)> = self
            .d
            .objects
            .values()
            .filter(|o| matches!(o.kind, ObjectKind::Control(_)))
            .map(|o| (o.type_code, o.handle))
            .collect();
        for (ctl, h) in ctls {
            let rec = TABLES.iter().find(|t| t.1 == ctl).map(|t| t.2).unwrap_or(0);
            let list = self.table_records.get(&rec).cloned().unwrap_or_default();
            let mut c = Control::default();
            for r in list {
                let name = self.d.record_name(r).unwrap_or("").to_ascii_uppercase();
                match (rec, name.as_str()) {
                    (0x39, "BYBLOCK") => self.raw("LTYPE_BYBLOCK", format!("#{:X}", r.0)),
                    (0x39, "BYLAYER") => self.raw("LTYPE_BYLAYER", format!("#{:X}", r.0)),
                    (0x31, "*MODEL_SPACE") => c.model_space = r,
                    (0x31, "*PAPER_SPACE") => c.paper_space = r,
                    _ => c.entries.push(r),
                }
                if rec == 0x39 && name == "CONTINUOUS" {
                    self.raw("LTYPE_CONTINUOUS", format!("#{:X}", r.0));
                }
            }
            if rec == 0x31 {
                self.raw("BLOCK_RECORD_MODEL_SPACE", format!("#{:X}", c.model_space.0));
                self.raw("BLOCK_RECORD_PAPER_SPACE", format!("#{:X}", c.paper_space.0));
                self.d.header.model_space = c.model_space;
                self.d.header.paper_space = c.paper_space;
            }
            if let Some(o) = self.d.objects.get_mut(&h) {
                o.kind = ObjectKind::Control(c);
            }
        }
        Ok(())
    }

    fn table_record(&mut self, rec: u16, r: &Rec) -> Result<()> {
        let g = G(&r.body);
        let name = g.s_or(2, "");
        let flags = g.i(70, 0);
        let kind = match rec {
            0x41 => {
                let mut v = Vport::looking_at(Vec2::ZERO, Vec2::new(1.0, 1.0));
                let b = g.sub("AcDbViewportTableRecord");
                v.name = name.clone();
                v.lower_left = b.p2(10, v.lower_left);
                v.upper_right = b.p2(11, v.upper_right);
                v.center = b.p2(12, v.center);
                v.snap_base = b.p2(13, v.snap_base);
                v.snap_spacing = b.p2(14, v.snap_spacing);
                v.grid_spacing = b.p2(15, v.grid_spacing);
                v.direction = b.p3(16, v.direction);
                v.target = b.p3(17, v.target);
                v.view_height = b.f(40, v.view_height);
                v.aspect = b.f(41, v.aspect);
                v.lens = b.f(42, v.lens);
                v.front_clip = b.f(43, v.front_clip);
                v.back_clip = b.f(44, v.back_clip);
                v.snap_rotation = b.a(50, v.snap_rotation);
                v.twist = b.a(51, v.twist);
                v.view_mode = b.i(71, v.view_mode as i64) as u16;
                v.circle_zoom = b.i(72, v.circle_zoom as i64) as u16;
                v.fast_zoom = b.i(73, v.fast_zoom as i64) != 0;
                v.ucsicon = b.i(74, v.ucsicon as i64) as u8;
                v.snap_on = b.i(75, v.snap_on as i64) != 0;
                v.grid_on = b.i(76, v.grid_on as i64) != 0;
                v.snap_style = b.i(77, v.snap_style as i64) != 0;
                v.snap_isopair = b.i(78, v.snap_isopair as i64) as u16;
                v.render_mode = b.i(281, v.render_mode as i64) as u8;
                v.ucs_per_viewport = b.i(65, v.ucs_per_viewport as i64) != 0;
                v.ucs_origin = b.p3(110, v.ucs_origin);
                v.ucs_x = b.p3(111, v.ucs_x);
                v.ucs_y = b.p3(112, v.ucs_y);
                v.ucs_ortho_type = b.i(79, v.ucs_ortho_type as i64) as u16;
                v.ucs_elevation = b.f(146, v.ucs_elevation);
                v.visual_style = b.h(348);
                v.grid_flags = b.i(60, v.grid_flags as i64) as u16;
                v.grid_major = b.i(61, v.grid_major as i64) as u16;
                v.default_lights = b.i(292, v.default_lights as i64) != 0;
                v.lighting_type = b.i(282, v.lighting_type as i64) as u8;
                v.brightness = b.f(141, v.brightness);
                v.contrast = b.f(142, v.contrast);
                if let Some(c) = b.get(63) {
                    v.ambient = Color::from_aci(vi(c) as i16);
                }
                if let Some(t) = b.get(421) {
                    let t = vi(t);
                    v.ambient = Color::Rgb((t >> 16) as u8, (t >> 8) as u8, t as u8);
                }
                v.background = b.h(332);
                v.sun = b.h(361);
                v.named_ucs = b.h(345);
                v.base_ucs = b.h(346);
                ObjectKind::Viewport(Box::new(v))
            }
            0x39 => {
                let b = g.sub("AcDbLinetypeTableRecord");
                let mut l = Linetype {
                    name: name.clone(),
                    description: b.s_or(3, ""),
                    flags: flags as u16,
                    pattern_length: b.f(40, 0.0),
                    elements: Vec::new(),
                };
                // Elements: each starts at a 49.
                let mut cur: Option<LinetypeElement> = None;
                for x in b.0 {
                    match x.code {
                        49 => {
                            if let Some(e) = cur.take() {
                                l.elements.push(e);
                            }
                            cur = Some(LinetypeElement::plain(vf(&x.value)));
                        }
                        74 => {
                            if let Some(e) = &mut cur {
                                e.complex = vi(&x.value) as u16;
                            }
                        }
                        75 => {
                            if let Some(e) = &mut cur {
                                e.shape = vi(&x.value) as u16;
                            }
                        }
                        340 => {
                            if let Some(e) = &mut cur {
                                e.style = vh(&x.value);
                            }
                        }
                        46 => {
                            if let Some(e) = &mut cur {
                                e.scale = vf(&x.value);
                            }
                        }
                        50 => {
                            if let Some(e) = &mut cur {
                                e.rotation = rad(vf(&x.value));
                            }
                        }
                        44 => {
                            if let Some(e) = &mut cur {
                                e.offset.x = vf(&x.value);
                            }
                        }
                        45 => {
                            if let Some(e) = &mut cur {
                                e.offset.y = vf(&x.value);
                            }
                        }
                        9 => {
                            if let Some(e) = &mut cur {
                                e.text = vs(&x.value);
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(e) = cur.take() {
                    l.elements.push(e);
                }
                ObjectKind::Linetype(l)
            }
            0x33 => {
                let b = g.sub("AcDbLayerTableRecord");
                let c = b.i(62, 7);
                let mut l = Layer {
                    name: name.clone(),
                    flags: if flags & 2 != 0 { 0x04 } else { 0 },
                    frozen: flags & 1 != 0,
                    off: c < 0,
                    locked: flags & 4 != 0,
                    plot: b.i(290, 1) != 0,
                    color: Color::from_aci(c.unsigned_abs() as i16),
                    linetype: Handle::NULL,
                    lineweight: Lineweight::from_dxf(b.i(370, -3) as i16).code(),
                };
                if let Some(t) = b.get(420) {
                    let t = vi(t);
                    l.color = Color::Rgb((t >> 16) as u8, (t >> 8) as u8, t as u8);
                }
                let lt = b.s_or(6, "Continuous");
                l.linetype = self.lookup(0x39, &lt);
                l.flags = l.dwg_flags();
                ObjectKind::Layer(l)
            }
            0x35 => {
                let b = g.sub("AcDbTextStyleTableRecord");
                ObjectKind::TextStyle(TextStyle {
                    name: name.clone(),
                    flags: flags as u16,
                    height: b.f(40, 0.0),
                    width_factor: b.f(41, 1.0),
                    oblique: b.a(50, 0.0),
                    generation: b.i(71, 0) as u16,
                    last_height: b.f(42, 0.0),
                    font_file: b.s_or(3, ""),
                    bigfont_file: b.s_or(4, ""),
                })
            }
            0x45 => {
                let b = g.sub("AcDbDimStyleTableRecord");
                ObjectKind::DimStyleRecord(Box::new(DimStyleRecord {
                    name: name.clone(),
                    vars: self.dim_vars(b),
                }))
            }
            0x31 => {
                let b = g.sub("AcDbBlockTableRecord");
                ObjectKind::BlockHeader(BlockHeader {
                    name: name.clone(),
                    anonymous: false,
                    has_attribs: false,
                    is_xref: false,
                    is_overlaid: false,
                    base_point: Vec3::ZERO,
                    xref_path: String::new(),
                    description: String::new(),
                    insert_units: b.i(70, 0) as u16,
                    block_entity: Handle::NULL,
                    entities: Vec::new(),
                    first_entity: Handle::NULL,
                    last_entity: Handle::NULL,
                    endblk: Handle::NULL,
                    layout: b.h(340),
                    part: Part::default(),
                })
            }
            _ => ObjectKind::NamedRecord(NamedRecord { name: name.clone() }),
        };
        let type_name = crate::dwg::fixed_type_name(rec).unwrap_or("RECORD");
        let o = self.new_object(r, rec, type_name, kind);
        self.names.insert((rec, name.to_ascii_uppercase()), r.handle);
        self.table_records.entry(rec).or_default().push(r.handle);
        self.insert(o);
        Ok(())
    }

    /// The dimension variables of a DIMSTYLE record; those DXF leaves out
    /// take the header's values.
    fn dim_vars(&self, b: G) -> DimVars {
        let mut tmp = empty_drawing();
        tmp.version = self.d.version;
        tmp.header.raw = self.d.header.raw.clone();
        let mut v = crate::dwg::nongraphic::dim_vars_from_header(&crate::dwg::hvars::Hv::new(&tmp));
        let f = |c: u16, d: f64| b.f(c, d);
        let i = |c: u16, d: u16| b.i(c, d as i64) as u16;
        let t = |c: u16, d: bool| b.i(c, d as i64) != 0;
        v.post = b.s_or(3, &v.post);
        v.apost = b.s_or(4, &v.apost);
        v.scale = f(40, v.scale);
        v.asz = f(41, v.asz);
        v.exo = f(42, v.exo);
        v.dli = f(43, v.dli);
        v.exe = f(44, v.exe);
        v.rnd = f(45, v.rnd);
        v.dle = f(46, v.dle);
        v.tp = f(47, v.tp);
        v.tm = f(48, v.tm);
        v.txt = f(140, v.txt);
        v.cen = f(141, v.cen);
        v.tsz = f(142, v.tsz);
        v.altf = f(143, v.altf);
        v.lfac = f(144, v.lfac);
        v.tvp = f(145, v.tvp);
        v.tfac = f(146, v.tfac);
        v.gap = f(147, v.gap);
        v.altrnd = f(148, v.altrnd);
        v.tol = t(71, v.tol);
        v.lim = t(72, v.lim);
        v.tih = t(73, v.tih);
        v.toh = t(74, v.toh);
        v.se1 = t(75, v.se1);
        v.se2 = t(76, v.se2);
        v.tad = i(77, v.tad);
        v.zin = i(78, v.zin);
        v.azin = i(79, v.azin);
        v.alt = t(170, v.alt);
        v.altd = i(171, v.altd);
        v.tofl = t(172, v.tofl);
        v.sah = t(173, v.sah);
        v.tix = t(174, v.tix);
        v.soxd = t(175, v.soxd);
        if let Some(c) = b.get(176) {
            v.clrd = Color::from_aci(vi(c) as i16);
        }
        if let Some(c) = b.get(177) {
            v.clre = Color::from_aci(vi(c) as i16);
        }
        if let Some(c) = b.get(178) {
            v.clrt = Color::from_aci(vi(c) as i16);
        }
        v.adec = i(179, v.adec);
        v.dec = i(271, v.dec);
        v.tdec = i(272, v.tdec);
        v.altu = i(273, v.altu);
        v.alttd = i(274, v.alttd);
        v.aunit = i(275, v.aunit);
        v.frac = i(276, v.frac);
        v.lunit = i(277, v.lunit);
        v.dsep = i(278, v.dsep);
        v.tmove = i(279, v.tmove);
        v.just = i(280, v.just);
        v.sd1 = t(281, v.sd1);
        v.sd2 = t(282, v.sd2);
        v.tolj = i(283, v.tolj);
        v.tzin = i(284, v.tzin);
        v.altz = i(285, v.altz);
        v.alttz = i(286, v.alttz);
        v.upt = t(288, v.upt);
        v.fit = i(289, v.fit);
        for (c, h) in [
            (340, &mut v.txsty),
            (341, &mut v.ldrblk),
            (342, &mut v.blk),
            (343, &mut v.blk1),
            (344, &mut v.blk2),
        ] {
            *h = b.h(c);
        }
        v.lwd = b.i(371, v.lwd as i64) as i16;
        v.lwe = b.i(372, v.lwe as i64) as i16;
        v
    }

    // ----- entities -----------------------------------------------------------

    fn entities(&mut self, groups: &[Group], in_blocks: bool) -> Result<()> {
        let recs = records(groups);
        let mut block: Handle = Handle::NULL;
        // The entity that owns the VERTEX, ATTRIB and SEQEND records that
        // follow it.
        let mut parent: Option<Handle> = None;
        for (kind, g) in recs {
            let mut r = self.rec(&kind, g);
            if !in_blocks {
                self.entities_seen += 1;
            }
            match kind.as_str() {
                "BLOCK" => {
                    block = r.owner;
                    parent = None;
                }
                "VERTEX" | "ATTRIB" | "SEQEND" => {}
                _ => parent = None,
            }
            if !in_blocks {
                let sub = matches!(kind.as_str(), "VERTEX" | "ATTRIB" | "SEQEND");
                match parent {
                    // Parts of a polyline or insert belong to it.
                    Some(p) if sub && r.owner.is_null() => r.owner = p,
                    // An entity with no owner, or one the drawing does not
                    // have, is in model space.
                    _ if !sub || parent.is_none() => {
                        let known = matches!(self.d.objects.get(&r.owner).map(|o| &o.kind), Some(ObjectKind::BlockHeader(_)));
                        if !known {
                            r.owner = self.d.header.model_space;
                        }
                    }
                    _ => {}
                }
            }
            let owner = if r.owner.is_null() { block } else { r.owner };
            let Some(o) = self.entity(&r, in_blocks, block)? else { continue };
            let h = o.handle;
            match kind.as_str() {
                "BLOCK" | "ENDBLK" => {}
                "VERTEX" | "ATTRIB" | "SEQEND" if parent.is_some() => {
                    self.owned.entry(parent.unwrap_or_default()).or_default().push(h);
                }
                _ => {
                    self.owned.entry(owner).or_default().push(h);
                }
            }
            if matches!(kind.as_str(), "POLYLINE" | "INSERT") {
                parent = Some(h);
            }
            if kind == "SEQEND" {
                parent = None;
            }
            if kind == "ENDBLK" {
                block = Handle::NULL;
            }
            self.insert(o);
        }
        Ok(())
    }

    fn entity(&mut self, r: &Rec, in_blocks: bool, block: Handle) -> Result<Option<Object>> {
        let body = G(&r.body);
        let common = body.sub("AcDbEntity");
        let kind_name = r.kind.as_str();
        let type_info = match kind_name {
            "POLYLINE" | "VERTEX" | "DIMENSION" | "INSERT" => Some((0, String::new())),
            other => self.type_of(other),
        };
        let Some((mut type_code, mut type_name)) = type_info else {
            *self.skipped.entry(kind_name.to_string()).or_default() += 1;
            return Ok(None);
        };
        let ms = self.d.header.model_space;
        let ps = self.d.header.paper_space;
        let owner = if r.owner.is_null() { block } else { r.owner };
        let mut e = EntityCommon {
            space: if owner == ms && !ms.is_null() {
                Space::Model
            } else if owner == ps && !ps.is_null() {
                Space::Paper
            } else {
                Space::Owned
            },
            layer: self.layer_for(&common.s_or(8, "0")),
            ..Default::default()
        };
        if let Some(lt) = common.s(6) {
            e.linetype = match lt.as_str() {
                "ByBlock" => LinetypeRef::ByBlock,
                "Continuous" => LinetypeRef::Continuous,
                "ByLayer" => LinetypeRef::ByLayer,
                name => LinetypeRef::Handle(self.lookup(0x39, name)),
            };
        }
        if let Some(c) = common.get(62) {
            e.color = match vi(c) {
                257 => Color::None,
                v => Color::from_aci(v as i16),
            };
        }
        if let Some(t) = common.get(420) {
            let t = vi(t);
            e.color = Color::Rgb((t >> 16) as u8, (t >> 8) as u8, t as u8);
        }
        if let Some(t) = common.get(440) {
            e.transparency = Some(vi(t) as u32);
        }
        e.plotstyle = common.h(390);
        e.material = common.h(347);
        if let Some(w) = common.get(370) {
            e.lineweight = Lineweight::from_dxf(vi(w) as i16).code();
        }
        e.linetype_scale = common.f(48, 1.0);
        e.invisible = common.i(60, 0) != 0;

        let style_of = |this: &Self, g: &G| -> Handle {
            this.lookup(0x35, &g.s_or(7, "Standard"))
        };
        let kind = match kind_name {
            "LINE" => {
                let b = body.sub("AcDbLine");
                ObjectKind::Line(Line {
                    start: b.p3(10, Vec3::ZERO),
                    end: b.p3(11, Vec3::ZERO),
                    thickness: b.f(39, 0.0),
                    extrusion: b.p3(210, Vec3::Z),
                })
            }
            "CIRCLE" => {
                let b = body.sub("AcDbCircle");
                ObjectKind::Circle(Circle {
                    center: b.p3(10, Vec3::ZERO),
                    radius: b.f(40, 0.0),
                    thickness: b.f(39, 0.0),
                    extrusion: b.p3(210, Vec3::Z),
                })
            }
            "ARC" => {
                let b = body.sub("AcDbCircle");
                let a = body.sub("AcDbArc");
                ObjectKind::Arc(Arc {
                    center: b.p3(10, Vec3::ZERO),
                    radius: b.f(40, 0.0),
                    thickness: b.f(39, 0.0),
                    extrusion: b.p3(210, Vec3::Z),
                    start_angle: a.a(50, 0.0),
                    end_angle: a.a(51, 0.0),
                })
            }
            "POINT" => {
                let b = body.sub("AcDbPoint");
                ObjectKind::Point(Point {
                    position: b.p3(10, Vec3::ZERO),
                    thickness: b.f(39, 0.0),
                    extrusion: b.p3(210, Vec3::Z),
                    x_axis_angle: b.a(50, 0.0),
                })
            }
            "ELLIPSE" => {
                let b = body.sub("AcDbEllipse");
                ObjectKind::Ellipse(Ellipse {
                    center: b.p3(10, Vec3::ZERO),
                    major_axis: b.p3(11, Vec3::X),
                    extrusion: b.p3(210, Vec3::Z),
                    axis_ratio: b.f(40, 1.0),
                    start_param: b.f(41, 0.0),
                    end_param: b.f(42, std::f64::consts::TAU),
                })
            }
            "LWPOLYLINE" => {
                let b = body.sub("AcDbPolyline");
                let f = b.i(70, 0);
                let mut p = LwPolyline {
                    flags: if f & 1 != 0 { LwPolyline::FLAG_CLOSED } else { 0 } | if f & 128 != 0 { 0x100 } else { 0 },
                    const_width: b.f(43, 0.0),
                    elevation: b.f(38, 0.0),
                    thickness: b.f(39, 0.0),
                    extrusion: b.p3(210, Vec3::Z),
                    ..Default::default()
                };
                let (mut bulges, mut widths, mut ids) = (Vec::new(), Vec::new(), Vec::new());
                let (mut any_b, mut any_w, mut any_i) = (false, false, false);
                let gs = b.0;
                let mut k = 0;
                while k < gs.len() {
                    let x = &gs[k];
                    match x.code {
                        10 => {
                            let y = gs.get(k + 1).filter(|n| n.code == 20).map(|n| vf(&n.value)).unwrap_or(0.0);
                            p.points.push(Vec2::new(vf(&x.value), y));
                            bulges.push(0.0);
                            widths.push((0.0, 0.0));
                            ids.push(0u32);
                        }
                        40 => {
                            if let Some(w) = widths.last_mut() {
                                w.0 = vf(&x.value);
                                any_w = true;
                            }
                        }
                        41 => {
                            if let Some(w) = widths.last_mut() {
                                w.1 = vf(&x.value);
                                any_w = true;
                            }
                        }
                        42 => {
                            if let Some(bv) = bulges.last_mut() {
                                *bv = vf(&x.value);
                                any_b = true;
                            }
                        }
                        91 => {
                            if let Some(id) = ids.last_mut() {
                                *id = vi(&x.value) as u32;
                                any_i = true;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                if any_b {
                    p.bulges = bulges;
                }
                if any_w {
                    p.widths = widths;
                }
                if any_i {
                    p.vertex_ids = ids;
                }
                ObjectKind::LwPolyline(p)
            }
            "POLYLINE" => {
                let (kind, sub) = if body.has_marker("AcDb3dPolyline") {
                    (PolylineKind::Polyline3d, "AcDb3dPolyline")
                } else if body.has_marker("AcDbPolyFaceMesh") {
                    (PolylineKind::PolyfaceMesh, "AcDbPolyFaceMesh")
                } else if body.has_marker("AcDbPolygonMesh") {
                    (PolylineKind::PolygonMesh, "AcDbPolygonMesh")
                } else {
                    (PolylineKind::Polyline2d, "AcDb2dPolyline")
                };
                let b = body.sub(sub);
                type_code = match kind {
                    PolylineKind::Polyline2d => 0x0F,
                    PolylineKind::Polyline3d => 0x10,
                    PolylineKind::PolyfaceMesh => 0x1D,
                    PolylineKind::PolygonMesh => 0x1E,
                };
                ObjectKind::Polyline(Polyline {
                    kind,
                    flags: b.i(70, 0) as u16,
                    curve_type: b.i(75, 0) as u16,
                    start_width: b.f(40, 0.0),
                    end_width: b.f(41, 0.0),
                    thickness: b.f(39, 0.0),
                    elevation: b.p3(10, Vec3::ZERO).z,
                    extrusion: b.p3(210, Vec3::Z),
                    ..Default::default()
                })
            }
            "VERTEX" => {
                let (kind, sub, bits, code) = if body.has_marker("AcDb3dPolylineVertex") {
                    (VertexKind::Vertex3d, "AcDb3dPolylineVertex", 32, 0x0B)
                } else if body.has_marker("AcDbPolygonMeshVertex") {
                    (VertexKind::Mesh, "AcDbPolygonMeshVertex", 64, 0x0C)
                } else if body.has_marker("AcDbPolyFaceMeshVertex") {
                    (VertexKind::PfaceVertex, "AcDbPolyFaceMeshVertex", 192, 0x0D)
                } else if body.has_marker("AcDbFaceRecord") {
                    (VertexKind::PfaceFace, "AcDbFaceRecord", 128, 0x0E)
                } else {
                    (VertexKind::Vertex2d, "AcDb2dVertex", 0, 0x0A)
                };
                type_code = code;
                let b = body.sub(sub);
                let mut face = [0i16; 4];
                for (j, c) in (71..75).enumerate() {
                    face[j] = b.i(c, 0) as i16;
                }
                ObjectKind::Vertex(Vertex {
                    kind,
                    flags: (b.i(70, 0) & !bits) as u8,
                    point: b.p3(10, Vec3::ZERO),
                    start_width: b.f(40, 0.0),
                    end_width: b.f(41, 0.0),
                    bulge: b.f(42, 0.0),
                    tangent_dir: b.a(50, 0.0),
                    id: b.i(91, 0) as u32,
                    face,
                })
            }
            "SEQEND" => ObjectKind::SeqEnd,
            "TEXT" => {
                let b = body.sub("AcDbText");
                let mut t = self.text(&b, style_of(self, &b));
                // The second AcDbText part holds the vertical alignment.
                let mut parts = r.body.iter().enumerate().filter(|(_, x)| x.code == 100 && vs(&x.value) == "AcDbText");
                let _ = parts.next();
                if let Some((j, _)) = parts.next() {
                    let rest = &r.body[j + 1..];
                    let end = rest.iter().position(|x| x.code == 100).unwrap_or(rest.len());
                    t.v_align = G(&rest[..end]).i(73, 0) as u16;
                }
                ObjectKind::Text(t)
            }
            "ATTRIB" | "ATTDEF" => {
                let b = body.sub("AcDbText");
                let mut t = self.text(&b, style_of(self, &b));
                let def = kind_name == "ATTDEF";
                let a = body.sub(if def { "AcDbAttributeDefinition" } else { "AcDbAttribute" });
                t.v_align = a.i(74, 0) as u16;
                ObjectKind::Attribute(Attribute {
                    text: t,
                    tag: a.s_or(2, ""),
                    prompt: a.s_or(3, ""),
                    flags: a.i(70, 0) as u8,
                    field_length: a.i(73, 0) as u16,
                    is_definition: def,
                })
            }
            "MTEXT" => {
                let b = body.sub("AcDbMText");
                let mut contents = String::new();
                for x in b.0 {
                    if x.code == 3 {
                        contents.push_str(&vs(&x.value));
                    }
                }
                contents.push_str(&b.s_or(1, ""));
                ObjectKind::MText(MText {
                    insertion: b.p3(10, Vec3::ZERO),
                    extrusion: b.p3(210, Vec3::Z),
                    x_axis: b.p3(11, Vec3::X),
                    rect_width: b.f(41, 0.0),
                    rect_height: b.f(46, 0.0),
                    text_height: b.f(40, 0.0),
                    attachment: b.i(71, 1) as u16,
                    drawing_dir: b.i(72, 1) as u16,
                    contents,
                    linespacing_factor: b.f(44, 1.0),
                    style: style_of(self, &b),
                })
            }
            "INSERT" => {
                let minsert = body.has_marker("AcDbMInsertBlock");
                let b = body.sub(if minsert { "AcDbMInsertBlock" } else { "AcDbBlockReference" });
                type_code = if minsert { 0x08 } else { 0x07 };
                let has = body.i(66, 0) != 0 || b.i(66, 0) != 0;
                ObjectKind::Insert(Insert {
                    insertion: b.p3(10, Vec3::ZERO),
                    scale: Vec3::new(b.f(41, 1.0), b.f(42, 1.0), b.f(43, 1.0)),
                    rotation: b.a(50, 0.0),
                    extrusion: b.p3(210, Vec3::Z),
                    has_attribs: has,
                    block_header: self.lookup(0x31, &b.s_or(2, "")),
                    array: minsert.then(|| (b.i(70, 1) as u16, b.i(71, 1) as u16, b.f(44, 0.0), b.f(45, 0.0))),
                    ..Default::default()
                })
            }
            "SOLID" | "TRACE" => {
                let b = body.sub("AcDbTrace");
                let mut corners = [Vec2::ZERO; 4];
                for (j, c) in (10..14).enumerate() {
                    corners[j] = b.p3(c, Vec3::ZERO).xy();
                }
                ObjectKind::Solid(Solid {
                    thickness: b.f(39, 0.0),
                    elevation: b.p3(10, Vec3::ZERO).z,
                    corners,
                    extrusion: b.p3(210, Vec3::Z),
                    is_trace: kind_name == "TRACE",
                })
            }
            "3DFACE" => {
                let b = body.sub("AcDbFace");
                let mut corners = [Vec3::ZERO; 4];
                for (j, c) in (10..14).enumerate() {
                    corners[j] = b.p3(c, Vec3::ZERO);
                }
                ObjectKind::Face3d(Face3d {
                    corners,
                    invisible_edges: b.i(70, 0) as u16,
                })
            }
            "SPLINE" => {
                let b = body.sub("AcDbSpline");
                let f = b.i(70, 8);
                let mut sp = Spline {
                    degree: b.i(71, 3) as u32,
                    closed: f & 1 != 0,
                    periodic: f & 2 != 0,
                    rational: f & 4 != 0,
                    fit_tolerance: b.f(44, 0.0),
                    start_tangent: b.p3(12, Vec3::ZERO),
                    end_tangent: b.p3(13, Vec3::ZERO),
                    ..Default::default()
                };
                let gs = b.0;
                for (k, x) in gs.iter().enumerate() {
                    let p = || {
                        let y = gs.get(k + 1).map(|n| vf(&n.value)).unwrap_or(0.0);
                        let z = gs.get(k + 2).map(|n| vf(&n.value)).unwrap_or(0.0);
                        Vec3::new(vf(&x.value), y, z)
                    };
                    match x.code {
                        40 => sp.knots.push(vf(&x.value)),
                        41 => sp.weights.push(vf(&x.value)),
                        10 => sp.control_points.push(p()),
                        11 => sp.fit_points.push(p()),
                        _ => {}
                    }
                }
                ObjectKind::Spline(sp)
            }
            "DIMENSION" => {
                let b = body.sub("AcDbDimension");
                let t = b.i(70, 0);
                let kind = match t & 7 {
                    1 => DimensionKind::Aligned,
                    2 => DimensionKind::Angular2Line,
                    3 => DimensionKind::Diameter,
                    4 => DimensionKind::Radius,
                    5 => DimensionKind::Angular3Pt,
                    6 => DimensionKind::Ordinate,
                    _ => DimensionKind::Linear,
                };
                type_code = match kind {
                    DimensionKind::Ordinate => 0x14,
                    DimensionKind::Linear => 0x15,
                    DimensionKind::Aligned => 0x16,
                    DimensionKind::Angular3Pt => 0x17,
                    DimensionKind::Angular2Line => 0x18,
                    DimensionKind::Radius => 0x19,
                    DimensionKind::Diameter => 0x1A,
                };
                let p11 = b.p3(11, Vec3::ZERO);
                let mut m = Dimension {
                    kind,
                    extrusion: b.p3(210, Vec3::Z),
                    text_midpoint: p11.xy(),
                    elevation: p11.z,
                    // DWG's flag byte: bit 0 the opposite of 70's 128, bit 1
                    // its 32 (spec §20.4.2x); AutoCAD 2027 also sets bit 3,
                    // which DXF does not carry, on every dimension it saves.
                    flags: 8 | (if t & 32 != 0 { 2 } else { 0 }) | (if t & 128 == 0 { 1 } else { 0 }),
                    user_text: b.s_or(1, ""),
                    text_rotation: b.a(53, 0.0),
                    horizontal_direction: b.a(51, 0.0),
                    insert_scale: Vec3::new(1.0, 1.0, 1.0),
                    insert_rotation: 0.0,
                    attachment: b.i(71, 5) as u16,
                    measurement: b.f(42, 0.0),
                    pt10: b.p3(10, Vec3::ZERO),
                    dimstyle: self.lookup(0x45, &b.s_or(3, "Standard")),
                    block: self.lookup(0x31, &b.s_or(2, "")),
                    ..Default::default()
                };
                let s = match kind {
                    DimensionKind::Linear | DimensionKind::Aligned => body.sub("AcDbAlignedDimension"),
                    DimensionKind::Angular2Line => body.sub("AcDb2LineAngularDimension"),
                    DimensionKind::Angular3Pt => body.sub("AcDb3PointAngularDimension"),
                    DimensionKind::Diameter => body.sub("AcDbDiametricDimension"),
                    DimensionKind::Radius => body.sub("AcDbRadialDimension"),
                    DimensionKind::Ordinate => body.sub("AcDbOrdinateDimension"),
                };
                m.clone_insertion = s.p3(12, Vec3::ZERO).xy();
                m.pt13 = s.p3(13, Vec3::ZERO);
                m.pt14 = s.p3(14, Vec3::ZERO);
                m.pt15 = s.p3(15, Vec3::ZERO);
                m.pt16 = s.p3(16, Vec3::ZERO).xy();
                m.dim_rotation = s.a(50, 0.0);
                m.ext_line_rotation = s.a(52, 0.0);
                m.leader_length = s.f(40, 0.0);
                ObjectKind::Dimension(m)
            }
            "HATCH" => ObjectKind::Hatch(self.hatch(body.sub("AcDbHatch"))),
            "XLINE" | "RAY" => {
                let b = body.sub(if kind_name == "RAY" { "AcDbRay" } else { "AcDbXline" });
                let x = ConstructionLine {
                    base: b.p3(10, Vec3::ZERO),
                    direction: b.p3(11, Vec3::X),
                };
                if kind_name == "RAY" {
                    ObjectKind::Ray(x)
                } else {
                    ObjectKind::Xline(x)
                }
            }
            "LEADER" => {
                let b = body.sub("AcDbLeader");
                let vertices: Vec<Vec3> = {
                    let xs: Vec<f64> = b.0.iter().filter(|g| g.code == 10).map(|g| vf(&g.value)).collect();
                    let ys: Vec<f64> = b.0.iter().filter(|g| g.code == 20).map(|g| vf(&g.value)).collect();
                    let zs: Vec<f64> = b.0.iter().filter(|g| g.code == 30).map(|g| vf(&g.value)).collect();
                    xs.iter()
                        .enumerate()
                        .map(|(i, x)| Vec3::new(*x, ys.get(i).copied().unwrap_or(0.0), zs.get(i).copied().unwrap_or(0.0)))
                        .collect()
                };
                ObjectKind::Leader(Leader {
                    origin: vertices.first().copied().unwrap_or(Vec3::ZERO),
                    vertices,
                    arrowhead: b.i(71, 1) != 0,
                    spline: b.i(72, 0) != 0,
                    annotation_type: b.i(73, 3) as u16,
                    hookline_on_x: b.i(74, 1) != 0,
                    hookline: b.i(75, 0) != 0,
                    text_height: b.f(40, 0.0),
                    text_width: b.f(41, 0.0),
                    extrusion: b.p3(210, Vec3::Z),
                    x_direction: b.p3(211, Vec3::X),
                    block_offset: b.p3(212, Vec3::ZERO),
                    annotation_offset: b.p3(213, Vec3::ZERO),
                    annotation: b.h(340),
                    dimstyle: self.lookup(0x45, &b.s_or(3, "Standard")),
                    unknown: (false, 0, false, false),
                })
            }
            "VIEWPORT" => {
                let b = body.sub("AcDbViewport");
                let mut v = PaperViewport::new(
                    b.p3(10, Vec3::ZERO).xy(),
                    b.f(40, 0.0),
                    b.f(41, 0.0),
                    b.p2(12, Vec2::ZERO),
                    1.0,
                    b.i(69, 0) as i16,
                );
                v.center = b.p3(10, Vec3::ZERO);
                v.view_height = b.f(45, v.height);
                v.view_target = b.p3(17, Vec3::ZERO);
                v.view_direction = b.p3(16, Vec3::Z);
                v.twist = b.a(51, 0.0);
                v.lens = b.f(42, 50.0);
                v.front_clip = b.f(43, 0.0);
                v.back_clip = b.f(44, 0.0);
                v.snap_angle = b.a(50, 0.0);
                v.snap_base = b.p2(13, Vec2::ZERO);
                v.snap_spacing = b.p2(14, Vec2::new(10.0, 10.0));
                v.grid_spacing = b.p2(15, Vec2::new(10.0, 10.0));
                v.circle_zoom = b.i(72, 1000) as u16;
                v.grid_major = b.i(61, 5) as u16;
                v.frozen_layers = b.0.iter().filter(|g| g.code == 331).map(|g| vh(&g.value)).collect();
                v.status = b.i(90, PaperViewport::DEFAULT_STATUS as i64) as u32;
                if b.i(68, 1) == 0 {
                    v.status |= PaperViewport::STATUS_OFF;
                }
                v.style_sheet = b.s_or(1, "");
                v.render_mode = b.i(281, 0) as u8;
                v.ucs_per_viewport = b.i(71, 1) != 0;
                v.ucs_at_origin = b.i(74, 1) != 0;
                v.ucs_origin = b.p3(110, Vec3::ZERO);
                v.ucs_x = b.p3(111, Vec3::X);
                v.ucs_y = b.p3(112, Vec3::Y);
                v.ucs_ortho_type = b.i(79, 0) as u16;
                v.ucs_elevation = b.f(146, 0.0);
                v.shade_plot_mode = b.i(170, 0) as u16;
                v.default_lights = b.i(292, 1) != 0;
                v.lighting_type = b.i(282, 1) as u8;
                v.brightness = b.f(141, 0.0);
                v.contrast = b.f(142, 0.0);
                v.ambient = Color::from_aci(b.i(63, 250) as i16);
                v.clip_boundary = b.h(340);
                v.named_ucs = b.h(345);
                v.base_ucs = b.h(346);
                v.background = b.h(332);
                v.visual_style = b.h(348);
                v.shade_plot = b.h(333);
                v.sun = b.h(361);
                ObjectKind::PaperViewport(Box::new(v))
            }
            "BLOCK" => {
                let b = body.sub("AcDbBlockBegin");
                // DWG keeps the record's name bare ("*D") and the BLOCK
                // entity's numbered ("*D0"), as DXF has it.
                let name = b.s_or(2, "");
                let flags = b.i(70, 0);
                if let Some(rec) = self.d.objects.get_mut(&owner) {
                    if let ObjectKind::BlockHeader(bh) = &mut rec.kind {
                        bh.anonymous = flags & 1 != 0;
                        bh.has_attribs = flags & 2 != 0;
                        bh.is_xref = flags & 4 != 0;
                        bh.is_overlaid = flags & 8 != 0;
                        bh.base_point = b.p3(10, Vec3::ZERO);
                        bh.xref_path = b.s_or(1, "");
                        bh.description = b.s_or(4, "");
                        bh.block_entity = r.handle;
                        if bh.anonymous {
                            bh.name = bare_anonymous(&name);
                        }
                    }
                }
                ObjectKind::BlockBegin { name }
            }
            "ENDBLK" => {
                if let Some(rec) = self.d.objects.get_mut(&owner) {
                    if let ObjectKind::BlockHeader(bh) = &mut rec.kind {
                        bh.endblk = r.handle;
                    }
                }
                ObjectKind::BlockEnd
            }
            _ => {
                *self.skipped.entry(kind_name.to_string()).or_default() += 1;
                return Ok(None);
            }
        };
        let _ = in_blocks;
        if type_name.is_empty() || type_code < 500 {
            if let Some(n) = crate::dwg::fixed_type_name(type_code) {
                type_name = n.to_string();
            }
        }
        let mut o = self.new_object(r, type_code, &type_name, kind);
        // DWG stores no owner for model- and paper-space entities: their
        // mode says where they are.
        o.owner = if matches!(e.space, Space::Model | Space::Paper) { Handle::NULL } else { owner };
        o.entity = Some(e);
        Ok(Some(o))
    }

    fn text(&self, b: &G, style: Handle) -> Text {
        let p10 = b.p3(10, Vec3::ZERO);
        let h_align = b.i(72, 0) as u16;
        Text {
            insertion: p10.xy(),
            // DWG keeps the alignment point at the insertion point for text
            // that DXF writes without one (left aligned).
            alignment: b.p3(11, p10).xy(),
            elevation: p10.z,
            extrusion: b.p3(210, Vec3::Z),
            thickness: b.f(39, 0.0),
            oblique: b.a(51, 0.0),
            rotation: b.a(50, 0.0),
            height: b.f(40, 0.0),
            width_factor: b.f(41, 1.0),
            value: b.s_or(1, ""),
            generation: b.i(71, 0) as u16,
            h_align,
            v_align: 0,
            style,
        }
    }

    fn hatch(&self, b: G) -> Hatch {
        let mut h = Hatch {
            elevation: b.p3(10, Vec3::ZERO).z,
            extrusion: b.p3(210, Vec3::Z),
            pattern_name: b.s_or(2, ""),
            solid_fill: b.i(70, 0) != 0,
            associative: b.i(71, 0) != 0,
            ..Default::default()
        };
        let gs = b.0;
        let n_paths = b.i(91, 0).max(0) as usize;
        let start = gs.iter().position(|x| x.code == 91).map(|p| p + 1).unwrap_or(gs.len());
        let mut k = start;
        let next = |k: &mut usize, code: u16| -> Option<&Value> {
            while *k < gs.len() && gs[*k].code != code {
                *k += 1;
            }
            let v = gs.get(*k).map(|x| &x.value);
            *k += 1;
            v
        };
        let p2 = |k: &mut usize, code: u16| -> Vec2 {
            let x = next(k, code).map(vf).unwrap_or(0.0);
            let y = next(k, code + 10).map(vf).unwrap_or(0.0);
            Vec2::new(x, y)
        };
        for _ in 0..n_paths {
            let flags = next(&mut k, 92).map(vi).unwrap_or(0) as u32;
            if flags & 2 != 0 {
                let has_bulge = next(&mut k, 72).map(vi).unwrap_or(0) != 0;
                let closed = next(&mut k, 73).map(vi).unwrap_or(0) != 0;
                let n = next(&mut k, 93).map(vi).unwrap_or(0).max(0) as usize;
                let mut vertices = Vec::with_capacity(n);
                for _ in 0..n {
                    let p = p2(&mut k, 10);
                    let bl = if has_bulge { next(&mut k, 42).map(vf).unwrap_or(0.0) } else { 0.0 };
                    vertices.push((p, bl));
                }
                h.paths.push(HatchPath::Polyline { flags: flags & !2, closed, vertices });
            } else {
                let n = next(&mut k, 93).map(vi).unwrap_or(0).max(0) as usize;
                let mut edges = Vec::with_capacity(n);
                for _ in 0..n {
                    let t = next(&mut k, 72).map(vi).unwrap_or(1);
                    edges.push(match t {
                        2 => {
                            let center = p2(&mut k, 10);
                            let radius = next(&mut k, 40).map(vf).unwrap_or(0.0);
                            let start = rad(next(&mut k, 50).map(vf).unwrap_or(0.0));
                            let end = rad(next(&mut k, 51).map(vf).unwrap_or(0.0));
                            let ccw = next(&mut k, 73).map(vi).unwrap_or(1) != 0;
                            HatchEdge::Arc { center, radius, start, end, ccw }
                        }
                        3 => {
                            let center = p2(&mut k, 10);
                            let major = p2(&mut k, 11);
                            let ratio = next(&mut k, 40).map(vf).unwrap_or(1.0);
                            let start = rad(next(&mut k, 50).map(vf).unwrap_or(0.0));
                            let end = rad(next(&mut k, 51).map(vf).unwrap_or(0.0));
                            let ccw = next(&mut k, 73).map(vi).unwrap_or(1) != 0;
                            HatchEdge::Ellipse { center, major, ratio, start, end, ccw }
                        }
                        4 => {
                            let degree = next(&mut k, 94).map(vi).unwrap_or(3) as u32;
                            let rational = next(&mut k, 73).map(vi).unwrap_or(0) != 0;
                            let _periodic = next(&mut k, 74);
                            let nk = next(&mut k, 95).map(vi).unwrap_or(0).max(0) as usize;
                            let nc = next(&mut k, 96).map(vi).unwrap_or(0).max(0) as usize;
                            let knots = (0..nk).map(|_| next(&mut k, 40).map(vf).unwrap_or(0.0)).collect();
                            let mut control_points = Vec::with_capacity(nc);
                            let mut weights = Vec::new();
                            for _ in 0..nc {
                                control_points.push(p2(&mut k, 10));
                                if rational {
                                    weights.push(next(&mut k, 42).map(vf).unwrap_or(1.0));
                                }
                            }
                            HatchEdge::Spline { degree, knots, control_points, weights }
                        }
                        _ => {
                            let a = p2(&mut k, 10);
                            let b2 = p2(&mut k, 11);
                            HatchEdge::Line { a, b: b2 }
                        }
                    });
                }
                h.paths.push(HatchPath::Edges { flags, edges });
            }
            let _ = next(&mut k, 97);
        }
        let tail_groups = &gs[k.min(gs.len())..];
        let tail = G(tail_groups);
        h.style = tail.i(75, 0) as u16;
        h.pattern_type = tail.i(76, 1) as u16;
        h.pattern_angle = tail.a(52, 0.0);
        h.pattern_scale = tail.f(41, 1.0);
        h.pattern_double = tail.i(77, 0) != 0;
        // Pattern line families (78), then seed points (98).
        if let Some(at) = tail_groups.iter().position(|g| g.code == 78) {
            let n = vi(&tail_groups[at].value).max(0) as usize;
            let mut j = at + 1;
            let take = |j: &mut usize, code: u16| -> Option<f64> {
                match tail_groups.get(*j) {
                    Some(g) if g.code == code => {
                        *j += 1;
                        Some(vf(&g.value))
                    }
                    _ => None,
                }
            };
            for _ in 0..n.min(1000) {
                let (Some(angle), Some(bx), Some(by), Some(ox), Some(oy)) =
                    (take(&mut j, 53), take(&mut j, 43), take(&mut j, 44), take(&mut j, 45), take(&mut j, 46))
                else {
                    break;
                };
                let nd = take(&mut j, 79).unwrap_or(0.0).max(0.0) as usize;
                let mut dashes = Vec::with_capacity(nd.min(64));
                for _ in 0..nd.min(64) {
                    match take(&mut j, 49) {
                        Some(v) => dashes.push(v),
                        None => break,
                    }
                }
                h.pattern_lines.push(HatchPatternLine {
                    angle: rad(angle),
                    base: Vec2::new(bx, by),
                    offset: Vec2::new(ox, oy),
                    dashes,
                });
            }
        }
        if let Some(at) = tail_groups.iter().position(|g| g.code == 98) {
            let n = vi(&tail_groups[at].value).max(0) as usize;
            let rest = &tail_groups[at + 1..];
            let xs: Vec<f64> = rest.iter().filter(|g| g.code == 10).map(|g| vf(&g.value)).take(n).collect();
            let ys: Vec<f64> = rest.iter().filter(|g| g.code == 20).map(|g| vf(&g.value)).take(n).collect();
            h.seed_points = xs.iter().zip(&ys).map(|(x, y)| Vec2::new(*x, *y)).collect();
        }
        h
    }

    // ----- objects ------------------------------------------------------------

    fn objects(&mut self, groups: &[Group]) -> Result<()> {
        let mut first_dict = true;
        for (kind, g) in records(groups) {
            let r = self.rec(&kind, g);
            let body = G(&r.body);
            let ng = match kind.as_str() {
                "DICTIONARY" | "ACDBDICTIONARYWDFLT" => {
                    let b = body.sub("AcDbDictionary");
                    let mut d = Dictionary {
                        cloning: b.i(281, 1) as u16,
                        hard_owner: b.i(280, 0) as u8,
                        entries: Vec::new(),
                        default: None,
                    };
                    let mut key: Option<String> = None;
                    for x in b.0 {
                        match x.code {
                            3 => key = Some(vs(&x.value)),
                            350 | 360 => {
                                if let Some(k) = key.take() {
                                    d.entries.push((k, vh(&x.value)));
                                }
                            }
                            _ => {}
                        }
                    }
                    if kind == "ACDBDICTIONARYWDFLT" {
                        d.default = Some(body.sub("AcDbDictionaryWithDefault").h(340));
                    }
                    if first_dict && r.owner.is_null() {
                        first_dict = false;
                        self.d.header.named_objects_dict = r.handle;
                        self.raw("DICTIONARY_NAMED_OBJECTS", format!("#{:X}", r.handle.0));
                        for (k, h) in &d.entries {
                            if let Some(var) = dictionary_var(k) {
                                self.raw(var, format!("#{:X}", h.0));
                            }
                        }
                    }
                    NonGraphic::Dictionary(d)
                }
                "XRECORD" => {
                    let b = body.sub("AcDbXrecord");
                    let gs = b.0;
                    let cloning = gs.first().filter(|x| x.code == 280).map(|x| vi(&x.value)).unwrap_or(1);
                    let mut items = Vec::new();
                    let mut k = usize::from(gs.first().is_some_and(|x| x.code == 280));
                    while k < gs.len() {
                        let x = &gs[k];
                        let Some(t) = xr_type(x.code) else {
                            k += 1;
                            continue;
                        };
                        let v = match t {
                            XrType::Str => XrValue::Str(vs(&x.value)),
                            XrType::Point => {
                                let y = gs.get(k + 1).map(|n| vf(&n.value)).unwrap_or(0.0);
                                let z = gs.get(k + 2).map(|n| vf(&n.value)).unwrap_or(0.0);
                                k += 2;
                                XrValue::Point(Vec3::new(vf(&x.value), y, z))
                            }
                            XrType::Real => XrValue::Real(vf(&x.value)),
                            XrType::Int8 => XrValue::Int8(vi(&x.value) as i8),
                            XrType::Int16 => XrValue::Int16(vi(&x.value) as i16),
                            XrType::Int32 => XrValue::Int32(vi(&x.value) as i32),
                            XrType::Int64 => XrValue::Int64(vi(&x.value)),
                            XrType::Bool => XrValue::Bool(vi(&x.value) != 0),
                            XrType::Bytes => XrValue::Bytes(match &x.value {
                                Value::Bytes(b) => b.clone(),
                                _ => Vec::new(),
                            }),
                            XrType::Handle => XrValue::Handle(vh(&x.value)),
                        };
                        items.push((x.code, v));
                        k += 1;
                    }
                    NonGraphic::XRecord(XRecord {
                        cloning: cloning as u16,
                        items,
                    })
                }
                "ACDBPLACEHOLDER" => NonGraphic::Placeholder,
                "DICTIONARYVAR" => {
                    let b = body.sub("DictionaryVariables");
                    NonGraphic::DictionaryVar {
                        schema: b.i(280, 0) as u8,
                        value: b.s_or(1, ""),
                    }
                }
                "SCALE" => {
                    let b = body.sub("AcDbScale");
                    NonGraphic::Scale(Scale {
                        flag: b.i(70, 0) as u16,
                        name: b.s_or(300, ""),
                        paper_units: b.f(140, 1.0),
                        drawing_units: b.f(141, 1.0),
                        unit_scale: b.i(290, 0) != 0,
                    })
                }
                "LAYOUT" => {
                    let p = body.sub("AcDbPlotSettings");
                    let l = body.sub("AcDbLayout");
                    let plot = PlotSettings {
                        page_setup_name: p.s_or(1, ""),
                        printer: p.s_or(2, ""),
                        paper_size: p.s_or(4, ""),
                        margins: [p.f(40, 0.0), p.f(41, 0.0), p.f(42, 0.0), p.f(43, 0.0)],
                        paper_width: p.f(44, 0.0),
                        paper_height: p.f(45, 0.0),
                        origin: Vec2::new(p.f(46, 0.0), p.f(47, 0.0)),
                        window_min: Vec2::new(p.f(48, 0.0), p.f(49, 0.0)),
                        window_max: Vec2::new(p.f(140, 0.0), p.f(141, 0.0)),
                        real_world_units: p.f(142, 1.0),
                        drawing_units: p.f(143, 1.0),
                        flags: p.i(70, 0) as u16,
                        paper_units: p.i(72, 0) as u16,
                        rotation: p.i(73, 0) as u16,
                        plot_type: p.i(74, 0) as u16,
                        style_sheet: p.s_or(7, ""),
                        scale_type: p.i(75, 0) as u16,
                        scale_factor: p.f(147, 1.0),
                        shade_plot_mode: p.i(76, 0) as u16,
                        shade_plot_res_level: p.i(77, 0) as u16,
                        shade_plot_custom_dpi: p.i(78, 0) as u16,
                        image_origin: Vec2::new(p.f(148, 0.0), p.f(149, 0.0)),
                        plot_view: Handle::NULL,
                        visual_style: Handle::NULL,
                    };
                    NonGraphic::Layout(Box::new(Layout {
                        plot,
                        name: l.s_or(1, ""),
                        flags: l.i(70, 0) as u16,
                        tab_order: l.i(71, 0) as u32,
                        limmin: l.p2(10, Vec2::ZERO),
                        limmax: l.p2(11, Vec2::ZERO),
                        insbase: l.p3(12, Vec3::ZERO),
                        extmin: l.p3(14, Vec3::ZERO),
                        extmax: l.p3(15, Vec3::ZERO),
                        elevation: l.f(146, 0.0),
                        ucs_origin: l.p3(13, Vec3::ZERO),
                        ucs_x: l.p3(16, Vec3::X),
                        ucs_y: l.p3(17, Vec3::new(0.0, 1.0, 0.0)),
                        ortho_type: l.i(76, 0) as u16,
                        block: l.h(330),
                        last_viewport: l.h(331),
                        base_ucs: l.h(346),
                        named_ucs: l.h(345),
                        viewports: Vec::new(),
                    }))
                }
                "MLINESTYLE" => {
                    let b = body.sub("AcDbMlineStyle");
                    let mut m = MLineStyle {
                        name: b.s_or(2, ""),
                        description: b.s_or(3, ""),
                        flags: b.i(70, 0) as u16,
                        fill_color: Color::from_aci(b.i(62, 256) as i16),
                        start_angle: b.a(51, std::f64::consts::FRAC_PI_2),
                        end_angle: b.a(52, std::f64::consts::FRAC_PI_2),
                        elements: Vec::new(),
                    };
                    // Elements follow the 71 count: 49, 62, 6 each.
                    let gs = b.0;
                    let start = gs.iter().position(|x| x.code == 71).map(|p| p + 1).unwrap_or(gs.len());
                    let mut cur: Option<MLineElement> = None;
                    for x in &gs[start..] {
                        match x.code {
                            49 => {
                                if let Some(e) = cur.take() {
                                    m.elements.push(e);
                                }
                                cur = Some(MLineElement {
                                    offset: vf(&x.value),
                                    color: Color::ByLayer,
                                    linetype: Handle::NULL,
                                    linetype_index: 0,
                                });
                            }
                            62 => {
                                if let Some(e) = &mut cur {
                                    e.color = Color::from_aci(vi(&x.value) as i16);
                                }
                            }
                            6 => {
                                if let Some(e) = &mut cur {
                                    let n = vs(&x.value);
                                    e.linetype = self.lookup(0x39, &n);
                                }
                            }
                            _ => {}
                        }
                    }
                    if let Some(e) = cur.take() {
                        m.elements.push(e);
                    }
                    NonGraphic::MLineStyle(m)
                }
                k if Groups::KINDS.contains(&k) => NonGraphic::Groups(Groups {
                    kind: k.to_string(),
                    items: typed_items(&r.body),
                }),
                other => {
                    *self.skipped.entry(other.to_string()).or_default() += 1;
                    continue;
                }
            };
            let Some((code, name)) = self.type_of(&kind) else {
                *self.skipped.entry(kind.clone()).or_default() += 1;
                continue;
            };
            let o = self.new_object(&r, code, &name, ObjectKind::NonGraphic(ng));
            self.insert(o);
        }
        Ok(())
    }

    // ----- finishing ----------------------------------------------------------

    fn finish(&mut self) {
        // Owned entity lists.
        let owned = std::mem::take(&mut self.owned);
        for (owner, list) in &owned {
            let Some(o) = self.d.objects.get(owner) else { continue };
            match o.kind.clone() {
                ObjectKind::BlockHeader(mut b) => {
                    b.entities = list.clone();
                    self.d.objects.get_mut(owner).unwrap().kind = ObjectKind::BlockHeader(b);
                }
                ObjectKind::Polyline(mut p) => {
                    let (verts, rest): (Vec<Handle>, Vec<Handle>) = list
                        .iter()
                        .partition(|h| matches!(self.d.objects.get(h).map(|o| &o.kind), Some(ObjectKind::Vertex(_))));
                    p.vertices = verts;
                    p.seqend = rest.last().copied().unwrap_or(Handle::NULL);
                    self.d.objects.get_mut(owner).unwrap().kind = ObjectKind::Polyline(p);
                }
                ObjectKind::Insert(mut ins) => {
                    let (attrs, rest): (Vec<Handle>, Vec<Handle>) = list
                        .iter()
                        .partition(|h| matches!(self.d.objects.get(h).map(|o| &o.kind), Some(ObjectKind::Attribute(_))));
                    ins.attribs = attrs;
                    ins.seqend = rest.last().copied().unwrap_or(Handle::NULL);
                    self.d.objects.get_mut(owner).unwrap().kind = ObjectKind::Insert(ins);
                }
                _ => {}
            }
        }
        // Header variables given by name.
        let named: Vec<(String, String)> = self
            .d
            .header
            .raw
            .iter()
            .filter_map(|(k, v)| k.strip_prefix("@NAME:").map(|n| (n.to_string(), v.clone())))
            .collect();
        self.d.header.raw.retain(|(k, _)| !k.starts_with("@NAME:"));
        for (var, name) in named {
            let h = if var == "CMLSTYLE" {
                self.mline_style(&name)
            } else if name.is_empty() {
                Handle::NULL
            } else {
                self.lookup(name_table(&var), &name)
            };
            self.raw(&var, format!("#{:X}", h.0));
            match var.as_str() {
                "CLAYER" => self.d.header.clayer = h,
                "TEXTSTYLE" => self.d.header.textstyle = h,
                "CELTYPE" => self.d.header.celtype = h,
                "DIMSTYLE" => self.d.header.dimstyle = h,
                _ => {}
            }
        }
        // The interference visual styles AutoCAD's DXF names when its DWG
        // leaves them null (the writer's mirror).
        for (var, name) in [("INTERFEREOBJVS", "Realistic"), ("INTERFEREVPVS", "Wireframe")] {
            let default = super::write::visual_style(&self.d, name);
            let set = self.d.header.raw.iter().find(|(k, _)| k == var).map(|(_, v)| v.clone());
            if !default.is_null() && set.as_deref() == Some(format!("#{:X}", default.0).as_str()) {
                self.raw(var, "#0".to_string());
            }
        }
        // Typed fields, as the DWG reader sets them.
        let raw = self.d.header.raw.clone();
        let hv = &mut self.d.header;
        let floats = |t: &str| -> Vec<f64> {
            t.trim_matches(|c| c == '(' || c == ')').split(',').filter_map(|x| x.trim().parse().ok()).collect()
        };
        for (k, v) in &raw {
            let h = || v.strip_prefix('#').and_then(|t| u64::from_str_radix(t, 16).ok()).map(Handle).unwrap_or(Handle::NULL);
            let p3 = || {
                let f = floats(v);
                (f.len() == 3).then(|| Vec3::new(f[0], f[1], f[2]))
            };
            let p2 = || {
                let f = floats(v);
                (f.len() == 2).then(|| Vec2::new(f[0], f[1]))
            };
            match k.as_str() {
                "INSUNITS" => hv.insunits = v.parse().ok(),
                "MEASUREMENT" => hv.measurement = v.parse().ok(),
                "EXTMIN" => hv.extmin = p3(),
                "EXTMAX" => hv.extmax = p3(),
                "LIMMIN" => hv.limmin = p2(),
                "LIMMAX" => hv.limmax = p2(),
                "INSBASE" => hv.insbase = p3(),
                "LTSCALE" => hv.ltscale = v.parse().ok(),
                "TEXTSIZE" => hv.textsize = v.parse().ok(),
                "CELTSCALE" => hv.celtscale = v.parse().ok(),
                "LUNITS" => hv.lunits = v.parse().ok(),
                "LUPREC" => hv.luprec = v.parse().ok(),
                "AUNITS" => hv.aunits = v.parse().ok(),
                "AUPREC" => hv.auprec = v.parse().ok(),
                "ANGBASE" => hv.angbase = v.parse().ok(),
                "ANGDIR" => hv.angdir = v.parse().ok(),
                "BLOCK_CONTROL" => hv.block_control = h(),
                "LAYER_CONTROL" => hv.layer_control = h(),
                "STYLE_CONTROL" => hv.style_control = h(),
                "LTYPE_CONTROL" => hv.linetype_control = h(),
                "DIMSTYLE_CONTROL" => hv.dimstyle_control = h(),
                _ => {}
            }
        }
        // MEASUREMENT has its own field only (the DWG reader takes it from
        // AcDb:Template).
        self.d.header.raw.retain(|(k, _)| k != "MEASUREMENT");
    }

    /// The multiline style of a dictionary key in ACAD_MLINESTYLE.
    fn mline_style(&self, key: &str) -> Handle {
        let dict = self
            .d
            .header
            .raw
            .iter()
            .find(|(k, _)| k == "DICTIONARY_ACAD_MLINESTYLE")
            .and_then(|(_, v)| v.strip_prefix('#'))
            .and_then(|t| u64::from_str_radix(t, 16).ok())
            .map(Handle);
        let Some(o) = dict.and_then(|h| self.d.objects.get(&h)) else { return Handle::NULL };
        match &o.kind {
            ObjectKind::NonGraphic(NonGraphic::Dictionary(d)) => d
                .entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, h)| *h)
                .unwrap_or(Handle::NULL),
            _ => Handle::NULL,
        }
    }
}

/// An anonymous block's name as DWG stores it: DXF numbers them ("*D0",
/// "*U12"), DWG keeps the letters ("*D").
fn bare_anonymous(name: &str) -> String {
    let t = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if t.len() >= 2 && t.starts_with('*') && t[1..].chars().all(|c| c.is_ascii_alphabetic()) {
        t.to_string()
    } else {
        name.to_string()
    }
}

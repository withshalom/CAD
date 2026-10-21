//! The sections around the objects: AcDb:Header (spec chapter 9),
//! AcDb:Classes (chapter 10), AcDb:Handles (chapter 23), AcDb:AuxHeader
//! (chapter 27), AcDb:Template (22), AcDb:ObjFreeSpace (21),
//! AcDb:RevHistory (18), AcDb:SummaryInfo (13), AcDb:Preview (14) and
//! AcDb:AppInfo (16), in the R2018 forms AutoCAD 2027 writes them.

use super::super::bits::BitWriter;
use super::super::crc::crc16;
use super::super::header::{self, VarKind};
use super::super::hvars::Hv;
use super::super::section::{CLASSES_END, CLASSES_START, HEADER_END, HEADER_START};
use super::super::version::DwgVersion as V;
use super::super::version::DwgVersion;
use crate::geom::{Vec2, Vec3};
use crate::model::*;

/// A body of bit fields with an R2007+ string stream and handle stream,
/// laid out like an object: main data, strings, string-stream size and
/// flag, then the handles.
pub(crate) struct Streams3 {
    pub dat: BitWriter,
    pub strs: BitWriter,
    pub hdl: BitWriter,
}

impl Streams3 {
    fn new() -> Self {
        Streams3 {
            dat: BitWriter::new(),
            strs: BitWriter::new(),
            hdl: BitWriter::new(),
        }
    }

    /// The body: RL end-of-data bit (counted from the body's start), main
    /// data, strings, their size and the presence bit, then the handles.
    /// `pad` fills the last byte.
    fn body(self, pad: bool) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.rl(0); // patched below
        w.append(&self.dat);
        let n = self.strs.position();
        w.append(&self.strs);
        if n >= 0x8000 {
            w.rs((n >> 15) as u16);
            w.rs((n & 0x7FFF) as u16 | 0x8000);
        } else {
            w.rs(n as u16);
        }
        w.b(true);
        let endbit = w.position();
        w.append(&self.hdl);
        while !w.position().is_multiple_of(8) {
            w.b(pad);
        }
        let mut out = w.into_bytes();
        out[..4].copy_from_slice(&(endbit as u32).to_le_bytes());
        out
    }
}

/// Wrap a body in sentinels: size, high size (R2010+), body, CRC.
fn wrap(start: &[u8; 16], end: &[u8; 16], body: &[u8], ver: DwgVersion) -> Vec<u8> {
    let mut out = start.to_vec();
    let from = out.len();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    if ver >= V::R2010 {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    out.extend_from_slice(body);
    let crc = crc16(0xC0C1, &out[from..]);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(end);
    out
}

fn julian(v: f64) -> (u32, u32) {
    let day = v.floor();
    let ms = ((v - day) * 86_400_000.0).round();
    (day as u32, ms as u32)
}

/// Default values of the header variables for a drawing that has none
/// (AutoCAD's own defaults, as an imperial drawing carries them).
fn default_text(name: &str) -> &'static str {
    match name {
        "UNKNOWN_BD_412148564080" => "412148564080",
        "UNKNOWN_BD_1A" | "UNKNOWN_BD_1B" | "UNKNOWN_BD_1C" => "1",
        "UNKNOWN_TV_1" => "\"m\"",
        // The spec gives 24; AutoCAD writes 0 in every file of the corpus.
        "UNKNOWN_BL_24" => "0",
        // Undocumented longs AutoCAD writes with the same values in every
        // file of the corpus, 2004 to 2018, whatever the drawing.
        "UNKNOWN_BL_R2004_1" | "UNKNOWN_BL_R2004_6" => "808464432",
        "UNKNOWN_BL_R2004_2" | "UNKNOWN_BL_R2004_5" => "486539264",
        "UNKNOWN_BL_R2004_3" | "UNKNOWN_BL_R2004_4" => "1295003696",
        "MENUNAME" => "\".\"",
        "DIMASO" | "DIMSHO" | "REGENMODE" | "FILLMODE" | "PSLTSCALE" | "USRTIMER"
        | "WORLDVIEW" | "TILEMODE" | "VISRETAIN" | "DIMTIH" | "DIMTOH" | "CAMERADISPLAY" => "",
        "PROXYGRAPHICS" | "ATTMODE" | "SHOWHIST" | "LOFTNORMALS" | "LIGHTGLYPHDISPLAY"
        | "TILEMODELIGHTSYNCH" | "HIDETEXT" | "DIMTOLJ" => "1",
        "TREEDEPTH" => "3020",
        "LUNITS" | "DIMALTD" | "DIMALTU" | "DIMALTTD" | "DIMLUNIT" | "XCLIPFRAME" | "DIMASSOC"
        | "DWFFRAME" => "2",
        "LUPREC" | "DIMDEC" | "DIMTDEC" => "4",
        "SPLINESEGS" | "ISOLINES" => "8",
        "SURFU" | "SURFV" | "SURFTYPE" | "SURFTAB1" | "SURFTAB2" | "SPLINETYPE" => "6",
        "SHADEDGE" | "DIMATFIT" => "3",
        "SHADEDIF" | "TSTACKSIZE" => "70",
        "MAXACTVP" => "64",
        "TEXTQLTY" => "50",
        "LTSCALE" | "CMLSCALE" | "CELTSCALE" | "DIMSCALE" | "DIMLFAC" | "DIMTFAC" | "DIMFXL"
        | "UNKNOWN_BD_R2007_1" => "1",
        "TEXTSIZE" => "0.2",
        "TRACEWID" => "0.05",
        "SKETCHINC" => "0.1",
        "FACETRES" => "0.5",
        "PEXTMIN" | "EXTMIN" => "(100000000000000000000, 100000000000000000000, 100000000000000000000)",
        "PEXTMAX" | "EXTMAX" => "(-100000000000000000000, -100000000000000000000, -100000000000000000000)",
        "PLIMMAX" | "LIMMAX" => "(12, 9)",
        "PUCSXDIR" | "UCSXDIR" => "(1, 0, 0)",
        "PUCSYDIR" | "UCSYDIR" => "(0, 1, 0)",
        "DIMASZ" | "DIMEXE" | "DIMTXT" => "0.18",
        "DIMEXO" => "0.0625",
        "DIMDLI" => "0.38",
        "DIMCEN" | "DIMGAP" => "0.09",
        "DIMALTF" => "25.4",
        "DIMJOGANG" => "0.7853981633974483",
        "DIMDSEP" => "46",
        "DIMLWD" | "DIMLWE" => "-2",
        "DIMALTMZF" | "DIMMZF" => "100",
        "FLAGS" => "10781",
        "SORTENTS" => "127",
        "OBSCUREDCOLOR" | "INTERSECTIONCOLOR" => "257",
        "TSTACKALIGN" => "1",
        "UNKNOWN_BL_R2007_2" => "10",
        "STEPSPERSEC" | "3DDWFPREC" => "2",
        "STEPSIZE" => "6",
        "LENSLENGTH" => "50",
        "PSOLWIDTH" => "0.25",
        "PSOLHEIGHT" => "4",
        "LOFTANG1" | "LOFTANG2" => "1.5707963267948966",
        "LOFTPARAM" => "7",
        "LATITUDE" => "37.795",
        "LONGITUDE" => "-122.394",
        "TIMEZONE" => "-8000",
        "UNKNOWN_B_R2007" => "true",
        _ => "",
    }
}

/// AcDb:Header for R2018 (the variable list of `dwg::header`), from the
/// drawing's header variables.
pub(crate) fn header_section(d: &Drawing, ver: DwgVersion, handseed: u64) -> Vec<u8> {
    let hv = Hv::new(d);
    let mut s = Streams3::new();
    let mut cepsntype = 0i64;
    for (name, kind, applies) in header::vars() {
        if !applies(ver, cepsntype) {
            continue;
        }
        let dt = default_text(name);
        let truthy = |t: &str| t == "true" || t == "1";
        match kind {
            VarKind::B => {
                let default = match dt {
                    "" => matches!(
                        name,
                        "DIMASO" | "DIMSHO" | "REGENMODE" | "FILLMODE" | "PSLTSCALE" | "USRTIMER"
                            | "WORLDVIEW" | "TILEMODE" | "VISRETAIN" | "DIMTIH" | "DIMTOH"
                    ),
                    t => truthy(t),
                };
                s.dat.b(hv.bool(name, default));
            }
            VarKind::Bs => {
                let v = hv.int(name, dt.parse().unwrap_or(0));
                if name == "CEPSNTYPE" {
                    cepsntype = v;
                }
                s.dat.bs(v as i16 as u16);
            }
            VarKind::Bl => s.dat.bl(hv.int(name, dt.parse().unwrap_or(0)) as i32 as u32),
            VarKind::Bll => s.dat.bll(hv.int(name, dt.parse().unwrap_or(0)) as u64),
            VarKind::Rc => s.dat.rc(hv.int(name, dt.parse().unwrap_or(0)) as u8),
            VarKind::Bd => s.dat.bd(hv.real(name, dt.parse().unwrap_or(0.0))),
            VarKind::Rd2 => {
                let p = hv.p2(name, parse_p2(dt));
                s.dat.rd2([p.x, p.y]);
            }
            VarKind::Bd3 => {
                let p = hv.p3(name, parse_p3(dt));
                s.dat.bd3(p.to_array());
            }
            VarKind::Tv => {
                let default = dt.trim_matches('"');
                s.strs.text16(&hv.text(name, default));
            }
            VarKind::Cmc => {
                let fallback = match name {
                    "CECOLOR" => Color::ByLayer,
                    "INTERFERECOLOR" => Color::Index(1),
                    _ => Color::ByBlock,
                };
                let c = hv.color(name, fallback);
                s.dat.bs(0);
                s.dat.bl(c.to_method_word());
                s.dat.rc(0);
            }
            VarKind::H => {
                // The tables' control objects and the named-object
                // dictionary are owned (code 3); the rest are pointers.
                let code = if name.ends_with("_CONTROL") || name == "DICTIONARY_NAMED_OBJECTS" {
                    3
                } else {
                    5
                };
                s.hdl.handle(code, hv.handle(name).0);
            }
            VarKind::Seed => s.dat.handle(0, handseed),
            VarKind::Time => {
                let (day, ms) = julian(hv.time(name, 0.0));
                s.dat.bl(day);
                s.dat.bl(ms);
            }
        }
    }
    // The spec's trailing "R14+" shorts (types 5 and 6 only) are not
    // written by AutoCAD 2027.
    let body = s.body(false);
    wrap(&HEADER_START, &HEADER_END, &body, ver)
}

fn parse_p2(t: &str) -> Vec2 {
    let v: Vec<f64> = t
        .trim_matches(|c| c == '(' || c == ')')
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    Vec2::new(v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0))
}

fn parse_p3(t: &str) -> Vec3 {
    let v: Vec<f64> = t
        .trim_matches(|c| c == '(' || c == ')')
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    Vec3::new(
        v.first().copied().unwrap_or(0.0),
        v.get(1).copied().unwrap_or(0.0),
        v.get(2).copied().unwrap_or(0.0),
    )
}

/// AcDb:Classes for R2004+.
pub(crate) fn classes_section(d: &Drawing, ver: DwgVersion) -> Vec<u8> {
    let mut s = Streams3::new();
    let max = d.classes.iter().map(|c| c.number).max().unwrap_or(499);
    s.dat.bs(max);
    s.dat.rc(0);
    s.dat.rc(0);
    s.dat.b(true);
    for c in &d.classes {
        s.dat.bs(c.number);
        s.dat.bs(c.proxy_flags);
        s.strs.text16(&c.app_name);
        s.strs.text16(&c.cpp_name);
        s.strs.text16(&c.dxf_name);
        s.dat.b(c.was_zombie);
        s.dat.bs(c.item_class_id);
        let count = d.objects.values().filter(|o| o.type_code == c.number).count() as u32;
        s.dat.bl(c.instance_count.max(count));
        let (dwg, maint) = if c.dwg_version != 0 {
            (c.dwg_version, c.maintenance_version)
        } else {
            class_versions(&c.dxf_name)
        };
        s.dat.bl(dwg);
        s.dat.bl(maint);
        s.dat.bl(0);
        s.dat.bl(0);
    }
    let body = s.body(true);
    let mut out = wrap(&CLASSES_START, &CLASSES_END, &body, ver);
    out.extend_from_slice(&[0u8; 8]);
    out
}

/// The DWG and maintenance version a class says it came with, for
/// drawings saved before the native format kept them (2.0): the values
/// AutoCAD 2027 writes for the classes of its own drawings, 0 otherwise.
fn class_versions(dxf_name: &str) -> (u32, u32) {
    match dxf_name {
        "ACDBDICTIONARYWDFLT" => (22, 42),
        "MATERIAL" => (23, 14),
        "VISUALSTYLE" => (26, 0),
        "TABLESTYLE" => (25, 20),
        "SCALE" => (27, 1),
        "MLEADERSTYLE" => (33, 427),
        "ACDBSECTIONVIEWSTYLE" => (27, 50),
        "ACDBDETAILVIEWSTYLE" => (33, 427),
        "DIMASSOC" => (23, 12),
        "DICTIONARYVAR" => (20, 0),
        _ => (0, 0),
    }
}

/// AcDb:Handles: (handle, offset) pairs in handle order, as modular-char
/// deltas, in blocks of at most 2032 bytes, each with a big-endian size
/// and CRC, ended by an empty block. Deltas restart in every block.
pub(crate) fn handles_section(entries: &[(u64, u64)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        let mut block: Vec<u8> = vec![0, 0];
        let (mut last_h, mut last_o) = (0u64, 0i64);
        while i < entries.len() {
            let (h, o) = entries[i];
            let mut w = BitWriter::new();
            w.mc_unsigned(h - last_h);
            w.mc_signed(o as i64 - last_o);
            let bytes = w.into_bytes();
            if block.len() + bytes.len() > 2032 {
                break;
            }
            block.extend_from_slice(&bytes);
            last_h = h;
            last_o = o as i64;
            i += 1;
        }
        let n = block.len() as u16;
        block[..2].copy_from_slice(&n.to_be_bytes());
        let crc = crc16(0xC0C1, &block);
        out.extend_from_slice(&block);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let end = [0u8, 2];
    let crc = crc16(0xC0C1, &end);
    out.extend_from_slice(&end);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

/// TU as the R2007+ small sections store it: RS length counting the
/// terminating 0, then UTF-16 units with that 0.
fn tu(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().filter(|&u| u != 0).chain([0]).collect();
    out.extend_from_slice(&(units.len() as u16).to_le_bytes());
    for u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
}

fn td(out: &mut Vec<u8>, v: f64) {
    let (day, ms) = julian(v);
    out.extend_from_slice(&day.to_le_bytes());
    out.extend_from_slice(&ms.to_le_bytes());
}

/// The maintenance release AutoCAD 2027 writes for AC1032 (file header,
/// auxiliary header, class records), whose object layout the writer
/// follows bit for bit.
pub(crate) const ACAD_MAINTENANCE: u32 = 427;

/// AcDb:AuxHeader (chapter 27) as AutoCAD 2027 writes it for AC1032: the
/// version and maintenance release three times, the save count, the
/// creation and update times, the handle seed. The spec's 5 and 0x893
/// shorts are zero in AutoCAD's file, and bytes 0x3D–0x44 hold values the
/// spec lists as zero; both are written as AutoCAD writes them.
pub(crate) fn aux_header(d: &Drawing, handseed: u64, saves: u32) -> Vec<u8> {
    let hv = Hv::new(d);
    let ver: u16 = 33;
    let maint = ACAD_MAINTENANCE;
    // AutoCAD starts the section 0xFF on a drawing's first save, 0x00 on
    // later ones.
    let mut o = vec![if saves <= 1 { 0xFF } else { 0x00 }, 0x77, 0x01];
    o.extend_from_slice(&ver.to_le_bytes());
    o.extend_from_slice(&maint.to_le_bytes());
    o.extend_from_slice(&saves.to_le_bytes());
    o.extend_from_slice(&(-1i32).to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&0u16.to_le_bytes());
    o.extend_from_slice(&0u32.to_le_bytes());
    for _ in 0..2 {
        o.extend_from_slice(&ver.to_le_bytes());
        o.extend_from_slice(&maint.to_le_bytes());
    }
    o.extend_from_slice(&[0; 8]);
    o.extend_from_slice(&0u16.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&[0; 12]);
    o.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x01, 0x00]);
    o.extend_from_slice(&(saves as u16).to_le_bytes());
    td(&mut o, hv.time("TDCREATE", 0.0));
    td(&mut o, hv.time("TDUPDATE", 0.0));
    let seed = if handseed < 0x7FFF_FFFF { handseed as u32 } else { u32::MAX };
    o.extend_from_slice(&seed.to_le_bytes());
    o.extend_from_slice(&0u32.to_le_bytes()); // educational plot stamp
    o.extend_from_slice(&0u16.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&[0; 12]);
    o.extend_from_slice(&saves.to_le_bytes());
    o.extend_from_slice(&[0; 16]);
    debug_assert_eq!(o.len(), 129);
    o
}

/// AcDb:Template (chapter 22): AutoCAD 2027 writes four bytes `01 00 00
/// 00`, then MEASUREMENT as a short.
pub(crate) fn template(d: &Drawing) -> Vec<u8> {
    let m = Hv::new(d).int("MEASUREMENT", 0) as u16;
    let mut o = vec![1, 0, 0, 0];
    o.extend_from_slice(&m.to_le_bytes());
    o
}

/// AcDb:ObjFreeSpace (chapter 21) in its R2004+ form as AutoCAD writes
/// it: 64-bit zero, the approximate object count, TDUPDATE, then the four
/// 64-bit value pairs the spec lists.
pub(crate) fn obj_free_space(d: &Drawing, objects: usize) -> Vec<u8> {
    let mut o = Vec::with_capacity(89);
    o.extend_from_slice(&0u64.to_le_bytes());
    o.extend_from_slice(&(objects as u64).to_le_bytes());
    td(&mut o, Hv::new(d).time("TDUPDATE", 0.0));
    o.push(4);
    for v in [0x32u64, 0x64, 0x200, 0xFFFF_FFFF] {
        o.extend_from_slice(&v.to_le_bytes());
        o.extend_from_slice(&0u64.to_le_bytes());
    }
    o
}

/// AcDb:RevHistory (chapter 18), as AutoCAD 2027 writes it for a drawing
/// with no revision records.
pub(crate) fn rev_history() -> Vec<u8> {
    [0u32, 0, 1, 0].iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// AcDb:SummaryInfo (chapter 13), R2007+ strings: the drawing
/// properties, total editing time, creation and update times, custom
/// properties.
pub(crate) fn summary_info(d: &Drawing) -> Vec<u8> {
    let hv = Hv::new(d);
    let p = &d.header.properties;
    let mut o = Vec::new();
    for s in [
        &p.title,
        &p.subject,
        &p.author,
        &p.keywords,
        &p.comments,
        &p.last_saved_by,
        &p.revision,
        &p.hyperlink_base,
    ] {
        tu(&mut o, s);
    }
    td(&mut o, hv.time("TDINDWG", 0.0));
    td(&mut o, hv.time("TDCREATE", 0.0));
    td(&mut o, hv.time("TDUPDATE", 0.0));
    o.extend_from_slice(&(p.custom.len() as u16).to_le_bytes());
    for (k, v) in &p.custom {
        tu(&mut o, k);
        tu(&mut o, v);
    }
    o.extend_from_slice(&[0; 8]);
    o
}

/// The kinds of entry in an application information list, each written
/// before its text. The spec calls these 16 bytes a checksum that may be
/// zero; in AutoCAD's files they are fixed identifiers, the same in every
/// file of the corpus whatever the text (the property set's is the format
/// id its XML names).
const APPINFO_COMMENT: [u8; 16] = [
    0xB8, 0xD0, 0xF0, 0x25, 0xA1, 0xD7, 0x93, 0x49, 0xB2, 0xFA, 0x9B, 0xF9, 0x28, 0x6F, 0xA1, 0xFD,
];
const APPINFO_PROPERTIES: [u8; 16] = [
    0xE0, 0x85, 0x9F, 0xF2, 0xF9, 0x4F, 0x68, 0x10, 0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3, 0xD9,
];
const APPINFO_PRODUCT: [u8; 16] = [
    0xE8, 0xE0, 0x96, 0x51, 0xC5, 0xCE, 0xB2, 0x44, 0xA8, 0xBF, 0xF6, 0xE8, 0x3B, 0x85, 0x9D, 0x44,
];
/// The 32 bytes AcDb:AppInfoHistory starts with, the same in every file of
/// the corpus (2004 to 2018, every save).
const APPINFO_HISTORY_START: [u8; 32] = [
    0x53, 0xDE, 0x38, 0x1D, 0xEC, 0x43, 0x21, 0xCA, 0x96, 0x19, 0xE1, 0xE2, 0x17, 0x1A, 0x2A, 0x67,
    0x3B, 0xD9, 0x7F, 0xF7, 0x3C, 0xBB, 0xCE, 0x08, 0xA0, 0x53, 0xD8, 0xED, 0xD2, 0x8D, 0xC5, 0xC7,
];

fn appinfo_comment(name: &str, version: &str) -> String {
    format!("Saved by {name} {version}.")
}

fn appinfo_product(name: &str, version: &str) -> String {
    let registry: String = version.split('.').take(2).collect::<Vec<_>>().join(".");
    format!(
        "<ProductInformation name =\"{name}\" build_version=\"{version}\" registry_version=\"{registry}\" install_id_string=\"{name}\" registry_localeID=\"1033\"/>"
    )
}

/// AcDb:AppInfo (chapter 16) in the layout AutoCAD 2027 writes: format 3,
/// "AppInfoDataList", two entries (a comment and the product
/// information), each after the identifier of its kind. It names the
/// program writing the file, never claims to be an Autodesk application.
pub(crate) fn app_info(name: &str, version: &str) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend_from_slice(&3u32.to_le_bytes());
    tu(&mut o, "AppInfoDataList");
    o.extend_from_slice(&2u32.to_le_bytes());
    o.extend_from_slice(&APPINFO_COMMENT);
    tu(&mut o, &appinfo_comment(name, version));
    o.extend_from_slice(&APPINFO_PRODUCT);
    tu(&mut o, &appinfo_product(name, version));
    o
}

/// A Julian date and time as `YYYY-MM-DDTHH:MM:SS`.
fn iso_datetime(v: f64) -> String {
    let (day, ms) = julian(v);
    // Days since 0000-03-01, then the civil calendar.
    let z = day as i64 - 1_721_120;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let s = ms / 1000;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// AcDb:AppInfoHistory (not in the spec), as AutoCAD 2027 writes it: the
/// fixed 32 bytes, a zero long, then an application information list of
/// three entries: the comment, a property set (on a drawing's first save:
/// creation and save times, an empty string, who saved it, the program
/// and its version; later saves leave out the creation time and the empty
/// string) and the product information.
pub(crate) fn app_info_history(d: &Drawing, first_save: bool, name: &str, version: &str) -> Vec<u8> {
    let hv = Hv::new(d);
    let mut props = String::from("<prop_set fmt_id=\"{f29f85e0-4ff9-1068-ab91-08002b27b3d9}\">");
    let saved = iso_datetime(hv.time("TDUPDATE", 0.0));
    let by = xml_text(&d.header.properties.last_saved_by);
    if first_save {
        let created = iso_datetime(hv.time("TDCREATE", 0.0));
        props += &format!("<prop id=\"12\"><datetime>{created}</datetime></prop>");
        props += &format!("<prop id=\"10\"><datetime>{saved}</datetime></prop>");
        props += "<prop id=\"257\"><string></string></prop>";
        props += &format!("<prop id=\"8\"><string>{by}</string></prop>");
    } else {
        props += &format!("<prop id=\"8\"><string>{by}</string></prop>");
        props += &format!("<prop id=\"10\"><datetime>{saved}</datetime></prop>");
    }
    props += &format!("<prop id=\"258\"><string>{name}</string></prop>");
    props += &format!("<prop id=\"259\"><string>{version}</string></prop>");
    props += "</prop_set>";
    let mut o = APPINFO_HISTORY_START.to_vec();
    o.extend_from_slice(&0u32.to_le_bytes());
    tu(&mut o, "AppInfoDataList");
    o.extend_from_slice(&3u32.to_le_bytes());
    o.extend_from_slice(&APPINFO_COMMENT);
    tu(&mut o, &appinfo_comment(name, version));
    o.extend_from_slice(&APPINFO_PROPERTIES);
    tu(&mut o, &props);
    o.extend_from_slice(&APPINFO_PRODUCT);
    tu(&mut o, &appinfo_product(name, version));
    o
}

fn json_text(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o += "\\\"",
            '\\' => o += "\\\\",
            c if (c as u32) < 0x20 => o += &format!("\\u{:04x}", c as u32),
            c => o.push(c),
        }
    }
    o
}

/// AcDb:XrefManifest (not in the spec), as AutoCAD 2027 writes it: a
/// 64-bit length, then JSON listing the files the drawing refers to (the
/// font of each text style, by the style's handle) with four-space
/// indents.
pub(crate) fn xref_manifest(d: &Drawing) -> Vec<u8> {
    let mut entries = Vec::new();
    for o in d.objects.values() {
        if let ObjectKind::TextStyle(t) = &o.kind {
            if !t.font_file.is_empty() {
                entries.push(format!(
                    "        {{\n            \"path\": \"{}\",\n            \"handle\": \"{:x}\",\n            \"class\": \"AcDbTextStyleTableRecord\"\n        }}",
                    json_text(&t.font_file),
                    o.handle.0
                ));
            }
        }
    }
    let list = if entries.is_empty() {
        "[]".to_string()
    } else {
        format!("[\n{}\n    ]", entries.join(",\n"))
    };
    let json = format!(
        "{{\n    \"xrefs\": {list},\n    \"version\": \"1.0\",\n    \"wasFullSave\": \"true\"\n}}\n"
    );
    let mut o = (json.len() as u64).to_le_bytes().to_vec();
    o.extend_from_slice(json.as_bytes());
    o
}

const PREVIEW_START: [u8; 16] = [
    0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D, 0x44, 0x10, 0x2B,
];
const PREVIEW_END: [u8; 16] = [
    0xE0, 0xDA, 0x92, 0xF8, 0x2B, 0xC9, 0xD7, 0xD7, 0x62, 0xA8, 0x35, 0xC0, 0x62, 0xBB, 0xEF, 0xD4,
];

/// AcDb:Preview (chapter 14.2) as AutoCAD 2027 writes it for R2013+: a
/// header entry (code 1, 80 zero bytes) and a PNG (code 6). The entries
/// give their data's position as an address in the file, so the caller
/// passes where the section's first byte will be.
pub(crate) fn preview(png: &[u8], section_address: u64) -> Vec<u8> {
    let header = [0u8; 80];
    let entries = 2usize;
    let head = 16 + 4 + 1 + entries * 9;
    let header_at = section_address + head as u64;
    let png_at = header_at + header.len() as u64;
    let mut o = PREVIEW_START.to_vec();
    let size = 1 + entries * 9 + header.len() + png.len();
    o.extend_from_slice(&(size as u32).to_le_bytes());
    o.push(entries as u8);
    o.push(1);
    o.extend_from_slice(&(header_at as u32).to_le_bytes());
    o.extend_from_slice(&(header.len() as u32).to_le_bytes());
    o.push(6);
    o.extend_from_slice(&(png_at as u32).to_le_bytes());
    o.extend_from_slice(&(png.len() as u32).to_le_bytes());
    o.extend_from_slice(&header);
    o.extend_from_slice(png);
    o.extend_from_slice(&PREVIEW_END);
    o
}

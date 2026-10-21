//! The writer against AutoCAD: every object of the MI-201 corpus drawing
//! (AutoCAD 2027, AC1032) that the model holds whole is encoded again and
//! compared with AutoCAD's bytes.

use super::super::nongraphic::{self, DimVars, NonGraphic, Vport};
use super::super::r2004;
use super::super::version::DwgVersion;
use super::objects::{self, Ctx};
use crate::model::*;
use std::collections::BTreeMap;

fn corpus() -> Option<Vec<u8>> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/mosaiciron-mi201/mi201-r2018.dwg");
    std::fs::read(p).ok()
}

/// The object's bytes in the file: size fields, data and CRC.
fn original(section: &[u8], offset: usize) -> Vec<u8> {
    let mut r = super::super::bits::BitReader::new(section);
    r.set_position(offset * 8).unwrap();
    let size = r.ms().unwrap() as usize;
    let _h = r.mc_unsigned().unwrap();
    let start = r.position() / 8;
    section[offset..start + size + 2].to_vec()
}

fn first_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    let n = a.len().min(b.len());
    for i in 0..n * 8 {
        let (x, y) = (a[i / 8] >> (7 - i % 8) & 1, b[i / 8] >> (7 - i % 8) & 1);
        if x != y {
            return Some(i);
        }
    }
    (a.len() != b.len()).then_some(n * 8)
}

/// Encode every whole-model object of the AutoCAD file and compare.
/// Returns (matched, mismatched descriptions).
fn compare_all(d: &Drawing, section: &[u8]) -> (usize, Vec<String>) {
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
    let dv = |o: &Object| -> DimVars {
        match nongraphic::dimstyle(&d.objects[&o.handle], &d.classes, d.codepage) {
            Some(Ok((_, v))) => v,
            _ => super::super::nongraphic::dim_vars_from_header(&super::super::hvars::Hv::new(d)),
        }
    };
    let vp = |o: &Object| -> Vport {
        match nongraphic::vport(&d.objects[&o.handle], &d.classes, d.codepage) {
            Some(Ok(v)) => v,
            _ => Vport::looking_at(Default::default(), Default::default()),
        }
    };
    let ctx = Ctx {
        d,
        appids: &appids,
        plot_style: Handle(0x3D095),
        material: Handle(0x3D0A7),
        dimstyle_vars: &dv,
        vport: &vp,
        text_width: &super::estimated_text_width,
        hatch_lines: None,
    };
    let mut matched = 0;
    let mut bad = Vec::new();
    for o in d.objects.values() {
        // Encode from the model even where the reader kept the record.
        let mut o2 = o.clone();
        o2.raw = None;
        let ours = match objects::encode(&o2, &ctx, DwgVersion::R2018) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(e) => {
                bad.push(format!("{:X} {}: {e}", o.handle.0, o.type_name));
                continue;
            }
        };
        let theirs = original(section, o.offset);
        if ours == theirs {
            matched += 1;
        } else {
            let at = first_difference(&ours, &theirs).unwrap_or(0);
            let tail = |b: &[u8]| -> String {
                let n = b.len();
                if n < 3 { return String::new(); }
                format!("{:08b}", b[n - 3])
            };
            bad.push(format!(
                "{:X} {} ({} vs {} bytes): first difference at bit {at} (byte {}) last data byte ours {} acad {}\n  ours  {:02x?}\n  acad  {:02x?}",
                o.handle.0,
                o.type_name,
                ours.len(),
                theirs.len(),
                at / 8,
                tail(&ours),
                tail(&theirs),
                &ours[..ours.len().min(96)],
                &theirs[..theirs.len().min(96)]
            ));
        }
    }
    (matched, bad)
}

#[test]
fn objects_encode_to_autocads_bytes() {
    let Some(file) = corpus() else { return };
    let d = super::super::read_bytes(&file).unwrap();
    let c = r2004::read(&file, DwgVersion::R2018).unwrap();
    let (matched, bad) = compare_all(&d, &c.objects);
    for b in &bad {
        eprintln!("{b}");
    }
    eprintln!("{matched} objects identical, {} different", bad.len());
    let _ = NonGraphic::Placeholder;
    // MTEXT stores the extents of its text as AutoCAD's fonts measure it;
    // ours come from this program's font metrics, so those two values
    // differ. Everything else must be AutoCAD's bytes.
    let unexpected: Vec<&String> = bad
        .iter()
        .filter(|b| b.split_whitespace().nth(1).is_none_or(|t| t != "MTEXT"))
        .collect();
    assert!(unexpected.is_empty(), "{unexpected:#?}");
    assert!(matched >= 150, "only {matched} objects compared");
}

/// Both encodings of chosen objects, bit by bit, for finding where they
/// part: `DUMP=9,3 cargo test dump_bits -- --ignored --nocapture`.
#[test]
#[ignore]
fn dump_bits() {
    let Some(file) = corpus() else { return };
    let d = super::super::read_bytes(&file).unwrap();
    let c = r2004::read(&file, DwgVersion::R2018).unwrap();
    let want: Vec<u64> = std::env::var("DUMP")
        .unwrap_or_default()
        .split(',')
        .filter_map(|h| u64::from_str_radix(h.trim(), 16).ok())
        .collect();
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
    let dv = |o: &Object| match nongraphic::dimstyle(&d.objects[&o.handle], &d.classes, d.codepage) {
        Some(Ok((_, v))) => v,
        _ => super::super::nongraphic::dim_vars_from_header(&super::super::hvars::Hv::new(&d)),
    };
    let vp = |o: &Object| match nongraphic::vport(&d.objects[&o.handle], &d.classes, d.codepage) {
        Some(Ok(v)) => v,
        _ => Vport::looking_at(Default::default(), Default::default()),
    };
    let ctx = Ctx {
        d: &d,
        appids: &appids,
        plot_style: Handle(0x3D095),
        material: Handle(0x3D0A7),
        dimstyle_vars: &dv,
        vport: &vp,
        text_width: &super::estimated_text_width,
        hatch_lines: None,
    };
    let bits = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:08b}")).collect() };
    for h in want {
        let o = &d.objects[&Handle(h)];
        let mut o2 = o.clone();
        o2.raw = None;
        let ours = objects::encode(&o2, &ctx, DwgVersion::R2018).unwrap().unwrap();
        let theirs = original(&c.objects, o.offset);
        let (a, b) = (bits(&ours), bits(&theirs));
        println!("== {h:X} {}", o.type_name);
        for i in (0..a.len().max(b.len())).step_by(64) {
            let sa = a.get(i..(i + 64).min(a.len())).unwrap_or("");
            let sb = b.get(i..(i + 64).min(b.len())).unwrap_or("");
            println!("{i:5} ours {sa}\n      acad {sb}{}", if sa != sb { "   <<" } else { "" });
        }
    }
}

/// Every field of AutoCAD's MTEXT records, for matching what it stores
/// beyond the model.
#[test]
#[ignore]
fn probe_mtext_fields() {
    let Some(file) = corpus() else { return };
    let d = super::super::read_bytes(&file).unwrap();
    let c = r2004::read(&file, DwgVersion::R2018).unwrap();
    for h in [0x3D103u64, 0x3D11D] {
        let o = &d.objects[&Handle(h)];
        let ctx = super::super::object::DecodeCtx { ver: DwgVersion::R2018, codepage: 30, classes: &d.classes };
        let mut op = super::super::object::open(&ctx, &c.objects, o.offset).unwrap();
        let s = &mut op.s;
        let ins = s.dat.bd3().unwrap();
        let ext = s.dat.bd3().unwrap();
        let xax = s.dat.bd3().unwrap();
        let rw = s.dat.bd().unwrap();
        let rh = s.dat.bd().unwrap();
        let th = s.dat.bd().unwrap();
        let att = s.dat.bs().unwrap();
        let dir = s.dat.bs().unwrap();
        let eh = s.dat.bd().unwrap();
        let ew = s.dat.bd().unwrap();
        let text = s.tv().unwrap();
        let lss = s.dat.bs().unwrap();
        let lsf = s.dat.bd().unwrap();
        let unk = s.dat.b().unwrap();
        let bg = s.dat.bl().unwrap();
        println!("{h:X}: ins {ins:?} ext {ext:?} x {xax:?} rect {rw} x {rh} height {th} att {att} dir {dir} extents {eh} x {ew} text {text:?} ls {lss} {lsf} unk {unk} bg {bg}");
        let not_annot = s.dat.b().unwrap();
        let ver = s.dat.bs().unwrap();
        let def = s.dat.b().unwrap();
        let att2 = s.dat.bl().unwrap();
        let x2 = s.dat.bd3().unwrap();
        let ins2 = s.dat.bd3().unwrap();
        let (a, b, cc, dd) = (s.dat.bd().unwrap(), s.dat.bd().unwrap(), s.dat.bd().unwrap(), s.dat.bd().unwrap());
        let cols = s.dat.bs().unwrap();
        println!("   not_annot {not_annot} ver {ver} default {def} att {att2} x {x2:?} ins {ins2:?} rect/ext {a} {b} {cc} {dd} columns {cols}; {} bits left", s.dat.remaining());
    }
}

fn compare_bytes(label: &str, ours: &[u8], theirs: &[u8]) -> bool {
    if ours == theirs {
        eprintln!("{label}: identical ({} bytes)", ours.len());
        return true;
    }
    let at = first_difference(ours, theirs).unwrap_or(0);
    eprintln!(
        "{label}: {} vs {} bytes, first difference at bit {at} (byte {})\n  ours {:02x?}\n  acad {:02x?}",
        ours.len(),
        theirs.len(),
        at / 8,
        &ours[(at / 8).saturating_sub(8)..(at / 8 + 24).min(ours.len())],
        &theirs[(at / 8).saturating_sub(8)..(at / 8 + 24).min(theirs.len())]
    );
    false
}

#[test]
fn sections_encode_to_autocads_bytes() {
    let Some(file) = corpus() else { return };
    let d = super::super::read_bytes(&file).unwrap();
    let c = r2004::read(&file, DwgVersion::R2018).unwrap();
    let seed = d.header.handseed.unwrap();
    let h = super::sections::header_section(&d, DwgVersion::R2018, seed);
    // AutoCAD 2027 ends its header with 69 bits after the handles that the
    // spec does not describe; ours stops at the handles. Up to there the
    // two must agree: the variables bit for bit and every handle.
    let ok_h = header_parts(&h) == header_parts(c.header.as_ref().unwrap());
    let cl = super::sections::classes_section(&d, DwgVersion::R2018);
    let ok_c = compare_bytes("classes", &cl, c.classes.as_ref().unwrap());
    let entries: Vec<(u64, u64)> = d.objects.values().map(|o| (o.handle.0, o.offset as u64)).collect();
    let hs = super::sections::handles_section(&entries);
    let ok_m = compare_bytes("handles", &hs, &c.handles);
    assert!(ok_h && ok_c && ok_m);
}

/// A header section's variables (the bytes before its handle stream) and
/// its handle references.
fn header_parts(s: &[u8]) -> (Vec<u8>, Vec<(u8, u64)>) {
    use super::super::bits::BitReader;
    let size = u32::from_le_bytes(s[16..20].try_into().unwrap()) as usize;
    let body = &s[24..24 + size];
    let endbit = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    let mut r = BitReader::with_bounds(body, endbit, size * 8);
    let mut refs = Vec::new();
    while r.remaining() >= 8 {
        match r.handle() {
            Ok(h) if h.code <= 5 && refs.len() < 64 => refs.push((h.code, h.value)),
            _ => break,
        }
    }
    (body[4..endbit / 8].to_vec(), refs)
}

#[test]
#[ignore]
fn probe_header_handles() {
    use super::super::bits::BitReader;
    let Some(file) = corpus() else { return };
    let d = super::super::read_bytes(&file).unwrap();
    let c = r2004::read(&file, DwgVersion::R2018).unwrap();
    let ours = super::sections::header_section(&d, DwgVersion::R2018, d.header.handseed.unwrap());
    for (label, s) in [("ours", &ours), ("acad", c.header.as_ref().unwrap())] {
        let size = u32::from_le_bytes(s[16..20].try_into().unwrap()) as usize;
        let body = &s[24..24 + size];
        let endbit = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
        let mut r = BitReader::with_bounds(body, endbit, size * 8);
        let mut refs = Vec::new();
        while r.remaining() >= 8 {
            match r.handle() {
                Ok(h) => refs.push(format!("{:X}.{:X}", h.code, h.value)),
                Err(_) => break,
            }
        }
        println!("{label}: endbit {endbit}, {} refs: {}; {} bits left", refs.len(), refs.join(" "), r.remaining());
        let mut tail = String::new();
        let mut t = r.clone();
        while t.remaining() > 0 {
            tail.push(if t.b().unwrap() { '1' } else { '0' });
        }
        println!("   tail bits {tail}; body bytes after endbit/8: {:02x?}", &body[endbit / 8..]);
        println!("   main data same: {}", body[..endbit / 8] == ours[24..24 + endbit / 8]);
    }
}

#[test]
#[ignore]
fn probe_class_records() {
    use super::super::bits::BitReader;
    use super::super::streams::{string_stream, Streams};
    let Some(file) = corpus() else { return };
    let c = r2004::read(&file, DwgVersion::R2018).unwrap();
    let sec = c.classes.as_ref().unwrap();
    let size = u32::from_le_bytes(sec[16..20].try_into().unwrap()) as usize;
    let body = &sec[24..24 + size];
    let endbit = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    let (strs, _) = string_stream(body, endbit).unwrap().unwrap();
    let mut s = Streams { ver: DwgVersion::R2018, codepage: 30, dat: BitReader::with_bounds(body, 32, endbit), strs: Some(strs), hdl: None };
    println!("max {} rc {} rc {} b {}", s.dat.bs().unwrap(), s.dat.rc().unwrap(), s.dat.rc().unwrap(), s.dat.b().unwrap());
    for _ in 0..10 {
        let num = s.dat.bs().unwrap();
        let proxy = s.dat.bs().unwrap();
        let (a, b, cc) = (s.tv().unwrap(), s.tv().unwrap(), s.tv().unwrap());
        let z = s.dat.b().unwrap();
        let id = s.dat.bs().unwrap();
        let n = s.dat.bl().unwrap();
        let (dv, mv, u1, u2) = (s.dat.bl().unwrap(), s.dat.bl().unwrap(), s.dat.bl().unwrap(), s.dat.bl().unwrap());
        println!("{num} {proxy} {a:?} {b:?} {cc:?} zombie {z} id {id:#x} count {n} dwg {dv} maint {mv} unk {u1} {u2}");
    }
    println!("{} bits left before strings", s.dat.remaining());
}

/// A drawing read from the DXF this program writes encodes to the same
/// DWG objects as the drawing the DXF came from, for every object DXF
/// carries (MTEXT aside, whose extents are measured again).
#[test]
fn objects_from_dxf_encode_as_objects_from_dwg() {
    let Some(file) = corpus() else { return };
    let mut d = super::super::read_bytes(&file).unwrap();
    let dxf = cad_dxf::encode::to_ascii(&cad_dxf::groups(&d).0);
    let d2 = cad_dxf::read_bytes(&dxf).unwrap();
    // References to objects DXF does not carry (visual styles, materials,
    // dimension associations) are not expected back.
    let kept: std::collections::BTreeSet<Handle> = d2.objects.keys().copied().collect();
    for o in d.objects.values_mut() {
        o.reactors.retain(|h| kept.contains(h));
        if !kept.contains(&o.xdictionary) {
            o.xdictionary = Handle::NULL;
        }
        match &mut o.kind {
            ObjectKind::NonGraphic(NonGraphic::Dictionary(dc)) => dc.entries.retain(|(_, h)| kept.contains(h)),
            ObjectKind::Viewport(v) if !kept.contains(&v.visual_style) => v.visual_style = Handle::NULL,
            _ => {}
        }
    }
    let a: BTreeMap<u64, Vec<u8>> = super::encode_objects(&d, DwgVersion::R2018, &super::WriteOptions::default()).objects.into_iter().collect();
    let b: BTreeMap<u64, Vec<u8>> = super::encode_objects(&d2, DwgVersion::R2018, &super::WriteOptions::default()).objects.into_iter().collect();
    let mut same = 0;
    let mut bad = Vec::new();
    for (h, bytes) in &b {
        let o = &d2.objects[&Handle(*h)];
        // DXF gives angles in degrees: an angle can come back a few units
        // in the last place off.
        let close = match (&d.objects[&Handle(*h)].kind, &o.kind) {
            (ObjectKind::Arc(p), ObjectKind::Arc(q)) => {
                (p.start_angle - q.start_angle).abs() < 1e-12
                    && (p.end_angle - q.end_angle).abs() < 1e-12
                    && Arc { start_angle: 0.0, end_angle: 0.0, ..p.clone() }
                        == Arc { start_angle: 0.0, end_angle: 0.0, ..q.clone() }
            }
            _ => false,
        };
        match a.get(h) {
            Some(x) if x == bytes || close => same += 1,
            Some(x) => {
                let mut o1 = d.objects[&Handle(*h)].clone();
                let mut o2 = o.clone();
                for q in [&mut o1, &mut o2] {
                    q.offset = 0;
                    q.size = 0;
                    q.raw = None;
                }
                let (t1, t2) = (format!("{o1:?}"), format!("{o2:?}"));
                let at = t1.chars().zip(t2.chars()).position(|(a, b)| a != b).unwrap_or(t1.len().min(t2.len()));
                let from = at.saturating_sub(60);
                bad.push(format!(
                    "{h:X} {}: differs at bit {:?}\n  dwg ...{}\n  dxf ...{}",
                    o.type_name,
                    first_difference(bytes, x),
                    t1.chars().skip(from).take(140).collect::<String>(),
                    t2.chars().skip(from).take(140).collect::<String>(),
                ))
            }
            None => bad.push(format!("{h:X} {}: not in the DWG", o.type_name)),
        }
    }
    let missing: Vec<String> = a
        .keys()
        .filter(|h| !b.contains_key(h))
        .map(|h| format!("{h:X} {}", d.objects[&Handle(*h)].type_name))
        .collect();
    for x in &bad {
        eprintln!("{x}");
    }
    eprintln!("{same} identical, {} different; not from DXF: {}", bad.len(), missing.join(", "));
    let unexpected: Vec<&String> = bad
        .iter()
        .filter(|b| !b.split_whitespace().nth(1).is_some_and(|t| t.starts_with("MTEXT")))
        .collect();
    assert!(unexpected.is_empty());
}

/// Files that tell apart where AutoCAD objects to a written DWG: AutoCAD's
/// own MI-201 sections packed by our container (T0), then one part of ours
/// at a time in their place (T1–T5), and T0 with every page stored as one
/// literal run (T6). `OUT=dir cargo test diagnostic_files -- --ignored`.
#[test]
#[ignore]
fn diagnostic_files() {
    use super::container::{pack, Section, MAX_PAGE};
    let Ok(out) = std::env::var("OUT") else { return };
    let Some(file) = corpus() else { return };
    let d = super::super::read_bytes(&file).unwrap();
    let c = r2004::read(&file, DwgVersion::R2018).unwrap();
    let other = |n: &str| c.others.iter().find(|(k, _)| k == n).map(|(_, b)| b.clone()).unwrap();
    let raw = |name: &'static str, data: Vec<u8>, compressed: bool, page_size: usize| Section { name, data, compressed, page_size };
    let acad = || {
        vec![
            raw("AcDb:SummaryInfo", other("AcDb:SummaryInfo"), false, 0x80),
            raw("AcDb:Preview", other("AcDb:Preview"), false, 0x800),
            raw("AcDb:AppInfo", other("AcDb:AppInfo"), false, 0x300),
            raw("AcDb:AppInfoHistory", other("AcDb:AppInfoHistory"), false, 0x580),
            raw("AcDb:XrefManifest", other("AcDb:XrefManifest"), true, MAX_PAGE),
            raw("AcDb:RevHistory", other("AcDb:RevHistory"), true, MAX_PAGE),
            raw("AcDb:AcDbObjects", c.objects.clone(), true, MAX_PAGE),
            raw("AcDb:ObjFreeSpace", other("AcDb:ObjFreeSpace"), true, MAX_PAGE),
            raw("AcDb:Template", other("AcDb:Template"), true, MAX_PAGE),
            raw("AcDb:Handles", c.handles.clone(), true, MAX_PAGE),
            raw("AcDb:Classes", c.classes.clone().unwrap(), true, MAX_PAGE),
            raw("AcDb:AuxHeader", other("AcDb:AuxHeader"), true, MAX_PAGE),
            raw("AcDb:Header", c.header.clone().unwrap(), true, MAX_PAGE),
        ]
    };
    let acad_order = [
        "AcDb:XrefManifest", "AcDb:AppInfoHistory", "AcDb:AppInfo", "AcDb:Preview",
        "AcDb:SummaryInfo", "AcDb:RevHistory", "AcDb:AcDbObjects", "AcDb:ObjFreeSpace",
        "AcDb:Template", "AcDb:Handles", "AcDb:Classes", "AcDb:AuxHeader", "AcDb:Header",
    ];
    let (ours, _, _) = super::stream_sections(&d, &super::WriteOptions::default()).unwrap();
    let ours_named = |n: &str| ours.iter().find(|s| s.name == n).unwrap();
    let replace = |mut v: Vec<Section>, names: &[&'static str]| {
        for s in &mut v {
            if names.contains(&s.name) {
                let o = ours_named(s.name);
                s.data = o.data.clone();
                s.compressed = o.compressed;
                s.page_size = o.page_size;
            }
        }
        v
    };
    let info = super::file_info(&d);
    let write = |name: &str, v: &[Section], order: &[&str]| {
        std::fs::write(format!("{out}/{name}"), pack(&info, v, order).unwrap()).unwrap();
    };
    write("T0-autocad-sections.dwg", &acad(), &acad_order);
    write("T1-our-header.dwg", &replace(acad(), &["AcDb:Header"]), &acad_order);
    write("T2-our-objects.dwg", &replace(acad(), &["AcDb:AcDbObjects", "AcDb:Handles"]), &acad_order);
    let without: Vec<Section> = acad().into_iter().filter(|s| !matches!(s.name, "AcDb:AppInfoHistory" | "AcDb:XrefManifest")).collect();
    let order11: Vec<&str> = acad_order.iter().copied().filter(|n| !matches!(*n, "AcDb:AppInfoHistory" | "AcDb:XrefManifest")).collect();
    write("T3-no-history-or-manifest.dwg", &without, &order11);
    let small = ["AcDb:SummaryInfo", "AcDb:Preview", "AcDb:AppInfo", "AcDb:RevHistory", "AcDb:ObjFreeSpace", "AcDb:Template", "AcDb:AuxHeader"];
    write("T4-our-small-sections.dwg", &replace(without, &small), &order11);
    std::fs::write(format!("{out}/T5-all-ours.dwg"), super::to_bytes(&d).unwrap().bytes).unwrap();
    let lit = super::container::pack_with(&info, &acad(), &acad_order, super::container::literal_only).unwrap();
    std::fs::write(format!("{out}/T6-autocad-sections-uncompressed-runs.dwg"), lit).unwrap();
}

/// AutoCAD's R2004-family files of the MI-201 corpus, by name.
fn autocad_containers() -> Vec<(&'static str, Vec<u8>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mosaiciron-mi201");
    [
        "mi201-r2004.dwg",
        "mi201-r2010.dwg",
        "mi201-r2013.dwg",
        "mi201-r2018.dwg",
        "mi201-r2018-from-ascii-dxf.dwg",
        "mi201-r2018-from-binary-dxf.dwg",
    ]
    .into_iter()
    .filter_map(|n| std::fs::read(dir.join(n)).ok().map(|b| (n, b)))
    .collect()
}

fn le32(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}
fn le64(d: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(d[at..at + 8].try_into().unwrap())
}

/// One of AutoCAD's files taken apart page by page, read straight from
/// the spec's layout (§4.1–§4.6) so that the container is checked against
/// the file rather than against our own reader.
struct Taken {
    /// Sections in stream order (by first page number).
    sections: Vec<super::container::Section>,
    /// Section names in section map order, after the empty section.
    map_order: Vec<&'static str>,
    /// The stored payload of each page in the order `pack` compresses:
    /// compressed data pages in stream order, the section map, the page map.
    stored: Vec<Vec<u8>>,
    /// Each compressed stream in the file.
    streams: Vec<(String, Vec<u8>)>,
}

fn take_apart(file: &[u8]) -> Taken {
    use super::super::lz77;
    use super::container::Section;
    let mask = r2004::magic_sequence(0x6C);
    let e: Vec<u8> = file[0x80..0xEC].iter().zip(&mask).map(|(a, b)| a ^ b).collect();
    let system = |at: usize| -> (Vec<u8>, Vec<u8>) {
        let (dec, comp) = (le32(file, at + 4) as usize, le32(file, at + 8) as usize);
        let stored = file[at + 20..at + 20 + comp].to_vec();
        (lz77::decompress(&stored, dec).unwrap(), stored)
    };
    let pm_at = le64(&e, 0x54) as usize + 0x100;
    let (pm, pm_stored) = system(pm_at);
    let mut address = std::collections::HashMap::new();
    let (mut a, mut i) = (0x100usize, 0);
    while i + 8 <= pm.len() {
        let (n, size) = (le32(&pm, i) as i32, le32(&pm, i + 4) as usize);
        i += if n < 0 { 24 } else { 8 };
        address.insert(n, a);
        a += size;
    }
    let (sm, sm_stored) = system(address[&(le32(&e, 0x5C) as i32)]);
    let mut streams = vec![("page map".to_string(), pm_stored.clone()), ("section map".to_string(), sm_stored.clone())];
    let mut sections: Vec<(u32, Section, Vec<Vec<u8>>)> = Vec::new();
    let mut map_order = Vec::new();
    let mut q = 20;
    for _ in 0..le32(&sm, 0) {
        let size = le64(&sm, q) as usize;
        let count = le32(&sm, q + 8) as usize;
        let page_size = le32(&sm, q + 12) as usize;
        let compressed = le32(&sm, q + 20) == 2;
        let name_bytes = &sm[q + 32..q + 96];
        let name = String::from_utf8_lossy(&name_bytes[..name_bytes.iter().position(|b| *b == 0).unwrap_or(64)]).into_owned();
        q += 96;
        let mut data = Vec::new();
        let mut stored_pages = Vec::new();
        let mut first = u32::MAX;
        for _ in 0..count {
            let (number, data_size) = (le32(&sm, q), le32(&sm, q + 4) as usize);
            q += 16;
            first = first.min(number);
            let at = address[&(number as i32)];
            let stored = file[at + 32..at + 32 + data_size].to_vec();
            if compressed {
                data.extend(lz77::decompress(&stored, page_size).unwrap());
                streams.push((format!("{name} page {number}"), stored.clone()));
            } else {
                data.extend_from_slice(&stored);
            }
            stored_pages.push(stored);
        }
        if name.is_empty() {
            continue;
        }
        data.truncate(size);
        let name: &'static str = Box::leak(name.into_boxed_str());
        map_order.push(name);
        sections.push((first, Section { name, data, compressed, page_size }, if compressed { stored_pages } else { Vec::new() }));
    }
    sections.sort_by_key(|s| s.0);
    let mut stored: Vec<Vec<u8>> = sections.iter().flat_map(|s| s.2.clone()).collect();
    stored.push(sm_stored);
    stored.push(pm_stored);
    Taken { sections: sections.into_iter().map(|s| s.1).collect(), map_order, stored, streams }
}

thread_local! {
    static REPLAY: std::cell::RefCell<std::collections::VecDeque<Vec<u8>>> = Default::default();
}

/// A "compressor" that hands back AutoCAD's stored payloads in turn, after
/// checking that each one decompresses to the page the container built.
fn replay(page: &[u8]) -> Option<Vec<u8>> {
    let stored = REPLAY.with(|q| q.borrow_mut().pop_front()).expect("a stored page for every compressed page");
    let built = super::super::lz77::decompress(&stored, page.len()).unwrap();
    assert!(built == page, "the container built a different page ({} bytes) than AutoCAD stored", page.len());
    Some(stored)
}

/// Given AutoCAD's own compressed payloads, the container lays out AutoCAD's
/// AC1032 files byte for byte: file header, encrypted block, page headers,
/// checksums, padding, both maps and the second header. What our files can
/// differ in is then only what the compressor writes.
#[test]
fn container_rebuilds_autocads_files() {
    let mut checked = 0;
    for (name, file) in autocad_containers() {
        if &file[..6] != b"AC1032" {
            continue;
        }
        let t = take_apart(&file);
        let info = super::container::FileInfo {
            version: *b"AC1032",
            maintenance: file[0x0B],
            app_version: file[0x11],
            app_maintenance: file[0x12],
            codepage: u16::from_le_bytes([file[0x13], file[0x14]]),
            acad_maintenance: le32(&file, 0x3C),
        };
        REPLAY.with(|q| *q.borrow_mut() = t.stored.into());
        let out = super::container::pack_with(&info, &t.sections, &t.map_order, replay).unwrap();
        assert!(REPLAY.with(|q| q.borrow().is_empty()), "{name}: pages left over");
        if let Some(i) = out.iter().zip(&file).position(|(a, b)| a != b) {
            panic!(
                "{name}: differs at byte {i:#x}: ours {:02x?}, AutoCAD's {:02x?}",
                &out[i..(i + 16).min(out.len())],
                &file[i..(i + 16).min(file.len())]
            );
        }
        assert_eq!(out.len(), file.len(), "{name}: length");
        checked += 1;
    }
    eprintln!("{checked} AC1032 files rebuilt byte for byte");
}

/// Every compressed stream in AutoCAD's 2004 to 2018 files keeps to the
/// forms our compressor limits itself to (`lz77` module notes), and so do
/// the streams of the file we write.
#[test]
fn compressed_streams_keep_to_autocads_forms() {
    let mut bad = Vec::new();
    let mut seen = 0;
    for (name, file) in autocad_containers() {
        for (what, s) in take_apart(&file).streams {
            seen += 1;
            if let Err(e) = super::super::lz77::autocad_form(&s) {
                bad.push(format!("{name} {what}: {e}"));
            }
        }
    }
    if let Some(file) = corpus() {
        let d = super::super::read_bytes(&file).unwrap();
        let ours = super::to_bytes(&d).unwrap().bytes;
        for (what, s) in take_apart(&ours).streams {
            seen += 1;
            if let Err(e) = super::super::lz77::autocad_form(&s) {
                bad.push(format!("ours {what}: {e}"));
            }
        }
    }
    eprintln!("{seen} streams checked");
    assert!(bad.is_empty(), "{bad:#?}");
}

/// An application information list (AcDb:AppInfo, and AcDb:AppInfoHistory
/// after its first 36 bytes): the list's name and each entry's kind and
/// text.
fn appinfo_list(s: &[u8]) -> (String, Vec<([u8; 16], String)>) {
    let text = |p: &mut usize| -> String {
        let n = u16::from_le_bytes([s[*p], s[*p + 1]]) as usize;
        let units: Vec<u16> = (0..n).map(|i| u16::from_le_bytes([s[*p + 2 + 2 * i], s[*p + 3 + 2 * i]])).collect();
        *p += 2 + 2 * n;
        String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
    };
    let mut p = 0;
    let name = text(&mut p);
    let count = le32(s, p);
    p += 4;
    let mut entries = Vec::new();
    for _ in 0..count {
        let kind: [u8; 16] = s[p..p + 16].try_into().unwrap();
        p += 16;
        entries.push((kind, text(&mut p)));
    }
    assert_eq!(p, s.len(), "list ends with its last entry");
    (name, entries)
}

/// The small sections against AutoCAD's own: its first save of a drawing
/// (its save of our DXF) for the auxiliary header, summary information and
/// application history, and its 2018 file for the xref manifest. Where
/// AutoCAD writes its own name, ours is this program's.
#[test]
fn small_sections_follow_autocads() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mosaiciron-mi201");
    let Ok(first) = std::fs::read(dir.join("mi201-r2018-from-ascii-dxf.dwg")) else { return };
    let d = super::super::read_bytes(&first).unwrap();
    let t = take_apart(&first);
    let acad = |n: &str| t.sections.iter().find(|s| s.name == n).unwrap().data.clone();

    let seed = d.header.handseed.unwrap();
    assert_eq!(super::sections::aux_header(&d, seed, 1), acad("AcDb:AuxHeader"), "auxiliary header");
    assert_eq!(super::sections::summary_info(&d), acad("AcDb:SummaryInfo"), "summary information");

    let (name, version) = ("BlackCAD", "0.7.0");
    let theirs = acad("AcDb:AppInfoHistory");
    let ours = super::sections::app_info_history(&d, true, name, version);
    assert_eq!(&ours[..36], &theirs[..36], "history start");
    let (n1, e1) = appinfo_list(&ours[36..]);
    let (n2, e2) = appinfo_list(&theirs[36..]);
    assert_eq!(n1, n2);
    assert_eq!(e1.iter().map(|e| e.0).collect::<Vec<_>>(), e2.iter().map(|e| e.0).collect::<Vec<_>>());
    let ours_props = e1[1]
        .1
        .replace(&format!(">{name}<"), ">AutoCAD 2027<")
        .replace(&format!(">{version}<"), ">X.118.0.0<");
    assert_eq!(ours_props, e2[1].1, "property set");

    let (n1, e1) = appinfo_list(&super::sections::app_info(name, version)[4..]);
    let (n2, e2) = appinfo_list(&acad("AcDb:AppInfo")[4..]);
    assert_eq!(n1, n2);
    assert_eq!(e1.iter().map(|e| e.0).collect::<Vec<_>>(), e2.iter().map(|e| e.0).collect::<Vec<_>>());
    assert!(e1.iter().all(|e| !e.1.contains("Autodesk") && !e.1.contains("AutoCAD")));

    let Some(file) = corpus() else { return };
    let d = super::super::read_bytes(&file).unwrap();
    let t = take_apart(&file);
    let manifest = t.sections.iter().find(|s| s.name == "AcDb:XrefManifest").unwrap();
    assert_eq!(super::sections::xref_manifest(&d), manifest.data, "xref manifest");
}

/// Print, for the handles in `DUMP` (hex, comma separated), the object's
/// DXF groups from AutoCAD's 2018 DXF and its DWG streams after the common
/// part: data-stream bits, strings and handle references.
#[test]
#[ignore]
fn probe_object_streams() {
    use super::super::object;
    let Some(file) = corpus() else { return };
    let d = super::super::read_bytes(&file).unwrap();
    let dxf = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mosaiciron-mi201/mi201-r2018.dxf")).unwrap();
    let groups = cad_dxf::encode::from_ascii(&dxf).unwrap();
    let want: Vec<u64> = std::env::var("DUMP").unwrap_or_default().split(',').filter_map(|h| u64::from_str_radix(h.trim(), 16).ok()).collect();
    for h in want {
        let o = &d.objects[&Handle(h)];
        println!("== {h:X} {} (type {})", o.type_name, o.type_code);
        // DXF groups of the record with handle h.
        let mut i = 0;
        while i < groups.len() {
            if groups[i].code == 0 && groups.get(i + 1).is_some_and(|g| g.code == 5 && matches!(g.value, cad_dxf::encode::Value::Handle(x) if x == h)) {
                let mut j = i;
                loop {
                    println!("   dxf {:>4} {:?}", groups[j].code, groups[j].value);
                    j += 1;
                    if j >= groups.len() || groups[j].code == 0 {
                        break;
                    }
                }
                break;
            }
            i += 1;
        }
        let raw = o.raw.as_ref().unwrap();
        let buf = object::raw_buffer(raw);
        let ctx = object::DecodeCtx { ver: raw.version, codepage: d.codepage, classes: &d.classes };
        let mut op = object::open(&ctx, &buf, 0).unwrap();
        let s = &mut op.s;
        let mut bits = String::new();
        let mut t = s.dat.clone();
        while t.remaining() > 0 {
            bits.push(if t.b().unwrap() { '1' } else { '0' });
        }
        println!("   dat {} bits: {bits}", bits.len());
        if let Some(st) = &s.strs {
            let mut st = st.clone();
            let mut out = Vec::new();
            while st.remaining() >= 2 {
                match st.text16() {
                    Ok(x) => out.push(x),
                    Err(_) => break,
                }
            }
            println!("   strs {out:?}");
        }
        let mut hs = Vec::new();
        while s.handle_bits_left() >= 8 {
            match s.h() {
                Ok(r) => hs.push(format!("{}.{:X}", r.code, r.value)),
                Err(_) => break,
            }
        }
        println!("   hdl {}", hs.join(" "));
    }
}

/// AutoCAD's own DXF of the 2018 drawing, read and written as DWG: each
/// object against the same handle in AutoCAD's DWG. What may differ: the
/// MTEXT extents (our fonts), values DXF rounds to 16 digits, and the
/// handles AutoCAD gives the variable dictionary in DXF only.
#[test]
fn autocads_dxf_writes_autocads_objects() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mosaiciron-mi201");
    let (Ok(dxf), Some(file)) = (std::fs::read(dir.join("mi201-r2018.dxf")), corpus()) else { return };
    let c = r2004::read(&file, DwgVersion::R2018).unwrap();
    let d = super::super::read_bytes(&file).unwrap();
    let from_dxf = cad_dxf::read_bytes(&dxf).unwrap();
    let enc = super::encode_objects(&from_dxf, DwgVersion::R2018, &super::WriteOptions::default());
    for n in &enc.notes {
        eprintln!("note: {n}");
    }
    let ours: BTreeMap<u64, Vec<u8>> = enc.objects.into_iter().collect();
    let (mut same, mut bad, mut missing) = (0, Vec::new(), Vec::new());
    for o in d.objects.values() {
        let theirs = original(&c.objects, o.offset);
        match ours.get(&o.handle.0) {
            Some(b) if *b == theirs => same += 1,
            Some(b) => bad.push(format!("{:X} {} at bit {:?}", o.handle.0, o.type_name, first_difference(b, &theirs))),
            None => missing.push(format!("{:X} {}", o.handle.0, o.type_name)),
        }
    }
    let extra: Vec<String> = ours
        .keys()
        .filter(|h| !d.objects.contains_key(&Handle(**h)))
        .map(|h| format!("{h:X} {}", from_dxf.objects[&Handle(*h)].type_name))
        .collect();
    eprintln!("{same} identical; different: {bad:#?}\nnot written: {missing:?}\nonly in ours: {extra:?}");
    // The six objects of the variable dictionary have other handles in
    // AutoCAD's DXF; nothing else may be missing.
    assert_eq!(missing.len(), 6, "{missing:?}");
    assert!(missing.iter().all(|m| m.contains("DICTIONARY")));
    assert!(same >= 152, "{same}");
    let bits = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:08b}")).collect() };
    for h in std::env::var("DUMP").unwrap_or_default().split(',').filter_map(|h| u64::from_str_radix(h.trim(), 16).ok()) {
        let o = &d.objects[&Handle(h)];
        let (a, b) = (bits(&ours[&h]), bits(&original(&c.objects, o.offset)));
        println!("== {h:X} {}", o.type_name);
        for i in (0..a.len().max(b.len())).step_by(64) {
            let sa = a.get(i..(i + 64).min(a.len())).unwrap_or("");
            let sb = b.get(i..(i + 64).min(b.len())).unwrap_or("");
            if sa != sb {
                println!("{i:5} dxf  {sa}\n      acad {sb}");
            }
        }
        println!("   from DXF: {:?}\n   from DWG: {:?}", from_dxf.objects[&Handle(h)].kind, o.kind);
        println!("   entity DXF: {:?}\n   entity DWG: {:?}", from_dxf.objects[&Handle(h)].entity, o.entity);
    }
}

/// Every style object of AutoCAD's 2018 DWG, decoded, gives the groups
/// AutoCAD's DXF of the drawing gives it (reals to DXF's 16 digits).
#[test]
fn style_objects_decode_to_autocads_dxf_groups() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mosaiciron-mi201");
    let (Ok(dxf), Some(file)) = (std::fs::read(dir.join("mi201-r2018.dxf")), corpus()) else { return };
    let d = super::super::read_bytes(&file).unwrap();
    let from_dxf = cad_dxf::read_bytes(&dxf).unwrap();
    let close = |a: &XrValue, b: &XrValue| match (a, b) {
        (XrValue::Real(x), XrValue::Real(y)) => (x - y).abs() <= 1e-12 * x.abs().max(1.0),
        (XrValue::Point(p), XrValue::Point(q)) => (p.x - q.x).abs() + (p.y - q.y).abs() + (p.z - q.z).abs() < 1e-9,
        _ => a == b,
    };
    let mut checked = 0;
    let mut bad = Vec::new();
    for o in d.objects.values() {
        let ObjectKind::NonGraphic(NonGraphic::Groups(g)) = &o.kind else { continue };
        let Some(ObjectKind::NonGraphic(NonGraphic::Groups(x))) = from_dxf.objects.get(&o.handle).map(|o| &o.kind) else {
            bad.push(format!("{:X} {}: not a kept object in the DXF", o.handle.0, g.kind));
            continue;
        };
        checked += 1;
        let same = g.items.len() == x.items.len()
            && g.items.iter().zip(&x.items).all(|(a, b)| a.0 == b.0 && close(&a.1, &b.1));
        if !same {
            let at = g.items.iter().zip(&x.items).position(|(a, b)| a.0 != b.0 || !close(&a.1, &b.1));
            bad.push(format!(
                "{:X} {}: {} vs {} items, first difference at {at:?}: {:?} / {:?}",
                o.handle.0,
                g.kind,
                g.items.len(),
                x.items.len(),
                at.map(|i| &g.items[i]),
                at.map(|i| &x.items[i])
            ));
        }
    }
    eprintln!("{checked} style objects checked");
    assert!(checked >= 33 && bad.is_empty(), "{bad:#?}");
}

#[test]
#[ignore]
fn probe_object_order() {
    let file = match std::env::var("DWGFILE") {
        Ok(p) => std::fs::read(p).unwrap(),
        Err(_) => corpus().unwrap(),
    };
    let d = super::super::read_bytes(&file).unwrap();
    let mut v: Vec<(usize, u64, String, u64)> = d.objects.values().map(|o| (o.offset, o.handle.0, o.type_name.clone(), o.owner.0)).collect();
    v.sort();
    for (off, h, t, owner) in v {
        println!("{off:6} {h:6X} {t} owner {owner:X}");
    }
    for h in [8u64, 0x3F, 0x16, 0x12, 0x3D134] {
        let o = &d.objects[&Handle(h)];
        match &o.kind {
            ObjectKind::Control(c) => println!("control {h:X}: {:X?}", c.entries.iter().map(|x| x.0).collect::<Vec<_>>()),
            ObjectKind::NonGraphic(NonGraphic::Dictionary(dc)) => println!("dict {h:X}: {:?}", dc.entries.iter().map(|(k, x)| format!("{k}={:X}", x.0)).collect::<Vec<_>>()),
            k => println!("{h:X}: {k:?}"),
        }
    }
}

/// Each object's owned children, in container order, for working out
/// AutoCAD's object order: `children <h> <type>: <c1> <c2> ...`.
#[test]
#[ignore]
fn probe_ownership() {
    let file = match std::env::var("DWGFILE") {
        Ok(p) => std::fs::read(p).unwrap(),
        Err(_) => corpus().unwrap(),
    };
    let d = super::super::read_bytes(&file).unwrap();
    for o in d.objects.values() {
        let mut kids: Vec<u64> = Vec::new();
        match &o.kind {
            ObjectKind::Control(c) => kids.extend(c.entries.iter().map(|h| h.0)),
            ObjectKind::NonGraphic(NonGraphic::Dictionary(dc)) => kids.extend(dc.entries.iter().map(|(_, h)| h.0)),
            ObjectKind::BlockHeader(b) => {
                kids.push(b.block_entity.0);
                kids.extend(b.entities.iter().map(|h| h.0));
                kids.push(b.endblk.0);
            }
            _ => {}
        }
        let xdic = o.xdictionary.0;
        println!("children {:X} {} xdic {:X}: {}", o.handle.0, o.type_name, xdic, kids.iter().map(|h| format!("{h:X}")).collect::<Vec<_>>().join(" "));
    }
}

/// What the writer leaves out is counted by type: nothing for a drawing
/// read from a 2018 file, the objects kept only as older records for one
/// read from a 2004 file.
#[test]
fn left_out_objects_are_counted() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mosaiciron-mi201");
    let (Ok(r2018), Ok(r2004)) = (std::fs::read(dir.join("mi201-r2018.dwg")), std::fs::read(dir.join("mi201-r2004.dwg"))) else {
        return;
    };
    let out = super::to_bytes(&super::super::read_bytes(&r2018).unwrap()).unwrap();
    assert!(out.left_out.is_empty() && out.notes.is_empty(), "{:?}", out.left_out);
    let out = super::to_bytes(&super::super::read_bytes(&r2004).unwrap()).unwrap();
    assert_eq!(out.left_out.values().sum::<usize>(), 67, "{:?}", out.left_out);
    assert_eq!(out.left_out.get("MATERIAL"), Some(&3), "{:?}", out.left_out);
    assert_eq!(out.notes.len(), 68, "a count, then one note per object");
}

//! The real-file corpus (`corpus/` at the repository root, TEST-PLAN.md):
//! every DWG is read and checked against a DXF that AutoCAD saved of the
//! same drawing.
//!
//! For each `<name>.dwg` the reference is `<name>.dxf` when there is one,
//! otherwise the first DXF of the folder (for example the R14 DWG of
//! MI-201 is checked against its R12 DXF). Checks:
//!
//! - the file reads, with no failed objects, no object notes and no
//!   warnings, and its header is verified against the object map;
//! - every model-space LINE, ARC, CIRCLE, TEXT and DIMENSION of the DXF is
//!   in the drawing under the same handle, on the same layer, with the same
//!   geometry and text, and the drawing has no others;
//! - the drawing survives the `.cad` round trip unchanged.
//!
//! A version this build cannot open yet (AC1021) is reported and skipped,
//! so the test starts checking it as soon as the reader supports it.
//!
//! Binary DXFs saved by AutoCAD (`*-binary.dxf`) check the binary DXF
//! encoder: each must read to its last byte and write back byte for byte,
//! and the DXF writer's header must use AutoCAD's variables, codes and
//! order.

use cad_core::dxf::encode::{self, Group, Value};
use cad_core::error::ErrorKind;
use cad_core::model::{Drawing, ObjectKind, Severity};
use cad_core::native;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const TOL: f64 = 1e-9;

/// One model-space entity of a DXF: its type, layer and the group values
/// the comparison uses.
#[derive(Debug)]
struct DxfEntity {
    kind: String,
    layer: String,
    reals: BTreeMap<u16, f64>,
    text: BTreeMap<u16, String>,
}

/// Model-space entities of a DXF, by handle. A minimal reader: pairs of
/// lines (group code, value), the ENTITIES section only, the first value of
/// each group code.
fn dxf_entities(path: &Path) -> BTreeMap<u64, DxfEntity> {
    let raw = std::fs::read(path).unwrap();
    let text = String::from_utf8_lossy(&raw);
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut out = BTreeMap::new();
    let mut in_entities = false;
    let mut cur: Option<(u64, DxfEntity)> = None;
    let mut i = 0;
    while i + 1 < lines.len() {
        let code: i32 = lines[i].trim().parse().unwrap_or(-1);
        let value = lines[i + 1];
        i += 2;
        if code == 0 {
            if let Some((h, e)) = cur.take() {
                out.insert(h, e);
            }
            if value == "ENDSEC" {
                in_entities = false;
            }
            if in_entities {
                cur = Some((
                    0,
                    DxfEntity {
                        kind: value.to_string(),
                        layer: String::new(),
                        reals: BTreeMap::new(),
                        text: BTreeMap::new(),
                    },
                ));
            }
            continue;
        }
        if code == 2 && value == "ENTITIES" {
            in_entities = true;
            continue;
        }
        let Some((h, e)) = cur.as_mut() else { continue };
        match code {
            5 => *h = u64::from_str_radix(value.trim(), 16).unwrap(),
            8 => e.layer = value.to_string(),
            1 | 2 => {
                e.text
                    .entry(code as u16)
                    .or_insert_with(|| value.to_string());
            }
            10..=59 => {
                if let Ok(v) = value.trim().parse::<f64>() {
                    e.reals.entry(code as u16).or_insert(v);
                }
            }
            _ => {}
        }
    }
    out
}

/// Layers of a DXF's LAYER table: name and whether it is off (a negative
/// colour, group 62).
fn dxf_layers(path: &Path) -> BTreeMap<String, bool> {
    let raw = std::fs::read(path).unwrap();
    let text = String::from_utf8_lossy(&raw);
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut out = BTreeMap::new();
    let (mut in_layer, mut name) = (false, None::<String>);
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        match (code.trim(), value.trim()) {
            ("0", v) => {
                in_layer = v == "LAYER";
                name = None;
            }
            ("2", v) if in_layer => name = Some(v.to_string()),
            ("62", v) if in_layer => {
                if let (Some(n), Ok(c)) = (name.clone(), v.parse::<i32>()) {
                    out.insert(n, c < 0);
                }
            }
            _ => {}
        }
    }
    out
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= TOL * b.abs().max(1.0)
}

/// Same angle, in degrees, modulo 360.
fn same_angle(rad: f64, deg: f64) -> bool {
    let d = (rad.to_degrees() - deg).rem_euclid(360.0);
    d.min(360.0 - d) <= 1e-7
}

/// Text equal up to characters a file transfer replaced with U+FFFD (some
/// reference DXFs lost their degree signs that way).
fn same_text(ours: &str, reference: &str) -> bool {
    let a: Vec<char> = ours.chars().collect();
    let b: Vec<char> = reference.chars().collect();
    a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x == y || *y == '\u{FFFD}')
}

/// Differences between a drawing's model space and a DXF's.
fn compare(d: &Drawing, dxf: &BTreeMap<u64, DxfEntity>) -> Vec<String> {
    let mut problems = Vec::new();
    let ours: BTreeMap<u64, _> = d
        .model_space_entities()
        .into_iter()
        .map(|o| (o.handle.0, o))
        .collect();
    for (h, e) in dxf {
        let Some(o) = ours.get(h) else {
            problems.push(format!("{h:X} {} missing", e.kind));
            continue;
        };
        let layer = o
            .entity
            .as_ref()
            .and_then(|c| d.layer(c.layer))
            .map(|l| l.name.as_str())
            .unwrap_or("");
        if !layer.eq_ignore_ascii_case(&e.layer) {
            problems.push(format!("{h:X}: layer {layer:?}, DXF {:?}", e.layer));
        }
        let r = |c: u16| e.reals.get(&c).copied().unwrap_or(0.0);
        let ok = match (&o.kind, e.kind.as_str()) {
            (ObjectKind::Line(l), "LINE") => {
                near(l.start.x, r(10))
                    && near(l.start.y, r(20))
                    && near(l.end.x, r(11))
                    && near(l.end.y, r(21))
            }
            (ObjectKind::Circle(c), "CIRCLE") => {
                near(c.center.x, r(10)) && near(c.center.y, r(20)) && near(c.radius, r(40))
            }
            (ObjectKind::Arc(a), "ARC") => {
                near(a.center.x, r(10))
                    && near(a.center.y, r(20))
                    && near(a.radius, r(40))
                    && same_angle(a.start_angle, r(50))
                    && same_angle(a.end_angle, r(51))
            }
            (ObjectKind::Text(t), "TEXT") => {
                near(t.insertion.x, r(10))
                    && near(t.insertion.y, r(20))
                    && near(t.height, r(40))
                    && same_text(&t.value, e.text.get(&1).map(String::as_str).unwrap_or(""))
            }
            (ObjectKind::Dimension(m), "DIMENSION") => {
                near(m.pt10.x, r(10))
                    && near(m.pt10.y, r(20))
                    && near(m.pt13.x, r(13))
                    && near(m.pt13.y, r(23))
                    && near(m.pt14.x, r(14))
                    && near(m.pt14.y, r(24))
            }
            (k, t) => {
                problems.push(format!("{h:X}: DXF {t}, read as {k:?}"));
                true
            }
        };
        if !ok {
            problems.push(format!("{h:X} {}: geometry or text differs", e.kind));
        }
    }
    for h in ours.keys() {
        if !dxf.contains_key(h) {
            problems.push(format!("{h:X}: not in the DXF"));
        }
    }
    problems
}

/// Problems with the drawing on its own: failed objects, notes, warnings,
/// an unverified header, a lossy round trip through `.cad`.
fn health(d: &Drawing) -> Vec<String> {
    let mut problems = Vec::new();
    if !d.header.verified {
        problems.push("header not verified".into());
    }
    for o in d.objects.values() {
        if let ObjectKind::Failed(e) = &o.kind {
            problems.push(format!("{:X} {} failed: {e}", o.handle.0, o.type_name));
        }
        if let Some(n) = &o.note {
            problems.push(format!("{:X} {}: {n}", o.handle.0, o.type_name));
        }
    }
    for dg in &d.diagnostics {
        if dg.severity != Severity::Info {
            problems.push(format!("diagnostic: {}", dg.message));
        }
    }
    let back = native::read_bytes(&native::to_bytes(d, &native::WriteOptions::default()))
        .expect("read back the .cad");
    if back.objects != d.objects || back.header != d.header || back.classes != d.classes {
        problems.push(".cad round trip changed the drawing".into());
    }
    problems
}

fn corpus_dirs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case(ext))
        })
        .collect();
    v.sort();
    v
}

#[test]
fn every_corpus_dwg_matches_its_dxf() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for dir in corpus_dirs() {
        let dxfs: Vec<PathBuf> = files(&dir, "dxf").into_iter().filter(|p| !is_binary_dxf(p)).collect();
        for dwg in files(&dir, "dwg") {
            let name = dwg.file_name().unwrap().to_string_lossy().to_string();
            // The DXF of the same name, or the one whose name the DWG's
            // extends (AutoCAD's saves of a DXF), or the first.
            let stem = |p: &std::path::PathBuf| p.file_stem().unwrap().to_string_lossy().to_string();
            let reference = dxfs
                .iter()
                .filter(|x| stem(&dwg).starts_with(&stem(x)))
                .max_by_key(|x| stem(x).len())
                .or(dxfs.first());
            let d = match cad_core::open(&dwg) {
                Ok(d) => d,
                Err(e) if e.kind == ErrorKind::UnsupportedVersion => {
                    eprintln!("{name}: skipped, {}", e.message);
                    continue;
                }
                Err(e) => {
                    failures.push(format!("{name}: {e}"));
                    continue;
                }
            };
            let mut problems = health(&d);
            if let Some(r) = reference {
                let entities = dxf_entities(r);
                if entities.is_empty() {
                    problems.push(format!("no entities read from {}", r.display()));
                }
                eprintln!(
                    "{name}: {} objects; model space checked against {} DXF entities of {}",
                    d.objects.len(),
                    entities.len(),
                    r.file_name().unwrap().to_string_lossy()
                );
                problems.extend(compare(&d, &entities));
                // Layers switched off in the reference are off here, and
                // only those.
                for (name, off) in dxf_layers(r) {
                    let ours = d.objects.values().find_map(|o| match &o.kind {
                        ObjectKind::Layer(l) if l.name == name => Some(l.off),
                        _ => None,
                    });
                    if ours != Some(off) {
                        problems.push(format!("layer {name}: off {ours:?}, reference {off}"));
                    }
                }
            }
            checked += 1;
            for p in problems {
                failures.push(format!("{name}: {p}"));
            }
        }
    }
    eprintln!("{checked} corpus drawings checked");
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

fn is_binary_dxf(p: &Path) -> bool {
    std::fs::read(p).is_ok_and(|b| b.starts_with(encode::BINARY_SENTINEL))
}

fn binary_dxfs() -> Vec<PathBuf> {
    corpus_dirs()
        .iter()
        .flat_map(|d| files(d, "dxf"))
        .filter(|p| is_binary_dxf(p))
        .collect()
}

#[test]
fn autocads_binary_dxfs_read_and_write_back_byte_for_byte() {
    let mut checked = 0;
    for p in binary_dxfs() {
        let bytes = std::fs::read(&p).unwrap();
        let groups = encode::from_binary(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let eof = Group { code: 0, value: Value::Str("EOF".into()) };
        assert_eq!(groups.last(), Some(&eof), "{}", p.display());
        let back = encode::to_binary(&groups);
        let first = back.iter().zip(&bytes).position(|(a, b)| a != b);
        assert!(
            back == bytes,
            "{}: written back differs at byte {:?} ({} vs {} bytes)",
            p.display(),
            first,
            back.len(),
            bytes.len()
        );
        eprintln!("{}: {} groups, {} bytes", p.display(), groups.len(), bytes.len());
        checked += 1;
    }
    eprintln!("{checked} binary DXFs checked");
}

/// Header variables in order, each with the codes of its groups.
fn header_vars(groups: &[Group]) -> Vec<(String, Vec<u16>)> {
    let mut out: Vec<(String, Vec<u16>)> = Vec::new();
    let mut in_header = false;
    for (i, g) in groups.iter().enumerate() {
        match (&g.code, &g.value) {
            (2, Value::Str(s)) if s == "HEADER" && i > 0 => in_header = true,
            (0, Value::Str(s)) if s == "ENDSEC" && in_header => break,
            (9, Value::Str(name)) if in_header => out.push((name.clone(), Vec::new())),
            (code, _) if in_header => {
                if let Some(last) = out.last_mut() {
                    last.1.push(*code);
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn dxf_writer_header_matches_autocads_binary_dxf() {
    let mut checked = 0;
    for p in binary_dxfs() {
        let dwg = p.with_file_name(p.file_name().unwrap().to_string_lossy().replace("-binary.dxf", ".dwg"));
        let Ok(d) = cad_core::open(&dwg) else { continue };
        let theirs = header_vars(&encode::from_binary(&std::fs::read(&p).unwrap()).unwrap());
        let (groups, _) = cad_core::dxf::groups(&d);
        let ours = header_vars(&groups);
        assert!(!theirs.is_empty());
        for (i, (a, b)) in ours.iter().zip(&theirs).enumerate() {
            assert_eq!(a, b, "header variable {i} of {}", dwg.display());
        }
        assert_eq!(ours.len(), theirs.len());
        eprintln!("{}: {} header variables as AutoCAD writes them", dwg.display(), ours.len());
        checked += 1;
    }
    assert!(binary_dxfs().is_empty() || checked > 0, "no drawing for the binary DXFs");
}

#[test]
fn binary_and_ascii_dxf_carry_the_same_groups() {
    let mut checked = 0;
    for dir in corpus_dirs() {
        for dwg in files(&dir, "dwg") {
            let Ok(d) = cad_core::open(&dwg) else { continue };
            let (groups, _) = cad_core::dxf::groups(&d);
            let back = encode::from_binary(&encode::to_binary(&groups)).unwrap();
            assert!(
                encode::to_ascii(&back) == encode::to_ascii(&groups),
                "{}: the binary DXF reads back differently",
                dwg.display()
            );
            checked += 1;
        }
    }
    eprintln!("{checked} drawings written as binary and ASCII DXF");
}

/// The first lines where two ASCII DXFs part, for a failure message.
fn first_difference(a: &[u8], b: &[u8]) -> Option<String> {
    let (la, lb): (Vec<&str>, Vec<&str>) = (
        std::str::from_utf8(a).unwrap().split("\r\n").collect(),
        std::str::from_utf8(b).unwrap().split("\r\n").collect(),
    );
    let i = la.iter().zip(&lb).position(|(x, y)| x != y).or((la.len() != lb.len()).then(|| la.len().min(lb.len())))?;
    let from = i.saturating_sub(6);
    Some(format!(
        "line {}:\n  written: {:?}\n  again:   {:?}",
        i + 1,
        &la[from..(i + 4).min(la.len())],
        &lb[from..(i + 4).min(lb.len())]
    ))
}

#[test]
fn dxf_reads_back_into_the_drawing_it_came_from() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for dir in corpus_dirs() {
        for dwg in files(&dir, "dwg") {
            let Ok(d) = cad_core::open(&dwg) else { continue };
            let name = dwg.file_name().unwrap().to_string_lossy().to_string();
            let (groups, _) = cad_core::dxf::groups(&d);
            let ascii = encode::to_ascii(&groups);
            for (label, bytes) in [("ASCII", ascii.clone()), ("binary", encode::to_binary(&groups))] {
                let back = match cad_core::dxf::read_bytes(&bytes) {
                    Ok(b) => b,
                    Err(e) => {
                        failures.push(format!("{name} ({label}): {e}"));
                        continue;
                    }
                };
                let again = encode::to_ascii(&cad_core::dxf::groups(&back).0);
                if let Some(diff) = first_difference(&ascii, &again) {
                    failures.push(format!("{name} ({label}): {diff}"));
                }
            }
            checked += 1;
        }
    }
    eprintln!("{checked} drawings written as DXF and read back");
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}


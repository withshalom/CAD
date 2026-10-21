//! Size and speed of the native format on a large generated drawing.
//!
//!     cargo run --release -p cad-core --example native_bench -- 200000
//!     cargo run --release -p cad-core --example native_bench -- 200000 --lines --rounded
//!
//! By default the drawing mixes lines, arcs, circles, polylines, text and
//! block references, with unrounded random coordinates: the worst case for
//! compression. `--rounded` snaps coordinates to 0.01, as drafted geometry
//! often is. `--lines` makes every entity a line. `--no-recovery` leaves
//! out the recovery data (about 1/16 of the file). `--save-uncompressed
//! PATH` also writes the drawing as an uncompressed `.cad`, the input for
//! `tools/compaction_lab.py`.

use cad_core::export::dxf;
use cad_core::geom::{Vec2, Vec3};
use cad_core::model::*;
use cad_core::native::{self, audit, Change, ChangeSet, WriteOptions};
use std::f64::consts::TAU;
use std::time::Instant;

struct Rng(u64, bool);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
    }
    fn pick(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn p2(&mut self) -> Vec2 {
        let (x, y) = (self.f(0.0, 3000.0), self.f(0.0, 1500.0));
        if self.1 {
            Vec2::new((x * 100.0).round() / 100.0, (y * 100.0).round() / 100.0)
        } else {
            Vec2::new(x, y)
        }
    }
    fn p3(&mut self) -> Vec3 {
        let p = self.p2();
        Vec3::new(p.x, p.y, 0.0)
    }
}

fn obj(
    h: u64,
    name: &str,
    code: u16,
    owner: u64,
    e: Option<EntityCommon>,
    kind: ObjectKind,
) -> Object {
    Object {
        handle: Handle(h),
        type_code: code,
        type_name: name.into(),
        owner: Handle(owner),
        reactors: vec![],
        xdictionary: Handle::NULL,
        entity: e,
        kind,
        offset: 0,
        size: 0,
        note: None,
        raw: None,
        xdata: Vec::new(),
        unknown_tags: Vec::new(),
    }
}

fn ent(space: Space, layer: u64) -> Option<EntityCommon> {
    Some(EntityCommon {
        space,
        layer: Handle(layer),
        linetype_scale: 1.0,
        ..Default::default()
    })
}

fn build(n: usize, rng: &mut Rng, lines_only: bool) -> Drawing {
    let layer_names = [
        "0", "CUT", "ETCH", "BEND", "DIM", "TEXT", "HIDDEN", "CENTER",
    ];
    let layers: Vec<u64> = (0..layer_names.len() as u64).map(|i| 0x10 + i).collect();
    let mut objs = vec![
        obj(
            0x1,
            "BLOCK_CONTROL",
            0x30,
            0,
            None,
            ObjectKind::Control(Control {
                entries: (0..5).map(|i| Handle(0x40 + i * 0x10)).collect(),
                model_space: Handle(0x1F),
                paper_space: Handle::NULL,
            }),
        ),
        obj(
            0x2,
            "LAYER_CONTROL",
            0x32,
            0,
            None,
            ObjectKind::Control(Control {
                entries: layers.iter().map(|h| Handle(*h)).collect(),
                ..Default::default()
            }),
        ),
        obj(
            0x1F,
            "BLOCK_HEADER",
            0x31,
            0x1,
            None,
            ObjectKind::BlockHeader(BlockHeader {
                name: "*Model_Space".into(),
                ..Default::default()
            }),
        ),
    ];
    for (i, name) in layer_names.iter().enumerate() {
        objs.push(obj(
            layers[i],
            "LAYER",
            0x33,
            0x2,
            None,
            ObjectKind::Layer(Layer {
                name: (*name).into(),
                plot: true,
                color: Color::Index(1 + i as u8),
                ..Default::default()
            }),
        ));
    }
    // Five block definitions of ten entities each.
    let mut blocks = Vec::new();
    for b in 0..5u64 {
        let bh = 0x40 + b * 0x10;
        blocks.push(bh);
        objs.push(obj(
            bh,
            "BLOCK_HEADER",
            0x31,
            0x1,
            None,
            ObjectKind::BlockHeader(BlockHeader {
                name: format!("PART-{b}"),
                ..Default::default()
            }),
        ));
        for k in 0..10u64 {
            let (a, c) = (rng.p3(), rng.p3());
            objs.push(obj(
                bh + 1 + k,
                "LINE",
                0x13,
                bh,
                ent(Space::Owned, layers[1]),
                ObjectKind::Line(Line {
                    start: Vec3::new(a.x / 20.0, a.y / 20.0, 0.0),
                    end: Vec3::new(c.x / 20.0, c.y / 20.0, 0.0),
                    extrusion: Vec3::Z,
                    ..Default::default()
                }),
            ));
        }
    }
    let mut h = 0x1000u64;
    for _ in 0..n {
        h += 1;
        let layer = layers[1 + rng.pick(7) as usize];
        let r = if lines_only { 0 } else { rng.pick(100) };
        let (name, code, kind) = if r < 40 {
            (
                "LINE",
                0x13,
                ObjectKind::Line(Line {
                    start: rng.p3(),
                    end: rng.p3(),
                    extrusion: Vec3::Z,
                    ..Default::default()
                }),
            )
        } else if r < 55 {
            (
                "ARC",
                0x11,
                ObjectKind::Arc(Arc {
                    center: rng.p3(),
                    radius: rng.f(1.0, 200.0),
                    extrusion: Vec3::Z,
                    start_angle: rng.f(0.0, TAU),
                    end_angle: rng.f(0.0, TAU),
                    ..Default::default()
                }),
            )
        } else if r < 65 {
            (
                "CIRCLE",
                0x12,
                ObjectKind::Circle(Circle {
                    center: rng.p3(),
                    radius: rng.f(1.0, 50.0),
                    extrusion: Vec3::Z,
                    ..Default::default()
                }),
            )
        } else if r < 80 {
            let k = 4 + rng.pick(9) as usize;
            let points: Vec<Vec2> = (0..k).map(|_| rng.p2()).collect();
            let bulges = if rng.pick(3) == 0 {
                (0..k)
                    .map(|_| {
                        if rng.pick(4) == 0 {
                            rng.f(-1.0, 1.0)
                        } else {
                            0.0
                        }
                    })
                    .collect()
            } else {
                vec![]
            };
            (
                "LWPOLYLINE",
                0x4D,
                ObjectKind::LwPolyline(LwPolyline {
                    flags: if rng.pick(2) == 0 {
                        LwPolyline::FLAG_CLOSED
                    } else {
                        0
                    },
                    extrusion: Vec3::Z,
                    points,
                    bulges,
                    ..Default::default()
                }),
            )
        } else if r < 90 {
            (
                "TEXT",
                0x01,
                ObjectKind::Text(Text {
                    insertion: rng.p2(),
                    extrusion: Vec3::Z,
                    height: 2.5,
                    width_factor: 1.0,
                    value: format!(
                        "P-{} 3/8\" A36 QTY {}",
                        1000 + rng.pick(9000),
                        1 + rng.pick(40)
                    ),
                    ..Default::default()
                }),
            )
        } else {
            (
                "INSERT",
                0x07,
                ObjectKind::Insert(Insert {
                    insertion: rng.p3(),
                    scale: Vec3::new(1.0, 1.0, 1.0),
                    rotation: rng.f(0.0, TAU),
                    extrusion: Vec3::Z,
                    block_header: Handle(blocks[rng.pick(5) as usize]),
                    ..Default::default()
                }),
            )
        };
        objs.push(obj(h, name, code, 0, ent(Space::Model, layer), kind));
    }
    let mut d = Drawing {
        version: cad_core::dwg::DwgVersion::R2018,
        maintenance_version: 0,
        codepage: 30,
        header: HeaderVars {
            verified: true,
            insunits: Some(4),
            model_space: Handle(0x1F),
            ..Default::default()
        },
        classes: vec![],
        objects: objs.into_iter().map(|o| (o.handle, o)).collect(),
        sections: vec![],
        diagnostics: vec![],
        index: DrawingIndex::default(),
    };
    d.rebuild_index();
    d
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn mb(b: u64) -> String {
    format!("{:.2} MB", b as f64 / 1_000_000.0)
}

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200_000);
    let lines_only = std::env::args().any(|a| a == "--lines");
    let rounded = std::env::args().any(|a| a == "--rounded");
    let mut rng = Rng(0x2545_F491_4F6C_DD1D, rounded);
    let d = build(n, &mut rng, lines_only);
    let count = d.objects.len();
    println!(
        "Drawing: {n} model-space entities ({}, {} coordinates), {count} objects in all",
        if lines_only {
            "lines only"
        } else {
            "mixed types"
        },
        if rounded {
            "rounded to 0.01"
        } else {
            "unrounded"
        }
    );
    println!();

    let opts = WriteOptions {
        preview: false,
        recovery: !std::env::args().any(|a| a == "--no-recovery"),
        ..Default::default()
    };
    let t = Instant::now();
    let bytes = native::to_bytes(&d, &opts);
    let write_ms = ms(t);
    let raw = native::to_bytes(
        &d,
        &WriteOptions {
            compress: false,
            ..opts.clone()
        },
    );
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--save-uncompressed") {
        let path = args.get(i + 1).expect("--save-uncompressed needs a path");
        std::fs::write(path, &raw).expect("write");
        println!("uncompressed copy written to {path}");
    }
    let t = Instant::now();
    let back = native::read_bytes(&bytes).expect("read");
    let read_ms = ms(t);
    assert_eq!(back.objects.len(), count);
    assert!(back
        .objects
        .iter()
        .all(|(h, o)| d.objects.get(h) == Some(o)));

    let t = Instant::now();
    let rep = audit::audit(&bytes);
    let audit_ms = ms(t);
    assert!(rep.passed(), "{:?}", rep.failed().collect::<Vec<_>>());

    let keys: Vec<Handle> = d.objects.keys().copied().collect();
    let t = Instant::now();
    let lookups = 200;
    for _ in 0..lookups {
        let k = keys[rng.pick(keys.len() as u64) as usize];
        let o = native::read_object(&bytes, k)
            .expect("lookup")
            .expect("present");
        assert_eq!(o.handle, k);
    }
    let lookup_ms = ms(t) / lookups as f64;
    // The same through a file opened once (the object map is read once).
    let file = native::NativeFile::from_bytes(&bytes).expect("open");
    let t = Instant::now();
    for _ in 0..lookups {
        let k = keys[rng.pick(keys.len() as u64) as usize];
        let o = file.object(k).expect("lookup").expect("present");
        assert_eq!(o.handle, k);
    }
    let warm_ms = ms(t) / lookups as f64;

    let scene = cad_core::render::model_space(&d);
    let dxf_text = dxf::to_dxf(&scene, Some(&d));

    println!("{:<46} {:>12} {:>14}", "", "size", "per entity");
    let per = |b: u64| format!("{:.1} B", b as f64 / n as f64);
    println!(
        "{:<46} {:>12} {:>14}",
        ".cad, compressed",
        mb(bytes.len() as u64),
        per(bytes.len() as u64)
    );
    println!(
        "{:<46} {:>12} {:>14}",
        ".cad, uncompressed",
        mb(raw.len() as u64),
        per(raw.len() as u64)
    );
    println!(
        "{:<46} {:>12} {:>14}",
        "object records as .cad tags",
        mb(rep.tags.cad_bytes),
        per(rep.tags.cad_bytes)
    );
    println!(
        "{:<46} {:>12} {:>14}",
        "same records as binary DXF tags",
        mb(rep.tags.binary_dxf_bytes),
        per(rep.tags.binary_dxf_bytes)
    );
    println!(
        "{:<46} {:>12} {:>14}",
        "ASCII DXF R12 export (blocks exploded)",
        mb(dxf_text.len() as u64),
        per(dxf_text.len() as u64)
    );
    println!();
    println!(
        "{:<46} {:>12.1} ms",
        "write snapshot (encode + compress)", write_ms
    );
    println!(
        "{:<46} {:>12.1} ms",
        "read snapshot (decompress + decode all)", read_ms
    );
    println!(
        "{:<46} {:>12.1} ms",
        "audit (every section, record, map entry)", audit_ms
    );
    println!(
        "{:<46} {:>12.2} ms",
        "read one object via the map (cold)", lookup_ms
    );
    println!(
        "{:<46} {:>12.2} ms",
        "read one object, file already open", warm_ms
    );

    // Incremental saves.
    let dir = std::env::temp_dir().join(format!("cad-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("bench.cad");
    native::write_file(&d, &path, &opts).expect("save");
    let before = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let sets = 100;
    let t = Instant::now();
    for s in 0..sets {
        let mut changes = Vec::new();
        for _ in 0..10 {
            let k = keys[rng.pick(keys.len() as u64) as usize];
            let mut o = d.objects[&k].clone();
            if let ObjectKind::Line(l) = &mut o.kind {
                l.end = rng.p3();
            }
            changes.push(Change::Put(Box::new(o)));
        }
        native::append_changes(
            &path,
            &ChangeSet {
                author: "bench".into(),
                time: s,
                changes,
            },
        )
        .expect("append");
    }
    let append_ms = ms(t) / sets as f64;
    let after = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let t = Instant::now();
    let with_journal = native::read_file(&path).expect("read journaled");
    let read_j_ms = ms(t);
    let bytes_j = std::fs::read(&path).expect("read bytes");
    let rep_j = audit::audit(&bytes_j);
    assert!(rep_j.passed());
    let t = Instant::now();
    native::write_file(&with_journal, &path, &opts).expect("compact");
    let compact_ms = ms(t);
    let compacted = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let _ = std::fs::remove_dir_all(&dir);

    println!(
        "{:<46} {:>12.1} ms",
        format!("incremental save of 10 objects (avg of {sets})"),
        append_ms
    );
    println!(
        "{:<46} {:>12}",
        "file growth per incremental save",
        format!("{:.1} KB", (after - before) as f64 / sets as f64 / 1000.0)
    );
    println!(
        "{:<46} {:>12}",
        "  of which superseded directories/trailers",
        format!(
            "{:.1} KB",
            rep_j.superseded_bytes() as f64 / sets as f64 / 1000.0
        )
    );
    println!(
        "{:<46} {:>12.1} ms",
        "read snapshot + 100 change sets", read_j_ms
    );
    println!(
        "{:<46} {:>12.1} ms  ({} → {})",
        "compact (full save)",
        compact_ms,
        mb(after),
        mb(compacted)
    );
}

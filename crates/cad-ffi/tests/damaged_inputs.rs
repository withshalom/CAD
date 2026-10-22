//! Damaged files never stop the core (docs/TEST-PLAN.md, "damaged
//! input"). Every file the browser and the apps open goes through
//! `Session::open`; a bad one must come back as an error with a message,
//! never a panic, and one that opens must also show, report and save.
//!
//! The seeds are the core's own files only (the sample plate in every
//! format it writes, and a DXF hand-written from the reference), so the
//! test needs nothing from outside this repository. Damage is made from a
//! fixed seed, so a failure names the case that reproduces it; add that
//! case to `REGRESSIONS` when it is fixed.
//!
//! `CAD_FUZZ_CASES=20000 cargo test --release -p cad-ffi --test damaged_inputs`
//! runs longer.

use cad_ffi::session::Session;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// xorshift64*: small, fixed, and the same on every platform.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

fn seeds() -> Vec<(&'static str, Vec<u8>)> {
    let mut s = Session::new();
    let id = s.sample().unwrap();
    s.exec(id, "line 0 0 30 40").unwrap();
    s.exec(id, "text 5 5 2.5 0 DAMAGE").unwrap();
    let mut out: Vec<(&'static str, Vec<u8>)> = ["blc", "dwg", "dxf", "dxfb"]
        .into_iter()
        .map(|f| (f, s.save(id, f, 0, "fuzz").unwrap().0))
        .collect();
    out.push((
        "dxf-entities",
        b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n10\n0\nCIRCLE\n8\nHOLES\n10\n5\n20\n5\n40\n2\n0\nENDSEC\n0\nEOF\n"
            .to_vec(),
    ));
    out
}

/// One way of damaging a file: cut it short, flip bits, overwrite a run
/// with noise, or repeat or drop a run.
fn damage(src: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut b = src.to_vec();
    match rng.below(5) {
        0 => b.truncate(rng.below(b.len())),
        1 => {
            for _ in 0..1 + rng.below(16) {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
        }
        2 => {
            let at = rng.below(b.len());
            let n = (1 + rng.below(64)).min(b.len() - at);
            for x in &mut b[at..at + n] {
                *x = rng.next() as u8;
            }
        }
        3 => {
            let at = rng.below(b.len());
            let n = rng.below(256).min(b.len() - at);
            let run = b[at..at + n].to_vec();
            b.splice(at..at, run);
        }
        _ => {
            let at = rng.below(b.len());
            let n = rng.below(256).min(b.len() - at);
            b.drain(at..at + n);
        }
    }
    b
}

/// Open, and when that works, read it back every way an app does.
fn exercise(bytes: &[u8]) -> Result<(), String> {
    let mut s = Session::new();
    let id = s.open(bytes)?;
    s.info(id)?;
    s.display(id)?;
    let _ = s.exec(id, "line 0 0 1 1");
    let _ = s.exec(id, "undo");
    for f in ["blc", "dxf", "dwg"] {
        let _ = s.save(id, f, 0, "fuzz");
    }
    Ok(())
}

/// Cases that once failed: (seed name, case number).
const REGRESSIONS: &[(&str, u64)] = &[
    // A cycle of dimension blocks drew without end (render MAX_VISITS).
    ("dxfb", 293),
    // Cut off after a SECTION marker: sliced past the end.
    ("dxf-entities", 179),
];

fn run(name: &str, bytes: &[u8]) -> Option<String> {
    match catch_unwind(AssertUnwindSafe(|| exercise(bytes))) {
        Ok(Ok(())) => None,
        Ok(Err(m)) if !m.trim().is_empty() => None,
        Ok(Err(_)) => Some(format!("{name}: an error with no message")),
        Err(p) => {
            let why = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            Some(format!("{name}: panicked: {why}"))
        }
    }
}

#[test]
fn damaged_files_are_refused_not_fatal() {
    // Quiet: a panic here is a finding, reported below with its case.
    if std::env::var("CAD_FUZZ_ONLY").is_err() {
        std::panic::set_hook(Box::new(|_| {}));
    }
    let cases: u64 = std::env::var("CAD_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1200);
    let seeds = seeds();
    let mut failures = Vec::new();
    // The seeds themselves open cleanly.
    for (name, bytes) in &seeds {
        assert!(exercise(bytes).is_ok(), "{name} does not open");
    }
    let one = |name: &str, bytes: &[u8], case: u64| -> Vec<u8> {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ case.wrapping_mul(0x1000_0000_01B3) ^ name.len() as u64);
        damage(bytes, &mut rng)
    };
    let trace = std::env::var("CAD_FUZZ_TRACE").is_ok();
    let only: Option<u64> = std::env::var("CAD_FUZZ_ONLY").ok().and_then(|v| v.parse().ok());
    for case in 0..cases {
        if only.is_some_and(|o| o != case) {
            continue;
        }
        let (name, bytes) = &seeds[(case as usize) % seeds.len()];
        if trace {
            eprintln!("case {case} {name}");
        }
        if let Some(f) = run(name, &one(name, bytes, case)) {
            failures.push(format!("case {case} ({f})"));
        }
    }
    for (name, case) in REGRESSIONS {
        let bytes = &seeds.iter().find(|(n, _)| n == name).expect("seed").1;
        if let Some(f) = run(name, &one(name, bytes, *case)) {
            failures.push(format!("regression {case} ({f})"));
        }
    }
    // Bytes that are no file at all.
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for case in 0..200 {
        let n = rng.below(4096);
        let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        if let Some(f) = run("random", &bytes) {
            failures.push(format!("random {case} ({f})"));
        }
    }
    let _ = std::panic::take_hook();
    assert!(failures.is_empty(), "{} of {} cases failed:\n{}", failures.len(), cases + 200, failures.join("\n"));
}

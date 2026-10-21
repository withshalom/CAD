//! Memory and time of edits on a large drawing, through the session the
//! browser uses (docs/TEST-PLAN.md, "edit cost"). Native, with the
//! allocator counting bytes in use.
//!
//!     cargo run --release -p cad-ffi --example history_bench [objects] [edits]

use cad_ffi::session::Session;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

struct Counting;
static HEAP: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: passes every call to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: the caller's contract, passed on.
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let n = HEAP.fetch_add(l.size(), Ordering::Relaxed) + l.size();
            PEAK.fetch_max(n, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: as above.
        unsafe { System.dealloc(p, l) };
        HEAP.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        // SAFETY: as above.
        let q = unsafe { System.realloc(p, l, size) };
        if !q.is_null() {
            HEAP.fetch_sub(l.size(), Ordering::Relaxed);
            let n = HEAP.fetch_add(size, Ordering::Relaxed) + size;
            PEAK.fetch_max(n, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

fn mb() -> f64 {
    HEAP.load(Ordering::Relaxed) as f64 / 1048576.0
}

/// A drawing of about `n` objects, saved as DWG and opened again, so model
/// space lists its entities the way real files do.
fn big(s: &mut Session, n: usize) -> u32 {
    let id = s.new_drawing(false).unwrap();
    s.exec(id, "begin").unwrap();
    for i in 0..n {
        let (x, y) = ((i % 200) as f64 * 10.0, (i / 200) as f64 * 10.0);
        let cmd = match i % 3 {
            0 => format!("line {x} {y} {} {}", x + 8.0, y + 8.0),
            1 => format!("circle {x} {y} 3"),
            _ => format!("text {x} {y} 2 0 T{i}"),
        };
        s.exec(id, &cmd).unwrap();
    }
    s.exec(id, "end").unwrap();
    let (dwg, _) = s.save(id, "dwg", 0, "bench").unwrap();
    s.close(id);
    s.open(&dwg).unwrap()
}

fn main() {
    let args: Vec<usize> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let n = args.first().copied().unwrap_or(20_000);
    let edits = args.get(1).copied().unwrap_or(500);
    let mut s = Session::new();
    let id = big(&mut s, n);
    s.display(id).unwrap();
    let info = s.info(id).unwrap();
    let objects = info.split("Objects: ").nth(1).and_then(|t| t.split(|c: char| !c.is_ascii_digit()).next()).unwrap_or("?").to_string();
    let base = mb();
    println!("drawing: {objects} objects after a DWG round trip, {base:.1} MB in use");

    // Single edits, each followed by what the page asks for: info and the display.
    PEAK.store(HEAP.load(Ordering::Relaxed), Ordering::Relaxed);
    let t = Instant::now();
    let mut edit_ms = 0.0;
    for i in 0..edits {
        let t1 = Instant::now();
        let cmd = match i % 2 {
            0 => format!("line {} -50 {} -60", i, i + 5),
            _ => format!("circle {} -80 2", i),
        };
        s.exec(id, &cmd).unwrap();
        edit_ms += t1.elapsed().as_secs_f64() * 1000.0;
        s.display_changes(id).unwrap();
        s.info(id).unwrap();
        if [25, 100, edits].contains(&(i + 1)) {
            println!("  after {:>4} edits: {:7.1} MB in use (+{:.1} MB), edit alone {:.2} ms each, edit+patch+info {:.2} ms each",
                i + 1, mb(), mb() - base, edit_ms / (i + 1) as f64, t.elapsed().as_secs_f64() * 1000.0 / (i + 1) as f64);
        }
    }
    let t = Instant::now();
    for _ in 0..50 {
        s.info(id).unwrap();
    }
    let info_ms = t.elapsed().as_secs_f64() * 1000.0 / 50.0;
    let t = Instant::now();
    for i in 0..50 {
        s.exec(id, &format!("line {i} -90 {} -95", i + 3)).unwrap();
        s.display_changes(id).unwrap();
    }
    println!("  info alone {info_ms:.2} ms; edit+display patch {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / 50.0);
    println!("  peak while editing: {:.1} MB", PEAK.load(Ordering::Relaxed) as f64 / 1048576.0);
    let t = Instant::now();
    for _ in 0..edits {
        s.exec(id, "undo").unwrap();
        s.display_changes(id).unwrap();
    }
    println!("  undo x{edits} with display patch: {:.2} ms each", t.elapsed().as_secs_f64() * 1000.0 / edits as f64);
    let t = Instant::now();
    for _ in 0..edits {
        s.exec(id, "redo").unwrap();
        s.display_changes(id).unwrap();
    }
    println!("  redo x{edits} with display patch: {:.2} ms each", t.elapsed().as_secs_f64() * 1000.0 / edits as f64);

    let t = Instant::now();
    s.exec(id, "pick 100 100 1").unwrap();
    let pick_first = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    for i in 0..50 {
        s.exec(id, &format!("pick {} {} 1", i * 7, i * 3)).unwrap();
    }
    let pick = t.elapsed().as_secs_f64() * 1000.0 / 50.0;
    let t = Instant::now();
    for i in 0..50 {
        s.exec(id, &format!("snap {} {} 1 255", i * 7, i * 3)).unwrap();
    }
    let snap = t.elapsed().as_secs_f64() * 1000.0 / 50.0;
    println!("  pick {pick:.2} ms (first after an edit {pick_first:.1} ms), snap {snap:.2} ms");
    let t = Instant::now();
    let disp = s.display(id).unwrap();
    println!("  full display list: {:.1} KB in {:.1} ms", disp.len() as f64 / 1024.0, t.elapsed().as_secs_f64() * 1000.0);

    // Many edits as one command.
    for k in [1000, 2000] {
        let mut s = Session::new();
        let id = s.new_drawing(false).unwrap();
        let before = mb();
        let t = Instant::now();
        s.exec(id, "begin").unwrap();
        for i in 0..k {
            s.exec(id, &format!("line {i} 0 {i} 10")).unwrap();
        }
        s.exec(id, "end").unwrap();
        println!("grouped: {k} lines in one command: +{:.1} MB, {:.0} ms", mb() - before, t.elapsed().as_secs_f64() * 1000.0);
        let mut s2 = Session::new();
        let id2 = s2.new_drawing(false).unwrap();
        let before = mb();
        let t = Instant::now();
        for i in 0..k {
            s2.exec(id2, &format!("line {i} 0 {i} 10")).unwrap();
        }
        println!("separate: {k} lines: +{:.1} MB, {:.0} ms", mb() - before, t.elapsed().as_secs_f64() * 1000.0);
    }
}

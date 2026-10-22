//! Bridge between the CAD core and the platform apps.
//!
//! This is the only crate in the workspace with `unsafe` code, and it keeps
//! it at the boundary: exporting entry points under the names the platform
//! looks up, and taking ownership of file descriptors the app hands over.
//! Everything behind that is safe Rust over `cad-core`.
//!
//! # Android
//!
//! `apps/android` declares, in `com.withshalom.cad.Native`:
//!
//! ```java
//! static native int abiVersion();
//! static native int view(int drawingFd, int infoFd, int svgFd);
//! ```
//!
//! Only integers cross the boundary. The app opens the drawing the user
//! picked, and two output files in its cache, detaches their file
//! descriptors and passes them in. This side reads and writes through them
//! and closes them. No JNI objects are touched, so there are no JNI
//! bindings to maintain and no third-party crate. The same functions work
//! on any Unix-like system, which is how the tests exercise them on Linux,
//! and iOS can call them the same way later.

#![warn(clippy::undocumented_unsafe_blocks)]
#![deny(unsafe_op_in_unsafe_fn)]

use cad_core::export::svg::{self, SvgOptions};
use cad_core::model::{units_name, Drawing, Handle, Layer, ObjectKind, Severity};
use std::collections::BTreeMap;
use cad_core::native;
use cad_core::render::{self, Scene};
use std::fmt::Write as _;

mod commands;
pub mod session;
#[cfg(target_family = "wasm")]
mod web;

/// Version of the interface the entry points implement. The app checks it
/// at startup, so an app and a library from different builds fail with a
/// clear message instead of in some odd way later. Bump it whenever an
/// entry point, a status code or an output changes meaning.
pub const ABI_VERSION: i32 = 1;

/// Status codes returned by the entry points.
pub mod status {
    /// Success: the outputs hold the summary and the drawing.
    pub const OK: i32 = 0;
    /// A file descriptor was negative, or the same one was passed twice.
    /// The valid ones were still closed.
    pub const BAD_ARGUMENT: i32 = 1;
    /// The drawing could not be read from its descriptor.
    pub const READ_FAILED: i32 = 2;
    /// The bytes are not a drawing the core can open.
    pub const NOT_A_DRAWING: i32 = 3;
    /// An output could not be written.
    pub const WRITE_FAILED: i32 = 4;
    /// The core panicked. That is a bug: the core is meant to report bad
    /// input, never to panic on it.
    pub const INTERNAL_ERROR: i32 = 5;
}

/// What the viewer shows for one drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// A short plain-text summary, one fact per line.
    pub info: String,
    /// Model space as an SVG document on a dark background.
    pub svg: String,
}

/// Open a drawing (native `.blc` or DWG, told apart by content) and
/// prepare what the viewer shows. The error is a message for the user.
pub fn view(bytes: &[u8]) -> Result<View, String> {
    let d = cad_core::open_bytes(bytes).map_err(|e| e.to_string())?;
    let scene = render::model_space(&d);
    let svg = svg::to_svg(&scene, &SvgOptions::default());
    Ok(View {
        info: summary(bytes, &d, &scene),
        svg,
    })
}

/// Most problems listed by name in the summary; the rest are counted.
const MAX_PROBLEMS: usize = 20;

fn summary(bytes: &[u8], d: &Drawing, scene: &Scene) -> String {
    let mut s = String::new();
    if native::looks_native(bytes) {
        match native::inspect(bytes) {
            Ok(n) => {
                let _ = writeln!(
                    s,
                    "Native .blc {}.{}, generation {}, {}",
                    n.format.0,
                    n.format.1,
                    n.generation,
                    count(n.change_sets, "change set", "change sets")
                );
                let _ = writeln!(s, "Written by {}", n.writer);
            }
            Err(_) => {
                let _ = writeln!(s, "Native .blc");
            }
        }
        let _ = writeln!(s, "Source: {}", d.version);
    } else {
        let _ = writeln!(s, "DWG {}", d.version);
    }
    summary_rest(&mut s, d, scene, &census(d));
    s
}

/// [`summary`] for a drawing already open, given what it was opened from
/// ("blc", "dwg", "dxf" or "new").
pub(crate) fn summary_of(d: &Drawing, scene: &Scene, source: &str, c: &Census) -> String {
    let mut s = String::new();
    match source {
        "new" => {
            let _ = writeln!(s, "New drawing");
        }
        "blc" => {
            let _ = writeln!(s, "Native .blc, source {}", d.version);
        }
        "dxf" => {
            let _ = writeln!(s, "DXF {}", d.version);
        }
        _ => {
            let _ = writeln!(s, "DWG {}", d.version);
        }
    }
    summary_rest(&mut s, d, scene, c);
    s
}

/// What the summary and the app's layer list count, in one pass over the
/// objects (a large drawing is read once per refresh, not once per layer).
pub(crate) struct Census<'a> {
    /// Layers in table order, then any the table does not list.
    pub layers: Vec<(Handle, &'a Layer)>,
    /// Entities on each layer, as [`Drawing::layer_entity_count`] counts.
    pub by_layer: BTreeMap<Handle, usize>,
    pub failed: usize,
    pub pending: usize,
    pub xdata_carriers: usize,
    pub xdata_apps: BTreeMap<&'a str, usize>,
}

pub(crate) fn census(d: &Drawing) -> Census<'_> {
    let mut c = Census {
        layers: Vec::new(),
        by_layer: BTreeMap::new(),
        failed: 0,
        pending: 0,
        xdata_carriers: 0,
        xdata_apps: BTreeMap::new(),
    };
    let mut stray = Vec::new();
    for o in d.objects.values() {
        match &o.kind {
            ObjectKind::Failed(_) | ObjectKind::Damaged(_) => c.failed += 1,
            ObjectKind::Unsupported | ObjectKind::NewerKind(_) => c.pending += 1,
            ObjectKind::Layer(l) => stray.push((o.handle, l)),
            _ => {}
        }
        if let Some(e) = &o.entity {
            if !matches!(
                o.kind,
                ObjectKind::BlockBegin { .. } | ObjectKind::BlockEnd | ObjectKind::SeqEnd | ObjectKind::Vertex(_)
            ) {
                *c.by_layer.entry(e.layer).or_default() += 1;
            }
        }
        if !o.xdata.is_empty() {
            c.xdata_carriers += 1;
            for x in &o.xdata {
                *c.xdata_apps.entry(x.app_name().unwrap_or("(missing APPID)")).or_default() += 1;
            }
        }
    }
    // Table order first, then the rest, as `Drawing::layers` gives them.
    c.layers = d.layers_in_table();
    for (h, l) in stray {
        if !c.layers.iter().any(|(x, _)| *x == h) {
            c.layers.push((h, l));
        }
    }
    c
}

fn summary_rest(s: &mut String, d: &Drawing, scene: &Scene, c: &Census) {
    let total = d.objects.len();
    let (failed, pending) = (c.failed, c.pending);
    let _ = writeln!(
        s,
        "Objects: {total} ({} decoded, {pending} not decoded yet, {failed} failed)",
        total - failed - pending
    );
    let h = &d.header;
    // The units the app works in (`info` in session.rs reads the same
    // value): a DWG header that failed its cross-check still has them
    // where the core wrote them.
    match h.insunits {
        Some(u) => {
            let shown = match (h.lunits, h.luprec) {
                (Some(f), Some(p)) => format!(" ({})", display_format(f, p)),
                _ => String::new(),
            };
            let _ = writeln!(s, "Units: {}{shown}", units_name(u));
        }
        // Not given: unitless, as the DXF reference reads INSUNITS 0.
        None => {
            let _ = writeln!(s, "Units: {}", units_name(0));
        }
    }
    let layers = &c.layers;
    if !layers.is_empty() {
        let current = d
            .current_layer()
            .and_then(|l| d.layer(l))
            .map(|l| format!(", current {}", l.name))
            .unwrap_or_default();
        let _ = writeln!(s, "Layers: {}{current}", layers.len());
    }

    let st = &scene.stats;
    let _ = write!(
        s,
        "Model space: {} drawn, {} hidden, {} failed",
        count(st.entities_drawn, "entity", "entities"),
        st.hidden,
        st.failed
    );
    if st.depth_limited > 0 {
        let _ = write!(s, ", {} beyond the nesting limit", st.depth_limited);
    }
    if st.over_budget > 0 {
        let _ = write!(
            s,
            ", {} not drawn (more than {} to draw)",
            st.over_budget,
            render::MAX_VISITS
        );
    }
    s.push('\n');
    if !st.skipped.is_empty() {
        let list: Vec<String> = st
            .skipped
            .iter()
            .map(|(k, v)| format!("{k} ×{v}"))
            .collect();
        let _ = writeln!(s, "Not drawn yet: {}", list.join(", "));
    }
    if !scene.bounds.is_empty() {
        let b = scene.bounds;
        let _ = writeln!(
            s,
            "Extents: ({}, {}) – ({}, {})",
            short(b.min.x),
            short(b.min.y),
            short(b.max.x),
            short(b.max.y)
        );
    }

    let (apps, carriers) = (&c.xdata_apps, c.xdata_carriers);
    if carriers > 0 {
        let list: Vec<String> = apps.iter().map(|(n, k)| format!("{n} ×{k}")).collect();
        let _ = writeln!(
            s,
            "XDATA: on {} ({})",
            count(carriers, "object", "objects"),
            list.join(", ")
        );
    }

    let problems: Vec<_> = d
        .diagnostics
        .iter()
        .filter(|x| !matches!(x.severity, Severity::Info))
        .collect();
    if !problems.is_empty() {
        let _ = writeln!(s, "Problems: {}", problems.len());
        for x in problems.iter().take(MAX_PROBLEMS) {
            let tag = match x.severity {
                Severity::Error => "error",
                _ => "warning",
            };
            let _ = writeln!(s, "  {tag}: {}", x.message);
        }
        if problems.len() > MAX_PROBLEMS {
            let _ = writeln!(s, "  … and {} more", problems.len() - MAX_PROBLEMS);
        }
    }
    s.truncate(s.trim_end().len());
}

/// How the drawing shows lengths (`$LUNITS`, `$LUPREC`), in words.
fn display_format(lunits: u16, luprec: u16) -> String {
    use cad_core::units::LinearFormat as F;
    let p = luprec.min(8);
    match F::from_lunits(lunits) {
        F::Scientific => format!("scientific, {p} places"),
        F::Decimal => format!("decimal, {p} places"),
        F::Engineering => format!("feet and decimal inches, {p} places"),
        F::Architectural => format!("feet and inches to 1/{}", 1u32 << p),
        F::Fractional => format!("fractions to 1/{}", 1u32 << p),
    }
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A coordinate with at most four decimals, without trailing zeros.
fn short(v: f64) -> String {
    let t = format!("{v:.4}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" {
        "0".into()
    } else {
        t.into()
    }
}

#[cfg(unix)]
pub use unix::view_fds;

#[cfg(unix)]
mod unix {
    use super::{status, view, View};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::{FromRawFd, RawFd};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    /// Take ownership of a descriptor, or `None` for a negative one.
    ///
    /// # Safety
    ///
    /// `fd` must be negative, or an open descriptor that the caller owns and
    /// gives up: nothing else may use or close it afterwards.
    unsafe fn take(fd: RawFd) -> Option<File> {
        if fd < 0 {
            return None;
        }
        // SAFETY: passed on from this function's contract. The `File`
        // becomes the descriptor's only owner and closes it when dropped.
        Some(unsafe { File::from_raw_fd(fd) })
    }

    /// [`view`] over file descriptors: read the drawing from `drawing`,
    /// write the summary to `info` and the SVG to `svg`, and return a
    /// [`status`] code. When it fails, `info` holds the reason if it can
    /// still be written.
    ///
    /// All three descriptors are closed, whatever the outcome. A negative
    /// one, or the same descriptor passed twice, gets
    /// [`status::BAD_ARGUMENT`].
    ///
    /// # Safety
    ///
    /// Each descriptor must be negative, or open and owned by the caller,
    /// who gives it up: nothing else may use or close it after the call.
    /// On Android, `ParcelFileDescriptor.detachFd()` hands a descriptor
    /// over this way.
    pub unsafe fn view_fds(drawing: RawFd, info: RawFd, svg: RawFd) -> i32 {
        let fds = [drawing, info, svg];
        let first = |i: usize| !fds[..i].contains(&fds[i]);
        if (0..3).any(|i| fds[i] >= 0 && !first(i)) {
            // Close each distinct descriptor once, then refuse.
            for i in (0..3).filter(|&i| first(i)) {
                // SAFETY: from this function's contract; each distinct
                // descriptor is taken once.
                drop(unsafe { take(fds[i]) });
            }
            return status::BAD_ARGUMENT;
        }
        // Take all three before checking any, so every valid one is closed.
        // SAFETY: from this function's contract; the three are distinct.
        let (drawing, info, svg) = unsafe { (take(drawing), take(info), take(svg)) };
        let (Some(mut drawing), Some(mut info), Some(mut svg)) = (drawing, info, svg) else {
            return status::BAD_ARGUMENT;
        };
        let mut bytes = Vec::new();
        if let Err(e) = drawing.read_to_end(&mut bytes) {
            let _ = write!(info, "The drawing could not be read: {e}");
            return status::READ_FAILED;
        }
        drop(drawing);
        match catch_unwind(AssertUnwindSafe(|| view(&bytes))) {
            Ok(Ok(View {
                info: text,
                svg: doc,
            })) => {
                if info.write_all(text.as_bytes()).is_err()
                    || svg.write_all(doc.as_bytes()).is_err()
                {
                    return status::WRITE_FAILED;
                }
                status::OK
            }
            Ok(Err(reason)) => {
                let _ = info.write_all(reason.as_bytes());
                status::NOT_A_DRAWING
            }
            Err(panic) => {
                let what = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "no message".into());
                let _ = write!(info, "Internal error in the drawing engine: {what}");
                status::INTERNAL_ERROR
            }
        }
    }
}

/// JNI entry points for `com.withshalom.cad.Native` in `apps/android`.
///
/// JNI passes the environment and the class as the first two arguments.
/// Neither is used, since only integers cross the boundary. The names
/// follow the JNI rule `Java_<package>_<class>_<method>`, with the dots of
/// the package written as underscores.
#[cfg(unix)]
#[allow(non_snake_case)]
pub mod jni {
    use std::ffi::c_void;

    /// `static native int abiVersion()`: [`crate::ABI_VERSION`].
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_withshalom_cad_Native_abiVersion(
        _env: *mut c_void,
        _class: *mut c_void,
    ) -> i32 {
        crate::ABI_VERSION
    }

    /// `static native int view(int drawingFd, int infoFd, int svgFd)`:
    /// [`crate::view_fds`].
    ///
    /// # Safety
    ///
    /// As for [`crate::view_fds`]. `Native.view` in the app requires
    /// descriptors detached from their `ParcelFileDescriptor`s.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn Java_com_withshalom_cad_Native_view(
        _env: *mut c_void,
        _class: *mut c_void,
        drawing_fd: i32,
        info_fd: i32,
        svg_fd: i32,
    ) -> i32 {
        // SAFETY: the app hands over detached descriptors, as `Native.view`
        // requires.
        unsafe { crate::view_fds(drawing_fd, info_fd, svg_fd) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cad_core::dwg::DwgVersion;
    use cad_core::geom::Vec3;
    use cad_core::model::*;

    fn entity(handle: u64, name: &str, code: u16, kind: ObjectKind) -> Object {
        Object {
            handle: Handle(handle),
            type_code: code,
            type_name: name.into(),
            owner: Handle::NULL,
            reactors: vec![],
            xdictionary: Handle::NULL,
            entity: Some(EntityCommon {
                space: Space::Model,
                linetype_scale: 1.0,
                ..Default::default()
            }),
            kind,
            offset: 0,
            size: 0,
            note: None,
            raw: None,
            xdata: Vec::new(),
            unknown_tags: Vec::new(),
        }
    }

    fn drawing() -> Drawing {
        let mut line = entity(
            0x100,
            "LINE",
            0x13,
            ObjectKind::Line(Line {
                start: Vec3::new(0.0, 0.0, 0.0),
                end: Vec3::new(200.0, 100.0, 0.0),
                extrusion: Vec3::Z,
                ..Default::default()
            }),
        );
        line.xdata = vec![XData {
            app: XDataApp::Name("TEST_APP".into()),
            items: vec![XValue::Str("part 7".into())],
        }];
        let circle = entity(
            0x101,
            "CIRCLE",
            0x12,
            ObjectKind::Circle(Circle {
                center: Vec3::new(50.0, 50.0, 0.0),
                radius: 10.0,
                extrusion: Vec3::Z,
                ..Default::default()
            }),
        );
        let mut d = Drawing {
            version: DwgVersion::R2018,
            maintenance_version: 0,
            codepage: 30,
            header: HeaderVars {
                verified: true,
                insunits: Some(4),
                handseed: Some(0x200),
                ..Default::default()
            },
            classes: vec![],
            objects: [line, circle].into_iter().map(|o| (o.handle, o)).collect(),
            sections: vec![],
            diagnostics: vec![],
            index: DrawingIndex::default(),
        };
        d.rebuild_index();
        d
    }

    /// A native `.blc` file with a line and a circle in model space.
    fn drawing_bytes() -> Vec<u8> {
        native::to_bytes(&drawing(), &native::WriteOptions::default())
    }

    #[test]
    fn a_drawing_is_summarised_and_rendered() {
        let v = view(&drawing_bytes()).expect("opens");
        let expected = [
            "Native .blc ",
            "Source: AC1032",
            "Objects: 2 (2 decoded, 0 not decoded yet, 0 failed)",
            "Units: millimeters",
            "Model space: 2 entities drawn, 0 hidden, 0 failed",
            "Extents: (0, 0) – (200, 100)",
            "XDATA: on 1 object (TEST_APP ×1)",
        ];
        for line in expected {
            assert!(v.info.contains(line), "missing {line:?} in\n{}", v.info);
        }
        assert!(!v.info.ends_with('\n'));
        assert!(!v.info.contains("Problems"), "{}", v.info);
        assert!(v.svg.contains("<svg "), "{}", v.svg);
        assert!(v.svg.trim_end().ends_with("</svg>"), "{}", v.svg);
    }

    #[test]
    fn a_new_style_drawing_shows_its_display_units_and_layers() {
        let bytes = native::to_bytes(&cad_core::samples::plate(), &Default::default());
        let v = view(&bytes).expect("opens");
        for line in [
            "Units: millimeters (decimal, 2 places)",
            "Layers: 6, current VISIBLE",
        ] {
            assert!(v.info.contains(line), "missing {line:?} in\n{}", v.info);
        }
        assert!(!v.info.contains("Not drawn yet"), "{}", v.info);
        // Drawn on the design canvas, text as strokes.
        assert!(v.svg.contains("#0d1013") && !v.svg.contains("<text"));
        assert_eq!(display_format(4, 4), "feet and inches to 1/16");
    }

    #[test]
    fn bytes_that_are_not_a_drawing_are_refused_with_a_reason() {
        for bytes in [&b""[..], b"hello", &[0u8; 4096]] {
            let reason = view(bytes).expect_err("not a drawing");
            assert!(!reason.trim().is_empty());
        }
    }

    #[test]
    fn problems_are_listed_up_to_a_limit() {
        let mut d = drawing();
        d.diagnostics.push(Diagnostic {
            severity: Severity::Info,
            message: "not shown".into(),
        });
        d.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            message: "first".into(),
        });
        for i in 0..24 {
            d.diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("warning {i}"),
            });
        }
        let text = summary(b"", &d, &render::model_space(&d));
        assert!(text.starts_with("DWG AC1032"), "{text}");
        assert!(text.contains("Problems: 25\n  error: first\n  warning: warning 0\n"));
        assert!(
            text.contains("warning: warning 18\n  … and 5 more"),
            "{text}"
        );
        assert!(!text.contains("not shown") && !text.contains("warning 19"));
    }

    #[test]
    fn coordinates_are_short() {
        assert_eq!(short(0.0), "0");
        assert_eq!(short(-0.00001), "0");
        assert_eq!(short(12.5), "12.5");
        assert_eq!(short(-12.34567), "-12.3457");
        assert_eq!(short(200.0), "200");
    }

    #[cfg(unix)]
    mod descriptors {
        use super::*;
        use crate::jni::{
            Java_com_withshalom_cad_Native_abiVersion, Java_com_withshalom_cad_Native_view,
        };
        use std::fs::File;
        use std::io::{PipeReader, Read, Write};
        use std::os::fd::{IntoRawFd, RawFd};
        use std::ptr::null_mut;
        use std::sync::mpsc;
        use std::time::Duration;

        /// Read one end of a pipe to its end on another thread, so writers
        /// never block on a full pipe.
        fn drain(mut r: PipeReader) -> mpsc::Receiver<std::io::Result<Vec<u8>>> {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = tx.send(r.read_to_end(&mut buf).map(|_| buf));
            });
            rx
        }

        /// What was written to a pipe. The read only ends once the write
        /// end is closed, and the library held the only copy, so this also
        /// checks that the library closed it.
        fn finished(rx: mpsc::Receiver<std::io::Result<Vec<u8>>>) -> String {
            let bytes = rx
                .recv_timeout(Duration::from_secs(60))
                .expect("the library left an output descriptor open")
                .expect("read");
            String::from_utf8(bytes).expect("UTF-8")
        }

        /// A pipe for one output: the descriptor to hand over and a receiver
        /// for what arrives.
        fn output() -> (RawFd, mpsc::Receiver<std::io::Result<Vec<u8>>>) {
            let (r, w) = std::io::pipe().expect("pipe");
            (w.into_raw_fd(), drain(r))
        }

        fn scratch_file(tag: &str, bytes: &[u8]) -> std::path::PathBuf {
            let p = std::env::temp_dir().join(format!("cad-ffi-{tag}-{}.blc", std::process::id()));
            std::fs::write(&p, bytes).expect("write scratch file");
            p
        }

        #[test]
        fn the_interface_version_is_exported() {
            assert_eq!(
                Java_com_withshalom_cad_Native_abiVersion(null_mut(), null_mut()),
                ABI_VERSION
            );
        }

        #[test]
        fn a_drawing_is_viewed_through_descriptors() {
            let bytes = drawing_bytes();
            let path = scratch_file("view", &bytes);
            let input = File::open(&path).expect("open").into_raw_fd();
            let (info, info_rx) = output();
            let (svg, svg_rx) = output();
            // SAFETY: three distinct descriptors, owned here and given up.
            let result = unsafe {
                Java_com_withshalom_cad_Native_view(null_mut(), null_mut(), input, info, svg)
            };
            let _ = std::fs::remove_file(&path);
            assert_eq!(result, status::OK);
            let got = View {
                info: finished(info_rx),
                svg: finished(svg_rx),
            };
            assert_eq!(got, view(&bytes).expect("opens"));
        }

        #[test]
        fn a_drawing_from_a_pipe_is_read_whole() {
            // Storage providers can hand over a pipe instead of a file.
            let (r, mut w) = std::io::pipe().expect("pipe");
            let bytes = drawing_bytes();
            let feeder = std::thread::spawn(move || w.write_all(&bytes));
            let (info, info_rx) = output();
            let (svg, svg_rx) = output();
            // SAFETY: three distinct descriptors, owned here and given up.
            let result = unsafe { view_fds(r.into_raw_fd(), info, svg) };
            feeder.join().expect("feeder").expect("fed");
            assert_eq!(result, status::OK, "{}", finished(info_rx));
            assert!(finished(svg_rx).contains("<svg "));
        }

        #[test]
        fn a_file_that_is_not_a_drawing_is_refused_with_a_reason() {
            let path = scratch_file("junk", b"this is not a drawing");
            let input = File::open(&path).expect("open").into_raw_fd();
            let (info, info_rx) = output();
            let (svg, svg_rx) = output();
            // SAFETY: three distinct descriptors, owned here and given up.
            let result = unsafe { view_fds(input, info, svg) };
            let _ = std::fs::remove_file(&path);
            assert_eq!(result, status::NOT_A_DRAWING);
            assert!(!finished(info_rx).trim().is_empty());
            assert_eq!(finished(svg_rx), "");
        }

        #[test]
        fn negative_descriptors_are_refused_and_the_others_still_closed() {
            let (info, info_rx) = output();
            let (svg, svg_rx) = output();
            // SAFETY: two distinct descriptors, owned here and given up.
            assert_eq!(unsafe { view_fds(-1, info, svg) }, status::BAD_ARGUMENT);
            assert_eq!(finished(info_rx), "");
            assert_eq!(finished(svg_rx), "");
        }

        #[test]
        fn a_descriptor_passed_twice_is_refused_and_closed_once() {
            let (out, out_rx) = output();
            let (other, other_rx) = output();
            // SAFETY: two distinct descriptors, owned here and given up; the
            // repeat is what is being tested and must not be closed twice.
            assert_eq!(unsafe { view_fds(out, out, other) }, status::BAD_ARGUMENT);
            assert_eq!(finished(out_rx), "");
            assert_eq!(finished(other_rx), "");
        }

        #[test]
        fn a_drawing_that_cannot_be_read_is_reported() {
            // The write end of a pipe cannot be read from.
            let (_r, w) = std::io::pipe().expect("pipe");
            let (info, info_rx) = output();
            let (svg, svg_rx) = output();
            // SAFETY: three distinct descriptors, owned here and given up.
            let result = unsafe { view_fds(w.into_raw_fd(), info, svg) };
            assert_eq!(result, status::READ_FAILED);
            assert!(finished(info_rx).starts_with("The drawing could not be read"));
            assert_eq!(finished(svg_rx), "");
        }

        #[test]
        fn an_output_that_cannot_be_written_is_reported() {
            // The read end of a pipe cannot be written to.
            let (r, _w) = std::io::pipe().expect("pipe");
            let (svg, svg_rx) = output();
            let path = scratch_file("unwritable", &drawing_bytes());
            let input = File::open(&path).expect("open").into_raw_fd();
            // SAFETY: three distinct descriptors, owned here and given up.
            let result = unsafe { view_fds(input, r.into_raw_fd(), svg) };
            let _ = std::fs::remove_file(&path);
            assert_eq!(result, status::WRITE_FAILED);
            finished(svg_rx);
        }
    }
}

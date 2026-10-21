//! CAD core: a clean-room CAD engine written from scratch in Rust with no
//! third-party dependencies. It saves to its own native format, reads and
//! writes DWG and DXF, and does every drafting operation itself: programs
//! on top of it (the web app, the desktop and phone shells) ask for points,
//! objects and values and hand them to the core.
//!
//! Files:
//! - [`native`]: the native `.cad` format (read, write, incremental save,
//!   `.cdl` locks)
//! - [`dwg`]: DWG reader (R13–R2000 and R2004–R2018; R2007 pending) and
//!   writer (AutoCAD 2018 format)
//! - [`dxf`]: DXF reader and writer, ASCII and binary
//! - [`export`]: SVG, PDF and display-list output; dimension graphics as
//!   blocks for the files other programs read
//!
//! The drawing:
//! - [`model`]: the drawing database, keyed by DWG-style handles, and
//!   editing (layers, records, new entities), with undoable changes
//! - [`standards`]: default layers, linetypes, text and dimension styles,
//!   canvas colours, and the templates new drawings start from
//! - [`units`]: unit conversion, typed lengths and angles, display formats
//! - [`layouts`]: layouts (paper space sheets), paper sizes, viewports
//! - [`sheet`]: a sheet ready to plot: border, title block, scaled viewport
//! - [`blocks`]: block definitions, inserts, attributes, explode
//!
//! Drafting:
//! - [`construct`]: arcs, dimensions, text boxes and construction lines
//!   from picked points; measuring
//! - [`modify`]: the geometry of offset, trim, extend, fillet, chamfer,
//!   stretch and transforms
//! - [`ops`]: the modify and annotate commands on a drawing (arrays,
//!   hatches, leaders, chained dimensions, unit conversion)
//! - [`boundary`]: the closed region around a point, for hatching
//! - [`grips`]: grips on selected objects, and dragging them
//! - [`snap`]: object snaps, picking and window selection
//! - [`check`]: a flat part's holes against the shop's limits
//! - [`takeoff`]: lengths, contours, areas and weight for a quote
//!
//! Showing it:
//! - [`geom`]: vectors, transforms, OCS, curves
//! - [`render`]: turns a drawing into plan-view display geometry, with
//!   linetype dashes, hatch patterns and dimension graphics
//! - [`font`]: the app's own single-stroke font and text layout
//! - [`text`]: code pages, escape sequences and MTEXT formatting codes
//! - [`raster`]: software rasteriser and PNG writer
//! - [`samples`]: sample drawings
//!
//! The core never panics on bad input by design: decoding errors are
//! returned or recorded per object. It uses no `unsafe` code.

#![forbid(unsafe_code)]

pub mod blocks;
pub mod boundary;
pub mod check;
pub mod codec;
pub mod construct;
pub mod dwg;
pub mod dxf;
pub mod error;
pub mod export;
pub mod font;
pub mod geom;
pub mod grips;
pub mod layouts;
pub mod model;
pub mod modify;
pub mod native;
pub mod ops;
pub mod raster;
pub mod render;
pub mod samples;
pub mod sheet;
pub mod snap;
pub mod standards;
pub mod takeoff;
pub mod text;
pub mod units;

pub use error::{Error, ErrorKind, Result};
pub use model::Drawing;
pub use standards::{new_drawing, Template};

/// Open a drawing: native `.cad` or DWG, detected from the file's first
/// bytes rather than its name.
pub fn open(path: impl AsRef<std::path::Path>) -> Result<Drawing> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)
        .map_err(|e| Error::new(ErrorKind::Io, format!("{}: {e}", path.display())))?;
    open_bytes(&bytes)
}

/// [`open`] for data already in memory.
pub fn open_bytes(bytes: &[u8]) -> Result<Drawing> {
    if native::looks_native(bytes) {
        native::read_bytes(bytes)
    } else if dxf::looks_like_dxf(bytes) {
        dxf::read_bytes(bytes)
    } else {
        dwg::read_bytes(bytes)
    }
}

/// Save a drawing in the native format (full snapshot).
pub fn save(drawing: &Drawing, path: impl AsRef<std::path::Path>) -> Result<u64> {
    native::write_file(drawing, path, &native::WriteOptions::default())
}

/// Time the app gave [`set_time`], for platforms without a clock.
static APP_TIME: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Tell the core the time, in seconds since 1970 (UTC). Only needed where
/// the platform has no clock the standard library can read: WebAssembly
/// in a browser, where the page passes `Date.now()` before each save.
pub fn set_time(unix_seconds: i64) {
    APP_TIME.store(unix_seconds, std::sync::atomic::Ordering::Relaxed);
}

/// Seconds since 1970 (UTC): the system clock, or where there is none,
/// the time last given to [`set_time`] (0 if none was).
pub fn now_unix() -> i64 {
    if cfg!(all(target_family = "wasm", target_os = "unknown")) {
        return APP_TIME.load(std::sync::atomic::Ordering::Relaxed);
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The software's product name. The one place it is defined in code, so a
/// rename is a one-line change (plus the README title). It names the
/// software only: the file format stays `.cad` and the crates keep their
/// `cad-` names. BlackCAD is the working name for development; it is the
/// name a DWG it writes gives as the program that saved it.
pub const PRODUCT_NAME: &str = "BlackCAD";

/// Core library version: the software's own number, `0.MINOR.PATCH` until
/// release. Not the `.cad` format's version (see `native::FORMAT_MAJOR`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

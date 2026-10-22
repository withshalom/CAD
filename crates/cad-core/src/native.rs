//! The native file format (`.cad`), from the `cad-native` crate.
//!
//! Everything `cad_native` offers is here under the same names. Writing
//! from here ([`to_bytes`], [`write_file`]) adds what the core draws and
//! the format leaves to the program: a preview of model space and the
//! core's name and version as the file's writer. [`lock`] names the core
//! in lock files the same way.
//!
//! ```no_run
//! let d = cad_core::open("part.dwg")?;
//! cad_core::native::write_file(&d, "part.cad", &Default::default())?;
//! let again = cad_core::native::read_file("part.cad")?;
//! // Or open once and look things up without loading everything:
//! let f = cad_core::native::NativeFile::open("part.cad")?;
//! let props = f.properties()?;
//! # Ok::<(), cad_core::Error>(())
//! ```

pub use cad_native::*;

use crate::error::Result;
use crate::model::Drawing;
use std::path::Path;

/// Width of the preview, in pixels.
const PREVIEW_WIDTH_PX: f64 = 512.0;

/// The name and version the core records as the writer of its files and
/// the holder of its locks.
fn writer() -> String {
    format!("cad-core {}", crate::VERSION)
}

/// `opts` with the core's preview and name where the caller left them out.
fn with_core(opts: &WriteOptions) -> WriteOptions {
    let mut o = opts.clone();
    if o.preview_svg.is_none() {
        o.preview_svg = Some(preview_svg_of);
    }
    if o.writer.is_none() {
        o.writer = Some(writer());
    }
    o
}

/// The preview the core stores in `.cad` files: model space as SVG on a
/// light background, or `None` when model space is empty.
pub fn preview_svg_of(d: &Drawing) -> Option<String> {
    let scene = crate::render::model_space(d);
    if scene.items.is_empty() {
        return None;
    }
    Some(crate::export::svg::to_svg(
        &scene,
        &crate::export::svg::SvgOptions {
            dark_background: false,
            width_px: PREVIEW_WIDTH_PX,
            ..Default::default()
        },
    ))
}

/// Serialize a drawing to bytes (generation 1), with the core's preview
/// and name ([`cad_native::to_bytes`]).
pub fn to_bytes(d: &Drawing, opts: &WriteOptions) -> Vec<u8> {
    cad_native::to_bytes(d, &with_core(opts))
}

/// Write a full snapshot, merging any journal, with the core's preview
/// and name ([`cad_native::write_file`]).
pub fn write_file(d: &Drawing, path: impl AsRef<Path>, opts: &WriteOptions) -> Result<u64> {
    cad_native::write_file(d, path, &with_core(opts))
}

pub mod lock {
    //! `.cdl` lock files ([`cad_native::lock`]), naming the core as the
    //! program that holds the lock.

    pub use cad_native::lock::*;

    use crate::error::Result;
    use std::path::Path;

    /// Lock a drawing for editing. A stale lock is taken over.
    pub fn acquire(drawing: impl AsRef<Path>) -> Result<Acquire> {
        acquire_as(drawing, user_name())
    }

    /// [`acquire`] with an explicit user name (for apps with their own
    /// sign-in).
    pub fn acquire_as(drawing: impl AsRef<Path>, user: String) -> Result<Acquire> {
        acquire_with(drawing, user, &super::writer())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_stores_a_preview_and_its_name() {
        let d = crate::samples::plate();
        let bytes = to_bytes(&d, &WriteOptions::default());
        let svg = preview_svg(&bytes).unwrap().expect("preview");
        assert!(svg.starts_with("<svg") || svg.starts_with("<?xml"), "{}", &svg[..40]);
        let info = inspect(&bytes).unwrap();
        assert_eq!(info.writer, writer());
        // Asked for none, none is stored.
        let none = to_bytes(&d, &WriteOptions { preview: false, ..Default::default() });
        assert_eq!(preview_svg(&none).unwrap(), None);
    }

    #[test]
    fn the_format_crate_alone_names_itself_and_stores_no_preview() {
        let d = crate::samples::plate();
        let bytes = cad_native::to_bytes(&d, &WriteOptions::default());
        assert_eq!(preview_svg(&bytes).unwrap(), None);
        assert_eq!(inspect(&bytes).unwrap().writer, format!("cad-native {}", cad_native::VERSION));
    }

    #[test]
    fn drawings_from_the_templates_round_trip() {
        for t in [crate::standards::Template::Metric, crate::standards::Template::Imperial] {
            let d = crate::standards::new_drawing(t);
            let back = read_bytes(&to_bytes(&d, &WriteOptions::default())).unwrap();
            assert_eq!(d.objects, back.objects);
        }
    }
}

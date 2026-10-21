//! DXF files, read and written by the `cad-dxf` crate.
//!
//! Everything `cad_dxf` offers is here under the same names. Writing from
//! here ([`groups`], [`to_bytes`]) adds what the core draws and the DXF
//! crate leaves to the program: line families for pattern hatches, and
//! for files, graphics blocks for dimensions.
//!
//! This is the full writer; `export::dxf` is the older R12 export of a
//! rendered scene (blocks exploded), kept for programs that read only R12.

pub use cad_dxf::*;

use crate::model::Drawing;

fn options() -> WriteOptions<'static> {
    WriteOptions {
        hatch_lines: Some(&crate::render::hatch::library_lines),
    }
}

/// The drawing as the groups of an AC1032 DXF, as BlackCAD writes it.
pub fn groups(d: &Drawing) -> (Vec<encode::Group>, Vec<String>) {
    groups_with(d, &options())
}

/// The drawing as an AC1032 DXF file, as BlackCAD writes it. Dimensions
/// drawn from their definition points get graphics blocks first, so that
/// other programs draw them.
pub fn to_bytes(d: &Drawing, format: DxfFormat) -> DxfOutput {
    let d = &*crate::export::dimblocks::with_dimension_blocks(d);
    to_bytes_with(d, format, &options())
}

//! DXF output of the whole drawing database: an AC1032 (AutoCAD 2018)
//! DXF, ASCII or binary, with every table, block, entity and the objects
//! the model can express. See [`write`] for what is written and how it
//! was checked.
//!
//! This is the full writer; `export::dxf` is the older R12 export of a
//! rendered scene (blocks exploded), kept for programs that read only R12.

pub mod encode;
pub mod read;
mod write;

pub use read::{looks_like_dxf, read_bytes, read_file};

use crate::model::Drawing;

/// ASCII or binary DXF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxfFormat {
    Ascii,
    Binary,
}

/// A DXF file and what the writer had to leave out or rebuild.
#[derive(Debug, Clone)]
pub struct DxfOutput {
    pub bytes: Vec<u8>,
    pub notes: Vec<String>,
}

/// The drawing as the groups of an AC1032 DXF.
pub fn groups(d: &Drawing) -> (Vec<encode::Group>, Vec<String>) {
    write::Writer::new(d).write()
}

/// The drawing as an AC1032 DXF file.
pub fn to_bytes(d: &Drawing, format: DxfFormat) -> DxfOutput {
    // Dimensions drawn from their definition points get graphics blocks,
    // which other programs draw.
    let d = &*crate::export::dimblocks::with_dimension_blocks(d);
    let (groups, notes) = groups(d);
    let bytes = match format {
        DxfFormat::Ascii => encode::to_ascii(&groups),
        DxfFormat::Binary => encode::to_binary(&groups),
    };
    DxfOutput { bytes, notes }
}

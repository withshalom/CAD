//! BlackCAD's drawing database, shared by its DWG and DXF codecs
//! (`cad-dwg`, `cad-dxf`) and by the CAD core (`cad-core`).
//!
//! - [`model`]: the drawing database, keyed by DWG-style handles, and
//!   editing (layers, records, new entities), with undoable changes
//! - [`geom`]: vectors, transforms, OCS, curves
//! - [`text`]: code pages, escape sequences and MTEXT formatting codes
//! - [`version`]: the DWG versions, which the database records for objects
//!   it keeps as their original DWG records
//! - [`error`]: the error type of every crate built on this one
//!
//! It uses the Rust standard library only and no `unsafe` code.

#![forbid(unsafe_code)]

pub mod error;
pub mod geom;
pub mod model;
pub mod text;
pub mod version;

pub use error::{Error, ErrorKind, Result};
pub use model::Drawing;
pub use version::DwgVersion;

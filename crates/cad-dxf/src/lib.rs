//! BlackCAD's DXF reader and writer.
//!
//! Reading: [`read_bytes`] takes ASCII or binary DXF of any version into a
//! [`Drawing`] (`cad-model`'s database, re-exported as [`cad_model`]).
//! Writing: [`to_bytes`] writes the whole drawing database as an AC1032
//! (AutoCAD 2018) DXF, ASCII or binary, with every table, block, entity
//! and the objects the model can express; [`to_bytes_within`] does the
//! same within a size limit. See `write.rs` for what is written and how it
//! was checked.
//!
//! Objects a drawing keeps as DWG records are written when the DWG crate
//! (`cad-dwg`) decodes them into DXF records.

#![forbid(unsafe_code)]
// A file is only a claim: no input reaches a panic. Tests may unwrap.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unimplemented,
        clippy::todo,
        clippy::unreachable
    )
)]

pub use cad_model;
use cad_dwg as dwg;
use cad_model::{error, geom, model, text};
use cad_model::{Error, Result};

pub mod encode;
pub mod read;
mod write;

pub use read::{looks_like_dxf, read_bytes, read_file};

use crate::model::{Drawing, HatchLibrary};
use std::collections::BTreeMap;

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
    /// What was left out or filled in, for people to read.
    pub notes: Vec<String>,
    /// The objects the file does not hold, by type name: those the model
    /// keeps only as DWG records it cannot express in DXF, and those whose
    /// records did not decode. Empty when the file holds the whole drawing.
    pub left_out: BTreeMap<String, usize>,
}

/// What the writer takes from the program writing the file when it has it.
/// [`WriteOptions::default`] writes a pattern hatch's line families only
/// when the drawing holds them.
#[derive(Clone, Copy, Default)]
pub struct WriteOptions<'a> {
    /// The line families of a pattern hatch that holds none, so that
    /// other programs draw the pattern: given the drawing and the hatch,
    /// the lines to write. `None` writes such hatches with no lines.
    pub hatch_lines: Option<&'a HatchLibrary<'a>>,
}

impl std::fmt::Debug for WriteOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteOptions").field("hatch_lines", &self.hatch_lines.is_some()).finish()
    }
}

/// The drawing as the groups of an AC1032 DXF, with
/// [`WriteOptions::default`].
pub fn groups(d: &Drawing) -> (Vec<encode::Group>, Vec<String>) {
    groups_with(d, &WriteOptions::default())
}

/// The drawing as the groups of an AC1032 DXF.
pub fn groups_with(d: &Drawing, opts: &WriteOptions<'_>) -> (Vec<encode::Group>, Vec<String>) {
    let (groups, notes, _) = write::Writer::new(d, opts.hatch_lines).write();
    (groups.unwrap_or_default(), notes)
}

/// The drawing as an AC1032 DXF file, with [`WriteOptions::default`].
pub fn to_bytes(d: &Drawing, format: DxfFormat) -> DxfOutput {
    to_bytes_with(d, format, &WriteOptions::default())
}

/// The drawing as an AC1032 DXF file.
///
/// Objects are written as the drawing holds them. A dimension whose
/// graphics block is missing is written without one; a program that draws
/// dimensions adds the blocks first, so that other programs show them.
pub fn to_bytes_with(d: &Drawing, format: DxfFormat, opts: &WriteOptions<'_>) -> DxfOutput {
    let (groups, notes, left_out) = write::Writer::new(d, opts.hatch_lines).write();
    DxfOutput { bytes: encode_groups(&groups.unwrap_or_default(), format), notes, left_out }
}

/// The DXF would take more than the limit it was written within.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge {
    /// The limit, in bytes.
    pub limit: usize,
}

impl std::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the DXF would take more than {} bytes", self.limit)
    }
}

impl std::error::Error for TooLarge {}

/// [`to_bytes_with`] within `max_bytes`: writing stops as soon as the file would pass the
/// limit, so a drawing whose DXF would be enormous (a damaged file, say) never takes much more
/// memory than the limit on the way to the error. A file within the limit is the one
/// [`to_bytes_with`] writes. Sizes are counted exactly, except text with control characters
/// or (in binary) NUL bytes, which counts at its full length.
pub fn to_bytes_within(
    d: &Drawing,
    format: DxfFormat,
    opts: &WriteOptions<'_>,
    max_bytes: usize,
) -> std::result::Result<DxfOutput, TooLarge> {
    let (groups, notes, left_out) = write::Writer::new(d, opts.hatch_lines).within(max_bytes, format).write();
    let groups = groups.ok_or(TooLarge { limit: max_bytes })?;
    Ok(DxfOutput { bytes: encode_groups(&groups, format), notes, left_out })
}

fn encode_groups(groups: &[encode::Group], format: DxfFormat) -> Vec<u8> {
    match format {
        DxfFormat::Ascii => encode::to_ascii(groups),
        DxfFormat::Binary => encode::to_binary(groups),
    }
}

#[cfg(test)]
mod left_out {
    //! What a conversion leaves out is counted by type, so a program can
    //! tell a whole conversion from a partial one without reading notes.

    use super::*;

    fn corpus(name: &str) -> Option<Vec<u8>> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mosaiciron-mi201");
        std::fs::read(dir.join(name)).ok()
    }

    #[test]
    fn a_dxf_of_the_whole_drawing_leaves_nothing_out() {
        let Some(file) = corpus("mi201-r2018.dwg") else { return };
        let d = crate::dwg::read_bytes(&file).unwrap();
        let out = to_bytes(&d, DxfFormat::Ascii);
        assert!(out.left_out.is_empty(), "{:?}", out.left_out);
    }

    #[test]
    fn objects_kept_only_as_dwg_records_are_counted() {
        let Some(file) = corpus("mi201-r2010.dwg") else { return };
        let d = crate::dwg::read_bytes(&file).unwrap();
        let out = to_bytes(&d, DxfFormat::Ascii);
        assert_eq!(out.left_out.get("VISUALSTYLE"), Some(&24), "{:?}", out.left_out);
        assert_eq!(out.left_out.values().sum::<usize>(), 41, "{:?}", out.left_out);
        assert!(out.notes.iter().any(|n| n.starts_with("left out")), "{:?}", out.notes);
    }

    #[test]
    fn entities_the_model_does_not_hold_are_a_warning() {
        let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n1\n21\n0\n31\n0\n\
                   0\nNOT_AN_ENTITY_TYPE\n8\n0\n0\nENDSEC\n0\nEOF\n";
        let d = read_bytes(dxf.as_bytes()).unwrap();
        let warnings: Vec<&str> = d
            .diagnostics
            .iter()
            .filter(|x| x.severity == crate::model::Severity::Warning)
            .map(|x| x.message.as_str())
            .collect();
        assert!(warnings.iter().any(|m| m.starts_with("left out") && m.contains("NOT_AN_ENTITY_TYPE")), "{:?}", d.diagnostics);
    }
}

#[cfg(test)]
mod within {
    //! Writing within a size limit: a drawing within it is written as without one, a drawing
    //! past it is refused before its file is built, and the bound each group is counted with is
    //! never smaller than the group's encoded size.

    use super::*;

    fn drawing() -> Drawing {
        let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n1\n21\n0\n31\n0\n\
                   0\nTEXT\n8\nNOTES\n10\n1\n20\n2\n30\n0\n40\n2.5\n1\nA note with a control\tcharacter\n\
                   0\nENDSEC\n0\nEOF\n";
        read_bytes(dxf.as_bytes()).unwrap()
    }

    fn corpus() -> Option<Drawing> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mosaiciron-mi201");
        let file = std::fs::read(dir.join("mi201-r2018.dwg")).ok()?;
        crate::dwg::read_bytes(&file).ok()
    }

    #[test]
    fn a_drawing_within_the_limit_is_written_as_without_one() {
        for d in std::iter::once(drawing()).chain(corpus()) {
            for format in [DxfFormat::Ascii, DxfFormat::Binary] {
                let whole = to_bytes(&d, format);
                let within = to_bytes_within(&d, format, &WriteOptions::default(), usize::MAX).unwrap();
                assert_eq!(within.bytes, whole.bytes);
                assert_eq!(within.left_out, whole.left_out);
                // The limit is the file's own size: it fits exactly.
                let exact = to_bytes_within(&d, format, &WriteOptions::default(), whole.bytes.len()).unwrap();
                assert_eq!(exact.bytes, whole.bytes);
            }
        }
    }

    #[test]
    fn a_drawing_past_the_limit_is_refused() {
        for format in [DxfFormat::Ascii, DxfFormat::Binary] {
            let d = drawing();
            let len = to_bytes(&d, format).bytes.len();
            for limit in [0, 100, len / 2, len - 1] {
                let e = to_bytes_within(&d, format, &WriteOptions::default(), limit).unwrap_err();
                assert_eq!(e, TooLarge { limit });
                assert!(e.to_string().contains(&limit.to_string()));
            }
        }
    }

    #[test]
    fn each_group_is_counted_at_its_encoded_size() {
        use encode::{Group, Value, encoded_len, to_ascii, to_binary, BINARY_SENTINEL};
        let ascii = |g: &Group| to_ascii(std::slice::from_ref(g)).len();
        let binary = |g: &Group| to_binary(std::slice::from_ref(g)).len() - BINARY_SENTINEL.len();
        for d in std::iter::once(drawing()).chain(corpus()) {
            let (groups, _) = groups(&d);
            assert!(!groups.is_empty());
            for g in &groups {
                let text_with_controls = matches!(&g.value, Value::Str(s) if s.chars().any(|c| c.is_control() || c == '\0'));
                if text_with_controls {
                    assert!(ascii(g) <= encoded_len(g, DxfFormat::Ascii), "{g:?}");
                    assert!(binary(g) <= encoded_len(g, DxfFormat::Binary), "{g:?}");
                } else {
                    assert_eq!(ascii(g), encoded_len(g, DxfFormat::Ascii), "{g:?}");
                    assert_eq!(binary(g), encoded_len(g, DxfFormat::Binary), "{g:?}");
                }
            }
        }
        // Values at their longest, and values under codes of another type.
        for g in [
            Group { code: 1071, value: Value::Int(i64::MIN) },
            Group { code: 160, value: Value::Int(i64::MIN) },
            Group { code: 70, value: Value::Int(-7) },
            Group { code: 290, value: Value::Bool(true) },
            Group { code: 10, value: Value::Real(-1.234_567_890_123_456_7e-300) },
            Group { code: 10, value: Value::Int(5) },
            Group { code: 5, value: Value::Handle(u64::MAX) },
            Group { code: 5, value: Value::Int(255) },
            Group { code: 1, value: Value::Real(f64::MIN) },
            Group { code: 1, value: Value::Int(42) },
            Group { code: 310, value: Value::Bytes(vec![0xAB; 300]) },
            Group { code: 10000, value: Value::Str("x".into()) },
        ] {
            assert_eq!(ascii(&g), encoded_len(&g, DxfFormat::Ascii), "{g:?}");
            assert_eq!(binary(&g), encoded_len(&g, DxfFormat::Binary), "{g:?}");
        }
        let g = Group { code: 1, value: Value::Str("line\r\nbreak\u{85}\0".into()) };
        assert!(ascii(&g) <= encoded_len(&g, DxfFormat::Ascii));
        assert!(binary(&g) <= encoded_len(&g, DxfFormat::Binary));
    }
}

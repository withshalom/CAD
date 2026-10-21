//! Header variables (`cad_dwg::hvars::Hv`) read from drawings made from
//! BlackCAD's templates: the typed values a template sets win over the
//! stored text, and stored text fills the rest.

use cad_core::dwg::hvars::Hv;
use cad_core::dwg::DwgVersion;
use cad_core::model::{Color, Handle};
use cad_core::{new_drawing, Template};

#[test]
fn stored_text_reads_back() {
    let mut d = new_drawing(Template::Imperial);
    d.version = DwgVersion::R2018;
    d.header.raw = vec![
        ("CECOLOR".into(), "0".into()),
        ("DIMCLRD".into(), "BYBLOCK".into()),
        ("INTERFERECOLOR".into(), "RGB(255,0,0)".into()),
        ("PUCSNAME".into(), "#3D0B5".into()),
        ("MENUNAME".into(), "\"acad\"".into()),
        ("DIMPOST".into(), "\".\"".into()),
        ("EXTMIN".into(), "(1, 2, 3)".into()),
    ];
    let hv = Hv::new(&d);
    assert_eq!(hv.color("CECOLOR", Color::ByLayer), Color::ByLayer);
    assert_eq!(hv.color("DIMCLRD", Color::Index(7)), Color::ByBlock);
    assert_eq!(hv.color("INTERFERECOLOR", Color::ByLayer), Color::Rgb(255, 0, 0));
    assert_eq!(hv.handle("PUCSNAME"), Handle(0x3D0B5));
    assert_eq!(hv.text("DIMPOST", ""), ".");
    assert_eq!(hv.int("MISSING", 42), 42);
}

/// A header saved by a reader that skipped MENUNAME reads back with
/// every string where it belongs.
#[test]
fn strings_read_one_place_early_are_put_back() {
    let mut d = new_drawing(Template::Imperial);
    d.version = DwgVersion::R2018;
    let q = |s: &str| format!("{s:?}");
    d.header.raw = vec![
        ("DIMPOST".into(), q(".")),
        ("DIMAPOST".into(), q("")),
        ("DIMALTMZS".into(), q("")),
        ("DIMMZS".into(), q("")),
        ("HYPERLINKBASE".into(), q("")),
        ("STYLESHEET".into(), q("")),
        ("FINGERPRINTGUID".into(), q("")),
        ("VERSIONGUID".into(), q("{FP}")),
        ("PROJECTNAME".into(), q("{VG}")),
    ];
    let hv = Hv::new(&d);
    assert_eq!(hv.text("MENUNAME", "?"), ".");
    assert_eq!(hv.text("DIMPOST", "?"), "");
    assert_eq!(hv.text("FINGERPRINTGUID", "?"), "{FP}");
    assert_eq!(hv.text("VERSIONGUID", "?"), "{VG}");
    assert_eq!(hv.text("PROJECTNAME", "?"), "");
}

//! The drawing database as DXF groups: HEADER, CLASSES, TABLES, BLOCKS,
//! ENTITIES and OBJECTS, laid out as AutoCAD writes an AC1032 DXF
//! (record order, subclass markers, which optional groups appear),
//! compared group by group with the DXF AutoCAD 2027 saved of the MI-201
//! corpus drawing.
//!
//! Group meanings are from the Autodesk DXF reference. Objects the model
//! keeps only as DWG records are written when `dwg::nongraphic` decodes
//! them (dictionaries, extension records, layouts, scales, multiline
//! styles, plot-style placeholders, dictionary variables); the others
//! (visual styles, materials, table and leader styles, dimension
//! associations, section and detail view styles) are left out, with every
//! reference to them, and AutoCAD supplies its defaults when it opens the
//! file. The writer reports what it left out.

use super::encode::{Group, Value};
use crate::dwg::hvars::Hv;
use crate::dwg::nongraphic::{self, DimVars, NonGraphic, Vport, XrValue};
use crate::geom::{Vec2, Vec3};
use crate::model::*;
use std::collections::{BTreeMap, BTreeSet};

/// Where a header variable's value comes from.
#[derive(Clone, Copy)]
pub(super) enum Src {
    /// The DWG variable of that name.
    Raw(&'static str),
    /// The name of the table record the DWG variable points at.
    Name(&'static str),
    /// A CmColor variable, with the value to use when it is unknown.
    Color(&'static str, Color),
    /// A date.
    Time(&'static str),
    /// A bit field of the DWG FLAGS variable: 0 CELWEIGHT, 1 ENDCAPS,
    /// 2 JOINSTYLE, 3 LWDISPLAY, 4 XEDIT, 5 EXTNAMES, 6 PSTYLEMODE,
    /// 7 OLESTARTUP.
    Flags(u8),
    /// An object handle, written only when that object is.
    Object(&'static str),
    Maint,
    Codepage,
    LastSavedBy,
    Handseed,
    MlineStyle,
}

/// The header variables of an AC1032 DXF in AutoCAD's order: name, first
/// group code, number of coordinates (1 for a single value), source, and
/// the value used when the drawing has none (AutoCAD's own defaults, as an
/// imperial drawing carries them).
#[rustfmt::skip]
pub(super) static HEADER: &[(&str, u16, u8, Src, &str)] = &[
    ("$ACADMAINTVER", 90, 1, Src::Maint, ""),
    ("$DWGCODEPAGE", 3, 1, Src::Codepage, "ANSI_1252"),
    ("$LASTSAVEDBY", 1, 1, Src::LastSavedBy, ""),
    ("$REQUIREDVERSIONS", 160, 1, Src::Raw("REQUIREDVERSIONS"), "0"),
    ("$INSBASE", 10, 3, Src::Raw("INSBASE"), "0.0,0.0,0.0"),
    ("$EXTMIN", 10, 3, Src::Raw("EXTMIN"), "1.0E+20,1.0E+20,1.0E+20"),
    ("$EXTMAX", 10, 3, Src::Raw("EXTMAX"), "-1.0E+20,-1.0E+20,-1.0E+20"),
    ("$LIMMIN", 10, 2, Src::Raw("LIMMIN"), "0.0,0.0"),
    ("$LIMMAX", 10, 2, Src::Raw("LIMMAX"), "12.0,9.0"),
    ("$ORTHOMODE", 70, 1, Src::Raw("ORTHOMODE"), "0"),
    ("$REGENMODE", 70, 1, Src::Raw("REGENMODE"), "1"),
    ("$FILLMODE", 70, 1, Src::Raw("FILLMODE"), "1"),
    ("$QTEXTMODE", 70, 1, Src::Raw("QTEXTMODE"), "0"),
    ("$MIRRTEXT", 70, 1, Src::Raw("MIRRTEXT"), "0"),
    ("$LTSCALE", 40, 1, Src::Raw("LTSCALE"), "1.0"),
    ("$ATTMODE", 70, 1, Src::Raw("ATTMODE"), "1"),
    ("$TEXTSIZE", 40, 1, Src::Raw("TEXTSIZE"), "0.2"),
    ("$TRACEWID", 40, 1, Src::Raw("TRACEWID"), "0.05"),
    ("$TEXTSTYLE", 7, 1, Src::Name("TEXTSTYLE"), "Standard"),
    ("$CLAYER", 8, 1, Src::Name("CLAYER"), "0"),
    ("$CELTYPE", 6, 1, Src::Name("CELTYPE"), "ByLayer"),
    ("$CECOLOR", 62, 1, Src::Color("CECOLOR", Color::ByLayer), "256"),
    ("$CELTSCALE", 40, 1, Src::Raw("CELTSCALE"), "1.0"),
    ("$DISPSILH", 70, 1, Src::Raw("DISPSILH"), "0"),
    ("$DIMSCALE", 40, 1, Src::Raw("DIMSCALE"), "1.0"),
    ("$DIMASZ", 40, 1, Src::Raw("DIMASZ"), "0.18"),
    ("$DIMEXO", 40, 1, Src::Raw("DIMEXO"), "0.0625"),
    ("$DIMDLI", 40, 1, Src::Raw("DIMDLI"), "0.38"),
    ("$DIMRND", 40, 1, Src::Raw("DIMRND"), "0.0"),
    ("$DIMDLE", 40, 1, Src::Raw("DIMDLE"), "0.0"),
    ("$DIMEXE", 40, 1, Src::Raw("DIMEXE"), "0.18"),
    ("$DIMTP", 40, 1, Src::Raw("DIMTP"), "0.0"),
    ("$DIMTM", 40, 1, Src::Raw("DIMTM"), "0.0"),
    ("$DIMTXT", 40, 1, Src::Raw("DIMTXT"), "0.18"),
    ("$DIMCEN", 40, 1, Src::Raw("DIMCEN"), "0.09"),
    ("$DIMTSZ", 40, 1, Src::Raw("DIMTSZ"), "0.0"),
    ("$DIMTOL", 70, 1, Src::Raw("DIMTOL"), "0"),
    ("$DIMLIM", 70, 1, Src::Raw("DIMLIM"), "0"),
    ("$DIMTIH", 70, 1, Src::Raw("DIMTIH"), "1"),
    ("$DIMTOH", 70, 1, Src::Raw("DIMTOH"), "1"),
    ("$DIMSE1", 70, 1, Src::Raw("DIMSE1"), "0"),
    ("$DIMSE2", 70, 1, Src::Raw("DIMSE2"), "0"),
    ("$DIMTAD", 70, 1, Src::Raw("DIMTAD"), "0"),
    ("$DIMZIN", 70, 1, Src::Raw("DIMZIN"), "0"),
    ("$DIMBLK", 1, 1, Src::Name("DIMBLK"), ""),
    ("$DIMASO", 70, 1, Src::Raw("DIMASO"), "1"),
    ("$DIMSHO", 70, 1, Src::Raw("DIMSHO"), "1"),
    ("$DIMPOST", 1, 1, Src::Raw("DIMPOST"), ""),
    ("$DIMAPOST", 1, 1, Src::Raw("DIMAPOST"), ""),
    ("$DIMALT", 70, 1, Src::Raw("DIMALT"), "0"),
    ("$DIMALTD", 70, 1, Src::Raw("DIMALTD"), "2"),
    ("$DIMALTF", 40, 1, Src::Raw("DIMALTF"), "25.4"),
    ("$DIMLFAC", 40, 1, Src::Raw("DIMLFAC"), "1.0"),
    ("$DIMTOFL", 70, 1, Src::Raw("DIMTOFL"), "0"),
    ("$DIMTVP", 40, 1, Src::Raw("DIMTVP"), "0.0"),
    ("$DIMTIX", 70, 1, Src::Raw("DIMTIX"), "0"),
    ("$DIMSOXD", 70, 1, Src::Raw("DIMSOXD"), "0"),
    ("$DIMSAH", 70, 1, Src::Raw("DIMSAH"), "0"),
    ("$DIMBLK1", 1, 1, Src::Name("DIMBLK1"), ""),
    ("$DIMBLK2", 1, 1, Src::Name("DIMBLK2"), ""),
    ("$DIMSTYLE", 2, 1, Src::Name("DIMSTYLE"), "Standard"),
    ("$DIMCLRD", 70, 1, Src::Color("DIMCLRD", Color::ByBlock), "0"),
    ("$DIMCLRE", 70, 1, Src::Color("DIMCLRE", Color::ByBlock), "0"),
    ("$DIMCLRT", 70, 1, Src::Color("DIMCLRT", Color::ByBlock), "0"),
    ("$DIMTFAC", 40, 1, Src::Raw("DIMTFAC"), "1.0"),
    ("$DIMGAP", 40, 1, Src::Raw("DIMGAP"), "0.09"),
    ("$DIMJUST", 70, 1, Src::Raw("DIMJUST"), "0"),
    ("$DIMSD1", 70, 1, Src::Raw("DIMSD1"), "0"),
    ("$DIMSD2", 70, 1, Src::Raw("DIMSD2"), "0"),
    ("$DIMTOLJ", 70, 1, Src::Raw("DIMTOLJ"), "1"),
    ("$DIMTZIN", 70, 1, Src::Raw("DIMTZIN"), "0"),
    ("$DIMALTZ", 70, 1, Src::Raw("DIMALTZ"), "0"),
    ("$DIMALTTZ", 70, 1, Src::Raw("DIMALTTZ"), "0"),
    ("$DIMUPT", 70, 1, Src::Raw("DIMUPT"), "0"),
    ("$DIMDEC", 70, 1, Src::Raw("DIMDEC"), "4"),
    ("$DIMTDEC", 70, 1, Src::Raw("DIMTDEC"), "4"),
    ("$DIMALTU", 70, 1, Src::Raw("DIMALTU"), "2"),
    ("$DIMALTTD", 70, 1, Src::Raw("DIMALTTD"), "2"),
    ("$DIMTXSTY", 7, 1, Src::Name("DIMTXSTY"), "Standard"),
    ("$DIMAUNIT", 70, 1, Src::Raw("DIMAUNIT"), "0"),
    ("$DIMADEC", 70, 1, Src::Raw("DIMADEC"), "0"),
    ("$DIMALTRND", 40, 1, Src::Raw("DIMALTRND"), "0.0"),
    ("$DIMAZIN", 70, 1, Src::Raw("DIMAZIN"), "0"),
    ("$DIMDSEP", 70, 1, Src::Raw("DIMDSEP"), "46"),
    ("$DIMATFIT", 70, 1, Src::Raw("DIMATFIT"), "3"),
    ("$DIMFRAC", 70, 1, Src::Raw("DIMFRAC"), "0"),
    ("$DIMLDRBLK", 1, 1, Src::Name("DIMLDRBLK"), ""),
    ("$DIMLUNIT", 70, 1, Src::Raw("DIMLUNIT"), "2"),
    ("$DIMLWD", 70, 1, Src::Raw("DIMLWD"), "-2"),
    ("$DIMLWE", 70, 1, Src::Raw("DIMLWE"), "-2"),
    ("$DIMTMOVE", 70, 1, Src::Raw("DIMTMOVE"), "0"),
    ("$DIMFXL", 40, 1, Src::Raw("DIMFXL"), "1.0"),
    ("$DIMFXLON", 70, 1, Src::Raw("DIMFXLON"), "0"),
    ("$DIMJOGANG", 40, 1, Src::Raw("DIMJOGANG"), "0.7853981633974483"),
    ("$DIMTFILL", 70, 1, Src::Raw("DIMTFILL"), "0"),
    ("$DIMTFILLCLR", 70, 1, Src::Color("DIMTFILLCLR", Color::ByBlock), "0"),
    ("$DIMARCSYM", 70, 1, Src::Raw("DIMARCSYM"), "0"),
    ("$DIMLTYPE", 6, 1, Src::Name("DIMLTYPE"), ""),
    ("$DIMLTEX1", 6, 1, Src::Name("DIMLTEX1"), ""),
    ("$DIMLTEX2", 6, 1, Src::Name("DIMLTEX2"), ""),
    ("$DIMTXTDIRECTION", 70, 1, Src::Raw("DIMTXTDIRECTION"), "0"),
    ("$LUNITS", 70, 1, Src::Raw("LUNITS"), "2"),
    ("$LUPREC", 70, 1, Src::Raw("LUPREC"), "4"),
    ("$SKETCHINC", 40, 1, Src::Raw("SKETCHINC"), "0.1"),
    ("$FILLETRAD", 40, 1, Src::Raw("FILLETRAD"), "0.0"),
    ("$AUNITS", 70, 1, Src::Raw("AUNITS"), "0"),
    ("$AUPREC", 70, 1, Src::Raw("AUPREC"), "0"),
    ("$MENU", 1, 1, Src::Raw("MENUNAME"), "."),
    ("$ELEVATION", 40, 1, Src::Raw("ELEVATION"), "0.0"),
    ("$PELEVATION", 40, 1, Src::Raw("PELEVATION"), "0.0"),
    ("$THICKNESS", 40, 1, Src::Raw("THICKNESS"), "0.0"),
    ("$LIMCHECK", 70, 1, Src::Raw("LIMCHECK"), "0"),
    ("$CHAMFERA", 40, 1, Src::Raw("CHAMFERA"), "0.0"),
    ("$CHAMFERB", 40, 1, Src::Raw("CHAMFERB"), "0.0"),
    ("$CHAMFERC", 40, 1, Src::Raw("CHAMFERC"), "0.0"),
    ("$CHAMFERD", 40, 1, Src::Raw("CHAMFERD"), "0.0"),
    ("$SKPOLY", 70, 1, Src::Raw("SKPOLY"), "0"),
    ("$TDCREATE", 40, 1, Src::Time("TDCREATE"), "0.0"),
    ("$TDUCREATE", 40, 1, Src::Time("TDCREATE"), "0.0"),
    ("$TDUPDATE", 40, 1, Src::Time("TDUPDATE"), "0.0"),
    ("$TDUUPDATE", 40, 1, Src::Time("TDUPDATE"), "0.0"),
    ("$TDINDWG", 40, 1, Src::Time("TDINDWG"), "0.0"),
    ("$TDUSRTIMER", 40, 1, Src::Time("TDUSRTIMER"), "0.0"),
    ("$USRTIMER", 70, 1, Src::Raw("USRTIMER"), "1"),
    ("$ANGBASE", 50, 1, Src::Raw("ANGBASE"), "0.0"),
    ("$ANGDIR", 70, 1, Src::Raw("ANGDIR"), "0"),
    ("$PDMODE", 70, 1, Src::Raw("PDMODE"), "0"),
    ("$PDSIZE", 40, 1, Src::Raw("PDSIZE"), "0.0"),
    ("$PLINEWID", 40, 1, Src::Raw("PLINEWID"), "0.0"),
    ("$SPLFRAME", 70, 1, Src::Raw("SPLFRAME"), "0"),
    ("$SPLINETYPE", 70, 1, Src::Raw("SPLINETYPE"), "6"),
    ("$SPLINESEGS", 70, 1, Src::Raw("SPLINESEGS"), "8"),
    ("$HANDSEED", 5, 1, Src::Handseed, ""),
    ("$SURFTAB1", 70, 1, Src::Raw("SURFTAB1"), "6"),
    ("$SURFTAB2", 70, 1, Src::Raw("SURFTAB2"), "6"),
    ("$SURFTYPE", 70, 1, Src::Raw("SURFTYPE"), "6"),
    ("$SURFU", 70, 1, Src::Raw("SURFU"), "6"),
    ("$SURFV", 70, 1, Src::Raw("SURFV"), "6"),
    ("$UCSBASE", 2, 1, Src::Name("UCSBASE"), ""),
    ("$UCSNAME", 2, 1, Src::Name("UCSNAME"), ""),
    ("$UCSORG", 10, 3, Src::Raw("UCSORG"), "0.0,0.0,0.0"),
    ("$UCSXDIR", 10, 3, Src::Raw("UCSXDIR"), "1.0,0.0,0.0"),
    ("$UCSYDIR", 10, 3, Src::Raw("UCSYDIR"), "0.0,1.0,0.0"),
    ("$UCSORTHOREF", 2, 1, Src::Name("UCSORTHOREF"), ""),
    ("$UCSORTHOVIEW", 70, 1, Src::Raw("UCSORTHOVIEW"), "0"),
    ("$UCSORGTOP", 10, 3, Src::Raw("UCSORGTOP"), "0.0,0.0,0.0"),
    ("$UCSORGBOTTOM", 10, 3, Src::Raw("UCSORGBOTTOM"), "0.0,0.0,0.0"),
    ("$UCSORGLEFT", 10, 3, Src::Raw("UCSORGLEFT"), "0.0,0.0,0.0"),
    ("$UCSORGRIGHT", 10, 3, Src::Raw("UCSORGRIGHT"), "0.0,0.0,0.0"),
    ("$UCSORGFRONT", 10, 3, Src::Raw("UCSORGFRONT"), "0.0,0.0,0.0"),
    ("$UCSORGBACK", 10, 3, Src::Raw("UCSORGBACK"), "0.0,0.0,0.0"),
    ("$PUCSBASE", 2, 1, Src::Name("PUCSBASE"), ""),
    ("$PUCSNAME", 2, 1, Src::Name("PUCSNAME"), ""),
    ("$PUCSORG", 10, 3, Src::Raw("PUCSORG"), "0.0,0.0,0.0"),
    ("$PUCSXDIR", 10, 3, Src::Raw("PUCSXDIR"), "1.0,0.0,0.0"),
    ("$PUCSYDIR", 10, 3, Src::Raw("PUCSYDIR"), "0.0,1.0,0.0"),
    ("$PUCSORTHOREF", 2, 1, Src::Name("PUCSORTHOREF"), ""),
    ("$PUCSORTHOVIEW", 70, 1, Src::Raw("PUCSORTHOVIEW"), "0"),
    ("$PUCSORGTOP", 10, 3, Src::Raw("PUCSORGTOP"), "0.0,0.0,0.0"),
    ("$PUCSORGBOTTOM", 10, 3, Src::Raw("PUCSORGBOTTOM"), "0.0,0.0,0.0"),
    ("$PUCSORGLEFT", 10, 3, Src::Raw("PUCSORGLEFT"), "0.0,0.0,0.0"),
    ("$PUCSORGRIGHT", 10, 3, Src::Raw("PUCSORGRIGHT"), "0.0,0.0,0.0"),
    ("$PUCSORGFRONT", 10, 3, Src::Raw("PUCSORGFRONT"), "0.0,0.0,0.0"),
    ("$PUCSORGBACK", 10, 3, Src::Raw("PUCSORGBACK"), "0.0,0.0,0.0"),
    ("$USERI1", 70, 1, Src::Raw("USERI1"), "0"),
    ("$USERI2", 70, 1, Src::Raw("USERI2"), "0"),
    ("$USERI3", 70, 1, Src::Raw("USERI3"), "0"),
    ("$USERI4", 70, 1, Src::Raw("USERI4"), "0"),
    ("$USERI5", 70, 1, Src::Raw("USERI5"), "0"),
    ("$USERR1", 40, 1, Src::Raw("USERR1"), "0.0"),
    ("$USERR2", 40, 1, Src::Raw("USERR2"), "0.0"),
    ("$USERR3", 40, 1, Src::Raw("USERR3"), "0.0"),
    ("$USERR4", 40, 1, Src::Raw("USERR4"), "0.0"),
    ("$USERR5", 40, 1, Src::Raw("USERR5"), "0.0"),
    ("$WORLDVIEW", 70, 1, Src::Raw("WORLDVIEW"), "1"),
    ("$SHADEDGE", 70, 1, Src::Raw("SHADEDGE"), "3"),
    ("$SHADEDIF", 70, 1, Src::Raw("SHADEDIF"), "70"),
    ("$TILEMODE", 70, 1, Src::Raw("TILEMODE"), "1"),
    ("$MAXACTVP", 70, 1, Src::Raw("MAXACTVP"), "64"),
    ("$PINSBASE", 10, 3, Src::Raw("PINSBASE"), "0.0,0.0,0.0"),
    ("$PLIMCHECK", 70, 1, Src::Raw("PLIMCHECK"), "0"),
    ("$PEXTMIN", 10, 3, Src::Raw("PEXTMIN"), "1.0E+20,1.0E+20,1.0E+20"),
    ("$PEXTMAX", 10, 3, Src::Raw("PEXTMAX"), "-1.0E+20,-1.0E+20,-1.0E+20"),
    ("$PLIMMIN", 10, 2, Src::Raw("PLIMMIN"), "0.0,0.0"),
    ("$PLIMMAX", 10, 2, Src::Raw("PLIMMAX"), "12.0,9.0"),
    ("$UNITMODE", 70, 1, Src::Raw("UNITMODE"), "0"),
    ("$VISRETAIN", 70, 1, Src::Raw("VISRETAIN"), "1"),
    ("$PLINEGEN", 70, 1, Src::Raw("PLINEGEN"), "0"),
    ("$PSLTSCALE", 70, 1, Src::Raw("PSLTSCALE"), "1"),
    ("$TREEDEPTH", 70, 1, Src::Raw("TREEDEPTH"), "3020"),
    ("$CMLSTYLE", 2, 1, Src::MlineStyle, "Standard"),
    ("$CMLJUST", 70, 1, Src::Raw("CMLJUST"), "0"),
    ("$CMLSCALE", 40, 1, Src::Raw("CMLSCALE"), "1.0"),
    ("$PROXYGRAPHICS", 70, 1, Src::Raw("PROXYGRAPHICS"), "1"),
    ("$MEASUREMENT", 70, 1, Src::Raw("MEASUREMENT"), "0"),
    ("$CELWEIGHT", 370, 1, Src::Flags(0), "-1"),
    ("$ENDCAPS", 280, 1, Src::Flags(1), "0"),
    ("$JOINSTYLE", 280, 1, Src::Flags(2), "0"),
    ("$LWDISPLAY", 290, 1, Src::Flags(3), "0"),
    ("$INSUNITS", 70, 1, Src::Raw("INSUNITS"), "0"),
    ("$HYPERLINKBASE", 1, 1, Src::Raw("HYPERLINKBASE"), ""),
    ("$STYLESHEET", 1, 1, Src::Raw("STYLESHEET"), ""),
    ("$XEDIT", 290, 1, Src::Flags(4), "1"),
    ("$CEPSNTYPE", 380, 1, Src::Raw("CEPSNTYPE"), "0"),
    ("$PSTYLEMODE", 290, 1, Src::Flags(6), "1"),
    ("$FINGERPRINTGUID", 2, 1, Src::Raw("FINGERPRINTGUID"), ""),
    ("$VERSIONGUID", 2, 1, Src::Raw("VERSIONGUID"), ""),
    ("$EXTNAMES", 290, 1, Src::Flags(5), "1"),
    ("$PSVPSCALE", 40, 1, Src::Raw("PSVPSCALE"), "0.0"),
    ("$OLESTARTUP", 290, 1, Src::Flags(7), "0"),
    ("$SORTENTS", 280, 1, Src::Raw("SORTENTS"), "127"),
    ("$INDEXCTL", 280, 1, Src::Raw("INDEXCTL"), "0"),
    ("$HIDETEXT", 280, 1, Src::Raw("HIDETEXT"), "1"),
    ("$XCLIPFRAME", 280, 1, Src::Raw("XCLIPFRAME"), "2"),
    ("$HALOGAP", 280, 1, Src::Raw("HALOGAP"), "0"),
    ("$OBSCOLOR", 70, 1, Src::Raw("OBSCUREDCOLOR"), "257"),
    ("$OBSLTYPE", 280, 1, Src::Raw("OBSCUREDLTYPE"), "0"),
    ("$INTERSECTIONDISPLAY", 280, 1, Src::Raw("INTERSECTIONDISPLAY"), "0"),
    ("$INTERSECTIONCOLOR", 70, 1, Src::Raw("INTERSECTIONCOLOR"), "257"),
    ("$DIMASSOC", 280, 1, Src::Raw("DIMASSOC"), "2"),
    ("$PROJECTNAME", 1, 1, Src::Raw("PROJECTNAME"), ""),
    ("$CAMERADISPLAY", 290, 1, Src::Raw("CAMERADISPLAY"), "0"),
    ("$LENSLENGTH", 40, 1, Src::Raw("LENSLENGTH"), "50.0"),
    ("$CAMERAHEIGHT", 40, 1, Src::Raw("CAMERAHEIGHT"), "0.0"),
    ("$STEPSPERSEC", 40, 1, Src::Raw("STEPSPERSEC"), "2.0"),
    ("$STEPSIZE", 40, 1, Src::Raw("STEPSIZE"), "6.0"),
    ("$3DDWFPREC", 40, 1, Src::Raw("3DDWFPREC"), "2.0"),
    ("$PSOLWIDTH", 40, 1, Src::Raw("PSOLWIDTH"), "0.25"),
    ("$PSOLHEIGHT", 40, 1, Src::Raw("PSOLHEIGHT"), "4.0"),
    ("$LOFTANG1", 40, 1, Src::Raw("LOFTANG1"), "1.570796326794896"),
    ("$LOFTANG2", 40, 1, Src::Raw("LOFTANG2"), "1.570796326794896"),
    ("$LOFTMAG1", 40, 1, Src::Raw("LOFTMAG1"), "0.0"),
    ("$LOFTMAG2", 40, 1, Src::Raw("LOFTMAG2"), "0.0"),
    ("$LOFTPARAM", 70, 1, Src::Raw("LOFTPARAM"), "7"),
    ("$LOFTNORMALS", 280, 1, Src::Raw("LOFTNORMALS"), "1"),
    ("$LATITUDE", 40, 1, Src::Raw("LATITUDE"), "37.795"),
    ("$LONGITUDE", 40, 1, Src::Raw("LONGITUDE"), "-122.394"),
    ("$NORTHDIRECTION", 40, 1, Src::Raw("NORTHDIRECTION"), "0.0"),
    ("$TIMEZONE", 70, 1, Src::Raw("TIMEZONE"), "-8000"),
    ("$LIGHTGLYPHDISPLAY", 280, 1, Src::Raw("LIGHTGLYPHDISPLAY"), "1"),
    ("$TILEMODELIGHTSYNCH", 280, 1, Src::Raw("TILEMODELIGHTSYNCH"), "1"),
    ("$CMATERIAL", 347, 1, Src::Object("CMATERIAL"), ""),
    ("$SOLIDHIST", 280, 1, Src::Raw("SOLIDHIST"), "0"),
    ("$SHOWHIST", 280, 1, Src::Raw("SHOWHIST"), "1"),
    ("$DWFFRAME", 280, 1, Src::Raw("DWFFRAME"), "2"),
    ("$DGNFRAME", 280, 1, Src::Raw("DGNFRAME"), "0"),
    ("$REALWORLDSCALE", 290, 1, Src::Raw("UNKNOWN_B_R2007"), "1"),
    ("$INTERFERECOLOR", 62, 1, Src::Color("INTERFERECOLOR", Color::Index(1)), "1"),
    ("$INTERFEREOBJVS", 345, 1, Src::Object("INTERFEREOBJVS"), ""),
    ("$INTERFEREVPVS", 346, 1, Src::Object("INTERFEREVPVS"), ""),
    ("$CSHADOW", 280, 1, Src::Raw("CSHADOW"), "0"),
    ("$SHADOWPLANELOCATION", 40, 1, Src::Raw("UNKNOWN_BD_R2007_2"), "0.0"),
];

/// The DXF type name of an entity, or `None` for one this writer cannot
/// express.
fn entity_type(o: &Object) -> Option<&'static str> {
    Some(match &o.kind {
        ObjectKind::Line(_) => "LINE",
        ObjectKind::Circle(_) => "CIRCLE",
        ObjectKind::Arc(_) => "ARC",
        ObjectKind::Point(_) => "POINT",
        ObjectKind::Ellipse(_) => "ELLIPSE",
        ObjectKind::LwPolyline(_) => "LWPOLYLINE",
        ObjectKind::Polyline(_) => "POLYLINE",
        ObjectKind::Vertex(_) => "VERTEX",
        ObjectKind::SeqEnd => "SEQEND",
        ObjectKind::Text(_) => "TEXT",
        ObjectKind::Attribute(a) => {
            if a.is_definition {
                "ATTDEF"
            } else {
                "ATTRIB"
            }
        }
        ObjectKind::MText(_) => "MTEXT",
        ObjectKind::Insert(_) => "INSERT",
        ObjectKind::Solid(s) => {
            if s.is_trace {
                "TRACE"
            } else {
                "SOLID"
            }
        }
        ObjectKind::Face3d(_) => "3DFACE",
        ObjectKind::Spline(_) => "SPLINE",
        ObjectKind::Dimension(_) => "DIMENSION",
        ObjectKind::Hatch(_) => "HATCH",
        ObjectKind::Xline(_) => "XLINE",
        ObjectKind::Ray(_) => "RAY",
        ObjectKind::Leader(_) => "LEADER",
        ObjectKind::Tolerance(_) => "TOLERANCE",
        ObjectKind::MLine(_) => "MLINE",
        ObjectKind::Image(_) => "IMAGE",
        ObjectKind::Wipeout(_) => "WIPEOUT",
        ObjectKind::PaperViewport(_) => "VIEWPORT",
        ObjectKind::BlockBegin { .. } => "BLOCK",
        ObjectKind::BlockEnd => "ENDBLK",
        _ => return None,
    })
}

pub(super) fn deg(r: f64) -> f64 {
    let d = r.to_degrees();
    // Keep exact values exact (90.0 rather than 89.99999999999999).
    let rounded = (d * 1e9).round() / 1e9;
    if (d - rounded).abs() < 1e-12 {
        rounded
    } else {
        d
    }
}

/// The DXF code page name of a DWGCODEPAGE value.
pub(super) fn codepage_name(cp: u16) -> &'static str {
    match cp {
        30 => "ANSI_1252",
        _ => "ANSI_1252",
    }
}

pub(crate) struct Writer<'a> {
    d: &'a Drawing,
    hv: Hv<'a>,
    out: Vec<Group>,
    /// At most how many bytes the file may take, and in which encoding (`within`); no limit
    /// when `None`.
    budget: Option<(usize, crate::DxfFormat)>,
    /// The bytes the groups in `out` take in that encoding (`encode::encoded_len`), with the
    /// binary sentinel.
    used: usize,
    /// The budget was passed: no more groups are kept.
    over: bool,
    /// Objects kept as DWG records that decode into DXF records.
    ng: BTreeMap<Handle, NonGraphic>,
    /// Every handle that gets a record of its own.
    written: BTreeSet<Handle>,
    /// Objects left out, by type.
    skipped: BTreeMap<String, usize>,
    /// Dictionary keys by handle.
    keys: BTreeMap<Handle, String>,
    /// Names given to anonymous blocks: DWG stores them unnumbered
    /// ("*D"), DXF needs each one unique ("*D0", "*D1").
    block_names: BTreeMap<Handle, String>,
    /// Line families for a pattern hatch without them
    /// (`WriteOptions::hatch_lines`).
    hatch_lines: Option<&'a HatchLibrary<'a>>,
    pub notes: Vec<String>,
}

impl<'a> Writer<'a> {
    pub fn new(d: &'a Drawing, hatch_lines: Option<&'a HatchLibrary<'a>>) -> Self {
        let mut w = Writer {
            d,
            hatch_lines,
            hv: Hv::new(d),
            out: Vec::new(),
            budget: None,
            used: 0,
            over: false,
            ng: BTreeMap::new(),
            written: BTreeSet::new(),
            skipped: BTreeMap::new(),
            keys: BTreeMap::new(),
            block_names: BTreeMap::new(),
            notes: Vec::new(),
        };
        w.plan();
        w
    }

    /// Stop keeping groups once the file, in `format`, would take more than `max_bytes`: the
    /// writer still walks the drawing, but what it keeps stays within the limit, and
    /// [`Writer::write`] says the file was not written.
    pub fn within(mut self, max_bytes: usize, format: crate::DxfFormat) -> Self {
        self.budget = Some((max_bytes, format));
        self.used = match format {
            crate::DxfFormat::Ascii => 0,
            crate::DxfFormat::Binary => crate::encode::BINARY_SENTINEL.len(),
        };
        self
    }

    /// Keep a group, unless the file would pass the budget.
    fn push(&mut self, g: Group) {
        if self.over {
            return;
        }
        if let Some((max, format)) = self.budget {
            self.used = self.used.saturating_add(crate::encode::encoded_len(&g, format));
            if self.used > max {
                self.over = true;
                self.out = Vec::new();
                return;
            }
        }
        self.out.push(g);
    }

    /// Decide what gets written: tables, records, block contents and the
    /// objects that can be expressed.
    fn plan(&mut self) {
        let d = self.d;
        for o in d.objects.values() {
            let keep = match &o.kind {
                ObjectKind::Control(_)
                | ObjectKind::Layer(_)
                | ObjectKind::Linetype(_)
                | ObjectKind::TextStyle(_)
                | ObjectKind::DimStyle(_)
                | ObjectKind::BlockHeader(_)
                | ObjectKind::NamedRecord(_)
                | ObjectKind::Viewport(_)
                | ObjectKind::DimStyleRecord(_) => true,
                _ if o.is_entity() => entity_type(o).is_some(),
                _ => match nongraphic::decode(o, &d.classes, d.codepage) {
                    Some(Ok(n)) => {
                        self.ng.insert(o.handle, n);
                        true
                    }
                    Some(Err(e)) => {
                        self.notes.push(format!("{} {}: {e}", o.type_name, o.handle));
                        false
                    }
                    None => false,
                },
            };
            if keep {
                self.written.insert(o.handle);
            } else {
                *self.skipped.entry(o.type_name.clone()).or_default() += 1;
            }
        }
        // A dictionary whose every entry was left out goes too, so AutoCAD
        // finds the collection missing and makes its own (an empty
        // ACAD_VISUALSTYLE would leave the drawing without visual styles).
        loop {
            let empty: Vec<Handle> = self
                .ng
                .iter()
                .filter_map(|(h, n)| match n {
                    NonGraphic::Dictionary(dict)
                        if !dict.entries.is_empty()
                            && dict.entries.iter().all(|(_, e)| !self.written.contains(e)) =>
                    {
                        Some(*h)
                    }
                    _ => None,
                })
                .collect();
            if empty.is_empty() {
                break;
            }
            for h in empty {
                self.ng.remove(&h);
                self.written.remove(&h);
                let t = d.objects[&h].type_name.clone();
                *self.skipped.entry(t).or_default() += 1;
            }
        }
        // Anonymous blocks, numbered per prefix in table order, as AutoCAD
        // numbers them when it writes a DXF.
        let mut next: BTreeMap<String, usize> = BTreeMap::new();
        let mut taken: BTreeSet<String> = d
            .objects
            .values()
            .filter_map(|o| match &o.kind {
                ObjectKind::BlockHeader(b) => Some(b.name.to_ascii_uppercase()),
                _ => None,
            })
            .collect();
        for rec in self.block_records() {
            let ObjectKind::BlockHeader(b) = &rec.kind else { continue };
            let bare = b.name.len() >= 2
                && b.name.starts_with('*')
                && b.name[1..].chars().all(|c| c.is_ascii_alphabetic())
                && !b.name.eq_ignore_ascii_case("*Model_Space")
                && !b.name.eq_ignore_ascii_case("*Paper_Space");
            if bare {
                let n = next.entry(b.name.to_ascii_uppercase()).or_insert(0);
                let name = loop {
                    let cand = format!("{}{}", b.name, n);
                    *n += 1;
                    if taken.insert(cand.to_ascii_uppercase()) {
                        break cand;
                    }
                };
                self.block_names.insert(rec.handle, name);
            }
        }
        // Dictionary keys, for names DXF gives by key ($CMLSTYLE).
        for n in self.ng.values() {
            if let NonGraphic::Dictionary(dict) = n {
                for (k, h) in &dict.entries {
                    self.keys.entry(*h).or_insert_with(|| k.clone());
                }
            }
        }
        if !self.skipped.is_empty() {
            let list: Vec<String> = self
                .skipped
                .iter()
                .map(|(t, n)| format!("{t} ×{n}"))
                .collect();
            self.notes.push(format!(
                "left out (the model keeps them only as DWG records, or they did not decode): {}",
                list.join(", ")
            ));
        }
    }

    fn ok(&self, h: Handle) -> bool {
        !h.is_null() && self.written.contains(&h)
    }

    // ----- group helpers ---------------------------------------------------

    fn s(&mut self, code: u16, v: impl Into<String>) {
        self.push(Group {
            code,
            value: Value::Str(v.into()),
        });
    }
    fn r(&mut self, code: u16, v: f64) {
        self.push(Group {
            code,
            value: Value::Real(v),
        });
    }
    fn i(&mut self, code: u16, v: i64) {
        self.push(Group {
            code,
            value: Value::Int(v),
        });
    }
    fn h(&mut self, code: u16, h: Handle) {
        self.push(Group {
            code,
            value: Value::Handle(h.0),
        });
    }
    fn p3(&mut self, code: u16, p: Vec3) {
        self.r(code, p.x);
        self.r(code + 10, p.y);
        self.r(code + 20, p.z);
    }
    fn p2(&mut self, code: u16, p: Vec2) {
        self.r(code, p.x);
        self.r(code + 10, p.y);
    }
    fn extrusion(&mut self, e: Vec3) {
        if e != Vec3::Z && e != Vec3::ZERO {
            self.p3(210, e);
        }
    }

    fn name(&self, h: Handle) -> String {
        if let Some(n) = self.block_names.get(&h) {
            return n.clone();
        }
        self.d.record_name(h).map(str::to_string).unwrap_or_default()
    }

    // ----- sections ------------------------------------------------------------

    /// The groups, the notes, and the objects left out by type; `None` instead of the groups
    /// when they would pass the limit given to [`Writer::within`].
    pub fn write(mut self) -> (Option<Vec<Group>>, Vec<String>, BTreeMap<String, usize>) {
        self.header();
        self.classes();
        self.tables();
        self.blocks();
        self.entities();
        self.objects();
        self.s(0, "EOF");
        let groups = if self.over { None } else { Some(self.out) };
        (groups, self.notes, self.skipped)
    }

    fn begin(&mut self, name: &str) {
        self.s(0, "SECTION");
        self.s(2, name);
    }
    fn end(&mut self) {
        self.s(0, "ENDSEC");
    }

    fn header(&mut self) {
        self.begin("HEADER");
        self.s(9, "$ACADVER");
        self.s(1, "AC1032");
        let flags = self.hv.int("FLAGS", 0x2A1D);
        for &(name, code, n, src, default) in HEADER {
            let dflt: Vec<&str> = default.split(',').collect();
            let dnum = |i: usize| dflt.get(i).and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0);
            let value: Option<Value> = match src {
                Src::Raw(dwg) => {
                    if n > 1 {
                        None
                    } else {
                        Some(match super::encode::kind(code) {
                            super::encode::Kind::Real => Value::Real(self.hv.real(dwg, dnum(0))),
                            super::encode::Kind::Str => Value::Str(self.hv.text(dwg, default)),
                            _ => Value::Int(self.hv.int(dwg, default.parse().unwrap_or(0))),
                        })
                    }
                }
                Src::Name(dwg) => {
                    let h = self.hv.handle(dwg);
                    let name = if h.is_null() {
                        default.to_string()
                    } else {
                        self.name(h)
                    };
                    Some(Value::Str(name))
                }
                Src::Color(dwg, fallback) => {
                    Some(Value::Int(self.hv.color(dwg, fallback).to_aci() as i64))
                }
                Src::Time(dwg) => Some(Value::Real(self.hv.time(dwg, 0.0))),
                Src::Flags(bit) => Some(Value::Int(match bit {
                    // CELWEIGHT: a lineweight code in the low five bits.
                    0 => Lineweight::from_code((flags & 0x1F) as u8).dxf() as i64,
                    1 => (flags >> 5) & 3,
                    2 => (flags >> 7) & 3,
                    3 => ((flags & 0x200) == 0) as i64,
                    4 => ((flags & 0x400) == 0) as i64,
                    5 => ((flags & 0x800) != 0) as i64,
                    6 => ((flags & 0x2000) != 0) as i64,
                    _ => ((flags & 0x4000) != 0) as i64,
                })),
                Src::Object(dwg) => {
                    let mut h = self.hv.handle(dwg);
                    // AutoCAD's DWG leaves the interference visual styles
                    // null; its DXF names the defaults.
                    if h.is_null() {
                        h = match dwg {
                            "INTERFEREOBJVS" => visual_style(self.d, "Realistic"),
                            "INTERFEREVPVS" => visual_style(self.d, "Wireframe"),
                            _ => h,
                        };
                    }
                    if !self.ok(h) {
                        continue;
                    }
                    Some(Value::Handle(h.0))
                }
                Src::Maint => Some(Value::Int(self.d.maintenance_version as i64)),
                Src::Codepage => Some(Value::Str(codepage_name(self.d.codepage).into())),
                Src::LastSavedBy => Some(Value::Str(
                    self.d.header.properties.last_saved_by.clone(),
                )),
                Src::Handseed => Some(Value::Handle(self.handseed())),
                Src::MlineStyle => {
                    let h = self.hv.handle("CMLSTYLE");
                    Some(Value::Str(
                        self.keys
                            .get(&h)
                            .cloned()
                            .unwrap_or_else(|| default.to_string()),
                    ))
                }
            };
            self.s(9, name);
            match value {
                Some(v) => self.push(Group { code, value: v }),
                None => {
                    // Points: only a raw variable leaves the value for here.
                    let Src::Raw(dwg) = src else {
                        continue;
                    };
                    if n == 3 {
                        let p = self.hv.p3(dwg, Vec3::new(dnum(0), dnum(1), dnum(2)));
                        self.p3(code, p);
                    } else {
                        let p = self.hv.p2(dwg, Vec2::new(dnum(0), dnum(1)));
                        self.p2(code, p);
                    }
                }
            }
        }
        self.end();
    }

    /// The next free handle.
    fn handseed(&self) -> u64 {
        let max = self.d.objects.keys().last().map(|h| h.0).unwrap_or(0);
        self.d.header.handseed.unwrap_or(0).max(max + 1)
    }

    fn classes(&mut self) {
        self.begin("CLASSES");
        let classes = self.d.classes.clone();
        for c in &classes {
            let count = self
                .d
                .objects
                .values()
                .filter(|o| o.type_code == c.number)
                .count();
            self.s(0, "CLASS");
            self.s(1, &c.dxf_name);
            self.s(2, &c.cpp_name);
            self.s(3, &c.app_name);
            self.i(90, c.proxy_flags as i64);
            // The class's own count when it knows more than this drawing
            // holds (objects of the class left out of a DXF).
            self.i(91, count.max(c.instance_count as usize) as i64);
            self.i(280, c.was_zombie as i64);
            self.i(281, c.is_entity() as i64);
        }
        self.end();
    }

    /// The control object of a table, by DWG type code.
    fn control(&self, code: u16) -> Option<(&'a Object, &'a Control)> {
        self.d.objects.values().find_map(|o| match &o.kind {
            ObjectKind::Control(c) if o.type_code == code => Some((o, c)),
            _ => None,
        })
    }

    /// The records of a table in order: the control's entries, then any
    /// other record of the type (ByBlock and ByLayer linetypes, the
    /// model- and paper-space block records).
    fn records(&self, record_code: u16, first: &[Handle], entries: &[Handle]) -> Vec<&'a Object> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for h in first.iter().chain(entries) {
            if let Some(o) = self.d.get(*h) {
                if o.type_code == record_code && seen.insert(*h) {
                    out.push(o);
                }
            }
        }
        for o in self.d.objects.values() {
            if o.type_code == record_code && seen.insert(o.handle) {
                out.push(o);
            }
        }
        out
    }

    fn tables(&mut self) {
        self.begin("TABLES");
        let ms = self.d.model_space();
        let ps = self.d.paper_space();
        let lt_first = [self.hv.handle("LTYPE_BYBLOCK"), self.hv.handle("LTYPE_BYLAYER")];
        for (name, ctl, rec) in [
            ("VPORT", 0x40u16, 0x41u16),
            ("LTYPE", 0x38, 0x39),
            ("LAYER", 0x32, 0x33),
            ("STYLE", 0x34, 0x35),
            ("VIEW", 0x3C, 0x3D),
            ("UCS", 0x3E, 0x3F),
            ("APPID", 0x42, 0x43),
            ("DIMSTYLE", 0x44, 0x45),
            ("BLOCK_RECORD", 0x30, 0x31),
        ] {
            let control = self.control(ctl);
            let entries: Vec<Handle> = control.map(|(_, c)| c.entries.clone()).unwrap_or_default();
            let first: Vec<Handle> = match rec {
                0x39 => lt_first.to_vec(),
                0x31 => vec![ms, ps],
                _ => Vec::new(),
            };
            let records = self.records(rec, &first, &entries);
            self.s(0, "TABLE");
            self.s(2, name);
            if let Some((o, _)) = control {
                self.h(5, o.handle);
                self.xdict_and_reactors(o);
            }
            self.h(330, Handle::NULL);
            self.s(100, "AcDbSymbolTable");
            let count = if control.is_some() { entries.len() } else { records.len() };
            self.i(70, count as i64);
            if rec == 0x45 {
                self.s(100, "AcDbDimStyleTable");
                self.i(71, records.len() as i64);
                for r in &records {
                    self.h(340, r.handle);
                }
            }
            for r in records {
                self.record(r);
            }
            self.s(0, "ENDTAB");
        }
        self.end();
    }

    /// 102 groups for the extension dictionary and the reactors.
    fn xdict_and_reactors(&mut self, o: &Object) {
        if self.ok(o.xdictionary) {
            self.s(102, "{ACAD_XDICTIONARY");
            self.h(360, o.xdictionary);
            self.s(102, "}");
        }
        let reactors: Vec<Handle> = o.reactors.iter().copied().filter(|h| self.ok(*h)).collect();
        if !reactors.is_empty() {
            self.s(102, "{ACAD_REACTORS");
            for h in reactors {
                self.h(330, h);
            }
            self.s(102, "}");
        }
    }

    fn record_start(&mut self, o: &Object, kind: &str, subclass: &str, name: &str, flags: i64) {
        self.s(0, kind);
        self.h(if kind == "DIMSTYLE" { 105 } else { 5 }, o.handle);
        self.xdict_and_reactors(o);
        self.h(330, o.owner);
        self.s(100, "AcDbSymbolTableRecord");
        self.s(100, subclass);
        self.s(2, name);
        self.i(70, flags);
    }

    fn record(&mut self, o: &Object) {
        let d = self.d;
        match (&o.kind, o.type_code) {
            (_, 0x41) => {
                let name = self.name(o.handle);
                let v = match nongraphic::vport(o, &d.classes, d.codepage) {
                    Some(Ok(v)) => v,
                    _ => {
                        let min = self.hv.p3("EXTMIN", Vec3::ZERO).xy();
                        let max = self.hv.p3("EXTMAX", Vec3::ZERO).xy();
                        self.notes.push(format!(
                            "VPORT {name}: no stored view settings; view set to the drawing extents"
                        ));
                        Vport::looking_at(min, max)
                    }
                };
                self.record_start(o, "VPORT", "AcDbViewportTableRecord", &name, 0);
                self.vport_body(&v);
            }
            (ObjectKind::Linetype(l), _) => {
                self.record_start(o, "LTYPE", "AcDbLinetypeTableRecord", &l.name, (l.flags & !0x50) as i64);
                self.s(3, &l.description);
                self.i(72, 65);
                self.i(73, l.elements.len() as i64);
                self.r(40, l.pattern_length);
                for e in &l.elements {
                    self.r(49, e.length);
                    self.i(74, e.complex as i64);
                    if e.complex != 0 {
                        self.i(75, if e.complex & 2 != 0 { 0 } else { e.shape as i64 });
                        if self.ok(e.style) {
                            self.h(340, e.style);
                        }
                        self.r(46, e.scale);
                        self.r(50, deg(e.rotation));
                        self.r(44, e.offset.x);
                        self.r(45, e.offset.y);
                        if e.complex & 2 != 0 {
                            self.s(9, &e.text);
                        }
                    }
                }
            }
            (ObjectKind::NamedRecord(r), 0x39) => {
                self.record_start(o, "LTYPE", "AcDbLinetypeTableRecord", &r.name, 0);
                self.s(3, "");
                self.i(72, 65);
                self.i(73, 0);
                self.r(40, 0.0);
            }
            (ObjectKind::Layer(l), _) => {
                let mut flags = 0;
                if l.frozen {
                    flags |= 1;
                }
                if l.flags & 0x04 != 0 {
                    flags |= 2;
                }
                if l.locked {
                    flags |= 4;
                }
                self.record_start(o, "LAYER", "AcDbLayerTableRecord", &l.name, flags);
                let aci = match l.color {
                    Color::Index(i) => i as i64,
                    other => other.to_aci() as i64,
                };
                self.i(62, if l.off { -aci } else { aci });
                if let Color::Rgb(r, g, b) = l.color {
                    self.i(420, ((r as i64) << 16) | ((g as i64) << 8) | b as i64);
                }
                let lt = if l.linetype.is_null() { "Continuous".to_string() } else { self.name(l.linetype) };
                self.s(6, lt);
                if !l.plot {
                    self.i(290, 0);
                }
                self.i(370, Lineweight::from_code(l.lineweight).dxf() as i64);
                if let Some(ps) = self.plot_style_normal() {
                    self.h(390, ps);
                }
            }
            (ObjectKind::TextStyle(t), _) => {
                let flags = t.flags & !0x05 | (t.flags & 0x05);
                self.record_start(o, "STYLE", "AcDbTextStyleTableRecord", &t.name, flags as i64);
                self.r(40, t.height);
                self.r(41, t.width_factor);
                self.r(50, deg(t.oblique));
                self.i(71, t.generation as i64);
                self.r(42, t.last_height);
                self.s(3, &t.font_file);
                self.s(4, &t.bigfont_file);
            }
            (ObjectKind::NamedRecord(r), 0x35) => {
                self.record_start(o, "STYLE", "AcDbTextStyleTableRecord", &r.name, 0);
                for (c, v) in [(40, 0.0), (41, 1.0), (50, 0.0)] {
                    self.r(c, v);
                }
                self.i(71, 0);
                self.r(42, 0.2);
                self.s(3, "txt");
                self.s(4, "");
            }
            (ObjectKind::NamedRecord(r), 0x43) => {
                self.record_start(o, "APPID", "AcDbRegAppTableRecord", &r.name, 0);
            }
            (ObjectKind::NamedRecord(r), 0x3D) => {
                // A named view kept by name only: written as a plain
                // top view of the drawing extents.
                self.record_start(o, "VIEW", "AcDbViewTableRecord", &r.name, 0);
                let min = self.hv.p3("EXTMIN", Vec3::ZERO).xy();
                let max = self.hv.p3("EXTMAX", Vec3::ZERO).xy();
                self.r(40, (max.y - min.y).abs().max(1.0));
                self.p2(10, (min + max) * 0.5);
                self.r(41, (max.x - min.x).abs().max(1.0));
                self.p3(11, Vec3::Z);
                self.p3(12, Vec3::ZERO);
                for c in [42, 43, 44, 50] {
                    self.r(c, if c == 42 { 50.0 } else { 0.0 });
                }
                self.i(71, 0);
                self.i(281, 0);
                self.i(72, 0);
            }
            (ObjectKind::NamedRecord(r), 0x3F) => {
                self.record_start(o, "UCS", "AcDbUCSTableRecord", &r.name, 0);
                self.p3(10, Vec3::ZERO);
                self.p3(11, Vec3::X);
                self.p3(12, Vec3::new(0.0, 1.0, 0.0));
                self.i(79, 0);
                self.r(146, 0.0);
            }
            (_, 0x45) => {
                let name = self.name(o.handle);
                let vars = self.dim_vars(o);
                self.record_start(o, "DIMSTYLE", "AcDbDimStyleTableRecord", &name, 0);
                self.dimstyle_body(&vars);
            }
            (ObjectKind::BlockHeader(b), _) => {
                let mut flags = 0;
                if b.anonymous {
                    flags |= 1;
                }
                if b.has_attribs {
                    flags |= 2;
                }
                if b.is_xref {
                    flags |= 4;
                }
                if b.is_overlaid {
                    flags |= 8;
                }
                self.s(0, "BLOCK_RECORD");
                self.h(5, o.handle);
                self.xdict_and_reactors(o);
                self.h(330, o.owner);
                self.s(100, "AcDbSymbolTableRecord");
                self.s(100, "AcDbBlockTableRecord");
                let name = self.name(o.handle);
                self.s(2, name);
                let layout = if self.ok(b.layout) { b.layout } else { Handle::NULL };
                self.h(340, layout);
                let _ = flags;
                self.i(70, b.insert_units as i64);
                self.i(280, 1);
                self.i(281, 0);
            }
            _ => return,
        }
        self.xdata(o);
    }

    /// The ACDBPLACEHOLDER of the "Normal" plot style, when written.
    fn plot_style_normal(&self) -> Option<Handle> {
        let mut nod = self.hv.handle("DICTIONARY_PLOTSTYLES");
        if nod.is_null() {
            // Files before R2000 have no header variable for it; the
            // named-object dictionary has the entry.
            let root = self.hv.handle("DICTIONARY_NAMED_OBJECTS");
            if let Some(NonGraphic::Dictionary(d)) = self.ng.get(&root) {
                nod = d
                    .entries
                    .iter()
                    .find(|(k, _)| k == "ACAD_PLOTSTYLENAME")
                    .map(|(_, h)| *h)
                    .unwrap_or(Handle::NULL);
            }
        }
        match self.ng.get(&nod) {
            Some(NonGraphic::Dictionary(dict)) => {
                let h = dict.default.or_else(|| {
                    dict.entries
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("Normal"))
                        .map(|(_, h)| *h)
                })?;
                self.ok(h).then_some(h)
            }
            _ => None,
        }
    }

    /// The dimension variables of a DIMSTYLE record: its own when the
    /// drawing kept them, otherwise the header's (the current style's).
    fn dim_vars(&mut self, o: &Object) -> DimVars {
        let d = self.d;
        if let Some(Ok((_, v))) = nongraphic::dimstyle(o, &d.classes, d.codepage) {
            return v;
        }
        let mut v = nongraphic::dim_vars_from_header(&self.hv);
        if let ObjectKind::DimStyle(s) = &o.kind {
            v.apply_model(s);
        } else {
            self.notes.push(format!(
                "DIMSTYLE {}: no stored variables; written from the header's dimension variables",
                self.name(o.handle)
            ));
        }
        v
    }

    fn vport_body(&mut self, v: &Vport) {
        self.p2(10, v.lower_left);
        self.p2(11, v.upper_right);
        self.p2(12, v.center);
        self.p2(13, v.snap_base);
        self.p2(14, v.snap_spacing);
        self.p2(15, v.grid_spacing);
        self.p3(16, v.direction);
        self.p3(17, v.target);
        self.r(40, v.view_height);
        self.r(41, v.aspect);
        self.r(42, v.lens);
        self.r(43, v.front_clip);
        self.r(44, v.back_clip);
        self.r(50, deg(v.snap_rotation));
        self.r(51, deg(v.twist));
        self.i(71, v.view_mode as i64);
        self.i(72, v.circle_zoom as i64);
        self.i(73, v.fast_zoom as i64);
        self.i(74, v.ucsicon as i64);
        self.i(75, v.snap_on as i64);
        self.i(76, v.grid_on as i64);
        self.i(77, v.snap_style as i64);
        self.i(78, v.snap_isopair as i64);
        self.i(281, v.render_mode as i64);
        self.i(65, v.ucs_per_viewport as i64);
        self.p3(110, v.ucs_origin);
        self.p3(111, v.ucs_x);
        self.p3(112, v.ucs_y);
        self.i(79, v.ucs_ortho_type as i64);
        self.r(146, v.ucs_elevation);
        if self.ok(v.visual_style) {
            self.h(348, v.visual_style);
        }
        self.i(60, v.grid_flags as i64);
        self.i(61, v.grid_major as i64);
        self.i(292, v.default_lights as i64);
        self.i(282, v.lighting_type as i64);
        self.r(141, v.brightness);
        self.r(142, v.contrast);
    }

    fn dimstyle_body(&mut self, v: &DimVars) {
        let aci = |c: Color| c.to_aci() as i64;
        self.s(3, &v.post);
        self.s(4, &v.apost);
        for (c, x) in [
            (40, v.scale), (41, v.asz), (42, v.exo), (43, v.dli), (44, v.exe), (45, v.rnd),
            (46, v.dle), (47, v.tp), (48, v.tm), (140, v.txt), (141, v.cen), (142, v.tsz),
            (143, v.altf), (144, v.lfac), (145, v.tvp), (146, v.tfac), (147, v.gap),
            (148, v.altrnd),
        ] {
            self.r(c, x);
        }
        for (c, x) in [
            (71, v.tol as i64), (72, v.lim as i64), (73, v.tih as i64), (74, v.toh as i64),
            (75, v.se1 as i64), (76, v.se2 as i64), (77, v.tad as i64), (78, v.zin as i64),
            (79, v.azin as i64), (170, v.alt as i64), (171, v.altd as i64), (172, v.tofl as i64),
            (173, v.sah as i64), (174, v.tix as i64), (175, v.soxd as i64), (176, aci(v.clrd)),
            (177, aci(v.clre)), (178, aci(v.clrt)), (179, v.adec as i64), (271, v.dec as i64),
            (272, v.tdec as i64), (273, v.altu as i64), (274, v.alttd as i64),
            (275, v.aunit as i64), (276, v.frac as i64), (277, v.lunit as i64),
            (278, v.dsep as i64), (279, v.tmove as i64), (280, v.just as i64),
            (281, v.sd1 as i64), (282, v.sd2 as i64), (283, v.tolj as i64), (284, v.tzin as i64),
            (285, v.altz as i64), (286, v.alttz as i64), (288, v.upt as i64), (289, v.fit as i64),
        ] {
            self.i(c, x);
        }
        for (c, h) in [(340, v.txsty), (341, v.ldrblk), (342, v.blk), (343, v.blk1), (344, v.blk2)] {
            if self.ok(h) {
                self.h(c, h);
            }
        }
        self.i(371, v.lwd as i64);
        self.i(372, v.lwe as i64);
    }

    // ----- blocks and entities ---------------------------------------------

    /// Block records in table order.
    fn block_records(&self) -> Vec<&'a Object> {
        let entries = self
            .control(0x30)
            .map(|(_, c)| c.entries.clone())
            .unwrap_or_default();
        self.records(0x31, &[self.d.model_space(), self.d.paper_space()], &entries)
    }

    fn blocks(&mut self) {
        self.begin("BLOCKS");
        let ms = self.d.model_space();
        let ps = self.d.paper_space();
        for rec in self.block_records() {
            let ObjectKind::BlockHeader(b) = &rec.kind else { continue };
            let paper = rec.handle == ps;
            if let Some(begin) = self.d.get(b.block_entity).filter(|o| self.ok(o.handle)) {
                self.entity(begin, rec.handle, paper);
            }
            if rec.handle != ms && rec.handle != ps {
                let ents: Vec<&Object> = self.d.block_entities(rec.handle);
                for e in ents {
                    self.entity_tree(e, rec.handle, false);
                }
            }
            if let Some(end) = self.d.get(b.endblk).filter(|o| self.ok(o.handle)) {
                self.entity(end, rec.handle, paper);
            }
        }
        self.end();
    }

    fn entities(&mut self) {
        self.begin("ENTITIES");
        let ms = self.d.model_space();
        let ps = self.d.paper_space();
        for (block, paper) in [(ms, false), (ps, true)] {
            if block.is_null() {
                continue;
            }
            for e in self.d.block_entities(block) {
                self.entity_tree(e, block, paper);
            }
        }
        self.end();
    }

    /// An entity and what it owns (vertices, attributes, the closing
    /// SEQEND).
    fn entity_tree(&mut self, o: &'a Object, block: Handle, paper: bool) {
        if !self.ok(o.handle) {
            return;
        }
        self.entity(o, block, paper);
        let d = self.d;
        let children: Vec<Handle> = match &o.kind {
            ObjectKind::Polyline(p) => {
                let mut v: Vec<Handle> = if p.vertices.is_empty() {
                    d.owned_by(o.handle)
                        .filter(|c| matches!(c.kind, ObjectKind::Vertex(_)))
                        .map(|c| c.handle)
                        .collect()
                } else {
                    p.vertices.clone()
                };
                v.push(p.seqend);
                v
            }
            ObjectKind::Insert(ins) if ins.has_attribs => {
                let mut v: Vec<Handle> = d.insert_attributes(o).iter().map(|c| c.handle).collect();
                v.push(ins.seqend);
                v
            }
            _ => Vec::new(),
        };
        for h in children {
            if let Some(c) = d.get(h) {
                if self.ok(c.handle) {
                    self.entity(c, o.handle, paper);
                }
            }
        }
    }

    fn entity(&mut self, o: &Object, owner_block: Handle, paper: bool) {
        let Some(kind) = entity_type(o) else { return };
        let Some(ent) = o.entity.clone() else { return };
        self.s(0, kind);
        self.h(5, o.handle);
        self.xdict_and_reactors(o);
        let owner = if o.owner.is_null() { owner_block } else { o.owner };
        self.h(330, owner);
        self.s(100, "AcDbEntity");
        if paper || ent.space == Space::Paper {
            self.i(67, 1);
        }
        let layer = self.name(ent.layer);
        self.s(8, if layer.is_empty() { "0".to_string() } else { layer });
        match ent.linetype {
            LinetypeRef::ByLayer => {}
            LinetypeRef::ByBlock => self.s(6, "ByBlock"),
            LinetypeRef::Continuous => self.s(6, "Continuous"),
            LinetypeRef::Handle(h) => {
                let n = self.name(h);
                if !n.eq_ignore_ascii_case("ByLayer") {
                    self.s(6, n);
                }
            }
        }
        match ent.color {
            Color::ByLayer => {}
            Color::ByBlock => self.i(62, 0),
            Color::Index(i) => self.i(62, i as i64),
            Color::Rgb(r, g, b) => {
                self.i(62, ent.color.to_aci() as i64);
                self.i(420, ((r as i64) << 16) | ((g as i64) << 8) | b as i64);
            }
            Color::None => self.i(62, 257),
        }
        if let Some(t) = ent.transparency {
            self.i(440, t as i32 as i64);
        }
        if self.ok(ent.plotstyle) {
            self.h(390, ent.plotstyle);
        }
        if ent.lineweight != Lineweight::CODE_BY_LAYER {
            self.i(370, Lineweight::from_code(ent.lineweight).dxf() as i64);
        }
        if ent.linetype_scale != 1.0 {
            self.r(48, ent.linetype_scale);
        }
        if ent.invisible {
            self.i(60, 1);
        }
        self.entity_body(o);
        self.xdata(o);
    }

    fn entity_body(&mut self, o: &Object) {
        let d = self.d;
        match &o.kind {
            ObjectKind::Line(l) => {
                self.s(100, "AcDbLine");
                if l.thickness != 0.0 {
                    self.r(39, l.thickness);
                }
                self.p3(10, l.start);
                self.p3(11, l.end);
                self.extrusion(l.extrusion);
            }
            ObjectKind::Circle(c) => {
                self.s(100, "AcDbCircle");
                if c.thickness != 0.0 {
                    self.r(39, c.thickness);
                }
                self.p3(10, c.center);
                self.r(40, c.radius);
                self.extrusion(c.extrusion);
            }
            ObjectKind::Arc(a) => {
                self.s(100, "AcDbCircle");
                if a.thickness != 0.0 {
                    self.r(39, a.thickness);
                }
                self.p3(10, a.center);
                self.r(40, a.radius);
                self.extrusion(a.extrusion);
                self.s(100, "AcDbArc");
                self.r(50, deg(a.start_angle));
                self.r(51, deg(a.end_angle));
            }
            ObjectKind::Point(p) => {
                self.s(100, "AcDbPoint");
                self.p3(10, p.position);
                if p.thickness != 0.0 {
                    self.r(39, p.thickness);
                }
                self.extrusion(p.extrusion);
                if p.x_axis_angle != 0.0 {
                    self.r(50, deg(p.x_axis_angle));
                }
            }
            ObjectKind::Ellipse(e) => {
                self.s(100, "AcDbEllipse");
                self.p3(10, e.center);
                self.p3(11, e.major_axis);
                self.p3(210, if e.extrusion == Vec3::ZERO { Vec3::Z } else { e.extrusion });
                self.r(40, e.axis_ratio);
                self.r(41, e.start_param);
                self.r(42, e.end_param);
            }
            ObjectKind::LwPolyline(p) => {
                self.s(100, "AcDbPolyline");
                self.i(90, p.points.len() as i64);
                let mut flags = 0;
                if p.is_closed() {
                    flags |= 1;
                }
                if p.flags & 0x100 != 0 {
                    flags |= 128;
                }
                self.i(70, flags);
                self.r(43, p.const_width);
                if p.elevation != 0.0 {
                    self.r(38, p.elevation);
                }
                if p.thickness != 0.0 {
                    self.r(39, p.thickness);
                }
                for (i, pt) in p.points.iter().enumerate() {
                    self.p2(10, *pt);
                    if let Some(&(a, b)) = p.widths.get(i) {
                        if a != 0.0 || b != 0.0 {
                            self.r(40, a);
                            self.r(41, b);
                        }
                    }
                    let bulge = p.bulge(i);
                    if bulge != 0.0 {
                        self.r(42, bulge);
                    }
                    if let Some(&id) = p.vertex_ids.get(i) {
                        self.i(91, id as i64);
                    }
                }
                self.extrusion(p.extrusion);
            }
            ObjectKind::Polyline(p) => {
                let sub = match p.kind {
                    PolylineKind::Polyline2d => "AcDb2dPolyline",
                    PolylineKind::Polyline3d => "AcDb3dPolyline",
                    PolylineKind::PolyfaceMesh => "AcDbPolyFaceMesh",
                    PolylineKind::PolygonMesh => "AcDbPolygonMesh",
                };
                self.s(100, sub);
                self.i(66, 1);
                self.p3(10, Vec3::new(0.0, 0.0, p.elevation));
                if p.thickness != 0.0 {
                    self.r(39, p.thickness);
                }
                self.i(70, p.flags as i64);
                if p.start_width != 0.0 {
                    self.r(40, p.start_width);
                }
                if p.end_width != 0.0 {
                    self.r(41, p.end_width);
                }
                if p.kind == PolylineKind::PolyfaceMesh {
                    let verts = d.polyline_vertices(o);
                    let nv = verts.iter().filter(|v| v.kind == VertexKind::PfaceVertex).count();
                    let nf = verts.iter().filter(|v| v.kind == VertexKind::PfaceFace).count();
                    self.i(71, nv as i64);
                    self.i(72, nf as i64);
                }
                if p.curve_type != 0 {
                    self.i(75, p.curve_type as i64);
                }
                self.extrusion(p.extrusion);
            }
            ObjectKind::Vertex(v) => {
                self.s(100, "AcDbVertex");
                let (sub, mut flags) = match v.kind {
                    VertexKind::Vertex2d => ("AcDb2dVertex", 0),
                    VertexKind::Vertex3d => ("AcDb3dPolylineVertex", 32),
                    VertexKind::Mesh => ("AcDbPolygonMeshVertex", 64),
                    VertexKind::PfaceVertex => ("AcDbPolyFaceMeshVertex", 192),
                    VertexKind::PfaceFace => ("AcDbFaceRecord", 128),
                };
                flags |= v.flags as i64;
                self.s(100, sub);
                self.p3(10, v.point);
                if v.kind == VertexKind::Vertex2d {
                    if v.start_width != 0.0 {
                        self.r(40, v.start_width);
                    }
                    if v.end_width != 0.0 {
                        self.r(41, v.end_width);
                    }
                    if v.bulge != 0.0 {
                        self.r(42, v.bulge);
                    }
                }
                self.i(70, flags);
                if v.tangent_dir != 0.0 {
                    self.r(50, deg(v.tangent_dir));
                }
                if v.kind == VertexKind::PfaceFace {
                    for (c, f) in (71..).zip(v.face) {
                        if f != 0 {
                            self.i(c, f as i64);
                        }
                    }
                }
                if v.id != 0 {
                    self.i(91, v.id as i64);
                }
            }
            ObjectKind::SeqEnd | ObjectKind::BlockEnd => {
                if matches!(o.kind, ObjectKind::BlockEnd) {
                    self.s(100, "AcDbBlockEnd");
                }
            }
            ObjectKind::Text(t) => {
                self.text_body(t, true);
            }
            ObjectKind::Attribute(a) => {
                self.text_body(&a.text, false);
                if a.is_definition {
                    self.s(100, "AcDbAttributeDefinition");
                    self.i(280, 0);
                    self.s(3, &a.prompt);
                } else {
                    self.s(100, "AcDbAttribute");
                    self.i(280, 0);
                }
                self.s(2, &a.tag);
                self.i(70, a.flags as i64);
                if a.field_length != 0 {
                    self.i(73, a.field_length as i64);
                }
                if a.text.v_align != 0 {
                    self.i(74, a.text.v_align as i64);
                }
                // AutoCAD 2007 and later: after the flags, unlike the
                // subclass version (280) before the tag.
                if a.lock_position {
                    self.i(280, 1);
                }
            }
            ObjectKind::MText(m) => {
                self.s(100, "AcDbMText");
                self.p3(10, m.insertion);
                self.r(40, m.text_height);
                self.r(41, m.rect_width);
                self.r(46, m.rect_height);
                self.i(71, m.attachment as i64);
                self.i(72, m.drawing_dir as i64);
                self.mtext_contents(&m.contents);
                let style = self.name(m.style);
                if !style.is_empty() && !style.eq_ignore_ascii_case("Standard") {
                    self.s(7, style);
                }
                self.extrusion(m.extrusion);
                if m.x_axis != Vec3::X && m.x_axis != Vec3::ZERO {
                    self.p3(11, m.x_axis);
                }
                self.i(73, 1);
                self.r(44, m.linespacing_factor);
            }
            ObjectKind::Insert(ins) => {
                if ins.has_attribs {
                    self.i(66, 1);
                }
                self.s(
                    100,
                    if ins.array.is_some() { "AcDbMInsertBlock" } else { "AcDbBlockReference" },
                );
                self.s(2, self.name(ins.block_header));
                self.p3(10, ins.insertion);
                if ins.scale.x != 1.0 {
                    self.r(41, ins.scale.x);
                }
                if ins.scale.y != 1.0 {
                    self.r(42, ins.scale.y);
                }
                if ins.scale.z != 1.0 {
                    self.r(43, ins.scale.z);
                }
                if ins.rotation != 0.0 {
                    self.r(50, deg(ins.rotation));
                }
                if let Some((cols, rows, cs, rs)) = ins.array {
                    self.i(70, cols as i64);
                    self.i(71, rows as i64);
                    self.r(44, cs);
                    self.r(45, rs);
                }
                self.extrusion(ins.extrusion);
            }
            ObjectKind::Solid(s) => {
                self.s(100, "AcDbTrace");
                if s.thickness != 0.0 {
                    self.r(39, s.thickness);
                }
                for (code, c) in (10..).step_by(1).zip(s.corners) {
                    self.p3(code, c.extend(s.elevation));
                }
                self.extrusion(s.extrusion);
            }
            ObjectKind::Face3d(f) => {
                self.s(100, "AcDbFace");
                for (code, c) in (10..).zip(f.corners) {
                    self.p3(code, c);
                }
                if f.invisible_edges != 0 {
                    self.i(70, f.invisible_edges as i64);
                }
            }
            ObjectKind::Spline(sp) => {
                self.s(100, "AcDbSpline");
                let mut flags = 8;
                if sp.closed {
                    flags |= 1;
                }
                if sp.periodic {
                    flags |= 2;
                }
                if sp.rational {
                    flags |= 4;
                }
                self.i(70, flags);
                self.i(71, sp.degree as i64);
                self.i(72, sp.knots.len() as i64);
                self.i(73, sp.control_points.len() as i64);
                self.i(74, sp.fit_points.len() as i64);
                self.r(42, 1e-10);
                self.r(43, 1e-10);
                if !sp.fit_points.is_empty() {
                    self.r(44, sp.fit_tolerance);
                    if sp.start_tangent != Vec3::ZERO {
                        self.p3(12, sp.start_tangent);
                    }
                    if sp.end_tangent != Vec3::ZERO {
                        self.p3(13, sp.end_tangent);
                    }
                }
                for k in &sp.knots {
                    self.r(40, *k);
                }
                if sp.rational {
                    for w in &sp.weights {
                        self.r(41, *w);
                    }
                }
                for p in &sp.control_points {
                    self.p3(10, *p);
                }
                for p in &sp.fit_points {
                    self.p3(11, *p);
                }
            }
            ObjectKind::Dimension(m) => self.dimension_body(m),
            ObjectKind::Hatch(h) => self.hatch_body(h),
            ObjectKind::Xline(x) | ObjectKind::Ray(x) => {
                self.s(100, if matches!(o.kind, ObjectKind::Ray(_)) { "AcDbRay" } else { "AcDbXline" });
                self.p3(10, x.base);
                self.p3(11, x.direction);
            }
            ObjectKind::Leader(l) => self.leader_body(l),
            ObjectKind::Tolerance(t) => self.tolerance_body(t),
            ObjectKind::MLine(m) => self.mline_body(m),
            ObjectKind::Image(r) => self.raster_body(r, "AcDbRasterImage"),
            ObjectKind::Wipeout(r) => self.raster_body(r, "AcDbWipeout"),
            ObjectKind::PaperViewport(v) => self.viewport_body(v),
            ObjectKind::BlockBegin { name } => {
                let b = d.block_header(o.owner).or_else(|| {
                    d.objects.values().find_map(|r| match &r.kind {
                        ObjectKind::BlockHeader(b) if b.block_entity == o.handle => Some(b),
                        _ => None,
                    })
                });
                self.s(100, "AcDbBlockBegin");
                let owner = if d.block_header(o.owner).is_some() {
                    o.owner
                } else {
                    d.objects
                        .values()
                        .find(|r| matches!(&r.kind, ObjectKind::BlockHeader(b) if b.block_entity == o.handle))
                        .map(|r| r.handle)
                        .unwrap_or(Handle::NULL)
                };
                let name = if owner.is_null() { name.clone() } else { self.name(owner) };
                self.s(2, &name);
                let mut flags = 0;
                if let Some(b) = b {
                    if b.anonymous {
                        flags |= 1;
                    }
                    if b.has_attribs {
                        flags |= 2;
                    }
                    if b.is_xref {
                        flags |= 4;
                    }
                    if b.is_overlaid {
                        flags |= 8;
                    }
                }
                self.i(70, flags);
                self.p3(10, b.map(|b| b.base_point).unwrap_or(Vec3::ZERO));
                self.s(3, &name);
                self.s(1, b.map(|b| b.xref_path.clone()).unwrap_or_default());
                if let Some(desc) = b.map(|b| b.description.clone()).filter(|s| !s.is_empty()) {
                    self.s(4, desc);
                }
            }
            _ => {}
        }
    }

    fn text_body(&mut self, t: &Text, plain_text: bool) {
        self.s(100, "AcDbText");
        if t.thickness != 0.0 {
            self.r(39, t.thickness);
        }
        self.p3(10, t.insertion.extend(t.elevation));
        self.r(40, t.height);
        self.s(1, &t.value);
        if t.rotation != 0.0 {
            self.r(50, deg(t.rotation));
        }
        if t.width_factor != 1.0 {
            self.r(41, t.width_factor);
        }
        if t.oblique != 0.0 {
            self.r(51, deg(t.oblique));
        }
        let style = self.name(t.style);
        if !style.is_empty() && !style.eq_ignore_ascii_case("Standard") {
            self.s(7, style);
        }
        if t.generation != 0 {
            self.i(71, t.generation as i64);
        }
        if t.h_align != 0 {
            self.i(72, t.h_align as i64);
        }
        if t.h_align != 0 || t.v_align != 0 {
            self.p3(11, t.alignment.extend(t.elevation));
        }
        self.extrusion(t.extrusion);
        if plain_text {
            self.s(100, "AcDbText");
            if t.v_align != 0 {
                self.i(73, t.v_align as i64);
            }
        }
    }

    /// MTEXT contents: groups 3 for each 250-character chunk but the last,
    /// which is group 1.
    fn mtext_contents(&mut self, s: &str) {
        let chars: Vec<char> = s.chars().collect();
        let mut chunks: Vec<String> = chars.chunks(250).map(|c| c.iter().collect()).collect();
        if chunks.is_empty() {
            chunks.push(String::new());
        }
        let last = chunks.pop().unwrap_or_default();
        for c in chunks {
            self.s(3, c);
        }
        self.s(1, last);
    }

    fn dimension_body(&mut self, m: &Dimension) {
        let d = self.d;
        self.s(100, "AcDbDimension");
        self.i(280, 0);
        self.s(2, self.name(m.block));
        self.p3(10, m.pt10);
        self.p3(11, m.text_midpoint.extend(m.elevation));
        let base = match m.kind {
            DimensionKind::Linear => 0,
            DimensionKind::Aligned => 1,
            DimensionKind::Angular2Line => 2,
            DimensionKind::Diameter => 3,
            DimensionKind::Radius => 4,
            DimensionKind::Angular3Pt => 5,
            DimensionKind::Ordinate => 6,
        };
        let mut t = base;
        if m.flags & 2 != 0 {
            t |= 32;
        }
        if m.flags & 1 == 0 {
            t |= 128;
        }
        self.i(70, t);
        self.i(71, m.attachment as i64);
        self.r(42, m.measurement);
        for c in [73, 74, 75] {
            self.i(c, 0);
        }
        if !m.user_text.is_empty() {
            self.s(1, &m.user_text);
        }
        if m.text_rotation != 0.0 {
            self.r(53, deg(m.text_rotation));
        }
        if m.horizontal_direction != 0.0 {
            self.r(51, deg(m.horizontal_direction));
        }
        self.extrusion(m.extrusion);
        let style = d.record_name(m.dimstyle).unwrap_or("Standard").to_string();
        self.s(3, style);
        match m.kind {
            DimensionKind::Linear | DimensionKind::Aligned => {
                self.s(100, "AcDbAlignedDimension");
                if m.clone_insertion != Vec2::ZERO {
                    self.p3(12, m.clone_insertion.extend(0.0));
                }
                self.p3(13, m.pt13);
                self.p3(14, m.pt14);
                if m.kind == DimensionKind::Linear {
                    if m.dim_rotation != 0.0 {
                        self.r(50, deg(m.dim_rotation));
                    }
                    if m.ext_line_rotation != 0.0 {
                        self.r(52, deg(m.ext_line_rotation));
                    }
                    self.s(100, "AcDbRotatedDimension");
                } else if m.ext_line_rotation != 0.0 {
                    self.r(52, deg(m.ext_line_rotation));
                }
            }
            DimensionKind::Angular2Line => {
                self.s(100, "AcDb2LineAngularDimension");
                self.p3(13, m.pt13);
                self.p3(14, m.pt14);
                self.p3(15, m.pt15);
                self.p3(16, m.pt16.extend(m.elevation));
            }
            DimensionKind::Angular3Pt => {
                self.s(100, "AcDb3PointAngularDimension");
                self.p3(13, m.pt13);
                self.p3(14, m.pt14);
                self.p3(15, m.pt15);
            }
            DimensionKind::Diameter => {
                self.s(100, "AcDbDiametricDimension");
                self.p3(15, m.pt15);
                self.r(40, m.leader_length);
            }
            DimensionKind::Radius => {
                self.s(100, "AcDbRadialDimension");
                self.p3(15, m.pt15);
                self.r(40, m.leader_length);
            }
            DimensionKind::Ordinate => {
                self.s(100, "AcDbOrdinateDimension");
                self.p3(13, m.pt13);
                self.p3(14, m.pt14);
            }
        }
    }

    fn hatch_body(&mut self, h: &Hatch) {
        self.s(100, "AcDbHatch");
        self.p3(10, Vec3::new(0.0, 0.0, h.elevation));
        self.p3(210, if h.extrusion == Vec3::ZERO { Vec3::Z } else { h.extrusion });
        self.s(2, &h.pattern_name);
        self.i(70, h.solid_fill as i64);
        self.i(71, h.associative as i64);
        self.i(91, h.paths.len() as i64);
        for (i, p) in h.paths.iter().enumerate() {
            match p {
                HatchPath::Polyline { flags, closed, vertices } => {
                    self.i(92, (*flags | 2) as i64);
                    let bulges = vertices.iter().any(|(_, b)| *b != 0.0);
                    self.i(72, bulges as i64);
                    self.i(73, *closed as i64);
                    self.i(93, vertices.len() as i64);
                    for (pt, b) in vertices {
                        self.p2(10, *pt);
                        if bulges {
                            self.r(42, *b);
                        }
                    }
                }
                HatchPath::Edges { flags, edges } => {
                    self.i(92, (*flags & !2) as i64);
                    self.i(93, edges.len() as i64);
                    for e in edges {
                        match e {
                            HatchEdge::Line { a, b } => {
                                self.i(72, 1);
                                self.p2(10, *a);
                                self.p2(11, *b);
                            }
                            HatchEdge::Arc { center, radius, start, end, ccw } => {
                                self.i(72, 2);
                                self.p2(10, *center);
                                self.r(40, *radius);
                                self.r(50, deg(*start));
                                self.r(51, deg(*end));
                                self.i(73, *ccw as i64);
                            }
                            HatchEdge::Ellipse { center, major, ratio, start, end, ccw } => {
                                self.i(72, 3);
                                self.p2(10, *center);
                                self.p2(11, *major);
                                self.r(40, *ratio);
                                self.r(50, deg(*start));
                                self.r(51, deg(*end));
                                self.i(73, *ccw as i64);
                            }
                            HatchEdge::Spline { degree, knots, control_points, weights } => {
                                self.i(72, 4);
                                self.i(94, *degree as i64);
                                self.i(73, !weights.is_empty() as i64);
                                self.i(74, 0);
                                self.i(95, knots.len() as i64);
                                self.i(96, control_points.len() as i64);
                                for k in knots {
                                    self.r(40, *k);
                                }
                                for (i, c) in control_points.iter().enumerate() {
                                    self.p2(10, *c);
                                    if let Some(w) = weights.get(i) {
                                        self.r(42, *w);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let sources = h.sources.get(i).map(Vec::as_slice).unwrap_or_default();
            self.i(97, sources.len() as i64);
            for s in sources {
                self.h(330, *s);
            }
        }
        self.i(75, h.style as i64);
        self.i(76, h.pattern_type as i64);
        if !h.solid_fill {
            self.r(52, deg(h.pattern_angle));
            self.r(41, h.pattern_scale);
            self.i(77, h.pattern_double as i64);
            // The line families: as stored, or from the program's library
            // for a hatch that has none, so other programs draw the same
            // pattern.
            let lines = match self.hatch_lines {
                Some(library) if h.pattern_lines.is_empty() => library(self.d, h),
                _ => h.pattern_lines.clone(),
            };
            self.i(78, lines.len() as i64);
            for l in &lines {
                self.r(53, deg(l.angle));
                self.r(43, l.base.x);
                self.r(44, l.base.y);
                self.r(45, l.offset.x);
                self.r(46, l.offset.y);
                self.i(79, l.dashes.len() as i64);
                for d in &l.dashes {
                    self.r(49, *d);
                }
            }
        }
        self.i(98, h.seed_points.len() as i64);
        for p in &h.seed_points {
            self.p2(10, *p);
        }
    }

    fn leader_body(&mut self, l: &Leader) {
        let d = self.d;
        self.s(100, "AcDbLeader");
        let style = d.record_name(l.dimstyle).unwrap_or("Standard").to_string();
        self.s(3, style);
        self.i(71, l.arrowhead as i64);
        self.i(72, l.spline as i64);
        self.i(73, l.annotation_type as i64);
        self.i(74, l.hookline_on_x as i64);
        self.i(75, l.hookline as i64);
        self.r(40, l.text_height);
        self.r(41, l.text_width);
        self.i(76, l.vertices.len() as i64);
        for v in &l.vertices {
            self.p3(10, *v);
        }
        if self.ok(l.annotation) {
            self.h(340, l.annotation);
        }
        self.p3(210, if l.extrusion == Vec3::ZERO { Vec3::Z } else { l.extrusion });
        self.p3(211, l.x_direction);
        self.p3(212, l.block_offset);
        self.p3(213, l.annotation_offset);
    }

    /// MLINE in the DXF reference's group order.
    fn mline_body(&mut self, m: &MLine) {
        let d = self.d;
        self.s(100, "AcDbMline");
        let name = match d.get(m.style).map(|o| &o.kind) {
            Some(ObjectKind::NonGraphic(NonGraphic::MLineStyle(st))) => st.name.clone(),
            _ => "STANDARD".to_string(),
        };
        self.s(2, name);
        if self.ok(m.style) {
            self.h(340, m.style);
        }
        self.r(40, m.scale);
        self.i(70, m.justification as i64);
        self.i(71, m.flags as i64);
        self.i(72, m.vertices.len() as i64);
        self.i(73, m.lines_in_style as i64);
        self.p3(10, m.base);
        self.p3(210, if m.extrusion == Vec3::ZERO { Vec3::Z } else { m.extrusion });
        for v in &m.vertices {
            self.p3(11, v.point);
            self.p3(12, v.direction);
            self.p3(13, v.miter);
            for i in 0..usize::from(m.lines_in_style) {
                let (seg, fill) = v.params.get(i).map(|(a, b)| (a.as_slice(), b.as_slice())).unwrap_or((&[], &[]));
                self.i(74, seg.len() as i64);
                for x in seg {
                    self.r(41, *x);
                }
                self.i(75, fill.len() as i64);
                for x in fill {
                    self.r(42, *x);
                }
            }
        }
    }

    /// IMAGE and WIPEOUT in the DXF reference's group order.
    fn raster_body(&mut self, r: &RasterImage, marker: &str) {
        self.s(100, marker);
        self.i(90, r.class_version as i64);
        self.p3(10, r.insertion);
        self.p3(11, r.u);
        self.p3(12, r.v);
        self.p2(13, r.size);
        if self.ok(r.definition) {
            self.h(340, r.definition);
        }
        self.i(70, r.display as i64);
        self.i(280, r.clipping as i64);
        self.i(281, r.brightness as i64);
        self.i(282, r.contrast as i64);
        self.i(283, r.fade as i64);
        if self.ok(r.reactor) {
            self.h(360, r.reactor);
        }
        let whole = [Vec2::new(-0.5, -0.5), Vec2::new(r.size.x - 0.5, r.size.y - 0.5)];
        let clip: &[Vec2] = if r.clip.is_empty() { &whole } else { &r.clip };
        let rect = clip.len() == 2 && (r.clip_type == 1 || r.clip.is_empty());
        self.i(71, if rect { 1 } else { 2 });
        self.i(91, clip.len() as i64);
        for p in clip {
            self.p2(14, *p);
        }
        self.i(290, r.clip_inside as i64);
    }

    /// TOLERANCE in the DXF reference's group order.
    fn tolerance_body(&mut self, t: &Tolerance) {
        let d = self.d;
        self.s(100, "AcDbFcf");
        let style = d.record_name(t.dimstyle).unwrap_or("Standard").to_string();
        self.s(3, style);
        self.p3(10, t.insertion);
        self.s(1, t.text.clone());
        self.p3(
            210,
            if t.extrusion == Vec3::ZERO {
                Vec3::Z
            } else {
                t.extrusion
            },
        );
        self.p3(11, t.x_direction);
    }

    fn viewport_body(&mut self, v: &PaperViewport) {
        self.s(100, "AcDbViewport");
        self.p3(10, v.center);
        self.r(40, v.width);
        self.r(41, v.height);
        // DXF 68: 0 off, else the stacking order (the id serves).
        self.i(68, if v.is_on() { v.id.max(1) as i64 } else { 0 });
        self.i(69, v.id as i64);
        self.p2(12, v.view_center);
        self.p2(13, v.snap_base);
        self.p2(14, v.snap_spacing);
        self.p2(15, v.grid_spacing);
        self.p3(16, v.view_direction);
        self.p3(17, v.view_target);
        self.r(42, v.lens);
        self.r(43, v.front_clip);
        self.r(44, v.back_clip);
        self.r(45, v.view_height);
        self.r(50, deg(v.snap_angle));
        self.r(51, deg(v.twist));
        self.i(72, v.circle_zoom as i64);
        for h in &v.frozen_layers {
            if self.ok(*h) {
                self.h(331, *h);
            }
        }
        self.i(90, v.status as i64);
        if self.ok(v.clip_boundary) {
            self.h(340, v.clip_boundary);
        }
        self.s(1, &v.style_sheet);
        self.i(281, v.render_mode as i64);
        self.i(71, v.ucs_per_viewport as i64);
        self.i(74, v.ucs_at_origin as i64);
        self.p3(110, v.ucs_origin);
        self.p3(111, v.ucs_x);
        self.p3(112, v.ucs_y);
        self.i(79, v.ucs_ortho_type as i64);
        self.r(146, v.ucs_elevation);
        if self.ok(v.named_ucs) {
            self.h(345, v.named_ucs);
        }
        if self.ok(v.base_ucs) {
            self.h(346, v.base_ucs);
        }
        self.i(170, v.shade_plot_mode as i64);
        self.i(61, v.grid_major as i64);
        if self.ok(v.visual_style) {
            self.h(348, v.visual_style);
        }
        self.i(292, v.default_lights as i64);
        self.i(282, v.lighting_type as i64);
        self.r(141, v.brightness);
        self.r(142, v.contrast);
        self.i(63, v.ambient.to_aci() as i64);
        if self.ok(v.sun) {
            self.h(361, v.sun);
        }
    }

    /// Extended data (1001 and the 1000-range groups).
    fn xdata(&mut self, o: &Object) {
        for x in &o.xdata {
            let app = match &x.app {
                XDataApp::Name(n) => n.clone(),
                XDataApp::Handle(h) => self.name(*h),
            };
            if app.is_empty() {
                continue;
            }
            self.s(1001, app);
            for it in &x.items {
                match it {
                    XValue::Str(s) => self.s(1000, s),
                    XValue::Open => self.s(1002, "{"),
                    XValue::Close => self.s(1002, "}"),
                    XValue::Layer(h) => self.s(1003, self.name(*h)),
                    XValue::Bytes(b) => self.push(Group { code: 1004, value: Value::Bytes(b.clone()) }),
                    XValue::Handle(h) => self.h(1005, *h),
                    XValue::Point(p) => self.p3(1010, *p),
                    XValue::Position(p) => self.p3(1011, *p),
                    XValue::Displacement(p) => self.p3(1012, *p),
                    XValue::Direction(p) => self.p3(1013, *p),
                    XValue::Real(v) => self.r(1040, *v),
                    XValue::Distance(v) => self.r(1041, *v),
                    XValue::ScaleFactor(v) => self.r(1042, *v),
                    XValue::Int16(v) => self.i(1070, *v as i64),
                    XValue::Int32(v) => self.i(1071, *v as i64),
                    XValue::Undecoded { .. } => {}
                }
            }
        }
    }

    // ----- objects -------------------------------------------------------------

    fn objects(&mut self) {
        self.begin("OBJECTS");
        let root = self.hv.handle("DICTIONARY_NAMED_OBJECTS");
        let mut order: Vec<Handle> = Vec::new();
        if self.ng.contains_key(&root) {
            order.push(root);
        }
        for (h, n) in &self.ng {
            if *h != root && matches!(n, NonGraphic::Dictionary(_)) {
                order.push(*h);
            }
        }
        for (h, n) in &self.ng {
            if !matches!(n, NonGraphic::Dictionary(_)) {
                order.push(*h);
            }
        }
        for h in order {
            let o = &self.d.objects[&h];
            let n = self.ng[&h].clone();
            self.object(o, &n);
        }
        self.end();
    }

    /// Typed groups as an extension record or a kept object holds them.
    fn items(&mut self, items: &[(u16, XrValue)]) {
        for (code, v) in items {
            match v {
                XrValue::Str(s) => self.s(*code, s),
                XrValue::Point(p) => self.p3(*code, *p),
                XrValue::Real(r) => self.r(*code, *r),
                XrValue::Int8(i) => self.i(*code, *i as i64),
                XrValue::Int16(i) => self.i(*code, *i as i64),
                XrValue::Int32(i) => self.i(*code, *i as i64),
                XrValue::Int64(i) => self.i(*code, *i),
                XrValue::Bool(b) => self.i(*code, *b as i64),
                XrValue::Bytes(b) => self.push(Group {
                    code: *code,
                    value: Value::Bytes(b.clone()),
                }),
                XrValue::Handle(h) => {
                    let h = if self.ok(*h) { *h } else { Handle::NULL };
                    self.h(*code, h)
                }
            }
        }
    }

    fn object(&mut self, o: &Object, n: &NonGraphic) {
        let kind = match n {
            NonGraphic::Dictionary(d) if d.default.is_some() => "ACDBDICTIONARYWDFLT",
            NonGraphic::Dictionary(_) => "DICTIONARY",
            NonGraphic::XRecord(_) => "XRECORD",
            NonGraphic::Placeholder => "ACDBPLACEHOLDER",
            NonGraphic::DictionaryVar { .. } => "DICTIONARYVAR",
            NonGraphic::Scale(_) => "SCALE",
            NonGraphic::Layout(_) => "LAYOUT",
            NonGraphic::MLineStyle(_) => "MLINESTYLE",
            NonGraphic::Groups(g) => g.kind.as_str(),
        };
        self.s(0, kind);
        self.h(5, o.handle);
        self.xdict_and_reactors(o);
        self.h(330, o.owner);
        match n {
            NonGraphic::Dictionary(dict) => {
                self.s(100, "AcDbDictionary");
                if dict.hard_owner != 0 {
                    self.i(280, dict.hard_owner as i64);
                }
                self.i(281, dict.cloning as i64);
                let code = if dict.hard_owner != 0 { 360 } else { 350 };
                for (k, h) in &dict.entries {
                    if self.ok(*h) {
                        self.s(3, k);
                        self.h(code, *h);
                    }
                }
                if let Some(def) = dict.default {
                    self.s(100, "AcDbDictionaryWithDefault");
                    self.h(340, def);
                }
            }
            NonGraphic::XRecord(x) => {
                self.s(100, "AcDbXrecord");
                self.i(280, x.cloning as i64);
                self.items(&x.items);
            }
            NonGraphic::Groups(g) => self.items(&g.items),
            NonGraphic::Placeholder => {}
            NonGraphic::DictionaryVar { schema, value } => {
                self.s(100, "DictionaryVariables");
                self.i(280, *schema as i64);
                self.s(1, value);
            }
            NonGraphic::Scale(s) => {
                self.s(100, "AcDbScale");
                self.i(70, s.flag as i64);
                self.s(300, &s.name);
                self.r(140, s.paper_units);
                self.r(141, s.drawing_units);
                self.i(290, s.unit_scale as i64);
            }
            NonGraphic::Layout(l) => {
                let p = &l.plot;
                self.s(100, "AcDbPlotSettings");
                self.s(1, &p.page_setup_name);
                self.s(2, &p.printer);
                self.s(4, &p.paper_size);
                self.s(6, "");
                for (c, v) in [(40, p.margins[0]), (41, p.margins[1]), (42, p.margins[2]), (43, p.margins[3]),
                    (44, p.paper_width), (45, p.paper_height), (46, p.origin.x), (47, p.origin.y),
                    (48, p.window_min.x), (49, p.window_min.y), (140, p.window_max.x), (141, p.window_max.y),
                    (142, p.real_world_units), (143, p.drawing_units)] {
                    self.r(c, v);
                }
                self.i(70, p.flags as i64);
                self.i(72, p.paper_units as i64);
                self.i(73, p.rotation as i64);
                self.i(74, p.plot_type as i64);
                self.s(7, &p.style_sheet);
                self.i(75, p.scale_type as i64);
                self.r(147, p.scale_factor);
                self.i(76, p.shade_plot_mode as i64);
                self.i(77, p.shade_plot_res_level as i64);
                self.i(78, p.shade_plot_custom_dpi as i64);
                self.r(148, p.image_origin.x);
                self.r(149, p.image_origin.y);
                self.s(100, "AcDbLayout");
                self.s(1, &l.name);
                self.i(70, l.flags as i64);
                self.i(71, l.tab_order as i64);
                self.p2(10, l.limmin);
                self.p2(11, l.limmax);
                self.p3(12, l.insbase);
                self.p3(14, l.extmin);
                self.p3(15, l.extmax);
                self.r(146, l.elevation);
                self.p3(13, l.ucs_origin);
                self.p3(16, l.ucs_x);
                self.p3(17, l.ucs_y);
                self.i(76, l.ortho_type as i64);
                self.h(330, l.block);
                if self.ok(l.last_viewport) {
                    self.h(331, l.last_viewport);
                }
                if self.ok(l.base_ucs) {
                    self.h(346, l.base_ucs);
                }
                if self.ok(l.named_ucs) {
                    self.h(345, l.named_ucs);
                }
            }
            NonGraphic::MLineStyle(m) => {
                self.s(100, "AcDbMlineStyle");
                self.s(2, &m.name);
                self.i(70, m.flags as i64);
                self.s(3, &m.description);
                self.i(62, m.fill_color.to_aci() as i64);
                self.r(51, deg(m.start_angle));
                self.r(52, deg(m.end_angle));
                self.i(71, m.elements.len() as i64);
                for e in &m.elements {
                    self.r(49, e.offset);
                    self.i(62, e.color.to_aci() as i64);
                    let lt = if e.linetype.is_null() {
                        "BYLAYER".to_string()
                    } else {
                        self.name(e.linetype).to_ascii_uppercase()
                    };
                    self.s(6, lt);
                }
            }
        }
        self.xdata(o);
    }
}

/// The visual style with this name, or null.
pub(super) fn visual_style(d: &Drawing, name: &str) -> Handle {
    d.objects
        .values()
        .find(|o| match &o.kind {
            ObjectKind::NonGraphic(NonGraphic::Groups(g)) => {
                g.kind == "VISUALSTYLE"
                    && g.items.iter().any(|(c, v)| *c == 2 && matches!(v, XrValue::Str(n) if n.eq_ignore_ascii_case(name)))
            }
            _ => false,
        })
        .map(|o| o.handle)
        .unwrap_or(Handle::NULL)
}

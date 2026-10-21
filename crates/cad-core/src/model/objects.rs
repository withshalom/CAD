//! Non-graphical objects and the table records the drawing keeps whole:
//! dictionaries, extension records, layouts, scales, multiline styles,
//! plot-style placeholders and dictionary variables (DXF OBJECTS
//! section), the VPORT record and a DIMSTYLE record's every variable.
//!
//! Their fields are those of the DXF reference and of the ODA
//! specification's chapter 20 (DICTIONARY §20.4.44, DICTIONARYWDFLT
//! §20.4.45, MLINESTYLE §20.4.73, DICTIONARYVAR §20.4.74, LAYOUT §20.4.84,
//! SCALE §20.4.92, XRECORD §20.4.104, VPORT §20.4.64, DIMSTYLE §20.4.68).
//! `dwg::nongraphic` reads them from DWG records, `dxf` reads and writes
//! them as DXF, `dwg::write` encodes them, and `native` stores them.

use super::{Color, Handle};
use crate::geom::{Vec2, Vec3};

#[derive(Debug, Clone, PartialEq)]
pub struct Dictionary {
    /// DXF 281.
    pub cloning: u16,
    /// DXF 280.
    pub hard_owner: u8,
    pub entries: Vec<(String, Handle)>,
    /// ACDBDICTIONARYWDFLT only: the default entry (DXF 340).
    pub default: Option<Handle>,
}

/// One value of an extension record, by DXF group code.
#[derive(Debug, Clone, PartialEq)]
pub enum XrValue {
    Str(String),
    Point(Vec3),
    Real(f64),
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    Bool(bool),
    Bytes(Vec<u8>),
    Handle(Handle),
}

#[derive(Debug, Clone, PartialEq)]
pub struct XRecord {
    /// DXF 280.
    pub cloning: u16,
    pub items: Vec<(u16, XrValue)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MLineElement {
    pub offset: f64,
    pub color: Color,
    /// R2018: the linetype record. Before R2018 the file stores an index.
    pub linetype: Handle,
    pub linetype_index: i16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MLineStyle {
    pub name: String,
    pub description: String,
    /// The DXF 70 flags (the DWG bit order converted).
    pub flags: u16,
    pub fill_color: Color,
    pub start_angle: f64,
    pub end_angle: f64,
    pub elements: Vec<MLineElement>,
}

/// The plot settings part of a layout (DXF groups of AcDbPlotSettings).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlotSettings {
    pub page_setup_name: String,
    pub printer: String,
    pub flags: u16,
    /// Left, bottom, right, top, millimetres.
    pub margins: [f64; 4],
    pub paper_width: f64,
    pub paper_height: f64,
    pub paper_size: String,
    pub origin: Vec2,
    pub paper_units: u16,
    pub rotation: u16,
    pub plot_type: u16,
    pub window_min: Vec2,
    pub window_max: Vec2,
    pub real_world_units: f64,
    pub drawing_units: f64,
    pub style_sheet: String,
    pub scale_type: u16,
    pub scale_factor: f64,
    pub image_origin: Vec2,
    pub shade_plot_mode: u16,
    pub shade_plot_res_level: u16,
    pub shade_plot_custom_dpi: u16,
    /// DXF 6, R2004+ by handle.
    pub plot_view: Handle,
    /// R2007+.
    pub visual_style: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub plot: PlotSettings,
    pub name: String,
    pub tab_order: u32,
    pub flags: u16,
    pub ucs_origin: Vec3,
    pub limmin: Vec2,
    pub limmax: Vec2,
    pub insbase: Vec3,
    pub ucs_x: Vec3,
    pub ucs_y: Vec3,
    pub elevation: f64,
    pub ortho_type: u16,
    pub extmin: Vec3,
    pub extmax: Vec3,
    /// The paper-space block record (DXF 330 of AcDbLayout).
    pub block: Handle,
    pub last_viewport: Handle,
    pub base_ucs: Handle,
    pub named_ucs: Handle,
    pub viewports: Vec<Handle>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scale {
    pub flag: u16,
    pub name: String,
    pub paper_units: f64,
    pub drawing_units: f64,
    pub unit_scale: bool,
}

/// An object kept as its DXF groups, for the style objects whose fields
/// the program carries without interpreting them: visual styles,
/// materials, table styles and their cell style maps, multileader, section
/// and detail view styles, and dimension associations. `items` are the
/// groups after the common part (from the first subclass marker, 100,
/// which is kept), in DXF order, points as one item. The DWG reader and
/// writer convert them with a codec per type (`dwg::styles`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Groups {
    /// The DXF object type, as written after group 0.
    pub kind: String,
    pub items: Vec<(u16, XrValue)>,
}

impl Groups {
    /// The object types kept this way.
    pub const KINDS: [&'static str; 8] = [
        "VISUALSTYLE",
        "MATERIAL",
        "TABLESTYLE",
        "CELLSTYLEMAP",
        "MLEADERSTYLE",
        "ACDBSECTIONVIEWSTYLE",
        "ACDBDETAILVIEWSTYLE",
        "DIMASSOC",
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub enum NonGraphic {
    Dictionary(Dictionary),
    XRecord(XRecord),
    Placeholder,
    DictionaryVar { schema: u8, value: String },
    Scale(Scale),
    Layout(Box<Layout>),
    MLineStyle(MLineStyle),
    Groups(Groups),
}

/// Every dimension variable a DIMSTYLE record stores (spec §20.4.68,
/// R2000+ layout), by its variable name. Colours are CmColors; angles in
/// radians; the handles are the text style, arrow blocks and linetypes.
#[derive(Debug, Clone, PartialEq)]
pub struct DimVars {
    pub post: String,
    pub apost: String,
    pub scale: f64,
    pub asz: f64,
    pub exo: f64,
    pub dli: f64,
    pub exe: f64,
    pub rnd: f64,
    pub dle: f64,
    pub tp: f64,
    pub tm: f64,
    pub fxl: f64,
    pub jogang: f64,
    pub tfill: u16,
    pub tfillclr: Color,
    pub tol: bool,
    pub lim: bool,
    pub tih: bool,
    pub toh: bool,
    pub se1: bool,
    pub se2: bool,
    pub tad: u16,
    pub zin: u16,
    pub azin: u16,
    pub arcsym: u16,
    pub txt: f64,
    pub cen: f64,
    pub tsz: f64,
    pub altf: f64,
    pub lfac: f64,
    pub tvp: f64,
    pub tfac: f64,
    pub gap: f64,
    pub altrnd: f64,
    pub alt: bool,
    pub altd: u16,
    pub tofl: bool,
    pub sah: bool,
    pub tix: bool,
    pub soxd: bool,
    pub clrd: Color,
    pub clre: Color,
    pub clrt: Color,
    pub adec: u16,
    pub dec: u16,
    pub tdec: u16,
    pub altu: u16,
    pub alttd: u16,
    pub aunit: u16,
    pub frac: u16,
    pub lunit: u16,
    pub dsep: u16,
    pub tmove: u16,
    pub just: u16,
    pub sd1: bool,
    pub sd2: bool,
    pub tolj: u16,
    pub tzin: u16,
    pub altz: u16,
    pub alttz: u16,
    pub upt: bool,
    pub fit: u16,
    pub fxlon: bool,
    pub txtdirection: bool,
    pub altmzf: f64,
    pub altmzs: String,
    pub mzf: f64,
    pub mzs: String,
    /// DXF lineweight values (-1 by layer, -2 by block, -3 default).
    pub lwd: i16,
    pub lwe: i16,
    /// The trailing bit the spec calls "unknown" (sets bit 1 of DXF 70).
    pub flag_bit: bool,
    pub txsty: Handle,
    pub ldrblk: Handle,
    pub blk: Handle,
    pub blk1: Handle,
    pub blk2: Handle,
    pub ltype: Handle,
    pub ltex1: Handle,
    pub ltex2: Handle,
}

/// A DIMSTYLE table record whole: its name and every variable.
#[derive(Debug, Clone, PartialEq)]
pub struct DimStyleRecord {
    pub name: String,
    pub vars: DimVars,
}

/// The viewport table record (VPORT, spec §20.4.64), R2000+ fields.
#[derive(Debug, Clone, PartialEq)]
pub struct Vport {
    pub name: String,
    pub view_height: f64,
    /// Width over height (the file stores width; this is divided back).
    pub aspect: f64,
    pub center: Vec2,
    pub target: Vec3,
    pub direction: Vec3,
    pub twist: f64,
    pub lens: f64,
    pub front_clip: f64,
    pub back_clip: f64,
    /// DXF 71 view mode bits.
    pub view_mode: u16,
    pub render_mode: u8,
    pub default_lights: bool,
    pub lighting_type: u8,
    pub brightness: f64,
    pub contrast: f64,
    pub ambient: Color,
    pub lower_left: Vec2,
    pub upper_right: Vec2,
    pub circle_zoom: u16,
    pub fast_zoom: bool,
    pub ucsicon: u8,
    pub grid_on: bool,
    pub grid_spacing: Vec2,
    pub snap_on: bool,
    pub snap_style: bool,
    pub snap_isopair: u16,
    pub snap_rotation: f64,
    pub snap_base: Vec2,
    pub snap_spacing: Vec2,
    pub unknown_bit: bool,
    pub ucs_per_viewport: bool,
    pub ucs_origin: Vec3,
    pub ucs_x: Vec3,
    pub ucs_y: Vec3,
    pub ucs_elevation: f64,
    pub ucs_ortho_type: u16,
    pub grid_flags: u16,
    pub grid_major: u16,
    pub background: Handle,
    pub visual_style: Handle,
    pub sun: Handle,
    pub named_ucs: Handle,
    pub base_ucs: Handle,
}

impl Vport {
    /// The *Active viewport AutoCAD's templates start with, looking at
    /// `min`–`max` with a margin.
    pub fn looking_at(min: Vec2, max: Vec2) -> Vport {
        let (w, h) = (max.x - min.x, max.y - min.y);
        let ok = w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0;
        let (center, height, aspect) = if ok {
            (Vec2::new((min.x + max.x) / 2.0, (min.y + max.y) / 2.0), h * 1.1, (w / h).max(1.0))
        } else {
            (Vec2::new(6.0, 4.5), 9.0, 12.0 / 9.0)
        };
        Vport {
            name: "*Active".into(),
            view_height: height,
            aspect,
            center,
            target: Vec3::ZERO,
            direction: Vec3::Z,
            twist: 0.0,
            lens: 50.0,
            front_clip: 0.0,
            back_clip: 0.0,
            view_mode: 0,
            render_mode: 0,
            default_lights: true,
            lighting_type: 1,
            brightness: 0.0,
            contrast: 0.0,
            ambient: Color::Index(250),
            lower_left: Vec2::ZERO,
            upper_right: Vec2::new(1.0, 1.0),
            circle_zoom: 100,
            fast_zoom: true,
            ucsicon: 3,
            grid_on: false,
            grid_spacing: Vec2::new(0.5, 0.5),
            snap_on: false,
            snap_style: false,
            snap_isopair: 0,
            snap_rotation: 0.0,
            snap_base: Vec2::ZERO,
            snap_spacing: Vec2::new(0.5, 0.5),
            unknown_bit: false,
            ucs_per_viewport: true,
            ucs_origin: Vec3::ZERO,
            ucs_x: Vec3::X,
            ucs_y: Vec3::new(0.0, 1.0, 0.0),
            ucs_elevation: 0.0,
            ucs_ortho_type: 0,
            grid_flags: 3,
            grid_major: 5,
            background: Handle::NULL,
            visual_style: Handle::NULL,
            sun: Handle::NULL,
            named_ucs: Handle::NULL,
            base_ucs: Handle::NULL,
        }
    }
}

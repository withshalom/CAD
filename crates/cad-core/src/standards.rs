//! The drafting standards new drawings start from: default layers,
//! linetypes, text and dimension styles, canvas and interaction colours,
//! and the Metric and Imperial templates.
//!
//! `docs/DESIGN.md` is the specification. The tests at the end of this
//! file read its tables and fail when the two disagree, so a change to the
//! design shows up as a failing test until the code follows.
//!
//! These are defaults, not rules. Every layer, the default ones included,
//! can be changed in each drawing (see the editing functions on
//! [`Drawing`]); drawings opened from DWG or DXF keep their own layers,
//! linetypes and styles exactly.

use crate::dwg::DwgVersion;
use crate::geom::{Vec2, Vec3};
use crate::model::color::aci_rgb;
use crate::model::*;
use crate::units::{convert, Unit};
use std::collections::BTreeMap;

/// An sRGB colour.
pub type Rgb = (u8, u8, u8);

/// The drawing canvas (DESIGN.md §2).
pub mod canvas {
    use super::Rgb;
    /// Model-space background: near-black.
    pub const BACKGROUND: Rgb = (0x0D, 0x10, 0x13);
    /// Grid lines.
    pub const GRID_MINOR: Rgb = (0x15, 0x1A, 0x1F);
    /// Every fifth grid line.
    pub const GRID_MAJOR: Rgb = (0x1D, 0x24, 0x2B);
    /// Grid spacing in drawing units.
    pub const GRID_SPACING: f64 = 10.0;
    /// Every this many grid lines is a major line.
    pub const GRID_MAJOR_EVERY: u32 = 5;
    /// Paper: printing is black on white.
    pub const PAPER: Rgb = (0xFF, 0xFF, 0xFF);
}

/// Colours reserved for interaction (DESIGN.md §5). No default layer uses
/// them and they never print.
pub mod interaction {
    use super::Rgb;
    /// Selected objects and their grips.
    pub const SELECTION: Rgb = (0x3D, 0x8B, 0xFF);
    /// Opacity of the glow on the object under the cursor.
    pub const HOVER_OPACITY: f64 = 0.35;
    /// Snap markers and tracking paths.
    pub const SNAP: Rgb = (0xFF, 0x4F, 0xD8);
    /// Interface accent (controls only, never the canvas).
    pub const ACCENT: Rgb = (0xF2, 0xA9, 0x3B);
    /// Size of a square grip, pixels.
    pub const GRIP_PX: f64 = 8.0;
    /// Snap aperture with a mouse, pixels.
    pub const APERTURE_MOUSE_PX: f64 = 10.0;
    /// Snap aperture on touch screens, pixels.
    pub const APERTURE_TOUCH_PX: f64 = 22.0;
}

/// Index colours drawn softened on the dark canvas, because the pure
/// values look harsh as thin lines (DESIGN.md §3). Printing uses the true
/// colours.
pub const SOFTENED: [(u8, Rgb); 4] = [
    (1, (0xFF, 0x4D, 0x4D)),
    (2, (0xFF, 0xE1, 0x4D)),
    (3, (0x3D, 0xDC, 0x84)),
    (4, (0x3F, 0xE0, 0xFF)),
];

/// How a resolved colour (an index or a true colour) is drawn on the dark
/// canvas.
pub fn screen_color(c: Color) -> Rgb {
    match c {
        Color::Index(i) => SOFTENED
            .iter()
            .find(|(k, _)| *k == i)
            .map(|(_, rgb)| *rgb)
            .unwrap_or_else(|| aci_rgb(i)),
        Color::Rgb(r, g, b) => (r, g, b),
        _ => aci_rgb(7),
    }
}

/// How a resolved colour prints on white paper (DESIGN.md §2). Printing
/// is black on white: in monochrome everything prints black; in colour,
/// white (index 7, which DWG draws as the opposite of the background, and
/// pure white) prints black and other colours print true.
pub fn print_color(c: Color, in_color: bool) -> Rgb {
    if !in_color {
        return (0, 0, 0);
    }
    match c {
        Color::Index(7 | 255) | Color::Rgb(255, 255, 255) => (0, 0, 0),
        Color::Index(i) => aci_rgb(i),
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

/// Pixels a line is drawn with on screen, at any zoom (DESIGN.md §3):
/// thin lines (below 0.40 mm, the defaults' 0.25 mm among them) 1 pixel,
/// thick lines (0.40 mm up to 1.00 mm, the default 0.50 mm among them)
/// 2 pixels, heavier ones 3 pixels.
pub fn screen_px(lineweight_mm: f64) -> f64 {
    if lineweight_mm < 0.40 {
        1.0
    } else if lineweight_mm < 1.0 {
        2.0
    } else {
        3.0
    }
}

/// Name of the continuous linetype record, as DWG names it.
pub const CONTINUOUS: &str = "Continuous";

/// A default layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerSpec {
    pub name: &'static str,
    /// DXF colour index.
    pub aci: u8,
    /// How the colour is drawn on the dark canvas.
    pub screen: Rgb,
    /// Linetype record name.
    pub linetype: &'static str,
    pub lineweight: Lineweight,
    pub purpose: &'static str,
}

/// The layers every new drawing starts with (DESIGN.md §3), `0` first.
pub const DEFAULT_LAYERS: [LayerSpec; 6] = [
    LayerSpec {
        name: "0",
        aci: 7,
        screen: (0xFF, 0xFF, 0xFF),
        linetype: CONTINUOUS,
        lineweight: Lineweight::Default,
        purpose: "Required by DWG and DXF; used inside block definitions",
    },
    LayerSpec {
        name: "VISIBLE",
        aci: 7,
        screen: (0xFF, 0xFF, 0xFF),
        linetype: CONTINUOUS,
        lineweight: Lineweight::Hundredths(50),
        purpose: "Visible outlines and edges",
    },
    LayerSpec {
        name: "CENTER",
        aci: 2,
        screen: (0xFF, 0xE1, 0x4D),
        linetype: "CENTER",
        lineweight: Lineweight::Hundredths(25),
        purpose: "Centre lines",
    },
    LayerSpec {
        name: "HIDDEN",
        aci: 1,
        screen: (0xFF, 0x4D, 0x4D),
        linetype: "HIDDEN",
        lineweight: Lineweight::Hundredths(25),
        purpose: "Hidden edges",
    },
    LayerSpec {
        name: "DIMENSION",
        aci: 3,
        screen: (0x3D, 0xDC, 0x84),
        linetype: CONTINUOUS,
        lineweight: Lineweight::Hundredths(25),
        purpose: "Dimensions, with their text",
    },
    LayerSpec {
        name: "NOTES",
        aci: 4,
        screen: (0x3F, 0xE0, 0xFF),
        linetype: CONTINUOUS,
        lineweight: Lineweight::Hundredths(25),
        purpose: "Notes, labels and title block text",
    },
];

/// The default layer of this name, compared without regard to case.
pub fn layer_spec(name: &str) -> Option<&'static LayerSpec> {
    DEFAULT_LAYERS
        .iter()
        .find(|l| l.name.eq_ignore_ascii_case(name))
}

/// A linetype of the project's own (DESIGN.md §4), in millimetres and in
/// inches. Lengths: positive dash, negative gap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinetypeSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub mm: &'static [f64],
    pub inch: &'static [f64],
}

/// Centre lines: long 12, gap 1.5, short 3, gap 1.5 (mm).
pub const CENTER: LinetypeSpec = LinetypeSpec {
    name: "CENTER",
    description: "Center ____ _ ____ _ ____",
    mm: &[12.0, -1.5, 3.0, -1.5],
    inch: &[0.5, -0.0625, 0.125, -0.0625],
};

/// Hidden lines: dash 3, gap 1.5 (mm).
pub const HIDDEN: LinetypeSpec = LinetypeSpec {
    name: "HIDDEN",
    description: "Hidden __ __ __ __",
    mm: &[3.0, -1.5],
    inch: &[0.125, -0.0625],
};

/// The patterned linetypes new drawings get.
pub const LINETYPES: [LinetypeSpec; 2] = [CENTER, HIDDEN];

/// The linetypes that can be loaded into a drawing (LINETYPE), each also
/// at half size (`…2`) and double (`…X2`). The patterns are this project's
/// own, built from the line kinds of technical drawing standards under the
/// names CAD programs use for them, so a drawing reads as intended
/// elsewhere.
pub const LIBRARY: [LinetypeSpec; 8] = [
    CENTER,
    HIDDEN,
    LinetypeSpec {
        name: "PHANTOM",
        description: "Phantom ______ __ __ ______",
        mm: &[12.0, -1.5, 3.0, -1.5, 3.0, -1.5],
        inch: &[0.5, -0.0625, 0.125, -0.0625, 0.125, -0.0625],
    },
    LinetypeSpec {
        name: "DASHED",
        description: "Dashed __ __ __ __ __",
        mm: &[6.0, -1.5],
        inch: &[0.25, -0.0625],
    },
    LinetypeSpec {
        name: "DASHDOT",
        description: "Dash dot __ . __ . __ .",
        mm: &[6.0, -1.5, 0.0, -1.5],
        inch: &[0.25, -0.0625, 0.0, -0.0625],
    },
    LinetypeSpec {
        name: "DOT",
        description: "Dot . . . . . . . .",
        mm: &[0.0, -1.5],
        inch: &[0.0, -0.0625],
    },
    LinetypeSpec {
        name: "DIVIDE",
        description: "Divide __ . . __ . . __",
        mm: &[6.0, -1.5, 0.0, -1.5, 0.0, -1.5],
        inch: &[0.25, -0.0625, 0.0, -0.0625, 0.0, -0.0625],
    },
    LinetypeSpec {
        name: "BORDER",
        description: "Border __ __ . __ __ .",
        mm: &[6.0, -1.5, 6.0, -1.5, 0.0, -1.5],
        inch: &[0.25, -0.0625, 0.25, -0.0625, 0.0, -0.0625],
    },
];

/// A library linetype by name, variants included ("PHANTOM2" is half
/// size, "PHANTOMX2" double): its name as written, description and
/// pattern in the drawing's unit.
pub fn library_linetype(name: &str, unit: Unit) -> Option<(String, String, Vec<f64>)> {
    let upper = name.trim().to_ascii_uppercase();
    for spec in &LIBRARY {
        let (scale, tag) = match upper.strip_prefix(spec.name) {
            Some("") => (1.0, ""),
            Some("2") => (0.5, " (.5x)"),
            Some("X2") => (2.0, " (2x)"),
            _ => continue,
        };
        // The tag goes after the name, before the picture of the line.
        let desc = match spec.description.find(['_', '.']) {
            Some(i) => format!("{}{tag} {}", spec.description[..i].trim_end(), &spec.description[i..]),
            None => format!("{}{tag}", spec.description),
        };
        return Some((upper, desc, spec.pattern(unit).iter().map(|v| v * scale).collect()));
    }
    None
}

/// Every library linetype name, variants included, in library order.
pub fn library_names() -> Vec<String> {
    LIBRARY
        .iter()
        .flat_map(|s| [s.name.to_string(), format!("{}2", s.name), format!("{}X2", s.name)])
        .collect()
}

impl LinetypeSpec {
    /// The pattern in a drawing's unit: the inch definition converted for
    /// inch-based units, the millimetre one for the others (as is when the
    /// drawing is unitless).
    pub fn pattern(&self, unit: Unit) -> Vec<f64> {
        let (base, from) = if unit.is_imperial() {
            (self.inch, Unit::Inch)
        } else {
            (self.mm, Unit::Millimeter)
        };
        base.iter()
            .map(|&v| convert(v, from, unit).unwrap_or(v))
            .collect()
    }
}

/// The project's pattern for a linetype name, for linetype records that
/// carry only a name (a DWG record the reader could not decode): the
/// names of the [`LIBRARY`] match, with `…2` half size and `…X2` double,
/// following the usual naming.
pub fn pattern_by_name(name: &str, unit: Unit) -> Option<Vec<f64>> {
    let upper = name.to_ascii_uppercase();
    for spec in &LIBRARY {
        let scale = match upper.strip_prefix(spec.name) {
            Some("") => 1.0,
            Some("2") => 0.5,
            Some("X2") => 2.0,
            _ => continue,
        };
        return Some(spec.pattern(unit).iter().map(|v| v * scale).collect());
    }
    None
}

/// Name of the text style new drawings use.
pub const TEXT_STYLE: &str = "Standard";

/// Font file the default text style names. Other programs draw it with
/// their own Simplex font; this app draws every SHX font with its own
/// single-stroke font (`crate::font`), so the file is never needed.
pub const FONT_FILE: &str = "simplex.shx";

/// Text height for notes: 3.5 mm or 1/8 in (DESIGN.md §6).
pub fn text_height(unit: Unit) -> f64 {
    if unit.is_imperial() {
        convert(0.125, Unit::Inch, unit).unwrap_or(0.125)
    } else {
        convert(3.5, Unit::Millimeter, unit).unwrap_or(3.5)
    }
}

/// Name of the dimension style new drawings use.
pub const DIM_STYLE: &str = "Standard";

/// The default dimension style for a unit (DESIGN.md §7): millimetre
/// sizes for metric and unitless drawings, inch sizes for inch-based ones,
/// converted to the drawing's unit.
pub fn dim_style(unit: Unit) -> DimStyle {
    let imperial = unit.is_imperial();
    let from = if imperial {
        Unit::Inch
    } else {
        Unit::Millimeter
    };
    let size = |mm: f64, inch: f64| {
        let v = if imperial { inch } else { mm };
        if unit == Unit::Unitless {
            v
        } else {
            convert(v, from, unit).unwrap_or(v)
        }
    };
    DimStyle {
        name: DIM_STYLE.into(),
        text_height: size(3.5, 0.125),
        arrow_size: size(3.0, 0.125),
        ext_offset: size(1.5, 0.0625),
        ext_extension: size(2.0, 0.125),
        baseline_spacing: size(10.0, 0.375),
        text_gap: size(1.0, 0.04),
        decimals: if imperial { 3 } else { 2 },
        alt_units: false,
        // Alternate units: millimetres beside imperial drawings, inches
        // beside the rest (unitless ones read as millimetres, like the
        // sizes above).
        alt_factor: match unit {
            Unit::Unitless => 1.0 / 25.4,
            u if imperial => convert(1.0, u, Unit::Millimeter).unwrap_or(25.4),
            u => convert(1.0, u, Unit::Inch).unwrap_or(1.0 / 25.4),
        },
        alt_decimals: if imperial { 2 } else { 3 },
        ..DimStyle::default()
    }
}

/// Arrowheads are filled triangles this many times as long as they are
/// wide (DESIGN.md §7).
pub const ARROW_LENGTH_TO_WIDTH: f64 = 3.0;

/// What a new drawing starts from (DESIGN.md §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// Millimetres.
    Metric,
    /// Inches.
    Imperial,
}

impl Template {
    pub fn unit(self) -> Unit {
        match self {
            Template::Metric => Unit::Millimeter,
            Template::Imperial => Unit::Inch,
        }
    }
}

/// Start a new drawing: the default layers, linetypes, text and dimension
/// styles, model and paper space, in the template's unit. The current
/// layer is `VISIBLE`.
pub fn new_drawing(template: Template) -> Drawing {
    let unit = template.unit();
    let mut objects: BTreeMap<Handle, Object> = BTreeMap::new();
    let mut next = 1u64;
    let mut alloc = || {
        let h = Handle(next);
        next += 1;
        h
    };
    let block_ctl = alloc();
    let layer_ctl = alloc();
    let style_ctl = alloc();
    let ltype_ctl = alloc();
    let dimstyle_ctl = alloc();
    let model = alloc();
    let paper = alloc();
    let (model_begin, model_end, paper_begin, paper_end) = (alloc(), alloc(), alloc(), alloc());
    let by_block = alloc();
    let by_layer = alloc();
    let continuous = alloc();
    let patterned: Vec<Handle> = LINETYPES.iter().map(|_| alloc()).collect();
    let style = alloc();
    let dimstyle = alloc();
    let layers: Vec<Handle> = DEFAULT_LAYERS.iter().map(|_| alloc()).collect();
    let seed = next;

    let mut put = |o: Object| {
        objects.insert(o.handle, o);
    };
    let control = |h, name: &str, code, entries: Vec<Handle>| {
        Object::record(
            h,
            name,
            code,
            Handle::NULL,
            ObjectKind::Control(Control {
                entries,
                model_space: if code == 0x30 { model } else { Handle::NULL },
                paper_space: if code == 0x30 { paper } else { Handle::NULL },
            }),
        )
    };
    let mut ltypes = vec![by_block, by_layer, continuous];
    ltypes.extend(&patterned);
    put(control(block_ctl, "BLOCK_CONTROL", 0x30, Vec::new()));
    put(control(layer_ctl, "LAYER_CONTROL", 0x32, layers.clone()));
    put(control(style_ctl, "STYLE_CONTROL", 0x34, vec![style]));
    put(control(ltype_ctl, "LTYPE_CONTROL", 0x38, ltypes));
    put(control(
        dimstyle_ctl,
        "DIMSTYLE_CONTROL",
        0x44,
        vec![dimstyle],
    ));

    let layer0 = layers[0];
    for (h, begin, end, name) in [
        (model, model_begin, model_end, "*Model_Space"),
        (paper, paper_begin, paper_end, "*Paper_Space"),
    ] {
        put(Object::record(
            h,
            "BLOCK_HEADER",
            0x31,
            block_ctl,
            ObjectKind::BlockHeader(BlockHeader {
                name: name.into(),
                insert_units: unit.insunits(),
                block_entity: begin,
                endblk: end,
                ..Default::default()
            }),
        ));
        let mut b = Object::record(
            begin,
            "BLOCK",
            0x04,
            h,
            ObjectKind::BlockBegin { name: name.into() },
        );
        b.entity = Some(EntityCommon {
            layer: layer0,
            ..Default::default()
        });
        put(b);
        let mut e = Object::record(end, "ENDBLK", 0x05, h, ObjectKind::BlockEnd);
        e.entity = Some(EntityCommon {
            layer: layer0,
            ..Default::default()
        });
        put(e);
    }

    let ltype =
        |h, lt: Linetype| Object::record(h, "LTYPE", 0x39, ltype_ctl, ObjectKind::Linetype(lt));
    put(ltype(by_block, Linetype::simple("ByBlock", "", &[])));
    put(ltype(by_layer, Linetype::simple("ByLayer", "", &[])));
    put(ltype(
        continuous,
        Linetype::simple(CONTINUOUS, "Solid line", &[]),
    ));
    for (spec, h) in LINETYPES.iter().zip(&patterned) {
        put(ltype(
            *h,
            Linetype::simple(spec.name, spec.description, &spec.pattern(unit)),
        ));
    }
    put(Object::record(
        style,
        "STYLE",
        0x35,
        style_ctl,
        ObjectKind::TextStyle(TextStyle {
            name: TEXT_STYLE.into(),
            last_height: text_height(unit),
            font_file: FONT_FILE.into(),
            ..Default::default()
        }),
    ));
    put(Object::record(
        dimstyle,
        "DIMSTYLE",
        0x45,
        dimstyle_ctl,
        ObjectKind::DimStyle(DimStyle {
            text_style: style,
            ..dim_style(unit)
        }),
    ));
    let linetype_of = |name: &str| {
        if name == CONTINUOUS {
            continuous
        } else {
            LINETYPES
                .iter()
                .position(|s| s.name == name)
                .map(|i| patterned[i])
                .unwrap_or(continuous)
        }
    };
    for (spec, h) in DEFAULT_LAYERS.iter().zip(&layers) {
        let mut layer = Layer {
            name: spec.name.into(),
            color: Color::Index(spec.aci),
            linetype: linetype_of(spec.linetype),
            lineweight: spec.lineweight.code(),
            ..Default::default()
        };
        layer.flags = layer.dwg_flags();
        put(Object::record(
            *h,
            "LAYER",
            0x33,
            layer_ctl,
            ObjectKind::Layer(layer),
        ));
    }

    let (limmax, measurement) = match template {
        // A3 landscape and ANSI B landscape.
        Template::Metric => (Vec2::new(420.0, 297.0), 1),
        Template::Imperial => (Vec2::new(17.0, 11.0), 0),
    };
    let header = HeaderVars {
        verified: true,
        insunits: Some(unit.insunits()),
        measurement: Some(measurement),
        limmin: Some(Vec2::ZERO),
        limmax: Some(limmax),
        insbase: Some(Vec3::ZERO),
        ltscale: Some(1.0),
        celtscale: Some(1.0),
        textsize: Some(text_height(unit)),
        lunits: Some(2),
        luprec: Some(if template == Template::Imperial { 3 } else { 2 }),
        aunits: Some(0),
        auprec: Some(2),
        angbase: Some(0.0),
        angdir: Some(0),
        handseed: Some(seed),
        clayer: layers[1],
        textstyle: style,
        celtype: by_layer,
        dimstyle,
        block_control: block_ctl,
        layer_control: layer_ctl,
        style_control: style_ctl,
        linetype_control: ltype_ctl,
        dimstyle_control: dimstyle_ctl,
        model_space: model,
        paper_space: paper,
        ..Default::default()
    };
    let mut d = Drawing {
        version: DwgVersion::R2018,
        maintenance_version: 0,
        codepage: 30,
        header,
        classes: Vec::new(),
        objects: objects.into(),
        sections: Vec::new(),
        diagnostics: Vec::new(),
        index: DrawingIndex::default(),
    };
    d.rebuild_index();
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::parse_length;

    #[test]
    fn a_new_drawing_has_the_default_layers_and_styles() {
        for t in [Template::Metric, Template::Imperial] {
            let d = new_drawing(t);
            let names: Vec<&str> = d.layers().iter().map(|(_, l)| l.name.as_str()).collect();
            assert_eq!(
                names,
                ["0", "VISIBLE", "CENTER", "HIDDEN", "DIMENSION", "NOTES"]
            );
            for spec in &DEFAULT_LAYERS {
                let (_, l) = d.layer_by_name(spec.name).unwrap();
                assert_eq!(l.color, Color::Index(spec.aci));
                assert_eq!(l.weight(), spec.lineweight);
                assert_eq!(d.record_name(l.linetype), Some(spec.linetype));
                assert!(l.plot && !l.off && !l.frozen && !l.locked);
            }
            assert_eq!(
                d.current_layer()
                    .and_then(|h| d.layer(h))
                    .map(|l| l.name.as_str()),
                Some("VISIBLE")
            );
            let unit = t.unit();
            let center = d.linetype(d.linetype_by_name("CENTER").unwrap()).unwrap();
            assert_eq!(center.dashes(), CENTER.pattern(unit));
            assert_eq!(Unit::of(&d), unit);
            let ds = d.dim_style(d.header.dimstyle).unwrap();
            assert_eq!(ds.text_height, text_height(unit));
            assert_eq!(d.text_style(ds.text_style).unwrap().font_file, FONT_FILE);
            assert!(d.model_space_entities().is_empty());
            // It saves and reads back exactly.
            let bytes = crate::native::to_bytes(&d, &Default::default());
            let back = crate::native::read_bytes(&bytes).unwrap();
            assert_eq!(back.objects, d.objects);
            assert_eq!(back.header, d.header);
        }
    }

    #[test]
    fn patterns_follow_the_drawing_unit() {
        assert_eq!(
            CENTER.pattern(Unit::Millimeter),
            vec![12.0, -1.5, 3.0, -1.5]
        );
        assert_eq!(
            CENTER.pattern(Unit::Inch),
            vec![0.5, -0.0625, 0.125, -0.0625]
        );
        assert_eq!(
            HIDDEN.pattern(Unit::Foot),
            vec![0.125 / 12.0, -0.0625 / 12.0]
        );
        assert_eq!(HIDDEN.pattern(Unit::Meter), vec![0.003, -0.0015]);
        assert_eq!(HIDDEN.pattern(Unit::Unitless), vec![3.0, -1.5]);
        assert_eq!(
            pattern_by_name("center", Unit::Millimeter),
            Some(CENTER.pattern(Unit::Millimeter))
        );
        assert_eq!(
            pattern_by_name("HIDDEN2", Unit::Millimeter),
            Some(vec![1.5, -0.75])
        );
        assert_eq!(
            pattern_by_name("CENTERX2", Unit::Inch),
            Some(vec![1.0, -0.125, 0.25, -0.125])
        );
        assert_eq!(
            pattern_by_name("PHANTOM", Unit::Inch),
            Some(vec![0.5, -0.0625, 0.125, -0.0625, 0.125, -0.0625])
        );
        assert_eq!(pattern_by_name("CENTER3", Unit::Inch), None);
        assert_eq!(pattern_by_name("ZIGZAG", Unit::Inch), None);
        let (name, desc, pattern) = library_linetype("dashdotx2", Unit::Millimeter).unwrap();
        assert_eq!((name.as_str(), desc.as_str()), ("DASHDOTX2", "Dash dot (2x) __ . __ . __ ."));
        assert_eq!(pattern, vec![12.0, -3.0, 0.0, -3.0]);
        assert_eq!(library_names().len(), 24);
    }

    #[test]
    fn every_layer_can_be_changed_and_layer_0_keeps_its_name() {
        let mut d = new_drawing(Template::Metric);
        let (hidden, _) = d.layer_by_name("HIDDEN").unwrap();
        d.set_layer_lineweight(hidden, Lineweight::Hundredths(35))
            .unwrap();
        d.set_layer_color(hidden, Color::Rgb(200, 30, 30)).unwrap();
        let center = d.linetype_by_name("CENTER").unwrap();
        d.set_layer_linetype(hidden, center).unwrap();
        d.rename_layer(hidden, "HIDDEN-EDGES").unwrap();
        let l = d.layer(hidden).unwrap();
        assert_eq!(l.weight(), Lineweight::Hundredths(35));
        assert_eq!(l.flags & 0x03E0, (l.lineweight as u16) << 5);
        assert_eq!(
            l.flags & 0x1B,
            0x10,
            "plotting, not off (0x02), frozen or locked"
        );
        assert_eq!(l.color, Color::Rgb(200, 30, 30));
        assert_eq!(l.name, "HIDDEN-EDGES");

        let (zero, _) = d.layer_by_name("0").unwrap();
        assert_eq!(d.rename_layer(zero, "BASE"), Err(EditError::LayerZero));
        assert_eq!(d.delete_layer(zero), Err(EditError::LayerZero));
        // Layer 0's properties can change.
        d.set_layer_color(zero, Color::Index(8)).unwrap();
        d.set_layer_lineweight(zero, Lineweight::Hundredths(18))
            .unwrap();

        // Names are unique without regard to case, and follow DWG's rules.
        assert!(matches!(
            d.add_layer(
                "visible",
                Color::Index(1),
                Handle::NULL,
                Lineweight::Default
            ),
            Err(EditError::NameInUse(_))
        ));
        for bad in ["", " X", "A/B", "A:B", "A*B", "TAB\t"] {
            assert!(matches!(
                d.add_layer(bad, Color::Index(1), Handle::NULL, Lineweight::Default),
                Err(EditError::BadName(_))
            ));
        }
        assert!(matches!(
            d.add_layer("WELD", Color::ByLayer, Handle::NULL, Lineweight::Default),
            Err(EditError::BadValue(_))
        ));
        assert!(matches!(
            d.set_layer_lineweight(hidden, Lineweight::ByLayer),
            Err(EditError::BadValue(_))
        ));
        let by_layer = d.linetype_by_name("ByLayer").unwrap();
        assert!(matches!(
            d.set_layer_linetype(hidden, by_layer),
            Err(EditError::BadValue(_))
        ));

        // A custom layer, an entity on it, and the rules for deleting.
        let weld = d
            .add_layer(
                "Weld symbols",
                Color::Index(6),
                Handle::NULL,
                Lineweight::Hundredths(35),
            )
            .unwrap();
        assert_eq!(d.layers().last().map(|(h, _)| *h), Some(weld));
        let line = d
            .add_entity(
                weld,
                ObjectKind::Line(Line::new(Vec2::ZERO, Vec2::new(10.0, 0.0))),
            )
            .unwrap();
        assert_eq!(d.model_space_entities().len(), 1);
        assert_eq!(
            d.delete_layer(weld),
            Err(EditError::InUse {
                name: "Weld symbols".into(),
                count: 1
            })
        );
        d.set_current_layer(weld).unwrap();
        assert_eq!(
            d.set_layer_state(
                weld,
                LayerState {
                    on: true,
                    frozen: true,
                    locked: false,
                    plot: true
                }
            ),
            Err(EditError::CurrentLayer)
        );
        let (visible, _) = d.layer_by_name("VISIBLE").unwrap();
        d.set_current_layer(visible).unwrap();
        d.objects.remove(&line);
        d.rebuild_index();
        d.delete_layer(weld).unwrap();
        assert!(d.layer_by_name("Weld symbols").is_none());
        assert!(d.layers().iter().all(|(h, _)| *h != weld));
        // The handle seed moved past everything handed out.
        assert!(d.header.handseed.unwrap() > weld.0);
        let h = d.new_handle();
        assert!(!d.objects.contains_key(&h));
    }

    #[test]
    fn entities_take_their_look_from_the_layer() {
        let mut d = new_drawing(Template::Imperial);
        let h = d
            .add_entity(
                Handle::NULL,
                ObjectKind::Circle(Circle::new(Vec2::ZERO, 0.25)),
            )
            .unwrap();
        let o = d.get(h).unwrap();
        let e = o.entity.as_ref().unwrap();
        assert_eq!(o.type_name, "CIRCLE");
        assert_eq!(e.space, Space::Model);
        assert_eq!(d.layer(e.layer).unwrap().name, "VISIBLE");
        assert_eq!(e.color, Color::ByLayer);
        assert_eq!(e.linetype, LinetypeRef::ByLayer);
        assert_eq!(e.weight(), Lineweight::ByLayer);
        // A hatch is not created this way yet.
        assert!(matches!(
            d.add_entity(Handle::NULL, ObjectKind::SeqEnd),
            Err(EditError::Unsupported(_))
        ));
        // Typed input in the drawing's unit.
        assert_eq!(parse_length("1'-2 1/2\"", Unit::of(&d)).unwrap(), 14.5);
        // New entities take the current linetype scale.
        d.header.celtscale = Some(0.5);
        let h = d
            .add_entity(
                Handle::NULL,
                ObjectKind::Line(Line::new(Vec2::ZERO, Vec2::new(1.0, 0.0))),
            )
            .unwrap();
        assert_eq!(
            d.get(h).unwrap().entity.as_ref().unwrap().linetype_scale,
            0.5
        );
        // The layer list counts entities, not the block markers on layer 0.
        let (zero, _) = d.layer_by_name("0").unwrap();
        assert!(d.layer_use_count(zero) > 0);
        assert_eq!(d.layer_entity_count(zero), 0);
        let (visible, _) = d.layer_by_name("VISIBLE").unwrap();
        assert_eq!(d.layer_entity_count(visible), 2);
    }

    #[test]
    fn handles_stay_unique_at_the_top_of_the_range() {
        // A damaged seed at the very top: handles still never repeat.
        let mut d = new_drawing(Template::Metric);
        d.header.handseed = Some(u64::MAX);
        let mut made = Vec::new();
        for name in ["A", "B", "C"] {
            made.push(
                d.add_layer(name, Color::Index(1), Handle::NULL, Lineweight::Default)
                    .unwrap(),
            );
        }
        made.sort();
        made.dedup();
        assert_eq!(made.len(), 3);
        for name in ["A", "B", "C"] {
            assert!(d.layer_by_name(name).is_some(), "{name}");
        }
    }

    #[test]
    fn alternate_units_follow_the_drawing_unit() {
        assert_eq!(dim_style(Unit::Inch).alt_factor, 25.4);
        assert_eq!(dim_style(Unit::Foot).alt_factor, 304.8);
        assert!((dim_style(Unit::Meter).alt_factor - 1000.0 / 25.4).abs() < 1e-12);
        for u in [Unit::Millimeter, Unit::Unitless] {
            assert!((dim_style(u).alt_factor - 1.0 / 25.4).abs() < 1e-15, "{u}");
        }
    }

    #[test]
    fn colours_on_screen_and_on_paper() {
        assert_eq!(screen_color(Color::Index(3)), (0x3D, 0xDC, 0x84));
        assert_eq!(screen_color(Color::Index(7)), (255, 255, 255));
        assert_eq!(screen_color(Color::Index(5)), (0, 0, 255));
        assert_eq!(screen_color(Color::Rgb(1, 2, 3)), (1, 2, 3));
        assert_eq!(print_color(Color::Index(7), true), (0, 0, 0));
        assert_eq!(print_color(Color::Index(1), true), (255, 0, 0));
        assert_eq!(print_color(Color::Index(1), false), (0, 0, 0));
        for spec in &DEFAULT_LAYERS {
            assert_eq!(
                screen_color(Color::Index(spec.aci)),
                spec.screen,
                "{}",
                spec.name
            );
        }
        assert_eq!(screen_px(0.25), 1.0);
        assert_eq!(screen_px(0.35), 1.0);
        assert_eq!(screen_px(0.50), 2.0);
        assert_eq!(screen_px(0.60), 2.0);
        assert_eq!(screen_px(1.00), 3.0);
    }
}

/// The code checked against `docs/DESIGN.md`: these tests read its tables
/// and fail when the two disagree.
#[cfg(test)]
mod design_doc {
    use super::*;

    const DESIGN: &str = include_str!("../../../docs/DESIGN.md");

    /// A section's text, from its heading to the next one.
    fn section(n: u32) -> &'static str {
        let head = format!("\n## {n}. ");
        let start = DESIGN
            .find(&head)
            .unwrap_or_else(|| panic!("DESIGN.md has no section {n}"));
        let rest = &DESIGN[start + head.len()..];
        &rest[..rest.find("\n## ").unwrap_or(rest.len())]
    }

    /// The rows of the first table in `text`, cells trimmed, without the
    /// header and divider rows.
    fn table(text: &str) -> Vec<Vec<String>> {
        let mut rows = Vec::new();
        let mut seen_header = false;
        for line in text.lines().map(str::trim) {
            if !line.starts_with('|') {
                if seen_header {
                    break;
                }
                continue;
            }
            let cells: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect();
            if !seen_header {
                seen_header = true;
            } else if !cells
                .iter()
                .all(|c| c.chars().all(|ch| ch == '-' || ch == ':'))
            {
                rows.push(cells);
            }
        }
        rows
    }

    fn row<'a>(rows: &'a [Vec<String>], starts: &str) -> &'a [String] {
        rows.iter()
            .find(|r| r[0].starts_with(starts))
            .unwrap_or_else(|| panic!("no row {starts:?} in {rows:?}"))
    }

    /// Every `#RRGGBB` in a cell, in order.
    fn hexes(cell: &str) -> Vec<Rgb> {
        cell.match_indices('#')
            .filter_map(|(i, _)| {
                let h = cell.get(i + 1..i + 7)?;
                let v = u32::from_str_radix(h, 16).ok()?;
                Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
            })
            .collect()
    }

    /// Every number in a text, in order (`1.5`, `0.0625`, `3`), not
    /// counting colour codes.
    fn numbers(text: &str) -> Vec<f64> {
        let mut plain = text.to_string();
        while let Some(i) = plain.find('#') {
            let end = (i + 7).min(plain.len());
            plain.replace_range(i..end, " ");
        }
        let mut out = Vec::new();
        let mut cur = String::new();
        for c in plain.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_digit() || (c == '.' && !cur.is_empty()) {
                cur.push(c);
            } else if !cur.is_empty() {
                out.push(cur.trim_end_matches('.').parse().unwrap_or(f64::NAN));
                cur.clear();
            }
        }
        out
    }

    #[test]
    fn default_layers_match_section_3() {
        let rows = table(section(3));
        assert_eq!(rows.len(), DEFAULT_LAYERS.len(), "{rows:?}");
        for (r, spec) in rows.iter().zip(&DEFAULT_LAYERS) {
            assert_eq!(r[0].trim_matches('`'), spec.name);
            assert_eq!(numbers(&r[1])[0], spec.aci as f64, "{} colour", spec.name);
            // "7, white": the name must be that index's colour too.
            const NAMES: [&str; 8] = [
                "", "red", "yellow", "green", "cyan", "blue", "magenta", "white",
            ];
            let name = r[1].split(',').nth(1).map(str::trim).unwrap_or("");
            assert_eq!(
                Some(name),
                NAMES.get(spec.aci as usize).copied(),
                "{} colour name",
                spec.name
            );
            assert_eq!(
                hexes(&r[2]),
                vec![spec.screen],
                "{} screen colour",
                spec.name
            );
            let linetype = r[3].split_whitespace().next().unwrap_or("");
            assert!(
                linetype.eq_ignore_ascii_case(spec.linetype),
                "{} linetype {linetype} vs {}",
                spec.name,
                spec.linetype
            );
            // Exact hundredths of a millimetre: no rounding to the nearest
            // standard weight, so 0.45 never passes for 0.50.
            let weight = if r[4].starts_with("Default") {
                Lineweight::Default
            } else {
                let hundredths = numbers(&r[4])[0] * 100.0;
                assert!(
                    (hundredths - hundredths.round()).abs() < 1e-9,
                    "{} lineweight {}",
                    spec.name,
                    r[4]
                );
                Lineweight::Hundredths(hundredths.round() as u16)
            };
            assert_eq!(weight, spec.lineweight, "{} lineweight", spec.name);
        }
    }

    #[test]
    fn linetypes_match_section_4() {
        let text = section(4);
        let rows = table(text);
        for spec in &LINETYPES {
            let r = rows
                .iter()
                .find(|r| r[0].eq_ignore_ascii_case(spec.name))
                .unwrap_or_else(|| panic!("no row for {}", spec.name));
            // "long 12, gap 1.5, short 3, gap 1.5": gaps are negative.
            let pattern: Vec<f64> = r[1]
                .split(',')
                .map(|part| {
                    let v = numbers(part)[0];
                    if part.contains("gap") {
                        -v
                    } else {
                        v
                    }
                })
                .collect();
            assert_eq!(pattern, spec.mm, "{} in mm", spec.name);
            // "Inch drawings use the same proportions in inches: Center
            // 0.5, 0.0625, 0.125, 0.0625; Hidden 0.125, 0.0625."
            let inch_line = text
                .split("Inch drawings")
                .nth(1)
                .expect("inch sentence")
                .split("\n\n")
                .next()
                .unwrap_or("");
            let name = format!("{}{} ", &spec.name[..1], spec.name[1..].to_lowercase());
            let after = inch_line
                .split(&name)
                .nth(1)
                .expect("linetype in inch sentence");
            // Up to the next linetype ("; "); '.' is also a decimal point.
            let values = numbers(after.split("; ").next().unwrap_or(after));
            let inch: Vec<f64> = spec.inch.iter().map(|v| v.abs()).collect();
            assert_eq!(values, inch, "{} in inches", spec.name);
        }
    }

    #[test]
    fn canvas_and_interaction_colours_match_sections_2_5_and_9() {
        let rows = table(section(2));
        assert_eq!(
            hexes(&row(&rows, "Background")[1]),
            vec![canvas::BACKGROUND]
        );
        let grid = &row(&rows, "Grid")[1];
        assert_eq!(hexes(grid), vec![canvas::GRID_MINOR, canvas::GRID_MAJOR]);
        assert_eq!(
            numbers(grid)[..2],
            [canvas::GRID_SPACING, canvas::GRID_MAJOR_EVERY as f64]
        );

        let rows = table(section(5));
        let selection = &row(&rows, "Selection");
        assert_eq!(hexes(&selection[1]), vec![interaction::SELECTION]);
        assert!(selection[2].contains(&format!("({} px)", interaction::GRIP_PX)));
        assert!(row(&rows, "Hover")[1].contains(&format!(
            "{}%",
            (interaction::HOVER_OPACITY * 100.0).round()
        )));
        assert_eq!(hexes(&row(&rows, "Snap")[1]), vec![interaction::SNAP]);
        assert_eq!(
            hexes(&row(&rows, "Interface accent")[1]),
            vec![interaction::ACCENT]
        );

        let aids = section(9);
        assert!(aids.contains(&format!(
            "{} px with a mouse, {} px on\n  touch",
            interaction::APERTURE_MOUSE_PX,
            interaction::APERTURE_TOUCH_PX
        )));
    }

    #[test]
    fn text_and_dimension_styles_match_sections_6_and_7() {
        assert!(section(6).contains("(3.5 mm or 1/8 in are typical)"));
        assert_eq!(text_height(Unit::Millimeter), 3.5);
        assert_eq!(text_height(Unit::Inch), 0.125);

        let rows = table(section(7));
        let mm = dim_style(Unit::Millimeter);
        let inch = dim_style(Unit::Inch);
        let check = |label: &str, f: &dyn Fn(&DimStyle) -> f64| {
            let r = row(&rows, label);
            assert_eq!(numbers(&r[1])[0], f(&mm), "{label}, mm");
            assert_eq!(numbers(&r[2])[0], f(&inch), "{label}, inch");
        };
        check("Text height", &|s| s.text_height);
        check("Arrowheads", &|s| s.arrow_size);
        check("Extension line gap", &|s| s.ext_offset);
        check("Extension line past", &|s| s.ext_extension);
        check("Spacing", &|s| s.baseline_spacing);
        check("Gap around text", &|s| s.text_gap);
        check("Decimal places", &|s| s.decimals as f64);
        assert!(row(&rows, "Arrowheads")[1].contains(&format!("{ARROW_LENGTH_TO_WIDTH}:1")));
        // Dual units as the table shows them.
        let fmt = |s: &DimStyle| crate::units::DimTextFormat {
            alternate: Some((s.alt_factor, s.alt_decimals)),
            ..crate::units::DimTextFormat::from_style(s)
        };
        let dual = row(&rows, "Dual units");
        let mm_text = crate::units::dimension_text(25.4, "", "", &fmt(&mm), Unit::Millimeter);
        let in_text = crate::units::dimension_text(1.0, "", "", &fmt(&inch), Unit::Inch);
        assert!(dual[1].contains(&format!("`{mm_text}`")), "{mm_text}");
        assert!(dual[2].contains(&format!("`{in_text}`")), "{in_text}");
    }
}

//! A drawing sheet ready to plot: a layout on a paper size, a border
//! inside the paper's margin, a title block whose fields are attributes,
//! and a viewport showing the model at the largest standard scale that
//! fits above the title block.
//!
//! The title block is a block (`SHEET TITLE`), made the first time and
//! inserted on each sheet with the sheet's values, so ATTEDIT or the
//! Properties panel changes a field later. Each field is a box with its
//! label in small text and its value as an attribute; the attribute's tag
//! is the label in capitals with `_` for anything but letters and digits
//! (`PART NO.` → `PART_NO`). A field labelled SCALE gets the viewport's
//! scale.
//!
//! Sizes on paper: a 10 mm margin, a 170 × 40 mm title block, 2 mm labels
//! and 3.5 mm values; on an inch sheet ½ in, 6½ × 1½ in, 0.08 in and ⅛ in.
//! The border and title block go on layer `BORDER`; the viewport on
//! `VIEWPORTS`, which does not plot, so the printed sheet shows the border
//! and not the viewport's frame.

use crate::blocks;
use crate::geom::{BBox, Vec2, Vec3};
use crate::layouts::{self, sheets};
use crate::model::*;
use crate::standards;
use crate::units::Unit;

type R<T> = Result<T, String>;

fn err(e: EditError) -> String {
    e.to_string()
}

/// The title block's name.
pub const TITLE_BLOCK: &str = "SHEET TITLE";

/// The fields a sheet's title block has when none are given.
pub const FIELDS: [&str; 9] = ["PART NAME", "PART NO.", "MATERIAL", "UNITS", "SCALE", "TOLERANCE", "DRAWN", "DATE", "REV"];

/// Sizes on paper, in paper units.
struct Sizes {
    margin: f64,
    gap: f64,
    width: f64,
    row: f64,
    label: f64,
    value: f64,
    pad: f64,
}

fn sizes(unit: Unit) -> Sizes {
    if unit.is_imperial() {
        Sizes { margin: 0.5, gap: 0.25, width: 6.5, row: 0.5, label: 0.08, value: 0.125, pad: 0.08 }
    } else {
        Sizes { margin: 10.0, gap: 5.0, width: 170.0, row: 40.0 / 3.0, label: 2.0, value: 3.5, pad: 2.0 }
    }
}

/// A field's attribute tag: its label in capitals, `_` for the rest.
pub fn tag_of(label: &str) -> String {
    let t: String = label.trim().to_ascii_uppercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let t = t.trim_matches('_').to_string();
    let mut out = String::new();
    for c in t.chars() {
        if c == '_' && out.ends_with('_') {
            continue;
        }
        out.push(c);
    }
    out
}

/// Standard scales, paper units per model unit, largest first: ISO 5455
/// for metric drawings, halves for inch drawings.
pub fn standard_scales(unit: Unit) -> Vec<(f64, String)> {
    let (up, down): (&[u32], &[u32]) = if unit.is_imperial() {
        (&[4, 2], &[1, 2, 4, 8, 16, 32, 48, 96, 192])
    } else {
        (&[10, 5, 2], &[1, 2, 5, 10, 20, 50, 100, 200, 500, 1000])
    };
    up.iter().map(|k| (*k as f64, format!("{k}:1"))).chain(down.iter().map(|k| (1.0 / *k as f64, format!("1:{k}")))).collect()
}

/// The scale a model `w` × `h` is shown at in an area `aw` × `ah` (paper
/// units): the largest reduction that fits, or full size; enlarged only
/// when full size would leave it a speck in the area.
pub fn fit_scale(w: f64, h: f64, aw: f64, ah: f64, unit: Unit) -> (f64, String) {
    let list = standard_scales(unit);
    let frac = |k: f64| (w * k / aw).max(h * k / ah);
    if w <= 0.0 && h <= 0.0 {
        return (1.0, "1:1".into());
    }
    if frac(1.0) < 0.2 {
        if let Some(s) = list.iter().find(|(k, _)| *k > 1.0 && frac(*k) <= 0.6) {
            return s.clone();
        }
    }
    list.iter().find(|(k, _)| *k <= 1.0 && frac(*k) <= 1.0).cloned().unwrap_or_else(|| list.last().cloned().expect("listed"))
}

/// A layer by name, made with `color` when the drawing has none.
fn layer(d: &mut Drawing, name: &str, color: u8, plot: bool) -> R<Handle> {
    if let Some((h, _)) = d.layer_by_name(name) {
        return Ok(h);
    }
    let continuous = d.linetype_by_name(standards::CONTINUOUS).unwrap_or(Handle::NULL);
    let h = d.add_layer(name, Color::Index(color), continuous, Lineweight::Default).map_err(err)?;
    if !plot {
        let mut s = d.layer_state(h).ok_or("no layer")?;
        s.plot = false;
        d.set_layer_state(h, s).map_err(err)?;
    }
    Ok(h)
}

/// The title block, made the first time it is needed.
fn title_block(d: &mut Drawing, labels: &[String], z: &Sizes) -> R<Handle> {
    if let Some(b) = blocks::find(d, TITLE_BLOCK) {
        return Ok(b);
    }
    let layer0 = d.ensure_layer_zero();
    let block = d.new_block_record(TITLE_BLOCK, Vec3::ZERO).map_err(err)?;
    let rows = labels.len().div_ceil(3).max(1);
    let (w, h, cw) = (z.width, z.row * rows as f64, z.width / 3.0);
    let style = d.header.textstyle;
    let add = |d: &mut Drawing, kind: ObjectKind| d.add_entity_in(block, layer0, kind).map_err(err);
    add(d, ObjectKind::LwPolyline(LwPolyline::new(vec![Vec2::ZERO, Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)], true)))?;
    for i in 1..3 {
        add(d, ObjectKind::Line(Line::new(Vec2::new(cw * i as f64, 0.0), Vec2::new(cw * i as f64, h))))?;
    }
    for j in 1..rows {
        add(d, ObjectKind::Line(Line::new(Vec2::new(0.0, z.row * j as f64), Vec2::new(w, z.row * j as f64))))?;
    }
    for (i, l) in labels.iter().enumerate() {
        // Rows from the top, left to right.
        let x = cw * (i % 3) as f64 + z.pad;
        let y = h - z.row * (i / 3 + 1) as f64;
        let mut t = Text::new(Vec2::new(x, y + z.row * 0.62), z.label, l);
        t.style = style;
        add(d, ObjectKind::Text(t))?;
        let a = blocks::attdef(d, block, &tag_of(l), l, "", Vec2::new(x, y + z.row * 0.18), z.value, 0.0)?;
        if let Some(e) = d.objects.get_mut(&a).and_then(|o| o.entity.as_mut()) {
            e.layer = layer0;
        }
    }
    Ok(block)
}

/// What [`new_sheet`] made.
#[derive(Debug, Clone, PartialEq)]
pub struct MadeSheet {
    pub name: String,
    /// The layout's block, where its objects are.
    pub block: Handle,
    pub title: Handle,
    pub viewport: Handle,
    /// The viewport's scale as written: "1:5".
    pub scale: String,
}

/// A new sheet named `name` (the next free "Sheet N" when empty) on
/// `paper` (the drawing's default when empty), its title block filled in
/// from `fields` (label, value), in order; the standard fields, empty,
/// when none are given.
pub fn new_sheet(d: &mut Drawing, name: &str, paper: &str, fields: &[(String, String)]) -> R<MadeSheet> {
    let unit = Unit::of(d);
    let z = sizes(unit);
    let name = if name.trim().is_empty() {
        (1..).map(|n| format!("Sheet {n}")).find(|n| layouts::sheet(d, n).is_none()).expect("a free name")
    } else {
        name.trim().to_string()
    };
    let fields: Vec<(String, String)> = if fields.is_empty() { FIELDS.iter().map(|l| (l.to_string(), String::new())).collect() } else { fields.to_vec() };
    if let Some((l, _)) = fields.iter().find(|(l, _)| tag_of(l).is_empty()) {
        return Err(format!("{l:?} cannot be a field's label"));
    }
    let block = layouts::new_layout(d, &name, paper)?;
    let (pw, ph) = sheets(d).into_iter().find(|s| s.block == block).and_then(|s| s.paper).ok_or("the new layout has no paper")?;
    let labels: Vec<String> = fields.iter().map(|(l, _)| l.trim().to_string()).collect();
    let rows = labels.len().div_ceil(3).max(1);
    let tb_h = z.row * rows as f64;
    if pw < z.width + 2.0 * z.margin || ph < tb_h + 2.0 * z.margin + 2.0 * z.gap {
        return Err("the paper is too small for the title block".into());
    }
    let border = layer(d, "BORDER", 7, true)?;
    let viewports = layer(d, "VIEWPORTS", 8, false)?;
    // The border.
    let m = z.margin;
    let frame = LwPolyline::new(vec![Vec2::new(m, m), Vec2::new(pw - m, m), Vec2::new(pw - m, ph - m), Vec2::new(m, ph - m)], true);
    d.add_entity_in(block, border, ObjectKind::LwPolyline(frame)).map_err(err)?;
    // The viewport: the model at the scale that fits above the title block.
    let area = BBox { min: Vec2::new(m + z.gap, m + tb_h + z.gap), max: Vec2::new(pw - m - z.gap, ph - m - z.gap) };
    let ext = crate::render::model_space(d).bounds;
    let (k, scale) = if ext.is_empty() {
        (1.0, "1:1".to_string())
    } else {
        fit_scale(ext.width(), ext.height(), area.width(), area.height(), unit)
    };
    let center = if ext.is_empty() { None } else { Some(ext.center()) };
    let viewport = layouts::add_viewport(d, block, area.min, area.max, Some(k), center)?;
    if let Some(e) = d.objects.get_mut(&viewport).and_then(|o| o.entity.as_mut()) {
        e.layer = viewports;
    }
    // The title block in the lower right corner.
    let tb = title_block(d, &labels, &z)?;
    let known = blocks::attribute_definitions(d, tb);
    let values: Vec<(String, String)> = fields
        .iter()
        .map(|(l, v)| {
            let tag = tag_of(l);
            let v = if tag == "SCALE" { scale.clone() } else { v.trim().to_string() };
            (tag, v)
        })
        .filter(|(t, _)| known.iter().any(|(k, _, _)| k.eq_ignore_ascii_case(t)))
        .collect();
    let title = blocks::insert(d, block, tb, Vec2::new(pw - m - z.width, m), 1.0, 0.0, &values)?;
    let parts: Vec<Handle> = std::iter::once(title).chain(blocks::attributes(d, title).into_iter().map(|(h, _, _)| h)).collect();
    for h in parts {
        if let Some(e) = d.objects.get_mut(&h).and_then(|o| o.entity.as_mut()) {
            e.layer = border;
        }
    }
    Ok(MadeSheet { name, block, title, viewport, scale })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standards::{new_drawing, Template};

    #[test]
    fn scales_fit_the_area() {
        let mm = Unit::Millimeter;
        assert_eq!(fit_scale(300.0, 100.0, 390.0, 220.0, mm).1, "1:1");
        assert_eq!(fit_scale(1200.0, 300.0, 390.0, 220.0, mm).1, "1:5");
        assert_eq!(fit_scale(20.0, 10.0, 390.0, 220.0, mm).1, "10:1");
        assert_eq!(fit_scale(40.0, 10.0, 390.0, 220.0, mm).1, "5:1");
        assert_eq!(fit_scale(9.0, 6.0, 15.0, 9.0, Unit::Inch).1, "1:1");
        assert_eq!(fit_scale(30.0, 6.0, 15.0, 9.0, Unit::Inch).1, "1:2");
        assert_eq!(tag_of("PART NO."), "PART_NO");
        assert_eq!(tag_of(" drawn  by "), "DRAWN_BY");
        assert_eq!(tag_of("--"), "");
    }

    #[test]
    fn a_sheet_with_border_title_block_and_viewport() {
        let mut d = new_drawing(Template::Metric);
        d.add_entity(Handle::NULL, ObjectKind::LwPolyline(LwPolyline::new(vec![Vec2::ZERO, Vec2::new(900.0, 0.0), Vec2::new(900.0, 300.0), Vec2::new(0.0, 300.0)], true)))
            .unwrap();
        let fields: Vec<(String, String)> = [("PART NAME", "BRACKET"), ("PART NO.", "P-1"), ("SCALE", "")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let s = new_sheet(&mut d, "", "A3", &fields).unwrap();
        assert_eq!((s.name.as_str(), s.scale.as_str()), ("Sheet 1", "1:5"));
        let vals = blocks::attributes(&d, s.title);
        assert!(vals.iter().any(|(_, t, v)| t == "PART_NO" && v == "P-1"));
        assert!(vals.iter().any(|(_, t, v)| t == "SCALE" && v == "1:5"));
        // The viewport is on a layer that does not plot.
        let vl = d.get(s.viewport).unwrap().entity.as_ref().unwrap().layer;
        assert!(!d.layer(vl).unwrap().plot);
        // A second sheet reuses the title block and takes the next name.
        let t = new_sheet(&mut d, "", "", &[]).unwrap();
        assert_eq!(t.name, "Sheet 2");
        assert_eq!(d.objects.values().filter(|o| matches!(&o.kind, ObjectKind::BlockHeader(b) if b.name == TITLE_BLOCK)).count(), 1);
        // On the layout, the part shows at a fifth of its size.
        let scene = crate::render::layout(&d, s.block);
        let plotted = crate::render::plotted(&d, &scene);
        assert!(plotted.items.len() < scene.items.len());
        // Written and read back, the sheet is there.
        for bytes in [crate::dwg::write::to_bytes(&d).unwrap().bytes, crate::dxf::to_bytes(&d, crate::dxf::DxfFormat::Ascii).bytes] {
            let back = crate::open_bytes(&bytes).unwrap();
            let sh = layouts::sheet(&back, "Sheet 1").unwrap();
            let scene = crate::render::layout(&back, sh.block);
            assert!(scene.items.iter().any(|i| matches!(&i.shape, crate::render::Shape::Text { text, .. } if text == "BRACKET")));
        }
        assert!(new_sheet(&mut d, "Sheet 1", "", &[]).is_err());
    }
}

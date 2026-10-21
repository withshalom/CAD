//! ASCII DXF export (AutoCAD R12 / AC1009 flavour) of a rendered scene.
//!
//! Block references are exploded into model space; arcs, circles and bulge
//! polylines stay true curves. Linetypes are written as LTYPE records with
//! their patterns and named on layers and entities, so other programs draw
//! the dashes themselves; `$LTSCALE` carries the drawing's linetype scale.
//! R12 has no true color, so colors are written as the nearest ACI, and no
//! lineweights or per-entity linetype scales (both came with R13/R2000).
//! Written from general knowledge of the openly published DXF group-code
//! format.

use crate::geom::Vec2;
use crate::model::{Drawing, ObjectKind};
use crate::render::{Scene, Shape};
use crate::standards;
use crate::units::Unit;
use std::collections::BTreeMap;
use std::fmt::Write;

fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0.0".into();
    }
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    }
}

/// R12 layer names allow letters, digits, '$', '-' and '_'.
fn layer_name(s: &str) -> String {
    let n: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '$' || c == '-' || c == '_' {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    if n.is_empty() {
        "0".into()
    } else {
        n
    }
}

/// Non-ASCII characters as AutoCAD \U+XXXX escapes.
fn dxf_text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' | '\r' => o.push(' '),
            c if c.is_ascii() && !c.is_ascii_control() => o.push(c),
            c if (c as u32) <= 0xFFFF => {
                let _ = write!(o, "\\U+{:04X}", c as u32);
            }
            _ => o.push('?'),
        }
    }
    o
}

struct W(String);

/// The writer for one entity: its `common` also names the linetype when
/// it differs from the layer's.
struct Entity<'a> {
    w: &'a mut W,
    linetype: Option<&'a str>,
}

impl Entity<'_> {
    fn pair(&mut self, code: i32, value: impl AsRef<str>) {
        self.w.pair(code, value)
    }
    fn f(&mut self, code: i32, v: f64) {
        self.w.f(code, v)
    }
    fn pt(&mut self, code: i32, p: Vec2) {
        self.w.pt(code, p)
    }
    fn common(&mut self, kind: &str, layer: &str, aci: i16) {
        self.w.common(kind, layer, aci);
        if let Some(lt) = self.linetype {
            self.w.pair(6, lt);
        }
    }
}

impl W {
    fn pair(&mut self, code: i32, value: impl AsRef<str>) {
        let _ = write!(self.0, "{code:>3}\r\n{}\r\n", value.as_ref());
    }
    fn f(&mut self, code: i32, v: f64) {
        self.pair(code, num(v));
    }
    fn pt(&mut self, code: i32, p: Vec2) {
        self.f(code, p.x);
        self.f(code + 10, p.y);
        self.f(code + 20, 0.0);
    }
    fn common(&mut self, kind: &str, layer: &str, aci: i16) {
        self.pair(0, kind);
        self.pair(8, layer);
        if (1..=255).contains(&aci) {
            self.pair(62, aci.to_string());
        }
    }
}

/// Linetype patterns by (sanitised) name: the drawing's own records, the
/// project's patterns for name-only records, and anything the scene uses.
fn linetypes(scene: &Scene, drawing: Option<&Drawing>) -> BTreeMap<String, (String, Vec<f64>)> {
    let mut out: BTreeMap<String, (String, Vec<f64>)> = BTreeMap::new();
    let ltscale = drawing
        .and_then(|d| d.header.ltscale)
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(1.0);
    if let Some(d) = drawing {
        let unit = Unit::of(d);
        for o in d.objects.values() {
            let (name, desc, pattern) = match &o.kind {
                ObjectKind::Linetype(l) => (l.name.as_str(), l.description.clone(), l.dashes()),
                ObjectKind::NamedRecord(r) if o.type_name == "LTYPE" => (
                    r.name.as_str(),
                    String::new(),
                    standards::pattern_by_name(&r.name, unit).unwrap_or_default(),
                ),
                _ => continue,
            };
            if crate::render::dash::is_dashed(&pattern) {
                out.insert(layer_name(name), (desc, pattern));
            }
        }
    }
    for it in &scene.items {
        let s = &it.style;
        if !s.linetype.is_empty() && !s.pattern.is_empty() {
            out.entry(layer_name(&s.linetype)).or_insert_with(|| {
                (
                    String::new(),
                    s.pattern.iter().map(|v| v / ltscale).collect(),
                )
            });
        }
    }
    out
}

/// Write a scene as DXF. `drawing` supplies the layer and linetype tables.
pub fn to_dxf(scene: &Scene, drawing: Option<&Drawing>) -> String {
    let mut w = W(String::new());
    let ltypes = linetypes(scene, drawing);
    let ltype_of = |name: &str| {
        let n = layer_name(name);
        if ltypes.contains_key(&n) {
            n
        } else {
            "CONTINUOUS".to_string()
        }
    };

    // Layers (colour, linetype): from the drawing, plus any the scene uses.
    let mut layers: BTreeMap<String, (i16, String)> = BTreeMap::new();
    layers.insert("0".into(), (7, "CONTINUOUS".into()));
    if let Some(d) = drawing {
        for o in d.objects.values() {
            if let ObjectKind::Layer(l) = &o.kind {
                let mut aci = l.color.to_aci();
                if !(1..=255).contains(&aci) {
                    aci = 7;
                }
                if l.off {
                    aci = -aci;
                }
                let lt = d
                    .record_name(l.linetype)
                    .map(ltype_of)
                    .unwrap_or_else(|| "CONTINUOUS".into());
                layers.insert(layer_name(&l.name), (aci, lt));
            }
        }
    }
    for it in &scene.items {
        layers
            .entry(layer_name(&it.style.layer))
            .or_insert((7, "CONTINUOUS".into()));
    }

    w.pair(
        999,
        "Written by the CAD core (clean-room DWG reader, draft build)",
    );
    w.pair(0, "SECTION");
    w.pair(2, "HEADER");
    w.pair(9, "$ACADVER");
    w.pair(1, "AC1009");
    if !scene.bounds.is_empty() {
        w.pair(9, "$EXTMIN");
        w.pt(10, scene.bounds.min);
        w.pair(9, "$EXTMAX");
        w.pt(10, scene.bounds.max);
    }
    // The scale the dashes were drawn at: a value that is not a positive
    // number draws (and is written) as 1.
    if let Some(v) = drawing.and_then(|d| d.header.ltscale) {
        w.pair(9, "$LTSCALE");
        w.f(40, if v.is_finite() && v > 0.0 { v } else { 1.0 });
    }
    w.pair(0, "ENDSEC");

    w.pair(0, "SECTION");
    w.pair(2, "TABLES");
    w.pair(0, "TABLE");
    w.pair(2, "LTYPE");
    w.pair(70, (ltypes.len() + 1).to_string());
    w.pair(0, "LTYPE");
    w.pair(2, "CONTINUOUS");
    w.pair(70, "0");
    w.pair(3, "Solid line");
    w.pair(72, "65");
    w.pair(73, "0");
    w.f(40, 0.0);
    for (name, (desc, pattern)) in &ltypes {
        w.pair(0, "LTYPE");
        w.pair(2, name);
        w.pair(70, "0");
        w.pair(3, dxf_text(desc));
        w.pair(72, "65");
        w.pair(73, pattern.len().to_string());
        w.f(40, pattern.iter().map(|v| v.abs()).sum());
        for v in pattern {
            w.f(49, *v);
        }
    }
    w.pair(0, "ENDTAB");
    w.pair(0, "TABLE");
    w.pair(2, "LAYER");
    w.pair(70, layers.len().to_string());
    for (name, (aci, lt)) in &layers {
        w.pair(0, "LAYER");
        w.pair(2, name);
        w.pair(70, "0");
        w.pair(62, aci.to_string());
        w.pair(6, lt);
    }
    w.pair(0, "ENDTAB");
    w.pair(0, "ENDSEC");

    w.pair(0, "SECTION");
    w.pair(2, "ENTITIES");
    for it in &scene.items {
        let layer = layer_name(&it.style.layer);
        let aci = it.style.aci;
        // A linetype other than the layer's is named on the entity.
        let own = if it.style.linetype.is_empty() {
            "CONTINUOUS".to_string()
        } else {
            ltype_of(&it.style.linetype)
        };
        let layer_lt = layers.get(&layer).map(|(_, lt)| lt.as_str());
        let lt = (layer_lt != Some(own.as_str())).then_some(own);
        let mut w = Entity {
            w: &mut w,
            linetype: lt.as_deref(),
        };
        let w = &mut w;
        match &it.shape {
            Shape::Line { a, b } => {
                w.common("LINE", &layer, aci);
                w.pt(10, *a);
                w.pt(11, *b);
            }
            Shape::Circle { center, radius } => {
                w.common("CIRCLE", &layer, aci);
                w.pt(10, *center);
                w.f(40, *radius);
            }
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                w.common("ARC", &layer, aci);
                w.pt(10, *center);
                w.f(40, *radius);
                w.f(50, start.to_degrees().rem_euclid(360.0));
                w.f(51, end.to_degrees().rem_euclid(360.0));
            }
            Shape::Polyline { vertices, closed } => {
                w.common("POLYLINE", &layer, aci);
                w.pair(66, "1");
                w.pt(10, Vec2::ZERO);
                w.pair(70, if *closed { "1" } else { "0" });
                for (p, bulge) in vertices {
                    w.common("VERTEX", &layer, aci);
                    w.pt(10, *p);
                    if *bulge != 0.0 {
                        w.f(42, *bulge);
                    }
                }
                w.common("SEQEND", &layer, aci);
            }
            Shape::Lines { segments } => {
                for (a, b) in segments {
                    w.common("LINE", &layer, aci);
                    w.pt(10, *a);
                    w.pt(11, *b);
                }
            }
            // R12 has no construction lines, and a cutting program has no
            // use for infinite ones: left out (reported in the notes).
            Shape::Ray { .. } => {}
            Shape::Path { points, closed } => {
                w.common("POLYLINE", &layer, aci);
                w.pair(66, "1");
                w.pt(10, Vec2::ZERO);
                w.pair(70, if *closed { "1" } else { "0" });
                for p in points {
                    w.common("VERTEX", &layer, aci);
                    w.pt(10, *p);
                }
                w.common("SEQEND", &layer, aci);
            }
            Shape::Fill { rings } => {
                for ring in rings {
                    if ring.len() == 3 || ring.len() == 4 {
                        // SOLID corner order is 1, 2, 4, 3 around the outline.
                        let (c1, c2) = (ring[0], ring[1]);
                        let (c3, c4) = if ring.len() == 4 {
                            (ring[3], ring[2])
                        } else {
                            (ring[2], ring[2])
                        };
                        w.common("SOLID", &layer, aci);
                        w.pt(10, c1);
                        w.pt(11, c2);
                        w.pt(12, c3);
                        w.pt(13, c4);
                    } else if ring.len() > 1 {
                        w.common("POLYLINE", &layer, aci);
                        w.pair(66, "1");
                        w.pt(10, Vec2::ZERO);
                        w.pair(70, "1");
                        for p in ring {
                            w.common("VERTEX", &layer, aci);
                            w.pt(10, *p);
                        }
                        w.common("SEQEND", &layer, aci);
                    }
                }
            }
            Shape::Point { at } => {
                w.common("POINT", &layer, aci);
                w.pt(10, *at);
            }
            Shape::Text {
                position,
                height,
                rotation,
                width_factor,
                oblique,
                text,
                h_align,
                v_align,
                fit_to,
            } => {
                w.common("TEXT", &layer, aci);
                w.pt(10, *position);
                w.f(40, *height);
                w.pair(1, dxf_text(text));
                if rotation.abs() > 1e-12 {
                    w.f(50, rotation.to_degrees().rem_euclid(360.0));
                }
                if (*width_factor - 1.0).abs() > 1e-12 {
                    w.f(41, *width_factor);
                }
                if oblique.abs() > 1e-12 {
                    w.f(51, oblique.to_degrees().rem_euclid(360.0));
                }
                if *h_align != 0 || *v_align != 0 {
                    w.pair(72, h_align.to_string());
                    w.pt(11, fit_to.unwrap_or(*position));
                    w.pair(73, v_align.to_string());
                }
            }
        }
    }
    w.pair(0, "ENDSEC");
    w.pair(0, "EOF");
    w.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Line, ObjectKind};
    use crate::standards::{new_drawing, Template};

    #[test]
    fn linetypes_are_written_with_their_patterns() {
        let mut d = new_drawing(Template::Metric);
        let hidden = d.layer_by_name("HIDDEN").unwrap().0;
        let visible = d.layer_by_name("VISIBLE").unwrap().0;
        let line = |y: f64| ObjectKind::Line(Line::new(Vec2::new(0.0, y), Vec2::new(50.0, y)));
        d.add_entity(hidden, line(0.0)).unwrap();
        let odd = d.add_entity(visible, line(10.0)).unwrap();
        let center = d.linetype_by_name("CENTER").unwrap();
        d.objects
            .get_mut(&odd)
            .unwrap()
            .entity
            .as_mut()
            .unwrap()
            .linetype = crate::model::LinetypeRef::Handle(center);
        let scene = crate::render::model_space(&d);
        let dxf = to_dxf(&scene, Some(&d));
        let pairs: Vec<(&str, &str)> = dxf
            .split("\r\n")
            .collect::<Vec<_>>()
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| (c[0].trim(), c[1]))
            .collect();
        // LTYPE HIDDEN with its two elements.
        let at = pairs
            .windows(2)
            .position(|w| w[0] == ("0", "LTYPE") && w[1] == ("2", "HIDDEN"))
            .expect("HIDDEN linetype");
        let elems: Vec<&str> = pairs[at..]
            .iter()
            .take_while(|p| p.0 != "0" || p.1 == "LTYPE")
            .filter(|p| p.0 == "49")
            .map(|p| p.1)
            .collect();
        assert_eq!(elems, ["3.0", "-1.5"]);
        // The HIDDEN layer names its linetype; the line on it does not need
        // to, the line with its own linetype does.
        let layer = pairs
            .windows(3)
            .find(|w| w[0] == ("0", "LAYER") && w[1] == ("2", "HIDDEN"))
            .unwrap();
        assert_eq!(layer[2].0, "70");
        assert!(dxf.contains("  2\r\nHIDDEN\r\n 70\r\n0\r\n 62\r\n1\r\n  6\r\nHIDDEN\r\n"));
        let lines: Vec<usize> = pairs
            .iter()
            .enumerate()
            .filter(|(_, p)| **p == ("0", "LINE"))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(!pairs[lines[0]..lines[0] + 4].iter().any(|p| p.0 == "6"));
        assert!(pairs[lines[1]..lines[1] + 4].contains(&("6", "CENTER")));
    }

    #[test]
    fn a_linetype_scale_that_is_not_a_positive_number_is_written_as_1() {
        let mut d = new_drawing(Template::Metric);
        for (v, want) in [(0.5, "0.5"), (0.0, "1.0"), (-2.0, "1.0"), (f64::NAN, "1.0")] {
            d.header.ltscale = Some(v);
            let dxf = to_dxf(&crate::render::model_space(&d), Some(&d));
            assert!(
                dxf.contains(&format!("$LTSCALE\r\n 40\r\n{want}\r\n")),
                "{v}"
            );
        }
    }
}

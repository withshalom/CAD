//! Checking a flat part's holes before it is cut: holes smaller than the
//! process can make, holes too close to each other or to an edge, and
//! holes too close to a bend line (the Blackboard's "Check my drawing").
//! The limits are the caller's (the shop's rules for the material,
//! thickness and process); this module measures the drawing against them.
//!
//! What it reads, from a drawn scene (what is on screen: model space or
//! a layout):
//!
//! - **Holes**: full circles drawn by CIRCLE objects. A circle with other
//!   holes inside it is an outline, not a hole.
//! - **Bends**: what is drawn on a layer named `BEND`.
//! - **Edges**: everything else that cuts: lines, arcs and polylines,
//!   except on layers that explain the part rather than cut it
//!   ([`NOT_CUT_LAYERS`]: dimensions, notes, centre lines, hidden lines,
//!   borders and the like), and except text, dimensions, block references,
//!   hatches, points, solids, leaders, tables, viewports and images.
//!
//! Distances are from a hole's edge: to another hole's edge, to the
//! nearest edge of the part, to the nearest bend line.

use crate::geom::curve::bulge_to_arc;
use crate::geom::Vec2;
use crate::model::*;
use crate::render::{Scene, Shape};
use crate::units::Unit;
use std::collections::BTreeSet;
use std::f64::consts::TAU;

/// Layer names (their beginnings, any case) whose objects do not cut.
pub const NOT_CUT_LAYERS: &[&str] = &["DIM", "NOTE", "TEXT", "TITLE", "BORDER", "DEFPOINTS", "CENTER", "CENTRE", "HIDDEN", "PHANTOM", "HATCH"];

/// Object types (any part of the name) that do not cut.
const NOT_CUT_TYPES: &[&str] = &["TEXT", "DIMENSION", "INSERT", "HATCH", "POINT", "SOLID", "LEADER", "TABLE", "VIEWPORT", "IMAGE"];

/// The smallest sizes allowed, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Smallest hole diameter.
    pub min_diameter: f64,
    /// Least metal between a hole and another hole or an edge.
    pub min_web: f64,
    /// Least distance from a hole's edge to a bend line.
    pub hole_to_bend: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hole {
    pub handle: Handle,
    pub center: Vec2,
    pub radius: f64,
}

/// What the check found. The sets hold the holes' handles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HoleCheck {
    pub holes: Vec<Hole>,
    /// Pieces of edge measured against.
    pub edges: usize,
    /// Bend line pieces.
    pub bends: usize,
    pub too_small: BTreeSet<Handle>,
    pub too_close: BTreeSet<Handle>,
    pub near_edge: BTreeSet<Handle>,
    pub near_bend: BTreeSet<Handle>,
}

impl HoleCheck {
    /// Every hole flagged for any reason.
    pub fn flagged(&self) -> BTreeSet<Handle> {
        let mut all = self.too_small.clone();
        all.extend(&self.too_close);
        all.extend(&self.near_edge);
        all.extend(&self.near_bend);
        all
    }
}

#[derive(Clone, Copy)]
enum Piece {
    Seg(Vec2, Vec2),
    /// Counter-clockwise from `a0` over `sweep`.
    Arc { c: Vec2, r: f64, a0: f64, sweep: f64 },
}

impl Piece {
    fn distance(&self, p: Vec2) -> f64 {
        match *self {
            Piece::Seg(a, b) => {
                let d = b - a;
                let l2 = d.dot(d);
                let t = if l2 == 0.0 { 0.0 } else { ((p - a).dot(d) / l2).clamp(0.0, 1.0) };
                p.distance(a + d * t)
            }
            Piece::Arc { c, r, a0, sweep } => {
                if sweep >= TAU - 1e-12 || (p.distance(c) > 0.0 && ((p - c).angle() - a0).rem_euclid(TAU) <= sweep) {
                    return (p.distance(c) - r).abs();
                }
                let end = |t: f64| p.distance(c + Vec2::from_angle(t) * r);
                end(a0).min(end(a0 + sweep))
            }
        }
    }
}

fn pieces(shape: &Shape, out: &mut Vec<Piece>) {
    match shape {
        Shape::Line { a, b } => out.push(Piece::Seg(*a, *b)),
        Shape::Circle { center, radius } => out.push(Piece::Arc { c: *center, r: *radius, a0: 0.0, sweep: TAU }),
        Shape::Arc { center, radius, start, end } => {
            let sweep = (end - start).rem_euclid(TAU);
            out.push(Piece::Arc { c: *center, r: *radius, a0: *start, sweep: if sweep == 0.0 { TAU } else { sweep } })
        }
        Shape::Polyline { vertices, closed } => {
            let n = vertices.len();
            let segs = if *closed { n } else { n.saturating_sub(1) };
            for i in 0..segs {
                let (p, bulge) = vertices[i];
                let q = vertices[(i + 1) % n].0;
                match bulge_to_arc(p, q, bulge) {
                    Some((a, _)) => out.push(Piece::Arc { c: a.center, r: a.radius, a0: a.start, sweep: a.sweep() }),
                    None => out.push(Piece::Seg(p, q)),
                }
            }
        }
        Shape::Path { points, closed } => {
            for w in points.windows(2) {
                out.push(Piece::Seg(w[0], w[1]));
            }
            if *closed && points.len() > 2 {
                out.push(Piece::Seg(points[points.len() - 1], points[0]));
            }
        }
        Shape::Lines { segments } => out.extend(segments.iter().map(|(a, b)| Piece::Seg(*a, *b))),
        _ => {}
    }
}

fn starts_with_any(s: &str, list: &[&str]) -> bool {
    let up = s.to_ascii_uppercase();
    list.iter().any(|p| up.starts_with(p))
}

/// Check the holes `scene` draws against `limits` (millimetres, turned
/// into the drawing's units here).
pub fn check_holes(d: &Drawing, scene: &Scene, limits: &Limits) -> HoleCheck {
    let k = Unit::of(d).mm().map(|mm| 1.0 / mm).unwrap_or(1.0);
    let (dia, web, bend_gap) = (limits.min_diameter * k, limits.min_web * k, limits.hole_to_bend * k);
    let mut holes: Vec<Hole> = Vec::new();
    let mut edges: Vec<Piece> = Vec::new();
    let mut bends: Vec<Piece> = Vec::new();
    for it in &scene.items {
        let bend = it.style.layer.eq_ignore_ascii_case("BEND");
        if !bend && starts_with_any(&it.style.layer, NOT_CUT_LAYERS) {
            continue;
        }
        let type_name = d.get(it.source).map(|o| o.type_name.to_ascii_uppercase()).unwrap_or_default();
        if NOT_CUT_TYPES.iter().any(|t| type_name.contains(t)) {
            continue;
        }
        if bend {
            pieces(&it.shape, &mut bends);
            continue;
        }
        match &it.shape {
            Shape::Circle { center, radius } if type_name == "CIRCLE" => holes.push(Hole { handle: it.source, center: *center, radius: *radius }),
            s => pieces(s, &mut edges),
        }
    }
    // A circle with other holes inside it is an outline.
    let outline: Vec<bool> = holes
        .iter()
        .map(|a| holes.iter().any(|b| b.radius < a.radius && a.center.distance(b.center) < a.radius - b.radius))
        .collect();
    let mut kept = Vec::new();
    for (h, o) in holes.into_iter().zip(outline) {
        if o {
            edges.push(Piece::Arc { c: h.center, r: h.radius, a0: 0.0, sweep: TAU });
        } else {
            kept.push(h);
        }
    }
    let holes = kept;
    let mut out = HoleCheck { edges: edges.len(), bends: bends.len(), ..Default::default() };
    const T: f64 = 1e-9;
    for (i, a) in holes.iter().enumerate() {
        if 2.0 * a.radius < dia - T {
            out.too_small.insert(a.handle);
        }
        for b in &holes[i + 1..] {
            if a.center.distance(b.center) - a.radius - b.radius < web - T {
                out.too_close.insert(a.handle);
                out.too_close.insert(b.handle);
            }
        }
        let edge = edges.iter().map(|e| e.distance(a.center) - a.radius).fold(f64::INFINITY, f64::min);
        if edge < web - T {
            out.near_edge.insert(a.handle);
        }
        let bend = bends.iter().map(|e| e.distance(a.center) - a.radius).fold(f64::INFINITY, f64::min);
        if bend < bend_gap - T {
            out.near_bend.insert(a.handle);
        }
    }
    out.holes = holes;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standards::{new_drawing, Template};

    #[test]
    fn holes_measured_against_the_rules() {
        let mut d = new_drawing(Template::Metric);
        let add = |d: &mut Drawing, k: ObjectKind| d.add_entity(Handle::NULL, k).unwrap();
        add(&mut d, ObjectKind::LwPolyline(LwPolyline::new(vec![Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(100.0, 50.0), Vec2::new(0.0, 50.0)], true)));
        let ok = add(&mut d, ObjectKind::Circle(Circle::new(Vec2::new(50.0, 25.0), 5.0)));
        let small = add(&mut d, ObjectKind::Circle(Circle::new(Vec2::new(20.0, 25.0), 0.5)));
        let edge = add(&mut d, ObjectKind::Circle(Circle::new(Vec2::new(4.0, 40.0), 3.0)));
        let pair = [
            add(&mut d, ObjectKind::Circle(Circle::new(Vec2::new(70.0, 10.0), 3.0))),
            add(&mut d, ObjectKind::Circle(Circle::new(Vec2::new(77.0, 10.0), 3.0))),
        ];
        // A bend line on its own layer, near one hole.
        let bend = d.add_layer("BEND", Color::Index(2), Handle::NULL, Lineweight::Default).unwrap();
        d.add_entity(bend, ObjectKind::Line(Line::new(Vec2::new(60.0, 0.0), Vec2::new(60.0, 50.0)))).unwrap();
        // Words and dimensions do not cut.
        let notes = d.layer_by_name("NOTES").map(|(h, _)| h).unwrap();
        d.add_entity(notes, ObjectKind::Line(Line::new(Vec2::new(50.0, 31.0), Vec2::new(60.0, 31.0)))).unwrap();
        let scene = crate::render::model_space(&d);
        let r = check_holes(&d, &scene, &Limits { min_diameter: 2.0, min_web: 2.0, hole_to_bend: 6.0 });
        assert_eq!(r.holes.len(), 5);
        assert_eq!(r.bends, 1);
        assert_eq!(r.too_small, BTreeSet::from([small]));
        assert_eq!(r.too_close, BTreeSet::from(pair));
        assert_eq!(r.near_edge, BTreeSet::from([edge]));
        assert_eq!(r.near_bend, BTreeSet::from([ok]));
        assert_eq!(r.flagged().len(), 5);
        // A round plate: the big circle is the outline, measured as an edge.
        let mut d = new_drawing(Template::Metric);
        add(&mut d, ObjectKind::Circle(Circle::new(Vec2::ZERO, 50.0)));
        let near = add(&mut d, ObjectKind::Circle(Circle::new(Vec2::new(45.0, 0.0), 4.0)));
        let scene = crate::render::model_space(&d);
        let r = check_holes(&d, &scene, &Limits { min_diameter: 2.0, min_web: 2.0, hole_to_bend: 6.0 });
        assert_eq!(r.holes.len(), 1);
        assert_eq!(r.near_edge, BTreeSet::from([near]));
    }

    #[test]
    fn limits_follow_the_drawing_unit() {
        let mut d = new_drawing(Template::Imperial);
        d.add_entity(Handle::NULL, ObjectKind::Circle(Circle::new(Vec2::ZERO, 0.05))).unwrap();
        let scene = crate::render::model_space(&d);
        // 2 mm is 0.0787 in: a 0.1 in hole passes.
        let r = check_holes(&d, &scene, &Limits { min_diameter: 2.0, min_web: 1.0, hole_to_bend: 1.0 });
        assert!(r.too_small.is_empty());
        let r = check_holes(&d, &scene, &Limits { min_diameter: 3.0, min_web: 1.0, hole_to_bend: 1.0 });
        assert_eq!(r.too_small.len(), 1);
    }
}

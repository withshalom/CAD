//! Constructions from picked points: what a drafting command makes of
//! where the user clicked. A program on the core sends the points as they
//! were picked and the geometry is worked out here, so every program makes
//! the same arc, dimension or text box from the same clicks.
//!
//! - [`arc_through`]: ARC by start, second and end point.
//! - [`linear_dimension_at`]: DIMLINEAR, horizontal or vertical by where
//!   the dimension line goes.
//! - [`aligned_dimension_at`]: DIMALIGNED with its line through a point.
//! - [`radial_dimension`]: DIMRADIUS and DIMDIAMETER of a picked circle or
//!   arc, its leader towards a point.
//! - [`mtext_in_box`]: MTEXT from two corners of its box.
//! - [`xline_at_angle`]: XLINE Hor, Ver and Ang.
//! - [`measure`] and [`describe_measure`]: DIST.
//! - [`selection_bounds`] and [`array_spacing`]: where a selection sits,
//!   and the spacing ARRAY offers for it.

use crate::geom::{BBox, Vec2};
use crate::model::*;
use crate::modify::{curves_of, Curve};
use crate::units::{format_angle, format_length, AngleFormat, LengthFormat, Unit};
use std::f64::consts::{FRAC_PI_2, TAU};

type R<T> = Result<T, String>;

/// The arc from `a` through `b` to `c` (counter-clockwise in the model,
/// whichever way the points were picked); `None` when the three points lie
/// in a line or two of them are the same.
pub fn arc_through(a: Vec2, b: Vec2, c: Vec2) -> Option<Arc> {
    // Worked from `a`, so far-off coordinates keep their precision.
    let (p, q) = (b - a, c - a);
    let cross = p.cross(q);
    if !cross.is_finite() || cross.abs() <= 1e-12 * p.length() * q.length() || p.length() == 0.0 || q.length() == 0.0 {
        return None;
    }
    let d = 2.0 * cross;
    let (pp, qq) = (p.dot(p), q.dot(q));
    let center = a + Vec2::new((q.y * pp - p.y * qq) / d, (p.x * qq - q.x * pp) / d);
    let radius = a.distance(center);
    let angle = |v: Vec2| (v - center).angle().rem_euclid(TAU);
    let (start, end) = if cross > 0.0 { (angle(a), angle(c)) } else { (angle(c), angle(a)) };
    Some(Arc::new(center, radius, start, end))
}

/// Whether DIMLINEAR measures vertically: when the dimension line is
/// placed further out to the side of the two points than above or below
/// them.
pub fn linear_is_vertical(p1: Vec2, p2: Vec2, at: Vec2) -> bool {
    let out = |v: f64, a: f64, b: f64| {
        let (lo, hi) = (a.min(b), a.max(b));
        if v > hi {
            v - hi
        } else if v < lo {
            lo - v
        } else {
            0.0
        }
    };
    out(at.x, p1.x, p2.x) > out(at.y, p1.y, p2.y)
}

/// A horizontal or vertical dimension between `p1` and `p2`, its line
/// through `at` ([`linear_is_vertical`] chooses); `None` when there is
/// nothing to measure that way.
pub fn linear_dimension_at(p1: Vec2, p2: Vec2, at: Vec2) -> Option<Dimension> {
    let rotation = if linear_is_vertical(p1, p2, at) { FRAC_PI_2 } else { 0.0 };
    let dm = Dimension::linear(p1, p2, at, rotation);
    (dm.measurement > 0.0).then_some(dm)
}

/// An aligned dimension between `p1` and `p2`, its line through `at`;
/// `None` when the points are the same.
pub fn aligned_dimension_at(p1: Vec2, p2: Vec2, at: Vec2) -> Option<Dimension> {
    let d = p2 - p1;
    let l = d.length();
    if l == 0.0 || !l.is_finite() {
        return None;
    }
    Some(Dimension::aligned(p1, p2, d.cross(at - p1) / l))
}

/// The circle or arc an object draws, in the world's plane: centre,
/// radius, and whether it is an arc.
pub fn round_of(kind: &ObjectKind) -> Option<(Vec2, f64, bool)> {
    match (kind, curves_of(kind).first()) {
        (ObjectKind::Circle(_), Some(Curve::Arc { c, r, .. })) => Some((*c, *r, false)),
        (ObjectKind::Arc(_), Some(Curve::Arc { c, r, .. })) => Some((*c, *r, true)),
        _ => None,
    }
}

/// A radius or diameter dimension of circle or arc `h`, its leader
/// pointing towards `at`. `diameter` `None` chooses as DIM does: a radius
/// for an arc, a diameter for a circle.
pub fn radial_dimension(d: &Drawing, h: Handle, at: Vec2, diameter: Option<bool>) -> R<Dimension> {
    let o = d.get(h).ok_or_else(|| format!("no object {:X}", h.0))?;
    let (c, r, arc) = round_of(&o.kind).ok_or("that is not an arc or a circle")?;
    let angle = if at.distance(c) > 0.0 { (at - c).angle() } else { 0.0 };
    Ok(if diameter.unwrap_or(!arc) {
        Dimension::diameter(c, r, angle)
    } else {
        Dimension::radius(c, r, angle)
    })
}

/// Multiline text filling the box between corners `a` and `b`: it starts
/// at the top left and wraps at the box's width.
pub fn mtext_in_box(a: Vec2, b: Vec2, height: f64, rotation: f64, text: &str) -> R<MText> {
    if height.is_nan() || height <= 0.0 {
        return Err("the text height must be more than zero".into());
    }
    if text.trim().is_empty() {
        return Err("there is no text".into());
    }
    let top_left = Vec2::new(a.x.min(b.x), a.y.max(b.y));
    let mut m = MText::new(top_left, height, (b.x - a.x).abs(), text, 1);
    m.x_axis = Vec2::from_angle(rotation).extend(0.0);
    Ok(m)
}

/// A construction line through `p` at `angle` (radians).
pub fn xline_at_angle(p: Vec2, angle: f64) -> ConstructionLine {
    ConstructionLine::new(p, Vec2::from_angle(angle))
}

/// What DIST reports between two points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measure {
    pub distance: f64,
    /// Radians, 0 to 2π, counter-clockwise from the X axis.
    pub angle: f64,
    pub delta: Vec2,
}

pub fn measure(a: Vec2, b: Vec2) -> Measure {
    let delta = b - a;
    Measure {
        distance: delta.length(),
        angle: if delta.length() > 0.0 { delta.angle().rem_euclid(TAU) } else { 0.0 },
        delta,
    }
}

/// DIST's line, in the drawing's length and angle formats.
pub fn describe_measure(d: &Drawing, m: &Measure) -> String {
    let unit = Unit::of(d);
    let f = LengthFormat::of(d);
    let len = |v: f64| format_length(v, unit, &f);
    let af = AngleFormat::from_aunits(d.header.aunits.unwrap_or(0));
    format!(
        "Distance = {}, Angle = {}, ΔX = {}, ΔY = {}",
        len(m.distance),
        format_angle(m.angle, af, d.header.auprec.unwrap_or(0)),
        len(m.delta.x),
        len(m.delta.y)
    )
}

/// How far the objects reach as drawn; empty when they draw nothing.
pub fn selection_bounds(d: &Drawing, hs: &[Handle]) -> BBox {
    let mut b = BBox::EMPTY;
    for h in hs {
        b.union(&crate::render::entity_bounds(d, *h));
    }
    b
}

/// Three significant figures: spacing a person would type.
fn round3(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    let p = 10f64.powi(2 - v.abs().log10().floor() as i32);
    (v * p).round() / p
}

/// The row and column spacing ARRAY offers: half as much again as the
/// selection is tall and wide, so the copies stand clear of each other.
pub fn array_spacing(d: &Drawing, hs: &[Handle]) -> (f64, f64) {
    let b = selection_bounds(d, hs);
    let fallback = if Unit::of(d).is_imperial() { 0.5 } else { 10.0 };
    let gap = |v: f64| if v > 0.0 && v.is_finite() { round3(v * 1.5) } else { fallback };
    if b.is_empty() {
        return (fallback, fallback);
    }
    (gap(b.height()), gap(b.width()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standards::{new_drawing, Template};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn arcs_through_three_points_either_way_round() {
        // Counter-clockwise: right, top, left of the unit circle about (5, 5).
        let a = arc_through(Vec2::new(6.0, 5.0), Vec2::new(5.0, 6.0), Vec2::new(4.0, 5.0)).unwrap();
        assert!(a.center.xy().distance(Vec2::new(5.0, 5.0)) < 1e-12 && close(a.radius, 1.0));
        assert!(close(a.start_angle, 0.0) && close(a.end_angle, std::f64::consts::PI));
        // Clockwise picks make the same arc, stored counter-clockwise.
        let b = arc_through(Vec2::new(4.0, 5.0), Vec2::new(5.0, 6.0), Vec2::new(6.0, 5.0)).unwrap();
        assert_eq!((a.start_angle, a.end_angle), (b.start_angle, b.end_angle));
        // Far from the origin, precision holds.
        let o = Vec2::new(1e7, -3e6);
        let c = arc_through(o + Vec2::new(10.0, 0.0), o + Vec2::new(0.0, 10.0), o + Vec2::new(-10.0, 0.0)).unwrap();
        assert!(c.center.xy().distance(o) < 1e-6 && (c.radius - 10.0).abs() < 1e-6);
        assert!(arc_through(Vec2::ZERO, Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0)).is_none());
        assert!(arc_through(Vec2::ZERO, Vec2::ZERO, Vec2::new(2.0, 2.0)).is_none());
    }

    #[test]
    fn dimensions_from_where_the_line_goes() {
        let (p1, p2) = (Vec2::new(0.0, 0.0), Vec2::new(100.0, 40.0));
        // Below the points: horizontal, measuring 100.
        let h = linear_dimension_at(p1, p2, Vec2::new(50.0, -20.0)).unwrap();
        assert!(close(h.dim_rotation, 0.0) && close(h.measurement, 100.0));
        // Out to the right: vertical, measuring 40.
        let v = linear_dimension_at(p1, p2, Vec2::new(130.0, 20.0)).unwrap();
        assert!(close(v.dim_rotation, FRAC_PI_2) && close(v.measurement, 40.0));
        // Nothing to measure horizontally between points one above the other.
        assert!(linear_dimension_at(Vec2::ZERO, Vec2::new(0.0, 10.0), Vec2::new(5.0, -20.0)).is_none());
        // Aligned: the line passes through the point, on its side.
        let a = aligned_dimension_at(Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(4.0, 7.0)).unwrap();
        assert!(close(a.measurement, 10.0) && close(a.pt10.y, 7.0));
        let b = aligned_dimension_at(Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(4.0, -3.0)).unwrap();
        assert!(close(b.pt10.y, -3.0));
        assert!(aligned_dimension_at(Vec2::ZERO, Vec2::ZERO, Vec2::new(1.0, 1.0)).is_none());
    }

    #[test]
    fn radial_dimensions_of_picked_objects() {
        let mut d = new_drawing(Template::Metric);
        let c = d.add_entity(Handle::NULL, ObjectKind::Circle(Circle::new(Vec2::new(10.0, 10.0), 5.0))).unwrap();
        let a = d.add_entity(Handle::NULL, ObjectKind::Arc(Arc::new(Vec2::ZERO, 8.0, 0.0, 1.0))).unwrap();
        let l = d.add_entity(Handle::NULL, ObjectKind::Line(Line::new(Vec2::ZERO, Vec2::new(1.0, 1.0)))).unwrap();
        let dc = radial_dimension(&d, c, Vec2::new(20.0, 10.0), None).unwrap();
        assert_eq!(dc.kind, DimensionKind::Diameter);
        assert!(close(dc.measurement, 10.0));
        let da = radial_dimension(&d, a, Vec2::new(0.0, 20.0), None).unwrap();
        assert_eq!(da.kind, DimensionKind::Radius);
        assert!(close(da.measurement, 8.0));
        assert_eq!(radial_dimension(&d, c, Vec2::ZERO, Some(false)).unwrap().kind, DimensionKind::Radius);
        assert!(radial_dimension(&d, l, Vec2::ZERO, None).is_err());
    }

    #[test]
    fn text_boxes_lines_and_measures() {
        let m = mtext_in_box(Vec2::new(80.0, -40.0), Vec2::new(0.0, -20.0), 3.5, 0.0, "A\\PB").unwrap();
        assert_eq!((m.insertion.x, m.insertion.y, m.rect_width, m.attachment), (0.0, -20.0, 80.0, 1));
        assert!(mtext_in_box(Vec2::ZERO, Vec2::new(1.0, 1.0), 0.0, 0.0, "x").is_err());
        assert!(mtext_in_box(Vec2::ZERO, Vec2::new(1.0, 1.0), 1.0, 0.0, " ").is_err());
        let x = xline_at_angle(Vec2::new(1.0, 2.0), FRAC_PI_2);
        assert!(close(x.direction.x, 0.0) && close(x.direction.y, 1.0));
        let d = new_drawing(Template::Metric);
        let m = measure(Vec2::ZERO, Vec2::new(3.0, -4.0));
        assert!(close(m.distance, 5.0) && m.angle > std::f64::consts::PI);
        let text = describe_measure(&d, &m);
        assert!(text.starts_with("Distance = 5") && text.contains("ΔY = -4"), "{text}");
    }

    #[test]
    fn array_spacing_clears_the_selection() {
        let mut d = new_drawing(Template::Metric);
        let h = d.add_entity(Handle::NULL, ObjectKind::Circle(Circle::new(Vec2::ZERO, 5.0))).unwrap();
        assert_eq!(array_spacing(&d, &[h]), (15.0, 15.0));
        assert_eq!(array_spacing(&d, &[]), (10.0, 10.0));
        assert_eq!(round3(123.456), 123.0);
        assert_eq!(round3(0.012345), 0.0123);
    }
}

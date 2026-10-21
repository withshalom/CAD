//! Grips: the handles a selected object shows on its defining points, and
//! what dragging one does (DESIGN.md §5). A circle shows its centre and
//! four quadrants, an arc its centre, ends and middle, a line its ends and
//! middle, a polyline its vertices, and text, inserts, construction lines,
//! viewports and leaders the point they are placed by.
//!
//! Dragging the centre or a placing point moves the object; a quadrant or
//! an arc's middle changes the radius; an arc's end turns that end; a
//! line's end or a polyline's vertex moves to where it is dropped.

use crate::geom::Vec2;
use crate::model::*;
use crate::modify::{to_world_plane, Affine};
use crate::ops;
use std::f64::consts::{FRAC_PI_2, TAU};

type R<T> = Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GripKind {
    /// A circle's or arc's centre: moves it.
    Center,
    /// On the curve: the radius.
    Radius,
    ArcStart,
    ArcEnd,
    /// A line's start (1) or end (2).
    End(u8),
    /// A polyline's vertex.
    Vertex(usize),
    /// The point an object is placed by: moves it.
    Move,
}

impl GripKind {
    /// The name the app protocol uses.
    pub fn name(self) -> &'static str {
        match self {
            GripKind::Center => "center",
            GripKind::Radius => "radius",
            GripKind::ArcStart => "arc-start",
            GripKind::ArcEnd => "arc-end",
            GripKind::End(_) => "end",
            GripKind::Vertex(_) => "vertex",
            GripKind::Move => "move",
        }
    }

    /// Which end or vertex, for the kinds that have one.
    pub fn field(self) -> Option<usize> {
        match self {
            GripKind::End(i) => Some(i as usize),
            GripKind::Vertex(i) => Some(i),
            _ => None,
        }
    }

    /// A grip kind from its name and field (`-` or empty for none).
    pub fn parse(name: &str, field: &str) -> Option<GripKind> {
        let n = || field.parse::<usize>().ok();
        Some(match name {
            "center" => GripKind::Center,
            "radius" => GripKind::Radius,
            "arc-start" => GripKind::ArcStart,
            "arc-end" => GripKind::ArcEnd,
            "end" => match n()? {
                1 => GripKind::End(1),
                2 => GripKind::End(2),
                _ => return None,
            },
            "vertex" => GripKind::Vertex(n()?),
            "move" => GripKind::Move,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grip {
    pub at: Vec2,
    pub kind: GripKind,
}

/// The object as it lies in the world's plane (an arc seen from below is
/// restated), or `None` for one tilted out of it.
fn flat(kind: &ObjectKind) -> Option<ObjectKind> {
    let mut k = kind.clone();
    to_world_plane(&mut k).ok()?;
    Some(k)
}

/// The grips of object `h`; none for an object that has none, or is not
/// in the world's plane.
pub fn grips(d: &Drawing, h: Handle) -> Vec<Grip> {
    let Some(o) = d.get(h) else {
        return Vec::new();
    };
    let Some(kind) = flat(&o.kind) else {
        return Vec::new();
    };
    let g = |at: Vec2, kind: GripKind| Grip { at, kind };
    match &kind {
        ObjectKind::Circle(c) => {
            let cen = c.center.xy();
            let mut v = vec![g(cen, GripKind::Center)];
            for q in 0..4 {
                v.push(g(cen + Vec2::from_angle(q as f64 * FRAC_PI_2) * c.radius, GripKind::Radius));
            }
            v
        }
        ObjectKind::Arc(a) => {
            let cen = a.center.xy();
            let sweep = (a.end_angle - a.start_angle).rem_euclid(TAU);
            let on = |t: f64| cen + Vec2::from_angle(t) * a.radius;
            vec![
                g(cen, GripKind::Center),
                g(on(a.start_angle), GripKind::ArcStart),
                g(on(a.end_angle), GripKind::ArcEnd),
                g(on(a.start_angle + sweep / 2.0), GripKind::Radius),
            ]
        }
        ObjectKind::Line(l) => {
            let (a, b) = (l.start.xy(), l.end.xy());
            vec![g(a, GripKind::End(1)), g(b, GripKind::End(2)), g((a + b) * 0.5, GripKind::Move)]
        }
        ObjectKind::LwPolyline(p) => p.points.iter().enumerate().map(|(i, q)| g(*q, GripKind::Vertex(i))).collect(),
        ObjectKind::Text(t) => vec![g(t.insertion, GripKind::Move)],
        ObjectKind::MText(m) => vec![g(m.insertion.xy(), GripKind::Move)],
        ObjectKind::Insert(i) => vec![g(i.insertion.xy(), GripKind::Move)],
        ObjectKind::Attribute(a) => vec![g(a.text.insertion, GripKind::Move)],
        ObjectKind::Xline(x) | ObjectKind::Ray(x) => vec![g(x.base.xy(), GripKind::Move)],
        ObjectKind::PaperViewport(v) => vec![g(v.center.xy(), GripKind::Move)],
        ObjectKind::Leader(l) => l.vertices.first().map(|p| vec![g(p.xy(), GripKind::Move)]).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Drag grip `kind` of object `h` from `from` to `to`.
pub fn drag(d: &mut Drawing, h: Handle, kind: GripKind, from: Vec2, to: Vec2) -> R<()> {
    ops::check_unlocked(d, h)?;
    if !to.is_finite() || !from.is_finite() {
        return Err("that point is out of reach".into());
    }
    if matches!(kind, GripKind::Center | GripKind::Move) {
        return ops::transform_entities(d, &[h], &Affine::translate(to - from));
    }
    let o = d.get(h).ok_or_else(|| format!("no object {:X}", h.0))?;
    let mut k = flat(&o.kind).ok_or("the object is not flat in the drawing's plane")?;
    match (&mut k, kind) {
        (ObjectKind::Circle(c), GripKind::Radius) => {
            let r = to.distance(c.center.xy());
            if r.is_nan() || r <= 0.0 {
                return Err("the radius must be more than zero".into());
            }
            c.radius = r;
        }
        (ObjectKind::Arc(a), GripKind::Radius) => {
            let r = to.distance(a.center.xy());
            if r.is_nan() || r <= 0.0 {
                return Err("the radius must be more than zero".into());
            }
            a.radius = r;
        }
        (ObjectKind::Arc(a), GripKind::ArcStart | GripKind::ArcEnd) => {
            let c = a.center.xy();
            if to.distance(c) == 0.0 {
                return Err("drop the end away from the centre".into());
            }
            let t = (to - c).angle().rem_euclid(TAU);
            if kind == GripKind::ArcStart {
                a.start_angle = t;
            } else {
                a.end_angle = t;
            }
            if (a.end_angle - a.start_angle).rem_euclid(TAU) < 1e-9 {
                return Err("the arc would have no length".into());
            }
        }
        (ObjectKind::Line(l), GripKind::End(i)) => {
            let p = if i == 1 { &mut l.start } else { &mut l.end };
            *p = to.extend(p.z);
            if l.start.xy().distance(l.end.xy()) == 0.0 {
                return Err("the line would have no length".into());
            }
        }
        (ObjectKind::LwPolyline(p), GripKind::Vertex(i)) => {
            *p.points.get_mut(i).ok_or_else(|| format!("the polyline has no vertex {i}"))? = to;
        }
        _ => return Err(format!("this object has no {} grip", kind.name())),
    }
    ops::replace(d, h, k).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standards::{new_drawing, Template};

    #[test]
    fn grips_where_the_object_is_defined() {
        let mut d = new_drawing(Template::Metric);
        let c = d.add_entity(Handle::NULL, ObjectKind::Circle(Circle::new(Vec2::new(1.0, 2.0), 3.0))).unwrap();
        let g = grips(&d, c);
        assert_eq!(g.len(), 5);
        assert_eq!(g[0], Grip { at: Vec2::new(1.0, 2.0), kind: GripKind::Center });
        assert!(g[1].at.distance(Vec2::new(4.0, 2.0)) < 1e-12);
        // An arc seen from below shows its grips where it is drawn.
        let mut arc = Arc::new(Vec2::new(10.0, 0.0), 2.0, 0.0, FRAC_PI_2);
        arc.extrusion = crate::geom::Vec3::new(0.0, 0.0, -1.0);
        let a = d.add_entity(Handle::NULL, ObjectKind::Arc(arc)).unwrap();
        let g = grips(&d, a);
        assert!(g[0].at.distance(Vec2::new(-10.0, 0.0)) < 1e-12);
        let l = d.add_entity(Handle::NULL, ObjectKind::Line(Line::new(Vec2::ZERO, Vec2::new(10.0, 0.0)))).unwrap();
        assert_eq!(grips(&d, l)[2], Grip { at: Vec2::new(5.0, 0.0), kind: GripKind::Move });
        for k in [GripKind::Center, GripKind::End(2), GripKind::Vertex(3), GripKind::ArcEnd] {
            let f = k.field().map(|v| v.to_string()).unwrap_or_default();
            assert_eq!(GripKind::parse(k.name(), &f), Some(k));
        }
        assert_eq!(GripKind::parse("end", "3"), None);
    }

    #[test]
    fn dragging_grips_edits() {
        let mut d = new_drawing(Template::Metric);
        let c = d.add_entity(Handle::NULL, ObjectKind::Circle(Circle::new(Vec2::ZERO, 3.0))).unwrap();
        drag(&mut d, c, GripKind::Radius, Vec2::new(3.0, 0.0), Vec2::new(0.0, 7.0)).unwrap();
        drag(&mut d, c, GripKind::Center, Vec2::ZERO, Vec2::new(5.0, 5.0)).unwrap();
        let ObjectKind::Circle(k) = &d.get(c).unwrap().kind else { panic!() };
        assert_eq!((k.radius, k.center.x, k.center.y), (7.0, 5.0, 5.0));
        let a = d.add_entity(Handle::NULL, ObjectKind::Arc(Arc::new(Vec2::ZERO, 2.0, 0.0, FRAC_PI_2))).unwrap();
        drag(&mut d, a, GripKind::ArcEnd, Vec2::new(0.0, 2.0), Vec2::new(-5.0, 0.0)).unwrap();
        let ObjectKind::Arc(k) = &d.get(a).unwrap().kind else { panic!() };
        assert!((k.end_angle - std::f64::consts::PI).abs() < 1e-12 && k.radius == 2.0);
        let l = d.add_entity(Handle::NULL, ObjectKind::Line(Line::new(Vec2::ZERO, Vec2::new(10.0, 0.0)))).unwrap();
        drag(&mut d, l, GripKind::End(2), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)).unwrap();
        assert!(drag(&mut d, l, GripKind::End(1), Vec2::ZERO, Vec2::new(10.0, 10.0)).is_err());
        let ObjectKind::Line(k) = &d.get(l).unwrap().kind else { panic!() };
        assert_eq!((k.end.x, k.end.y), (10.0, 10.0));
        let p = d.add_entity(Handle::NULL, ObjectKind::LwPolyline(LwPolyline::new(vec![Vec2::ZERO, Vec2::new(5.0, 0.0)], false))).unwrap();
        drag(&mut d, p, GripKind::Vertex(1), Vec2::new(5.0, 0.0), Vec2::new(6.0, 1.0)).unwrap();
        assert!(drag(&mut d, p, GripKind::Vertex(4), Vec2::ZERO, Vec2::ZERO).is_err());
        assert!(drag(&mut d, p, GripKind::Radius, Vec2::ZERO, Vec2::ZERO).is_err());
        let ObjectKind::LwPolyline(k) = &d.get(p).unwrap().kind else { panic!() };
        assert_eq!(k.points[1], Vec2::new(6.0, 1.0));
    }
}

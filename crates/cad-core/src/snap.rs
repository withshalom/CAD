//! Object snaps and picking (DESIGN.md §9): exact points of existing
//! geometry near the cursor, the object under it, and the objects in a
//! selection box. Shared by every app, so they all snap and select alike.
//!
//! Everything works on a rendered [`Scene`] in world coordinates, so
//! block contents and dimensions snap like any other geometry, and an
//! app that keeps its scene answers each cursor move without walking the
//! drawing again. Searches are a straight pass over the scene; a spatial
//! index can come when drawings outgrow it.

use crate::font;
use crate::geom::curve::{bulge_to_arc, normalize_angle, normalize_sweep};
use crate::geom::{BBox, Vec2};
use crate::model::Handle;
use crate::render::{shape_bounds, Scene, Shape};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Snap kinds, as bits, so an app can keep any set running.
pub mod kind {
    pub const ENDPOINT: u32 = 1;
    pub const MIDPOINT: u32 = 2;
    pub const CENTER: u32 = 4;
    pub const QUADRANT: u32 = 8;
    pub const INTERSECTION: u32 = 16;
    pub const NEAREST: u32 = 32;
    pub const NODE: u32 = 64;
    pub const INSERTION: u32 = 128;
    /// Every kind above.
    pub const ALL: u32 = 255;
    /// What new drawings run with: the snaps that find defined points.
    pub const DEFAULT: u32 = ENDPOINT | MIDPOINT | CENTER | QUADRANT | INTERSECTION | NODE | INSERTION;

    /// The tag an app shows beside the marker.
    pub fn name(k: u32) -> &'static str {
        match k {
            ENDPOINT => "Endpoint",
            MIDPOINT => "Midpoint",
            CENTER => "Center",
            QUADRANT => "Quadrant",
            INTERSECTION => "Intersection",
            NEAREST => "Nearest",
            NODE => "Node",
            INSERTION => "Insertion",
            _ => "",
        }
    }
}

/// A snap point found near the cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    /// One of the [`kind`] bits.
    pub kind: u32,
    pub point: Vec2,
    /// The entity it belongs to (the first of two, for an intersection).
    pub source: Handle,
    /// Distance from the cursor.
    pub distance: f64,
}

/// A piece of stroked geometry: a segment, or an arc counter-clockwise
/// from `start` to `end` (a full circle when `full`).
#[derive(Debug, Clone, Copy)]
enum Prim {
    Seg {
        a: Vec2,
        b: Vec2,
        /// Its ends and middle are points a user drew (not the facets of a
        /// tessellated curve).
        defined: bool,
    },
    Arc {
        c: Vec2,
        r: f64,
        start: f64,
        end: f64,
        full: bool,
    },
    /// A construction line through `a` along the unit vector `u`, from `a`
    /// on when `ray`.
    Ray {
        a: Vec2,
        u: Vec2,
        ray: bool,
    },
}

impl Prim {
    /// A straight prim as origin, direction and parameter range.
    fn linear(&self) -> Option<(Vec2, Vec2, f64, f64)> {
        match *self {
            Prim::Seg { a, b, .. } => Some((a, b - a, 0.0, 1.0)),
            Prim::Ray { a, u, ray } => Some((a, u, if ray { 0.0 } else { f64::NEG_INFINITY }, f64::INFINITY)),
            Prim::Arc { .. } => None,
        }
    }

    fn on_arc(start: f64, end: f64, full: bool, angle: f64) -> bool {
        if full {
            return true;
        }
        let sweep = normalize_sweep(end - start);
        normalize_angle(angle - start) <= sweep + 1e-12
    }

    /// The closest point of the prim to `p`.
    fn nearest(&self, p: Vec2) -> Vec2 {
        match *self {
            Prim::Ray { a, u, ray } => {
                let t = (p - a).dot(u);
                a + u * if ray { t.max(0.0) } else { t }
            }
            Prim::Seg { a, b, .. } => {
                let d = b - a;
                let len2 = d.dot(d);
                if len2 == 0.0 {
                    return a;
                }
                let t = ((p - a).dot(d) / len2).clamp(0.0, 1.0);
                a + d * t
            }
            Prim::Arc {
                c,
                r,
                start,
                end,
                full,
            } => {
                let v = p - c;
                let ang = if v.length() == 0.0 { start } else { v.angle() };
                if Prim::on_arc(start, end, full, ang) {
                    c + Vec2::from_angle(ang) * r
                } else {
                    let p0 = c + Vec2::from_angle(start) * r;
                    let p1 = c + Vec2::from_angle(end) * r;
                    if (p0 - p).length() <= (p1 - p).length() {
                        p0
                    } else {
                        p1
                    }
                }
            }
        }
    }

    fn distance(&self, p: Vec2) -> f64 {
        (self.nearest(p) - p).length()
    }
}

/// The stroked pieces of a shape. Text and fills have none.
fn prims(shape: &Shape) -> Vec<Prim> {
    let mut out = Vec::new();
    match shape {
        Shape::Line { a, b } => out.push(Prim::Seg {
            a: *a,
            b: *b,
            defined: true,
        }),
        Shape::Circle { center, radius } => out.push(Prim::Arc {
            c: *center,
            r: *radius,
            start: 0.0,
            end: TAU,
            full: true,
        }),
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => out.push(Prim::Arc {
            c: *center,
            r: *radius,
            start: *start,
            end: *end,
            full: false,
        }),
        Shape::Polyline { vertices, closed } => {
            let n = vertices.len();
            let segs = if *closed { n } else { n.saturating_sub(1) };
            for i in 0..segs {
                let (a, bulge) = vertices[i];
                let b = vertices[(i + 1) % n].0;
                match bulge_to_arc(a, b, bulge) {
                    Some((arc, _)) => out.push(Prim::Arc {
                        c: arc.center,
                        r: arc.radius,
                        start: arc.start,
                        end: arc.end,
                        full: false,
                    }),
                    None => out.push(Prim::Seg { a, b, defined: true }),
                }
            }
        }
        Shape::Path { points, closed } => {
            let n = points.len();
            let segs = if *closed { n } else { n.saturating_sub(1) };
            for i in 0..segs {
                out.push(Prim::Seg {
                    a: points[i],
                    b: points[(i + 1) % n],
                    defined: false,
                });
            }
        }
        Shape::Ray {
            origin,
            direction,
            ray,
        } => out.push(Prim::Ray {
            a: *origin,
            u: direction.normalized(),
            ray: *ray,
        }),
        // A hatch's pattern lines give no snap points (as other CAD
        // programs leave hatches out of object snaps by default).
        Shape::Fill { .. } | Shape::Point { .. } | Shape::Text { .. } | Shape::Lines { .. } => {}
    }
    out
}

/// Whether an item can be skipped for a query at `at` within `radius`:
/// it lies wholly away from it. Construction lines have no extent and are
/// never skipped.
fn far(scene: &Scene, i: usize, at: Vec2, radius: f64) -> bool {
    !matches!(scene.items[i].shape, Shape::Ray { .. }) && !contains(&grow(scene.item_box(i), radius), at)
}

fn grow(mut b: BBox, by: f64) -> BBox {
    b.min = b.min - Vec2::new(by, by);
    b.max += Vec2::new(by, by);
    b
}

fn contains(b: &BBox, p: Vec2) -> bool {
    !b.is_empty() && p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y
}

fn bounds(shape: &Shape) -> BBox {
    let mut b = BBox::default();
    shape_bounds(shape, &mut b);
    b
}

/// Where two prims cross.
fn intersections(p: &Prim, q: &Prim) -> Vec<Vec2> {
    let mut out = Vec::new();
    match (*p, *q) {
        (Prim::Seg { .. } | Prim::Ray { .. }, Prim::Seg { .. } | Prim::Ray { .. }) => {
            let (Some((a, r, t0, t1)), Some((c, s, u0, u1))) = (p.linear(), q.linear()) else {
                return out;
            };
            let den = r.cross(s);
            if den.abs() < 1e-15 * r.length().max(1.0) * s.length().max(1.0) {
                return out;
            }
            let t = (c - a).cross(s) / den;
            let u = (c - a).cross(r) / den;
            let tol = 1e-9;
            if t >= t0 - tol && t <= t1 + tol && u >= u0 - tol && u <= u1 + tol {
                out.push(a + r * t);
            }
        }
        (lin @ (Prim::Seg { .. } | Prim::Ray { .. }), arc @ Prim::Arc { .. })
        | (arc @ Prim::Arc { .. }, lin @ (Prim::Seg { .. } | Prim::Ray { .. })) => {
            let Prim::Arc {
                c,
                r,
                start,
                end,
                full,
            } = arc
            else {
                unreachable!()
            };
            let Some((a, d, t0, t1)) = lin.linear() else {
                return out;
            };
            let f = a - c;
            let qa = d.dot(d);
            if qa == 0.0 {
                return out;
            }
            let qb = 2.0 * f.dot(d);
            let qc = f.dot(f) - r * r;
            let disc = qb * qb - 4.0 * qa * qc;
            if disc < 0.0 {
                return out;
            }
            let sq = disc.sqrt();
            for t in [(-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa)] {
                if t >= t0 - 1e-9 && t <= t1 + 1e-9 {
                    let pt = a + d * t;
                    if Prim::on_arc(start, end, full, (pt - c).angle()) {
                        out.push(pt);
                    }
                }
            }
        }
        (
            Prim::Arc {
                c: c0,
                r: r0,
                start: s0,
                end: e0,
                full: f0,
            },
            Prim::Arc {
                c: c1,
                r: r1,
                start: s1,
                end: e1,
                full: f1,
            },
        ) => {
            let v = c1 - c0;
            let dist = v.length();
            if dist == 0.0 || dist > r0 + r1 || dist < (r0 - r1).abs() {
                return out;
            }
            let a = (r0 * r0 - r1 * r1 + dist * dist) / (2.0 * dist);
            let h = (r0 * r0 - a * a).max(0.0).sqrt();
            let u = v / dist;
            let base = c0 + u * a;
            for pt in [base + u.perp() * h, base - u.perp() * h] {
                if Prim::on_arc(s0, e0, f0, (pt - c0).angle()) && Prim::on_arc(s1, e1, f1, (pt - c1).angle()) {
                    out.push(pt);
                }
            }
        }
    }
    out
}

/// Snap points within `radius` of `at`, closest first. Nearest is the
/// fallback: it comes after every other kind found, as a user expects a
/// defined point to win over a point merely on an object.
pub fn snaps(scene: &Scene, at: Vec2, radius: f64, kinds: u32) -> Vec<Snap> {
    let mut found: Vec<Snap> = Vec::new();
    let mut push = |kind: u32, point: Vec2, source: Handle| {
        if kinds & kind == 0 || !point.is_finite() {
            return;
        }
        let distance = (point - at).length();
        if distance <= radius {
            found.push(Snap {
                kind,
                point,
                source,
                distance,
            });
        }
    };
    let mut near_prims: Vec<(Prim, Handle)> = Vec::new();
    for (i, it) in scene.items.iter().enumerate() {
        if far(scene, i, at, radius) {
            continue;
        }
        let src = it.source;
        match &it.shape {
            Shape::Point { at: p } => push(kind::NODE, *p, src),
            Shape::Text { position, .. } => push(kind::INSERTION, *position, src),
            Shape::Fill { .. } => {}
            shape => {
                for p in prims(shape) {
                    match p {
                        Prim::Seg { a, b, defined } => {
                            if defined {
                                push(kind::ENDPOINT, a, src);
                                push(kind::ENDPOINT, b, src);
                                push(kind::MIDPOINT, (a + b) * 0.5, src);
                            }
                        }
                        // A ray ends at its base point; an infinite line's
                        // base point is its middle, as other CAD programs
                        // snap it.
                        Prim::Ray { a, ray, .. } => {
                            push(if ray { kind::ENDPOINT } else { kind::MIDPOINT }, a, src);
                        }
                        Prim::Arc {
                            c,
                            r,
                            start,
                            end,
                            full,
                        } => {
                            push(kind::CENTER, c, src);
                            if !full {
                                push(kind::ENDPOINT, c + Vec2::from_angle(start) * r, src);
                                push(kind::ENDPOINT, c + Vec2::from_angle(end) * r, src);
                                let mid = start + normalize_sweep(end - start) / 2.0;
                                push(kind::MIDPOINT, c + Vec2::from_angle(mid) * r, src);
                            }
                            for q in 0..4 {
                                let ang = q as f64 * FRAC_PI_2;
                                if Prim::on_arc(start, end, full, ang) {
                                    push(kind::QUADRANT, c + Vec2::from_angle(ang) * r, src);
                                }
                            }
                        }
                    }
                    if p.distance(at) <= radius {
                        push(kind::NEAREST, p.nearest(at), src);
                        near_prims.push((p, src));
                    }
                }
                // Open tessellated paths still have two real ends.
                if let Shape::Path { points, closed: false } = shape {
                    if let (Some(a), Some(b)) = (points.first(), points.last()) {
                        push(kind::ENDPOINT, *a, src);
                        push(kind::ENDPOINT, *b, src);
                    }
                }
            }
        }
    }
    if kinds & kind::INTERSECTION != 0 {
        for i in 0..near_prims.len() {
            for j in i + 1..near_prims.len() {
                for pt in intersections(&near_prims[i].0, &near_prims[j].0) {
                    push(kind::INTERSECTION, pt, near_prims[i].1);
                }
            }
        }
    }
    found.sort_by(|a, b| {
        (a.kind == kind::NEAREST)
            .cmp(&(b.kind == kind::NEAREST))
            .then(a.distance.total_cmp(&b.distance))
    });
    found.dedup_by(|a, b| a.kind == b.kind && (a.point - b.point).length() < 1e-9);
    found
}

fn point_in_rings(rings: &[Vec<Vec2>], p: Vec2) -> bool {
    let mut inside = false;
    for r in rings {
        let n = r.len();
        for i in 0..n {
            let (a, b) = (r[i], r[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
    }
    inside
}

/// How far `at` is from what a shape draws.
fn distance_to(shape: &Shape, at: Vec2) -> f64 {
    match shape {
        Shape::Point { at: p } => (*p - at).length(),
        Shape::Fill { rings } => {
            if point_in_rings(rings, at) {
                0.0
            } else {
                rings
                    .iter()
                    .flat_map(|r| {
                        (0..r.len()).map(move |i| Prim::Seg {
                            a: r[i],
                            b: r[(i + 1) % r.len()],
                            defined: false,
                        })
                    })
                    .map(|p| p.distance(at))
                    .fold(f64::INFINITY, f64::min)
            }
        }
        Shape::Text { text, .. } => {
            let Some(pl) = shape.placement() else {
                return f64::INFINITY;
            };
            let corners = font::text_box(text, &pl).to_vec();
            if point_in_rings(std::slice::from_ref(&corners), at) {
                0.0
            } else {
                (0..4)
                    .map(|i| {
                        Prim::Seg {
                            a: corners[i],
                            b: corners[(i + 1) % 4],
                            defined: false,
                        }
                        .distance(at)
                    })
                    .fold(f64::INFINITY, f64::min)
            }
        }
        Shape::Lines { segments } => segments
            .iter()
            .map(|(a, b)| {
                Prim::Seg {
                    a: *a,
                    b: *b,
                    defined: false,
                }
                .distance(at)
            })
            .fold(f64::INFINITY, f64::min),
        s => prims(s).iter().map(|p| p.distance(at)).fold(f64::INFINITY, f64::min),
    }
}

/// The entity drawn closest to `at`, if any is within `radius`. Later
/// items win ties, as they are drawn on top.
pub fn pick(scene: &Scene, at: Vec2, radius: f64) -> Option<Handle> {
    let mut best: Option<(f64, Handle)> = None;
    for (i, it) in scene.items.iter().enumerate() {
        if far(scene, i, at, radius) {
            continue;
        }
        let d = distance_to(&it.shape, at);
        if d <= radius && best.is_none_or(|(bd, _)| d <= bd) {
            best = Some((d, it.source));
        }
    }
    best.map(|(_, h)| h)
}

/// Whether a segment touches the box (Liang–Barsky clipping).
fn segment_touches(a: Vec2, b: Vec2, bx: &BBox) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-d.x, a.x - bx.min.x),
        (d.x, bx.max.x - a.x),
        (-d.y, a.y - bx.min.y),
        (d.y, bx.max.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// Whether a shape crosses or lies in the box.
fn touches(shape: &Shape, bx: &BBox) -> bool {
    let corners = [
        bx.min,
        Vec2::new(bx.max.x, bx.min.y),
        bx.max,
        Vec2::new(bx.min.x, bx.max.y),
    ];
    match shape {
        Shape::Point { at } => contains(bx, *at),
        Shape::Fill { rings } => {
            rings.iter().flatten().any(|p| contains(bx, *p))
                || corners.iter().any(|c| point_in_rings(rings, *c))
                || rings.iter().any(|r| (0..r.len()).any(|i| segment_touches(r[i], r[(i + 1) % r.len()], bx)))
        }
        Shape::Text { .. } => {
            let b = bounds(shape);
            !(b.max.x < bx.min.x || b.min.x > bx.max.x || b.max.y < bx.min.y || b.min.y > bx.max.y)
        }
        Shape::Lines { segments } => segments.iter().any(|(a, b)| segment_touches(*a, *b, bx)),
        s => prims(s).iter().any(|p| match *p {
            Prim::Seg { a, b, .. } => segment_touches(a, b, bx),
            Prim::Ray { a, u, ray } => crate::render::clip::clip_ray(a, u, ray, bx).is_some(),
            Prim::Arc {
                c,
                r,
                start,
                end,
                full,
            } => {
                // Facets fine enough that a box never slips between them.
                let sweep = if full { TAU } else { normalize_sweep(end - start) };
                let n = ((sweep / TAU) * 128.0).ceil().max(2.0) as usize;
                let pts: Vec<Vec2> = (0..=n)
                    .map(|i| c + Vec2::from_angle(start + sweep * i as f64 / n as f64) * r)
                    .collect();
                pts.windows(2).any(|w| segment_touches(w[0], w[1], bx))
            }
        }),
    }
}

/// The entities in a selection box from `a` to `b`. A window (`crossing`
/// false, drawn left to right) takes what lies wholly inside; a crossing
/// box takes whatever it touches. Each entity is listed once, in drawing
/// order.
pub fn in_box(scene: &Scene, a: Vec2, b: Vec2, crossing: bool) -> Vec<Handle> {
    let mut bx = BBox::default();
    bx.add(a);
    bx.add(b);
    // Per source entity: whether every piece is inside, whether any touches.
    let mut order: Vec<Handle> = Vec::new();
    let mut state: std::collections::HashMap<Handle, (bool, bool)> = Default::default();
    for (i, it) in scene.items.iter().enumerate() {
        let ib = scene.item_box(i);
        let inside = !matches!(it.shape, Shape::Ray { .. }) && !ib.is_empty() && contains(&bx, ib.min) && contains(&bx, ib.max);
        let touch = inside || (crossing && touches(&it.shape, &bx));
        let e = state.entry(it.source).or_insert_with(|| {
            order.push(it.source);
            (true, false)
        });
        e.0 &= inside;
        e.1 |= touch;
    }
    order
        .into_iter()
        .filter(|h| {
            let (all_inside, any_touch) = state[h];
            if crossing {
                any_touch
            } else {
                all_inside
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, Lineweight};
    use crate::render::{Item, Style};

    fn item(shape: Shape, h: u64) -> Item {
        Item {
            shape,
            style: Style {
                rgb: (255, 255, 255),
                aci: 7,
                color: Color::Index(7),
                layer: "0".into(),
                lineweight: Lineweight::Default,
                linetype: String::new(),
                pattern: Vec::new(),
            },
            source: Handle(h),
        }
    }

    fn scene() -> Scene {
        let mut s = Scene::default();
        s.items.push(item(
            Shape::Line {
                a: Vec2::new(0.0, 0.0),
                b: Vec2::new(120.0, 0.0),
            },
            1,
        ));
        s.items.push(item(
            Shape::Line {
                a: Vec2::new(50.0, -50.0),
                b: Vec2::new(50.0, 30.0),
            },
            2,
        ));
        s.items.push(item(
            Shape::Circle {
                center: Vec2::new(200.0, 0.0),
                radius: 20.0,
            },
            3,
        ));
        s.items.push(item(
            Shape::Point {
                at: Vec2::new(300.0, 300.0),
            },
            4,
        ));
        s
    }

    fn first(s: &Scene, x: f64, y: f64) -> Snap {
        snaps(s, Vec2::new(x, y), 5.0, kind::ALL)[0]
    }

    #[test]
    fn each_kind_is_found() {
        let s = scene();
        let e = first(&s, 118.0, 1.0);
        assert_eq!((e.kind, e.point, e.source), (kind::ENDPOINT, Vec2::new(120.0, 0.0), Handle(1)));
        assert_eq!(first(&s, 61.0, 1.0).point, Vec2::new(60.0, 0.0));
        assert_eq!(first(&s, 51.0, -11.0).kind, kind::MIDPOINT);
        let i = first(&s, 51.0, 1.0);
        assert_eq!((i.kind, i.point), (kind::INTERSECTION, Vec2::new(50.0, 0.0)));
        assert_eq!(first(&s, 201.0, 1.0).kind, kind::CENTER);
        let q = first(&s, 221.0, 1.0);
        assert_eq!((q.kind, q.point), (kind::QUADRANT, Vec2::new(220.0, 0.0)));
        assert_eq!(first(&s, 301.0, 299.0).kind, kind::NODE);
        let n = first(&s, 75.0, 2.0);
        assert_eq!((n.kind, n.point), (kind::NEAREST, Vec2::new(75.0, 0.0)));
        assert!(snaps(&s, Vec2::new(500.0, 500.0), 5.0, kind::ALL).is_empty());
        // Only the running kinds.
        assert_eq!(snaps(&s, Vec2::new(75.0, 2.0), 5.0, kind::DEFAULT), vec![]);
    }

    #[test]
    fn line_and_circle_cross() {
        let mut s = Scene::default();
        s.items.push(item(
            Shape::Circle {
                center: Vec2::new(0.0, 0.0),
                radius: 10.0,
            },
            1,
        ));
        s.items.push(item(
            Shape::Line {
                a: Vec2::new(-20.0, 6.0),
                b: Vec2::new(20.0, 6.0),
            },
            2,
        ));
        let i = snaps(&s, Vec2::new(8.5, 5.0), 3.0, kind::INTERSECTION);
        assert_eq!(i.len(), 1);
        assert!((i[0].point - Vec2::new(8.0, 6.0)).length() < 1e-9);
    }

    #[test]
    fn picking_and_boxes() {
        let s = scene();
        assert_eq!(pick(&s, Vec2::new(30.0, 0.5), 2.0), Some(Handle(1)));
        assert_eq!(pick(&s, Vec2::new(219.5, 0.0), 2.0), Some(Handle(3)));
        assert_eq!(pick(&s, Vec2::new(200.0, 0.0), 2.0), None, "inside a circle is not on it");
        // Window: only what lies wholly inside.
        let w = in_box(&s, Vec2::new(-10.0, -10.0), Vec2::new(130.0, 10.0), false);
        assert_eq!(w, vec![Handle(1)]);
        // Crossing: what it touches.
        let c = in_box(&s, Vec2::new(40.0, -10.0), Vec2::new(60.0, 10.0), true);
        assert_eq!(c, vec![Handle(1), Handle(2)]);
        // A box inside a circle does not touch it.
        assert!(in_box(&s, Vec2::new(195.0, -5.0), Vec2::new(205.0, 5.0), true).is_empty());
        assert_eq!(in_box(&s, Vec2::new(215.0, -5.0), Vec2::new(225.0, 5.0), true), vec![Handle(3)]);
    }
}

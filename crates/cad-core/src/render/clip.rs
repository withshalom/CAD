//! Clipping shapes to a rectangle: what a layout's viewport shows of model
//! space. Lines, arcs and segments are cut exactly at the edges; fills are
//! clipped ring by ring; text becomes the strokes of the app's font first,
//! so a word half outside the window is cut like any other line.

use super::Shape;
use crate::font;
use crate::geom::curve::{bulge_to_arc, normalize_angle, normalize_sweep, tessellate_bulge_polyline, DEFAULT_STEP};
use crate::geom::{BBox, Vec2};
use std::f64::consts::TAU;

fn inside(r: &BBox, p: Vec2) -> bool {
    p.x >= r.min.x && p.x <= r.max.x && p.y >= r.min.y && p.y <= r.max.y
}

/// The part of segment `a`–`b` inside `r` (Liang–Barsky), as parameters.
pub fn segment_params(a: Vec2, b: Vec2, r: &BBox) -> Option<(f64, f64)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-d.x, a.x - r.min.x),
        (d.x, r.max.x - a.x),
        (-d.y, a.y - r.min.y),
        (d.y, r.max.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some((t0, t1))
}

fn clip_segment(a: Vec2, b: Vec2, r: &BBox) -> Option<(Vec2, Vec2)> {
    let (t0, t1) = segment_params(a, b, r)?;
    let d = b - a;
    Some((a + d * t0, a + d * t1))
}

/// A line through `origin` along `dir` (from `origin` on when `ray`) cut
/// to `r`.
pub fn clip_ray(origin: Vec2, dir: Vec2, ray: bool, r: &BBox) -> Option<(Vec2, Vec2)> {
    let len = dir.length();
    if len < 1e-12 || r.is_empty() {
        return None;
    }
    let u = dir / len;
    // Far enough to cross the whole rectangle from anywhere near it.
    let reach = (r.max - r.min).length() + (origin - r.center()).length() + 1.0;
    let a = if ray { origin } else { origin - u * reach };
    clip_segment(a, origin + u * reach, r)
}

/// Angles (radians) where the circle crosses the rectangle's edges.
fn circle_crossings(c: Vec2, radius: f64, r: &BBox) -> Vec<f64> {
    let mut out = Vec::new();
    for (x, vertical) in [(r.min.x, true), (r.max.x, true), (r.min.y, false), (r.max.y, false)] {
        let d = if vertical { x - c.x } else { x - c.y };
        if d.abs() > radius {
            continue;
        }
        let h = (radius * radius - d * d).max(0.0).sqrt();
        for s in [h, -h] {
            let p = if vertical { Vec2::new(x, c.y + s) } else { Vec2::new(c.x + s, x) };
            out.push((p - c).angle());
        }
    }
    out
}

/// The parts of the arc from `start` counter-clockwise to `end` (a full
/// circle when `full`) inside `r`, as (start, end) angle pairs.
fn clip_arc(c: Vec2, radius: f64, start: f64, end: f64, full: bool, r: &BBox) -> Vec<(f64, f64)> {
    let sweep = if full { TAU } else { normalize_sweep(end - start) };
    let mut cuts: Vec<f64> = circle_crossings(c, radius, r)
        .into_iter()
        .map(|a| normalize_angle(a - start))
        .filter(|t| *t > 1e-12 && *t < sweep - 1e-12)
        .collect();
    cuts.push(0.0);
    cuts.push(sweep);
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for w in cuts.windows(2) {
        let mid = start + (w[0] + w[1]) / 2.0;
        if inside(r, c + Vec2::from_angle(mid) * radius) {
            match out.last_mut() {
                Some(last) if (last.1 - (start + w[0])).abs() < 1e-12 => last.1 = start + w[1],
                _ => out.push((start + w[0], start + w[1])),
            }
        }
    }
    // A whole circle cut nowhere stays a circle.
    if full && out.len() == 2 && (out[0].0 - start).abs() < 1e-12 && (out[1].1 - start - TAU).abs() < 1e-12 {
        let (a, b) = (out[1].0, out[0].1 + TAU);
        out = vec![(a, b)];
    }
    out
}

fn arc_shape(center: Vec2, radius: f64, a: f64, b: f64) -> Shape {
    if (b - a - TAU).abs() < 1e-9 {
        Shape::Circle { center, radius }
    } else {
        Shape::Arc {
            center,
            radius,
            start: a,
            end: b,
        }
    }
}

/// Points of a path kept inside `r`, as runs.
fn clip_path(points: &[Vec2], closed: bool, r: &BBox, out: &mut Vec<Shape>) {
    let n = points.len();
    if n == 1 {
        if inside(r, points[0]) {
            out.push(Shape::Path {
                points: points.to_vec(),
                closed: false,
            });
        }
        return;
    }
    let segs = if closed { n } else { n.saturating_sub(1) };
    let mut run: Vec<Vec2> = Vec::new();
    for i in 0..segs {
        let (a, b) = (points[i], points[(i + 1) % n]);
        match clip_segment(a, b, r) {
            Some((p, q)) => {
                if run.last().is_some_and(|l| l.distance(p) < 1e-12) {
                    run.push(q);
                } else {
                    if run.len() >= 2 {
                        out.push(Shape::Path {
                            points: std::mem::take(&mut run),
                            closed: false,
                        });
                    }
                    run = vec![p, q];
                }
            }
            None => {
                if run.len() >= 2 {
                    out.push(Shape::Path {
                        points: std::mem::take(&mut run),
                        closed: false,
                    });
                }
                run.clear();
            }
        }
    }
    if run.len() >= 2 {
        // A closed path wholly inside stays closed.
        let whole = closed && run.len() == n + 1 && run[0].distance(run[n]) < 1e-12;
        if whole {
            run.pop();
        }
        out.push(Shape::Path {
            points: run,
            closed: whole,
        });
    }
}

/// One side of the clip rectangle: whether a point is on its inner side,
/// and where the segment between two points crosses it.
type ClipEdge = (fn(Vec2, &BBox) -> bool, fn(Vec2, Vec2, &BBox) -> Vec2);

/// Sutherland–Hodgman: a ring clipped to the rectangle.
fn clip_ring(ring: &[Vec2], r: &BBox) -> Vec<Vec2> {
    let mut poly = ring.to_vec();
    let edges: [ClipEdge; 4] = [
        (|p, r| p.x >= r.min.x, |a, b, r| lerp_x(a, b, r.min.x)),
        (|p, r| p.x <= r.max.x, |a, b, r| lerp_x(a, b, r.max.x)),
        (|p, r| p.y >= r.min.y, |a, b, r| lerp_y(a, b, r.min.y)),
        (|p, r| p.y <= r.max.y, |a, b, r| lerp_y(a, b, r.max.y)),
    ];
    for (keep, cross) in edges {
        if poly.is_empty() {
            break;
        }
        let input = std::mem::take(&mut poly);
        let n = input.len();
        for i in 0..n {
            let (a, b) = (input[(i + n - 1) % n], input[i]);
            match (keep(a, r), keep(b, r)) {
                (true, true) => poly.push(b),
                (true, false) => poly.push(cross(a, b, r)),
                (false, true) => {
                    poly.push(cross(a, b, r));
                    poly.push(b);
                }
                (false, false) => {}
            }
        }
    }
    poly
}

fn lerp_x(a: Vec2, b: Vec2, x: f64) -> Vec2 {
    let t = if (b.x - a.x).abs() < 1e-300 { 0.0 } else { (x - a.x) / (b.x - a.x) };
    Vec2::new(x, a.y + (b.y - a.y) * t)
}

fn lerp_y(a: Vec2, b: Vec2, y: f64) -> Vec2 {
    let t = if (b.y - a.y).abs() < 1e-300 { 0.0 } else { (y - a.y) / (b.y - a.y) };
    Vec2::new(a.x + (b.x - a.x) * t, y)
}

/// What of `shape` lies inside `r`.
pub fn shape_to_rect(shape: &Shape, r: &BBox) -> Vec<Shape> {
    let mut out = Vec::new();
    match shape {
        Shape::Line { a, b } => {
            if let Some((p, q)) = clip_segment(*a, *b, r) {
                out.push(Shape::Line { a: p, b: q });
            }
        }
        Shape::Circle { center, radius } => {
            for (a, b) in clip_arc(*center, *radius, 0.0, TAU, true, r) {
                out.push(arc_shape(*center, *radius, a, b));
            }
        }
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            for (a, b) in clip_arc(*center, *radius, *start, *end, false, r) {
                out.push(Shape::Arc {
                    center: *center,
                    radius: *radius,
                    start: a,
                    end: b,
                });
            }
        }
        Shape::Polyline { vertices, closed } => {
            let n = vertices.len();
            if n == 1 {
                if inside(r, vertices[0].0) {
                    out.push(shape.clone());
                }
                return out;
            }
            let all_in = tessellate_bulge_polyline(vertices, *closed, DEFAULT_STEP * 4.0)
                .iter()
                .all(|p| inside(r, *p));
            if all_in {
                out.push(shape.clone());
                return out;
            }
            let segs = if *closed { n } else { n - 1 };
            for i in 0..segs {
                let (p1, b) = vertices[i];
                let p2 = vertices[(i + 1) % n].0;
                let piece = match bulge_to_arc(p1, p2, b) {
                    Some((arc, _)) => Shape::Arc {
                        center: arc.center,
                        radius: arc.radius,
                        start: arc.start,
                        end: arc.end,
                    },
                    None => Shape::Line { a: p1, b: p2 },
                };
                out.extend(shape_to_rect(&piece, r));
            }
        }
        Shape::Path { points, closed } => clip_path(points, *closed, r, &mut out),
        Shape::Fill { rings } => {
            let rings: Vec<Vec<Vec2>> = rings.iter().map(|ring| clip_ring(ring, r)).filter(|x| x.len() >= 3).collect();
            if !rings.is_empty() {
                out.push(Shape::Fill { rings });
            }
        }
        Shape::Point { at } => {
            if inside(r, *at) {
                out.push(shape.clone());
            }
        }
        Shape::Text { text, .. } => {
            if let Some(p) = shape.placement() {
                let corners = font::text_box(text, &p);
                if corners.iter().all(|c| inside(r, *c)) {
                    out.push(shape.clone());
                } else {
                    for stroke in font::layout(text, &p) {
                        clip_path(&stroke, false, r, &mut out);
                    }
                }
            }
        }
        Shape::Ray {
            origin,
            direction,
            ray,
        } => {
            if let Some((a, b)) = clip_ray(*origin, *direction, *ray, r) {
                out.push(Shape::Line { a, b });
            }
        }
        Shape::Lines { segments } => {
            let kept: Vec<(Vec2, Vec2)> = segments.iter().filter_map(|(a, b)| clip_segment(*a, *b, r)).collect();
            if !kept.is_empty() {
                out.push(Shape::Lines { segments: kept });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> BBox {
        BBox {
            min: Vec2::new(0.0, 0.0),
            max: Vec2::new(10.0, 10.0),
        }
    }

    #[test]
    fn lines_rays_and_arcs_are_cut_at_the_edges() {
        let r = rect();
        let s = shape_to_rect(&Shape::Line { a: Vec2::new(-5.0, 5.0), b: Vec2::new(5.0, 5.0) }, &r);
        assert_eq!(s, vec![Shape::Line { a: Vec2::new(0.0, 5.0), b: Vec2::new(5.0, 5.0) }]);
        let x = shape_to_rect(
            &Shape::Ray { origin: Vec2::new(5.0, 5.0), direction: Vec2::new(1.0, 0.0), ray: false },
            &r,
        );
        assert_eq!(x, vec![Shape::Line { a: Vec2::new(0.0, 5.0), b: Vec2::new(10.0, 5.0) }]);
        let ray = shape_to_rect(
            &Shape::Ray { origin: Vec2::new(5.0, 5.0), direction: Vec2::new(0.0, 1.0), ray: true },
            &r,
        );
        assert_eq!(ray, vec![Shape::Line { a: Vec2::new(5.0, 5.0), b: Vec2::new(5.0, 10.0) }]);
        // A circle crossing the left edge keeps the arc inside.
        let c = shape_to_rect(&Shape::Circle { center: Vec2::new(1.0, 5.0), radius: 2.0 }, &r);
        assert_eq!(c.len(), 1);
        match c[0] {
            Shape::Arc { start, end, .. } => {
                let sweep = normalize_sweep(end - start);
                assert!((sweep - (2.0 * std::f64::consts::PI - 2.0 * (60f64).to_radians())).abs() < 1e-9, "{sweep}");
            }
            ref s => panic!("{s:?}"),
        }
        // Inside: unchanged; outside: nothing.
        assert_eq!(shape_to_rect(&Shape::Circle { center: Vec2::new(5.0, 5.0), radius: 1.0 }, &r).len(), 1);
        assert!(shape_to_rect(&Shape::Circle { center: Vec2::new(50.0, 5.0), radius: 1.0 }, &r).is_empty());
    }

    #[test]
    fn fills_and_paths_are_clipped() {
        let r = rect();
        let f = shape_to_rect(
            &Shape::Fill {
                rings: vec![vec![Vec2::new(-5.0, -5.0), Vec2::new(5.0, -5.0), Vec2::new(5.0, 5.0), Vec2::new(-5.0, 5.0)]],
            },
            &r,
        );
        match &f[0] {
            Shape::Fill { rings } => assert!(rings[0].iter().all(|p| inside(&r, *p))),
            s => panic!("{s:?}"),
        }
        let p = shape_to_rect(
            &Shape::Path {
                points: vec![Vec2::new(-1.0, 1.0), Vec2::new(5.0, 1.0), Vec2::new(5.0, 20.0), Vec2::new(6.0, 20.0)],
                closed: false,
            },
            &r,
        );
        assert_eq!(p.len(), 1);
    }
}

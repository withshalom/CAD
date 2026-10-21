//! Linetype patterns: splitting shapes into dashes.
//!
//! A pattern is a list of lengths in world units: positive for a dash,
//! negative for a gap, zero for a dot. The core makes the dashes, so every
//! output (screen, SVG, PNG) shows the same ones.
//!
//! Open curves are dashed so they **start and end with a dash and the
//! pattern sits centred on them** (DESIGN.md §4): with a pattern that
//! starts with a dash, a feature of length L gets n = ⌊L / period⌋ whole
//! periods, and the two end dashes share what is left, so they are never
//! shorter than half the first dash. A centre line across a hole then
//! crosses at the short dash in the middle and ends in long dashes, and a
//! hidden line ends in dashes touching the outline. A feature shorter than
//! one period is drawn solid. Each segment of a polyline is a feature of
//! its own, so dashes meet at corners. A pattern that starts with a gap or
//! a dot repeats from the start of the feature instead.
//!
//! Closed curves (circles, closed ellipses and splines) fit a whole number
//! of periods, stretched slightly, with a dash centred on the start point.
//! A feature that would take more than [`MAX_DASHES`] dashes is drawn
//! solid, as CAD programs do when a pattern is far too small for the view.
//! So is any pattern holding a length that is not a finite number.

use super::Shape;
use crate::geom::curve::{bulge_to_arc, ArcSeg};
use crate::geom::Vec2;
use std::f64::consts::TAU;

/// Most dashes made for one feature (a line, an arc, one segment of a
/// polyline, a whole path or circle) before it is drawn solid instead.
pub const MAX_DASHES: usize = 20_000;

/// True when the pattern draws anything but a solid line: it has a gap,
/// every length is a finite number, and the period is not vanishingly
/// small (or too large to add up).
pub fn is_dashed(pattern: &[f64]) -> bool {
    let p = period(pattern);
    pattern.iter().all(|v| v.is_finite())
        && pattern.iter().any(|&v| v < 0.0)
        && p.is_finite()
        && p > 1e-12
}

fn period(pattern: &[f64]) -> f64 {
    pattern.iter().map(|v| v.abs()).sum()
}

/// Intervals `[start, end]` along an open feature of length `len` that are
/// drawn (equal ends are dots).
pub fn open_intervals(len: f64, pattern: &[f64]) -> Option<Vec<(f64, f64)>> {
    let p = period(pattern);
    if !is_dashed(pattern) || !len.is_finite() || len <= 0.0 {
        return None;
    }
    let periods = len / p;
    if periods * pattern.len() as f64 > MAX_DASHES as f64 {
        return None;
    }
    let mut out = Vec::new();
    let first = pattern[0];
    if first > 0.0 {
        // Centred: end dashes share the remainder.
        let n = (len / p).floor();
        if n < 1.0 {
            return None;
        }
        let rest = len - n * p - first;
        let end = first + rest / 2.0;
        out.push((0.0, end));
        let mut pos = end;
        for k in 0..n as usize {
            for &e in &pattern[1..] {
                if e > 0.0 {
                    out.push((pos, pos + e));
                } else if e == 0.0 {
                    out.push((pos, pos));
                }
                pos += e.abs();
            }
            let last = k + 1 == n as usize;
            let d = if last { len - pos } else { first };
            out.push((pos, (pos + d).min(len)));
            pos += d;
        }
    } else {
        // A pattern that starts with a gap or a dot repeats from the start.
        if len < p {
            return None;
        }
        let mut pos = 0.0;
        'outer: loop {
            for &e in pattern {
                if pos >= len {
                    break 'outer;
                }
                if e > 0.0 {
                    out.push((pos, (pos + e).min(len)));
                } else if e == 0.0 {
                    out.push((pos, pos));
                }
                pos += e.abs();
            }
        }
    }
    Some(out)
}

/// Intervals along a closed feature of length `len`, as fractions of the
/// way round; the first dash is centred on the start.
pub fn closed_intervals(len: f64, pattern: &[f64]) -> Option<Vec<(f64, f64)>> {
    let p = period(pattern);
    if !is_dashed(pattern) || !len.is_finite() || len <= 0.0 {
        return None;
    }
    let n = (len / p).round();
    if n < 1.0 || n * pattern.len() as f64 > MAX_DASHES as f64 {
        return None;
    }
    let s = len / (n * p);
    let shift = if pattern[0] > 0.0 {
        pattern[0] * s / 2.0
    } else {
        0.0
    };
    let mut out = Vec::new();
    let mut pos = -shift;
    for _ in 0..n as usize {
        for &e in pattern {
            let l = e.abs() * s;
            if e > 0.0 {
                out.push(((pos) / len, (pos + l) / len));
            } else if e == 0.0 {
                out.push((pos / len, pos / len));
            }
            pos += l;
        }
    }
    Some(out)
}

/// A piece of an arc from angle `a0` to `a1`. A piece too narrow in angle
/// to be told from its chord is drawn as the chord: an arc whose end angle
/// equals its start (within [`normalize_sweep`]'s 1e-12) is a full circle
/// to every output, which a dash on a nearly straight bulge segment of
/// huge radius must not become. Dots come out as zero-length lines.
///
/// [`normalize_sweep`]: crate::geom::curve::normalize_sweep
fn arc_piece(c: Vec2, r: f64, a0: f64, a1: f64) -> Shape {
    if (a1 - a0).abs() <= 1e-12 {
        Shape::Line {
            a: c + Vec2::from_angle(a0) * r,
            b: c + Vec2::from_angle(a1) * r,
        }
    } else {
        Shape::Arc {
            center: c,
            radius: r,
            start: a0,
            end: a1,
        }
    }
}

/// Dashes of a straight segment.
fn dash_line(a: Vec2, b: Vec2, pattern: &[f64], out: &mut Vec<Shape>) {
    let len = a.distance(b);
    match open_intervals(len, pattern) {
        None => out.push(Shape::Line { a, b }),
        Some(iv) => {
            let d = (b - a) / len;
            for (s, e) in iv {
                out.push(Shape::Line {
                    a: a + d * s,
                    b: a + d * e,
                });
            }
        }
    }
}

/// Dashes of an open arc, counter-clockwise from `start`.
fn dash_arc(arc: &ArcSeg, pattern: &[f64], out: &mut Vec<Shape>) {
    let sweep = arc.sweep();
    let len = arc.radius * sweep;
    match open_intervals(len, pattern) {
        None => out.push(arc_piece(
            arc.center,
            arc.radius,
            arc.start,
            arc.start + sweep,
        )),
        Some(iv) => {
            for (s, e) in iv {
                let a0 = arc.start + s / arc.radius;
                let a1 = arc.start + e / arc.radius;
                out.push(arc_piece(arc.center, arc.radius, a0, a1));
            }
        }
    }
}

/// Point at distance `t` along a polyline whose cumulative lengths are
/// `acc`.
fn point_at(points: &[Vec2], acc: &[f64], t: f64) -> (usize, Vec2) {
    let i = acc.partition_point(|&x| x < t).clamp(1, points.len() - 1);
    let seg = acc[i] - acc[i - 1];
    let f = if seg > 0.0 {
        (t - acc[i - 1]) / seg
    } else {
        0.0
    };
    (i, points[i - 1].lerp(points[i], f.clamp(0.0, 1.0)))
}

/// The part of a polyline between distances `s` and `e`.
fn sub_path(points: &[Vec2], acc: &[f64], s: f64, e: f64) -> Vec<Vec2> {
    let (i0, p0) = point_at(points, acc, s);
    let (i1, p1) = point_at(points, acc, e);
    let mut out = vec![p0];
    for p in points.iter().take(i1).skip(i0) {
        out.push(*p);
    }
    out.push(p1);
    out
}

fn dash_path(points: &[Vec2], closed: bool, pattern: &[f64], out: &mut Vec<Shape>) {
    let mut pts = points.to_vec();
    if closed && pts.len() > 1 && pts.first() != pts.last() {
        pts.push(pts[0]);
    }
    if pts.len() < 2 {
        out.push(Shape::Path {
            points: points.to_vec(),
            closed,
        });
        return;
    }
    let mut acc = Vec::with_capacity(pts.len());
    acc.push(0.0);
    for w in pts.windows(2) {
        let last = *acc.last().unwrap_or(&0.0);
        acc.push(last + w[0].distance(w[1]));
    }
    let len = *acc.last().unwrap_or(&0.0);
    let intervals = if closed {
        closed_intervals(len, pattern).map(|iv| {
            // Wrap the fractions round: pieces that cross the start split.
            let mut v = Vec::new();
            for (a, b) in iv {
                let (a, b) = (a * len, b * len);
                if a < 0.0 {
                    v.push((a + len, len));
                    v.push((0.0, b));
                } else {
                    v.push((a, b.min(len)));
                }
            }
            v
        })
    } else {
        open_intervals(len, pattern)
    };
    match intervals {
        None => out.push(Shape::Path {
            points: points.to_vec(),
            closed,
        }),
        Some(iv) => {
            for (s, e) in iv {
                out.push(Shape::Path {
                    points: sub_path(&pts, &acc, s, e),
                    closed: false,
                });
            }
        }
    }
}

/// Split a shape into the dashes of `pattern`. Shapes that take no
/// pattern (fills, points, text), and any shape when the pattern is
/// continuous, come back unchanged.
pub fn dash(shape: &Shape, pattern: &[f64]) -> Vec<Shape> {
    if !is_dashed(pattern) {
        return vec![shape.clone()];
    }
    let mut out = Vec::new();
    match shape {
        Shape::Line { a, b } => dash_line(*a, *b, pattern, &mut out),
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => dash_arc(
            &ArcSeg {
                center: *center,
                radius: *radius,
                start: *start,
                end: *end,
            },
            pattern,
            &mut out,
        ),
        Shape::Circle { center, radius } => match closed_intervals(radius * TAU, pattern) {
            None => out.push(shape.clone()),
            Some(iv) => {
                for (a, b) in iv {
                    out.push(arc_piece(*center, *radius, a * TAU, b * TAU));
                }
            }
        },
        Shape::Polyline { vertices, closed } => {
            let n = vertices.len();
            if n < 2 {
                return vec![shape.clone()];
            }
            let segs = if *closed { n } else { n - 1 };
            for i in 0..segs {
                let (p1, bulge) = vertices[i];
                let p2 = vertices[(i + 1) % n].0;
                match bulge_to_arc(p1, p2, bulge) {
                    None => {
                        if p1.distance(p2) > 0.0 {
                            dash_line(p1, p2, pattern, &mut out)
                        }
                    }
                    // The centred pattern is symmetric, so dashing the arc
                    // counter-clockwise gives the same dashes as following
                    // the polyline's direction.
                    Some((arc, _)) => dash_arc(&arc, pattern, &mut out),
                }
            }
        }
        Shape::Path { points, closed } => dash_path(points, *closed, pattern, &mut out),
        // Construction lines are dashed by the output that cuts them to
        // its view; a hatch's pattern carries its own dashes.
        Shape::Fill { .. } | Shape::Point { .. } | Shape::Text { .. } | Shape::Ray { .. } | Shape::Lines { .. } => {
            out.push(shape.clone())
        }
    }
    out
}

/// Drawn length of a line or arc piece.
#[cfg(test)]
fn arc_len(s: &Shape) -> f64 {
    match s {
        Shape::Arc {
            radius, start, end, ..
        } => radius * crate::geom::curve::normalize_sweep(end - start),
        Shape::Line { a, b } => a.distance(*b),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTER: [f64; 4] = [12.0, -1.5, 3.0, -1.5];
    const HIDDEN: [f64; 2] = [3.0, -1.5];

    fn lengths(iv: &[(f64, f64)]) -> Vec<f64> {
        iv.iter().map(|(a, b)| b - a).collect()
    }

    #[test]
    fn centred_pattern_starts_and_ends_with_a_dash() {
        // 19 mm centre line across a 13 mm hole (3 mm past each side):
        // one period, long ends, short dash in the middle.
        let iv = open_intervals(19.0, &CENTER).unwrap();
        let l = lengths(&iv);
        assert_eq!(l.len(), 3);
        assert!((l[0] - 6.5).abs() < 1e-12 && (l[2] - 6.5).abs() < 1e-12);
        assert!((l[1] - 3.0).abs() < 1e-12);
        let mid = (iv[1].0 + iv[1].1) / 2.0;
        assert!((mid - 9.5).abs() < 1e-12, "short dash centred: {mid}");
        assert_eq!(iv[0].0, 0.0);
        assert!((iv[2].1 - 19.0).abs() < 1e-12);

        // Longer: interior long dashes keep their length, ends share the
        // rest equally.
        let iv = open_intervals(100.0, &CENTER).unwrap();
        let l = lengths(&iv);
        assert!((l[0] - l[l.len() - 1]).abs() < 1e-9);
        assert!(l[0] >= 6.0 - 1e-9);
        for x in &l[1..l.len() - 1] {
            assert!((x - 12.0).abs() < 1e-9 || (x - 3.0).abs() < 1e-9, "{x}");
        }
        assert!((iv.last().unwrap().1 - 100.0).abs() < 1e-9);

        // Hidden lines end in dashes touching both ends.
        for len in [5.0, 10.0, 47.3, 200.0] {
            let iv = open_intervals(len, &HIDDEN).unwrap();
            assert_eq!(iv[0].0, 0.0);
            assert!((iv.last().unwrap().1 - len).abs() < 1e-9);
            assert!(lengths(&iv).iter().all(|&d| d >= 1.5 - 1e-9));
        }
        // Too short for one period: solid.
        assert_eq!(open_intervals(17.0, &CENTER), None);
        assert_eq!(open_intervals(4.0, &HIDDEN), None);
        // Continuous patterns: solid.
        assert_eq!(open_intervals(100.0, &[]), None);
        assert_eq!(open_intervals(100.0, &[5.0]), None);
    }

    #[test]
    fn closed_curves_fit_whole_periods() {
        let c = Shape::Circle {
            center: Vec2::ZERO,
            radius: 10.0,
        };
        let pieces = dash(&c, &HIDDEN);
        // 2π·10 / 4.5 ≈ 13.96 → 14 periods, 14 dashes.
        assert_eq!(pieces.len(), 14);
        let total: f64 = pieces.iter().map(arc_len).sum();
        let expected = 10.0 * TAU * 3.0 / 4.5;
        assert!((total - expected).abs() < 1e-9, "{total} vs {expected}");
        // The first dash is centred on angle 0.
        match pieces[0] {
            Shape::Arc { start, end, .. } => assert!((start + end).abs() < 1e-12),
            _ => panic!(),
        }
    }

    #[test]
    fn polylines_dash_each_segment_and_arcs_follow_bulges() {
        let pl = Shape::Polyline {
            vertices: vec![
                (Vec2::new(0.0, 0.0), 0.0),
                (Vec2::new(30.0, 0.0), 1.0),
                (Vec2::new(30.0, 30.0), 0.0),
            ],
            closed: false,
        };
        let pieces = dash(&pl, &HIDDEN);
        // Segment one ends exactly at the corner, segment two (a half
        // circle) starts there.
        let corner = Vec2::new(30.0, 0.0);
        let touches = pieces.iter().filter(|s| match s {
            Shape::Line { b, .. } => b.distance(corner) < 1e-9,
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let p0 = *center + Vec2::from_angle(*start) * *radius;
                let p1 = *center + Vec2::from_angle(*end) * *radius;
                p0.distance(corner) < 1e-9 || p1.distance(corner) < 1e-9
            }
            _ => false,
        });
        assert_eq!(touches.count(), 2);
        assert!(pieces.iter().any(|s| matches!(s, Shape::Arc { .. })));
    }

    #[test]
    fn paths_dots_and_limits() {
        let path = Shape::Path {
            points: vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)],
            closed: false,
        };
        let pieces = dash(&path, &HIDDEN);
        assert!(pieces.len() > 3);
        let total: f64 = pieces
            .iter()
            .map(|s| match s {
                Shape::Path { points, .. } => points.windows(2).map(|w| w[0].distance(w[1])).sum(),
                _ => 0.0,
            })
            .sum();
        assert!(total > 10.0 && total < 20.0);
        // Dot-dash: dots come out as zero-length lines.
        let dots = dash(
            &Shape::Line {
                a: Vec2::ZERO,
                b: Vec2::new(20.0, 0.0),
            },
            &[0.0, -2.0],
        );
        assert!(dots
            .iter()
            .all(|s| matches!(s, Shape::Line { a, b } if a == b)));
        // Far too dense: drawn solid.
        let long = Shape::Line {
            a: Vec2::ZERO,
            b: Vec2::new(1e9, 0.0),
        };
        assert_eq!(dash(&long, &HIDDEN), vec![long.clone()]);
        // Text and fills pass through.
        let p = Shape::Point { at: Vec2::ZERO };
        assert_eq!(dash(&p, &HIDDEN), vec![p]);
    }

    #[test]
    fn bad_patterns_draw_solid_and_never_hang() {
        let line = Shape::Line {
            a: Vec2::ZERO,
            b: Vec2::new(100.0, 0.0),
        };
        for bad in [
            vec![-1.0, f64::NAN],
            vec![f64::NAN, -1.0],
            vec![3.0, f64::INFINITY, -1.5],
            vec![0.0, f64::NEG_INFINITY],
            vec![1e308, -1e308, 1e308],
        ] {
            assert!(!is_dashed(&bad), "{bad:?}");
            assert_eq!(dash(&line, &bad), vec![line.clone()], "{bad:?}");
        }
        // Patterns starting with a gap or a dot: shorter than one period
        // is solid too, not nothing or a lone dot.
        assert_eq!(open_intervals(0.5, &[-1.0, 2.0]), None);
        assert_eq!(open_intervals(1.0, &[0.0, -2.0]), None);
        let iv = open_intervals(7.0, &[-1.0, 2.0]).unwrap();
        assert_eq!(iv, vec![(1.0, 3.0), (4.0, 6.0)]);
    }

    #[test]
    fn nearly_straight_arcs_dash_into_chords_not_circles() {
        // A 10 m segment with a bulge of 1e-10: radius 2.5e13, every dash
        // far narrower in angle than 1e-12.
        let pl = Shape::Polyline {
            vertices: vec![(Vec2::ZERO, 1e-10), (Vec2::new(10_000.0, 0.0), 0.0)],
            closed: false,
        };
        let pieces = dash(&pl, &HIDDEN);
        assert!(pieces.len() > 2000);
        for s in &pieces {
            match s {
                // On the segment, to the precision a radius of 2.5e13
                // leaves (micrometres).
                Shape::Line { a, b } => {
                    for p in [a, b] {
                        assert!(p.x > -0.01 && p.x < 10_000.01 && p.y.abs() < 0.01, "{p:?}");
                    }
                }
                Shape::Arc { start, end, .. } => {
                    assert!(
                        (end - start).abs() > 1e-12,
                        "a piece that reads as a circle"
                    )
                }
                other => panic!("{other:?}"),
            }
        }
    }
}

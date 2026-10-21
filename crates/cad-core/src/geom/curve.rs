//! Curve helpers: polyline bulges, arc and ellipse tessellation, and NURBS
//! evaluation.

use super::{Vec2, Vec3, EPS};
use std::f64::consts::{PI, TAU};

/// A circular arc described by centre, radius and start/end angles
/// (radians). The arc runs counter-clockwise from `start` to `end`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcSeg {
    pub center: Vec2,
    pub radius: f64,
    pub start: f64,
    pub end: f64,
}

impl ArcSeg {
    /// Counter-clockwise sweep in (0, 2π].
    pub fn sweep(&self) -> f64 {
        normalize_sweep(self.end - self.start)
    }
    pub fn point_at(&self, angle: f64) -> Vec2 {
        self.center + Vec2::from_angle(angle) * self.radius
    }
}

/// Map an angle difference into (0, 2π]; a zero difference means a full turn.
pub fn normalize_sweep(mut d: f64) -> f64 {
    if !d.is_finite() {
        return TAU;
    }
    d %= TAU;
    if d <= 1e-12 {
        d += TAU;
    }
    d
}

pub fn normalize_angle(a: f64) -> f64 {
    let mut a = a % TAU;
    if a < 0.0 {
        a += TAU;
    }
    a
}

/// Convert a polyline segment with a bulge into an arc. A bulge is the
/// tangent of a quarter of the included angle; positive bulges turn
/// counter-clockwise. Returns the arc plus whether it runs clockwise from
/// `p1` to `p2` (in which case start/end are swapped so the arc itself is
/// still expressed counter-clockwise). `None` for straight segments.
pub fn bulge_to_arc(p1: Vec2, p2: Vec2, bulge: f64) -> Option<(ArcSeg, bool)> {
    if bulge.abs() < 1e-12 || !bulge.is_finite() {
        return None;
    }
    let chord = p2 - p1;
    let d = chord.length();
    if d < EPS {
        return None;
    }
    let u = chord / d;
    let h = (d / 2.0) * (1.0 - bulge * bulge) / (2.0 * bulge);
    let center = (p1 + p2) * 0.5 + u.perp() * h;
    let radius = d * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
    let a1 = (p1 - center).angle();
    let a2 = (p2 - center).angle();
    if bulge > 0.0 {
        Some((
            ArcSeg {
                center,
                radius,
                start: a1,
                end: a2,
            },
            false,
        ))
    } else {
        Some((
            ArcSeg {
                center,
                radius,
                start: a2,
                end: a1,
            },
            true,
        ))
    }
}

/// Number of segments for a sweep so each covers at most `max_step`
/// radians.
pub fn segment_count(sweep: f64, max_step: f64) -> usize {
    let n = (sweep.abs() / max_step).ceil();
    if n.is_finite() {
        (n as usize).clamp(1, 4096)
    } else {
        1
    }
}

/// Default angular step for tessellation: 2° per segment.
pub const DEFAULT_STEP: f64 = PI / 90.0;

/// Points along an arc, including both ends, counter-clockwise.
pub fn tessellate_arc(arc: &ArcSeg, max_step: f64) -> Vec<Vec2> {
    let sweep = arc.sweep();
    let n = segment_count(sweep, max_step);
    (0..=n)
        .map(|i| arc.point_at(arc.start + sweep * (i as f64) / (n as f64)))
        .collect()
}

/// Points along a polyline with bulges. Vertices are `(point, bulge)`
/// where the bulge applies to the segment leaving that vertex.
pub fn tessellate_bulge_polyline(verts: &[(Vec2, f64)], closed: bool, max_step: f64) -> Vec<Vec2> {
    let mut out = Vec::new();
    if verts.is_empty() {
        return out;
    }
    let n = verts.len();
    let segs = if closed { n } else { n - 1 };
    out.push(verts[0].0);
    for i in 0..segs {
        let (p1, b) = verts[i];
        let p2 = verts[(i + 1) % n].0;
        match bulge_to_arc(p1, p2, b) {
            None => out.push(p2),
            Some((arc, reversed)) => {
                let mut pts = tessellate_arc(&arc, max_step);
                if reversed {
                    pts.reverse();
                }
                out.extend_from_slice(&pts[1..]);
            }
        }
    }
    out
}

/// Points of an ellipse in 3D: centre, major-axis vector (full length to
/// the ellipse), minor-axis vector, parameter range. `end <= start` wraps.
pub fn tessellate_ellipse(
    center: Vec3,
    major: Vec3,
    minor: Vec3,
    start: f64,
    end: f64,
    max_step: f64,
) -> Vec<Vec3> {
    let sweep = normalize_sweep(end - start);
    let n = segment_count(sweep, max_step);
    (0..=n)
        .map(|i| {
            let t = start + sweep * (i as f64) / (n as f64);
            center + major * t.cos() + minor * t.sin()
        })
        .collect()
}

/// Evaluate a (possibly rational) B-spline at parameter `u` with de Boor's
/// algorithm. `weights` may be empty for a non-rational curve.
pub fn nurbs_point(
    degree: usize,
    knots: &[f64],
    ctrl: &[Vec3],
    weights: &[f64],
    u: f64,
) -> Option<Vec3> {
    let n = ctrl.len();
    if n == 0 || degree == 0 || knots.len() < n + degree + 1 {
        return None;
    }
    let p = degree;
    // Find the knot span k with knots[k] <= u < knots[k+1], clamped to the
    // valid range [p, n-1].
    let mut k = p;
    while k < n - 1 && u >= knots[k + 1] {
        k += 1;
    }
    let rational = weights.len() == n;
    // Homogeneous control points.
    let mut d: Vec<[f64; 4]> = (0..=p)
        .map(|j| {
            let i = k + j - p;
            let w = if rational { weights[i] } else { 1.0 };
            let c = ctrl[i];
            [c.x * w, c.y * w, c.z * w, w]
        })
        .collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = k + j - p;
            let denom = knots[i + p + 1 - r] - knots[i];
            let alpha = if denom.abs() < EPS {
                0.0
            } else {
                (u - knots[i]) / denom
            };
            let prev = d[j - 1];
            for (c, v) in d[j].iter_mut().enumerate() {
                *v = (1.0 - alpha) * prev[c] + alpha * *v;
            }
        }
    }
    let h = d[p];
    if h[3].abs() < EPS {
        return None;
    }
    Some(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]))
}

/// Sample a B-spline over its valid parameter range.
pub fn tessellate_nurbs(degree: usize, knots: &[f64], ctrl: &[Vec3], weights: &[f64]) -> Vec<Vec3> {
    let n = ctrl.len();
    if n < 2 || degree == 0 || knots.len() < n + degree + 1 {
        return ctrl.to_vec();
    }
    let u0 = knots[degree];
    let u1 = knots[n];
    // Also rejects NaN knots.
    if u1.partial_cmp(&u0) != Some(std::cmp::Ordering::Greater) {
        return ctrl.to_vec();
    }
    let samples = (n * 16).clamp(32, 2048);
    let mut out = Vec::with_capacity(samples + 1);
    for i in 0..=samples {
        let mut u = u0 + (u1 - u0) * (i as f64) / (samples as f64);
        if i == samples {
            u = u1 - (u1 - u0) * 1e-12;
        }
        if let Some(p) = nurbs_point(degree, knots, ctrl, weights, u) {
            out.push(p);
        }
    }
    if let Some(last) = ctrl.last() {
        // Clamped splines end exactly on the last control point.
        if knots[n..].iter().all(|&k| (k - u1).abs() < 1e-12) {
            if let Some(tail) = out.last_mut() {
                *tail = *last;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn semicircle_bulge() {
        // Bulge 1 = 180° counter-clockwise from (0,0) to (2,0): centre (1,0),
        // radius 1, passing through (1,-1).
        let (arc, rev) = bulge_to_arc(Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), 1.0).unwrap();
        assert!(!rev);
        assert!(close(arc.center.x, 1.0) && close(arc.center.y, 0.0));
        assert!(close(arc.radius, 1.0));
        let mid = arc.point_at(arc.start + arc.sweep() / 2.0);
        assert!(close(mid.y, -1.0));
    }

    #[test]
    fn negative_bulge_goes_clockwise() {
        let (arc, rev) = bulge_to_arc(Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), -1.0).unwrap();
        assert!(rev);
        let mid = arc.point_at(arc.start + arc.sweep() / 2.0);
        assert!(close(mid.y, 1.0));
    }

    #[test]
    fn nurbs_line() {
        // Degree 1 clamped spline is the control polygon.
        let knots = [0.0, 0.0, 1.0, 1.0];
        let ctrl = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(4.0, 2.0, 0.0)];
        let p = nurbs_point(1, &knots, &ctrl, &[], 0.5).unwrap();
        assert!(close(p.x, 2.0) && close(p.y, 1.0));
    }
}

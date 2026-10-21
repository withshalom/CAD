//! Modify operations on entity geometry, shared by every app: moving,
//! rotating, mirroring and scaling any entity kind, offsetting, trimming,
//! extending, filleting, chamfering and stretching (COMMANDS.md §6).
//!
//! Everything works on entities in the XY plane of the world, as drafting
//! draws them. An entity whose object coordinate system is the world's
//! seen from below (extrusion (0, 0, −1), common in DWG files written by
//! mirroring) is first restated with the usual extrusion; other 3D
//! orientations are refused with a message.
//!
//! Trim and extend take their edges as [`Curve`]s, made from any entities
//! with [`curves_of`]; they report what they could not do as an error
//! message for the user, and change nothing then.

use crate::geom::curve::{bulge_to_arc, normalize_angle, normalize_sweep, tessellate_ellipse, tessellate_nurbs};
use crate::geom::{BBox, Vec2, Vec3};
use crate::model::*;
use std::f64::consts::{PI, TAU};

const TOL: f64 = 1e-9;

// ---------------------------------------------------------------------------
// Affine transforms
// ---------------------------------------------------------------------------

/// A similarity of the plane (rotation, reflection, uniform scale) and a
/// move: `p' = ax·p.x + ay·p.y + t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub ax: Vec2,
    pub ay: Vec2,
    pub t: Vec2,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        ax: Vec2 { x: 1.0, y: 0.0 },
        ay: Vec2 { x: 0.0, y: 1.0 },
        t: Vec2 { x: 0.0, y: 0.0 },
    };

    pub fn translate(d: Vec2) -> Affine {
        Affine { t: d, ..Affine::IDENTITY }
    }

    /// Rotation by `a` radians counter-clockwise about `c`.
    pub fn rotate(c: Vec2, a: f64) -> Affine {
        let (s, k) = a.sin_cos();
        let ax = Vec2::new(k, s);
        let ay = Vec2::new(-s, k);
        Affine { ax, ay, t: c - (ax * c.x + ay * c.y) }
    }

    /// Reflection across the line through `a` and `b`.
    pub fn mirror(a: Vec2, b: Vec2) -> Affine {
        let u = (b - a).normalized();
        let ax = Vec2::new(2.0 * u.x * u.x - 1.0, 2.0 * u.x * u.y);
        let ay = Vec2::new(2.0 * u.x * u.y, 2.0 * u.y * u.y - 1.0);
        Affine { ax, ay, t: a - (ax * a.x + ay * a.y) }
    }

    /// Uniform scale by `k` about `c`.
    pub fn scale(c: Vec2, k: f64) -> Affine {
        let ax = Vec2::new(k, 0.0);
        let ay = Vec2::new(0.0, k);
        Affine { ax, ay, t: c - c * k }
    }

    /// `self` after `first`.
    pub fn after(&self, first: &Affine) -> Affine {
        Affine {
            ax: self.v(first.ax),
            ay: self.v(first.ay),
            t: self.p(first.t),
        }
    }

    pub fn p(&self, p: Vec2) -> Vec2 {
        self.ax * p.x + self.ay * p.y + self.t
    }
    pub fn p3(&self, p: Vec3) -> Vec3 {
        self.p(p.xy()).extend(p.z)
    }
    /// The linear part on a vector.
    pub fn v(&self, v: Vec2) -> Vec2 {
        self.ax * v.x + self.ay * v.y
    }
    pub fn v3(&self, v: Vec3) -> Vec3 {
        self.v(v.xy()).extend(v.z)
    }
    pub fn reflects(&self) -> bool {
        self.ax.cross(self.ay) < 0.0
    }
    /// The scale factor (lengths are multiplied by it).
    pub fn k(&self) -> f64 {
        self.ax.cross(self.ay).abs().sqrt()
    }
    /// Where the direction at angle `a` goes.
    pub fn angle(&self, a: f64) -> f64 {
        self.v(Vec2::from_angle(a)).angle()
    }
}

fn is_z(n: Vec3) -> bool {
    n.x.abs() < 1e-12 && n.y.abs() < 1e-12 && n.z > 0.0
}

fn is_minus_z(n: Vec3) -> bool {
    n.x.abs() < 1e-12 && n.y.abs() < 1e-12 && n.z < 0.0
}

/// Restate an entity drawn in the world's XY plane seen from below
/// (extrusion (0, 0, −1)) with the usual extrusion, so its coordinates are
/// world coordinates. Other orientations are an error.
pub fn to_world_plane(kind: &mut ObjectKind) -> Result<(), String> {
    let flip = |p: Vec3| Vec3::new(-p.x, p.y, -p.z);
    match kind {
        ObjectKind::Circle(c) if !is_z(c.extrusion) => {
            if !is_minus_z(c.extrusion) {
                return Err(not_flat());
            }
            c.center = flip(c.center);
            c.extrusion = Vec3::Z;
        }
        ObjectKind::Arc(a) if !is_z(a.extrusion) => {
            if !is_minus_z(a.extrusion) {
                return Err(not_flat());
            }
            a.center = flip(a.center);
            // x → −x turns angle t into π − t and reverses the direction.
            let (s, e) = (PI - a.end_angle, PI - a.start_angle);
            a.start_angle = normalize_angle(s);
            a.end_angle = normalize_angle(e);
            a.extrusion = Vec3::Z;
        }
        ObjectKind::LwPolyline(p) if !is_z(p.extrusion) => {
            if !is_minus_z(p.extrusion) {
                return Err(not_flat());
            }
            for q in &mut p.points {
                q.x = -q.x;
            }
            for b in &mut p.bulges {
                *b = -*b;
            }
            p.elevation = -p.elevation;
            p.extrusion = Vec3::Z;
        }
        ObjectKind::Line(l) if !is_z(l.extrusion) && l.extrusion != Vec3::ZERO => {
            // Line points are world coordinates already; only thickness
            // follows the extrusion.
            l.extrusion = Vec3::Z;
        }
        ObjectKind::Text(t) if !is_z(t.extrusion) => {
            if !is_minus_z(t.extrusion) {
                return Err(not_flat());
            }
            t.insertion.x = -t.insertion.x;
            t.alignment.x = -t.alignment.x;
            t.rotation = PI - t.rotation;
            t.extrusion = Vec3::Z;
        }
        ObjectKind::Insert(i) if !is_z(i.extrusion) => {
            if !is_minus_z(i.extrusion) {
                return Err(not_flat());
            }
            i.insertion = flip(i.insertion);
            i.rotation = PI - i.rotation;
            i.scale.x = -i.scale.x;
            i.extrusion = Vec3::Z;
        }
        _ => {}
    }
    Ok(())
}

fn not_flat() -> String {
    "this object is not in the drawing's XY plane".into()
}

/// Map a dimension's points and angles.
fn transform_dimension(d: &mut Dimension, xf: &Affine) {
    d.pt10 = xf.p3(d.pt10);
    d.pt13 = xf.p3(d.pt13);
    d.pt14 = xf.p3(d.pt14);
    d.pt15 = xf.p3(d.pt15);
    d.pt16 = xf.p(d.pt16);
    if d.text_midpoint != Vec2::ZERO {
        d.text_midpoint = xf.p(d.text_midpoint);
    }
    if d.clone_insertion != Vec2::ZERO {
        d.clone_insertion = xf.p(d.clone_insertion);
    }
    d.dim_rotation = xf.angle(d.dim_rotation);
    d.ext_line_rotation = if d.ext_line_rotation != 0.0 { xf.angle(d.ext_line_rotation) } else { 0.0 };
    d.text_rotation = if d.text_rotation != 0.0 { xf.angle(d.text_rotation) } else { 0.0 };
    match d.kind {
        DimensionKind::Angular3Pt | DimensionKind::Angular2Line => {}
        _ => d.measurement *= xf.k(),
    }
    d.leader_length *= xf.k();
    // Graphics made for the old place are stale: drawn from the
    // definition points again.
    d.block = Handle::NULL;
}

fn transform_text(t: &mut Text, xf: &Affine) {
    t.insertion = xf.p(t.insertion);
    t.alignment = xf.p(t.alignment);
    t.height *= xf.k();
    // Mirrored text stays readable (MIRRTEXT 0): only its place moves.
    if !xf.reflects() {
        t.rotation = xf.angle(t.rotation);
    }
}

fn transform_edge(e: &mut HatchEdge, xf: &Affine) {
    let k = xf.k();
    match e {
        HatchEdge::Line { a, b } => {
            *a = xf.p(*a);
            *b = xf.p(*b);
        }
        HatchEdge::Arc {
            center,
            radius,
            start,
            end,
            ccw,
        } => {
            // Clockwise edges measure their angles clockwise.
            let (s, en) = if *ccw { (*start, *end) } else { (-*end, -*start) };
            let (mut s2, mut e2) = (xf.angle(s), xf.angle(en));
            if xf.reflects() {
                std::mem::swap(&mut s2, &mut e2);
            }
            *center = xf.p(*center);
            *radius *= k;
            if *ccw != xf.reflects() || *ccw {
                // Store counter-clockwise from here on.
            }
            *start = normalize_angle(s2);
            *end = normalize_angle(e2);
            if *end <= *start {
                *end += TAU;
            }
            // Direction of travel along the boundary reverses with a
            // reflection; the even-odd fill does not depend on it.
            *ccw = true;
        }
        HatchEdge::Ellipse {
            center,
            major,
            start,
            end,
            ccw,
            ..
        } => {
            *center = xf.p(*center);
            *major = xf.v(*major);
            if xf.reflects() {
                let (s, en) = (*start, *end);
                *start = normalize_angle(-en);
                *end = normalize_angle(-s);
                if *end <= *start {
                    *end += TAU;
                }
                let _ = ccw;
            }
        }
        HatchEdge::Spline { control_points, .. } => {
            for c in control_points {
                *c = xf.p(*c);
            }
        }
    }
}

/// Apply `xf` to an entity's geometry. Sizes scale with it (text heights,
/// radii, block scales); text stays readable under a reflection.
pub fn transform(kind: &mut ObjectKind, xf: &Affine) -> Result<(), String> {
    to_world_plane(kind)?;
    let k = xf.k();
    match kind {
        ObjectKind::Line(l) => {
            l.start = xf.p3(l.start);
            l.end = xf.p3(l.end);
        }
        ObjectKind::Circle(c) => {
            c.center = xf.p3(c.center);
            c.radius *= k;
        }
        ObjectKind::Arc(a) => {
            a.center = xf.p3(a.center);
            a.radius *= k;
            let (s, e) = (xf.angle(a.start_angle), xf.angle(a.end_angle));
            (a.start_angle, a.end_angle) = if xf.reflects() { (e, s) } else { (s, e) };
        }
        ObjectKind::Point(p) => p.position = xf.p3(p.position),
        ObjectKind::Ellipse(e) => {
            e.center = xf.p3(e.center);
            e.major_axis = xf.v3(e.major_axis);
            if xf.reflects() {
                let (s, en) = (e.start_param, e.end_param);
                let full = (normalize_sweep(en - s) - TAU).abs() < 1e-12;
                if !full {
                    e.start_param = normalize_angle(-en);
                    e.end_param = normalize_angle(-s);
                }
            }
        }
        ObjectKind::LwPolyline(p) => {
            for q in &mut p.points {
                *q = xf.p(*q);
            }
            if xf.reflects() {
                for b in &mut p.bulges {
                    *b = -*b;
                }
            }
            p.const_width *= k;
            for w in &mut p.widths {
                w.0 *= k;
                w.1 *= k;
            }
        }
        ObjectKind::Vertex(v) => {
            v.point = xf.p3(v.point);
            if xf.reflects() {
                v.bulge = -v.bulge;
            }
        }
        ObjectKind::Polyline(_) | ObjectKind::SeqEnd => {}
        ObjectKind::Text(t) => transform_text(t, xf),
        ObjectKind::Attribute(a) => transform_text(&mut a.text, xf),
        ObjectKind::MText(m) => {
            m.insertion = xf.p3(m.insertion);
            m.text_height *= k;
            m.rect_width *= k;
            m.rect_height *= k;
            if !xf.reflects() {
                let x = xf.v(m.x_axis.xy());
                if x.length() > 0.0 {
                    let u = x.normalized();
                    m.x_axis = Vec3::new(u.x, u.y, 0.0);
                }
            }
        }
        ObjectKind::Insert(i) => {
            i.insertion = xf.p3(i.insertion);
            if xf.reflects() {
                // R(2θ − r) S(sx, −sy) = R(2θ − r + π) S(−sx, sy).
                let dir = xf.angle(i.rotation);
                i.rotation = normalize_angle(dir + PI);
                i.scale.x = -i.scale.x;
            } else {
                i.rotation = normalize_angle(xf.angle(i.rotation));
            }
            i.scale = Vec3::new(i.scale.x * k, i.scale.y * k, i.scale.z * k);
            if let Some((c, r, dx, dy)) = i.array {
                i.array = Some((c, r, dx * k, dy * k));
            }
        }
        ObjectKind::Solid(s) => {
            for c in &mut s.corners {
                *c = xf.p(*c);
            }
        }
        ObjectKind::Face3d(f) => {
            for c in &mut f.corners {
                *c = xf.p3(*c);
            }
        }
        ObjectKind::Spline(sp) => {
            for c in &mut sp.control_points {
                *c = xf.p3(*c);
            }
            for c in &mut sp.fit_points {
                *c = xf.p3(*c);
            }
            sp.start_tangent = xf.v3(sp.start_tangent);
            sp.end_tangent = xf.v3(sp.end_tangent);
        }
        ObjectKind::Dimension(d) => transform_dimension(d, xf),
        ObjectKind::Hatch(h) => {
            if !is_z(h.extrusion) {
                return Err(not_flat());
            }
            for p in &mut h.paths {
                match p {
                    HatchPath::Polyline { vertices, .. } => {
                        for (q, b) in vertices {
                            *q = xf.p(*q);
                            if xf.reflects() {
                                *b = -*b;
                            }
                        }
                    }
                    HatchPath::Edges { edges, .. } => {
                        for e in edges {
                            transform_edge(e, xf);
                        }
                    }
                }
            }
            for l in &mut h.pattern_lines {
                l.base = xf.p(l.base);
                l.offset = xf.v(l.offset);
                l.angle = xf.angle(l.angle);
                for d in &mut l.dashes {
                    *d *= k;
                }
            }
            for s in &mut h.seed_points {
                *s = xf.p(*s);
            }
            h.pattern_scale *= k;
            if !h.pattern_lines.is_empty() || !xf.reflects() {
                h.pattern_angle = normalize_angle(xf.angle(h.pattern_angle) - xf.angle(0.0) + xf.angle(0.0));
            }
        }
        ObjectKind::Xline(x) | ObjectKind::Ray(x) => {
            x.base = xf.p3(x.base);
            let d = xf.v(x.direction.xy()).normalized();
            x.direction = Vec3::new(d.x, d.y, x.direction.z);
        }
        ObjectKind::Leader(l) => {
            for v in &mut l.vertices {
                *v = xf.p3(*v);
            }
            l.origin = xf.p3(l.origin);
            let x = xf.v(l.x_direction.xy()).normalized();
            if x.length() > 0.0 {
                l.x_direction = Vec3::new(x.x, x.y, 0.0);
            }
            l.block_offset = xf.v3(l.block_offset);
            l.annotation_offset = xf.v3(l.annotation_offset);
        }
        ObjectKind::PaperViewport(v) => {
            v.center = xf.p3(v.center);
            v.width *= k;
            v.height *= k;
            v.view_height *= 1.0;
            if (k - 1.0).abs() > 1e-12 {
                // Same scale on the paper: the viewport shows more or less.
                v.view_height *= k;
            }
        }
        other => {
            return Err(format!("moving {} objects is not built yet", kind_name(other)));
        }
    }
    Ok(())
}

fn kind_name(k: &ObjectKind) -> String {
    let s = format!("{k:?}");
    s.split(['(', ' ', '{']).next().unwrap_or("").to_string()
}

// ---------------------------------------------------------------------------
// Curves: what trim, extend and the joins of offset and fillet work on
// ---------------------------------------------------------------------------

/// A piece of an edge, in the world's XY plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve {
    /// A segment.
    Seg { a: Vec2, b: Vec2 },
    /// An arc counter-clockwise from `start` to `end` (radians); a whole
    /// circle when `full`.
    Arc { c: Vec2, r: f64, start: f64, end: f64, full: bool },
    /// A line through `a` along the unit vector `u`, from `a` on when
    /// `ray`.
    Line { a: Vec2, u: Vec2, ray: bool },
}

impl Curve {
    fn linear(&self) -> Option<(Vec2, Vec2, f64, f64)> {
        match *self {
            Curve::Seg { a, b } => Some((a, b - a, 0.0, 1.0)),
            Curve::Line { a, u, ray } => Some((a, u, if ray { 0.0 } else { f64::NEG_INFINITY }, f64::INFINITY)),
            Curve::Arc { .. } => None,
        }
    }

    fn on_arc(start: f64, end: f64, full: bool, angle: f64) -> bool {
        full || normalize_angle(angle - start) <= normalize_sweep(end - start) + 1e-9
    }
}

/// Where two curves cross (or touch).
pub fn intersect(p: &Curve, q: &Curve) -> Vec<Vec2> {
    let mut out = Vec::new();
    match (p.linear(), q.linear()) {
        (Some((a, r, t0, t1)), Some((c, s, u0, u1))) => {
            let den = r.cross(s);
            if den.abs() < 1e-14 * r.length().max(1.0) * s.length().max(1.0) {
                return out;
            }
            let t = (c - a).cross(s) / den;
            let u = (c - a).cross(r) / den;
            if t >= t0 - TOL && t <= t1 + TOL && u >= u0 - TOL && u <= u1 + TOL {
                out.push(a + r * t);
            }
        }
        (Some(lin), None) | (None, Some(lin)) => {
            let arc = if p.linear().is_none() { p } else { q };
            let Curve::Arc { c, r, start, end, full } = *arc else { return out };
            for (pt, t) in line_circle(lin.0, lin.1, c, r) {
                if t >= lin.2 - TOL && t <= lin.3 + TOL && Curve::on_arc(start, end, full, (pt - c).angle()) {
                    out.push(pt);
                }
            }
        }
        (None, None) => {
            let (Curve::Arc { c: c0, r: r0, start: s0, end: e0, full: f0 }, Curve::Arc { c: c1, r: r1, start: s1, end: e1, full: f1 }) = (*p, *q) else {
                return out;
            };
            for pt in circle_circle(c0, r0, c1, r1) {
                if Curve::on_arc(s0, e0, f0, (pt - c0).angle()) && Curve::on_arc(s1, e1, f1, (pt - c1).angle()) {
                    out.push(pt);
                }
            }
        }
    }
    out
}

/// Points where the line `a + d·t` meets the circle, with their `t`.
fn line_circle(a: Vec2, d: Vec2, c: Vec2, r: f64) -> Vec<(Vec2, f64)> {
    let f = a - c;
    let qa = d.dot(d);
    if qa == 0.0 {
        return Vec::new();
    }
    let qb = 2.0 * f.dot(d);
    let qc = f.dot(f) - r * r;
    let mut disc = qb * qb - 4.0 * qa * qc;
    // A line that only grazes the circle (a tangent) counts once.
    if disc < 0.0 {
        if disc > -1e-9 * qb.abs().max(qa * r * r).max(1e-300) {
            disc = 0.0;
        } else {
            return Vec::new();
        }
    }
    let sq = disc.sqrt();
    let mut v = vec![(-qb - sq) / (2.0 * qa)];
    if sq > 0.0 {
        v.push((-qb + sq) / (2.0 * qa));
    }
    v.into_iter().map(|t| (a + d * t, t)).collect()
}

fn circle_circle(c0: Vec2, r0: f64, c1: Vec2, r1: f64) -> Vec<Vec2> {
    let v = c1 - c0;
    let dist = v.length();
    if dist < 1e-12 || dist > r0 + r1 + 1e-9 || dist < (r0 - r1).abs() - 1e-9 {
        return Vec::new();
    }
    let a = (r0 * r0 - r1 * r1 + dist * dist) / (2.0 * dist);
    let h = (r0 * r0 - a * a).max(0.0).sqrt();
    let u = v / dist;
    let base = c0 + u * a;
    if h < 1e-12 {
        vec![base]
    } else {
        vec![base + u.perp() * h, base - u.perp() * h]
    }
}

/// The edges an entity offers trim and extend: its own outline pieces.
/// Ellipses and splines come as fine chords; text and fills as nothing.
pub fn curves_of(kind: &ObjectKind) -> Vec<Curve> {
    let mut k = kind.clone();
    if to_world_plane(&mut k).is_err() {
        return Vec::new();
    }
    match &k {
        ObjectKind::Line(l) => vec![Curve::Seg { a: l.start.xy(), b: l.end.xy() }],
        ObjectKind::Circle(c) => vec![Curve::Arc { c: c.center.xy(), r: c.radius, start: 0.0, end: TAU, full: true }],
        ObjectKind::Arc(a) => vec![Curve::Arc {
            c: a.center.xy(),
            r: a.radius,
            start: a.start_angle,
            end: a.end_angle,
            full: false,
        }],
        ObjectKind::LwPolyline(p) => poly_segments(&p.points, &p.bulges, p.is_closed())
            .into_iter()
            .map(|s| s.curve())
            .collect(),
        ObjectKind::Xline(x) => vec![Curve::Line { a: x.base.xy(), u: x.direction.xy().normalized(), ray: false }],
        ObjectKind::Ray(x) => vec![Curve::Line { a: x.base.xy(), u: x.direction.xy().normalized(), ray: true }],
        ObjectKind::Ellipse(e) => {
            let pts = ellipse_points(e, 0.25f64.to_radians());
            pts.windows(2).map(|w| Curve::Seg { a: w[0], b: w[1] }).collect()
        }
        ObjectKind::Spline(sp) => {
            let pts: Vec<Vec2> = if sp.control_points.len() >= 2 && !sp.knots.is_empty() {
                tessellate_nurbs(sp.degree as usize, &sp.knots, &sp.control_points, &sp.weights).iter().map(|p| p.xy()).collect()
            } else {
                sp.fit_points.iter().map(|p| p.xy()).collect()
            };
            pts.windows(2).map(|w| Curve::Seg { a: w[0], b: w[1] }).collect()
        }
        ObjectKind::Solid(s) => {
            let c = [s.corners[0], s.corners[1], s.corners[3], s.corners[2]];
            (0..4).map(|i| Curve::Seg { a: c[i], b: c[(i + 1) % 4] }).filter(|c| matches!(c, Curve::Seg { a, b } if a != b)).collect()
        }
        ObjectKind::Leader(l) => l.vertices.windows(2).map(|w| Curve::Seg { a: w[0].xy(), b: w[1].xy() }).collect(),
        _ => Vec::new(),
    }
}

fn ellipse_points(e: &Ellipse, step: f64) -> Vec<Vec2> {
    let n = e.extrusion.normalized();
    let n = if n.length() < 0.5 { Vec3::Z } else { n };
    let minor = n.cross(e.major_axis) * e.axis_ratio;
    tessellate_ellipse(e.center, e.major_axis, minor, e.start_param, e.end_param, step)
        .iter()
        .map(|p| p.xy())
        .collect()
}

// ---------------------------------------------------------------------------
// Polyline segments
// ---------------------------------------------------------------------------

/// One segment of a polyline, in its direction of travel.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PSeg {
    Line { a: Vec2, b: Vec2 },
    /// From angle `a0` to `a1` around `c`, counter-clockwise when `ccw`.
    Arc { c: Vec2, r: f64, a0: f64, a1: f64, ccw: bool },
}

impl PSeg {
    fn start(&self) -> Vec2 {
        match *self {
            PSeg::Line { a, .. } => a,
            PSeg::Arc { c, r, a0, .. } => c + Vec2::from_angle(a0) * r,
        }
    }
    fn end(&self) -> Vec2 {
        match *self {
            PSeg::Line { b, .. } => b,
            PSeg::Arc { c, r, a1, .. } => c + Vec2::from_angle(a1) * r,
        }
    }
    /// Signed sweep, positive counter-clockwise.
    fn sweep(&self) -> f64 {
        match *self {
            PSeg::Line { .. } => 0.0,
            PSeg::Arc { a0, a1, ccw, .. } => {
                if ccw {
                    normalize_sweep_open(a1 - a0)
                } else {
                    -normalize_sweep_open(a0 - a1)
                }
            }
        }
    }
    fn bulge(&self) -> f64 {
        match self {
            PSeg::Line { .. } => 0.0,
            PSeg::Arc { .. } => (self.sweep() / 4.0).tan(),
        }
    }
    fn curve(&self) -> Curve {
        match *self {
            PSeg::Line { a, b } => Curve::Seg { a, b },
            PSeg::Arc { c, r, a0, a1, ccw } => {
                let (s, e) = if ccw { (a0, a1) } else { (a1, a0) };
                Curve::Arc { c, r, start: s, end: e, full: false }
            }
        }
    }
    /// The point at fraction `f` (0 start, 1 end) of the segment.
    fn at(&self, f: f64) -> Vec2 {
        match *self {
            PSeg::Line { a, b } => a.lerp(b, f),
            PSeg::Arc { c, r, a0, .. } => c + Vec2::from_angle(a0 + self.sweep() * f) * r,
        }
    }
    /// The fraction of a point on (or near) the segment.
    fn param(&self, p: Vec2) -> f64 {
        match *self {
            PSeg::Line { a, b } => {
                let d = b - a;
                let l2 = d.dot(d);
                if l2 == 0.0 {
                    0.0
                } else {
                    (p - a).dot(d) / l2
                }
            }
            PSeg::Arc { c, a0, .. } => {
                let sw = self.sweep();
                if sw == 0.0 {
                    return 0.0;
                }
                let ang = (p - c).angle();
                let off = if sw > 0.0 { normalize_angle(ang - a0) } else { -normalize_angle(a0 - ang) };
                // Points just before the start read as a small negative.
                let f = off / sw;
                if f > 1.0 + 1e-9 {
                    let alt = if sw > 0.0 { off - TAU } else { off + TAU };
                    let fa = alt / sw;
                    if (fa - 0.0).abs() < (f - 1.0).abs() {
                        return fa;
                    }
                }
                f
            }
        }
    }
    /// The part from fraction `f0` to `f1`.
    fn sub(&self, f0: f64, f1: f64) -> PSeg {
        match *self {
            PSeg::Line { .. } => PSeg::Line { a: self.at(f0), b: self.at(f1) },
            PSeg::Arc { c, r, a0, ccw, .. } => {
                let sw = self.sweep();
                PSeg::Arc { c, r, a0: a0 + sw * f0, a1: a0 + sw * f1, ccw }
            }
        }
    }
    /// Direction of travel at fraction `f`.
    fn tangent(&self, f: f64) -> Vec2 {
        match *self {
            PSeg::Line { a, b } => (b - a).normalized(),
            PSeg::Arc { a0, ccw, .. } => {
                let ang = a0 + self.sweep() * f;
                let t = Vec2::from_angle(ang).perp();
                if ccw {
                    t
                } else {
                    -t
                }
            }
        }
    }
}

/// A sweep in (0, 2π), a zero difference staying zero.
fn normalize_sweep_open(d: f64) -> f64 {
    let s = normalize_angle(d);
    if s < 1e-15 {
        0.0
    } else {
        s
    }
}

fn poly_segments(points: &[Vec2], bulges: &[f64], closed: bool) -> Vec<PSeg> {
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    let segs = if closed { n } else { n - 1 };
    (0..segs)
        .filter_map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            if a.distance(b) < 1e-12 {
                return None;
            }
            let bulge = bulges.get(i).copied().unwrap_or(0.0);
            Some(match bulge_to_arc(a, b, bulge) {
                None => PSeg::Line { a, b },
                Some((arc, reversed)) => {
                    if reversed {
                        PSeg::Arc { c: arc.center, r: arc.radius, a0: arc.end, a1: arc.start, ccw: false }
                    } else {
                        PSeg::Arc { c: arc.center, r: arc.radius, a0: arc.start, a1: arc.end, ccw: true }
                    }
                }
            })
        })
        .collect()
}

/// Segments back to vertices and bulges.
fn from_segments(segs: &[PSeg], closed: bool) -> (Vec<Vec2>, Vec<f64>) {
    let mut pts: Vec<Vec2> = segs.iter().map(|s| s.start()).collect();
    let mut bulges: Vec<f64> = segs.iter().map(|s| s.bulge()).collect();
    if !closed {
        if let Some(last) = segs.last() {
            pts.push(last.end());
            bulges.push(0.0);
        }
    }
    if bulges.iter().all(|b| *b == 0.0) {
        bulges.clear();
    }
    (pts, bulges)
}

fn polyline_like(src: &LwPolyline, points: Vec<Vec2>, bulges: Vec<f64>, closed: bool) -> LwPolyline {
    LwPolyline {
        flags: if closed { src.flags | LwPolyline::FLAG_CLOSED } else { src.flags & !LwPolyline::FLAG_CLOSED },
        const_width: src.const_width,
        elevation: src.elevation,
        thickness: src.thickness,
        extrusion: src.extrusion,
        points,
        bulges,
        widths: Vec::new(),
        vertex_ids: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Offset
// ---------------------------------------------------------------------------

/// A parallel copy `distance` away, on the side of `side` (OFFSET).
pub fn offset(kind: &ObjectKind, distance: f64, side: Vec2) -> Result<ObjectKind, String> {
    if !(distance.is_finite() && distance > 0.0) {
        return Err("the offset distance must be more than zero".into());
    }
    let mut k = kind.clone();
    to_world_plane(&mut k)?;
    Ok(match k {
        ObjectKind::Line(l) => {
            let (a, b) = (l.start.xy(), l.end.xy());
            let n = (b - a).normalized().perp();
            if n.length() < 0.5 {
                return Err("the line has no length".into());
            }
            let s = if (side - a).dot(n) >= 0.0 { 1.0 } else { -1.0 };
            let d = (n * (distance * s)).extend(0.0);
            ObjectKind::Line(Line { start: l.start + d, end: l.end + d, ..l })
        }
        ObjectKind::Circle(c) => {
            let outside = side.distance(c.center.xy()) > c.radius;
            let r = if outside { c.radius + distance } else { c.radius - distance };
            if r <= TOL {
                return Err("the offset would shrink the circle to nothing".into());
            }
            ObjectKind::Circle(Circle { radius: r, ..c })
        }
        ObjectKind::Arc(a) => {
            let outside = side.distance(a.center.xy()) > a.radius;
            let r = if outside { a.radius + distance } else { a.radius - distance };
            if r <= TOL {
                return Err("the offset would shrink the arc to nothing".into());
            }
            ObjectKind::Arc(Arc { radius: r, ..a })
        }
        ObjectKind::Xline(x) | ObjectKind::Ray(x) => {
            let is_ray = matches!(kind, ObjectKind::Ray(_));
            let n = x.direction.xy().normalized().perp();
            let s = if (side - x.base.xy()).dot(n) >= 0.0 { 1.0 } else { -1.0 };
            let moved = ConstructionLine { base: x.base + (n * (distance * s)).extend(0.0), ..x };
            if is_ray {
                ObjectKind::Ray(moved)
            } else {
                ObjectKind::Xline(moved)
            }
        }
        ObjectKind::LwPolyline(p) => {
            let segs = poly_segments(&p.points, &p.bulges, p.is_closed());
            if segs.is_empty() {
                return Err("the polyline has no length".into());
            }
            // Left of travel is positive: the side of the nearest segment.
            let (i, f) = nearest_on(&segs, side);
            let t = segs[i].tangent(f.clamp(0.0, 1.0));
            let s = if t.cross(side - segs[i].at(f.clamp(0.0, 1.0))) >= 0.0 { 1.0 } else { -1.0 };
            let out = offset_segments(&segs, distance * s, p.is_closed())?;
            let (pts, bulges) = from_segments(&out, p.is_closed());
            ObjectKind::LwPolyline(polyline_like(&p, pts, bulges, p.is_closed()))
        }
        ObjectKind::Ellipse(e) => {
            // An exact offset of an ellipse is no ellipse: fine chords.
            let full = (normalize_sweep(e.end_param - e.start_param) - TAU).abs() < 1e-9;
            let mut pts = ellipse_points(&e, 2f64.to_radians());
            if full {
                pts.pop();
            }
            offset_points(&pts, full, distance, side)?
        }
        ObjectKind::Spline(sp) => {
            let pts: Vec<Vec2> = if sp.control_points.len() >= 2 && !sp.knots.is_empty() {
                tessellate_nurbs(sp.degree as usize, &sp.knots, &sp.control_points, &sp.weights).iter().map(|p| p.xy()).collect()
            } else {
                sp.fit_points.iter().map(|p| p.xy()).collect()
            };
            offset_points(&pts, sp.closed, distance, side)?
        }
        other => return Err(format!("{} objects cannot be offset", kind_name(&other))),
    })
}

fn offset_points(pts: &[Vec2], closed: bool, distance: f64, side: Vec2) -> Result<ObjectKind, String> {
    let src = LwPolyline::new(pts.to_vec(), closed);
    offset(&ObjectKind::LwPolyline(src), distance, side)
}

/// The segment nearest `p`, and the fraction along it of the nearest
/// point.
fn nearest_on(segs: &[PSeg], p: Vec2) -> (usize, f64) {
    let mut best = (0usize, 0.0f64, f64::INFINITY);
    for (i, s) in segs.iter().enumerate() {
        let f = s.param(p).clamp(0.0, 1.0);
        let d = s.at(f).distance(p);
        if d < best.2 {
            best = (i, f, d);
        }
    }
    (best.0, best.1)
}

/// Each segment moved `d` to its left (negative: right), joined to the
/// next where their carriers meet; a gap with no meeting point is closed
/// with an arc about the corner.
fn offset_segments(segs: &[PSeg], d: f64, closed: bool) -> Result<Vec<PSeg>, String> {
    let mut moved: Vec<PSeg> = Vec::with_capacity(segs.len());
    for s in segs {
        match *s {
            PSeg::Line { a, b } => {
                let n = (b - a).normalized().perp() * d;
                moved.push(PSeg::Line { a: a + n, b: b + n });
            }
            PSeg::Arc { c, r, a0, a1, ccw } => {
                // Left of travel is inward on a counter-clockwise arc.
                let nr = if ccw { r - d } else { r + d };
                if nr > TOL {
                    moved.push(PSeg::Arc { c, r: nr, a0, a1, ccw });
                }
            }
        }
    }
    if moved.is_empty() {
        return Err("the offset would shrink the polyline to nothing".into());
    }
    let corners: Vec<Vec2> = segs.iter().map(|s| s.start()).collect();
    let n = moved.len();
    let joins = if closed { n } else { n - 1 };
    let mut out: Vec<PSeg> = Vec::new();
    // Join pairs, collecting fills.
    let mut starts: Vec<Option<Vec2>> = vec![None; n];
    let mut ends: Vec<Option<Vec2>> = vec![None; n];
    let mut fills: Vec<Option<PSeg>> = vec![None; n];
    for i in 0..joins {
        let j = (i + 1) % n;
        let (s0, s1) = (moved[i], moved[j]);
        let (e, st) = (s0.end(), s1.start());
        if e.distance(st) < 1e-9 {
            continue;
        }
        let corner = if j < corners.len() { corners[j] } else { e };
        let cands = carrier_intersections(&s0, &s1);
        let pick = cands
            .into_iter()
            .filter(|p| p.is_finite())
            .min_by(|p, q| p.distance(corner).total_cmp(&q.distance(corner)));
        match pick {
            Some(x) if x.distance(corner) <= 10.0 * d.abs() + e.distance(st) * 10.0 => {
                ends[i] = Some(x);
                starts[j] = Some(x);
            }
            _ => {
                // Round the gap about the corner.
                let (u0, u1) = (e - corner, st - corner);
                let (a0, a1) = (u0.angle(), u1.angle());
                let ccw = u0.cross(u1) > 0.0;
                fills[i] = Some(PSeg::Arc { c: corner, r: d.abs(), a0, a1, ccw });
            }
        }
    }
    for i in 0..n {
        let s = moved[i];
        let a = starts[i].unwrap_or(s.start());
        let b = ends[i].unwrap_or(s.end());
        let seg = match s {
            PSeg::Line { .. } => PSeg::Line { a, b },
            PSeg::Arc { c, r, ccw, .. } => PSeg::Arc { c, r, a0: (a - c).angle(), a1: (b - c).angle(), ccw },
        };
        if seg.start().distance(seg.end()) > 1e-12 {
            out.push(seg);
        }
        if let Some(f) = fills[i] {
            out.push(f);
        }
    }
    if out.is_empty() {
        return Err("the offset would shrink the polyline to nothing".into());
    }
    Ok(out)
}

/// Where the full carriers (infinite line, whole circle) of two segments
/// meet.
fn carrier_intersections(a: &PSeg, b: &PSeg) -> Vec<Vec2> {
    let carrier = |s: &PSeg| match *s {
        PSeg::Line { a, b } => Curve::Line { a, u: (b - a).normalized(), ray: false },
        PSeg::Arc { c, r, .. } => Curve::Arc { c, r, start: 0.0, end: TAU, full: true },
    };
    intersect(&carrier(a), &carrier(b))
}

// ---------------------------------------------------------------------------
// Trim and extend
// ---------------------------------------------------------------------------

/// Where `edges` cross `target`, as parameters of the target's own
/// parametrisation (see [`trim`]).
fn crossings(target: &[Curve], edges: &[Curve]) -> Vec<Vec2> {
    let mut out = Vec::new();
    for t in target {
        for e in edges {
            out.extend(intersect(t, e));
        }
    }
    out
}

/// Cut `target` back at the edges nearest `pick` on either side, taking
/// away the part `pick` is on (TRIM). Returns what is left: nothing, one
/// or two objects (a line trimmed in the middle comes back as two). An
/// infinite line trimmed on one side becomes a ray; a ray trimmed at its
/// far side becomes a line.
pub fn trim(target: &ObjectKind, pick: Vec2, edges: &[Curve]) -> Result<Vec<ObjectKind>, String> {
    let mut k = target.clone();
    to_world_plane(&mut k)?;
    let none = || "no cutting edge crosses the object".to_string();
    match &k {
        ObjectKind::Line(l) => {
            let (a, b) = (l.start.xy(), l.end.xy());
            let seg = Curve::Seg { a, b };
            let d = b - a;
            let l2 = d.dot(d);
            if l2 == 0.0 {
                return Err("the line has no length".into());
            }
            let param = |p: Vec2| (p - a).dot(d) / l2;
            let cuts: Vec<f64> = crossings(&[seg], edges).into_iter().map(param).filter(|t| *t > 1e-9 && *t < 1.0 - 1e-9).collect();
            let (lo, hi) = around(&cuts, param(pick).clamp(0.0, 1.0)).ok_or_else(none)?;
            let mut out = Vec::new();
            let z = |t: f64| (a + d * t).extend(l.start.z + (l.end.z - l.start.z) * t);
            if let Some(lo) = lo {
                out.push(ObjectKind::Line(Line { start: l.start, end: z(lo), ..l.clone() }));
            }
            if let Some(hi) = hi {
                out.push(ObjectKind::Line(Line { start: z(hi), end: l.end, ..l.clone() }));
            }
            Ok(out)
        }
        ObjectKind::Arc(arc) => {
            let c = arc.center.xy();
            let sweep = normalize_sweep(arc.end_angle - arc.start_angle);
            let curve = Curve::Arc { c, r: arc.radius, start: arc.start_angle, end: arc.end_angle, full: false };
            let param = |p: Vec2| normalize_angle((p - c).angle() - arc.start_angle);
            let cuts: Vec<f64> = crossings(&[curve], edges).into_iter().map(param).filter(|t| *t > 1e-9 && *t < sweep - 1e-9).collect();
            let pp = param(pick);
            let pp = if pp > sweep { if pp - sweep < TAU - pp { sweep } else { 0.0 } } else { pp };
            let (lo, hi) = around(&cuts, pp).ok_or_else(none)?;
            let mut out = Vec::new();
            if let Some(lo) = lo {
                out.push(ObjectKind::Arc(Arc { end_angle: arc.start_angle + lo, ..arc.clone() }));
            }
            if let Some(hi) = hi {
                out.push(ObjectKind::Arc(Arc { start_angle: arc.start_angle + hi, ..arc.clone() }));
            }
            Ok(out)
        }
        ObjectKind::Circle(circle) => {
            let c = circle.center.xy();
            let curve = Curve::Arc { c, r: circle.radius, start: 0.0, end: TAU, full: true };
            let mut cuts: Vec<f64> = crossings(&[curve], edges).into_iter().map(|p| normalize_angle((p - c).angle())).collect();
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
            if cuts.len() < 2 {
                return Err("a circle needs two crossings to be trimmed".into());
            }
            let pa = normalize_angle((pick - c).angle());
            // The crossings either side of the pick, going round.
            let next = cuts.iter().copied().find(|t| *t > pa).unwrap_or(cuts[0]);
            let prev = cuts.iter().copied().rev().find(|t| *t < pa).unwrap_or(*cuts.last().expect("two"));
            Ok(vec![ObjectKind::Arc(Arc {
                center: circle.center,
                radius: circle.radius,
                thickness: circle.thickness,
                extrusion: circle.extrusion,
                start_angle: next,
                end_angle: prev,
            })])
        }
        ObjectKind::Xline(x) | ObjectKind::Ray(x) => {
            let is_ray = matches!(k, ObjectKind::Ray(_));
            let (a, u) = (x.base.xy(), x.direction.xy().normalized());
            let curve = Curve::Line { a, u, ray: is_ray };
            let param = |p: Vec2| (p - a).dot(u);
            let cuts: Vec<f64> = crossings(&[curve], edges)
                .into_iter()
                .map(param)
                .filter(|t| !is_ray || *t > 1e-9)
                .collect();
            let pp = param(pick);
            let pp = if is_ray { pp.max(0.0) } else { pp };
            let (lo, hi) = around(&cuts, pp).ok_or_else(none)?;
            let at = |t: f64| (a + u * t).extend(x.base.z);
            let mut out = Vec::new();
            if let Some(lo) = lo {
                if is_ray {
                    out.push(ObjectKind::Line(Line::new(a, a + u * lo)));
                } else {
                    out.push(ObjectKind::Ray(ConstructionLine { base: at(lo), direction: (-u).extend(0.0) }));
                }
            }
            if let Some(hi) = hi {
                out.push(ObjectKind::Ray(ConstructionLine { base: at(hi), direction: u.extend(0.0) }));
            }
            Ok(out)
        }
        ObjectKind::LwPolyline(p) => {
            let closed = p.is_closed();
            let segs = poly_segments(&p.points, &p.bulges, closed);
            if segs.is_empty() {
                return Err("the polyline has no length".into());
            }
            let n = segs.len() as f64;
            // Parameter: segment index plus the fraction along it.
            let mut cuts: Vec<f64> = Vec::new();
            for (i, s) in segs.iter().enumerate() {
                for x in crossings(&[s.curve()], edges) {
                    let f = s.param(x).clamp(0.0, 1.0);
                    cuts.push(i as f64 + f);
                }
            }
            if !closed {
                cuts.retain(|t| *t > 1e-9 && *t < n - 1e-9);
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
            let (i, f) = nearest_on(&segs, pick);
            let pp = i as f64 + f;
            let sub = |t0: f64, t1: f64| -> Vec<PSeg> { sub_path(&segs, t0, t1) };
            if closed {
                if cuts.len() < 2 {
                    return Err("a closed polyline needs two crossings to be trimmed".into());
                }
                let next = cuts.iter().copied().find(|t| *t > pp).unwrap_or(cuts[0]);
                let prev = cuts.iter().copied().rev().find(|t| *t < pp).unwrap_or(*cuts.last().expect("two"));
                // Keep from `next` round to `prev`.
                let end = if prev <= next { prev + n } else { prev };
                let kept = sub(next, end);
                let (pts, bulges) = from_segments(&kept, false);
                return Ok(vec![ObjectKind::LwPolyline(polyline_like(p, pts, bulges, false))]);
            }
            let (lo, hi) = around(&cuts, pp).ok_or_else(none)?;
            let mut out = Vec::new();
            if let Some(lo) = lo {
                let (pts, bulges) = from_segments(&sub(0.0, lo), false);
                out.push(ObjectKind::LwPolyline(polyline_like(p, pts, bulges, false)));
            }
            if let Some(hi) = hi {
                let (pts, bulges) = from_segments(&sub(hi, n), false);
                out.push(ObjectKind::LwPolyline(polyline_like(p, pts, bulges, false)));
            }
            Ok(out)
        }
        ObjectKind::Ellipse(e) => {
            let full = (normalize_sweep(e.end_param - e.start_param) - TAU).abs() < 1e-9;
            let maj = e.major_axis.xy();
            let a = maj.length();
            if a == 0.0 || e.axis_ratio <= 0.0 {
                return Err("the ellipse has no size".into());
            }
            let (u, v) = (maj / a, (maj / a).perp());
            let b = a * e.axis_ratio;
            let c = e.center.xy();
            let param = |p: Vec2| -> f64 {
                let q = p - c;
                normalize_angle((q.dot(v) / b).atan2(q.dot(u) / a))
            };
            let chords: Vec<Curve> = ellipse_points(e, 0.1f64.to_radians()).windows(2).map(|w| Curve::Seg { a: w[0], b: w[1] }).collect();
            let sweep = normalize_sweep(e.end_param - e.start_param);
            let rel = |t: f64| normalize_angle(t - e.start_param);
            let mut cuts: Vec<f64> = crossings(&chords, edges).into_iter().map(|p| rel(param(p))).collect();
            if !full {
                cuts.retain(|t| *t > 1e-6 && *t < sweep - 1e-6);
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-6);
            let pp = rel(param(pick));
            if full {
                if cuts.len() < 2 {
                    return Err("an ellipse needs two crossings to be trimmed".into());
                }
                let next = cuts.iter().copied().find(|t| *t > pp).unwrap_or(cuts[0]);
                let prev = cuts.iter().copied().rev().find(|t| *t < pp).unwrap_or(*cuts.last().expect("two"));
                return Ok(vec![ObjectKind::Ellipse(Ellipse {
                    start_param: normalize_angle(e.start_param + next),
                    end_param: normalize_angle(e.start_param + prev),
                    ..e.clone()
                })]);
            }
            let (lo, hi) = around(&cuts, pp.min(sweep)).ok_or_else(none)?;
            let mut out = Vec::new();
            if let Some(lo) = lo {
                out.push(ObjectKind::Ellipse(Ellipse { end_param: normalize_angle(e.start_param + lo), ..e.clone() }));
            }
            if let Some(hi) = hi {
                out.push(ObjectKind::Ellipse(Ellipse { start_param: normalize_angle(e.start_param + hi), ..e.clone() }));
            }
            Ok(out)
        }
        other => Err(format!("{} objects cannot be trimmed", kind_name(other))),
    }
}

/// The crossings either side of `p`: the largest below and the smallest
/// above. `None` when there is neither.
fn around(cuts: &[f64], p: f64) -> Option<(Option<f64>, Option<f64>)> {
    let lo = cuts.iter().copied().filter(|t| *t < p - 1e-12).fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))));
    let hi = cuts.iter().copied().filter(|t| *t > p + 1e-12).fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.min(t))));
    if lo.is_none() && hi.is_none() {
        None
    } else {
        Some((lo, hi))
    }
}

/// The part of a chain of segments between parameters `t0` and `t1`
/// (segment index plus fraction; `t1` may run past the end of a closed
/// chain and wrap).
fn sub_path(segs: &[PSeg], t0: f64, t1: f64) -> Vec<PSeg> {
    let n = segs.len();
    let mut out = Vec::new();
    if t1 <= t0 {
        return out;
    }
    let mut t = t0;
    while t < t1 - 1e-12 {
        let i = t.floor() as usize;
        let seg = segs[i % n];
        let f0 = t - i as f64;
        let f1 = (t1 - i as f64).min(1.0);
        if f1 - f0 > 1e-12 {
            out.push(seg.sub(f0, f1));
        }
        t = i as f64 + 1.0;
    }
    out
}

/// Lengthen `target` at its end nearest `pick` to the nearest of `edges`
/// in the way (EXTEND).
pub fn extend(target: &ObjectKind, pick: Vec2, edges: &[Curve]) -> Result<ObjectKind, String> {
    let mut k = target.clone();
    to_world_plane(&mut k)?;
    let none = || "no boundary edge lies in the way".to_string();
    match &k {
        ObjectKind::Line(l) => {
            let (a, b) = (l.start.xy(), l.end.xy());
            let d = b - a;
            let l2 = d.dot(d);
            if l2 == 0.0 {
                return Err("the line has no length".into());
            }
            let carrier = Curve::Line { a, u: d.normalized(), ray: false };
            let ts: Vec<f64> = crossings(&[carrier], edges).into_iter().map(|p| (p - a).dot(d) / l2).collect();
            let at_end = pick.distance(b) <= pick.distance(a);
            let mut out = l.clone();
            if at_end {
                let t = ts.into_iter().filter(|t| *t > 1.0 + 1e-9).fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.min(t)))).ok_or_else(none)?;
                out.end = (a + d * t).extend(l.end.z);
            } else {
                let t = ts.into_iter().filter(|t| *t < -1e-9).fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t)))).ok_or_else(none)?;
                out.start = (a + d * t).extend(l.start.z);
            }
            Ok(ObjectKind::Line(out))
        }
        ObjectKind::Arc(arc) => {
            let c = arc.center.xy();
            let circle = Curve::Arc { c, r: arc.radius, start: 0.0, end: TAU, full: true };
            let angles: Vec<f64> = crossings(&[circle], edges).into_iter().map(|p| (p - c).angle()).collect();
            let ps = c + Vec2::from_angle(arc.start_angle) * arc.radius;
            let pe = c + Vec2::from_angle(arc.end_angle) * arc.radius;
            let sweep = normalize_sweep(arc.end_angle - arc.start_angle);
            let mut out = arc.clone();
            if pick.distance(pe) <= pick.distance(ps) {
                // On past the end, counter-clockwise, short of a full turn.
                let gap = TAU - sweep;
                let step = angles.iter().map(|a| normalize_angle(*a - arc.end_angle)).filter(|s| *s > 1e-9 && *s < gap - 1e-9).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.min(s)))).ok_or_else(none)?;
                out.end_angle = arc.end_angle + step;
            } else {
                let gap = TAU - sweep;
                let step = angles.iter().map(|a| normalize_angle(arc.start_angle - *a)).filter(|s| *s > 1e-9 && *s < gap - 1e-9).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.min(s)))).ok_or_else(none)?;
                out.start_angle = arc.start_angle - step;
            }
            Ok(ObjectKind::Arc(out))
        }
        ObjectKind::LwPolyline(p) => {
            if p.is_closed() {
                return Err("a closed polyline has no end to extend".into());
            }
            let mut segs = poly_segments(&p.points, &p.bulges, false);
            if segs.is_empty() {
                return Err("the polyline has no length".into());
            }
            let first = segs[0].start();
            let last = segs[segs.len() - 1].end();
            let at_end = pick.distance(last) <= pick.distance(first);
            let idx = if at_end { segs.len() - 1 } else { 0 };
            let seg = segs[idx];
            let new_seg = match seg {
                PSeg::Line { a, b } => {
                    let line = ObjectKind::Line(Line::new(a, b));
                    match extend(&line, if at_end { b } else { a }, edges)? {
                        ObjectKind::Line(l) => PSeg::Line { a: l.start.xy(), b: l.end.xy() },
                        _ => unreachable!(),
                    }
                }
                PSeg::Arc { c, r, a0, a1, ccw } => {
                    let (s, e) = if ccw { (a0, a1) } else { (a1, a0) };
                    let arc = ObjectKind::Arc(Arc::new(c, r, s, e));
                    // The arc end that is the polyline's end.
                    let tip = if at_end { seg.end() } else { seg.start() };
                    match extend(&arc, tip, edges)? {
                        ObjectKind::Arc(x) => {
                            if ccw {
                                PSeg::Arc { c, r, a0: x.start_angle, a1: x.end_angle, ccw }
                            } else {
                                PSeg::Arc { c, r, a0: x.end_angle, a1: x.start_angle, ccw }
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            };
            segs[idx] = new_seg;
            let (pts, bulges) = from_segments(&segs, false);
            Ok(ObjectKind::LwPolyline(polyline_like(p, pts, bulges, false)))
        }
        other => Err(format!("{} objects cannot be extended", kind_name(other))),
    }
}

// ---------------------------------------------------------------------------
// Fillet and chamfer
// ---------------------------------------------------------------------------

/// What a fillet or chamfer leaves: the two objects cut back (or
/// lengthened) to the corner, and the arc or line joining them (none for
/// a radius of zero).
#[derive(Debug, Clone, PartialEq)]
pub struct Corner {
    pub first: ObjectKind,
    pub second: ObjectKind,
    pub join: Option<ObjectKind>,
}

/// A line or arc for corner work, in the world plane.
#[derive(Debug, Clone)]
enum Edge {
    Line(Line),
    Arc(Arc),
    Circle(Circle),
}

fn edge_of(kind: &ObjectKind) -> Result<Edge, String> {
    let mut k = kind.clone();
    to_world_plane(&mut k)?;
    match k {
        ObjectKind::Line(l) => Ok(Edge::Line(l)),
        ObjectKind::Arc(a) => Ok(Edge::Arc(a)),
        ObjectKind::Circle(c) => Ok(Edge::Circle(c)),
        other => Err(format!("{} objects cannot be filleted or chamfered here; pick lines, arcs or circles", kind_name(&other))),
    }
}

/// Where two lines meet, as [`line_corner`] finds it: the corner point,
/// then for each line the unit direction from the corner along the kept
/// part and the far end of the kept part.
type LineCorner = (Vec2, (Vec2, Vec2), (Vec2, Vec2));

/// Lines `a` and `b`, kept on the sides of `pa` and `pb`, and the corner
/// they make.
fn line_corner(a: &Line, pa: Vec2, b: &Line, pb: Vec2) -> Result<LineCorner, String> {
    let (a0, a1, b0, b1) = (a.start.xy(), a.end.xy(), b.start.xy(), b.end.xy());
    let (da, db) = (a1 - a0, b1 - b0);
    let den = da.cross(db);
    if den.abs() < 1e-12 * da.length().max(1.0) * db.length().max(1.0) {
        return Err("the lines are parallel".into());
    }
    let t = (b0 - a0).cross(db) / den;
    let x = a0 + da * t;
    let side = |p0: Vec2, p1: Vec2, pick: Vec2| -> Result<(Vec2, Vec2), String> {
        let d = (p1 - p0).normalized();
        let s = (pick - x).dot(d);
        let u = if s.abs() > 1e-12 {
            d * s.signum()
        } else {
            // Picked at the corner: keep the longer part.
            if (p0 - x).length() > (p1 - x).length() {
                (p0 - x).normalized()
            } else {
                (p1 - x).normalized()
            }
        };
        let far = if (p0 - x).dot(u) >= (p1 - x).dot(u) { p0 } else { p1 };
        if (far - x).dot(u) <= 1e-12 {
            return Err("the picked part of a line lies wholly on the far side of the corner".into());
        }
        Ok((u, far))
    };
    Ok((x, side(a0, a1, pa)?, side(b0, b1, pb)?))
}

fn line_from(l: &Line, p: Vec2, q: Vec2) -> ObjectKind {
    // Keep the original direction where the original start survives.
    let (s, e) = if l.start.xy().distance(q) < l.start.xy().distance(p) { (q, p) } else { (p, q) };
    let _ = (s, e);
    ObjectKind::Line(Line { start: p.extend(l.start.z), end: q.extend(l.end.z), ..l.clone() })
}

/// Round the corner between two lines or arcs with `radius` (0: meet at a
/// sharp corner), keeping each on the side it was picked (FILLET). Circles
/// are not cut; lines and arcs are cut back or lengthened to the fillet.
pub fn fillet(a: &ObjectKind, pa: Vec2, b: &ObjectKind, pb: Vec2, radius: f64) -> Result<Corner, String> {
    if !(radius.is_finite() && radius >= 0.0) {
        return Err("the fillet radius cannot be negative".into());
    }
    let (ea, eb) = (edge_of(a)?, edge_of(b)?);
    if let (Edge::Line(la), Edge::Line(lb)) = (&ea, &eb) {
        let (x, (ua, fa), (ub, fb)) = line_corner(la, pa, lb, pb)?;
        if radius == 0.0 {
            return Ok(Corner {
                first: line_from(la, x, fa),
                second: line_from(lb, x, fb),
                join: None,
            });
        }
        let theta = ua.dot(ub).clamp(-1.0, 1.0).acos();
        if theta < 1e-9 || (PI - theta) < 1e-9 {
            return Err("the lines are parallel".into());
        }
        let t = radius / (theta / 2.0).tan();
        if t > (fa - x).length() + 1e-9 || t > (fb - x).length() + 1e-9 {
            return Err("the radius is too large for these lines".into());
        }
        let (ta, tb) = (x + ua * t, x + ub * t);
        let c = x + (ua + ub).normalized() * (radius / (theta / 2.0).sin());
        return Ok(Corner {
            first: line_from(la, ta, fa),
            second: line_from(lb, tb, fb),
            join: Some(ObjectKind::Arc(minor_arc(c, radius, ta, tb))),
        });
    }
    if radius == 0.0 {
        return Err("a fillet of radius 0 needs two lines".into());
    }
    // General case: the fillet's centre is `radius` from both, on the
    // picked sides.
    let centers = fillet_centers(&ea, &eb, radius);
    let mut best: Option<(f64, Vec2, Vec2, Vec2)> = None;
    for c in centers {
        let ta = touch(&ea, c);
        let tb = touch(&eb, c);
        let score = ta.distance(pa) + tb.distance(pb);
        if best.is_none_or(|b| score < b.0) {
            best = Some((score, c, ta, tb));
        }
    }
    let (_, c, ta, tb) = best.ok_or("no fillet of that radius fits between them")?;
    Ok(Corner {
        first: cut_to(&ea, ta, pa)?,
        second: cut_to(&eb, tb, pb)?,
        join: Some(ObjectKind::Arc(minor_arc(c, radius, ta, tb))),
    })
}

/// The arc about `c` from `p` to `q` that turns less than half a circle.
fn minor_arc(c: Vec2, r: f64, p: Vec2, q: Vec2) -> Arc {
    let (a0, a1) = ((p - c).angle(), (q - c).angle());
    if normalize_sweep(a1 - a0) <= PI {
        Arc::new(c, r, a0, a1)
    } else {
        Arc::new(c, r, a1, a0)
    }
}

/// Where the edge comes closest to `c` (its tangent point with a circle
/// centred there).
fn touch(e: &Edge, c: Vec2) -> Vec2 {
    match e {
        Edge::Line(l) => {
            let (a, b) = (l.start.xy(), l.end.xy());
            let d = b - a;
            a + d * ((c - a).dot(d) / d.dot(d))
        }
        Edge::Arc(Arc { center, radius, .. }) | Edge::Circle(Circle { center, radius, .. }) => {
            let cc = center.xy();
            cc + (c - cc).normalized() * *radius
        }
    }
}

/// Centres of circles of radius `r` tangent to both edges.
fn fillet_centers(a: &Edge, b: &Edge, r: f64) -> Vec<Vec2> {
    let offsets = |e: &Edge| -> Vec<Curve> {
        match e {
            Edge::Line(l) => {
                let (p, q) = (l.start.xy(), l.end.xy());
                let u = (q - p).normalized();
                let n = u.perp();
                vec![Curve::Line { a: p + n * r, u, ray: false }, Curve::Line { a: p - n * r, u, ray: false }]
            }
            Edge::Arc(Arc { center, radius, .. }) | Edge::Circle(Circle { center, radius, .. }) => {
                let mut v = vec![Curve::Arc { c: center.xy(), r: radius + r, start: 0.0, end: TAU, full: true }];
                if (radius - r).abs() > 1e-12 {
                    v.push(Curve::Arc { c: center.xy(), r: (radius - r).abs(), start: 0.0, end: TAU, full: true });
                }
                v
            }
        }
    };
    let mut out = Vec::new();
    for p in offsets(a) {
        for q in offsets(b) {
            out.extend(intersect(&p, &q));
        }
    }
    out
}

/// An edge cut back or lengthened to `t`, keeping the side of `pick`.
fn cut_to(e: &Edge, t: Vec2, pick: Vec2) -> Result<ObjectKind, String> {
    match e {
        Edge::Line(l) => {
            let (a, b) = (l.start.xy(), l.end.xy());
            let d = (b - a).normalized();
            let s = (pick - t).dot(d);
            let far = if s >= 0.0 {
                if (b - t).dot(d) >= (a - t).dot(d) { b } else { a }
            } else if (a - t).dot(d) <= (b - t).dot(d) {
                a
            } else {
                b
            };
            if far.distance(t) < 1e-12 {
                return Err("nothing of the line would be left".into());
            }
            Ok(line_from(l, t, far))
        }
        Edge::Circle(c) => Ok(ObjectKind::Circle(c.clone())),
        Edge::Arc(arc) => {
            let c = arc.center.xy();
            let tau = (t - c).angle();
            let pick_a = (pick - c).angle();
            // Two candidates: start → tangent, tangent → end (each may run
            // past the arc's ends); keep the one holding the pick.
            let first = (arc.start_angle, tau);
            let second = (tau, arc.end_angle);
            let holds = |(s, e): (f64, f64)| normalize_angle(pick_a - s) <= normalize_sweep(e - s);
            let (s, e) = if holds(first) && !holds(second) {
                first
            } else if holds(second) && !holds(first) {
                second
            } else {
                // Both or neither: the one closer to the original span.
                let sw1 = normalize_sweep(first.1 - first.0);
                let sw2 = normalize_sweep(second.1 - second.0);
                if sw1 <= sw2 {
                    first
                } else {
                    second
                }
            };
            Ok(ObjectKind::Arc(Arc { start_angle: s, end_angle: e, ..arc.clone() }))
        }
    }
}

/// Bevel the corner between two lines: `d1` along the first from the
/// corner, `d2` along the second (CHAMFER).
pub fn chamfer(a: &ObjectKind, pa: Vec2, b: &ObjectKind, pb: Vec2, d1: f64, d2: f64) -> Result<Corner, String> {
    if !(d1.is_finite() && d2.is_finite() && d1 >= 0.0 && d2 >= 0.0) {
        return Err("chamfer distances cannot be negative".into());
    }
    let (Edge::Line(la), Edge::Line(lb)) = (edge_of(a)?, edge_of(b)?) else {
        return Err("chamfers join two lines".into());
    };
    let (x, (ua, fa), (ub, fb)) = line_corner(&la, pa, &lb, pb)?;
    if d1 > (fa - x).length() + 1e-9 || d2 > (fb - x).length() + 1e-9 {
        return Err("the chamfer is longer than the lines".into());
    }
    let (ca, cb) = (x + ua * d1, x + ub * d2);
    Ok(Corner {
        first: line_from(&la, ca, fa),
        second: line_from(&lb, cb, fb),
        join: (ca.distance(cb) > 1e-12).then(|| ObjectKind::Line(Line::new(ca, cb))),
    })
}

/// Round every corner of a polyline where two straight segments meet
/// (FILLET's Polyline option). Corners next to arc segments, corners too
/// tight for the radius and the ends of an open polyline are left as they
/// are. Returns the polyline and how many corners were rounded.
pub fn fillet_polyline(p: &LwPolyline, radius: f64) -> Result<(LwPolyline, usize), String> {
    fillet_polyline_at(p, None, radius)
}

/// [`fillet_polyline`] at one vertex only (`Some(i)`), as FILLET does when
/// both picks are segments of the same polyline that meet there.
pub fn fillet_polyline_at(p: &LwPolyline, only: Option<usize>, radius: f64) -> Result<(LwPolyline, usize), String> {
    if !(radius.is_finite() && radius >= 0.0) {
        return Err("the fillet radius cannot be negative".into());
    }
    corner_polyline(p, only, |din, dout, len_in, len_out| {
        let theta = (-din).dot(dout).clamp(-1.0, 1.0).acos();
        if theta < 1e-9 || PI - theta < 1e-9 {
            return None;
        }
        let t = radius / (theta / 2.0).tan();
        if t > len_in || t > len_out {
            return None;
        }
        let turn = din.cross(dout);
        let bulge = ((PI - theta) / 4.0).tan() * turn.signum();
        Some((t, t, bulge))
    })
}

/// Bevel every corner of a polyline where two straight segments meet.
pub fn chamfer_polyline(p: &LwPolyline, d1: f64, d2: f64) -> Result<(LwPolyline, usize), String> {
    chamfer_polyline_at(p, None, d1, d2)
}

/// [`chamfer_polyline`] at one vertex only.
pub fn chamfer_polyline_at(p: &LwPolyline, only: Option<usize>, d1: f64, d2: f64) -> Result<(LwPolyline, usize), String> {
    if !(d1.is_finite() && d2.is_finite() && d1 >= 0.0 && d2 >= 0.0) {
        return Err("chamfer distances cannot be negative".into());
    }
    corner_polyline(p, only, |din, dout, len_in, len_out| {
        if din.cross(dout).abs() < 1e-12 || d1 > len_in || d2 > len_out {
            return None;
        }
        Some((d1, d2, 0.0))
    })
}

/// Shared walk of `fillet_polyline` and `chamfer_polyline`: `cut` gives,
/// for a corner, how far back along the incoming and outgoing segments to
/// cut and the bulge of the piece that joins them.
fn corner_polyline(p: &LwPolyline, only: Option<usize>, cut: impl Fn(Vec2, Vec2, f64, f64) -> Option<(f64, f64, f64)>) -> Result<(LwPolyline, usize), String> {
    let n = p.points.len();
    if n < 3 {
        return Err("the polyline has no corners".into());
    }
    let closed = p.is_closed();
    let pts = &p.points;
    let bulge = |i: usize| p.bulge(i % n);
    let mut out_pts: Vec<Vec2> = Vec::new();
    let mut out_bulges: Vec<f64> = Vec::new();
    let mut done = 0;
    // How much of each segment the corners before and after it take.
    let mut taken_start = vec![0.0f64; n];
    let mut plans: Vec<Option<(f64, f64, f64)>> = vec![None; n];
    for i in 0..n {
        let first_or_last = !closed && (i == 0 || i == n - 1);
        if first_or_last || only.is_some_and(|v| v != i) {
            continue;
        }
        let (prev, next) = ((i + n - 1) % n, (i + 1) % n);
        if bulge(prev) != 0.0 || bulge(i) != 0.0 {
            continue;
        }
        let (din, dout) = (pts[i] - pts[prev], pts[next] - pts[i]);
        let (lin, lout) = (din.length(), dout.length());
        if lin < 1e-12 || lout < 1e-12 {
            continue;
        }
        // The incoming segment may already be shortened at its start.
        let avail_in = lin - taken_start[prev];
        if let Some(plan) = cut(din / lin, dout / lout, avail_in, lout) {
            taken_start[i] = plan.1;
            plans[i] = Some(plan);
        }
    }
    // A closed polyline's first corner can clash with its last.
    if closed {
        if let (Some(first), Some(last)) = (plans[0], plans[n - 1]) {
            let len = pts[0].distance(pts[n - 1]);
            if first.0 + last.1 > len + 1e-9 {
                plans[0] = None;
            }
        }
    }
    for i in 0..n {
        match plans[i] {
            Some((t_in, t_out, b)) => {
                let prev = (i + n - 1) % n;
                let next = (i + 1) % n;
                let din = (pts[i] - pts[prev]).normalized();
                let dout = (pts[next] - pts[i]).normalized();
                out_pts.push(pts[i] - din * t_in);
                out_bulges.push(b);
                out_pts.push(pts[i] + dout * t_out);
                out_bulges.push(bulge(i));
                done += 1;
            }
            None => {
                out_pts.push(pts[i]);
                out_bulges.push(bulge(i));
            }
        }
    }
    // Merge points that coincide (a corner whose cut took a whole
    // segment).
    let mut q: Vec<(Vec2, f64)> = Vec::new();
    for (pt, b) in out_pts.into_iter().zip(out_bulges) {
        if q.last().is_some_and(|l| l.0.distance(pt) < 1e-12) {
            q.last_mut().expect("last").1 = b;
        } else {
            q.push((pt, b));
        }
    }
    let (points, bulges): (Vec<Vec2>, Vec<f64>) = q.into_iter().unzip();
    let bulges = if bulges.iter().all(|b| *b == 0.0) { Vec::new() } else { bulges };
    Ok((polyline_like(p, points, bulges, closed), done))
}

/// The segment of a polyline nearest `p` (its index: the segment leaving
/// vertex `i`).
pub fn polyline_segment_at(p: &LwPolyline, at: Vec2) -> Option<usize> {
    let segs = poly_segments(&p.points, &p.bulges, p.is_closed());
    if segs.is_empty() {
        return None;
    }
    Some(nearest_on(&segs, at).0)
}

/// The vertex where the segments picked at `a` and `b` of a polyline meet,
/// when they are neighbours.
pub fn polyline_corner_between(p: &LwPolyline, a: Vec2, b: Vec2) -> Option<usize> {
    let (i, j) = (polyline_segment_at(p, a)?, polyline_segment_at(p, b)?);
    let n = p.points.len();
    let segs = if p.is_closed() { n } else { n.saturating_sub(1) };
    if segs < 2 || i == j {
        return None;
    }
    if (i + 1) % n == j && (p.is_closed() || i + 1 < segs) {
        Some(j)
    } else if (j + 1) % n == i && (p.is_closed() || j + 1 < segs) {
        Some(i)
    } else {
        None
    }
}

/// How far `p` is from an entity's outline (its nearest edge).
pub fn distance_to(kind: &ObjectKind, p: Vec2) -> Option<f64> {
    curves_of(kind)
        .iter()
        .map(|c| match *c {
            Curve::Seg { a, b } => {
                let d = b - a;
                let l2 = d.dot(d);
                let t = if l2 == 0.0 { 0.0 } else { ((p - a).dot(d) / l2).clamp(0.0, 1.0) };
                p.distance(a + d * t)
            }
            Curve::Line { a, u, ray } => {
                let t = (p - a).dot(u);
                let t = if ray { t.max(0.0) } else { t };
                p.distance(a + u * t)
            }
            Curve::Arc { c, r, start, end, full } => {
                if full || Curve::on_arc(start, end, false, (p - c).angle()) {
                    (p.distance(c) - r).abs()
                } else {
                    let ps = c + Vec2::from_angle(start) * r;
                    let pe = c + Vec2::from_angle(end) * r;
                    p.distance(ps).min(p.distance(pe))
                }
            }
        })
        .min_by(f64::total_cmp)
}

// ---------------------------------------------------------------------------
// Stretch
// ---------------------------------------------------------------------------

fn in_window(w: &BBox, p: Vec2) -> bool {
    p.x >= w.min.x && p.x <= w.max.x && p.y >= w.min.y && p.y <= w.max.y
}

/// Move the defining points of an entity that lie in the crossing window
/// `w` by `delta` (STRETCH); an entity wholly inside moves whole. Whether
/// anything moved.
pub fn stretch(kind: &mut ObjectKind, w: &BBox, delta: Vec2) -> Result<bool, String> {
    to_world_plane(kind)?;
    let d3 = delta.extend(0.0);
    let mv = |p: &mut Vec2| -> bool {
        if in_window(w, *p) {
            *p += delta;
            true
        } else {
            false
        }
    };
    let mv3 = |p: &mut Vec3| -> bool {
        if in_window(w, p.xy()) {
            *p += d3;
            true
        } else {
            false
        }
    };
    Ok(match kind {
        ObjectKind::Line(l) => {
            let a = mv3(&mut l.start);
            let b = mv3(&mut l.end);
            a || b
        }
        ObjectKind::LwPolyline(p) => {
            let mut any = false;
            for q in &mut p.points {
                any |= mv(q);
            }
            any
        }
        ObjectKind::Arc(a) => {
            let c = a.center.xy();
            let mut p0 = c + Vec2::from_angle(a.start_angle) * a.radius;
            let mut p1 = c + Vec2::from_angle(a.end_angle) * a.radius;
            let (i0, i1) = (in_window(w, p0), in_window(w, p1));
            if i0 && i1 {
                a.center += d3;
                true
            } else if i0 || i1 {
                // Keep the arc's included angle: endpoints and bulge.
                let bulge = (normalize_sweep(a.end_angle - a.start_angle) / 4.0).tan();
                if i0 {
                    p0 += delta;
                } else {
                    p1 += delta;
                }
                match bulge_to_arc(p0, p1, bulge) {
                    Some((arc, _)) => {
                        a.center = arc.center.extend(a.center.z);
                        a.radius = arc.radius;
                        a.start_angle = arc.start;
                        a.end_angle = arc.end;
                        true
                    }
                    None => false,
                }
            } else {
                false
            }
        }
        ObjectKind::Circle(c) => mv3(&mut c.center),
        ObjectKind::Point(p) => mv3(&mut p.position),
        ObjectKind::Ellipse(e) => mv3(&mut e.center),
        ObjectKind::Text(t) => {
            if in_window(w, t.insertion) {
                t.insertion += delta;
                t.alignment += delta;
                true
            } else {
                false
            }
        }
        ObjectKind::Attribute(a) => {
            if in_window(w, a.text.insertion) {
                a.text.insertion += delta;
                a.text.alignment += delta;
                true
            } else {
                false
            }
        }
        ObjectKind::MText(m) => mv3(&mut m.insertion),
        ObjectKind::Insert(i) => mv3(&mut i.insertion),
        ObjectKind::Solid(s) => {
            let mut any = false;
            for c in &mut s.corners {
                any |= mv(c);
            }
            any
        }
        ObjectKind::Spline(sp) => {
            let mut any = false;
            for c in sp.control_points.iter_mut().chain(sp.fit_points.iter_mut()) {
                any |= mv3(c);
            }
            any
        }
        ObjectKind::Leader(l) => {
            let mut any = false;
            for v in &mut l.vertices {
                any |= mv3(v);
            }
            any
        }
        ObjectKind::Xline(x) | ObjectKind::Ray(x) => mv3(&mut x.base),
        ObjectKind::Hatch(h) => {
            let mut any = false;
            for p in &mut h.paths {
                match p {
                    HatchPath::Polyline { vertices, .. } => {
                        for (q, _) in vertices {
                            any |= mv(q);
                        }
                    }
                    HatchPath::Edges { edges, .. } => {
                        for e in edges {
                            if let HatchEdge::Line { a, b } = e {
                                any |= mv(a);
                                any |= mv(b);
                            }
                        }
                    }
                }
            }
            any
        }
        ObjectKind::Dimension(dm) => {
            let mut any = false;
            any |= mv3(&mut dm.pt10);
            any |= mv3(&mut dm.pt13);
            any |= mv3(&mut dm.pt14);
            any |= mv3(&mut dm.pt15);
            if dm.text_midpoint != Vec2::ZERO {
                any |= mv(&mut dm.text_midpoint);
            }
            if any {
                remeasure(dm);
                dm.block = Handle::NULL;
            }
            any
        }
        _ => false,
    })
}

/// A dimension's measurement from its definition points.
pub fn remeasure(d: &mut Dimension) {
    match d.kind {
        DimensionKind::Linear => {
            let u = Vec2::from_angle(d.dim_rotation);
            d.measurement = (d.pt14.xy() - d.pt13.xy()).dot(u).abs();
        }
        DimensionKind::Aligned => d.measurement = d.pt13.xy().distance(d.pt14.xy()),
        DimensionKind::Radius => d.measurement = d.pt10.xy().distance(d.pt15.xy()),
        DimensionKind::Diameter => d.measurement = d.pt10.xy().distance(d.pt15.xy()),
        DimensionKind::Angular3Pt => {
            let c = d.pt15.xy();
            let (a, b) = ((d.pt13.xy() - c).angle(), (d.pt14.xy() - c).angle());
            d.measurement = normalize_sweep(b - a).min(TAU);
        }
        DimensionKind::Angular2Line | DimensionKind::Ordinate => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }
    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-9
    }
    fn line(a: Vec2, b: Vec2) -> ObjectKind {
        ObjectKind::Line(Line::new(a, b))
    }
    fn ends(k: &ObjectKind) -> (Vec2, Vec2) {
        match k {
            ObjectKind::Line(l) => (l.start.xy(), l.end.xy()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn transforms_keep_shapes() {
        let mut a = ObjectKind::Arc(Arc::new(v(0.0, 0.0), 5.0, 0.0, PI / 2.0));
        transform(&mut a, &Affine::mirror(v(0.0, 0.0), v(0.0, 1.0))).unwrap();
        match &a {
            ObjectKind::Arc(x) => {
                assert!((normalize_angle(x.start_angle) - PI / 2.0).abs() < 1e-9);
                assert!((normalize_angle(x.end_angle) - PI).abs() < 1e-9);
            }
            o => panic!("{o:?}"),
        }
        let mut c = ObjectKind::Circle(Circle::new(v(1.0, 1.0), 2.0));
        transform(&mut c, &Affine::scale(v(0.0, 0.0), 3.0)).unwrap();
        assert_eq!(c, ObjectKind::Circle(Circle::new(v(3.0, 3.0), 6.0)));
        let mut ins = ObjectKind::Insert(Insert { scale: Vec3::new(1.0, 1.0, 1.0), extrusion: Vec3::Z, ..Default::default() });
        transform(&mut ins, &Affine::mirror(v(0.0, 0.0), v(0.0, 1.0))).unwrap();
        match &ins {
            ObjectKind::Insert(i) => {
                assert_eq!(i.scale.x, -1.0);
                assert!((i.rotation - 0.0).abs() < 1e-9 || (i.rotation - TAU).abs() < 1e-9, "{}", i.rotation);
            }
            o => panic!("{o:?}"),
        }
        // A DWG arc seen from below is restated first.
        let mut low = ObjectKind::Arc(Arc { extrusion: Vec3::new(0.0, 0.0, -1.0), ..Arc::new(v(2.0, 0.0), 1.0, 0.0, PI / 2.0) });
        transform(&mut low, &Affine::IDENTITY).unwrap();
        match &low {
            ObjectKind::Arc(x) => {
                assert!(close(x.center.xy(), v(-2.0, 0.0)));
                assert!((normalize_angle(x.start_angle) - PI / 2.0).abs() < 1e-9);
                assert!((normalize_angle(x.end_angle) - PI).abs() < 1e-9);
            }
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn offsets_lines_circles_and_polylines() {
        let l = offset(&line(v(0.0, 0.0), v(10.0, 0.0)), 2.0, v(5.0, -1.0)).unwrap();
        assert_eq!(ends(&l), (v(0.0, -2.0), v(10.0, -2.0)));
        let c = offset(&ObjectKind::Circle(Circle::new(v(0.0, 0.0), 5.0)), 1.0, v(1.0, 0.0)).unwrap();
        assert_eq!(c, ObjectKind::Circle(Circle::new(v(0.0, 0.0), 4.0)));
        assert!(offset(&ObjectKind::Circle(Circle::new(v(0.0, 0.0), 1.0)), 2.0, v(0.1, 0.0)).is_err());
        // A square offset outward: a bigger square.
        let sq = LwPolyline::new(vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)], true);
        match offset(&ObjectKind::LwPolyline(sq.clone()), 1.0, v(-5.0, 5.0)).unwrap() {
            ObjectKind::LwPolyline(p) => {
                assert_eq!(p.points.len(), 4);
                assert!(p.points.iter().any(|q| close(*q, v(-1.0, -1.0))));
                assert!(p.points.iter().any(|q| close(*q, v(11.0, 11.0))));
            }
            o => panic!("{o:?}"),
        }
        // Inward.
        match offset(&ObjectKind::LwPolyline(sq), 1.0, v(5.0, 5.0)).unwrap() {
            ObjectKind::LwPolyline(p) => assert!(p.points.iter().any(|q| close(*q, v(1.0, 1.0)))),
            o => panic!("{o:?}"),
        }
        // A slot (two lines and two half circles) offset outward stays a
        // slot with bigger ends.
        let slot = LwPolyline {
            bulges: vec![0.0, 1.0, 0.0, 1.0],
            ..LwPolyline::new(vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 4.0), v(0.0, 4.0)], true)
        };
        match offset(&ObjectKind::LwPolyline(slot), 1.0, v(5.0, 10.0)).unwrap() {
            ObjectKind::LwPolyline(p) => {
                assert_eq!(p.points.len(), 4, "{:?}", p.points);
                assert!(p.points.iter().any(|q| close(*q, v(10.0, -1.0))));
                assert!(p.bulges.iter().filter(|b| (**b - 1.0).abs() < 1e-9).count() == 2);
            }
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn trims_and_extends() {
        let cutter = curves_of(&line(v(5.0, -5.0), v(5.0, 5.0)));
        // Trim the right half of a line crossing the cutter.
        let r = trim(&line(v(0.0, 0.0), v(10.0, 0.0)), v(8.0, 0.0), &cutter).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(ends(&r[0]), (v(0.0, 0.0), v(5.0, 0.0)));
        // Between two cutters: two pieces.
        let mut two = cutter.clone();
        two.extend(curves_of(&line(v(7.0, -5.0), v(7.0, 5.0))));
        let r = trim(&line(v(0.0, 0.0), v(10.0, 0.0)), v(6.0, 0.0), &two).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(ends(&r[1]), (v(7.0, 0.0), v(10.0, 0.0)));
        // A circle cut by a line loses the arc picked.
        let r = trim(&ObjectKind::Circle(Circle::new(v(5.0, 0.0), 2.0)), v(7.0, 0.0), &cutter).unwrap();
        match &r[0] {
            ObjectKind::Arc(a) => {
                assert!((normalize_angle(a.start_angle) - PI / 2.0).abs() < 1e-9);
                assert!((normalize_angle(a.end_angle) - 3.0 * PI / 2.0).abs() < 1e-9);
            }
            o => panic!("{o:?}"),
        }
        // An infinite line trimmed on one side becomes a ray.
        let r = trim(
            &ObjectKind::Xline(ConstructionLine::new(v(0.0, 0.0), v(1.0, 0.0))),
            v(8.0, 0.0),
            &cutter,
        )
        .unwrap();
        assert!(matches!(&r[0], ObjectKind::Ray(x) if close(x.base.xy(), v(5.0, 0.0)) && x.direction.x < 0.0));
        // Extend a short line to the cutter.
        let e = extend(&line(v(0.0, 1.0), v(3.0, 1.0)), v(2.9, 1.0), &cutter).unwrap();
        assert_eq!(ends(&e), (v(0.0, 1.0), v(5.0, 1.0)));
        assert!(extend(&line(v(0.0, 1.0), v(3.0, 1.0)), v(0.1, 1.0), &cutter).is_err());
        // Trim an open polyline's end segment.
        let pl = ObjectKind::LwPolyline(LwPolyline::new(vec![v(0.0, 0.0), v(4.0, 0.0), v(8.0, 0.0)], false));
        match &trim(&pl, v(7.0, 0.0), &cutter).unwrap()[0] {
            ObjectKind::LwPolyline(p) => assert_eq!(p.points, vec![v(0.0, 0.0), v(4.0, 0.0), v(5.0, 0.0)]),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn fillets_and_chamfers() {
        // Two lines meeting at a right angle at the origin.
        let a = line(v(10.0, 0.0), v(0.0, 0.0));
        let b = line(v(0.0, 0.0), v(0.0, 10.0));
        let f = fillet(&a, v(5.0, 0.0), &b, v(0.0, 5.0), 2.0).unwrap();
        let (a0, a1) = ends(&f.first);
        assert!(close(a0, v(2.0, 0.0)) && close(a1, v(10.0, 0.0)), "{a0:?} {a1:?}");
        match f.join.unwrap() {
            ObjectKind::Arc(arc) => {
                assert!(close(arc.center.xy(), v(2.0, 2.0)));
                assert!((arc.radius - 2.0).abs() < 1e-9);
                assert!((normalize_sweep(arc.end_angle - arc.start_angle) - PI / 2.0).abs() < 1e-9);
            }
            o => panic!("{o:?}"),
        }
        // Radius 0: lines meet. Gaps close, overhangs go.
        let a = line(v(10.0, 0.0), v(2.0, 0.0));
        let b = line(v(0.0, -3.0), v(0.0, 10.0));
        let f = fillet(&a, v(5.0, 0.0), &b, v(0.0, 5.0), 0.0).unwrap();
        assert!(close(ends(&f.first).0, v(0.0, 0.0)));
        assert!(close(ends(&f.second).0, v(0.0, 0.0)) && close(ends(&f.second).1, v(0.0, 10.0)));
        assert!(f.join.is_none());
        let c = chamfer(&line(v(10.0, 0.0), v(0.0, 0.0)), v(5.0, 0.0), &line(v(0.0, 0.0), v(0.0, 10.0)), v(0.0, 5.0), 1.0, 2.0).unwrap();
        assert_eq!(ends(&c.join.unwrap()), (v(1.0, 0.0), v(0.0, 2.0)));
        // Line and arc.
        let arc = ObjectKind::Arc(Arc::new(v(0.0, 0.0), 10.0, 0.0, PI));
        let l = line(v(-20.0, 5.0), v(20.0, 5.0));
        let f = fillet(&l, v(15.0, 5.0), &arc, v(9.5, 2.0), 1.0).unwrap();
        match f.join.unwrap() {
            ObjectKind::Arc(j) => {
                let c = j.center.xy();
                assert!((c.length() - 9.0).abs() < 1e-6 || (c.length() - 11.0).abs() < 1e-6);
                assert!((c.y - 4.0).abs() < 1e-6 || (c.y - 6.0).abs() < 1e-6);
            }
            o => panic!("{o:?}"),
        }
        // Every corner of a rectangle.
        let r = LwPolyline::new(vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 6.0), v(0.0, 6.0)], true);
        let (p, n) = fillet_polyline(&r, 1.0).unwrap();
        assert_eq!(n, 4);
        assert_eq!(p.points.len(), 8);
        assert_eq!(p.bulges.iter().filter(|b| (**b - (PI / 8.0).tan()).abs() < 1e-9).count(), 4);
        let (p, n) = chamfer_polyline(&r, 1.0, 1.0).unwrap();
        assert_eq!((n, p.points.len()), (4, 8));
    }

    #[test]
    fn stretch_moves_what_is_in_the_window() {
        let w = BBox { min: v(8.0, -1.0), max: v(12.0, 1.0) };
        let mut l = line(v(0.0, 0.0), v(10.0, 0.0));
        assert!(stretch(&mut l, &w, v(5.0, 0.0)).unwrap());
        assert_eq!(ends(&l), (v(0.0, 0.0), v(15.0, 0.0)));
        let mut dm = ObjectKind::Dimension(Dimension::linear(v(0.0, 0.0), v(10.0, 0.0), v(0.0, -5.0), 0.0));
        let w2 = BBox { min: v(8.0, -10.0), max: v(12.0, 1.0) };
        assert!(stretch(&mut dm, &w2, v(5.0, 0.0)).unwrap());
        match dm {
            ObjectKind::Dimension(d) => assert!((d.measurement - 15.0).abs() < 1e-9),
            o => panic!("{o:?}"),
        }
    }
}

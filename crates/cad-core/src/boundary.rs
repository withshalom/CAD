//! Hatch boundaries: the closed region around a picked point, worked out
//! from the lines, arcs, circles, polylines, ellipses, splines and
//! construction lines of a space, with the islands inside it (HATCH's
//! pick-points method); loops from objects that are closed already (its
//! select-objects method); and the HATCH entity made from either.
//!
//! The region is found the way a person traces one: every edge is cut
//! where it meets another, loose ends are cleared away, and the cycles of
//! edges around each face are walked keeping to the left. The picked
//! point's region is the smallest counter-clockwise cycle around it.
//! Islands are the outlines of the separate groups of edges inside it,
//! all of them, so that islands inside islands are hatched again (the
//! "normal" island style).

use crate::geom::curve::{normalize_angle, normalize_sweep, tessellate_ellipse, tessellate_nurbs};
use crate::geom::{BBox, Vec2, Vec3};
use crate::model::*;
use crate::modify::{self, Curve};
use crate::units::Unit;
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

/// A closed loop: each vertex with the bulge of the segment leaving it.
pub type Loop = Vec<(Vec2, f64)>;

/// A region to hatch: its outline and the islands inside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub outer: Loop,
    pub islands: Vec<Loop>,
}

/// Most edge pieces one boundary search works through.
pub const MAX_PIECES: usize = 40_000;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Piece {
    Line { a: Vec2, b: Vec2 },
    /// Counter-clockwise from angle `a0` through `sw` (0 < sw ≤ 2π).
    Arc { c: Vec2, r: f64, a0: f64, sw: f64 },
}

impl Piece {
    fn at(&self, t: f64) -> Vec2 {
        match *self {
            Piece::Line { a, b } => a.lerp(b, t),
            Piece::Arc { c, r, a0, sw } => c + Vec2::from_angle(a0 + sw * t) * r,
        }
    }

    fn start(&self) -> Vec2 {
        self.at(0.0)
    }

    fn end(&self) -> Vec2 {
        self.at(1.0)
    }

    /// The fraction along the piece of a point on (or near) it; points off
    /// an arc's span read as just before its start or just after its end,
    /// whichever is nearer.
    fn param(&self, p: Vec2) -> f64 {
        match *self {
            Piece::Line { a, b } => {
                let d = b - a;
                let l2 = d.dot(d);
                if l2 == 0.0 {
                    0.0
                } else {
                    (p - a).dot(d) / l2
                }
            }
            Piece::Arc { c, a0, sw, .. } => {
                let off = normalize_angle((p - c).angle() - a0);
                if off <= sw {
                    off / sw
                } else if TAU - off < off - sw {
                    -(TAU - off) / sw
                } else {
                    off / sw
                }
            }
        }
    }

    fn sub(&self, t0: f64, t1: f64) -> Piece {
        match *self {
            Piece::Line { .. } => Piece::Line {
                a: self.at(t0),
                b: self.at(t1),
            },
            Piece::Arc { c, r, a0, sw } => Piece::Arc {
                c,
                r,
                a0: a0 + sw * t0,
                sw: sw * (t1 - t0),
            },
        }
    }

    fn curve(&self) -> Curve {
        match *self {
            Piece::Line { a, b } => Curve::Seg { a, b },
            Piece::Arc { c, r, a0, sw } => Curve::Arc {
                c,
                r,
                start: a0,
                end: a0 + sw,
                full: sw >= TAU - 1e-12,
            },
        }
    }

    fn bbox(&self) -> BBox {
        let mut b = BBox::EMPTY;
        b.add(self.start());
        b.add(self.end());
        if let Piece::Arc { c, r, a0, sw } = *self {
            for k in 0..4 {
                let ang = k as f64 * PI / 2.0;
                if normalize_angle(ang - a0) <= sw {
                    b.add(c + Vec2::from_angle(ang) * r);
                }
            }
        }
        b
    }

    fn length(&self) -> f64 {
        match *self {
            Piece::Line { a, b } => a.distance(b),
            Piece::Arc { r, sw, .. } => r * sw,
        }
    }

    /// The direction of travel leaving the piece's start (or, travelled
    /// backwards, its end), and the curvature, left turns positive.
    fn leave(&self, reversed: bool) -> (f64, f64) {
        match *self {
            Piece::Line { a, b } => {
                let d = if reversed { a - b } else { b - a };
                (d.angle(), 0.0)
            }
            Piece::Arc { r, a0, sw, .. } => {
                if reversed {
                    ((-Vec2::from_angle(a0 + sw).perp()).angle(), -1.0 / r)
                } else {
                    (Vec2::from_angle(a0).perp().angle(), 1.0 / r)
                }
            }
        }
    }

    fn bulge(&self, reversed: bool) -> f64 {
        match *self {
            Piece::Line { .. } => 0.0,
            Piece::Arc { sw, .. } => {
                let b = (sw / 4.0).tan();
                if reversed {
                    -b
                } else {
                    b
                }
            }
        }
    }

    /// Points along it in the direction of travel, the start included and
    /// the end left out.
    fn points(&self, reversed: bool, out: &mut Vec<Vec2>) {
        match *self {
            Piece::Line { a, b } => out.push(if reversed { b } else { a }),
            Piece::Arc { sw, .. } => {
                let n = ((sw / (PI / 90.0)).ceil() as usize).clamp(1, 720);
                for i in 0..n {
                    let f = i as f64 / n as f64;
                    out.push(self.at(if reversed { 1.0 - f } else { f }));
                }
            }
        }
    }
}

fn pieces_from_curve(c: &Curve, out: &mut Vec<Piece>, lines: &mut Vec<(Vec2, Vec2, bool)>) {
    match *c {
        Curve::Seg { a, b } => {
            if a.distance(b) > 0.0 {
                out.push(Piece::Line { a, b });
            }
        }
        Curve::Arc { c, r, start, end, full } => {
            if r <= 0.0 || !r.is_finite() {
                return;
            }
            if full {
                // Two halves, so the circle has points to meet others at.
                out.push(Piece::Arc { c, r, a0: 0.0, sw: PI });
                out.push(Piece::Arc { c, r, a0: PI, sw: PI });
            } else {
                out.push(Piece::Arc {
                    c,
                    r,
                    a0: start,
                    sw: normalize_sweep(end - start),
                });
            }
        }
        Curve::Line { a, u, ray } => lines.push((a, u, ray)),
    }
}

/// Whether an entity bounds hatches: outlines do, annotation does not.
fn bounds(kind: &ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::Line(_)
            | ObjectKind::Arc(_)
            | ObjectKind::Circle(_)
            | ObjectKind::LwPolyline(_)
            | ObjectKind::Ellipse(_)
            | ObjectKind::Spline(_)
            | ObjectKind::Xline(_)
            | ObjectKind::Ray(_)
            | ObjectKind::Solid(_)
    )
}

/// The region around `p` bounded by the visible outlines in `block` (a
/// space or block record).
pub fn region_at(d: &Drawing, block: Handle, p: Vec2) -> Result<Region, String> {
    let mut pieces = Vec::new();
    let mut lines = Vec::new();
    for o in d.block_entities(block) {
        if !bounds(&o.kind) {
            continue;
        }
        if let Some(e) = &o.entity {
            if e.invisible {
                continue;
            }
            if let Some(l) = d.layer(e.layer) {
                if l.off || l.frozen {
                    continue;
                }
            }
        }
        for c in modify::curves_of(&o.kind) {
            pieces_from_curve(&c, &mut pieces, &mut lines);
        }
        if pieces.len() > MAX_PIECES {
            return Err("there are too many objects to search for a boundary: select the boundary objects instead".into());
        }
    }
    region_from_pieces(pieces, &lines, p)
}

/// [`region_at`] for loose curves (for tests and other callers).
pub fn region_of_curves(curves: &[Curve], p: Vec2) -> Result<Region, String> {
    let mut pieces = Vec::new();
    let mut lines = Vec::new();
    for c in curves {
        pieces_from_curve(c, &mut pieces, &mut lines);
    }
    region_from_pieces(pieces, &lines, p)
}

fn region_from_pieces(mut pieces: Vec<Piece>, lines: &[(Vec2, Vec2, bool)], p: Vec2) -> Result<Region, String> {
    if !p.is_finite() {
        return Err("the point is not a number".into());
    }
    let mut ext = BBox::EMPTY;
    for pc in &pieces {
        ext.union(&pc.bbox());
    }
    ext.add(p);
    let size = ext.width().max(ext.height()).max(1e-6);
    // Construction lines are cut to a box well beyond everything else.
    if !lines.is_empty() {
        let m = size * 2.0 + 1.0;
        let bx = BBox {
            min: ext.min - Vec2::new(m, m),
            max: ext.max + Vec2::new(m, m),
        };
        for &(a, u, ray) in lines {
            if let Some((s, e)) = crate::render::clip::clip_ray(a, u, ray, &bx) {
                if s.distance(e) > 0.0 {
                    pieces.push(Piece::Line { a: s, b: e });
                }
            }
        }
    }
    let tol = size * 1e-9;
    let pieces = split(&pieces, tol);
    let g = Graph::build(&pieces, tol * 10.0);
    g.region(p, tol)
}

/// Every piece cut where it meets another.
fn split(pieces: &[Piece], tol: f64) -> Vec<Piece> {
    let n = pieces.len();
    let boxes: Vec<BBox> = pieces.iter().map(|p| p.bbox()).collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| boxes[i].min.x.total_cmp(&boxes[j].min.x));
    let mut cuts: Vec<Vec<f64>> = vec![Vec::new(); n];
    for (k, &i) in order.iter().enumerate() {
        for &j in &order[k + 1..] {
            if boxes[j].min.x > boxes[i].max.x + tol {
                break;
            }
            if boxes[j].min.y > boxes[i].max.y + tol || boxes[i].min.y > boxes[j].max.y + tol {
                continue;
            }
            let (pi, pj) = (pieces[i], pieces[j]);
            let mut pts = modify::intersect(&pi.curve(), &pj.curve());
            pts.extend(overlaps(&pi, &pj, tol));
            for x in pts {
                cuts[i].push(pi.param(x));
                cuts[j].push(pj.param(x));
            }
        }
    }
    let mut out = Vec::with_capacity(n);
    for (i, pc) in pieces.iter().enumerate() {
        let len = pc.length();
        if len <= tol {
            continue;
        }
        let eps = tol / len;
        let mut ts: Vec<f64> = cuts[i].iter().copied().filter(|t| *t > eps && *t < 1.0 - eps).collect();
        ts.sort_by(f64::total_cmp);
        let mut last = 0.0;
        for t in ts.into_iter().chain([1.0]) {
            if t - last > eps {
                out.push(pc.sub(last, t));
                last = t;
            }
        }
    }
    out
}

/// Where two pieces lying on the same line or circle overlap: the ends of
/// each that fall on the other.
fn overlaps(p: &Piece, q: &Piece, tol: f64) -> Vec<Vec2> {
    let mut out = Vec::new();
    match (*p, *q) {
        (Piece::Line { a, b }, Piece::Line { a: c, b: e }) => {
            let d = b - a;
            let len = d.length();
            if len == 0.0 {
                return out;
            }
            let u = d / len;
            let off = |x: Vec2| (x - a).cross(u).abs();
            if off(c) > tol || off(e) > tol {
                return out;
            }
            for x in [c, e] {
                let t = (x - a).dot(u);
                if t > -tol && t < len + tol {
                    out.push(x);
                }
            }
            let f = e - c;
            let fl = f.length();
            if fl > 0.0 {
                for x in [a, b] {
                    let t = (x - c).dot(f / fl);
                    if t > -tol && t < fl + tol {
                        out.push(x);
                    }
                }
            }
        }
        (Piece::Arc { c, r, .. }, Piece::Arc { c: c2, r: r2, .. }) => {
            if c.distance(c2) > tol || (r - r2).abs() > tol {
                return out;
            }
            let on = |pc: &Piece, x: Vec2| {
                let t = pc.param(x);
                t > -1e-9 && t < 1.0 + 1e-9
            };
            for x in [q.start(), q.end()] {
                if on(p, x) {
                    out.push(x);
                }
            }
            for x in [p.start(), p.end()] {
                if on(q, x) {
                    out.push(x);
                }
            }
        }
        _ => {}
    }
    out
}

/// The pieces as a plane graph: vertices where pieces end, each piece an
/// edge from the vertex at its start to the one at its end.
struct Graph {
    verts: Vec<Vec2>,
    edges: Vec<(usize, usize, Piece)>,
}

impl Graph {
    fn build(pieces: &[Piece], tol: f64) -> Graph {
        let mut verts: Vec<Vec2> = Vec::new();
        let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        let cell = tol.max(1e-300) * 4.0;
        let mut vertex = |p: Vec2, verts: &mut Vec<Vec2>| -> usize {
            let (gx, gy) = ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(list) = grid.get(&(gx + dx, gy + dy)) {
                        for &i in list {
                            if verts[i].distance(p) <= tol {
                                return i;
                            }
                        }
                    }
                }
            }
            verts.push(p);
            grid.entry((gx, gy)).or_default().push(verts.len() - 1);
            verts.len() - 1
        };
        let mut edges = Vec::new();
        let mut seen: HashMap<(usize, usize, i64, i64), ()> = HashMap::new();
        for pc in pieces {
            let u = vertex(pc.start(), &mut verts);
            let v = vertex(pc.end(), &mut verts);
            if u == v {
                continue;
            }
            // The same edge twice (an outline drawn over another).
            let mid = pc.at(0.5);
            let key = (u.min(v), u.max(v), (mid.x / cell).round() as i64, (mid.y / cell).round() as i64);
            if seen.insert(key, ()).is_some() {
                continue;
            }
            edges.push((u, v, *pc));
        }
        // Clear loose ends: edges with an end that meets nothing.
        let mut degree = vec![0usize; verts.len()];
        for (u, v, _) in &edges {
            degree[*u] += 1;
            degree[*v] += 1;
        }
        let mut alive = vec![true; edges.len()];
        let mut by_vertex: Vec<Vec<usize>> = vec![Vec::new(); verts.len()];
        for (i, (u, v, _)) in edges.iter().enumerate() {
            by_vertex[*u].push(i);
            by_vertex[*v].push(i);
        }
        let mut stack: Vec<usize> = (0..verts.len()).filter(|&i| degree[i] == 1).collect();
        while let Some(x) = stack.pop() {
            if degree[x] != 1 {
                continue;
            }
            let Some(&e) = by_vertex[x].iter().find(|&&e| alive[e]) else {
                continue;
            };
            alive[e] = false;
            let (u, v, _) = edges[e];
            degree[u] -= 1;
            degree[v] -= 1;
            let other = if u == x { v } else { u };
            if degree[other] == 1 {
                stack.push(other);
            }
        }
        let edges = edges.into_iter().zip(alive).filter(|(_, a)| *a).map(|(e, _)| e).collect();
        Graph { verts, edges }
    }

    fn region(&self, p: Vec2, tol: f64) -> Result<Region, String> {
        let ne = self.edges.len();
        if ne == 0 {
            return Err("no closed boundary was found around the point".into());
        }
        let nh = 2 * ne;
        let origin = |h: usize| {
            let (u, v, _) = self.edges[h / 2];
            if h.is_multiple_of(2) {
                u
            } else {
                v
            }
        };
        // Outgoing half-edges at each vertex, counter-clockwise; edges
        // leaving in the same direction ordered by how they turn.
        let mut around: Vec<Vec<(i64, f64, usize)>> = vec![Vec::new(); self.verts.len()];
        for h in 0..nh {
            let (_, _, pc) = self.edges[h / 2];
            let (ang, curv) = pc.leave(h % 2 == 1);
            around[origin(h)].push(((ang / 1e-9).round() as i64, curv, h));
        }
        let mut pos = vec![0usize; nh];
        for list in &mut around {
            list.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
            for (i, item) in list.iter().enumerate() {
                pos[item.2] = i;
            }
        }
        let next = |h: usize| -> usize {
            let t = h ^ 1;
            let list = &around[origin(t)];
            let i = pos[t];
            list[(i + list.len() - 1) % list.len()].2
        };
        // Components, to tell islands from the region's own edges.
        let mut parent: Vec<usize> = (0..self.verts.len()).collect();
        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }
        for (u, v, _) in &self.edges {
            let (a, b) = (find(&mut parent, *u), find(&mut parent, *v));
            if a != b {
                parent[a] = b;
            }
        }
        // Walk every face.
        struct Cycle {
            halves: Vec<usize>,
            poly: Vec<Vec2>,
            area: f64,
            comp: usize,
        }
        let mut visited = vec![false; nh];
        let mut cycles: Vec<Cycle> = Vec::new();
        for h0 in 0..nh {
            if visited[h0] {
                continue;
            }
            let mut halves = Vec::new();
            let mut h = h0;
            let ok;
            loop {
                if visited[h] {
                    ok = h == h0;
                    break;
                }
                visited[h] = true;
                halves.push(h);
                h = next(h);
                if halves.len() > nh {
                    ok = false;
                    break;
                }
            }
            if !ok {
                continue;
            }
            let mut poly = Vec::new();
            for &x in &halves {
                self.edges[x / 2].2.points(x % 2 == 1, &mut poly);
            }
            let area = signed_area(&poly);
            let comp = find(&mut parent, origin(h0));
            cycles.push(Cycle { halves, poly, area, comp });
        }
        let tiny = tol * tol;
        let outer = cycles
            .iter()
            .filter(|c| c.area > tiny && inside(&c.poly, p))
            .min_by(|a, b| a.area.total_cmp(&b.area))
            .ok_or("no closed boundary was found around the point")?;
        let as_loop = |c: &Cycle| -> Loop {
            c.halves
                .iter()
                .map(|&x| {
                    let (_, _, pc) = self.edges[x / 2];
                    let rev = x % 2 == 1;
                    (if rev { pc.end() } else { pc.start() }, pc.bulge(rev))
                })
                .collect()
        };
        let islands = cycles
            .iter()
            .filter(|c| c.area < -tiny && c.comp != outer.comp && c.poly.first().is_some_and(|q| inside(&outer.poly, *q)))
            .map(as_loop)
            .collect();
        Ok(Region {
            outer: as_loop(outer),
            islands,
        })
    }
}

fn signed_area(poly: &[Vec2]) -> f64 {
    let n = poly.len();
    let mut s = 0.0;
    for i in 0..n {
        s += poly[i].cross(poly[(i + 1) % n]);
    }
    s / 2.0
}

/// Even-odd point in polygon.
fn inside(poly: &[Vec2], p: Vec2) -> bool {
    let n = poly.len();
    let mut c = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if p.x < x {
                c = !c;
            }
        }
    }
    c
}

/// The loop of an object that is closed already: a circle, a closed
/// polyline, a whole ellipse or a closed spline.
pub fn loop_of(kind: &ObjectKind) -> Result<Loop, String> {
    let mut k = kind.clone();
    modify::to_world_plane(&mut k)?;
    match &k {
        ObjectKind::Circle(c) => {
            let (cc, r) = (c.center.xy(), c.radius);
            if r <= 0.0 {
                return Err("the circle has no size".into());
            }
            Ok(vec![(cc + Vec2::new(r, 0.0), 1.0), (cc - Vec2::new(r, 0.0), 1.0)])
        }
        ObjectKind::LwPolyline(p) => {
            let mut pts: Vec<(Vec2, f64)> = p.points.iter().enumerate().map(|(i, q)| (*q, p.bulge(i))).collect();
            // A last point on the first closes it too.
            let closes = pts.len() > 2 && pts[0].0.distance(pts[pts.len() - 1].0) < 1e-9;
            if !p.is_closed() && !closes {
                return Err("the polyline is not closed".into());
            }
            if closes && !p.is_closed() {
                pts.pop();
            }
            if pts.len() < 2 {
                return Err("the polyline has no area".into());
            }
            Ok(pts)
        }
        ObjectKind::Ellipse(e) => {
            let full = (normalize_sweep(e.end_param - e.start_param) - TAU).abs() < 1e-9;
            if !full {
                return Err("an elliptical arc is not closed".into());
            }
            let n = e.extrusion.normalized();
            let n = if n.length() < 0.5 { Vec3::Z } else { n };
            let minor = n.cross(e.major_axis) * e.axis_ratio;
            let mut pts: Vec<Vec2> = tessellate_ellipse(e.center, e.major_axis, minor, 0.0, TAU, PI / 180.0)
                .iter()
                .map(|q| q.xy())
                .collect();
            pts.pop();
            Ok(pts.into_iter().map(|q| (q, 0.0)).collect())
        }
        ObjectKind::Spline(sp) => {
            let mut pts: Vec<Vec2> = if sp.control_points.len() >= 2 && !sp.knots.is_empty() {
                tessellate_nurbs(sp.degree as usize, &sp.knots, &sp.control_points, &sp.weights).iter().map(|q| q.xy()).collect()
            } else {
                sp.fit_points.iter().map(|q| q.xy()).collect()
            };
            let closes = pts.len() > 2 && pts[0].distance(pts[pts.len() - 1]) < 1e-9;
            if !sp.closed && !closes {
                return Err("the spline is not closed".into());
            }
            if closes {
                pts.pop();
            }
            Ok(pts.into_iter().map(|q| (q, 0.0)).collect())
        }
        _ => Err("only circles, closed polylines, ellipses and closed splines can bound a hatch".into()),
    }
}

/// Boundary path flags (DXF 92): external, polyline, derived (found by
/// picking a point), outermost.
pub mod flags {
    pub const EXTERNAL: u32 = 1;
    pub const POLYLINE: u32 = 2;
    pub const DERIVED: u32 = 4;
    pub const OUTERMOST: u32 = 16;
}

/// A pattern as HATCH asks for it: a name from the pattern library (or
/// USER, or SOLID), its scale (for USER, the line spacing), its angle in
/// radians, and for USER whether the lines are crossed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HatchPattern<'a> {
    pub name: &'a str,
    pub scale: f64,
    pub angle: f64,
    pub double: bool,
}

/// A hatch of `loops` (the first the outline) with `pattern`. `derived`
/// marks loops found by picking a point; `seed` is that point. Too fine a
/// pattern for the area is refused.
pub fn hatch(
    loops: &[Loop],
    derived: bool,
    pattern: HatchPattern<'_>,
    unit: Unit,
    seed: Option<Vec2>,
) -> Result<Hatch, String> {
    let HatchPattern { name: pattern, scale, angle, double } = pattern;
    if loops.is_empty() {
        return Err("nothing to hatch".into());
    }
    if !(scale.is_finite() && scale > 0.0) {
        return Err("the pattern scale must be more than zero".into());
    }
    if !angle.is_finite() {
        return Err("the pattern angle is not a number".into());
    }
    let upper = pattern.trim().to_ascii_uppercase();
    let user = upper == "USER" || upper == "_USER" || upper == "U";
    let name = if user { "USER".to_string() } else { upper };
    let lines = crate::render::hatch::pattern_lines(&name, scale, angle, double && user, unit)
        .ok_or_else(|| format!("there is no hatch pattern {:?}; one of {}", pattern.trim(), crate::render::hatch::names().join(", ")))?;
    let solid = name == "SOLID";
    let rings: Vec<Vec<Vec2>> = loops
        .iter()
        .map(|l| crate::geom::curve::tessellate_bulge_polyline(l, true, PI / 90.0))
        .collect();
    if !solid && crate::render::hatch::fill_lines(&rings, &lines).is_none() {
        return Err("the pattern is too fine for this area: make its scale larger".into());
    }
    let d = if derived { flags::DERIVED } else { 0 };
    let paths = loops
        .iter()
        .enumerate()
        .map(|(i, l)| HatchPath::Polyline {
            flags: flags::POLYLINE | d | if i == 0 { flags::EXTERNAL } else { flags::OUTERMOST },
            closed: true,
            vertices: l.clone(),
        })
        .collect();
    Ok(Hatch {
        elevation: 0.0,
        extrusion: Vec3::Z,
        pattern_name: if user { "_USER".into() } else { name },
        solid_fill: solid,
        associative: false,
        is_gradient: false,
        paths,
        style: 0,
        pattern_type: if user { 0 } else { 1 },
        pattern_angle: angle,
        pattern_scale: scale,
        pattern_double: double && user,
        pattern_lines: lines,
        seed_points: seed.into_iter().collect(),
        // Not associative, so no boundary objects.
        sources: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(ax: f64, ay: f64, bx: f64, by: f64) -> Curve {
        Curve::Seg {
            a: Vec2::new(ax, ay),
            b: Vec2::new(bx, by),
        }
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Curve> {
        vec![seg(x0, y0, x1, y0), seg(x1, y0, x1, y1), seg(x1, y1, x0, y1), seg(x0, y1, x0, y0)]
    }

    fn circle(x: f64, y: f64, r: f64) -> Curve {
        Curve::Arc {
            c: Vec2::new(x, y),
            r,
            start: 0.0,
            end: TAU,
            full: true,
        }
    }

    fn area(l: &Loop) -> f64 {
        signed_area(&crate::geom::curve::tessellate_bulge_polyline(l, true, PI / 720.0))
    }

    #[test]
    fn a_rectangle_with_a_hole() {
        let mut cs = rect(0.0, 0.0, 100.0, 50.0);
        cs.push(circle(30.0, 25.0, 10.0));
        let r = region_of_curves(&cs, Vec2::new(70.0, 25.0)).unwrap();
        assert!((area(&r.outer) - 5000.0).abs() < 1e-6, "{}", area(&r.outer));
        assert_eq!(r.islands.len(), 1);
        assert!((area(&r.islands[0]).abs() - PI * 100.0).abs() < 0.05);
        // Inside the hole: the circle is the region.
        let r = region_of_curves(&cs, Vec2::new(30.0, 25.0)).unwrap();
        assert!((area(&r.outer) - PI * 100.0).abs() < 0.05);
        assert!(r.islands.is_empty());
        // Outside everything.
        assert!(region_of_curves(&cs, Vec2::new(200.0, 25.0)).is_err());
    }

    #[test]
    fn crossing_lines_make_faces() {
        // Two overlapping rectangles: three faces.
        let mut cs = rect(0.0, 0.0, 60.0, 40.0);
        cs.extend(rect(40.0, 20.0, 100.0, 80.0));
        let r = region_of_curves(&cs, Vec2::new(50.0, 30.0)).unwrap();
        assert!((area(&r.outer) - 400.0).abs() < 1e-6, "{}", area(&r.outer));
        let r = region_of_curves(&cs, Vec2::new(10.0, 10.0)).unwrap();
        assert!((area(&r.outer) - (2400.0 - 400.0)).abs() < 1e-6, "{}", area(&r.outer));
        // A loose line inside does not split the region.
        cs.push(seg(5.0, 5.0, 15.0, 5.0));
        let r = region_of_curves(&cs, Vec2::new(10.0, 10.0)).unwrap();
        assert!((area(&r.outer) - 2000.0).abs() < 1e-6);
        // A line across a rectangle does.
        let mut cs = rect(0.0, 0.0, 100.0, 50.0);
        cs.push(seg(40.0, -10.0, 40.0, 60.0));
        let r = region_of_curves(&cs, Vec2::new(10.0, 10.0)).unwrap();
        assert!((area(&r.outer) - 2000.0).abs() < 1e-6);
    }

    #[test]
    fn rounded_slot_and_nested_islands() {
        // A slot: two lines and two half circles.
        let cs = vec![
            seg(0.0, 0.0, 40.0, 0.0),
            seg(40.0, 20.0, 0.0, 20.0),
            Curve::Arc { c: Vec2::new(40.0, 10.0), r: 10.0, start: -PI / 2.0, end: PI / 2.0, full: false },
            Curve::Arc { c: Vec2::new(0.0, 10.0), r: 10.0, start: PI / 2.0, end: 3.0 * PI / 2.0, full: false },
        ];
        let r = region_of_curves(&cs, Vec2::new(20.0, 10.0)).unwrap();
        assert!((area(&r.outer) - (800.0 + PI * 100.0)).abs() < 0.01, "{}", area(&r.outer));
        // Islands inside islands are all listed (alternating fill).
        let mut cs = rect(0.0, 0.0, 100.0, 100.0);
        cs.extend(rect(20.0, 20.0, 80.0, 80.0));
        cs.push(circle(50.0, 50.0, 10.0));
        let r = region_of_curves(&cs, Vec2::new(10.0, 10.0)).unwrap();
        assert_eq!(r.islands.len(), 2);
        // Construction lines bound regions too.
        let cs = vec![
            Curve::Line { a: Vec2::new(0.0, 0.0), u: Vec2::new(1.0, 0.0), ray: false },
            Curve::Line { a: Vec2::new(0.0, 0.0), u: Vec2::new(0.0, 1.0), ray: false },
            seg(10.0, -5.0, -5.0, 10.0),
        ];
        let r = region_of_curves(&cs, Vec2::new(1.0, 1.0)).unwrap();
        assert!((area(&r.outer) - 12.5).abs() < 1e-6, "{}", area(&r.outer));
    }

    #[test]
    fn hatches_are_made_from_regions() {
        let mut cs = rect(0.0, 0.0, 100.0, 50.0);
        cs.push(circle(30.0, 25.0, 10.0));
        let r = region_of_curves(&cs, Vec2::new(70.0, 25.0)).unwrap();
        let mut loops = vec![r.outer.clone()];
        loops.extend(r.islands.iter().cloned());
        let p = |name, scale, angle, double| HatchPattern { name, scale, angle, double };
        let h = hatch(&loops, true, p("ansi31", 1.0, 0.0, false), Unit::Millimeter, Some(Vec2::new(70.0, 25.0))).unwrap();
        assert_eq!(h.pattern_name, "ANSI31");
        assert_eq!(h.paths.len(), 2);
        assert_eq!(h.pattern_lines.len(), 1);
        assert!(!h.solid_fill);
        let s = hatch(&loops, true, p("SOLID", 1.0, 0.0, false), Unit::Millimeter, None).unwrap();
        assert!(s.solid_fill && s.pattern_lines.is_empty());
        let u = hatch(&loops, false, p("user", 3.0, PI / 4.0, true), Unit::Millimeter, None).unwrap();
        assert_eq!((u.pattern_name.as_str(), u.pattern_type, u.pattern_lines.len()), ("_USER", 0, 2));
        assert!(hatch(&loops, true, p("NOPE", 1.0, 0.0, false), Unit::Millimeter, None).is_err());
        assert!(hatch(&loops, true, p("ANSI31", 1e-6, 0.0, false), Unit::Millimeter, None).is_err());
        // Closed objects give loops.
        let c = ObjectKind::Circle(Circle::new(Vec2::new(0.0, 0.0), 5.0));
        assert_eq!(loop_of(&c).unwrap().len(), 2);
        let open = ObjectKind::LwPolyline(LwPolyline::new(vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)], false));
        assert!(loop_of(&open).is_err());
    }
}

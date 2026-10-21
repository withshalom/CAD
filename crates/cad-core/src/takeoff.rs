//! Takeoff: the lengths, outlines, areas and weight a quote needs,
//! measured from the drawing (Proposed).
//!
//! The drawing says what is drawn; which layers hold the outline of the
//! part is the caller's choice (DESIGN §1: CAD, not CAM). By default the
//! `VISIBLE` layer of model space is measured, or every layer when the
//! drawing has none by that name.
//!
//! - **Length**: every measured curve, end to end.
//! - **Contours**: curves that close on themselves (a circle, a closed
//!   polyline, or open curves whose ends meet, chained in any order and
//!   direction). Each needs its own start when cut, as does each open
//!   path that does not close.
//! - **Area**: the outer contours less the holes inside them (islands
//!   inside holes count again), from the contours alone.
//! - **Weight**: area × the part's thickness × a nominal density for its
//!   material, when the drawing has units and the material is one this
//!   module knows.
//!
//! Block references, text, dimensions and hatches are not measured; they
//! are counted in `skipped`.
//!
//! **Repeats are set aside.** Drawings that came through other programs
//! often hold the same circle twice, or a short line lying on top of a
//! plate's edge (a real AC1032 file did both). Drawn twice, a curve would
//! be measured, and cut, twice, and a stray segment at a corner stops the
//! outline from closing. So before measuring, a curve that repeats another
//! (same shape, either direction) or a straight segment lying entirely
//! along another straight segment is set aside and reported in
//! `duplicates` and `duplicate_length`. The drawing is not changed;
//! OVERKILL (COMMANDS.md) is the command that cleans it. Segments that
//! only partly overlap are not merged.

use crate::geom::curve::{
    bulge_to_arc, tessellate_bulge_polyline, tessellate_ellipse, tessellate_nurbs,
};
use crate::geom::{BBox, Transform, Vec2, Vec3};
use crate::model::*;
use crate::units::Unit;
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::{PI, TAU};

/// Angular step for curves measured as polygons: fine enough that areas
/// and lengths are within about 0.02 % of the true curve.
const STEP: f64 = PI / 360.0;

/// What to measure.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Block record to measure; null for model space.
    pub block: Handle,
    /// Layer names (case-insensitive). Empty: `VISIBLE`, or every layer
    /// when the drawing has no layer of that name.
    pub layers: Vec<String>,
}

/// The measurements. Lengths and areas are in drawing units.
#[derive(Debug, Clone, PartialEq)]
pub struct Takeoff {
    pub block: Handle,
    pub block_name: String,
    /// The layers measured (empty when every layer was).
    pub layers: Vec<String>,
    pub unit: Unit,
    /// Entities measured.
    pub measured: usize,
    /// Entities on the measured layers that are not curves, by type.
    pub skipped: BTreeMap<String, usize>,
    pub length: f64,
    /// Closed contours: outer outlines and holes.
    pub contours: usize,
    /// Contours inside an odd number of others.
    pub holes: usize,
    /// Paths that do not close.
    pub open_paths: usize,
    /// Curves set aside because they repeat another curve or lie along a
    /// longer straight segment; not in `length`, the contours or the area.
    pub duplicates: usize,
    /// Their total length.
    pub duplicate_length: f64,
    /// Net area: outer contours less their holes.
    pub area: f64,
    pub bbox: BBox,
    /// The block record's part data.
    pub part: Part,
    /// kg/m³ for the part's material, when known.
    pub density: Option<f64>,
    /// Kilograms per part, when the material, thickness and units allow.
    pub weight_kg: Option<f64>,
}

/// A nominal density (kg/m³) for a material name, by the words in it:
/// stainless steel 8000, steel 7850, aluminium 2700, brass 8500, bronze
/// 8800, copper 8960, titanium 4510, zinc 7140. Alloys vary by a few per
/// cent; a quote needing better states its own.
pub fn density(material: &str) -> Option<f64> {
    let m = material.to_lowercase();
    let table = [
        ("stainless", 8000.0),
        ("steel", 7850.0),
        ("alumin", 2700.0),
        ("brass", 8500.0),
        ("bronze", 8800.0),
        ("copper", 8960.0),
        ("titanium", 4510.0),
        ("zinc", 7140.0),
    ];
    table.iter().find(|(k, _)| m.contains(k)).map(|(_, d)| *d)
}

/// One measured curve: its points (world XY) and its true length.
struct Piece {
    pts: Vec<Vec2>,
    length: f64,
    closed: bool,
}

fn xy(t: &Transform, p: Vec3) -> Vec2 {
    t.apply(p).xy()
}

fn polyline_length(pts: &[Vec2]) -> f64 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Length of a polyline with bulges, using true arc lengths.
fn bulge_length(verts: &[(Vec2, f64)], closed: bool) -> f64 {
    let n = verts.len();
    if n < 2 {
        return 0.0;
    }
    let segs = if closed { n } else { n - 1 };
    (0..segs)
        .map(|i| {
            let (p1, b) = verts[i];
            let p2 = verts[(i + 1) % n].0;
            match bulge_to_arc(p1, p2, b) {
                Some((arc, _)) => arc.radius * arc.sweep(),
                None => p1.distance(p2),
            }
        })
        .sum()
}

fn piece(d: &Drawing, o: &Object) -> Option<Piece> {
    use ObjectKind as K;
    let ocs = |n: Vec3| Transform::ocs_to_wcs(n);
    Some(match &o.kind {
        K::Line(l) => {
            let pts = vec![l.start.xy(), l.end.xy()];
            Piece {
                length: polyline_length(&pts),
                pts,
                closed: false,
            }
        }
        K::Circle(c) => {
            let t = ocs(c.extrusion);
            let n = (TAU / STEP) as usize;
            let pts = (0..=n)
                .map(|i| {
                    let a = TAU * i as f64 / n as f64;
                    xy(&t, c.center + Vec3::new(a.cos(), a.sin(), 0.0) * c.radius)
                })
                .collect();
            Piece {
                pts,
                length: TAU * c.radius,
                closed: true,
            }
        }
        K::Arc(a) => {
            let t = ocs(a.extrusion);
            let sweep = crate::geom::curve::normalize_sweep(a.end_angle - a.start_angle);
            let n = ((sweep / STEP).ceil() as usize).max(1);
            let pts = (0..=n)
                .map(|i| {
                    let ang = a.start_angle + sweep * i as f64 / n as f64;
                    xy(
                        &t,
                        a.center + Vec3::new(ang.cos(), ang.sin(), 0.0) * a.radius,
                    )
                })
                .collect();
            Piece {
                pts,
                length: a.radius * sweep,
                closed: false,
            }
        }
        K::LwPolyline(p) => {
            let t = ocs(p.extrusion);
            let verts: Vec<(Vec2, f64)> = p
                .points
                .iter()
                .enumerate()
                .map(|(i, v)| (xy(&t, v.extend(p.elevation)), p.bulge(i)))
                .collect();
            // A bulge in OCS turns the other way when the OCS is mirrored.
            let flip = t.apply_vector(Vec3::Z).z < 0.0;
            let verts: Vec<(Vec2, f64)> = verts
                .into_iter()
                .map(|(v, b)| (v, if flip { -b } else { b }))
                .collect();
            // Two bulged vertices make a closed outline (a donut, a circle
            // drawn as a polyline).
            let closed = p.is_closed() && verts.len() >= 2;
            Piece {
                pts: tessellate_bulge_polyline(&verts, closed, STEP),
                length: bulge_length(&verts, closed),
                closed,
            }
        }
        K::Polyline(p)
            if p.kind == PolylineKind::Polyline2d || p.kind == PolylineKind::Polyline3d =>
        {
            let flat = p.kind == PolylineKind::Polyline2d;
            let t = if flat {
                ocs(p.extrusion)
            } else {
                Transform::default()
            };
            // As for LWPOLYLINE: the elevation places a 2D polyline's plane,
            // and a mirrored OCS turns its bulges the other way.
            let flip = flat && t.apply_vector(Vec3::Z).z < 0.0;
            let verts: Vec<(Vec2, f64)> = d
                .polyline_vertices(o)
                .iter()
                .map(|v| {
                    let at = if flat {
                        Vec3::new(v.point.x, v.point.y, p.elevation)
                    } else {
                        v.point
                    };
                    (xy(&t, at), if flip { -v.bulge } else { v.bulge })
                })
                .collect();
            let closed = p.is_closed() && verts.len() >= 2;
            Piece {
                pts: tessellate_bulge_polyline(&verts, closed, STEP),
                length: bulge_length(&verts, closed),
                closed,
            }
        }
        K::Ellipse(e) => {
            let n = e.extrusion.normalized();
            let n = if n.length() < 0.5 { Vec3::Z } else { n };
            let minor = n.cross(e.major_axis) * e.axis_ratio;
            let full = (e.end_param - e.start_param - TAU).abs() < 1e-9
                || (e.end_param - e.start_param).abs() < 1e-12;
            let pts: Vec<Vec2> = tessellate_ellipse(
                e.center,
                e.major_axis,
                minor,
                e.start_param,
                e.end_param,
                STEP,
            )
            .into_iter()
            .map(|p| p.xy())
            .collect();
            Piece {
                length: polyline_length(&pts),
                pts,
                closed: full,
            }
        }
        K::Spline(s) => {
            let pts: Vec<Vec2> = if s.control_points.len() > s.degree as usize {
                tessellate_nurbs(s.degree as usize, &s.knots, &s.control_points, &s.weights)
                    .into_iter()
                    .map(|p| p.xy())
                    .collect()
            } else {
                s.fit_points.iter().map(|p| p.xy()).collect()
            };
            let closed = (s.closed || s.periodic) && pts.len() > 2;
            Piece {
                length: polyline_length(&pts),
                pts,
                closed,
            }
        }
        _ => return None,
    })
}

/// Bounding box of a piece.
fn piece_box(p: &Piece) -> BBox {
    let mut b = BBox::EMPTY;
    for q in &p.pts {
        b.add(*q);
    }
    b
}

/// Distance from `p` to the segment `a`–`b`.
fn to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Whether piece `a` repeats piece `b`, or (both straight) lies along it.
fn covered_by(a: &Piece, b: &Piece, tol: f64) -> bool {
    if a.closed != b.closed {
        return false;
    }
    let (ba, bb) = (piece_box(a), piece_box(b));
    let near = |x: f64, y: f64| (x - y).abs() <= tol;
    if a.pts.len() == 2 && b.pts.len() == 2 {
        // A straight segment lying along another.
        return to_segment(a.pts[0], b.pts[0], b.pts[1]) <= tol
            && to_segment(a.pts[1], b.pts[0], b.pts[1]) <= tol;
    }
    // The same curve: same box, length and shape.
    if !(near(ba.min.x, bb.min.x)
        && near(ba.min.y, bb.min.y)
        && near(ba.max.x, bb.max.x)
        && near(ba.max.y, bb.max.y)
        && (a.length - b.length).abs() <= tol * a.pts.len().max(1) as f64)
    {
        return false;
    }
    if a.closed {
        let (sa, sb) = (signed_area(&a.pts).abs(), signed_area(&b.pts).abs());
        return (sa - sb).abs() <= tol * (a.length + b.length);
    }
    let (a0, a1) = (a.pts[0], *a.pts.last().unwrap_or(&a.pts[0]));
    let (b0, b1) = (b.pts[0], *b.pts.last().unwrap_or(&b.pts[0]));
    let am = a.pts[a.pts.len() / 2];
    let ends = (a0.distance(b0) <= tol && a1.distance(b1) <= tol)
        || (a0.distance(b1) <= tol && a1.distance(b0) <= tol);
    ends && b.pts.iter().any(|q| q.distance(am) <= tol * 1e3)
}

/// Boxes indexed by the grid cells they cover, to find the boxes that
/// contain a point without testing every one. Boxes spanning too many
/// cells sit in a short list returned with every query.
struct BoxGrid {
    origin: Vec2,
    side: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
    big: Vec<usize>,
}

impl BoxGrid {
    fn new(boxes: &[BBox], pad: f64) -> BoxGrid {
        let mut all = BBox::EMPTY;
        for b in boxes.iter().filter(|b| !b.is_empty()) {
            all.add(b.min);
            all.add(b.max);
        }
        let origin = if all.is_empty() { Vec2::ZERO } else { all.min };
        let side = (all.width().max(all.height()) / 256.0)
            .max(pad * 16.0)
            .max(1e-12);
        let mut g = BoxGrid {
            origin,
            side,
            cells: HashMap::new(),
            big: Vec::new(),
        };
        for (j, b) in boxes.iter().enumerate() {
            if b.is_empty() {
                continue;
            }
            let (x0, y0) = g.cell(Vec2::new(b.min.x - pad, b.min.y - pad));
            let (x1, y1) = g.cell(Vec2::new(b.max.x + pad, b.max.y + pad));
            if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > 1024 {
                g.big.push(j);
                continue;
            }
            for cx in x0..=x1 {
                for cy in y0..=y1 {
                    g.cells.entry((cx, cy)).or_default().push(j);
                }
            }
        }
        g
    }

    fn cell(&self, p: Vec2) -> (i64, i64) {
        (
            ((p.x - self.origin.x) / self.side).floor() as i64,
            ((p.y - self.origin.y) / self.side).floor() as i64,
        )
    }

    /// Indexes of the boxes that may contain `p`.
    fn near(&self, p: Vec2) -> impl Iterator<Item = usize> + '_ {
        self.cells
            .get(&self.cell(p))
            .into_iter()
            .flatten()
            .chain(self.big.iter())
            .copied()
    }
}

/// Split pieces into those to measure and those set aside as repeats
/// (see the module notes). Of two identical pieces the first is kept; a
/// straight segment is set aside when it lies along a longer one.
fn set_aside_repeats(pieces: Vec<Piece>, tol: f64) -> (Vec<Piece>, usize, f64) {
    let n = pieces.len();
    let boxes: Vec<BBox> = pieces.iter().map(piece_box).collect();
    // A piece can only be covered by one whose box contains its own.
    let grid = BoxGrid::new(&boxes, tol);
    let mut drop = vec![false; n];
    for i in 0..n {
        let bi = &boxes[i];
        if bi.is_empty() {
            continue;
        }
        for j in grid.near(bi.center()) {
            if i == j || drop[j] {
                continue;
            }
            let bj = &boxes[j];
            if bi.min.x < bj.min.x - tol
                || bi.min.y < bj.min.y - tol
                || bi.max.x > bj.max.x + tol
                || bi.max.y > bj.max.y + tol
            {
                continue; // i is not inside j's box
            }
            if !covered_by(&pieces[i], &pieces[j], tol) {
                continue;
            }
            // Same length: keep the first. Otherwise the shorter goes.
            let same = (pieces[i].length - pieces[j].length).abs() <= tol;
            if (same && j < i) || (!same && pieces[i].length < pieces[j].length) {
                drop[i] = true;
                break;
            }
        }
    }
    let mut kept = Vec::with_capacity(n);
    let (mut count, mut length) = (0, 0.0);
    for (p, d) in pieces.into_iter().zip(drop) {
        if d {
            count += 1;
            length += p.length;
        } else {
            kept.push(p);
        }
    }
    (kept, count, length)
}

/// Signed area of a closed polygon (shoelace).
fn signed_area(pts: &[Vec2]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    (0..n).map(|i| pts[i].cross(pts[(i + 1) % n])).sum::<f64>() / 2.0
}

/// Point in polygon by ray casting.
fn contains(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Chain open pieces whose ends meet (within `tol`) into paths. Returns
/// closed polygons and the number of paths that stay open.
fn chain(open: Vec<Piece>, tol: f64) -> (Vec<Vec<Vec2>>, usize) {
    let key = |p: Vec2| ((p.x / tol).round() as i64, (p.y / tol).round() as i64);
    // Cell → (piece, at its end?) for both ends of every piece.
    let mut ends: HashMap<(i64, i64), Vec<(usize, bool)>> = HashMap::new();
    for (i, pc) in open.iter().enumerate() {
        for (at_end, p) in [
            (false, pc.pts[0]),
            (true, *pc.pts.last().unwrap_or(&pc.pts[0])),
        ] {
            ends.entry(key(p)).or_default().push((i, at_end));
        }
    }
    let near = |p: Vec2, used: &[bool]| -> Option<(usize, bool)> {
        let (kx, ky) = key(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = ends.get(&(kx + dx, ky + dy)) {
                    for &(i, at_end) in v {
                        let q = if at_end {
                            *open[i].pts.last().unwrap_or(&open[i].pts[0])
                        } else {
                            open[i].pts[0]
                        };
                        if !used[i] && q.distance(p) <= tol {
                            return Some((i, at_end));
                        }
                    }
                }
            }
        }
        None
    };
    let mut used = vec![false; open.len()];
    let mut closed = Vec::new();
    let mut open_paths = 0;
    for start in 0..open.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut path: Vec<Vec2> = open[start].pts.clone();
        // Grow forwards from the end, then backwards from the start.
        for forwards in [true, false] {
            loop {
                let tip = if forwards {
                    *path.last().unwrap()
                } else {
                    path[0]
                };
                let Some((i, at_end)) = near(tip, &used) else {
                    break;
                };
                used[i] = true;
                let mut pts = open[i].pts.clone();
                // Orient the piece to continue from the tip.
                if forwards == at_end {
                    pts.reverse();
                }
                if forwards {
                    path.extend_from_slice(&pts[1..]);
                } else {
                    pts.pop();
                    pts.extend_from_slice(&path);
                    path = pts;
                }
            }
        }
        let first = path[0];
        let last = *path.last().unwrap();
        if path.len() > 3 && first.distance(last) <= tol {
            path.pop();
            closed.push(path);
        } else {
            open_paths += 1;
        }
    }
    (closed, open_paths)
}

fn layer_name(d: &Drawing, o: &Object) -> String {
    o.entity
        .as_ref()
        .and_then(|e| d.layer(e.layer))
        .map(|l| l.name.clone())
        .unwrap_or_default()
}

/// Measure a block (or model space) of a drawing.
pub fn takeoff(d: &Drawing, opts: &Options) -> Takeoff {
    let block = if opts.block.is_null() {
        d.model_space()
    } else {
        opts.block
    };
    let header = d.block_header(block);
    let mut layers: Vec<String> = opts.layers.iter().map(|l| l.to_uppercase()).collect();
    if layers.is_empty() && d.layer_by_name("VISIBLE").is_some() {
        layers.push("VISIBLE".into());
    }
    let mut out = Takeoff {
        block,
        block_name: header.map(|b| b.name.clone()).unwrap_or_default(),
        layers: layers.clone(),
        unit: Unit::of(d),
        measured: 0,
        skipped: BTreeMap::new(),
        length: 0.0,
        contours: 0,
        holes: 0,
        open_paths: 0,
        duplicates: 0,
        duplicate_length: 0.0,
        area: 0.0,
        bbox: BBox::EMPTY,
        part: header.map(|b| b.part.clone()).unwrap_or_default(),
        density: None,
        weight_kg: None,
    };
    let entities = if opts.block.is_null() {
        d.model_space_entities()
    } else {
        d.block_entities(block)
    };
    let mut pieces: Vec<Piece> = Vec::new();
    for o in entities {
        if !layers.is_empty() && !layers.contains(&layer_name(d, o).to_uppercase()) {
            continue;
        }
        if o.entity.as_ref().is_some_and(|e| e.invisible) {
            *out.skipped.entry("(invisible)".into()).or_default() += 1;
            continue;
        }
        match piece(d, o) {
            Some(p) if p.pts.len() >= 2 && p.length.is_finite() => {
                out.measured += 1;
                for q in &p.pts {
                    out.bbox.add(*q);
                }
                pieces.push(p);
            }
            _ => *out.skipped.entry(o.type_name.clone()).or_default() += 1,
        }
    }
    let size = out.bbox.width().max(out.bbox.height()).max(1.0);
    let tol = size * 1e-7;
    let (pieces, dups, dup_length) = set_aside_repeats(pieces, tol);
    out.duplicates = dups;
    out.duplicate_length = dup_length;
    let mut closed: Vec<Vec<Vec2>> = Vec::new();
    let mut open: Vec<Piece> = Vec::new();
    for p in pieces {
        out.length += p.length;
        if p.closed {
            let mut pts = p.pts;
            if pts.len() > 1 && pts[0].distance(*pts.last().unwrap()) < 1e-12 {
                pts.pop();
            }
            closed.push(pts);
        } else {
            open.push(p);
        }
    }
    let (chained, open_paths) = chain(open, tol);
    closed.extend(chained);
    out.open_paths = open_paths;
    out.contours = closed.len();
    // Depth of each contour: how many larger contours contain it. Only
    // contours whose box holds the probe point can.
    let areas: Vec<f64> = closed.iter().map(|c| signed_area(c).abs()).collect();
    let boxes: Vec<BBox> = closed
        .iter()
        .map(|c| {
            let mut b = BBox::EMPTY;
            for q in c {
                b.add(*q);
            }
            b
        })
        .collect();
    let grid = BoxGrid::new(&boxes, 0.0);
    for (i, c) in closed.iter().enumerate() {
        let probe = c[0];
        let depth = grid
            .near(probe)
            .filter(|&j| {
                j != i
                    && areas[j] > areas[i]
                    && (boxes[j].min.x..=boxes[j].max.x).contains(&probe.x)
                    && (boxes[j].min.y..=boxes[j].max.y).contains(&probe.y)
                    && contains(&closed[j], probe)
            })
            .count();
        if depth % 2 == 0 {
            out.area += areas[i];
        } else {
            out.area -= areas[i];
            out.holes += 1;
        }
    }
    out.density = density(&out.part.material);
    if let (Some(rho), Some(mm)) = (out.density, out.unit.mm()) {
        if out.part.thickness > 0.0 {
            let m3 = out.area * mm * mm * out.part.thickness * mm * 1e-9;
            out.weight_kg = Some(m3 * rho);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawing() -> (Drawing, Handle) {
        let d = crate::standards::new_drawing(crate::standards::Template::Metric);
        let visible = d.layer_by_name("VISIBLE").unwrap().0;
        (d, visible)
    }

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1e-9)
    }

    /// The pattern of a real splice plate (AC1032, through a nesting
    /// program): a 7 × 6.375 outline in eight lines, a short line lying on
    /// each side at the corners, eight 3/8" holes and four of them drawn
    /// twice. The repeats are set aside; the plate is one outline with
    /// eight holes.
    #[test]
    fn repeated_and_overlapping_curves_are_set_aside() {
        let (mut d, layer) = drawing();
        let line = |d: &mut Drawing, a: (f64, f64), b: (f64, f64)| {
            d.add_entity(
                layer,
                ObjectKind::Line(Line::new(Vec2::new(a.0, a.1), Vec2::new(b.0, b.1))),
            )
            .unwrap();
        };
        let (w, h, mx, my) = (7.0, 6.375, 3.5, 3.1875);
        for (x, y) in [(0.0, h), (w, h), (0.0, 0.0), (w, 0.0)] {
            line(&mut d, (x, y), (mx, y)); // half of the top or bottom edge
            line(&mut d, (x, y), (x, my)); // half of a side
            let inward = if y > 0.0 { y - 0.375 } else { 0.375 };
            line(&mut d, (x, y), (x, inward)); // lies on that side
        }
        let mut holes = Vec::new();
        for y in [0.375, h - 0.375] {
            for x in [1.375, 2.5, 4.5, 5.625] {
                holes.push((x, y));
            }
        }
        for (i, &(x, y)) in holes.iter().enumerate() {
            let c = ObjectKind::Circle(Circle::new(Vec2::new(x, y), 0.1875));
            d.add_entity(layer, c.clone()).unwrap();
            if i % 2 == 0 {
                d.add_entity(layer, c).unwrap(); // drawn twice
            }
        }
        let t = takeoff(&d, &Options::default());
        assert_eq!(t.measured, 24);
        assert_eq!(t.duplicates, 8);
        assert!(close(
            t.duplicate_length,
            4.0 * 0.375 + 4.0 * TAU * 0.1875,
            1e-3
        ));
        assert_eq!((t.contours, t.holes, t.open_paths), (9, 8, 0));
        let hole = PI * 0.1875 * 0.1875;
        assert!(close(t.area, w * h - 8.0 * hole, 1e-4), "{}", t.area);
        assert!(close(t.length, 2.0 * (w + h) + 8.0 * TAU * 0.1875, 1e-4));
    }

    #[test]
    fn a_plate_with_a_hole() {
        let (mut d, layer) = drawing();
        let mut rect = LwPolyline::new(
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(100.0, 0.0),
                Vec2::new(100.0, 50.0),
                Vec2::new(0.0, 50.0),
            ],
            true,
        );
        rect.bulges = vec![0.0; 4];
        d.add_entity(layer, ObjectKind::LwPolyline(rect)).unwrap();
        d.add_entity(
            layer,
            ObjectKind::Circle(Circle::new(Vec2::new(25.0, 25.0), 10.0)),
        )
        .unwrap();
        // On another layer: not measured.
        let dims = d.layer_by_name("DIMENSION").unwrap().0;
        d.add_entity(
            dims,
            ObjectKind::Line(Line::new(Vec2::ZERO, Vec2::new(500.0, 0.0))),
        )
        .unwrap();
        let ms = d.model_space();
        if let ObjectKind::BlockHeader(b) = &mut d.objects.get_mut(&ms).unwrap().kind {
            b.part = Part {
                material: "Mild steel".into(),
                grade: "S355JR".into(),
                thickness: 6.0,
                quantity: 4,
            };
        }
        let t = takeoff(&d, &Options::default());
        assert_eq!(t.layers, vec!["VISIBLE"]);
        assert_eq!(
            (t.measured, t.contours, t.holes, t.open_paths),
            (2, 2, 1, 0)
        );
        assert!(close(t.length, 300.0 + TAU * 10.0, 1e-12));
        let area = 5000.0 - PI * 100.0;
        assert!(close(t.area, area, 1e-4), "{}", t.area);
        assert_eq!(t.density, Some(7850.0));
        let kg = area * 6.0 * 1e-9 * 7850.0;
        assert!(close(t.weight_kg.unwrap(), kg, 1e-4));
        // Every layer.
        let t = takeoff(
            &d,
            &Options {
                layers: vec!["visible".into(), "Dimension".into()],
                ..Default::default()
            },
        );
        assert_eq!((t.measured, t.open_paths), (3, 1));
    }

    #[test]
    fn loose_lines_and_arcs_chain_into_contours() {
        let (mut d, layer) = drawing();
        // A 60 × 20 slot with round ends, drawn as two lines and two arcs
        // in no particular order or direction, plus a lone line.
        let pieces = [
            ObjectKind::Line(Line::new(Vec2::new(60.0, 20.0), Vec2::new(0.0, 20.0))),
            ObjectKind::Arc(Arc::new(Vec2::new(60.0, 10.0), 10.0, -PI / 2.0, PI / 2.0)),
            ObjectKind::Line(Line::new(Vec2::new(0.0, 0.0), Vec2::new(60.0, 0.0))),
            ObjectKind::Arc(Arc::new(
                Vec2::new(0.0, 10.0),
                10.0,
                PI / 2.0,
                3.0 * PI / 2.0,
            )),
            ObjectKind::Line(Line::new(Vec2::new(200.0, 0.0), Vec2::new(210.0, 0.0))),
        ];
        for k in pieces {
            d.add_entity(layer, k).unwrap();
        }
        let t = takeoff(&d, &Options::default());
        assert_eq!((t.contours, t.holes, t.open_paths), (1, 0, 1));
        assert!(close(t.length, 120.0 + TAU * 10.0 + 10.0, 1e-12));
        assert!(close(t.area, 60.0 * 20.0 + PI * 100.0, 1e-4), "{}", t.area);
        // The same slot as one polyline with bulges.
        let (mut d, layer) = drawing();
        let mut slot = LwPolyline::new(
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(60.0, 0.0),
                Vec2::new(60.0, 20.0),
                Vec2::new(0.0, 20.0),
            ],
            true,
        );
        slot.bulges = vec![0.0, 1.0, 0.0, 1.0];
        d.add_entity(layer, ObjectKind::LwPolyline(slot)).unwrap();
        let t = takeoff(&d, &Options::default());
        assert_eq!((t.contours, t.open_paths), (1, 0));
        assert!(close(t.length, 120.0 + TAU * 10.0, 1e-12));
        assert!(close(t.area, 60.0 * 20.0 + PI * 100.0, 1e-4), "{}", t.area);
        assert_eq!(t.weight_kg, None, "no material");
    }

    #[test]
    fn closed_two_vertex_and_mirrored_polylines() {
        // A circle of radius 10 drawn as a closed polyline of two bulges.
        let (mut d, layer) = drawing();
        let mut ring = LwPolyline::new(vec![Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0)], true);
        ring.bulges = vec![1.0, 1.0];
        d.add_entity(layer, ObjectKind::LwPolyline(ring)).unwrap();
        let t = takeoff(&d, &Options::default());
        assert_eq!((t.contours, t.open_paths), (1, 0));
        assert!(close(t.length, TAU * 10.0, 1e-12));
        assert!(close(t.area, PI * 100.0, 1e-4), "{}", t.area);

        // The same slot as a mirrored LWPOLYLINE and a mirrored 2D POLYLINE
        // (extrusion −Z): the same area either way.
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(60.0, 0.0),
            Vec2::new(60.0, 20.0),
            Vec2::new(0.0, 20.0),
        ];
        let bulges = [0.0, 1.0, 0.0, 1.0];
        let want = 60.0 * 20.0 + PI * 100.0;
        let (mut d, layer) = drawing();
        let mut lw = LwPolyline::new(pts.to_vec(), true);
        lw.bulges = bulges.to_vec();
        lw.extrusion = Vec3::new(0.0, 0.0, -1.0);
        d.add_entity(layer, ObjectKind::LwPolyline(lw)).unwrap();
        let t = takeoff(&d, &Options::default());
        assert!(close(t.area, want, 1e-4), "LWPOLYLINE {}", t.area);
        let (mut d, layer) = drawing();
        // An entity in model space, made a POLYLINE with its own vertices.
        let ph = d
            .add_entity(layer, ObjectKind::Line(Line::new(Vec2::ZERO, Vec2::ZERO)))
            .unwrap();
        let mut handles = Vec::new();
        for (p, b) in pts.iter().zip(bulges) {
            let v = Vertex {
                point: p.extend(0.0),
                bulge: b,
                ..Default::default()
            };
            let h = d.new_handle();
            d.objects.insert(
                h,
                Object::record(h, "VERTEX", 0x0A, ph, ObjectKind::Vertex(v)),
            );
            handles.push(h);
        }
        d.objects.get_mut(&ph).unwrap().kind = ObjectKind::Polyline(Polyline {
            flags: 1,
            extrusion: Vec3::new(0.0, 0.0, -1.0),
            vertices: handles,
            ..Default::default()
        });
        d.rebuild_index();
        let t = takeoff(&d, &Options::default());
        assert!(close(t.area, want, 1e-4), "POLYLINE {}", t.area);
        assert_eq!(t.contours, 1);
        // Invisible entities are not measured.
        let (mut d, layer) = drawing();
        let h = d
            .add_entity(layer, ObjectKind::Circle(Circle::new(Vec2::ZERO, 5.0)))
            .unwrap();
        d.objects
            .get_mut(&h)
            .unwrap()
            .entity
            .as_mut()
            .unwrap()
            .invisible = true;
        let t = takeoff(&d, &Options::default());
        assert_eq!((t.measured, t.skipped.get("(invisible)")), (0, Some(&1)));
    }

    #[test]
    fn islands_count_again_and_units_convert() {
        let mut d = crate::standards::new_drawing(crate::standards::Template::Imperial);
        let layer = d.layer_by_name("VISIBLE").unwrap().0;
        for (c, r) in [(0.0, 3.0), (0.0, 2.0), (0.0, 1.0)] {
            d.add_entity(layer, ObjectKind::Circle(Circle::new(Vec2::new(c, 0.0), r)))
                .unwrap();
        }
        let ms = d.model_space();
        if let ObjectKind::BlockHeader(b) = &mut d.objects.get_mut(&ms).unwrap().kind {
            b.part = Part {
                material: "Aluminium".into(),
                thickness: 0.25,
                ..Default::default()
            };
        }
        let t = takeoff(&d, &Options::default());
        assert_eq!((t.contours, t.holes), (3, 1));
        let area = PI * (9.0 - 4.0 + 1.0);
        assert!(close(t.area, area, 1e-4));
        // Square inches × inches → m³ through 25.4 mm.
        let kg = area * 25.4 * 25.4 * 0.25 * 25.4 * 1e-9 * 2700.0;
        assert!(close(t.weight_kg.unwrap(), kg, 1e-4));
        assert_eq!(density("Stainless steel 316L"), Some(8000.0));
        assert_eq!(density("unobtainium"), None);
    }
}

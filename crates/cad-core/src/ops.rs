//! Drafting operations on a drawing: what the modify and annotate commands
//! do, given handles, points and numbers. The geometry is [`modify`]'s and
//! [`boundary`]'s; this module finds the objects, checks their layers are
//! not locked, keeps their properties (layer, colour, linetype, extended
//! data) and what they own (an insert's attributes, an old polyline's
//! vertices), and puts results back in the space or block they came from.
//!
//! Errors are messages for the user. An operation that fails part way may
//! have changed some objects: callers record edits as one change
//! ([`Drawing::begin_change`]) and take it back on an error.

use crate::boundary;
use crate::geom::{BBox, Vec2, Vec3};
use crate::model::*;
use crate::modify::{self, Affine, Curve};
use crate::standards;
use crate::units::Unit;
use std::f64::consts::TAU;

type R<T> = Result<T, String>;

fn err(e: EditError) -> String {
    e.to_string()
}

fn no_object(h: Handle) -> String {
    format!("no object {:X}", h.0)
}

/// Most copies one array makes.
pub const MAX_ARRAY: usize = 10_000;

/// The block an entity belongs to: model space, the active paper space,
/// or the block or layout record that owns it.
pub fn block_of(d: &Drawing, h: Handle) -> R<Handle> {
    let o = d.get(h).ok_or_else(|| no_object(h))?;
    let e = o.entity.as_ref().ok_or("that is not a drawn object")?;
    Ok(match e.space {
        Space::Model => d.model_space(),
        Space::Paper => d.paper_space(),
        Space::Owned => o.owner,
    })
}

/// Refuse objects on locked layers, with the layer's name.
pub fn check_unlocked(d: &Drawing, h: Handle) -> R<()> {
    let o = d.get(h).ok_or_else(|| no_object(h))?;
    let layer = o.entity.as_ref().map(|e| e.layer).unwrap_or(Handle::NULL);
    if let Some(l) = d.layer(layer) {
        if l.locked {
            return Err(format!("layer {} is locked", l.name));
        }
    }
    Ok(())
}

fn kind_of(d: &Drawing, h: Handle) -> R<ObjectKind> {
    Ok(d.get(h).ok_or_else(|| no_object(h))?.kind.clone())
}

/// Add `kind` beside entity `src`: in its block, with its layer, colour,
/// linetype, lineweight and extended data.
pub fn add_like(d: &mut Drawing, src: Handle, kind: ObjectKind) -> R<Handle> {
    let o = d.get(src).ok_or_else(|| no_object(src))?;
    let common = o.entity.clone().ok_or("that is not a drawn object")?;
    let xdata = o.xdata.clone();
    let block = block_of(d, src)?;
    let nh = d.add_entity_in(block, common.layer, kind).map_err(err)?;
    if let Some(n) = d.objects.get_mut(&nh) {
        if let Some(e) = n.entity.as_mut() {
            let space = e.space;
            *e = EntityCommon { space, ..common };
        }
        n.xdata = xdata;
    }
    Ok(nh)
}

/// Give entity `h` the geometry `kind`. An entity that becomes another
/// type (an infinite line cut to a ray, a circle to an arc) is replaced by
/// a new one with the same properties. The handle it ends up with.
pub fn replace(d: &mut Drawing, h: Handle, kind: ObjectKind) -> R<Handle> {
    let o = d.get(h).ok_or_else(|| no_object(h))?;
    let same = entity_type(&kind).is_some_and(|(n, c)| n == o.type_name && c == o.type_code);
    if same {
        d.objects.get_mut(&h).expect("checked").kind = kind;
        return Ok(h);
    }
    let nh = add_like(d, h, kind)?;
    d.erase_entity(h).map_err(err)?;
    Ok(nh)
}

/// What an entity owns that moves with it: an insert's attributes, an old
/// polyline's vertices, and the SEQEND closing their list.
fn parts(d: &Drawing, h: Handle) -> Vec<Handle> {
    d.owned_by(h)
        .filter(|o| matches!(o.kind, ObjectKind::Attribute(_) | ObjectKind::Vertex(_) | ObjectKind::SeqEnd))
        .map(|o| o.handle)
        .collect()
}

/// Move, rotate, mirror or scale entities in place.
pub fn transform_entities(d: &mut Drawing, hs: &[Handle], xf: &Affine) -> R<()> {
    for &h in hs {
        check_unlocked(d, h)?;
    }
    for &h in hs {
        let ps = parts(d, h);
        let o = d.objects.get_mut(&h).ok_or_else(|| no_object(h))?;
        modify::transform(&mut o.kind, xf)?;
        for p in ps {
            if let Some(o) = d.objects.get_mut(&p) {
                modify::transform(&mut o.kind, xf)?;
            }
        }
    }
    Ok(())
}

/// Copies of entities with `xf` applied, beside the originals.
pub fn copy_entities(d: &mut Drawing, hs: &[Handle], xf: &Affine) -> R<Vec<Handle>> {
    hs.iter().map(|h| copy_entity(d, *h, None, xf)).collect()
}

/// A copy of an entity with what it owns, `xf` applied, in block `into`
/// (or beside the original).
pub fn copy_entity(d: &mut Drawing, h: Handle, into: Option<Handle>, xf: &Affine) -> R<Handle> {
    let o = d.get(h).ok_or_else(|| no_object(h))?.clone();
    let common = o.entity.clone().ok_or("that is not a drawn object")?;
    let mut kind = o.kind.clone();
    modify::transform(&mut kind, xf)?;
    let owned: Vec<Object> = parts(d, h).iter().filter_map(|p| d.get(*p).cloned()).collect();
    match &mut kind {
        ObjectKind::Insert(i) => {
            i.attribs.clear();
            i.first_attrib = Handle::NULL;
            i.last_attrib = Handle::NULL;
            i.seqend = Handle::NULL;
        }
        ObjectKind::Polyline(p) => {
            p.vertices.clear();
            p.first_vertex = Handle::NULL;
            p.last_vertex = Handle::NULL;
            p.seqend = Handle::NULL;
        }
        _ => {}
    }
    let block = match into {
        Some(b) => b,
        None => block_of(d, h)?,
    };
    let nh = d.add_entity_in(block, common.layer, kind).map_err(err)?;
    if let Some(n) = d.objects.get_mut(&nh) {
        if let Some(e) = n.entity.as_mut() {
            let space = e.space;
            *e = EntityCommon { space, ..common };
        }
        n.xdata = o.xdata.clone();
    }
    let mut kids = Vec::new();
    let mut seqend = Handle::NULL;
    for p in owned {
        let mut k = p.kind.clone();
        modify::transform(&mut k, xf)?;
        let ch = d.add_owned(nh, &p.type_name, p.type_code, k, p.entity.clone());
        if matches!(p.kind, ObjectKind::SeqEnd) {
            seqend = ch;
        } else {
            kids.push(ch);
        }
    }
    if !kids.is_empty() || !seqend.is_null() {
        match &mut d.objects.get_mut(&nh).expect("added").kind {
            ObjectKind::Insert(i) => {
                i.first_attrib = kids.first().copied().unwrap_or(Handle::NULL);
                i.last_attrib = kids.last().copied().unwrap_or(Handle::NULL);
                i.attribs = kids;
                i.seqend = seqend;
            }
            ObjectKind::Polyline(p) => {
                p.first_vertex = kids.first().copied().unwrap_or(Handle::NULL);
                p.last_vertex = kids.last().copied().unwrap_or(Handle::NULL);
                p.vertices = kids;
                p.seqend = seqend;
            }
            _ => {}
        }
    }
    Ok(nh)
}

/// The visible entities of a block that can be edges for trim and extend.
fn visible_entities(d: &Drawing, block: Handle) -> Vec<Handle> {
    d.block_entities(block)
        .iter()
        .filter(|o| {
            let Some(e) = &o.entity else { return false };
            !e.invisible && d.layer(e.layer).is_none_or(|l| !l.off && !l.frozen)
        })
        .map(|o| o.handle)
        .collect()
}

/// The edges trim and extend cut to: those of `edges`, or, when it is
/// empty, of every visible object in the space; `except` left out.
fn edge_curves(d: &Drawing, block: Handle, edges: &[Handle], except: Handle) -> Vec<Curve> {
    let list = if edges.is_empty() {
        visible_entities(d, block)
    } else {
        edges.to_vec()
    };
    list.iter()
        .filter(|h| **h != except)
        .filter_map(|h| d.get(*h))
        .flat_map(|o| modify::curves_of(&o.kind))
        .collect()
}

/// A parallel copy `distance` away on the side of `side` (OFFSET).
pub fn offset(d: &mut Drawing, h: Handle, distance: f64, side: Vec2) -> R<Handle> {
    let k = kind_of(d, h)?;
    let nk = modify::offset(&k, distance, side)?;
    add_like(d, h, nk)
}

/// A parallel copy through `through` (OFFSET's Through option).
pub fn offset_through(d: &mut Drawing, h: Handle, through: Vec2) -> R<Handle> {
    let k = kind_of(d, h)?;
    let dist = modify::distance_to(&k, through).ok_or("that object cannot be offset")?;
    if dist <= 1e-12 {
        return Err("the point is on the object: pick a point to one side".into());
    }
    offset(d, h, dist, through)
}

/// Cut entity `h` back to the edges either side of `pick` (TRIM): the
/// handles of what is left.
pub fn trim(d: &mut Drawing, h: Handle, pick: Vec2, edges: &[Handle]) -> R<Vec<Handle>> {
    check_unlocked(d, h)?;
    let block = block_of(d, h)?;
    let curves = edge_curves(d, block, edges, h);
    let pieces = modify::trim(&kind_of(d, h)?, pick, &curves)?;
    let mut out = Vec::new();
    let mut it = pieces.into_iter();
    match it.next() {
        None => d.erase_entity(h).map_err(err)?,
        Some(first) => {
            let kept = replace(d, h, first)?;
            out.push(kept);
            for k in it {
                out.push(add_like(d, kept, k)?);
            }
        }
    }
    Ok(out)
}

/// Lengthen entity `h` at its end nearest `pick` to the next edge
/// (EXTEND).
pub fn extend(d: &mut Drawing, h: Handle, pick: Vec2, edges: &[Handle]) -> R<Handle> {
    check_unlocked(d, h)?;
    let block = block_of(d, h)?;
    let curves = edge_curves(d, block, edges, h);
    let k = modify::extend(&kind_of(d, h)?, pick, &curves)?;
    replace(d, h, k)
}

/// The two objects of a corner (fillet or chamfer) put back, with the
/// piece joining them: on their layer when they share one, otherwise on
/// the current layer.
fn finish_corner(d: &mut Drawing, a: Handle, b: Handle, c: modify::Corner) -> R<Vec<Handle>> {
    let la = d.get(a).and_then(|o| o.entity.as_ref()).map(|e| e.layer);
    let lb = d.get(b).and_then(|o| o.entity.as_ref()).map(|e| e.layer);
    let block = block_of(d, a)?;
    let ha = replace(d, a, c.first)?;
    let hb = replace(d, b, c.second)?;
    let mut out = vec![ha, hb];
    if let Some(j) = c.join {
        let nh = if la == lb {
            add_like(d, ha, j)?
        } else {
            d.add_entity_in(block, Handle::NULL, j).map_err(err)?
        };
        out.push(nh);
    }
    Ok(out)
}

/// Round the corner between two objects (FILLET), or between two
/// neighbouring segments of one polyline.
pub fn fillet(d: &mut Drawing, a: Handle, pa: Vec2, b: Handle, pb: Vec2, radius: f64) -> R<Vec<Handle>> {
    check_unlocked(d, a)?;
    check_unlocked(d, b)?;
    if a == b {
        let ObjectKind::LwPolyline(p) = kind_of(d, a)? else {
            return Err("pick two different objects, or two segments of one polyline".into());
        };
        let v = modify::polyline_corner_between(&p, pa, pb).ok_or("the two segments do not meet")?;
        let (np, n) = modify::fillet_polyline_at(&p, Some(v), radius)?;
        if n == 0 {
            return Err("the radius is too large for these segments, or they are arcs".into());
        }
        return Ok(vec![replace(d, a, ObjectKind::LwPolyline(np))?]);
    }
    let c = modify::fillet(&kind_of(d, a)?, pa, &kind_of(d, b)?, pb, radius)?;
    finish_corner(d, a, b, c)
}

/// Bevel the corner between two lines (CHAMFER), or between two
/// neighbouring segments of one polyline.
pub fn chamfer(d: &mut Drawing, a: Handle, pa: Vec2, b: Handle, pb: Vec2, d1: f64, d2: f64) -> R<Vec<Handle>> {
    check_unlocked(d, a)?;
    check_unlocked(d, b)?;
    if a == b {
        let ObjectKind::LwPolyline(p) = kind_of(d, a)? else {
            return Err("pick two different lines, or two segments of one polyline".into());
        };
        let v = modify::polyline_corner_between(&p, pa, pb).ok_or("the two segments do not meet")?;
        let (np, n) = modify::chamfer_polyline_at(&p, Some(v), d1, d2)?;
        if n == 0 {
            return Err("the chamfer is longer than the segments, or they are arcs".into());
        }
        return Ok(vec![replace(d, a, ObjectKind::LwPolyline(np))?]);
    }
    let c = modify::chamfer(&kind_of(d, a)?, pa, &kind_of(d, b)?, pb, d1, d2)?;
    finish_corner(d, a, b, c)
}

/// Round (or bevel) every corner of a polyline: how many were done.
pub fn corners_of_polyline(d: &mut Drawing, h: Handle, fillet: Option<f64>, chamfer: Option<(f64, f64)>) -> R<usize> {
    check_unlocked(d, h)?;
    let ObjectKind::LwPolyline(p) = kind_of(d, h)? else {
        return Err("that is not a polyline".into());
    };
    let (np, n) = match (fillet, chamfer) {
        (Some(r), _) => modify::fillet_polyline(&p, r)?,
        (None, Some((d1, d2))) => modify::chamfer_polyline(&p, d1, d2)?,
        _ => return Ok(0),
    };
    if n > 0 {
        replace(d, h, ObjectKind::LwPolyline(np))?;
    }
    Ok(n)
}

/// Copies in rows and columns (ARRAYRECT): row `r`, column `c` moved by
/// `c·column_gap` along X and `r·row_gap` along Y.
pub fn array_rect(d: &mut Drawing, hs: &[Handle], rows: usize, cols: usize, row_gap: f64, column_gap: f64) -> R<Vec<Handle>> {
    if rows == 0 || cols == 0 {
        return Err("an array needs at least one row and one column".into());
    }
    if rows.saturating_mul(cols).saturating_mul(hs.len().max(1)) > MAX_ARRAY {
        return Err(format!("that array would make more than {MAX_ARRAY} copies"));
    }
    if !(row_gap.is_finite() && column_gap.is_finite()) {
        return Err("the spacing is not a number".into());
    }
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            if r == 0 && c == 0 {
                continue;
            }
            let xf = Affine::translate(Vec2::new(c as f64 * column_gap, r as f64 * row_gap));
            out.extend(copy_entities(d, hs, &xf)?);
        }
    }
    Ok(out)
}

/// Where a selection sits: the middle of what it draws.
pub fn selection_center(d: &Drawing, hs: &[Handle]) -> Option<Vec2> {
    let mut b = BBox::EMPTY;
    for h in hs {
        b.union(&crate::render::entity_bounds(d, *h));
    }
    (!b.is_empty()).then(|| b.center())
}

/// Copies around a centre (ARRAYPOLAR): `count` items in all over `fill`
/// radians (a whole turn spaces them evenly; negative goes clockwise),
/// each turned to face the centre as the original does when `rotate`.
pub fn array_polar(d: &mut Drawing, hs: &[Handle], center: Vec2, count: usize, fill: f64, rotate: bool) -> R<Vec<Handle>> {
    if count < 2 {
        return Err("a polar array needs at least two items".into());
    }
    if count.saturating_mul(hs.len().max(1)) > MAX_ARRAY {
        return Err(format!("that array would make more than {MAX_ARRAY} copies"));
    }
    if !fill.is_finite() || fill == 0.0 || fill.abs() > TAU + 1e-9 {
        return Err("the angle to fill must be more than 0° and at most 360°".into());
    }
    let whole = (fill.abs() - TAU).abs() < 1e-9;
    let step = if whole { fill / count as f64 } else { fill / (count - 1) as f64 };
    let reference = selection_center(d, hs).unwrap_or(center);
    let mut out = Vec::new();
    for i in 1..count {
        let a = step * i as f64;
        let xf = if rotate {
            Affine::rotate(center, a)
        } else {
            let moved = Affine::rotate(center, a).p(reference);
            Affine::translate(moved - reference)
        };
        out.extend(copy_entities(d, hs, &xf)?);
    }
    Ok(out)
}

/// Move the points of entities that lie inside `window` by `delta`
/// (STRETCH): what lies wholly inside moves, what crosses it stretches.
/// The handles of what changed.
pub fn stretch(d: &mut Drawing, hs: &[Handle], window: &BBox, delta: Vec2) -> R<Vec<Handle>> {
    for &h in hs {
        check_unlocked(d, h)?;
    }
    let mut out = Vec::new();
    for &h in hs {
        let ps = parts(d, h);
        let o = d.objects.get_mut(&h).ok_or_else(|| no_object(h))?;
        let moved = modify::stretch(&mut o.kind, window, delta)?;
        if !moved {
            continue;
        }
        if matches!(o.kind, ObjectKind::Insert(_)) {
            let xf = Affine::translate(delta);
            for p in ps {
                if let Some(o) = d.objects.get_mut(&p) {
                    modify::transform(&mut o.kind, &xf)?;
                }
            }
        }
        out.push(h);
    }
    Ok(out)
}

/// The dimension style a dimension uses, or the drawing's standard one.
fn style_of(d: &Drawing, dm: &Dimension) -> DimStyle {
    d.dim_style(dm.dimstyle)
        .or_else(|| d.dim_style(d.header.dimstyle))
        .cloned()
        .unwrap_or_else(|| standards::dim_style(Unit::of(d)))
}

/// The next dimension of a row: from the first extension line of `base`
/// to `p`, a spacing (DIMDLI) further out (DIMBASELINE), or from its
/// second extension line to `p` in line with it (DIMCONTINUE). Angular
/// dimensions go on round the same vertex. The new dimension's handle;
/// it can be the base of the next.
pub fn dim_next(d: &mut Drawing, base: Handle, p: Vec2, baseline: bool) -> R<Handle> {
    let ObjectKind::Dimension(bd) = kind_of(d, base)? else {
        return Err("that is not a dimension".into());
    };
    let st = style_of(d, &bd);
    let std = standards::dim_style(Unit::of(d));
    let spacing = if st.baseline_spacing > 0.0 { st.baseline_spacing } else { std.baseline_spacing }
        * if st.scale > 0.0 { st.scale } else { 1.0 };
    let mut new = match bd.kind {
        DimensionKind::Linear | DimensionKind::Aligned => {
            let (p13, p14, on) = (bd.pt13.xy(), bd.pt14.xy(), bd.pt10.xy());
            let rot = if bd.kind == DimensionKind::Linear {
                bd.dim_rotation
            } else {
                (p14 - p13).angle()
            };
            let n = Vec2::from_angle(rot).perp();
            if baseline {
                let side = if (on - p13).dot(n) >= 0.0 { 1.0 } else { -1.0 };
                Dimension::linear(p13, p, on + n * (side * spacing), rot)
            } else {
                Dimension::linear(p14, p, on, rot)
            }
        }
        DimensionKind::Angular3Pt | DimensionKind::Angular2Line => {
            let arc = bd.angular_arc().ok_or("the dimension's angle cannot be worked out")?;
            let c = arc.center;
            let ray = |a: f64, reach: f64| c + Vec2::from_angle(a) * reach.max(1e-6);
            let (from, r) = if baseline {
                (ray(arc.start, arc.legs[0].1), arc.radius + spacing)
            } else {
                (ray(arc.start + arc.sweep, arc.legs[1].1), arc.radius)
            };
            let a0 = (from - c).angle();
            let sweep = crate::geom::curve::normalize_sweep((p - c).angle() - a0);
            let at = c + Vec2::from_angle(a0 + sweep / 2.0) * r;
            Dimension::angular(c, from, p, at)
        }
        _ => return Err("baseline and continued dimensions follow linear, aligned and angular ones".into()),
    };
    if new.measurement <= 1e-12 {
        return Err("nothing to measure: the point is on the last extension line".into());
    }
    new.dimstyle = bd.dimstyle;
    add_like(d, base, ObjectKind::Dimension(new))
}

/// A leader from `points[0]` (the arrow) through the rest, with a short
/// horizontal landing, and a multiline note at its end (LEADER, QLEADER
/// and MLEADER as classic leaders). The leader's and the note's handles.
pub fn leader(d: &mut Drawing, block: Handle, points: &[Vec2], text: &str, height: f64, dimstyle: Handle) -> R<(Handle, Handle)> {
    if points.len() < 2 {
        return Err("a leader needs at least two points".into());
    }
    if points.windows(2).any(|w| w[0].distance(w[1]) < 1e-12) {
        return Err("two points of the leader are the same".into());
    }
    if !(height.is_finite() && height > 0.0) {
        return Err("the text height must be more than zero".into());
    }
    let st = d
        .dim_style(dimstyle)
        .cloned()
        .unwrap_or_else(|| standards::dim_style(Unit::of(d)));
    let sz = crate::render::dim::sizes(&st, Unit::of(d));
    let n = points.len();
    let last = points[n - 1];
    let side = if last.x >= points[n - 2].x { 1.0 } else { -1.0 };
    let mut verts: Vec<Vec3> = points.iter().map(|p| p.extend(0.0)).collect();
    let landing_len = if sz.arrow > 0.0 { sz.arrow * 2.0 } else { height };
    let landing = last + Vec2::new(side * landing_len, 0.0);
    if (last - points[n - 2]).normalized().y.abs() > 1e-9 {
        verts.push(landing.extend(0.0));
    }
    let end = verts[verts.len() - 1].xy();
    let lines: Vec<&str> = text.split("\\P").collect();
    let widest = lines.iter().map(|l| crate::font::text_width(l)).fold(0.0, f64::max) * height;
    let leader = Leader {
        vertices: verts,
        dimstyle,
        text_height: height,
        text_width: widest,
        ..Default::default()
    };
    let gap = if sz.gap > 0.0 { sz.gap } else { height / 4.0 };
    let at = end + Vec2::new(side * gap, 0.0);
    let style = d.header.textstyle;
    let mut note = MText::new(at, height, 0.0, text, if side > 0.0 { 4 } else { 6 });
    note.style = style;
    let lh = d.add_entity_in(block, Handle::NULL, ObjectKind::Leader(leader)).map_err(err)?;
    let th = d.add_entity_in(block, Handle::NULL, ObjectKind::MText(note)).map_err(err)?;
    Ok((lh, th))
}

/// A hatch in `block` of the region around `p`, bounded by the visible
/// outlines there, with its islands (HATCH, pick points).
pub fn hatch_at(d: &mut Drawing, block: Handle, p: Vec2, pattern: &str, scale: f64, angle: f64, double: bool) -> R<Handle> {
    let region = boundary::region_at(d, block, p)?;
    let mut loops = vec![region.outer];
    loops.extend(region.islands);
    let h = boundary::hatch(&loops, true, pattern, scale, angle, double, Unit::of(d), Some(p))?;
    d.add_entity_in(block, Handle::NULL, ObjectKind::Hatch(h)).map_err(err)
}

/// A hatch of closed objects: circles, closed polylines, ellipses,
/// splines (HATCH, select objects). Objects inside others become islands.
pub fn hatch_objects(d: &mut Drawing, hs: &[Handle], pattern: &str, scale: f64, angle: f64, double: bool) -> R<Handle> {
    let first = *hs.first().ok_or("select the closed objects to hatch")?;
    let block = block_of(d, first)?;
    let mut loops = Vec::new();
    for h in hs {
        loops.push(boundary::loop_of(&kind_of(d, *h)?)?);
    }
    // The largest first: the outline.
    let area = |l: &boundary::Loop| -> f64 {
        let pts = crate::geom::curve::tessellate_bulge_polyline(l, true, std::f64::consts::PI / 90.0);
        let n = pts.len();
        (0..n).map(|i| pts[i].cross(pts[(i + 1) % n])).sum::<f64>().abs() / 2.0
    };
    loops.sort_by(|a, b| area(b).total_cmp(&area(a)));
    let h = boundary::hatch(&loops, false, pattern, scale, angle, double, Unit::of(d), None)?;
    d.add_entity_in(block, Handle::NULL, ObjectKind::Hatch(h)).map_err(err)
}

/// Change the drawing's unit (UNITS): `$INSUNITS` and `$MEASUREMENT`.
/// With `scale`, model space is scaled so every object keeps its real
/// size (a 100 mm line becomes 3.937 in long), with the sizes that are in
/// drawing units: text and dimension style sizes, linetype patterns, the
/// limits. Without it only the unit's name changes, as in other CAD
/// programs, and the numbers stay as they are.
pub fn convert_units(d: &mut Drawing, to: Unit, scale: bool) -> R<()> {
    let from = Unit::of(d);
    d.header.insunits = Some(to.insunits());
    if to.is_imperial() || to.is_metric() {
        d.header.measurement = Some(if to.is_imperial() { 0 } else { 1 });
    }
    if !scale || from == to {
        return Ok(());
    }
    let k = crate::units::convert(1.0, from, to).ok_or("the drawing's unit has no size to convert from: choose the unit without scaling")?;
    let xf = Affine::scale(Vec2::ZERO, k);
    let ms = d.model_space();
    let hs: Vec<Handle> = d.block_entities(ms).iter().map(|o| o.handle).collect();
    for h in hs {
        let ps = parts(d, h);
        if let Some(o) = d.objects.get_mut(&h) {
            // Objects that cannot be scaled (3D ones) stay as they are.
            let _ = modify::transform(&mut o.kind, &xf);
        }
        for p in ps {
            if let Some(o) = d.objects.get_mut(&p) {
                let _ = modify::transform(&mut o.kind, &xf);
            }
        }
    }
    for o in d.objects.values_mut() {
        match &mut o.kind {
            ObjectKind::DimStyle(st) => {
                for v in [
                    &mut st.text_height,
                    &mut st.arrow_size,
                    &mut st.ext_offset,
                    &mut st.baseline_spacing,
                    &mut st.ext_extension,
                    &mut st.text_gap,
                    &mut st.center_mark,
                    &mut st.tick_size,
                    &mut st.dim_line_extension,
                    &mut st.rounding,
                ] {
                    *v *= k;
                }
            }
            ObjectKind::TextStyle(t) => {
                t.height *= k;
                t.last_height *= k;
            }
            ObjectKind::Linetype(l) => {
                for e in &mut l.elements {
                    e.length *= k;
                }
                l.pattern_length *= k;
            }
            _ => {}
        }
    }
    let h = &mut d.header;
    for v in [&mut h.textsize].into_iter().flatten() {
        *v *= k;
    }
    for v in [&mut h.limmin, &mut h.limmax].into_iter().flatten() {
        *v = *v * k;
    }
    for v in [&mut h.extmin, &mut h.extmax, &mut h.insbase].into_iter().flatten() {
        *v = *v * k;
    }
    Ok(())
}

/// The dimension style new dimensions use: the current one (`$DIMSTYLE`),
/// or the drawing's first.
pub fn current_dimstyle(d: &Drawing) -> Handle {
    if d.dim_style(d.header.dimstyle).is_some() {
        return d.header.dimstyle;
    }
    d.objects
        .values()
        .find(|o| matches!(o.kind, ObjectKind::DimStyle(_)))
        .map(|o| o.handle)
        .unwrap_or(Handle::NULL)
}

/// The layer dimensions go on: DIMENSION when the drawing has it
/// (DESIGN.md §3), otherwise the current layer (null).
pub fn dimension_layer(d: &Drawing) -> Handle {
    d.layer_by_name("DIMENSION").map(|(h, _)| h).unwrap_or(Handle::NULL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standards::{new_drawing, Template};

    fn add(d: &mut Drawing, k: ObjectKind) -> Handle {
        d.add_entity(Handle::NULL, k).unwrap()
    }

    fn line(ax: f64, ay: f64, bx: f64, by: f64) -> ObjectKind {
        ObjectKind::Line(Line::new(Vec2::new(ax, ay), Vec2::new(bx, by)))
    }

    #[test]
    fn copies_keep_properties_and_owned_parts() {
        let mut d = new_drawing(Template::Metric);
        let l = add(&mut d, line(0.0, 0.0, 10.0, 0.0));
        d.objects.get_mut(&l).unwrap().entity.as_mut().unwrap().color = Color::Index(3);
        let c = copy_entities(&mut d, &[l], &Affine::translate(Vec2::new(0.0, 5.0))).unwrap();
        let o = d.get(c[0]).unwrap();
        assert_eq!(o.entity.as_ref().unwrap().color, Color::Index(3));
        assert!(matches!(&o.kind, ObjectKind::Line(x) if x.start.y == 5.0));
        // Rotating in place.
        transform_entities(&mut d, &[l], &Affine::rotate(Vec2::ZERO, std::f64::consts::FRAC_PI_2)).unwrap();
        assert!(matches!(&d.get(l).unwrap().kind, ObjectKind::Line(x) if (x.end.y - 10.0).abs() < 1e-9));
    }

    #[test]
    fn trim_extend_offset_fillet_on_a_drawing() {
        let mut d = new_drawing(Template::Metric);
        let a = add(&mut d, line(0.0, 0.0, 100.0, 0.0));
        let b = add(&mut d, line(50.0, -10.0, 50.0, 10.0));
        // Trim the right half of a away at b.
        let left = trim(&mut d, a, Vec2::new(80.0, 0.0), &[]).unwrap();
        assert_eq!(left, vec![a]);
        assert!(matches!(&d.get(a).unwrap().kind, ObjectKind::Line(x) if (x.end.x - 50.0).abs() < 1e-9));
        // Trim the middle of a crossing line: two pieces.
        let c = add(&mut d, line(40.0, 5.0, 60.0, 5.0));
        let e = add(&mut d, line(45.0, 0.0, 45.0, 10.0));
        let pieces = trim(&mut d, c, Vec2::new(47.0, 5.0), &[b, e]).unwrap();
        assert_eq!(pieces.len(), 2);
        // Extend a short line to b.
        let s = add(&mut d, line(0.0, 20.0, 10.0, 20.0));
        let wall = add(&mut d, line(30.0, 0.0, 30.0, 40.0));
        extend(&mut d, s, Vec2::new(9.0, 20.0), &[wall]).unwrap();
        assert!(matches!(&d.get(s).unwrap().kind, ObjectKind::Line(x) if (x.end.x - 30.0).abs() < 1e-9));
        // Offset through a point.
        let o = offset_through(&mut d, s, Vec2::new(5.0, 25.0)).unwrap();
        assert!(matches!(&d.get(o).unwrap().kind, ObjectKind::Line(x) if (x.start.y - 25.0).abs() < 1e-9));
        // Fillet two lines: an arc joins them.
        let p = add(&mut d, line(200.0, 0.0, 300.0, 0.0));
        let q = add(&mut d, line(200.0, 0.0, 200.0, 100.0));
        let made = fillet(&mut d, p, Vec2::new(250.0, 0.0), q, Vec2::new(200.0, 50.0), 10.0).unwrap();
        assert_eq!(made.len(), 3);
        assert!(matches!(&d.get(made[2]).unwrap().kind, ObjectKind::Arc(x) if (x.radius - 10.0).abs() < 1e-9));
        // A circle trimmed becomes an arc, a new object.
        let ci = add(&mut d, ObjectKind::Circle(Circle::new(Vec2::new(500.0, 0.0), 10.0)));
        let cut = add(&mut d, line(480.0, 0.0, 520.0, 0.0));
        let arcs = trim(&mut d, ci, Vec2::new(500.0, 10.0), &[cut]).unwrap();
        assert!(d.get(ci).is_none());
        assert!(matches!(&d.get(arcs[0]).unwrap().kind, ObjectKind::Arc(_)));
    }

    #[test]
    fn arrays_and_baselines() {
        let mut d = new_drawing(Template::Metric);
        let c = add(&mut d, ObjectKind::Circle(Circle::new(Vec2::new(10.0, 10.0), 2.0)));
        let made = array_rect(&mut d, &[c], 2, 3, 20.0, 25.0).unwrap();
        assert_eq!(made.len(), 5);
        let made = array_polar(&mut d, &[c], Vec2::new(0.0, 10.0), 6, TAU, true).unwrap();
        assert_eq!(made.len(), 5);
        // Item 3 of 6 is opposite.
        assert!(matches!(&d.get(made[2]).unwrap().kind, ObjectKind::Circle(x) if (x.center.x + 10.0).abs() < 1e-9));
        let dim = add(&mut d, ObjectKind::Dimension(Dimension::linear(Vec2::ZERO, Vec2::new(20.0, 0.0), Vec2::new(0.0, -10.0), 0.0)));
        let b = dim_next(&mut d, dim, Vec2::new(50.0, 0.0), true).unwrap();
        let ObjectKind::Dimension(bd) = &d.get(b).unwrap().kind else { panic!() };
        assert!((bd.measurement - 50.0).abs() < 1e-9);
        assert!(bd.pt10.y < -10.0);
        let c2 = dim_next(&mut d, dim, Vec2::new(45.0, 0.0), false).unwrap();
        let ObjectKind::Dimension(cd) = &d.get(c2).unwrap().kind else { panic!() };
        assert!((cd.measurement - 25.0).abs() < 1e-9 && (cd.pt10.y + 10.0).abs() < 1e-9);
    }

    #[test]
    fn hatches_and_leaders() {
        let mut d = new_drawing(Template::Metric);
        add(&mut d, ObjectKind::LwPolyline(LwPolyline::new(vec![Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(100.0, 50.0), Vec2::new(0.0, 50.0)], true)));
        let hole = add(&mut d, ObjectKind::Circle(Circle::new(Vec2::new(30.0, 25.0), 10.0)));
        let ms = d.model_space();
        let h = hatch_at(&mut d, ms, Vec2::new(70.0, 25.0), "ANSI31", 1.0, 0.0, false).unwrap();
        let ObjectKind::Hatch(hh) = &d.get(h).unwrap().kind else { panic!() };
        assert_eq!(hh.paths.len(), 2);
        let h2 = hatch_objects(&mut d, &[hole], "SOLID", 1.0, 0.0, false).unwrap();
        assert!(matches!(&d.get(h2).unwrap().kind, ObjectKind::Hatch(x) if x.solid_fill));
        let (l, t) = leader(&mut d, ms, &[Vec2::new(30.0, 35.0), Vec2::new(50.0, 60.0)], "SEE NOTE 1", 3.5, Handle::NULL).unwrap();
        let ObjectKind::Leader(ld) = &d.get(l).unwrap().kind else { panic!() };
        assert_eq!(ld.vertices.len(), 3);
        assert!(matches!(&d.get(t).unwrap().kind, ObjectKind::MText(m) if m.attachment == 4));
    }
}

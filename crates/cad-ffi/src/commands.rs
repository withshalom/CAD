//! The drafting commands past the first ones in [`crate::session`]: modify
//! (move, copy, rotate, mirror and scale any object; offset, trim, extend,
//! fillet, chamfer, arrays, stretch, explode), construction lines, hatches,
//! multiline text, leaders, angular and chained dimensions, blocks with
//! attributes, units, linetypes and lineweights, layouts and viewports.
//! Each parses its words and calls the core, which does the work
//! (`cad_core::ops`, `blocks`, `layouts`, `boundary`).
//!
//! | Command | Does |
//! |---|---|
//! | `move dx dy h …` · `copy dx dy h …` | moves or copies objects |
//! | `rotate cx cy degrees h …` · `scale cx cy factor h …` | turns or scales objects about a point |
//! | `mirror x1 y1 x2 y2 keep h …` | mirrors objects (keeping the originals when keep is 1) |
//! | `offset distance sx sy h` · `offset-through x y h` | a parallel copy on the side of (sx, sy), or through (x, y) |
//! | `trim px py h [edge …]` · `extend px py h [edge …]` | cuts back at, or lengthens to, the edges (every visible object when none are given); (px, py) picks the part or end |
//! | `fillet r x1 y1 h1 x2 y2 h2` · `chamfer d1 d2 x1 y1 h1 x2 y2 h2` | rounds or bevels the corner between two objects (two segments of one polyline too), keeping the picked sides |
//! | `fillet-pline r h` · `chamfer-pline d1 d2 h` | every corner of a polyline |
//! | `array-rect rows columns row-gap column-gap h …` · `array-polar cx cy count fill-degrees rotate h …` | arrays of copies |
//! | `stretch x1 y1 x2 y2 dx dy h …` | moves the points inside the window |
//! | `explode h …` | inserts into their objects, polylines into lines and arcs |
//! | `scale-ref cx cy reference new h …` | scales objects so the reference length becomes the new one |
//! | `arc-3p x1 y1 x2 y2 x3 y3` | the arc from the first point through the second to the third |
//! | `grip h kind field fx fy tx ty` | drags a grip of h (as `grips` lists it) from (fx, fy) to (tx, ty) |
//! | `xline x1 y1 x2 y2` · `ray x1 y1 x2 y2` | a construction line through, or from, the first point towards the second |
//! | `xline-angle x y degrees` | a construction line through (x, y) at the angle |
//! | `mtext x y width height degrees text…` | multiline text (`\P` breaks a line; width 0 for no wrapping) |
//! | `mtext-box x1 y1 x2 y2 height degrees text…` | multiline text filling the box between two corners, from its top left |
//! | `leader height x1 y1 x2 y2 … \| text` | a leader with its note |
//! | `hatch pattern scale degrees double x y` · `hatch-objects pattern scale degrees double h …` | hatches the region around (x, y), or closed objects |
//! | `dim-angular cx cy x1 y1 x2 y2 lx ly` · `dim-angular-lines h1 h2 lx ly` · `dim-angular-arc h lx ly` | angular dimensions: vertex and two points, two lines, an arc |
//! | `dim-baseline h x y` · `dim-continue h x y` | the next dimension of a row from dimension h |
//! | `attdef x y height degrees \| TAG \| prompt \| default` | an attribute definition |
//! | `block bx by retain\|convert\|delete h … \| name` | a block from objects, base point (bx, by) |
//! | `insert x y scale degrees \| name \| TAG=value …` | an insert, with attribute values |
//! | `attedit h \| TAG \| value` | changes an insert's attribute |
//! | `units insunits lunits luprec aunits auprec scale` | the drawing's unit and formats; scale 1 converts the drawing |
//! | `linetype-load name …` · `linetype-current name` · `ltscale k` | linetypes |
//! | `color-current c` · `lineweight-current w` | properties of new objects (ByLayer, ByBlock, an index, millimetres) |
//! | `layer-linetype h name` · `layer-lineweight h mm` | layer properties |
//! | `layout-new paper name…` · `layout-rename old \| new` · `layout-delete name…` | layouts |
//! | `sheet paper \| name \| LABEL=value \| …` | a sheet ready to plot: border, title block and a viewport at the scale that fits ([`cad_core::sheet`]); `-` for the default paper or the next "Sheet N" |
//! | `page-setup paper landscape\|portrait monochrome` | the paper and colours the current sheet plots with |
//! | `mview x1 y1 x2 y2 [scale [cx cy]]` · `mview fit [scale [cx cy]]` | a viewport on the current layout (scale `fit`, `1:2`, `0.5` …); `fit` fills the paper inside a margin |
//! | `vp-scale h scale` · `vp-center h x y` | what a viewport shows |
//!
//! Queries: `layouts`, `blocks`, `linetypes`, `patterns`, `attribs h`;
//! `bounds h …` (how far objects reach, and the spacing ARRAY offers);
//! `measure x1 y1 x2 y2` (DIST, in the drawing's formats); `grips h …`
//! (the grips of selected objects, [`cad_core::grips`]).

use crate::session::{flag, nums, parse_handle, parse_num};
use cad_core::export::display::{json_escape, json_num};
use cad_core::geom::{BBox, Vec2};
use cad_core::model::*;
use cad_core::modify::Affine;
use cad_core::units::Unit;
use cad_core::{blocks, construct, grips, layouts, ops, standards};
use std::fmt::Write as _;

type R<T> = Result<T, String>;

fn err(e: EditError) -> String {
    e.to_string()
}

fn handle_list(args: &[&str]) -> R<Vec<Handle>> {
    if args.is_empty() {
        return Err("select objects first".into());
    }
    args.iter().map(|a| parse_handle(a)).collect()
}

/// The words of a command split at `|` into trimmed fields.
fn fields(args: &[&str]) -> Vec<String> {
    args.join(" ").split('|').map(|f| f.trim().to_string()).collect()
}

fn pt(v: &[f64], i: usize) -> Vec2 {
    Vec2::new(v[i], v[i + 1])
}

/// A colour as typed: ByLayer, ByBlock, an index 1–255, or #rrggbb.
fn parse_color(s: &str) -> R<Color> {
    let t = s.trim();
    match t.to_ascii_lowercase().as_str() {
        "bylayer" => return Ok(Color::ByLayer),
        "byblock" => return Ok(Color::ByBlock),
        _ => {}
    }
    if let Some(hex) = t.strip_prefix('#') {
        if hex.len() == 6 {
            if let Ok(v) = u32::from_str_radix(hex, 16) {
                return Ok(Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
            }
        }
    }
    match t.parse::<u16>() {
        Ok(i) if (1..=255).contains(&i) => Ok(Color::Index(i as u8)),
        _ => Err(format!("{t:?} is not a colour: ByLayer, ByBlock, 1 to 255, or #rrggbb")),
    }
}

/// A lineweight as typed: ByLayer, ByBlock, Default, or millimetres.
fn parse_weight(s: &str) -> R<Lineweight> {
    let t = s.trim();
    match t.to_ascii_lowercase().as_str() {
        "bylayer" => return Ok(Lineweight::ByLayer),
        "byblock" => return Ok(Lineweight::ByBlock),
        "default" => return Ok(Lineweight::Default),
        _ => {}
    }
    let mm = parse_num(t.trim_end_matches("mm"))?;
    if !(0.0..=2.11).contains(&mm) {
        return Err("a lineweight is 0 to 2.11 mm".into());
    }
    Ok(Lineweight::from_mm(mm))
}

/// A linetype record by name, loaded from the library when the drawing
/// does not have it yet.
fn linetype(d: &mut Drawing, name: &str) -> R<Handle> {
    let name = name.trim();
    if let Some(h) = d.linetype_by_name(name) {
        return Ok(h);
    }
    let (n, desc, pattern) = standards::library_linetype(name, Unit::of(d))
        .ok_or_else(|| format!("there is no linetype {name:?}; one of Continuous, {}", standards::library_names().join(", ")))?;
    d.add_linetype(&n, &desc, &pattern).map_err(err)
}

fn layer(d: &Drawing, s: &str) -> R<Handle> {
    let h = parse_handle(s)?;
    d.layer(h).map(|_| h).ok_or_else(|| format!("{s} is not a layer"))
}

/// Run a command this module knows; `None` for one it does not.
pub(crate) fn edit(d: &mut Drawing, space: Handle, cmd: &str, args: &[&str]) -> Option<R<Vec<Handle>>> {
    Some(run(d, space, cmd, args).transpose()?)
}

fn run(d: &mut Drawing, space: Handle, cmd: &str, args: &[&str]) -> R<Option<Vec<Handle>>> {
    let take = |n: usize| -> R<(Vec<f64>, Vec<Handle>)> {
        if args.len() <= n {
            return Err(format!("{cmd} takes {n} numbers and then objects"));
        }
        Ok((nums(&args[..n], n)?, handle_list(&args[n..])?))
    };
    let out = match cmd {
        "move" | "copy" => {
            let (v, hs) = take(2)?;
            let xf = Affine::translate(pt(&v, 0));
            if cmd == "move" {
                ops::transform_entities(d, &hs, &xf)?;
                hs
            } else {
                ops::copy_entities(d, &hs, &xf)?
            }
        }
        "rotate" => {
            let (v, hs) = take(3)?;
            ops::transform_entities(d, &hs, &Affine::rotate(pt(&v, 0), v[2].to_radians()))?;
            hs
        }
        "scale" => {
            let (v, hs) = take(3)?;
            if !(v[2] > 0.0) {
                return Err("the scale factor must be more than zero".into());
            }
            ops::transform_entities(d, &hs, &Affine::scale(pt(&v, 0), v[2]))?;
            hs
        }
        "scale-ref" => {
            // SCALE Reference: a length that is to become another.
            let (v, hs) = take(4)?;
            if !(v[2] > 0.0 && v[3] > 0.0) {
                return Err("the lengths must be more than zero".into());
            }
            ops::transform_entities(d, &hs, &Affine::scale(pt(&v, 0), v[3] / v[2]))?;
            hs
        }
        "arc-3p" => {
            let v = nums(args, 6)?;
            let a = construct::arc_through(pt(&v, 0), pt(&v, 2), pt(&v, 4)).ok_or("the three points are in a line: no arc")?;
            vec![d.add_entity_in(space, Handle::NULL, ObjectKind::Arc(a)).map_err(err)?]
        }
        "grip" => {
            if args.len() != 7 {
                return Err("grip takes an object, the grip's kind and field (- for none), where it was and where it goes".into());
            }
            let h = parse_handle(args[0])?;
            let kind = grips::GripKind::parse(args[1], args[2]).ok_or_else(|| format!("{} {} is not a grip", args[1], args[2]))?;
            let v = nums(&args[3..], 4)?;
            grips::drag(d, h, kind, pt(&v, 0), pt(&v, 2))?;
            vec![h]
        }
        "mirror" => {
            let (v, hs) = take(5)?;
            let (a, b) = (pt(&v, 0), pt(&v, 2));
            if a.distance(b) == 0.0 {
                return Err("the mirror line has no length".into());
            }
            let xf = Affine::mirror(a, b);
            if v[4] != 0.0 {
                ops::copy_entities(d, &hs, &xf)?
            } else {
                ops::transform_entities(d, &hs, &xf)?;
                hs
            }
        }
        "offset" => {
            let (v, hs) = take(3)?;
            let mut out = Vec::new();
            for h in hs {
                out.push(ops::offset(d, h, v[0], pt(&v, 1))?);
            }
            out
        }
        "offset-through" => {
            let (v, hs) = take(2)?;
            let mut out = Vec::new();
            for h in hs {
                out.push(ops::offset_through(d, h, pt(&v, 0))?);
            }
            out
        }
        "trim" | "extend" => {
            let (v, hs) = take(2)?;
            let (target, edges) = hs.split_first().expect("one at least");
            if cmd == "trim" {
                ops::trim(d, *target, pt(&v, 0), edges)?
            } else {
                vec![ops::extend(d, *target, pt(&v, 0), edges)?]
            }
        }
        "fillet" | "chamfer" => {
            let n = if cmd == "fillet" { 1 } else { 2 };
            if args.len() != n + 6 {
                return Err(format!("{cmd} takes {} then x y and an object, twice", if n == 1 { "the radius" } else { "two distances" }));
            }
            let lead = nums(&args[..n], n)?;
            let p1 = nums(&args[n..n + 2], 2)?;
            let h1 = parse_handle(args[n + 2])?;
            let p2 = nums(&args[n + 3..n + 5], 2)?;
            let h2 = parse_handle(args[n + 5])?;
            if cmd == "fillet" {
                ops::fillet(d, h1, pt(&p1, 0), h2, pt(&p2, 0), lead[0])?
            } else {
                ops::chamfer(d, h1, pt(&p1, 0), h2, pt(&p2, 0), lead[0], lead[1])?
            }
        }
        "fillet-pline" | "chamfer-pline" => {
            let n = if cmd == "fillet-pline" { 1 } else { 2 };
            let (v, hs) = take(n)?;
            let mut out = Vec::new();
            for h in hs {
                let done = if n == 1 {
                    ops::corners_of_polyline(d, h, Some(v[0]), None)?
                } else {
                    ops::corners_of_polyline(d, h, None, Some((v[0], v[1])))?
                };
                if done == 0 {
                    return Err("no corner of the polyline could be done at that size".into());
                }
                out.push(h);
            }
            out
        }
        "array-rect" => {
            let (v, hs) = take(4)?;
            if v[0] < 1.0 || v[1] < 1.0 || v[0].fract() != 0.0 || v[1].fract() != 0.0 {
                return Err("rows and columns are whole numbers, 1 or more".into());
            }
            ops::array_rect(d, &hs, v[0] as usize, v[1] as usize, v[2], v[3])?
        }
        "array-polar" => {
            let (v, hs) = take(5)?;
            if v[2] < 2.0 || v[2].fract() != 0.0 {
                return Err("a polar array has 2 or more items".into());
            }
            ops::array_polar(d, &hs, pt(&v, 0), v[2] as usize, v[3].to_radians(), v[4] != 0.0)?
        }
        "stretch" => {
            let (v, hs) = take(6)?;
            let mut w = BBox::EMPTY;
            w.add(pt(&v, 0));
            w.add(pt(&v, 2));
            ops::stretch(d, &hs, &w, pt(&v, 4))?
        }
        "explode" => {
            let hs = handle_list(args)?;
            let mut out = Vec::new();
            for h in hs {
                out.extend(blocks::explode(d, h)?);
            }
            out
        }
        "xline" | "ray" => {
            let v = nums(args, 4)?;
            let (a, b) = (pt(&v, 0), pt(&v, 2));
            if a.distance(b) == 0.0 {
                return Err("the two points are the same: no direction".into());
            }
            let c = ConstructionLine::new(a, b - a);
            let kind = if cmd == "xline" { ObjectKind::Xline(c) } else { ObjectKind::Ray(c) };
            vec![d.add_entity_in(space, Handle::NULL, kind).map_err(err)?]
        }
        "xline-angle" => {
            let v = nums(args, 3)?;
            let c = construct::xline_at_angle(pt(&v, 0), v[2].to_radians());
            vec![d.add_entity_in(space, Handle::NULL, ObjectKind::Xline(c)).map_err(err)?]
        }
        "mtext-box" => {
            if args.len() < 7 {
                return Err("mtext-box takes two corners, the height, the rotation and the text".into());
            }
            let v = nums(&args[..6], 6)?;
            let mut m = construct::mtext_in_box(pt(&v, 0), pt(&v, 2), v[4], v[5].to_radians(), &args[6..].join(" "))?;
            m.style = d.header.textstyle;
            vec![d.add_entity_in(space, Handle::NULL, ObjectKind::MText(m)).map_err(err)?]
        }
        "mtext" => {
            if args.len() < 6 {
                return Err("mtext takes x y width height rotation and the text".into());
            }
            let v = nums(&args[..5], 5)?;
            if v[3] <= 0.0 {
                return Err("the text height must be more than zero".into());
            }
            if v[2] < 0.0 {
                return Err("the width cannot be negative".into());
            }
            let text = args[5..].join(" ");
            let mut m = MText::new(pt(&v, 0), v[3], v[2], &text, 1);
            m.x_axis = Vec2::from_angle(v[4].to_radians()).extend(0.0);
            m.style = d.header.textstyle;
            vec![d.add_entity_in(space, Handle::NULL, ObjectKind::MText(m)).map_err(err)?]
        }
        "leader" => {
            let f = fields(args);
            if f.len() != 2 || f[1].is_empty() {
                return Err("leader takes the text height and points, then | and the text".into());
            }
            let words: Vec<&str> = f[0].split_whitespace().collect();
            let v = nums(&words, words.len())?;
            if v.len() < 5 || v.len() % 2 == 0 {
                return Err("leader takes the text height and two or more points".into());
            }
            let pts: Vec<Vec2> = v[1..].chunks(2).map(|c| Vec2::new(c[0], c[1])).collect();
            let style = ops::current_dimstyle(d);
            let (l, t) = ops::leader(d, space, &pts, &f[1], v[0], style)?;
            vec![l, t]
        }
        "hatch" | "hatch-objects" => {
            if args.len() < 5 {
                return Err(format!("{cmd} takes the pattern, scale, angle and double, then {}", if cmd == "hatch" { "a point" } else { "objects" }));
            }
            let pattern = args[0];
            let v = nums(&args[1..3], 2)?;
            let double = flag(args[3])?;
            if cmd == "hatch" {
                let p = nums(&args[4..], 2)?;
                vec![ops::hatch_at(d, space, pt(&p, 0), pattern, v[0], v[1].to_radians(), double)?]
            } else {
                let hs = handle_list(&args[4..])?;
                vec![ops::hatch_objects(d, &hs, pattern, v[0], v[1].to_radians(), double)?]
            }
        }
        "dim-angular" | "dim-angular-lines" | "dim-angular-arc" => {
            let dim = match cmd {
                "dim-angular" => {
                    let v = nums(args, 8)?;
                    let dm = Dimension::angular(pt(&v, 0), pt(&v, 2), pt(&v, 4), pt(&v, 6));
                    if dm.angular_arc().is_none() {
                        return Err("the points do not make an angle".into());
                    }
                    dm
                }
                "dim-angular-lines" => {
                    if args.len() != 4 {
                        return Err("dim-angular-lines takes two lines and a point".into());
                    }
                    let ends = |h: Handle| -> R<(Vec2, Vec2)> {
                        match &d.get(h).ok_or("no such object")?.kind {
                            ObjectKind::Line(l) => Ok((l.start.xy(), l.end.xy())),
                            _ => Err("pick two lines".into()),
                        }
                    };
                    let (a, b) = (ends(parse_handle(args[0])?)?, ends(parse_handle(args[1])?)?);
                    let at = nums(&args[2..], 2)?;
                    Dimension::angular_lines(a.0, a.1, b.0, b.1, pt(&at, 0)).ok_or("the lines are parallel: they make no angle")?
                }
                _ => {
                    if args.len() != 3 {
                        return Err("dim-angular-arc takes an arc and a point".into());
                    }
                    let h = parse_handle(args[0])?;
                    let ObjectKind::Arc(a) = &d.get(h).ok_or("no such object")?.kind else {
                        return Err("that is not an arc".into());
                    };
                    let c = a.center.xy();
                    let p1 = c + Vec2::from_angle(a.start_angle) * a.radius;
                    let p2 = c + Vec2::from_angle(a.end_angle) * a.radius;
                    let at = nums(&args[1..], 2)?;
                    let mut dm = Dimension::angular(c, p1, p2, pt(&at, 0));
                    // The arc's own angle, whichever side the dimension is.
                    if dm.angular_arc().is_some_and(|x| (x.start - a.start_angle).rem_euclid(std::f64::consts::TAU) > 1e-9) {
                        let r = pt(&at, 0).distance(c);
                        let mid = a.start_angle + cad_core::geom::curve::normalize_sweep(a.end_angle - a.start_angle) / 2.0;
                        dm = Dimension::angular(c, p1, p2, c + Vec2::from_angle(mid) * r);
                    }
                    dm
                }
            };
            let dimstyle = ops::current_dimstyle(d);
            let layer = ops::dimension_layer(d);
            vec![d.add_entity_in(space, layer, ObjectKind::Dimension(Dimension { dimstyle, ..dim })).map_err(err)?]
        }
        "dim-baseline" | "dim-continue" => {
            if args.len() != 3 {
                return Err(format!("{cmd} takes a dimension and a point"));
            }
            let h = parse_handle(args[0])?;
            let v = nums(&args[1..], 2)?;
            vec![ops::dim_next(d, h, pt(&v, 0), cmd == "dim-baseline")?]
        }
        "attdef" => {
            let f = fields(args);
            if f.len() != 4 {
                return Err("attdef takes x y height rotation | TAG | prompt | default".into());
            }
            let words: Vec<&str> = f[0].split_whitespace().collect();
            let v = nums(&words, 4)?;
            vec![blocks::attdef(d, space, &f[1], &f[2], &f[3], pt(&v, 0), v[2], v[3].to_radians())?]
        }
        "block" => {
            let f = fields(args);
            if f.len() != 2 || f[1].is_empty() {
                return Err("block takes the base point, retain, convert or delete, the objects, then | and the name".into());
            }
            let words: Vec<&str> = f[0].split_whitespace().collect();
            if words.len() < 4 {
                return Err("block takes the base point, what to do with the objects, and the objects".into());
            }
            let v = nums(&words[..2], 2)?;
            let keep = match words[2].to_ascii_lowercase().as_str() {
                "retain" | "r" => blocks::Keep::Retain,
                "convert" | "c" => blocks::Keep::Convert,
                "delete" | "d" => blocks::Keep::Delete,
                o => return Err(format!("{o:?}: retain, convert or delete")),
            };
            let hs = handle_list(&words[3..])?;
            let (b, i) = blocks::define(d, &f[1], pt(&v, 0), &hs, keep)?;
            let mut out = vec![b];
            out.extend(i);
            out
        }
        "insert" => {
            let f = fields(args);
            if f.len() < 2 || f[1].is_empty() {
                return Err("insert takes x y scale rotation | the block's name | TAG=value …".into());
            }
            let words: Vec<&str> = f[0].split_whitespace().collect();
            let v = nums(&words, 4)?;
            let block = blocks::find(d, &f[1]).ok_or_else(|| format!("there is no block named {}", f[1]))?;
            let mut values = Vec::new();
            for kv in &f[2..] {
                if kv.is_empty() {
                    continue;
                }
                let (k, val) = kv.split_once('=').ok_or_else(|| format!("{kv:?}: attribute values are TAG=value"))?;
                values.push((k.trim().to_string(), val.trim().to_string()));
            }
            vec![blocks::insert(d, space, block, pt(&v, 0), v[2], v[3].to_radians(), &values)?]
        }
        "attedit" => {
            let f = fields(args);
            if f.len() != 3 {
                return Err("attedit takes an insert | TAG | value".into());
            }
            let h = parse_handle(&f[0])?;
            vec![blocks::set_attribute(d, h, &f[1], &f[2])?]
        }
        "units" => {
            let v = nums(args, 6)?;
            let code = v[0] as u16;
            if v[0].fract() != 0.0 || Unit::from_insunits(code).is_none() {
                return Err("that is not a unit code".into());
            }
            let (lunits, luprec, aunits, auprec) = (v[1] as u16, v[2] as u16, v[3] as u16, v[4] as u16);
            if !(1..=5).contains(&lunits) || luprec > 8 || aunits > 4 || auprec > 8 {
                return Err("formats: lengths 1 to 5 with 0 to 8 places, angles 0 to 4 with 0 to 8 places".into());
            }
            ops::convert_units(d, Unit::from_insunits(code).expect("checked"), v[5] != 0.0)?;
            d.header.lunits = Some(lunits);
            d.header.luprec = Some(luprec);
            d.header.aunits = Some(aunits);
            d.header.auprec = Some(auprec);
            Vec::new()
        }
        "linetype-load" => {
            if args.is_empty() {
                return Err("which linetypes?".into());
            }
            let mut out = Vec::new();
            for name in args {
                out.push(linetype(d, name)?);
            }
            out
        }
        "linetype-current" => {
            let name = args.join(" ");
            let h = match name.to_ascii_lowercase().as_str() {
                "bylayer" | "byblock" => d.linetype_by_name(&name).ok_or("the drawing has no such linetype record")?,
                _ => linetype(d, &name)?,
            };
            d.set_current_linetype(h).map_err(err)?;
            vec![h]
        }
        "ltscale" => {
            let v = nums(args, 1)?;
            if v[0] <= 0.0 {
                return Err("the linetype scale must be more than zero".into());
            }
            d.header.ltscale = Some(v[0]);
            Vec::new()
        }
        "color-current" => {
            d.set_current_color(parse_color(&args.join(" "))?).map_err(err)?;
            Vec::new()
        }
        "lineweight-current" => {
            d.set_current_lineweight(parse_weight(&args.join(" "))?).map_err(err)?;
            Vec::new()
        }
        "layer-linetype" | "layer-lineweight" => {
            let (h, rest) = args.split_first().ok_or("which layer?")?;
            let l = layer(d, h)?;
            if cmd == "layer-linetype" {
                let lt = linetype(d, &rest.join(" "))?;
                d.set_layer_linetype(l, lt).map_err(err)?;
            } else {
                let w = parse_weight(&rest.join(" "))?;
                d.set_layer_lineweight(l, w).map_err(err)?;
            }
            vec![l]
        }
        "layout-new" => {
            let (paper, name) = args.split_first().ok_or("layout-new takes a paper size and a name")?;
            let paper = if *paper == "-" { "" } else { paper };
            vec![layouts::new_layout(d, &name.join(" "), paper)?]
        }
        "sheet" => {
            // A sheet ready to plot: paper | name | LABEL=value | …
            let f = fields(args);
            let paper = f.first().map(|p| if p == "-" { "" } else { p.as_str() }).unwrap_or("");
            let name = f.get(1).map(|n| if n == "-" { "" } else { n.as_str() }).unwrap_or("");
            let mut values = Vec::new();
            for kv in f.iter().skip(2).filter(|kv| !kv.is_empty()) {
                let (k, v) = kv.split_once('=').ok_or_else(|| format!("{kv:?}: title block fields are LABEL=value"))?;
                values.push((k.trim().to_string(), v.trim().to_string()));
            }
            let made = cad_core::sheet::new_sheet(d, name, paper, &values)?;
            vec![made.block, made.title, made.viewport]
        }
        "layout-rename" => {
            let f = fields(args);
            if f.len() != 2 {
                return Err("layout-rename takes the old name | the new name".into());
            }
            layouts::rename_layout(d, &f[0], &f[1])?;
            Vec::new()
        }
        "layout-delete" => {
            layouts::delete_layout(d, &args.join(" "))?;
            Vec::new()
        }
        "page-setup" => {
            if args.len() != 3 {
                return Err("page-setup takes the paper, landscape or portrait, and monochrome 0 or 1".into());
            }
            let portrait = match args[1].to_ascii_lowercase().as_str() {
                "portrait" | "p" => true,
                "landscape" | "l" => false,
                o => return Err(format!("{o:?}: landscape or portrait")),
            };
            layouts::set_paper(d, space, args[0], portrait)?;
            layouts::set_monochrome(d, space, flag(args[2])?)?;
            Vec::new()
        }
        "mview" if args.first().is_some_and(|a| a.eq_ignore_ascii_case("fit")) => {
            // MVIEW Fit: the paper inside its margin.
            let (a, b) = layouts::fit_corners(d, space)?;
            let corners = [a.x, a.y, b.x, b.y].map(|v| v.to_string());
            let mut v: Vec<&str> = corners.iter().map(String::as_str).collect();
            v.extend_from_slice(&args[1..]);
            return run(d, space, "mview", &v);
        }
        "mview" => {
            if args.len() < 4 {
                return Err("mview takes two corners or fit, then a scale (fit, 1:2, 0.5) and the model point at the middle".into());
            }
            let v = nums(&args[..4], 4)?;
            let scale = match args.get(4) {
                None => None,
                Some(s) if s.eq_ignore_ascii_case("fit") => None,
                Some(s) => Some(layouts::parse_scale(s, Unit::of(d)).ok_or_else(|| format!("{s:?} is not a scale"))?),
            };
            let center = match args.len() {
                5 | 4 => None,
                7 => {
                    let c = nums(&args[5..], 2)?;
                    Some(pt(&c, 0))
                }
                _ => return Err("the model point is x y".into()),
            };
            vec![layouts::add_viewport(d, space, pt(&v, 0), pt(&v, 2), scale, center)?]
        }
        "vp-scale" | "vp-center" => {
            let (h, rest) = args.split_first().ok_or("which viewport?")?;
            let h = parse_handle(h)?;
            if cmd == "vp-scale" {
                let s = rest.join(" ");
                let k = layouts::parse_scale(&s, Unit::of(d)).ok_or_else(|| format!("{s:?} is not a scale"))?;
                layouts::set_view(d, h, Some(k), None)?;
            } else {
                let c = nums(rest, 2)?;
                layouts::set_view(d, h, None, Some(pt(&c, 0)))?;
            }
            vec![h]
        }
        _ => return Ok(None),
    };
    Ok(Some(out))
}

fn hex(h: Handle) -> String {
    format!("{:X}", h.0)
}

/// Answer a query this module knows; `None` for one it does not.
pub(crate) fn query(d: &Drawing, space: Handle, cmd: &str, args: &[&str]) -> Option<R<String>> {
    let out = match cmd {
        "layouts" => {
            let mut j = String::from("{\"layouts\":[");
            for (i, s) in layouts::sheets(d).iter().enumerate() {
                if i > 0 {
                    j.push(',');
                }
                let _ = write!(
                    j,
                    "{{\"name\":\"{}\",\"block\":\"{}\",\"model\":{},\"current\":{}",
                    json_escape(&s.name),
                    hex(s.block),
                    s.block == d.model_space(),
                    s.block == space
                );
                if let Some((w, h)) = s.paper {
                    let _ = write!(j, ",\"paper\":[{},{}],\"paperName\":\"{}\"", json_num(w), json_num(h), json_escape(&s.paper_name));
                }
                let _ = write!(j, ",\"monochrome\":{}}}", layouts::monochrome(d, s.block));
            }
            j.push_str("],\"papers\":[");
            let names: Vec<String> = layouts::PAPERS.iter().map(|p| format!("\"{}\"", p.name)).collect();
            j.push_str(&names.join(","));
            let _ = write!(j, "],\"defaultPaper\":\"{}\"}}", json_escape(layouts::default_paper(Unit::of(d)).name));
            Ok(j)
        }
        "blocks" => {
            let mut j = String::from("{\"blocks\":[");
            for (i, (h, b)) in blocks::insertable(d).iter().enumerate() {
                if i > 0 {
                    j.push(',');
                }
                let _ = write!(j, "{{\"name\":\"{}\",\"handle\":\"{}\",\"attributes\":[", json_escape(&b.name), hex(*h));
                for (k, (tag, prompt, default)) in blocks::attribute_definitions(d, *h).iter().enumerate() {
                    if k > 0 {
                        j.push(',');
                    }
                    let _ = write!(
                        j,
                        "{{\"tag\":\"{}\",\"prompt\":\"{}\",\"default\":\"{}\"}}",
                        json_escape(tag),
                        json_escape(prompt),
                        json_escape(default)
                    );
                }
                j.push_str("]}");
            }
            j.push_str("]}");
            Ok(j)
        }
        "linetypes" => {
            let mut j = String::from("{\"loaded\":[");
            let mut first = true;
            for o in d.objects.values() {
                if let ObjectKind::Linetype(l) = &o.kind {
                    if l.name.eq_ignore_ascii_case("ByLayer") || l.name.eq_ignore_ascii_case("ByBlock") {
                        continue;
                    }
                    if !first {
                        j.push(',');
                    }
                    first = false;
                    let _ = write!(
                        j,
                        "{{\"name\":\"{}\",\"description\":\"{}\",\"handle\":\"{}\",\"pattern\":[{}]}}",
                        json_escape(&l.name),
                        json_escape(&l.description),
                        hex(o.handle),
                        l.dashes().iter().map(|v| json_num(*v)).collect::<Vec<_>>().join(",")
                    );
                }
            }
            let current = d.record_name(d.header.celtype).unwrap_or("ByLayer");
            let lib: Vec<String> = standards::library_names().iter().map(|n| format!("\"{n}\"")).collect();
            let _ = write!(
                j,
                "],\"library\":[{}],\"current\":\"{}\",\"ltscale\":{}}}",
                lib.join(","),
                json_escape(current),
                json_num(d.header.ltscale.unwrap_or(1.0))
            );
            Ok(j)
        }
        "patterns" => {
            let mut j = String::from("{\"patterns\":[{\"name\":\"SOLID\",\"description\":\"Solid fill\"},{\"name\":\"USER\",\"description\":\"Parallel lines at the spacing and angle given\"}");
            for p in cad_core::render::hatch::PATTERNS {
                let _ = write!(j, ",{{\"name\":\"{}\",\"description\":\"{}\"}}", p.name, json_escape(p.description));
            }
            j.push_str("]}");
            Ok(j)
        }
        "bounds" => (|| {
            let hs = handle_list(args)?;
            let b = construct::selection_bounds(d, &hs);
            let (row, col) = construct::array_spacing(d, &hs);
            let bx = if b.is_empty() {
                "null".to_string()
            } else {
                format!("[{},{},{},{}]", json_num(b.min.x), json_num(b.min.y), json_num(b.max.x), json_num(b.max.y))
            };
            Ok(format!("{{\"box\":{bx},\"rowGap\":{},\"columnGap\":{}}}", json_num(row), json_num(col)))
        })(),
        "measure" => (|| {
            let v = nums(args, 4)?;
            let m = construct::measure(pt(&v, 0), pt(&v, 2));
            Ok(format!(
                "{{\"distance\":{},\"angle\":{},\"dx\":{},\"dy\":{},\"text\":\"{}\"}}",
                json_num(m.distance),
                json_num(m.angle.to_degrees()),
                json_num(m.delta.x),
                json_num(m.delta.y),
                json_escape(&construct::describe_measure(d, &m))
            ))
        })(),
        "grips" => (|| {
            let hs = handle_list(args)?;
            let mut list = Vec::new();
            for h in hs {
                for g in grips::grips(d, h) {
                    list.push(format!(
                        "{{\"x\":{},\"y\":{},\"handle\":\"{}\",\"kind\":\"{}\",\"field\":\"{}\"}}",
                        json_num(g.at.x),
                        json_num(g.at.y),
                        hex(h),
                        g.kind.name(),
                        g.kind.field().map(|f| f.to_string()).unwrap_or_else(|| "-".into())
                    ));
                }
            }
            Ok(format!("{{\"grips\":[{}]}}", list.join(",")))
        })(),
        "attribs" => (|| {
            let h = parse_handle(args.first().ok_or("which insert?")?)?;
            let mut j = String::from("{\"attributes\":[");
            for (i, (a, tag, value)) in blocks::attributes(d, h).iter().enumerate() {
                if i > 0 {
                    j.push(',');
                }
                let _ = write!(j, "{{\"handle\":\"{}\",\"tag\":\"{}\",\"value\":\"{}\"}}", hex(*a), json_escape(tag), json_escape(value));
            }
            j.push_str("]}");
            Ok(j)
        })(),
        _ => return None,
    };
    Some(out)
}

/// What `get` adds for the object kinds this module brought in.
pub(crate) fn describe(d: &Drawing, o: &Object, j: &mut String) {
    let n = json_num;
    match &o.kind {
        ObjectKind::Xline(x) | ObjectKind::Ray(x) => {
            let _ = write!(
                j,
                ",\"base\":[{},{}],\"direction\":[{},{}],\"angle\":{}",
                n(x.base.x),
                n(x.base.y),
                n(x.direction.x),
                n(x.direction.y),
                n(x.direction.xy().angle().to_degrees().rem_euclid(360.0))
            );
        }
        ObjectKind::MText(m) => {
            let _ = write!(
                j,
                ",\"insertion\":[{},{}],\"height\":{},\"width\":{},\"rotation\":{},\"value\":\"{}\"",
                n(m.insertion.x),
                n(m.insertion.y),
                n(m.text_height),
                n(m.rect_width),
                n(m.x_axis.xy().angle().to_degrees()),
                json_escape(&m.contents)
            );
        }
        ObjectKind::Hatch(h) => {
            let _ = write!(
                j,
                ",\"pattern\":\"{}\",\"scale\":{},\"angle\":{},\"loops\":{},\"solid\":{}",
                json_escape(if h.pattern_name == "_USER" { "USER" } else { &h.pattern_name }),
                n(h.pattern_scale),
                n(h.pattern_angle.to_degrees()),
                h.paths.len(),
                h.solid_fill
            );
        }
        ObjectKind::Leader(l) => {
            let pts: Vec<String> = l.vertices.iter().map(|p| format!("[{},{}]", n(p.x), n(p.y))).collect();
            let _ = write!(j, ",\"points\":[{}]", pts.join(","));
        }
        ObjectKind::Insert(i) => {
            let name = d.block_header(i.block_header).map(|b| b.name.clone()).unwrap_or_default();
            let _ = write!(
                j,
                ",\"block\":\"{}\",\"insertion\":[{},{}],\"scale\":{},\"rotation\":{},\"attributes\":[",
                json_escape(&name),
                n(i.insertion.x),
                n(i.insertion.y),
                n(i.scale.x),
                n(i.rotation.to_degrees())
            );
            for (k, (a, tag, value)) in blocks::attributes(d, o.handle).iter().enumerate() {
                if k > 0 {
                    j.push(',');
                }
                let _ = write!(j, "{{\"handle\":\"{}\",\"tag\":\"{}\",\"value\":\"{}\"}}", hex(*a), json_escape(tag), json_escape(value));
            }
            j.push(']');
        }
        ObjectKind::Attribute(a) => {
            let _ = write!(
                j,
                ",\"tag\":\"{}\",\"prompt\":\"{}\",\"value\":\"{}\",\"insertion\":[{},{}],\"height\":{},\"definition\":{}",
                json_escape(&a.tag),
                json_escape(&a.prompt),
                json_escape(&a.text.value),
                n(a.text.insertion.x),
                n(a.text.insertion.y),
                n(a.text.height),
                a.is_definition
            );
        }
        ObjectKind::PaperViewport(v) => {
            let _ = write!(
                j,
                ",\"center\":[{},{}],\"width\":{},\"height\":{},\"scale\":{},\"viewCenter\":[{},{}]",
                n(v.center.x),
                n(v.center.y),
                n(v.width),
                n(v.height),
                n(v.scale()),
                n(v.view_center.x),
                n(v.view_center.y)
            );
        }
        ObjectKind::Dimension(dm) => {
            let kind = match dm.kind {
                DimensionKind::Linear => "linear",
                DimensionKind::Aligned => "aligned",
                DimensionKind::Angular3Pt | DimensionKind::Angular2Line => "angular",
                DimensionKind::Radius => "radius",
                DimensionKind::Diameter => "diameter",
                DimensionKind::Ordinate => "ordinate",
            };
            let measured = match dm.kind {
                DimensionKind::Angular3Pt | DimensionKind::Angular2Line => dm.measurement.to_degrees(),
                _ => dm.measurement,
            };
            let _ = write!(
                j,
                ",\"dimension\":\"{kind}\",\"measurement\":{},\"value\":\"{}\",\"p1\":[{},{}],\"p2\":[{},{}]",
                n(measured),
                json_escape(&dm.user_text),
                n(dm.pt13.x),
                n(dm.pt13.y),
                n(dm.pt14.x),
                n(dm.pt14.y)
            );
        }
        ObjectKind::Ellipse(e) => {
            let _ = write!(
                j,
                ",\"center\":[{},{}],\"major\":{},\"ratio\":{}",
                n(e.center.x),
                n(e.center.y),
                n(e.major_axis.length()),
                n(e.axis_ratio)
            );
        }
        _ => {}
    }
    if let Some(e) = &o.entity {
        let _ = write!(j, ",\"ltscale\":{}", n(e.linetype_scale));
    }
}

/// Set a value of the kinds this module brought in, and the properties
/// every object has; `None` for fields it does not know.
pub(crate) fn set(d: &mut Drawing, h: Handle, field: &str, raw: &str) -> Option<R<()>> {
    let v = || parse_num(raw.trim());
    let positive = || -> R<f64> {
        let x = parse_num(raw.trim())?;
        if x > 0.0 {
            Ok(x)
        } else {
            Err("the value must be more than zero".into())
        }
    };
    // Properties of any object.
    match field {
        "color" | "colour" => {
            return Some((|| {
                let c = parse_color(raw)?;
                d.objects.get_mut(&h).and_then(|o| o.entity.as_mut()).ok_or("not an object")?.color = c;
                Ok(())
            })())
        }
        "linetype" => {
            return Some((|| {
                let lt = match raw.trim().to_ascii_lowercase().as_str() {
                    "bylayer" => LinetypeRef::ByLayer,
                    "byblock" => LinetypeRef::ByBlock,
                    "continuous" => LinetypeRef::Continuous,
                    _ => LinetypeRef::Handle(linetype(d, raw)?),
                };
                d.objects.get_mut(&h).and_then(|o| o.entity.as_mut()).ok_or("not an object")?.linetype = lt;
                Ok(())
            })())
        }
        "lineweight" => {
            return Some((|| {
                let w = parse_weight(raw)?;
                d.objects.get_mut(&h).and_then(|o| o.entity.as_mut()).ok_or("not an object")?.lineweight = w.code();
                Ok(())
            })())
        }
        "ltscale" => {
            return Some((|| {
                let k = positive()?;
                d.objects.get_mut(&h).and_then(|o| o.entity.as_mut()).ok_or("not an object")?.linetype_scale = k;
                Ok(())
            })())
        }
        _ => {}
    }
    let unit = Unit::of(d);
    match &d.get(h)?.kind {
        ObjectKind::Insert(_) => {
            return Some(match field {
                "x" | "y" | "scale" | "rotation" => set_insert(d, h, field, raw),
                _ => Err(format!("{field:?} is not a property that can be set here")),
            })
        }
        ObjectKind::PaperViewport(vp) => {
            let center = vp.view_center;
            return Some(match field {
                "scale" => layouts::parse_scale(raw, unit)
                    .ok_or_else(|| format!("{raw:?} is not a scale"))
                    .and_then(|k| layouts::set_view(d, h, Some(k), None)),
                "cx" | "cy" => v().and_then(|x| {
                    let c = if field == "cx" { Vec2::new(x, center.y) } else { Vec2::new(center.x, x) };
                    layouts::set_view(d, h, None, Some(c))
                }),
                _ => Err(format!("{field:?} is not a property that can be set here")),
            });
        }
        _ => {}
    }
    let o = d.objects.get_mut(&h)?;
    let unknown = || Err(format!("{field:?} is not a property that can be set here"));
    let r: R<()> = match &mut o.kind {
        ObjectKind::MText(m) => (|| {
            match field {
                "x" => m.insertion.x = v()?,
                "y" => m.insertion.y = v()?,
                "height" => m.text_height = positive()?,
                "width" => {
                    let w = v()?;
                    if w < 0.0 {
                        return Err("the width cannot be negative".into());
                    }
                    m.rect_width = w
                }
                "rotation" => m.x_axis = Vec2::from_angle(v()?.to_radians()).extend(0.0),
                "value" => {
                    if raw.is_empty() {
                        return Err("text cannot be empty".into());
                    }
                    m.contents = raw.to_string()
                }
                _ => return unknown(),
            }
            Ok(())
        })(),
        ObjectKind::Hatch(hh) => (|| {
            match field {
                "pattern" => hh.pattern_name = raw.trim().to_ascii_uppercase(),
                "scale" => hh.pattern_scale = positive()?,
                "angle" => hh.pattern_angle = v()?.to_radians(),
                _ => return unknown(),
            }
            let user = hh.pattern_name == "USER" || hh.pattern_name == "_USER";
            let name = if user { "USER".to_string() } else { hh.pattern_name.clone() };
            let lines = cad_core::render::hatch::pattern_lines(&name, hh.pattern_scale, hh.pattern_angle, hh.pattern_double, unit)
                .ok_or_else(|| format!("there is no hatch pattern {name:?}"))?;
            hh.solid_fill = name == "SOLID";
            hh.pattern_type = if user { 0 } else { 1 };
            hh.pattern_name = if user { "_USER".into() } else { name };
            hh.pattern_lines = lines;
            Ok(())
        })(),
        ObjectKind::Attribute(a) => (|| {
            match field {
                "value" => a.text.value = raw.to_string(),
                "height" => a.text.height = positive()?,
                "x" => a.text.insertion.x = v()?,
                "y" => a.text.insertion.y = v()?,
                "tag" if a.is_definition => {
                    let t = raw.trim().to_ascii_uppercase();
                    if t.is_empty() || t.contains(char::is_whitespace) {
                        return Err("a tag is one word, with no spaces".into());
                    }
                    a.tag = t
                }
                "prompt" if a.is_definition => a.prompt = raw.to_string(),
                _ => return unknown(),
            }
            Ok(())
        })(),
        ObjectKind::Dimension(dm) => (|| {
            match field {
                "value" | "text" => dm.user_text = raw.to_string(),
                _ => return unknown(),
            }
            dm.block = Handle::NULL;
            Ok(())
        })(),
        ObjectKind::Xline(x) | ObjectKind::Ray(x) => (|| {
            match field {
                "x" => x.base.x = v()?,
                "y" => x.base.y = v()?,
                "angle" => x.direction = Vec2::from_angle(v()?.to_radians()).extend(0.0),
                _ => return unknown(),
            }
            Ok(())
        })(),
        _ => return None,
    };
    Some(r)
}

/// An insert moved, scaled or turned, its attributes with it.
fn set_insert(d: &mut Drawing, h: Handle, field: &str, raw: &str) -> R<()> {
    let ObjectKind::Insert(i) = &d.get(h).ok_or("no such object")?.kind else {
        return Err("not an insert".into());
    };
    let at = i.insertion.xy();
    let x = parse_num(raw.trim())?;
    let xf = match field {
        "x" => Affine::translate(Vec2::new(x - at.x, 0.0)),
        "y" => Affine::translate(Vec2::new(0.0, x - at.y)),
        "scale" => {
            if !(x > 0.0) || i.scale.x == 0.0 {
                return Err("the scale must be more than zero".into());
            }
            Affine::scale(at, x / i.scale.x.abs())
        }
        _ => Affine::rotate(at, x.to_radians() - i.rotation),
    };
    ops::transform_entities(d, &[h], &xf)
}

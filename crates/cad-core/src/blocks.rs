//! Blocks: definitions made from drawn objects (BLOCK), references to them
//! with attribute values (INSERT), attribute definitions and values
//! (ATTDEF, ATTEDIT), and taking a reference apart again (EXPLODE).
//!
//! A definition keeps its objects where they were drawn and records the
//! base point; an insert places the base point, scaled and turned. Objects
//! on layer `0` inside a block take the insert's layer when drawn, as in
//! every DWG program.

use crate::geom::{Vec2, Vec3};
use crate::model::*;
use crate::modify::{self, Affine};
use crate::ops;

type R<T> = Result<T, String>;

fn err(e: EditError) -> String {
    e.to_string()
}

/// What BLOCK does with the objects it was made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Leave them as they are.
    Retain,
    /// Replace them with an insert of the new block (the default).
    Convert,
    /// Erase them.
    Delete,
}

/// A block definition named `name` with base point `base`, from copies of
/// the objects `hs`. The new block record, and the insert that replaced
/// the objects when `keep` is [`Keep::Convert`].
pub fn define(d: &mut Drawing, name: &str, base: Vec2, hs: &[Handle], keep: Keep) -> R<(Handle, Option<Handle>)> {
    if hs.is_empty() {
        return Err("select the objects for the block".into());
    }
    for h in hs {
        let o = d.get(*h).ok_or_else(|| format!("no object {:X}", h.0))?;
        if o.entity.is_none() {
            return Err("only drawn objects can go in a block".into());
        }
        if keep != Keep::Retain {
            ops::check_unlocked(d, *h)?;
        }
    }
    let space = ops::block_of(d, hs[0])?;
    let block = d.add_block_record(name.trim(), base.extend(0.0)).map_err(err)?;
    let mut attribs = false;
    for h in hs {
        let nh = ops::copy_entity(d, *h, Some(block), &Affine::IDENTITY)?;
        attribs |= matches!(&d.get(nh).map(|o| &o.kind), Some(ObjectKind::Attribute(a)) if a.is_definition);
    }
    if attribs {
        if let Some(ObjectKind::BlockHeader(b)) = d.objects.get_mut(&block).map(|o| &mut o.kind) {
            b.has_attribs = true;
        }
    }
    let mut made = None;
    if keep != Keep::Retain {
        for h in hs {
            d.erase_entity(*h).map_err(err)?;
        }
        if keep == Keep::Convert {
            made = Some(insert(d, space, block, base, 1.0, 0.0, &[])?);
        }
    }
    Ok((block, made))
}

/// A block record by name, or by handle in hex.
pub fn find(d: &Drawing, name: &str) -> Option<Handle> {
    if let Some((h, _)) = d.block_by_name(name) {
        return Some(h);
    }
    let h = Handle(u64::from_str_radix(name.trim_start_matches("0x"), 16).ok()?);
    d.block_header(h).map(|_| h)
}

/// The blocks a user can insert: not the spaces, not anonymous ones
/// (dimension graphics, hatches), not external references.
pub fn insertable(d: &Drawing) -> Vec<(Handle, &BlockHeader)> {
    let mut v: Vec<(Handle, &BlockHeader)> = d
        .objects
        .values()
        .filter_map(|o| match &o.kind {
            ObjectKind::BlockHeader(b) if !b.anonymous && !b.is_xref && !b.name.starts_with('*') => Some((o.handle, b)),
            _ => None,
        })
        .collect();
    v.sort_by_key(|(_, b)| b.name.to_ascii_lowercase());
    v
}

/// The attribute definitions of a block, in drawing order: tag, prompt,
/// default value. Constant ones (shown from the definition) are left out.
pub fn attribute_definitions(d: &Drawing, block: Handle) -> Vec<(String, String, String)> {
    d.block_entities(block)
        .iter()
        .filter_map(|o| match &o.kind {
            ObjectKind::Attribute(a) if a.is_definition && a.flags & 2 == 0 => Some((a.tag.clone(), a.prompt.clone(), a.text.value.clone())),
            _ => None,
        })
        .collect()
}

/// The transform of an insert: block coordinates to where it is drawn.
fn placement(base: Vec3, at: Vec2, scale: f64, rotation: f64) -> Affine {
    Affine::translate(at)
        .after(&Affine::rotate(Vec2::ZERO, rotation))
        .after(&Affine::scale(Vec2::ZERO, scale))
        .after(&Affine::translate(-base.xy()))
}

/// Insert `block` into `space` at `at`, `scale` times its size, turned
/// `rotation` radians, on the current layer. Each attribute definition
/// becomes an attribute: the value given for its tag (any case), or its
/// default.
pub fn insert(d: &mut Drawing, space: Handle, block: Handle, at: Vec2, scale: f64, rotation: f64, values: &[(String, String)]) -> R<Handle> {
    if !(scale.is_finite() && scale != 0.0) {
        return Err("the scale must be a number other than zero".into());
    }
    if !(rotation.is_finite() && at.is_finite()) {
        return Err("the insertion point and rotation must be numbers".into());
    }
    let bh = d.block_header(block).ok_or("there is no such block")?.clone();
    if bh.is_xref {
        return Err("external references cannot be inserted here".into());
    }
    if block == space || block == d.model_space() || block == d.paper_space() {
        return Err("a space cannot be inserted".into());
    }
    if let Some((tag, _)) = values.iter().find(|(t, _)| !attribute_definitions(d, block).iter().any(|a| a.0.eq_ignore_ascii_case(t))) {
        return Err(format!("the block has no attribute {tag}"));
    }
    let defs: Vec<Object> = d
        .block_entities(block)
        .into_iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Attribute(a) if a.is_definition && a.flags & 2 == 0))
        .cloned()
        .collect();
    let ins = Insert {
        insertion: at.extend(0.0),
        scale: Vec3::new(scale, scale, scale),
        rotation,
        extrusion: Vec3::Z,
        has_attribs: !defs.is_empty(),
        block_header: block,
        ..Default::default()
    };
    let h = d.add_entity_in(space, Handle::NULL, ObjectKind::Insert(ins)).map_err(err)?;
    if defs.is_empty() {
        return Ok(h);
    }
    let insert_common = d.get(h).and_then(|o| o.entity.clone()).expect("added");
    let xf = placement(bh.base_point, at, scale, rotation);
    let layer0 = d.layer_by_name("0").map(|(l, _)| l);
    let mut kids = Vec::new();
    for def in defs {
        let ObjectKind::Attribute(a) = &def.kind else { continue };
        let value = values
            .iter()
            .find(|(t, _)| t.eq_ignore_ascii_case(&a.tag))
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| a.text.value.clone());
        let mut kind = ObjectKind::Attribute(Attribute {
            text: Text { value, ..a.text.clone() },
            tag: a.tag.clone(),
            prompt: String::new(),
            flags: a.flags,
            field_length: a.field_length,
            is_definition: false,
        });
        modify::transform(&mut kind, &xf)?;
        // Definitions on layer 0 take the insert's layer.
        let mut common = def.entity.clone().unwrap_or_default();
        if Some(common.layer) == layer0 {
            common.layer = insert_common.layer;
        }
        kids.push(d.add_owned(h, "ATTRIB", 0x02, kind, Some(common)));
    }
    let seqend = d.add_owned(h, "SEQEND", 0x06, ObjectKind::SeqEnd, Some(insert_common));
    if let Some(ObjectKind::Insert(i)) = d.objects.get_mut(&h).map(|o| &mut o.kind) {
        i.first_attrib = kids.first().copied().unwrap_or(Handle::NULL);
        i.last_attrib = kids.last().copied().unwrap_or(Handle::NULL);
        i.attribs = kids;
        i.seqend = seqend;
    }
    Ok(h)
}

/// An insert's attributes: (handle, tag, value).
pub fn attributes(d: &Drawing, insert: Handle) -> Vec<(Handle, String, String)> {
    let Some(o) = d.get(insert) else { return Vec::new() };
    d.insert_attributes(o)
        .iter()
        .filter_map(|a| match &a.kind {
            ObjectKind::Attribute(x) => Some((a.handle, x.tag.clone(), x.text.value.clone())),
            _ => None,
        })
        .collect()
}

/// Change the value of an insert's attribute (ATTEDIT).
pub fn set_attribute(d: &mut Drawing, insert: Handle, tag: &str, value: &str) -> R<Handle> {
    ops::check_unlocked(d, insert)?;
    let (h, _, _) = attributes(d, insert)
        .into_iter()
        .find(|(_, t, _)| t.eq_ignore_ascii_case(tag))
        .ok_or_else(|| format!("the block has no attribute {tag}"))?;
    if let Some(ObjectKind::Attribute(a)) = d.objects.get_mut(&h).map(|o| &mut o.kind) {
        a.text.value = value.to_string();
    }
    Ok(h)
}

/// An attribute definition (ATTDEF) in `space`: shown as its tag until it
/// is made part of a block, whose inserts then ask for a value.
#[allow(clippy::too_many_arguments)]
pub fn attdef(d: &mut Drawing, space: Handle, tag: &str, prompt: &str, default: &str, at: Vec2, height: f64, rotation: f64) -> R<Handle> {
    let tag = tag.trim().to_ascii_uppercase();
    if tag.is_empty() || tag.contains(char::is_whitespace) {
        return Err("a tag is one word, with no spaces".into());
    }
    if !(height.is_finite() && height > 0.0) {
        return Err("the text height must be more than zero".into());
    }
    let style = d.header.textstyle;
    let text = Text {
        rotation,
        style,
        ..Text::new(at, height, default)
    };
    let kind = ObjectKind::Attribute(Attribute {
        text,
        tag,
        prompt: prompt.to_string(),
        flags: 0,
        field_length: 0,
        is_definition: true,
    });
    d.add_entity_in(space, Handle::NULL, kind).map_err(err)
}

/// Take an insert apart into copies of its block's objects, placed as
/// drawn, with its attributes as plain text; or a polyline into lines and
/// arcs (EXPLODE). The new objects' handles.
pub fn explode(d: &mut Drawing, h: Handle) -> R<Vec<Handle>> {
    ops::check_unlocked(d, h)?;
    let o = d.get(h).ok_or_else(|| format!("no object {:X}", h.0))?.clone();
    let space = ops::block_of(d, h)?;
    let mut out = Vec::new();
    match &o.kind {
        ObjectKind::Insert(ins) => {
            let (sx, sy) = (ins.scale.x, ins.scale.y);
            if (sx.abs() - sy.abs()).abs() > 1e-9 * sx.abs().max(sy.abs()) {
                return Err("an insert scaled differently along X and Y cannot be exploded here".into());
            }
            if ins.array.is_some() {
                return Err("a multiple insert cannot be exploded".into());
            }
            let bh = d.block_header(ins.block_header).ok_or("the insert's block is missing")?.clone();
            let mut xf = placement(bh.base_point, ins.insertion.xy(), sx.abs(), ins.rotation);
            if sx * sy < 0.0 {
                // A mirrored insert: mirror about the block's own Y axis.
                xf = xf.after(&Affine::mirror(bh.base_point.xy(), bh.base_point.xy() + Vec2::new(0.0, 1.0)));
            }
            let kids: Vec<Handle> = d.block_entities(ins.block_header).iter().map(|e| e.handle).collect();
            let insert_layer = o.entity.as_ref().map(|e| e.layer).unwrap_or(Handle::NULL);
            let layer0 = d.layer_by_name("0").map(|(l, _)| l);
            for k in kids {
                if matches!(&d.get(k).map(|x| &x.kind), Some(ObjectKind::Attribute(a)) if a.is_definition) {
                    continue;
                }
                let nh = ops::copy_entity(d, k, Some(space), &xf)?;
                if let Some(e) = d.objects.get_mut(&nh).and_then(|x| x.entity.as_mut()) {
                    if Some(e.layer) == layer0 {
                        e.layer = insert_layer;
                    }
                }
                out.push(nh);
            }
            for a in d.insert_attributes(&o).iter().map(|a| (*a).clone()).collect::<Vec<_>>() {
                if let ObjectKind::Attribute(at) = &a.kind {
                    if at.flags & 1 != 0 {
                        continue;
                    }
                    let nh = d.add_entity_in(space, a.entity.as_ref().map(|e| e.layer).unwrap_or(Handle::NULL), ObjectKind::Text(at.text.clone())).map_err(err)?;
                    out.push(nh);
                }
            }
        }
        ObjectKind::LwPolyline(p) => {
            let n = p.points.len();
            let segs = if p.is_closed() { n } else { n.saturating_sub(1) };
            for i in 0..segs {
                let (a, b) = (p.points[i], p.points[(i + 1) % n]);
                if a.distance(b) < 1e-12 {
                    continue;
                }
                let kind = match crate::geom::curve::bulge_to_arc(a, b, p.bulge(i)) {
                    Some((arc, _)) => ObjectKind::Arc(Arc::new(arc.center, arc.radius, arc.start, arc.end)),
                    None => ObjectKind::Line(Line::new(a, b)),
                };
                out.push(ops::add_like(d, h, kind)?);
            }
        }
        _ => return Err("only inserts and polylines can be exploded".into()),
    }
    d.erase_entity(h).map_err(err)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standards::{new_drawing, Template};

    #[test]
    fn a_title_block_with_attributes() {
        let mut d = new_drawing(Template::Metric);
        let ms = d.model_space();
        let frame = d
            .add_entity(Handle::NULL, ObjectKind::LwPolyline(LwPolyline::new(vec![Vec2::ZERO, Vec2::new(180.0, 0.0), Vec2::new(180.0, 40.0), Vec2::new(0.0, 40.0)], true)))
            .unwrap();
        let t = attdef(&mut d, ms, "title", "Drawing title", "PLATE", Vec2::new(5.0, 25.0), 5.0, 0.0).unwrap();
        let n = attdef(&mut d, ms, "DWGNO", "Drawing number", "P-001", Vec2::new(5.0, 10.0), 3.5, 0.0).unwrap();
        let (block, made) = define(&mut d, "TITLE", Vec2::ZERO, &[frame, t, n], Keep::Convert).unwrap();
        assert!(d.block_header(block).unwrap().has_attribs);
        assert_eq!(attribute_definitions(&d, block).len(), 2);
        let first = made.unwrap();
        assert_eq!(attributes(&d, first)[0].2, "PLATE");
        assert!(d.get(frame).is_none());
        // Insert it again, with values, turned and scaled.
        let vals = vec![("Title".to_string(), "BRACKET".to_string())];
        let i = insert(&mut d, ms, block, Vec2::new(400.0, 0.0), 2.0, std::f64::consts::FRAC_PI_2, &vals).unwrap();
        let attrs = attributes(&d, i);
        assert_eq!((attrs[0].1.as_str(), attrs[0].2.as_str()), ("TITLE", "BRACKET"));
        assert_eq!(attrs[1].2, "P-001");
        let ObjectKind::Attribute(a) = &d.get(attrs[0].0).unwrap().kind else { panic!() };
        // (5, 25) scaled 2 and turned 90° about the base, then moved.
        assert!(a.text.insertion.distance(Vec2::new(350.0, 10.0)) < 1e-9, "{:?}", a.text.insertion);
        assert!((a.text.height - 10.0).abs() < 1e-9);
        set_attribute(&mut d, i, "dwgno", "P-002").unwrap();
        assert_eq!(attributes(&d, i)[1].2, "P-002");
        assert!(insert(&mut d, ms, block, Vec2::ZERO, 1.0, 0.0, &[("NOPE".into(), "x".into())]).is_err());
        // Copies take their attributes along.
        let c = ops::copy_entities(&mut d, &[i], &Affine::translate(Vec2::new(0.0, 100.0))).unwrap()[0];
        assert_eq!(attributes(&d, c).len(), 2);
        assert_ne!(attributes(&d, c)[0].0, attrs[0].0);
        // The drawing saves and reads back with them.
        let bytes = crate::native::to_bytes(&d, &crate::native::WriteOptions::default());
        let back = crate::native::read_bytes(&bytes).unwrap();
        assert_eq!(attributes(&back, c).len(), 2);
        // Exploding gives the frame back and the values as text.
        let parts = explode(&mut d, i).unwrap();
        assert_eq!(parts.len(), 3);
        assert!(insertable(&d).iter().any(|(_, b)| b.name == "TITLE"));
        assert!(define(&mut d, "TITLE", Vec2::ZERO, &[parts[0]], Keep::Retain).is_err());
    }
}

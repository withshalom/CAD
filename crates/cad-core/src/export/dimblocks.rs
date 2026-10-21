//! Dimension graphics as blocks, for the files other programs read.
//!
//! DWG and DXF keep each dimension's graphics in an anonymous block (`*D`)
//! that the dimension names, and most programs draw that block rather than
//! work the graphics out again from the definition points. Dimensions made
//! or changed in this program carry no block (the renderer draws them from
//! their definition points, [`crate::render::dim`]). Before a drawing is
//! written as DWG or DXF, [`with_dimension_blocks`] gives a copy of it one
//! block per such dimension, holding what the renderer would draw: lines
//! and arcs, solids for the arrowheads, and the text as MTEXT, on layer
//! `0` in colour, linetype and lineweight by block, so they take the
//! dimension's own.

use crate::geom::{Vec2, Vec3};
use crate::model::*;
use crate::render::dim::{self, Part};
use crate::render::Shape;
use crate::standards;
use crate::units::Unit;
use std::borrow::Cow;

/// The drawing with a graphics block for every dimension that has none;
/// the drawing itself when none needs one.
pub fn with_dimension_blocks(d: &Drawing) -> Cow<'_, Drawing> {
    let needs: Vec<Handle> = d
        .objects
        .values()
        .filter(|o| o.entity.is_some())
        .filter(|o| matches!(&o.kind, ObjectKind::Dimension(dm) if d.block_header(dm.block).is_none()))
        .map(|o| o.handle)
        .collect();
    if needs.is_empty() {
        return Cow::Borrowed(d);
    }
    let mut out = d.clone();
    let unit = Unit::of(d);
    let layer0 = out.ensure_layer_zero();
    let mut n = 0usize;
    for h in needs {
        let Some(ObjectKind::Dimension(dm)) = out.get(h).map(|o| o.kind.clone()) else {
            continue;
        };
        let st = out
            .dim_style(dm.dimstyle)
            .or_else(|| out.dim_style(out.header.dimstyle))
            .cloned()
            .unwrap_or_else(|| standards::dim_style(unit));
        let Some(parts) = dim::graphics(&dm, &st, unit) else {
            continue;
        };
        // A name no block has: *D1, *D2 …
        let name = loop {
            n += 1;
            let name = format!("*D{n}");
            if out.block_by_name(&name).is_none() {
                break name;
            }
        };
        let Ok(block) = out.new_block_record(&name, Vec3::ZERO) else {
            continue;
        };
        if let Some(ObjectKind::BlockHeader(b)) = out.objects.get_mut(&block).map(|o| &mut o.kind) {
            b.anonymous = true;
        }
        let text_style = if out.get(st.text_style).is_some() { st.text_style } else { out.header.textstyle };
        for (part, shape) in parts {
            for kind in entities(&shape, part, text_style) {
                if let Ok(e) = out.add_entity_in(block, layer0, kind) {
                    if let Some(c) = out.objects.get_mut(&e).and_then(|o| o.entity.as_mut()) {
                        c.color = Color::ByBlock;
                        c.linetype = LinetypeRef::ByBlock;
                        c.lineweight = Lineweight::ByBlock.code();
                        c.linetype_scale = 1.0;
                    }
                }
            }
        }
        if let Some(ObjectKind::Dimension(x)) = out.objects.get_mut(&h).map(|o| &mut o.kind) {
            x.block = block;
        }
    }
    Cow::Owned(out)
}

/// The entities that draw one piece of a dimension.
fn entities(shape: &Shape, part: Part, text_style: Handle) -> Vec<ObjectKind> {
    match shape {
        Shape::Line { a, b } => vec![ObjectKind::Line(Line::new(*a, *b))],
        Shape::Arc { center, radius, start, end } => vec![ObjectKind::Arc(Arc::new(*center, *radius, *start, *end))],
        Shape::Circle { center, radius } => vec![ObjectKind::Circle(Circle::new(*center, *radius))],
        Shape::Fill { rings } => rings
            .iter()
            .filter(|r| r.len() >= 3)
            .map(|r| {
                // A SOLID's corners go 0, 1, 3, 2 round its outline.
                let c = if r.len() >= 4 { [r[0], r[1], r[3], r[2]] } else { [r[0], r[1], r[2], r[2]] };
                ObjectKind::Solid(Solid {
                    corners: c,
                    extrusion: Vec3::Z,
                    ..Default::default()
                })
            })
            .collect(),
        Shape::Path { points, closed } => {
            let mut v: Vec<ObjectKind> = points.windows(2).map(|w| ObjectKind::Line(Line::new(w[0], w[1]))).collect();
            if *closed && points.len() > 2 {
                v.push(ObjectKind::Line(Line::new(points[points.len() - 1], points[0])));
            }
            v
        }
        Shape::Text { position, height, rotation, text, h_align, .. } if part == Part::Text => {
            let attachment = match h_align {
                0 => 4,
                2 => 6,
                _ => 5,
            };
            let mut m = MText::new(*position, *height, 0.0, text, attachment);
            m.x_axis = Vec2::from_angle(*rotation).extend(0.0);
            m.style = text_style;
            vec![ObjectKind::MText(m)]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standards::{new_drawing, Template};

    #[test]
    fn dimensions_get_blocks_that_draw_the_same() {
        let mut d = new_drawing(Template::Metric);
        d.add_entity(Handle::NULL, ObjectKind::Dimension(Dimension::linear(Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(0.0, -15.0), 0.0)))
            .unwrap();
        d.add_entity(Handle::NULL, ObjectKind::Dimension(Dimension::angular(Vec2::ZERO, Vec2::new(50.0, 0.0), Vec2::new(0.0, 50.0), Vec2::new(20.0, 20.0))))
            .unwrap();
        let before = crate::render::model_space(&d);
        let with = with_dimension_blocks(&d);
        assert!(matches!(with, Cow::Owned(_)));
        let dims: Vec<&Dimension> = with.objects.values().filter_map(|o| match &o.kind { ObjectKind::Dimension(x) => Some(x), _ => None }).collect();
        assert!(dims.iter().all(|x| with.block_header(x.block).is_some_and(|b| b.anonymous && b.name.starts_with("*D"))));
        let after = crate::render::model_space(&with);
        assert_eq!(before.items.len(), after.items.len());
        // Written and read back, the blocks are drawn.
        for bytes in [crate::dwg::write::to_bytes(&d).unwrap().bytes, crate::dxf::to_bytes(&d, crate::dxf::DxfFormat::Ascii).bytes] {
            let back = crate::open_bytes(&bytes).unwrap();
            let blocks = back.objects.values().filter(|o| matches!(&o.kind, ObjectKind::BlockHeader(b) if b.anonymous)).count();
            assert_eq!(blocks, 2);
            assert_eq!(crate::render::model_space(&back).items.len(), before.items.len());
        }
        // A drawing whose dimensions all have blocks is written as it is.
        assert!(matches!(with_dimension_blocks(&with), Cow::Borrowed(_)));
    }
}

//! Sample drawings, for demos, previews and tests.

use crate::geom::Vec2;
use crate::model::*;
use crate::standards::{new_drawing, Template};
use std::f64::consts::FRAC_PI_2;

/// A plate drawn to DESIGN.md: 200 × 100 × 10 mm with four ⌀13 holes, in
/// two views (third-angle projection: the top view above the front view),
/// on the default layers: outlines on `VISIBLE`, centre lines on `CENTER`,
/// the holes' hidden edges on `HIDDEN`, dimensions on `DIMENSION` and
/// notes on `NOTES`.
pub fn plate() -> Drawing {
    let mut d = new_drawing(Template::Metric);
    let layer = |d: &Drawing, name: &str| d.layer_by_name(name).map(|(h, _)| h).unwrap_or_default();
    let visible = layer(&d, "VISIBLE");
    let center = layer(&d, "CENTER");
    let hidden = layer(&d, "HIDDEN");
    let dimension = layer(&d, "DIMENSION");
    let notes = layer(&d, "NOTES");
    let add = |d: &mut Drawing, layer: Handle, kind: ObjectKind| {
        // The layers and model space exist: new_drawing made them.
        d.add_entity(layer, kind).unwrap_or_default()
    };
    let line = |a: (f64, f64), b: (f64, f64)| {
        ObjectKind::Line(Line::new(Vec2::new(a.0, a.1), Vec2::new(b.0, b.1)))
    };
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        ObjectKind::LwPolyline(LwPolyline::new(
            vec![
                Vec2::new(x0, y0),
                Vec2::new(x1, y0),
                Vec2::new(x1, y1),
                Vec2::new(x0, y1),
            ],
            true,
        ))
    };
    let (hole_r, past) = (6.5, 3.0);

    // Top view: the plate from above, its bottom edge at y = 40.
    let outline = add(&mut d, visible, rect(0.0, 40.0, 200.0, 140.0));
    for (x, y) in [(15.0, 55.0), (185.0, 55.0), (185.0, 125.0), (15.0, 125.0)] {
        add(
            &mut d,
            visible,
            ObjectKind::Circle(Circle::new(Vec2::new(x, y), hole_r)),
        );
        let r = hole_r + past;
        add(&mut d, center, line((x - r, y), (x + r, y)));
        add(&mut d, center, line((x, y - r), (x, y + r)));
    }

    // Front view: the plate's 10 mm edge, with the holes hidden inside.
    add(&mut d, visible, rect(0.0, 0.0, 200.0, 10.0));
    for x in [15.0, 185.0] {
        add(&mut d, hidden, line((x - hole_r, 0.0), (x - hole_r, 10.0)));
        add(&mut d, hidden, line((x + hole_r, 0.0), (x + hole_r, 10.0)));
        add(&mut d, center, line((x, -past), (x, 10.0 + past)));
    }

    // Dimensions: hole positions chained from the edges, overall sizes
    // outside them, 10 mm apart (DIMDLI).
    let dim = |a: (f64, f64), b: (f64, f64), at: (f64, f64), rot: f64| {
        Dimension::linear(
            Vec2::new(a.0, a.1),
            Vec2::new(b.0, b.1),
            Vec2::new(at.0, at.1),
            rot,
        )
    };
    let edge = Dimension {
        // Too small for its text: set beside it, away from the next one.
        text_midpoint: Vec2::new(-9.0, 150.0),
        ..dim((0.0, 140.0), (15.0, 125.0), (0.0, 150.0), 0.0)
    };
    add(&mut d, dimension, ObjectKind::Dimension(edge));
    add(
        &mut d,
        dimension,
        ObjectKind::Dimension(dim((15.0, 125.0), (185.0, 125.0), (0.0, 150.0), 0.0)),
    );
    add(
        &mut d,
        dimension,
        ObjectKind::Dimension(dim((0.0, 140.0), (200.0, 140.0), (0.0, 160.0), 0.0)),
    );
    let edge = Dimension {
        text_midpoint: Vec2::new(210.0, 31.0),
        ..dim((200.0, 40.0), (185.0, 55.0), (210.0, 0.0), FRAC_PI_2)
    };
    add(&mut d, dimension, ObjectKind::Dimension(edge));
    add(
        &mut d,
        dimension,
        ObjectKind::Dimension(dim((185.0, 55.0), (185.0, 125.0), (210.0, 0.0), FRAC_PI_2)),
    );
    add(
        &mut d,
        dimension,
        // Horizontal text needs more room between vertical dimensions.
        ObjectKind::Dimension(dim((200.0, 40.0), (200.0, 140.0), (228.0, 0.0), FRAC_PI_2)),
    );
    // The thickness, in the front view: too tight for text or arrows.
    add(
        &mut d,
        dimension,
        ObjectKind::Dimension(dim((200.0, 0.0), (200.0, 10.0), (210.0, 0.0), FRAC_PI_2)),
    );
    // The holes: one callout for all four, on a leader from the top-left
    // hole down to text set clear of the plate, on its left.
    add(
        &mut d,
        dimension,
        ObjectKind::Dimension(Dimension {
            user_text: "4X <> THRU".into(),
            text_midpoint: Vec2::new(-29.0, 102.0),
            ..Dimension::diameter(Vec2::new(15.0, 125.0), hole_r, 1.25 * std::f64::consts::PI)
        }),
    );

    // Notes.
    add(
        &mut d,
        notes,
        ObjectKind::Text(Text::new(Vec2::new(0.0, -22.0), 5.0, "PLATE P-1042")),
    );
    add(
        &mut d,
        notes,
        ObjectKind::MText(MText::new(
            Vec2::new(0.0, -29.0),
            3.5,
            120.0,
            "MATERIAL: A36 STEEL, 10 THK\\PBREAK ALL SHARP EDGES 0.5 x 45%%d\\PFINISH: MILL",
            1,
        )),
    );

    // Data another application attached to the outline (XDATA).
    if let Some(o) = d.objects.get_mut(&outline) {
        o.xdata = vec![XData {
            app: XDataApp::Name("DEMO_APP".into()),
            items: vec![
                XValue::Str("part P-1042".into()),
                XValue::Open,
                XValue::Str("qty".into()),
                XValue::Int16(4),
                XValue::Close,
            ],
        }];
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plate_draws_on_every_default_layer() {
        let d = plate();
        let scene = crate::render::model_space(&d);
        assert!(scene.stats.skipped.is_empty(), "{:?}", scene.stats.skipped);
        assert_eq!(scene.stats.failed, 0);
        for name in ["VISIBLE", "CENTER", "HIDDEN", "DIMENSION", "NOTES"] {
            assert!(
                scene.items.iter().any(|i| i.style.layer == name),
                "nothing on {name}"
            );
        }
        let texts: Vec<String> = scene
            .items
            .iter()
            .filter_map(|i| match &i.shape {
                crate::render::Shape::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for t in [
            "200.00",
            "170.00",
            "100.00",
            "70.00",
            "15.00",
            "10.00",
            "4X ⌀13.00 THRU",
            "PLATE P-1042",
        ] {
            assert!(texts.iter().any(|x| x == t), "missing {t:?} in {texts:?}");
        }
        assert!(texts.iter().any(|x| x.ends_with("0.5 x 45°")));
        // It saves and reads back exactly, and draws the same after.
        let back =
            crate::native::read_bytes(&crate::native::to_bytes(&d, &Default::default())).unwrap();
        assert_eq!(back.objects, d.objects);
        assert_eq!(crate::render::model_space(&back), scene);
    }
}

//! A display list for app canvases: the rendered scene in a compact
//! binary form that an app draws itself, at any zoom, with true arcs.
//!
//! Dashes are made here (by [`dash::dash`]) and text becomes strokes of
//! the app's own font, so every canvas draws only lines, arcs, polylines,
//! fills and dots, the same on every platform.
//!
//! # Layout
//!
//! All numbers little-endian.
//!
//! | Bytes | Holds |
//! |---|---|
//! | 4 | `DSPL` |
//! | 4 | layout version, `u32` (1) |
//! | 4 | length of the JSON that follows, `u32` |
//! | n | JSON, UTF-8: `{"bounds":[x0,y0,x1,y1] or null,"styles":[…],"items":N,"skipped":{…}}` |
//! | 0–7 | zeros, to an 8-byte boundary |
//! | rest | `f64` values: a sequence of operations |
//!
//! Each style is `{"screen":"#rrggbb","print":"#rrggbb","px":1,"layer":"…","linetype":"…"}`:
//! the colour on the dark canvas, the colour printed in colour, and the
//! line width in pixels (DESIGN.md §3).
//!
//! Operations, each a code and its values, world coordinates:
//!
//! | Code | Values | Meaning |
//! |---|---|---|
//! | 1 | handle, style | an item starts: the entity it draws (the insert, for block contents) and its style index |
//! | 2 | x1, y1, x2, y2 | a line |
//! | 3 | cx, cy, r, start, end | an arc, counter-clockwise, radians (a full circle is 0 to 2π) |
//! | 4 | n, closed, then n × (x, y) | straight segments |
//! | 5 | rings, then per ring n, n × (x, y) | a fill, even-odd |
//! | 6 | x, y | a dot (point entity) |
//! | 7 | x, y, dx, dy, ray, n, then n dash lengths | a construction line through (x, y) along the unit vector (dx, dy): both ways, or from (x, y) on when `ray` is 1; the app cuts it to its view and dashes it by the pattern (world units: positive dash, negative gap, 0 dot; n is 0 for a solid line) |
//!
//! Handles are below 2^53 in every real drawing, so an `f64` holds them
//! exactly.

use crate::font;
use crate::geom::curve::bulge_to_arc;
use crate::geom::Vec2;
use crate::render::{dash, Scene, Shape};
use crate::standards;
use std::f64::consts::TAU;
use std::fmt::Write;

pub const MAGIC: &[u8; 4] = b"DSPL";
pub const LAYOUT_VERSION: u32 = 1;

pub const OP_ITEM: f64 = 1.0;
pub const OP_LINE: f64 = 2.0;
pub const OP_ARC: f64 = 3.0;
pub const OP_PATH: f64 = 4.0;
pub const OP_FILL: f64 = 5.0;
pub const OP_DOT: f64 = 6.0;
pub const OP_RAY: f64 = 7.0;

/// Text for a JSON string, without the quotes.
pub fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o
}

/// A number as JSON writes it (non-finite numbers become 0).
pub fn json_num(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "0".into()
    }
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[derive(PartialEq)]
struct StyleKey {
    screen: (u8, u8, u8),
    print: (u8, u8, u8),
    px: f64,
    layer: String,
    linetype: String,
}

fn segment(ops: &mut Vec<f64>, p1: Vec2, p2: Vec2, bulge: f64) {
    match bulge_to_arc(p1, p2, bulge) {
        Some((a, _)) => ops.extend_from_slice(&[OP_ARC, a.center.x, a.center.y, a.radius, a.start, a.end]),
        None => ops.extend_from_slice(&[OP_LINE, p1.x, p1.y, p2.x, p2.y]),
    }
}

fn path(ops: &mut Vec<f64>, points: &[Vec2], closed: bool) {
    if points.is_empty() {
        return;
    }
    ops.extend_from_slice(&[OP_PATH, points.len() as f64, if closed { 1.0 } else { 0.0 }]);
    for p in points {
        ops.extend_from_slice(&[p.x, p.y]);
    }
}

/// The operations that stroke one (already dashed) shape.
fn stroke(ops: &mut Vec<f64>, shape: &Shape) {
    match shape {
        Shape::Line { a, b } => ops.extend_from_slice(&[OP_LINE, a.x, a.y, b.x, b.y]),
        Shape::Circle { center, radius } => {
            ops.extend_from_slice(&[OP_ARC, center.x, center.y, *radius, 0.0, TAU])
        }
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => ops.extend_from_slice(&[OP_ARC, center.x, center.y, *radius, *start, *end]),
        Shape::Polyline { vertices, closed } => {
            let n = vertices.len();
            if n == 1 {
                path(ops, &[vertices[0].0], false);
                return;
            }
            if vertices.iter().all(|v| v.1.abs() < 1e-12) {
                let pts: Vec<Vec2> = vertices.iter().map(|v| v.0).collect();
                path(ops, &pts, *closed);
                return;
            }
            let segs = if *closed { n } else { n.saturating_sub(1) };
            for i in 0..segs {
                segment(ops, vertices[i].0, vertices[(i + 1) % n].0, vertices[i].1);
            }
        }
        Shape::Path { points, closed } => path(ops, points, *closed),
        Shape::Lines { segments } => {
            for (a, b) in segments {
                ops.extend_from_slice(&[OP_LINE, a.x, a.y, b.x, b.y]);
            }
        }
        Shape::Fill { .. } | Shape::Point { .. } | Shape::Text { .. } | Shape::Ray { .. } => {}
    }
}

/// The display list of a rendered scene.
pub fn display_list(scene: &Scene) -> Vec<u8> {
    write(scene, None)
}

/// A display list of only the items drawn from `handles` (top-level
/// entities), to patch an app's copy after those entities changed: the
/// app drops what it holds for each of `handles` (some may now draw
/// nothing, having been erased) and adds these items. The JSON adds
/// `"patch":["2A",…]` (the handles, in hex); `bounds`, `items` and
/// `skipped` are the whole scene's, so the app's copy stays whole.
pub fn display_patch(scene: &Scene, handles: &[crate::model::Handle]) -> Vec<u8> {
    write(scene, Some(handles))
}

fn write(scene: &Scene, only: Option<&[crate::model::Handle]>) -> Vec<u8> {
    let set: Option<std::collections::BTreeSet<_>> = only.map(|h| h.iter().copied().collect());
    let mut styles: Vec<StyleKey> = Vec::new();
    let mut ops: Vec<f64> = Vec::new();
    for it in &scene.items {
        if set.as_ref().is_some_and(|s| !s.contains(&it.source)) {
            continue;
        }
        let key = StyleKey {
            screen: standards::screen_color(it.style.color),
            print: standards::print_color(it.style.color, true),
            px: standards::screen_px(it.style.weight_mm()),
            layer: it.style.layer.clone(),
            linetype: it.style.linetype.clone(),
        };
        let si = match styles.iter().position(|s| *s == key) {
            Some(i) => i,
            None => {
                styles.push(key);
                styles.len() - 1
            }
        };
        ops.extend_from_slice(&[OP_ITEM, it.source.0 as f64, si as f64]);
        match &it.shape {
            Shape::Fill { rings } => {
                ops.extend_from_slice(&[OP_FILL, rings.len() as f64]);
                for r in rings {
                    ops.push(r.len() as f64);
                    for p in r {
                        ops.extend_from_slice(&[p.x, p.y]);
                    }
                }
            }
            Shape::Point { at } => ops.extend_from_slice(&[OP_DOT, at.x, at.y]),
            Shape::Ray {
                origin,
                direction,
                ray,
            } => {
                let pattern = if dash::is_dashed(&it.style.pattern) { it.style.pattern.as_slice() } else { &[] };
                ops.extend_from_slice(&[
                    OP_RAY,
                    origin.x,
                    origin.y,
                    direction.x,
                    direction.y,
                    if *ray { 1.0 } else { 0.0 },
                    pattern.len() as f64,
                ]);
                ops.extend_from_slice(pattern);
            }
            Shape::Text { text, .. } => {
                if let Some(p) = it.shape.placement() {
                    for s in font::layout(text, &p) {
                        path(&mut ops, &s, false);
                    }
                }
            }
            shape => {
                for piece in dash::dash(shape, &it.style.pattern) {
                    stroke(&mut ops, &piece);
                }
            }
        }
    }

    let mut json = String::from("{\"bounds\":");
    if scene.bounds.is_empty() {
        json.push_str("null");
    } else {
        let b = scene.bounds;
        let _ = write!(
            json,
            "[{},{},{},{}]",
            json_num(b.min.x),
            json_num(b.min.y),
            json_num(b.max.x),
            json_num(b.max.y)
        );
    }
    json.push_str(",\"styles\":[");
    for (i, s) in styles.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(
            json,
            "{{\"screen\":\"{}\",\"print\":\"{}\",\"px\":{},\"layer\":\"{}\",\"linetype\":\"{}\"}}",
            hex(s.screen),
            hex(s.print),
            json_num(s.px),
            json_escape(&s.layer),
            json_escape(&s.linetype)
        );
    }
    let _ = write!(json, "],\"items\":{},\"skipped\":{{", scene.items.len());
    for (i, (k, v)) in scene.stats.skipped.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(json, "\"{}\":{v}", json_escape(k));
    }
    json.push('}');
    if let Some(hs) = only {
        json.push_str(",\"patch\":[");
        for (i, h) in hs.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            let _ = write!(json, "\"{:X}\"", h.0);
        }
        json.push(']');
    }
    json.push('}');

    let mut out = Vec::with_capacity(16 + json.len() + ops.len() * 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&LAYOUT_VERSION.to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    while out.len() % 8 != 0 {
        out.push(0);
    }
    for v in ops {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, Handle, Lineweight};
    use crate::render::{Item, Style};

    fn item(shape: Shape, pattern: Vec<f64>) -> Item {
        Item {
            shape,
            style: Style {
                rgb: (255, 255, 255),
                aci: 7,
                color: Color::Index(7),
                layer: "VISIBLE".into(),
                lineweight: Lineweight::Hundredths(50),
                linetype: if pattern.is_empty() { String::new() } else { "HIDDEN".into() },
                pattern,
            },
            source: Handle(0x2A),
        }
    }

    fn parse(bytes: &[u8]) -> (String, Vec<f64>) {
        assert_eq!(&bytes[..4], MAGIC);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1);
        let n = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let json = String::from_utf8(bytes[12..12 + n].to_vec()).unwrap();
        let start = (12 + n).div_ceil(8) * 8;
        assert_eq!((bytes.len() - start) % 8, 0);
        let ops = bytes[start..]
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
            .collect();
        (json, ops)
    }

    #[test]
    fn lines_arcs_and_styles() {
        let mut scene = Scene::default();
        scene.items.push(item(
            Shape::Line {
                a: Vec2::new(0.0, 0.0),
                b: Vec2::new(10.0, 0.0),
            },
            vec![],
        ));
        scene.items.push(item(
            Shape::Circle {
                center: Vec2::new(5.0, 5.0),
                radius: 2.0,
            },
            vec![],
        ));
        scene.bounds.add(Vec2::new(0.0, 0.0));
        scene.bounds.add(Vec2::new(10.0, 7.0));
        let (json, ops) = parse(&display_list(&scene));
        assert_eq!(
            json,
            "{\"bounds\":[0,0,10,7],\"styles\":[{\"screen\":\"#ffffff\",\"print\":\"#000000\",\"px\":2,\"layer\":\"VISIBLE\",\"linetype\":\"\"}],\"items\":2,\"skipped\":{}}"
        );
        assert_eq!(
            ops,
            vec![1.0, 42.0, 0.0, 2.0, 0.0, 0.0, 10.0, 0.0, 1.0, 42.0, 0.0, 3.0, 5.0, 5.0, 2.0, 0.0, TAU]
        );
    }

    #[test]
    fn dashed_lines_come_out_in_pieces() {
        let mut scene = Scene::default();
        scene.items.push(item(
            Shape::Line {
                a: Vec2::new(0.0, 0.0),
                b: Vec2::new(100.0, 0.0),
            },
            vec![5.0, -2.5],
        ));
        let (_, ops) = parse(&display_list(&scene));
        let lines = ops.iter().filter(|v| **v == OP_LINE).count();
        assert!(lines > 5, "{lines} pieces");
    }

    #[test]
    fn bulges_become_arcs_and_text_becomes_strokes() {
        let mut scene = Scene::default();
        scene.items.push(item(
            Shape::Polyline {
                vertices: vec![(Vec2::new(0.0, 0.0), 1.0), (Vec2::new(10.0, 0.0), 0.0)],
                closed: false,
            },
            vec![],
        ));
        let (_, ops) = parse(&display_list(&scene));
        assert_eq!(ops[3], OP_ARC);
        assert!((ops[6] - 5.0).abs() < 1e-9, "radius {}", ops[6]);

        let mut scene = Scene::default();
        scene.items.push(item(
            Shape::Text {
                position: Vec2::new(0.0, 0.0),
                height: 2.5,
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                text: "A1".into(),
                h_align: 0,
                v_align: 0,
                fit_to: None,
            },
            vec![],
        ));
        let (_, ops) = parse(&display_list(&scene));
        assert_eq!(ops[3], OP_PATH);
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_escape("a\"b\\c\n"), "a\\\"b\\\\c\\u000a");
    }
}

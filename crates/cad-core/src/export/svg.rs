//! SVG export of a rendered scene (plan view, world units).
//!
//! Drawn the way the app draws the canvas (DESIGN.md §2–§4): on the dark
//! background with the softened screen colours, or black on white paper;
//! lines 1, 2 or 3 pixels wide by lineweight, at any zoom; linetype dashes
//! made by the core; text in the app's own single-stroke font, so it looks
//! the same in every browser and needs no fonts installed.

use crate::font;
use crate::geom::curve::{bulge_to_arc, normalize_sweep};
use crate::geom::Vec2;
use crate::render::{dash, Scene, Shape, Style};
use crate::standards::{self, canvas};
use std::f64::consts::PI;
use std::fmt::Write;

#[derive(Debug, Clone)]
pub struct SvgOptions {
    /// The dark drafting canvas with screen colours; `false` for paper:
    /// black on white.
    pub dark_background: bool,
    /// On paper, print colours (white still prints black) instead of all
    /// black.
    pub paper_color: bool,
    /// Width of the SVG in pixels; the height follows the drawing's aspect.
    pub width_px: f64,
    /// Draw text as strokes of the app's font. `false` writes SVG text
    /// elements instead, which a viewer can search but draws in its own
    /// font.
    pub stroke_text: bool,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions {
            dark_background: true,
            paper_color: false,
            width_px: 1600.0,
            stroke_text: true,
        }
    }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' => o.push(' '),
            // Not characters XML allows at all.
            '\u{FFFE}' | '\u{FFFF}' => o.push('\u{FFFD}'),
            c => o.push(c),
        }
    }
    o
}

fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn arc_cmd(d: &mut String, center: Vec2, radius: f64, start: f64, end: f64) {
    let sweep = normalize_sweep(end - start);
    let p0 = center + Vec2::from_angle(start) * radius;
    let p1 = center + Vec2::from_angle(end) * radius;
    if (sweep - 2.0 * PI).abs() < 1e-9 {
        // Full turn: two halves.
        let pm = center + Vec2::from_angle(start + PI) * radius;
        let _ = write!(
            d,
            "M{} {}A{r} {r} 0 0 1 {} {}A{r} {r} 0 0 1 {} {}",
            num(p0.x),
            num(p0.y),
            num(pm.x),
            num(pm.y),
            num(p0.x),
            num(p0.y),
            r = num(radius)
        );
        return;
    }
    let large = if sweep > PI { 1 } else { 0 };
    let _ = write!(
        d,
        "M{} {}A{r} {r} 0 {large} 1 {} {}",
        num(p0.x),
        num(p0.y),
        num(p1.x),
        num(p1.y),
        r = num(radius)
    );
}

fn polyline_cmd(d: &mut String, points: &[Vec2], closed: bool) {
    let Some(first) = points.first() else {
        return;
    };
    let _ = write!(d, "M{} {}", num(first.x), num(first.y));
    for p in &points[1..] {
        let _ = write!(d, "L{} {}", num(p.x), num(p.y));
    }
    if points.len() == 1 {
        // A dot: a zero-length segment, drawn with round caps.
        let _ = write!(d, "L{} {}", num(first.x), num(first.y));
    }
    if closed {
        d.push('Z');
    }
}

/// Path data for the outline of a stroked shape (after dashing).
fn stroke_path(d: &mut String, shape: &Shape) {
    match shape {
        Shape::Line { a, b } => polyline_cmd(d, &[*a, *b], false),
        Shape::Circle { center, radius } => arc_cmd(d, *center, *radius, 0.0, 2.0 * PI),
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => arc_cmd(d, *center, *radius, *start, *end),
        Shape::Polyline { vertices, closed } => {
            if vertices.is_empty() {
                return;
            }
            let _ = write!(d, "M{} {}", num(vertices[0].0.x), num(vertices[0].0.y));
            let n = vertices.len();
            let segs = if *closed { n } else { n - 1 };
            for i in 0..segs {
                let (p1, bulge) = vertices[i];
                let p2 = vertices[(i + 1) % n].0;
                match bulge_to_arc(p1, p2, bulge) {
                    None => {
                        let _ = write!(d, "L{} {}", num(p2.x), num(p2.y));
                    }
                    Some((arc, _)) => {
                        let large = if arc.sweep() > PI { 1 } else { 0 };
                        let sweep_flag = if bulge > 0.0 { 1 } else { 0 };
                        let _ = write!(
                            d,
                            "A{r} {r} 0 {large} {sweep_flag} {} {}",
                            num(p2.x),
                            num(p2.y),
                            r = num(arc.radius)
                        );
                    }
                }
            }
            if *closed {
                d.push('Z');
            }
        }
        Shape::Path { points, closed } => polyline_cmd(d, points, *closed),
        Shape::Lines { segments } => {
            for (a, b) in segments {
                polyline_cmd(d, &[*a, *b], false);
            }
        }
        // Cut to the view first (see `to_svg`).
        Shape::Fill { .. } | Shape::Point { .. } | Shape::Text { .. } | Shape::Ray { .. } => {}
    }
}

/// Render a scene to an SVG document string.
pub fn to_svg(scene: &Scene, opt: &SvgOptions) -> String {
    let b = if scene.bounds.is_empty() {
        crate::geom::BBox {
            min: Vec2::new(0.0, 0.0),
            max: Vec2::new(1.0, 1.0),
        }
    } else {
        scene.bounds
    };
    let w = b.width().max(1e-9);
    let h = b.height().max(1e-9);
    let margin = 0.03 * w.max(h);
    let (x0, y0) = (b.min.x - margin, b.min.y - margin);
    let (vw, vh) = (w + 2.0 * margin, h + 2.0 * margin);
    let px_w = opt.width_px.max(16.0);
    let px_h = (px_w * vh / vw).clamp(16.0, 100_000.0);
    let bg = if opt.dark_background {
        canvas::BACKGROUND
    } else {
        canvas::PAPER
    };
    // A point is drawn as a dot about 2 pixels across.
    let point_r = vw / px_w;

    let mut out = String::new();
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}">"#,
        num(px_w),
        num(px_h),
        num(x0),
        num(-(y0 + vh)),
        num(vw),
        num(vh)
    );
    let _ = writeln!(
        out,
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
        num(x0),
        num(-(y0 + vh)),
        num(vw),
        num(vh),
        hex(bg)
    );
    // World coordinates are y-up: flip once for the whole drawing.
    let _ = writeln!(
        out,
        r#"<g transform="scale(1,-1)" fill="none" stroke-linecap="round" stroke-linejoin="round">"#
    );

    let color_of = |s: &Style| {
        if opt.dark_background {
            standards::screen_color(s.color)
        } else {
            standards::print_color(s.color, opt.paper_color)
        }
    };
    for it in &scene.items {
        let color = hex(color_of(&it.style));
        let px = standards::screen_px(it.style.weight_mm());
        let stroke =
            format!(r#"stroke="{color}" stroke-width="{px}" vector-effect="non-scaling-stroke""#);
        match &it.shape {
            Shape::Fill { rings } => {
                let mut d = String::new();
                for ring in rings {
                    polyline_cmd(&mut d, ring, true);
                }
                let _ = writeln!(
                    out,
                    r#"<path d="{d}" fill="{color}" fill-rule="evenodd" stroke="none"/>"#
                );
            }
            Shape::Point { at } => {
                let _ = writeln!(
                    out,
                    r#"<circle cx="{}" cy="{}" r="{}" fill="{color}" stroke="none"/>"#,
                    num(at.x),
                    num(at.y),
                    num(point_r)
                );
            }
            Shape::Text {
                position,
                height,
                rotation,
                width_factor,
                text,
                h_align,
                v_align,
                ..
            } => {
                if opt.stroke_text {
                    let Some(p) = it.shape.placement() else {
                        continue;
                    };
                    let mut d = String::new();
                    for s in font::layout(text, &p) {
                        polyline_cmd(&mut d, &s, false);
                    }
                    if !d.is_empty() {
                        let _ = writeln!(out, r#"<path d="{d}" {stroke}/>"#);
                    }
                } else {
                    // CAD text height is cap height; SVG font-size is the
                    // em box.
                    let size = height / 0.7;
                    let anchor = match h_align {
                        1 | 4 => "middle",
                        2 => "end",
                        _ => "start",
                    };
                    let baseline = match v_align {
                        2 => "central",
                        3 => "hanging",
                        _ => "alphabetic",
                    };
                    let _ = writeln!(
                        out,
                        r#"<text transform="translate({} {}) rotate({}) scale({},-1)" font-family="sans-serif" font-size="{}" fill="{color}" stroke="none" text-anchor="{anchor}" dominant-baseline="{baseline}">{}</text>"#,
                        num(position.x),
                        num(position.y),
                        num(rotation.to_degrees()),
                        num(*width_factor),
                        num(size),
                        esc(text)
                    );
                }
            }
            shape => {
                // A construction line is cut to the picture first.
                let cut;
                let shape = match shape {
                    Shape::Ray { origin, direction, ray } => {
                        let mut view = crate::geom::BBox::EMPTY;
                        view.add(Vec2::new(x0, y0));
                        view.add(Vec2::new(x0 + vw, y0 + vh));
                        match crate::render::clip::clip_ray(*origin, *direction, *ray, &view) {
                            Some((a, b)) => {
                                cut = Shape::Line { a, b };
                                &cut
                            }
                            None => continue,
                        }
                    }
                    s => s,
                };
                let mut d = String::new();
                for piece in dash::dash(shape, &it.style.pattern) {
                    stroke_path(&mut d, &piece);
                }
                if !d.is_empty() {
                    let _ = writeln!(out, r#"<path d="{d}" {stroke}/>"#);
                }
            }
        }
    }
    let _ = writeln!(out, "</g>\n</svg>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Vec2;
    use crate::model::{Color, Lineweight};
    use crate::render::{Item, Scene};

    fn item(shape: Shape, color: Color, mm: u16, pattern: Vec<f64>) -> Item {
        Item {
            shape,
            style: Style {
                rgb: color.resolve(Color::Index(7), Color::Index(7)),
                aci: color.to_aci(),
                color,
                layer: "L".into(),
                lineweight: Lineweight::Hundredths(mm),
                linetype: if pattern.is_empty() {
                    String::new()
                } else {
                    "HIDDEN".into()
                },
                pattern,
            },
            source: Default::default(),
        }
    }

    #[test]
    fn colours_weights_dashes_and_text() {
        let mut scene = Scene::default();
        scene.items.push(item(
            Shape::Line {
                a: Vec2::ZERO,
                b: Vec2::new(100.0, 0.0),
            },
            Color::Index(7),
            50,
            vec![],
        ));
        scene.items.push(item(
            Shape::Line {
                a: Vec2::new(0.0, 10.0),
                b: Vec2::new(100.0, 10.0),
            },
            Color::Index(1),
            25,
            vec![3.0, -1.5],
        ));
        scene.items.push(item(
            Shape::Text {
                position: Vec2::new(0.0, 20.0),
                height: 3.5,
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                text: "A1".into(),
                h_align: 0,
                v_align: 0,
                fit_to: None,
            },
            Color::Index(4),
            25,
            vec![],
        ));
        for it in &scene.items {
            crate::render::shape_bounds(&it.shape, &mut scene.bounds);
        }
        let svg = to_svg(&scene, &SvgOptions::default());
        assert!(svg.contains(r##"fill="#0d1013""##), "design background");
        assert!(
            svg.contains(r##"stroke="#ffffff" stroke-width="2""##),
            "thick white"
        );
        assert!(
            svg.contains(r##"stroke="#ff4d4d" stroke-width="1""##),
            "softened red, thin"
        );
        assert!(svg.contains(r##"stroke="#3fe0ff""##), "cyan text strokes");
        assert!(!svg.contains("<text"), "text drawn as strokes");
        // The hidden line comes out as many dash segments in one path.
        let dashed = svg.lines().find(|l| l.contains("#ff4d4d")).unwrap();
        assert!(dashed.matches('M').count() > 10);

        let paper = to_svg(
            &scene,
            &SvgOptions {
                dark_background: false,
                paper_color: true,
                stroke_text: false,
                ..Default::default()
            },
        );
        assert!(paper.contains(r##"fill="#ffffff""##));
        assert!(
            paper.contains(r##"stroke="#000000""##),
            "white prints black"
        );
        assert!(paper.contains(r##"stroke="#ff0000""##), "true red on paper");
        assert!(paper.contains("<text"));
        let mono = to_svg(
            &scene,
            &SvgOptions {
                dark_background: false,
                ..Default::default()
            },
        );
        assert!(
            !mono.contains("#ff0000") && !mono.contains("#3fe0ff"),
            "all black"
        );
    }

    #[test]
    fn text_escapes_to_characters_xml_allows() {
        assert_eq!(
            esc("a<b>&\"c\u{1}\u{FFFE}\u{FFFF}\td"),
            "a&lt;b&gt;&amp;&quot;c \u{FFFD}\u{FFFD}\td"
        );
    }
}

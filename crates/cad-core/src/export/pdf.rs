//! PDF export of a rendered sheet: what PLOT and PRINT write.
//!
//! One page of vector drawing, PDF 1.4: lines and Bézier arcs, filled
//! areas, and text drawn as strokes of the app's own single-stroke font,
//! so the file needs no fonts and looks the same in every viewer. Colours
//! print as DESIGN.md §2 has them (white and colour 7 black, or everything
//! black in monochrome); lineweights print at their size in millimetres.
//! The page content is compressed with the core's own deflate.
//!
//! A layout plots 1:1 (paper units onto the paper); model space plots to
//! fit the paper, centred, with a margin.

use crate::codec::{self, Level};
use crate::font;
use crate::geom::curve::{bulge_to_arc, normalize_sweep};
use crate::geom::{BBox, Vec2};
use crate::render::{dash, Scene, Shape};
use crate::standards;
use std::f64::consts::TAU;
use std::fmt::Write;

/// Points per millimetre.
const PT: f64 = 72.0 / 25.4;

/// Where the drawing goes on the paper.
#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    /// `mm_per_unit` millimetres of paper per drawing unit, the drawing's
    /// `origin` at the paper's lower-left corner: a layout at 1:1, or a
    /// model plotted to scale.
    Scale { mm_per_unit: f64, origin: Vec2 },
    /// `window` (the scene's extents when none) as large as the paper
    /// allows inside `margin_mm`, centred.
    Fit { window: Option<BBox>, margin_mm: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PdfOptions {
    /// Paper width and height in millimetres.
    pub paper_mm: (f64, f64),
    /// Every line black.
    pub monochrome: bool,
    /// Lines as thick as their lineweight; otherwise all 0.25 mm.
    pub lineweights: bool,
    /// The document title.
    pub title: String,
}

impl Default for PdfOptions {
    fn default() -> Self {
        PdfOptions {
            paper_mm: (420.0, 297.0),
            monochrome: true,
            lineweights: true,
            title: String::new(),
        }
    }
}

fn n(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

/// Drawing coordinates to page points.
#[derive(Clone, Copy)]
struct Map {
    k: f64,
    origin: Vec2,
    offset: Vec2,
}

impl Map {
    fn p(&self, q: Vec2) -> Vec2 {
        (q - self.origin) * self.k + self.offset
    }
}

struct Page {
    out: String,
    map: Map,
}

impl Page {
    fn mv(&mut self, p: Vec2) {
        let q = self.map.p(p);
        let _ = write!(self.out, "{} {} m ", n(q.x), n(q.y));
    }
    fn ln(&mut self, p: Vec2) {
        let q = self.map.p(p);
        let _ = write!(self.out, "{} {} l ", n(q.x), n(q.y));
    }
    /// An arc counter-clockwise from `a0` through `sweep` (negative:
    /// clockwise), as Bézier curves of at most a quarter turn; the pen is
    /// at its start.
    fn arc(&mut self, c: Vec2, r: f64, a0: f64, sweep: f64) {
        let parts = (sweep.abs() / (TAU / 4.0)).ceil().max(1.0) as usize;
        let step = sweep / parts as f64;
        let kk = 4.0 / 3.0 * (step / 4.0).tan();
        for i in 0..parts {
            let s = a0 + step * i as f64;
            let e = s + step;
            let (p0, p3) = (c + Vec2::from_angle(s) * r, c + Vec2::from_angle(e) * r);
            let p1 = p0 + Vec2::from_angle(s).perp() * (r * kk);
            let p2 = p3 - Vec2::from_angle(e).perp() * (r * kk);
            let (q1, q2, q3) = (self.map.p(p1), self.map.p(p2), self.map.p(p3));
            let _ = write!(self.out, "{} {} {} {} {} {} c ", n(q1.x), n(q1.y), n(q2.x), n(q2.y), n(q3.x), n(q3.y));
        }
    }
    fn polyline(&mut self, pts: &[Vec2], closed: bool) {
        let Some(first) = pts.first() else { return };
        self.mv(*first);
        for p in &pts[1..] {
            self.ln(*p);
        }
        if pts.len() == 1 {
            self.ln(*first);
        }
        if closed {
            self.out.push_str("h ");
        }
    }
    /// The outline of a shape as path operators (not yet stroked).
    fn path(&mut self, shape: &Shape) {
        match shape {
            Shape::Line { a, b } => {
                self.mv(*a);
                self.ln(*b);
            }
            Shape::Circle { center, radius } => {
                self.mv(*center + Vec2::new(*radius, 0.0));
                self.arc(*center, *radius, 0.0, TAU);
                self.out.push_str("h ");
            }
            Shape::Arc { center, radius, start, end } => {
                self.mv(*center + Vec2::from_angle(*start) * *radius);
                self.arc(*center, *radius, *start, normalize_sweep(end - start));
            }
            Shape::Polyline { vertices, closed } => {
                let cnt = vertices.len();
                if cnt == 0 {
                    return;
                }
                self.mv(vertices[0].0);
                let segs = if *closed { cnt } else { cnt - 1 };
                for i in 0..segs {
                    let (p, b) = vertices[i];
                    let q = vertices[(i + 1) % cnt].0;
                    match bulge_to_arc(p, q, b) {
                        Some((arc, _)) => {
                            let sw = 4.0 * b.atan();
                            let a0 = (p - arc.center).angle();
                            self.arc(arc.center, arc.radius, a0, sw);
                        }
                        None => self.ln(q),
                    }
                }
                if *closed {
                    self.out.push_str("h ");
                }
            }
            Shape::Path { points, closed } => self.polyline(points, *closed),
            Shape::Lines { segments } => {
                for (a, b) in segments {
                    self.mv(*a);
                    self.ln(*b);
                }
            }
            Shape::Fill { .. } | Shape::Point { .. } | Shape::Text { .. } | Shape::Ray { .. } => {}
        }
    }
}

/// The scene as a one-page PDF.
pub fn to_pdf(scene: &Scene, place: &Placement, opt: &PdfOptions) -> Vec<u8> {
    let (pw, ph) = (opt.paper_mm.0.max(1.0), opt.paper_mm.1.max(1.0));
    let (wpt, hpt) = (pw * PT, ph * PT);
    let map = match place {
        Placement::Scale { mm_per_unit, origin } => Map {
            k: mm_per_unit * PT,
            origin: *origin,
            offset: Vec2::ZERO,
        },
        Placement::Fit { window, margin_mm } => {
            let w = window.unwrap_or(scene.bounds);
            let w = if w.is_empty() {
                BBox {
                    min: Vec2::ZERO,
                    max: Vec2::new(1.0, 1.0),
                }
            } else {
                w
            };
            let m = margin_mm.max(0.0).min(pw.min(ph) / 3.0);
            let (aw, ah) = ((pw - 2.0 * m) * PT, (ph - 2.0 * m) * PT);
            let k = (aw / w.width().max(1e-9)).min(ah / w.height().max(1e-9));
            let used = Vec2::new(w.width() * k, w.height() * k);
            Map {
                k,
                origin: w.min,
                offset: Vec2::new((wpt - used.x) / 2.0, (hpt - used.y) / 2.0),
            }
        }
    };
    // The page in drawing coordinates, to cut construction lines to.
    let view = BBox {
        min: (Vec2::ZERO - map.offset) * (1.0 / map.k) + map.origin,
        max: (Vec2::new(wpt, hpt) - map.offset) * (1.0 / map.k) + map.origin,
    };
    let mut page = Page {
        out: String::new(),
        map,
    };
    let _ = write!(page.out, "0 0 {} {} re W n 1 J 1 j ", n(wpt), n(hpt));
    let mut state: Option<((u8, u8, u8), f64)> = None;
    for it in &scene.items {
        let rgb = standards::print_color(it.style.color, !opt.monochrome);
        let mm = if opt.lineweights { it.style.weight_mm() } else { 0.25 };
        let width = (mm * PT).max(0.1);
        if state != Some((rgb, width)) {
            let (r, g, b) = (rgb.0 as f64 / 255.0, rgb.1 as f64 / 255.0, rgb.2 as f64 / 255.0);
            let _ = write!(page.out, "\n{} {} {} RG {} {} {} rg {} w ", n(r), n(g), n(b), n(r), n(g), n(b), n(width));
            state = Some((rgb, width));
        }
        match &it.shape {
            Shape::Fill { rings } => {
                for ring in rings {
                    page.polyline(ring, true);
                }
                page.out.push_str("f* ");
            }
            Shape::Point { at } => {
                // A dot: a line of no length with round caps.
                page.mv(*at);
                page.ln(*at);
                page.out.push_str("S ");
            }
            Shape::Text { text, .. } => {
                let Some(p) = it.shape.placement() else { continue };
                let strokes = font::layout(text, &p);
                if strokes.is_empty() {
                    continue;
                }
                for s in strokes {
                    page.polyline(&s, false);
                }
                page.out.push_str("S ");
            }
            Shape::Ray { origin, direction, ray } => {
                if let Some((a, b)) = crate::render::clip::clip_ray(*origin, *direction, *ray, &view) {
                    for piece in dash::dash(&Shape::Line { a, b }, &it.style.pattern) {
                        page.path(&piece);
                    }
                    page.out.push_str("S ");
                }
            }
            shape => {
                for piece in dash::dash(shape, &it.style.pattern) {
                    page.path(&piece);
                }
                page.out.push_str("S ");
            }
        }
    }
    document(page.out.as_bytes(), wpt, hpt, &opt.title)
}

/// A PDF text string: UTF-16 with a byte-order mark, in hex.
fn text_string(s: &str) -> String {
    let mut out = String::from("<FEFF");
    for u in s.encode_utf16() {
        let _ = write!(out, "{u:04X}");
    }
    out.push('>');
    out
}

/// Days since 1970 to (year, month, day), proleptic Gregorian.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn pdf_date(unix: i64) -> String {
    let (y, m, d) = civil(unix.div_euclid(86_400));
    let s = unix.rem_euclid(86_400);
    format!("D:{y:04}{m:02}{d:02}{:02}{:02}{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

/// The file: catalog, page tree, the page, its compressed content and
/// the document information, with the cross-reference table.
fn document(content: &[u8], wpt: f64, hpt: f64, title: &str) -> Vec<u8> {
    let stream = codec::zlib(content, Level::Default);
    let product = format!("{} {}", crate::PRODUCT_NAME, crate::VERSION);
    let date = pdf_date(crate::now_unix());
    let mut info = format!("<< /Producer {} /Creator {} /CreationDate ({date})", text_string(&product), text_string(crate::PRODUCT_NAME));
    if !title.is_empty() {
        let _ = write!(info, " /Title {}", text_string(title));
    }
    info.push_str(" >>");
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R /Resources << >> >>", n(wpt), n(hpt)).into_bytes(),
        {
            let mut v = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", stream.len()).into_bytes();
            v.extend_from_slice(&stream);
            v.extend_from_slice(b"\nendstream");
            v
        },
        info.into_bytes(),
    ];
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, Handle, Lineweight};
    use crate::render::{Item, Style};

    fn item(shape: Shape) -> Item {
        Item {
            shape,
            style: Style {
                rgb: (255, 255, 255),
                aci: 7,
                color: Color::Index(7),
                layer: "0".into(),
                lineweight: Lineweight::Hundredths(50),
                linetype: String::new(),
                pattern: Vec::new(),
            },
            source: Handle(1),
        }
    }

    #[test]
    fn a_page_of_vectors() {
        let mut scene = Scene::default();
        scene.items.push(item(Shape::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(100.0, 0.0) }));
        scene.items.push(item(Shape::Circle { center: Vec2::new(50.0, 50.0), radius: 20.0 }));
        scene.items.push(item(Shape::Text {
            position: Vec2::new(10.0, 10.0),
            height: 5.0,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            text: "A-1".into(),
            h_align: 0,
            v_align: 0,
            fit_to: None,
        }));
        scene.bounds = BBox { min: Vec2::ZERO, max: Vec2::new(100.0, 70.0) };
        let pdf = to_pdf(&scene, &Placement::Scale { mm_per_unit: 1.0, origin: Vec2::ZERO }, &PdfOptions::default());
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/MediaBox [0 0 1190.551 841.89]"));
        assert!(text.trim_end().ends_with("%%EOF"));
        // The content inflates back to drawing operators.
        let at = pdf.windows(7).position(|w| w == b"stream\n").unwrap() + 7;
        let len: usize = text.split("/Length ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
        let content = codec::unzlib(&pdf[at..at + len], 1 << 20).unwrap();
        let ops = String::from_utf8(content).unwrap();
        assert!(ops.contains("0 0 m 283.465 0 l S"), "{ops}");
        assert!(ops.contains(" c "));
        assert!(ops.contains("1.417 w"));
        // The cross-reference table points at each object.
        let xref: usize = text.split("startxref\n").nth(1).unwrap().lines().next().unwrap().parse().unwrap();
        assert!(pdf[xref..].starts_with(b"xref"));
        let fit = to_pdf(&scene, &Placement::Fit { window: None, margin_mm: 10.0 }, &PdfOptions { paper_mm: (297.0, 210.0), ..Default::default() });
        assert!(String::from_utf8_lossy(&fit).contains("/MediaBox [0 0 841.89 595.276]"));
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_738), (2026, 10, 12));
    }
}

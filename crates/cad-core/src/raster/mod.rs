//! Software rasteriser: draws a scene the way the app's canvas looks
//! (DESIGN.md §2–§4) into an image, using every core of the machine.
//!
//! The dark canvas with its grid, screen colours, lines 1, 2 or 3 pixels
//! wide by lineweight at any zoom, the core's linetype dashes, and text in
//! the app's own font; or black on white paper. Lines and fills are
//! anti-aliased. The image is split into horizontal bands rendered in
//! parallel from one display list; the result is the same whatever the
//! number of threads.
//!
//! It serves screenshots, previews and tests now, and is the fallback
//! renderer for platforms without a GPU path later.

pub mod png;

use crate::font;
use crate::geom::curve::bulge_to_arc;
use crate::geom::{BBox, Vec2};
use crate::render::{dash, Scene, Shape};
use crate::standards::{self, canvas, Rgb};
use std::f64::consts::TAU;

/// Largest width or height rendered, in pixels.
pub const MAX_SIZE: u32 = 16_384;

/// An RGBA image, 8 bits per channel, rows from the top.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// An image filled with one colour.
    pub fn new(width: u32, height: u32, fill: Rgb) -> Image {
        let n = width as usize * height as usize;
        let mut rgba = Vec::with_capacity(n * 4);
        for _ in 0..n {
            rgba.extend_from_slice(&[fill.0, fill.1, fill.2, 255]);
        }
        Image {
            width,
            height,
            rgba,
        }
    }

    /// The pixel at (x, y), or `None` outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ])
    }

    /// The image as a PNG file.
    pub fn to_png(&self) -> Vec<u8> {
        png::encode(self)
    }
}

/// What part of the drawing the image shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Drawing point at the centre of the image.
    pub center: Vec2,
    /// Pixels per drawing unit.
    pub scale: f64,
}

impl View {
    /// The view that fits `bounds` into the image with a margin.
    pub fn fit(bounds: &BBox, width: u32, height: u32, margin_px: f64) -> View {
        if bounds.is_empty() {
            return View {
                center: Vec2::ZERO,
                scale: 1.0,
            };
        }
        let aw = (width as f64 - 2.0 * margin_px).max(1.0);
        let ah = (height as f64 - 2.0 * margin_px).max(1.0);
        let sx = aw / bounds.width().max(1e-9);
        let sy = ah / bounds.height().max(1e-9);
        let scale = sx.min(sy);
        View {
            center: bounds.center(),
            scale: if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            },
        }
    }

    /// Pixel position (x right, y down) of a drawing point.
    pub fn to_px(&self, p: Vec2, width: u32, height: u32) -> Vec2 {
        Vec2::new(
            width as f64 / 2.0 + (p.x - self.center.x) * self.scale,
            height as f64 / 2.0 - (p.y - self.center.y) * self.scale,
        )
    }

    /// Drawing point under a pixel position.
    pub fn to_world(&self, px: Vec2, width: u32, height: u32) -> Vec2 {
        Vec2::new(
            self.center.x + (px.x - width as f64 / 2.0) / self.scale,
            self.center.y - (px.y - height as f64 / 2.0) / self.scale,
        )
    }
}

#[derive(Debug, Clone)]
pub struct RasterOptions {
    pub width: u32,
    pub height: u32,
    /// `None`: fit the whole drawing.
    pub view: Option<View>,
    /// The dark canvas with screen colours; `false` for black on white
    /// paper.
    pub dark: bool,
    /// On paper, print colours (white still prints black) instead of all
    /// black.
    pub paper_color: bool,
    /// Draw the grid (dark canvas only; the grid never prints).
    pub grid: bool,
    /// Draw the X/Y axis icon in the bottom-left corner.
    pub axis_icon: bool,
    /// Device pixels per screen pixel (2 on most phones), which scales
    /// line widths.
    pub pixel_ratio: f64,
    /// Threads to use; 0 for one per core.
    pub threads: usize,
}

impl Default for RasterOptions {
    fn default() -> Self {
        RasterOptions {
            width: 1600,
            height: 1000,
            view: None,
            dark: true,
            paper_color: false,
            grid: true,
            axis_icon: true,
            pixel_ratio: 1.0,
            threads: 0,
        }
    }
}

type Color3 = [f32; 3];

fn c3((r, g, b): Rgb) -> Color3 {
    [r as f32, g as f32, b as f32]
}

/// One thing to draw, in pixel coordinates.
enum Prim {
    /// A polyline of width `2·r` with round ends and joins (one point: a
    /// dot).
    Stroke {
        pts: Vec<Vec2>,
        closed: bool,
        r: f64,
        color: Color3,
    },
    /// Even-odd filled rings.
    Fill {
        rings: Vec<Vec<Vec2>>,
        color: Color3,
    },
    /// A one-pixel axis-aligned line drawn opaque (grid lines).
    Rule {
        vertical: bool,
        at: i64,
        color: Color3,
    },
}

struct Placed {
    prim: Prim,
    y0: f64,
    y1: f64,
}

/// Turns world shapes into pixel primitives.
struct Lister<'a> {
    view: &'a View,
    w: u32,
    h: u32,
    out: Vec<Placed>,
}

impl Lister<'_> {
    fn px(&self, p: Vec2) -> Vec2 {
        self.view.to_px(p, self.w, self.h)
    }

    fn push(&mut self, prim: Prim) {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        let mut grow = |p: &Vec2, r: f64| {
            x0 = x0.min(p.x - r);
            y0 = y0.min(p.y - r);
            x1 = x1.max(p.x + r);
            y1 = y1.max(p.y + r);
        };
        match &prim {
            Prim::Stroke { pts, r, .. } => pts.iter().for_each(|p| grow(p, r + 1.0)),
            Prim::Fill { rings, .. } => rings.iter().flatten().for_each(|p| grow(p, 1.0)),
            Prim::Rule { .. } => {
                x0 = 0.0;
                y0 = 0.0;
                x1 = self.w as f64;
                y1 = self.h as f64;
            }
        }
        let visible = x1 >= 0.0 && y1 >= 0.0 && x0 <= self.w as f64 && y0 <= self.h as f64;
        if visible && x0.is_finite() && y1.is_finite() {
            self.out.push(Placed { prim, y0, y1 });
        }
    }

    /// Points along an arc, close enough that no chord strays more than a
    /// fifth of a pixel from the curve.
    fn arc(&self, c: Vec2, r: f64, start: f64, sweep: f64) -> Vec<Vec2> {
        let r_px = r * self.view.scale;
        let step = if r_px > 0.25 {
            2.0 * (1.0 - 0.2 / r_px).max(-1.0).acos()
        } else {
            sweep.abs()
        };
        let n = ((sweep.abs() / step.max(1e-6)).ceil() as usize).clamp(1, 20_000);
        (0..=n)
            .map(|i| self.px(c + Vec2::from_angle(start + sweep * i as f64 / n as f64) * r))
            .collect()
    }

    fn stroke_shape(&mut self, shape: &Shape, r: f64, color: Color3) {
        let stroke = |pts: Vec<Vec2>, closed: bool| Prim::Stroke {
            pts,
            closed,
            r,
            color,
        };
        match shape {
            Shape::Line { a, b } => {
                let pts = if a == b {
                    vec![self.px(*a)]
                } else {
                    vec![self.px(*a), self.px(*b)]
                };
                self.push(stroke(pts, false));
            }
            Shape::Circle { center, radius } => {
                let mut pts = self.arc(*center, *radius, 0.0, TAU);
                pts.pop();
                self.push(stroke(pts, true));
            }
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sweep = crate::geom::curve::normalize_sweep(end - start);
                let pts = self.arc(*center, *radius, *start, sweep);
                self.push(stroke(pts, false));
            }
            Shape::Polyline { vertices, closed } => {
                let n = vertices.len();
                if n == 0 {
                    return;
                }
                let mut pts = vec![self.px(vertices[0].0)];
                let segs = if *closed { n } else { n - 1 };
                for i in 0..segs {
                    let (p1, bulge) = vertices[i];
                    let p2 = vertices[(i + 1) % n].0;
                    match bulge_to_arc(p1, p2, bulge) {
                        None => pts.push(self.px(p2)),
                        Some((arc, reversed)) => {
                            let mut a = self.arc(arc.center, arc.radius, arc.start, arc.sweep());
                            if reversed {
                                a.reverse();
                            }
                            pts.extend_from_slice(&a[1..]);
                        }
                    }
                }
                self.push(stroke(pts, *closed));
            }
            Shape::Path { points, closed } => {
                let pts = points.iter().map(|p| self.px(*p)).collect();
                self.push(stroke(pts, *closed));
            }
            Shape::Lines { segments } => {
                for (a, b) in segments {
                    let pts = vec![self.px(*a), self.px(*b)];
                    self.push(stroke(pts, false));
                }
            }
            // Cut to the picture before stroking (see `display_list`).
            Shape::Ray { .. } | Shape::Fill { .. } | Shape::Point { .. } | Shape::Text { .. } => {}
        }
    }
}

fn display_list(scene: &Scene, view: &View, opt: &RasterOptions, w: u32, h: u32) -> Vec<Placed> {
    let mut l = Lister {
        view,
        w,
        h,
        out: Vec::new(),
    };
    let ratio = if opt.pixel_ratio.is_finite() && opt.pixel_ratio > 0.0 {
        opt.pixel_ratio
    } else {
        1.0
    };
    if opt.grid && opt.dark {
        grid(&mut l);
    }
    for it in &scene.items {
        let color = c3(if opt.dark {
            standards::screen_color(it.style.color)
        } else {
            standards::print_color(it.style.color, opt.paper_color)
        });
        let r = standards::screen_px(it.style.weight_mm()) * ratio / 2.0;
        match &it.shape {
            Shape::Fill { rings } => {
                let rings = rings
                    .iter()
                    .map(|ring| ring.iter().map(|p| l.px(*p)).collect())
                    .collect();
                l.push(Prim::Fill { rings, color });
            }
            Shape::Point { at } => {
                let p = l.px(*at);
                l.push(Prim::Stroke {
                    pts: vec![p],
                    closed: false,
                    r: 1.5 * ratio,
                    color,
                });
            }
            Shape::Text { text, .. } => {
                if let Some(p) = it.shape.placement() {
                    // Text far too small to read is drawn as nothing.
                    if p.height * view.scale < 0.5 {
                        continue;
                    }
                    for s in font::layout(text, &p) {
                        let pts = s.iter().map(|q| l.px(*q)).collect();
                        l.push(Prim::Stroke {
                            pts,
                            closed: false,
                            r,
                            color,
                        });
                    }
                }
            }
            Shape::Ray {
                origin,
                direction,
                ray,
            } => {
                // Cut to the picture, with a margin for the line's width.
                let a = view.to_world(Vec2::new(-4.0, h as f64 + 4.0), w, h);
                let b = view.to_world(Vec2::new(w as f64 + 4.0, -4.0), w, h);
                let mut rect = crate::geom::BBox::EMPTY;
                rect.add(a);
                rect.add(b);
                if let Some((p, q)) = crate::render::clip::clip_ray(*origin, *direction, *ray, &rect) {
                    for piece in dash::dash(&Shape::Line { a: p, b: q }, &it.style.pattern) {
                        l.stroke_shape(&piece, r, color);
                    }
                }
            }
            shape => {
                for piece in dash::dash(shape, &it.style.pattern) {
                    l.stroke_shape(&piece, r, color);
                }
            }
        }
    }
    if opt.axis_icon {
        axis_icon(&mut l, ratio);
    }
    l.out
}

/// Grid lines: every [`canvas::GRID_SPACING`] units, every fifth one
/// brighter; the spacing grows or shrinks by fives so lines stay 8 to 40
/// pixels apart at any zoom.
fn grid(l: &mut Lister) {
    let scale = l.view.scale;
    let mut s = canvas::GRID_SPACING;
    if !(scale.is_finite() && scale > 0.0) {
        return;
    }
    let every = canvas::GRID_MAJOR_EVERY as i64;
    // At extreme zooms the spacing overflows or underflows: no grid then.
    while s * scale < 8.0 {
        s *= every as f64;
        if !s.is_finite() {
            return;
        }
    }
    while s * scale >= 40.0 {
        s /= every as f64;
        if s == 0.0 {
            return;
        }
    }
    let lo = l.view.to_world(Vec2::new(0.0, l.h as f64), l.w, l.h);
    let hi = l.view.to_world(Vec2::new(l.w as f64, 0.0), l.w, l.h);
    let mut rules: Vec<(bool, i64, bool)> = Vec::new();
    for (vertical, a, b) in [(true, lo.x, hi.x), (false, lo.y, hi.y)] {
        let (k0, k1) = ((a / s).floor(), (b / s).ceil());
        if !(k0.is_finite() && k1.is_finite()) || k1 - k0 > 10_000.0 || k0.abs() > 1e15 {
            return;
        }
        let (k0, k1) = (k0 as i64, k1 as i64);
        for k in k0..=k1 {
            let world = k as f64 * s;
            let p = if vertical {
                l.px(Vec2::new(world, 0.0)).x
            } else {
                l.px(Vec2::new(0.0, world)).y
            };
            rules.push((vertical, p.floor() as i64, k.rem_euclid(every) == 0));
        }
    }
    // Minor lines first, so major ones win where they cross.
    for major in [false, true] {
        for &(vertical, at, m) in rules.iter().filter(|r| r.2 == major) {
            let color = c3(if m {
                canvas::GRID_MAJOR
            } else {
                canvas::GRID_MINOR
            });
            l.push(Prim::Rule {
                vertical,
                at,
                color,
            });
        }
    }
}

/// The X/Y icon in the bottom-left corner.
fn axis_icon(l: &mut Lister, ratio: f64) {
    let color = c3((0x8A, 0x93, 0x9C));
    let k = ratio;
    let o = Vec2::new(28.0 * k, l.h as f64 - 28.0 * k);
    let len = 36.0 * k;
    let r = 0.5 * k;
    for (dir, label) in [(Vec2::new(1.0, 0.0), "X"), (Vec2::new(0.0, -1.0), "Y")] {
        let tip = o + dir * len;
        l.push(Prim::Stroke {
            pts: vec![o, tip],
            closed: false,
            r,
            color,
        });
        let side = dir.perp() * (3.0 * k);
        l.push(Prim::Fill {
            rings: vec![vec![tip + dir * (6.0 * k), tip + side, tip - side]],
            color,
        });
        // Labels drawn with the font, in pixel space (y down).
        let at = tip + dir * (12.0 * k) + Vec2::new(-4.0 * k, 5.0 * k);
        let p = font::Placement::at(Vec2::ZERO, 10.0 * k);
        for s in font::layout(label, &p) {
            let pts = s.iter().map(|q| at + Vec2::new(q.x, -q.y)).collect();
            l.push(Prim::Stroke {
                pts,
                closed: false,
                r,
                color,
            });
        }
    }
}

/// One horizontal band of the image, drawn into working buffers that a
/// thread reuses from band to band.
struct Band<'a> {
    y0: usize,
    rows: usize,
    w: usize,
    buf: &'a mut [f32],
    mask: &'a mut [f32],
    row_min: &'a mut [usize],
    row_max: &'a mut [usize],
}

impl Band<'_> {
    fn touch(&mut self, row: usize, x: usize, cov: f32) {
        let i = row * self.w + x;
        if cov > self.mask[i] {
            self.mask[i] = cov;
        }
        if x < self.row_min[row] {
            self.row_min[row] = x;
        }
        if x + 1 > self.row_max[row] {
            self.row_max[row] = x + 1;
        }
    }

    /// Blend the mask onto the image in `color`, and clear it.
    fn composite(&mut self, color: Color3) {
        for row in 0..self.rows {
            let (a, b) = (self.row_min[row], self.row_max[row]);
            if a >= b {
                continue;
            }
            for x in a..b {
                let i = row * self.w + x;
                let m = self.mask[i];
                if m > 0.0 {
                    let m = m.min(1.0);
                    let p = &mut self.buf[i * 3..i * 3 + 3];
                    for c in 0..3 {
                        p[c] += (color[c] - p[c]) * m;
                    }
                    self.mask[i] = 0.0;
                }
            }
            self.row_min[row] = usize::MAX;
            self.row_max[row] = 0;
        }
    }

    /// Coverage of a capsule (the segment p–q widened by `r` each side).
    fn segment(&mut self, p: Vec2, q: Vec2, r: f64) {
        let reach = r + 0.5;
        let lo_y = p.y.min(q.y) - reach;
        let hi_y = p.y.max(q.y) + reach;
        let first = (lo_y.floor().max(self.y0 as f64)) as i64;
        let last = (hi_y.ceil().min((self.y0 + self.rows) as f64)) as i64;
        let d = q - p;
        let len = d.length();
        let (min_x, max_x) = (p.x.min(q.x) - reach, p.x.max(q.x) + reach);
        for y in first..last {
            let yc = y as f64 + 0.5;
            let (mut xa, mut xb) = (min_x, max_x);
            if len > 1e-12 && d.y.abs() > 1e-12 {
                let xc = p.x + (yc - p.y) * d.x / d.y;
                let half = reach * len / d.y.abs();
                xa = xa.max(xc - half);
                xb = xb.min(xc + half);
            }
            let xa = xa.floor().max(0.0) as i64;
            let xb = xb.ceil().min(self.w as f64) as i64;
            let row = (y - self.y0 as i64) as usize;
            for x in xa..xb {
                let c = Vec2::new(x as f64 + 0.5, yc);
                let dist = if len > 1e-12 {
                    let t = ((c - p).dot(d) / (len * len)).clamp(0.0, 1.0);
                    c.distance(p + d * t)
                } else {
                    c.distance(p)
                };
                let cov = (r + 0.5 - dist).clamp(0.0, 1.0) as f32;
                if cov > 0.0 {
                    self.touch(row, x as usize, cov);
                }
            }
        }
    }

    fn stroke(&mut self, pts: &[Vec2], closed: bool, r: f64, color: Color3) {
        match pts.len() {
            0 => return,
            1 => self.segment(pts[0], pts[0], r),
            _ => {
                for w in pts.windows(2) {
                    self.segment(w[0], w[1], r);
                }
                if closed {
                    self.segment(pts[pts.len() - 1], pts[0], r);
                }
            }
        }
        self.composite(color);
    }

    /// Anti-aliased even-odd fill: four sub-rows per pixel, exact
    /// coverage across each row.
    fn fill(&mut self, rings: &[Vec<Vec2>], y0: f64, y1: f64, color: Color3) {
        let first = (y0.floor().max(self.y0 as f64)) as i64;
        let last = (y1.ceil().min((self.y0 + self.rows) as f64)) as i64;
        let mut xs: Vec<f64> = Vec::new();
        let mut acc: Vec<f32> = vec![0.0; self.w];
        for y in first..last {
            let row = (y - self.y0 as i64) as usize;
            let (mut lo, mut hi) = (usize::MAX, 0usize);
            for s in 0..4 {
                let ys = y as f64 + (s as f64 + 0.5) / 4.0;
                xs.clear();
                for ring in rings {
                    let n = ring.len();
                    for i in 0..n {
                        let (a, b) = (ring[i], ring[(i + 1) % n]);
                        if (a.y <= ys) != (b.y <= ys) {
                            xs.push(a.x + (ys - a.y) * (b.x - a.x) / (b.y - a.y));
                        }
                    }
                }
                xs.sort_by(|a, b| a.total_cmp(b));
                for pair in xs.chunks_exact(2) {
                    let x0 = pair[0].clamp(0.0, self.w as f64);
                    let x1 = pair[1].clamp(0.0, self.w as f64);
                    if x1 <= x0 {
                        continue;
                    }
                    let (i0, i1) = (x0.floor() as usize, (x1.ceil() as usize).min(self.w));
                    for (x, a) in acc.iter_mut().enumerate().take(i1).skip(i0) {
                        let cover = (x1.min(x as f64 + 1.0) - x0.max(x as f64)).max(0.0);
                        *a += (cover / 4.0) as f32;
                    }
                    lo = lo.min(i0);
                    hi = hi.max(i1);
                }
            }
            // Spans never touched leave lo at usize::MAX: take/skip then
            // yields nothing.
            for (x, a) in acc.iter_mut().enumerate().take(hi).skip(lo) {
                if *a > 0.0 {
                    let v = std::mem::take(a);
                    self.touch(row, x, v.min(1.0));
                }
            }
        }
        self.composite(color);
    }

    fn rule(&mut self, vertical: bool, at: i64, color: Color3) {
        if vertical {
            if at < 0 || at >= self.w as i64 {
                return;
            }
            for row in 0..self.rows {
                let i = (row * self.w + at as usize) * 3;
                self.buf[i..i + 3].copy_from_slice(&color);
            }
        } else {
            let row = at - self.y0 as i64;
            if row < 0 || row >= self.rows as i64 {
                return;
            }
            let start = row as usize * self.w * 3;
            for x in 0..self.w {
                self.buf[start + x * 3..start + x * 3 + 3].copy_from_slice(&color);
            }
        }
    }

    fn draw(&mut self, list: &[Placed]) {
        let (top, bottom) = (self.y0 as f64, (self.y0 + self.rows) as f64);
        for p in list {
            if p.y1 < top || p.y0 > bottom {
                continue;
            }
            match &p.prim {
                Prim::Stroke {
                    pts,
                    closed,
                    r,
                    color,
                } => self.stroke(pts, *closed, *r, *color),
                Prim::Fill { rings, color } => self.fill(rings, p.y0, p.y1, *color),
                Prim::Rule {
                    vertical,
                    at,
                    color,
                } => self.rule(*vertical, *at, *color),
            }
        }
    }
}

/// Rows in one band of work. Bands are handed round the threads in turn,
/// so each thread's working buffers stay small (a few megabytes at the
/// largest size) however big the image.
const BAND_ROWS: usize = 64;

/// Draw a scene into an image.
pub fn render(scene: &Scene, opt: &RasterOptions) -> Image {
    let w = opt.width.clamp(1, MAX_SIZE);
    let h = opt.height.clamp(1, MAX_SIZE);
    let view = opt
        .view
        .unwrap_or_else(|| View::fit(&scene.bounds, w, h, 0.05 * w.min(h) as f64));
    let list = display_list(scene, &view, opt, w, h);
    let bg = c3(if opt.dark {
        canvas::BACKGROUND
    } else {
        canvas::PAPER
    });
    let (wu, hu) = (w as usize, h as usize);
    let mut rgba = vec![0u8; wu * hu * 4];
    let bands: Vec<(usize, &mut [u8])> = rgba.chunks_mut(BAND_ROWS * wu * 4).enumerate().collect();
    let threads = if opt.threads == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        opt.threads
    }
    .clamp(1, bands.len());
    let mut work: Vec<Vec<(usize, &mut [u8])>> = (0..threads).map(|_| Vec::new()).collect();
    for (i, band) in bands {
        work[i % threads].push((i, band));
    }
    let list = &list;
    let run = |jobs: Vec<(usize, &mut [u8])>| {
        let mut buf = vec![0f32; BAND_ROWS * wu * 3];
        let mut mask = vec![0f32; BAND_ROWS * wu];
        let mut row_min = vec![usize::MAX; BAND_ROWS];
        let mut row_max = vec![0usize; BAND_ROWS];
        for (i, out) in jobs {
            let rows = out.len() / (wu * 4);
            for p in buf[..rows * wu * 3].chunks_exact_mut(3) {
                p.copy_from_slice(&bg);
            }
            mask.fill(0.0);
            row_min.fill(usize::MAX);
            row_max.fill(0);
            Band {
                y0: i * BAND_ROWS,
                rows,
                w: wu,
                buf: &mut buf[..rows * wu * 3],
                mask: &mut mask[..rows * wu],
                row_min: &mut row_min[..rows],
                row_max: &mut row_max[..rows],
            }
            .draw(list);
            for (o, p) in out.chunks_exact_mut(4).zip(buf.chunks_exact(3)) {
                o[0] = p[0].round().clamp(0.0, 255.0) as u8;
                o[1] = p[1].round().clamp(0.0, 255.0) as u8;
                o[2] = p[2].round().clamp(0.0, 255.0) as u8;
                o[3] = 255;
            }
        }
    };
    if threads == 1 {
        // No threads to spawn (WebAssembly in a browser has none): draw
        // the bands here.
        for jobs in work {
            run(jobs);
        }
    } else {
        std::thread::scope(|scope| {
            for jobs in work {
                scope.spawn(move || run(jobs));
            }
        });
    }
    Image {
        width: w,
        height: h,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, Lineweight};
    use crate::render::{Item, Style};

    fn item(shape: Shape, color: Color, hundredths: u16, pattern: Vec<f64>) -> Item {
        Item {
            shape,
            style: Style {
                rgb: (0, 0, 0),
                aci: 7,
                color,
                layer: "0".into(),
                lineweight: Lineweight::Hundredths(hundredths),
                linetype: String::new(),
                pattern,
            },
            source: Default::default(),
        }
    }

    fn scene(items: Vec<Item>) -> Scene {
        let mut s = Scene {
            items,
            ..Default::default()
        };
        for it in &s.items {
            crate::render::shape_bounds(&it.shape, &mut s.bounds);
        }
        s
    }

    fn opts(w: u32, h: u32, view: View) -> RasterOptions {
        RasterOptions {
            width: w,
            height: h,
            view: Some(view),
            grid: false,
            axis_icon: false,
            ..Default::default()
        }
    }

    #[test]
    fn lines_are_drawn_one_or_two_pixels_wide_by_weight() {
        // Horizontal lines through pixel centres: y = 10.5 and 30.5 px.
        let view = View {
            center: Vec2::new(50.0, 25.0),
            scale: 1.0,
        };
        let s = scene(vec![
            item(
                Shape::Line {
                    a: Vec2::new(10.0, 25.0 - (10.5 - 25.0)),
                    b: Vec2::new(90.0, 25.0 - (10.5 - 25.0)),
                },
                Color::Index(7),
                25,
                vec![],
            ),
            item(
                Shape::Line {
                    a: Vec2::new(10.0, 25.0 - (30.0 - 25.0)),
                    b: Vec2::new(90.0, 25.0 - (30.0 - 25.0)),
                },
                Color::Index(3),
                50,
                vec![],
            ),
        ]);
        let img = render(&s, &opts(100, 50, view));
        let bg = canvas::BACKGROUND;
        let px = |x, y| img.pixel(x, y).unwrap();
        // Thin white line: one full row.
        assert_eq!(px(50, 10), [255, 255, 255, 255]);
        assert_eq!(&px(50, 9)[..3], &[bg.0, bg.1, bg.2]);
        assert_eq!(&px(50, 11)[..3], &[bg.0, bg.1, bg.2]);
        // Thick line on a pixel boundary (y = 30): rows 29 and 30, in the
        // softened green.
        let g = standards::screen_color(Color::Index(3));
        assert_eq!(&px(50, 29)[..3], &[g.0, g.1, g.2]);
        assert_eq!(&px(50, 30)[..3], &[g.0, g.1, g.2]);
        assert_eq!(&px(50, 27)[..3], &[bg.0, bg.1, bg.2]);
    }

    #[test]
    fn dashes_fills_and_paper() {
        let view = View {
            center: Vec2::new(50.0, 50.0),
            scale: 1.0,
        };
        let s = scene(vec![
            item(
                Shape::Line {
                    a: Vec2::new(0.0, 49.5),
                    b: Vec2::new(100.0, 49.5),
                },
                Color::Index(7),
                25,
                vec![10.0, -10.0],
            ),
            item(
                Shape::Fill {
                    rings: vec![vec![
                        Vec2::new(10.0, 70.0),
                        Vec2::new(30.0, 70.0),
                        Vec2::new(30.0, 90.0),
                        Vec2::new(10.0, 90.0),
                    ]],
                },
                Color::Index(1),
                25,
                vec![],
            ),
        ]);
        let img = render(&s, &opts(100, 100, view));
        // Along the dashed line, some pixels lit and some not.
        let lit = (0..100)
            .filter(|&x| img.pixel(x, 50).unwrap()[0] > 128)
            .count();
        assert!(lit > 30 && lit < 70, "{lit} lit pixels");
        // Inside the fill (y from 10 to 30 px).
        let red = standards::screen_color(Color::Index(1));
        assert_eq!(&img.pixel(20, 20).unwrap()[..3], &[red.0, red.1, red.2]);
        // On paper in colour, white prints black and red stays true red.
        let paper = render(
            &s,
            &RasterOptions {
                dark: false,
                paper_color: true,
                ..opts(100, 100, view)
            },
        );
        assert_eq!(paper.pixel(0, 0).unwrap(), [255, 255, 255, 255]);
        assert_eq!(&paper.pixel(20, 20).unwrap()[..3], &[255, 0, 0]);
        let dark_px = (0..100)
            .filter(|&x| paper.pixel(x, 50).unwrap()[0] < 128)
            .count();
        assert!(dark_px > 30);
    }

    #[test]
    fn the_result_does_not_depend_on_the_thread_count() {
        let items = (0..40)
            .map(|i| {
                let a = i as f64 * 0.15;
                item(
                    Shape::Circle {
                        center: Vec2::new(a.cos() * 30.0, a.sin() * 30.0),
                        radius: 5.0 + i as f64,
                    },
                    Color::Index((i % 7 + 1) as u8),
                    if i % 2 == 0 { 25 } else { 50 },
                    if i % 3 == 0 { vec![4.0, -2.0] } else { vec![] },
                )
            })
            .chain(std::iter::once(item(
                Shape::Text {
                    position: Vec2::ZERO,
                    height: 6.0,
                    rotation: 0.3,
                    width_factor: 1.0,
                    oblique: 0.0,
                    text: "DETAIL A".into(),
                    h_align: 4,
                    v_align: 0,
                    fit_to: None,
                },
                Color::Index(4),
                25,
                vec![],
            )))
            .collect();
        let s = scene(items);
        let base = RasterOptions {
            width: 320,
            height: 200,
            ..Default::default()
        };
        let one = render(
            &s,
            &RasterOptions {
                threads: 1,
                ..base.clone()
            },
        );
        let many = render(
            &s,
            &RasterOptions {
                threads: 7,
                ..base.clone()
            },
        );
        assert_eq!(one, many);
        // The grid shows on the dark canvas.
        let empty = render(&Scene::default(), &base);
        let grid_px = empty
            .rgba
            .chunks(4)
            .filter(|p| {
                (p[0], p[1], p[2]) == canvas::GRID_MINOR || (p[0], p[1], p[2]) == canvas::GRID_MAJOR
            })
            .count();
        assert!(grid_px > 1000);
    }

    #[test]
    fn views_fit_and_invert() {
        let mut b = BBox::EMPTY;
        b.add(Vec2::new(0.0, 0.0));
        b.add(Vec2::new(200.0, 100.0));
        let v = View::fit(&b, 1000, 1000, 50.0);
        assert!((v.scale - 4.5).abs() < 1e-12);
        let p = Vec2::new(12.5, -7.0);
        let back = v.to_world(v.to_px(p, 1000, 1000), 1000, 1000);
        assert!(back.distance(p) < 1e-9);
        // Huge sizes are clamped, odd input does not panic.
        let img = render(
            &Scene::default(),
            &RasterOptions {
                width: 0,
                height: 3,
                pixel_ratio: f64::NAN,
                ..Default::default()
            },
        );
        assert_eq!((img.width, img.height), (1, 3));
    }
}

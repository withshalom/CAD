//! Dimension graphics, drawn from a dimension's definition points and its
//! dimension style.
//!
//! DWG files store each dimension's graphics in an anonymous block, and
//! those are drawn as stored. Dimensions this app makes carry no block;
//! their graphics come from here, following DESIGN.md §7: thin lines,
//! filled 3:1 arrowheads, text that reads horizontally and sits in a break
//! of the dimension line. When text does not fit between the extension
//! lines it moves outside; when the arrows do not fit either they turn
//! round and point in from outside. Radius dimensions, and diameters too
//! small for their text or with text put outside the circle, use a leader
//! with a short horizontal landing.
//!
//! Linear, aligned, radius, diameter and angular dimensions are drawn;
//! ordinate ones not yet. Angular dimensions draw an arc about the vertex
//! with arrows at its ends, extension lines out along the rays to it where
//! the measured lines stop short, and the angle in degrees.

use super::Shape;
use crate::font;
use crate::geom::Vec2;
use crate::model::{DimStyle, Dimension, DimensionKind};
use crate::standards;
use crate::units::{dimension_text, DimTextFormat, Unit};

/// What a piece of a dimension is, which decides its colour and weight
/// (DIMCLRD / DIMLWD for lines and arrows, DIMCLRE / DIMLWE for
/// extension lines, DIMCLRT for text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Line,
    Extension,
    Text,
}

/// Sizes in drawing units, after DIMSCALE.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    pub text: f64,
    pub arrow: f64,
    pub ext_offset: f64,
    pub ext_extension: f64,
    pub gap: f64,
    pub tick: f64,
    pub line_extension: f64,
}

/// The sizes a style gives. A style with no text height (an empty or
/// imported record) takes the default style's sizes for the unit.
pub fn sizes(st: &DimStyle, unit: Unit) -> Sizes {
    let base = if st.text_height > 0.0 {
        st.clone()
    } else {
        standards::dim_style(unit)
    };
    let k = if st.scale > 0.0 && st.scale.is_finite() {
        st.scale
    } else {
        1.0
    };
    let pos = |v: f64| if v.is_finite() { v.max(0.0) * k } else { 0.0 };
    Sizes {
        text: pos(base.text_height),
        arrow: pos(base.arrow_size),
        ext_offset: pos(base.ext_offset),
        ext_extension: pos(base.ext_extension),
        gap: pos(base.text_gap),
        tick: pos(st.tick_size),
        line_extension: pos(st.dim_line_extension),
    }
}

/// Graphics of one dimension, in its plane, or `None` for kinds not drawn
/// yet. Coordinates are those of the definition points.
pub fn graphics(dm: &Dimension, st: &DimStyle, unit: Unit) -> Option<Vec<(Part, Shape)>> {
    let sz = sizes(st, unit);
    let fmt = DimTextFormat::from_style(st);
    let lfac = if st.linear_factor != 0.0 && st.linear_factor.is_finite() {
        st.linear_factor
    } else {
        1.0
    };
    let label = |measured: f64, prefix: &str| {
        dimension_text(measured * lfac, prefix, &dm.user_text, &fmt, unit)
    };
    let mut g = Builder {
        out: Vec::new(),
        sz,
        horizontal: st.text_inside_horizontal,
        above: st.text_above != 0,
    };
    match dm.kind {
        DimensionKind::Linear | DimensionKind::Aligned => {
            let p1 = dm.pt13.xy();
            let p2 = dm.pt14.xy();
            let on = dm.pt10.xy();
            let u = if dm.kind == DimensionKind::Aligned {
                (p2 - p1).normalized()
            } else {
                Vec2::from_angle(dm.dim_rotation)
            };
            let u = if u.length() < 0.5 {
                Vec2::new(1.0, 0.0)
            } else {
                u
            };
            let a = on + u * (p1 - on).dot(u);
            let b = on + u * (p2 - on).dot(u);
            g.extension(p1, a);
            g.extension(p2, b);
            let text = label((p2 - p1).dot(u).abs(), "");
            let mid = (a + b) * 0.5;
            g.dimension_line(a, b, u, text_position(dm, mid), &text);
        }
        DimensionKind::Diameter => {
            let p1 = dm.pt15.xy();
            let p2 = dm.pt10.xy();
            let d = p1.distance(p2);
            if d < 1e-12 {
                return Some(Vec::new());
            }
            let e = (p1 - p2) / d;
            let c = (p1 + p2) * 0.5;
            let text = label(d, "⌀");
            match text_position(dm, c) {
                // Text put outside the circle: a leader to the nearer end.
                Some(placed) if placed.distance(c) > d / 2.0 => {
                    let tip = if placed.distance(p1) <= placed.distance(p2) {
                        p1
                    } else {
                        p2
                    };
                    g.leader_to(tip, c, placed, &text)
                }
                Some(placed) => g.dimension_line(p2, p1, e, Some(placed), &text),
                None => {
                    let t = g.half_extent(e, g.text_angle(e), &text);
                    if d >= 2.0 * (t + g.sz.arrow) {
                        g.dimension_line(p2, p1, e, None, &text);
                    } else {
                        g.leader(p1, e, &text);
                    }
                }
            }
        }
        DimensionKind::Radius => {
            let c = dm.pt10.xy();
            let p = dm.pt15.xy();
            let r = c.distance(p);
            if r < 1e-12 {
                return Some(Vec::new());
            }
            let e = (p - c) / r;
            let text = label(r, "R");
            match text_position(dm, Vec2::new(f64::NAN, f64::NAN)) {
                None => g.leader(p, e, &text),
                Some(t) => g.leader_to(p, c, t, &text),
            }
        }
        DimensionKind::Angular3Pt | DimensionKind::Angular2Line => {
            let Some(arc) = dm.angular_arc() else {
                return Some(Vec::new());
            };
            let text = angle_text(arc.sweep, st, &dm.user_text);
            let mid = arc.center + Vec2::from_angle(arc.start + arc.sweep / 2.0) * arc.radius;
            g.angular(&arc, text_position(dm, mid), &text);
        }
        DimensionKind::Ordinate => return None,
    }
    Some(g.out)
}

/// An angle as a dimension shows it: degrees with the style's decimal
/// places, trailing zeros dropped (so 45° and 22.5° read as such), and
/// the dimension's own text applied as for lengths.
pub fn angle_text(radians: f64, st: &DimStyle, user_text: &str) -> String {
    let mut v = crate::units::format_angle(radians, crate::units::AngleFormat::Degrees, st.decimals.min(4));
    if v.contains('.') {
        let deg = v.trim_end_matches('°').trim_end_matches('0').trim_end_matches('.').to_string();
        v = format!("{deg}°");
    }
    match user_text {
        "" => v,
        " " => String::new(),
        t if t.contains("<>") => t.replace("<>", &v),
        t => t.to_string(),
    }
}

/// Where the text was put, if not in its default place. A zero text
/// position, or the default one, means "default".
fn text_position(dm: &Dimension, default: Vec2) -> Option<Vec2> {
    let t = dm.text_midpoint;
    let tol = 1e-9 * (1.0 + t.length().max(default.length()));
    if t == Vec2::ZERO || (default.is_finite() && t.distance(default) <= tol) {
        None
    } else {
        Some(t)
    }
}

struct Builder {
    out: Vec<(Part, Shape)>,
    sz: Sizes,
    horizontal: bool,
    above: bool,
}

impl Builder {
    fn line(&mut self, part: Part, a: Vec2, b: Vec2) {
        if a.distance(b) > 1e-12 {
            self.out.push((part, Shape::Line { a, b }));
        }
    }

    /// An extension line from the feature at `from` to the dimension line
    /// at `to`: a gap at the feature, running on past the dimension line.
    fn extension(&mut self, from: Vec2, to: Vec2) {
        let d = to - from;
        let len = d.length();
        if len < 1e-12 {
            return;
        }
        let dir = d / len;
        if self.sz.ext_offset < len + self.sz.ext_extension {
            self.line(
                Part::Extension,
                from + dir * self.sz.ext_offset,
                to + dir * self.sz.ext_extension,
            );
        }
    }

    /// An arrowhead (or tick) with its tip at `tip`, pointing along `dir`.
    fn arrow(&mut self, tip: Vec2, dir: Vec2) {
        if self.sz.tick > 0.0 {
            let s = (dir + dir.perp()).normalized() * self.sz.tick;
            self.line(Part::Line, tip - s, tip + s);
            return;
        }
        let base = tip - dir * self.sz.arrow;
        let w = dir.perp() * (self.sz.arrow / (2.0 * standards::ARROW_LENGTH_TO_WIDTH));
        self.out.push((
            Part::Line,
            Shape::Fill {
                rings: vec![vec![tip, base + w, base - w]],
            },
        ));
    }

    /// Angle text is drawn at next to a line along `e`: horizontal
    /// (unidirectional), or along the line, reading from the bottom or the
    /// right.
    fn text_angle(&self, e: Vec2) -> f64 {
        if self.horizontal {
            return 0.0;
        }
        let mut a = e.angle();
        if a > std::f64::consts::FRAC_PI_2 + 1e-9 {
            a -= std::f64::consts::PI;
        } else if a <= -std::f64::consts::FRAC_PI_2 + 1e-9 {
            a += std::f64::consts::PI;
        }
        a
    }

    /// Half the length of the line along `e` that the text box (with its
    /// gap) covers when centred on the line.
    fn half_extent(&self, e: Vec2, angle: f64, text: &str) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let hw = font::text_width(text) * self.sz.text / 2.0 + self.sz.gap;
        let hh = self.sz.text / 2.0 + self.sz.gap;
        let (s, c) = angle.sin_cos();
        let lx = (e.x * c + e.y * s).abs();
        let ly = (-e.x * s + e.y * c).abs();
        let tx = if lx > 1e-12 { hw / lx } else { f64::INFINITY };
        let ty = if ly > 1e-12 { hh / ly } else { f64::INFINITY };
        tx.min(ty)
    }

    fn text(&mut self, at: Vec2, angle: f64, text: &str, h_align: u16) {
        if text.is_empty() {
            return;
        }
        self.out.push((
            Part::Text,
            Shape::Text {
                position: at,
                height: self.sz.text,
                rotation: angle,
                width_factor: 1.0,
                oblique: 0.0,
                text: text.to_string(),
                h_align,
                v_align: 2,
                fit_to: None,
            },
        ));
    }

    /// A dimension line from `a` to `b` with arrows at both ends pointing
    /// out, and the text: in a break in the middle when it fits, outside
    /// past `b` when it does not, or where the user put it. Text outside
    /// the extension lines but on the line's path gets the line extended
    /// to reach it; text set above the line past an end gets the line run
    /// on beneath it.
    ///
    /// Text above (DIMTAD) applies when the text runs along the line, or
    /// the line is horizontal. Horizontal text on a vertical or sloped line
    /// sits in a break instead, as DIMTAD defines it: lifted only by half
    /// its height it would still cross the line.
    fn dimension_line(&mut self, a: Vec2, b: Vec2, u: Vec2, placed: Option<Vec2>, text: &str) {
        let len = a.distance(b);
        let e = if len > 1e-12 { (b - a) / len } else { u };
        let angle = self.text_angle(e);
        let above = self.above && (!self.horizontal || e.y.abs() < 1e-9);
        let t = self.half_extent(e, angle, text);
        let asz = if self.sz.tick > 0.0 {
            0.0
        } else {
            self.sz.arrow
        };
        let ext = if self.sz.tick > 0.0 {
            self.sz.line_extension
        } else {
            0.0
        };
        // "Up" for text above the line: towards +y, or left of a vertical
        // line.
        let n = {
            let p = e.perp();
            if p.y < -1e-9 || (p.y.abs() <= 1e-9 && p.x > 0.0) {
                -p
            } else {
                p
            }
        };
        let hh = self.sz.text / 2.0 + self.sz.gap;
        let (center, inside) = match placed {
            Some(p) => (p, len >= 2.0 * asz),
            None => {
                let mid = (a + b) * 0.5;
                let lift = if above { n * hh } else { Vec2::ZERO };
                let fits = if above {
                    len >= 2.0 * asz && 2.0 * t <= len
                } else {
                    len >= 2.0 * (t + asz)
                };
                if fits {
                    (mid + lift, true)
                } else if len >= 2.0 * asz {
                    // Arrows inside, text outside past b.
                    (b + e * (asz + t) + lift, true)
                } else {
                    // Nothing fits: arrows outside, text past b's arrow.
                    (b + e * (2.0 * asz + t) + lift, false)
                }
            }
        };
        // The line's extent along e from a, then the break for the text.
        let (mut s0, mut s1) = (-ext, len + ext);
        if !inside {
            s0 = s0.min(-2.0 * asz);
            s1 = s1.max(len + 2.0 * asz);
        }
        let rel = center - a;
        let tc = rel.dot(e);
        // Text lifted clear of the line (above it) leaves it whole.
        let on_line = !text.is_empty() && rel.dot(e.perp()).abs() < hh * (1.0 - 1e-9);
        let at = |s: f64| a + e * s;
        if on_line {
            s1 = s1.max(tc - t);
            s0 = s0.min(tc + t);
            let (lo, hi) = (tc - t, tc + t);
            if lo > s0 {
                self.line(Part::Line, at(s0), at(lo.min(s1)));
            }
            if hi < s1 {
                self.line(Part::Line, at(hi.max(s0)), at(s1));
            }
        } else {
            // Text just above the line, past either end: the line runs on
            // beneath it.
            let beside = rel.dot(e.perp()).abs() <= 2.0 * hh;
            if !text.is_empty() && beside {
                if tc > len {
                    s1 = s1.max(tc + t);
                }
                if tc < 0.0 {
                    s0 = s0.min(tc - t);
                }
            }
            self.line(Part::Line, at(s0), at(s1));
        }
        if inside {
            self.arrow(a, -e);
            self.arrow(b, e);
        } else {
            // Arrows point in from outside.
            self.arrow(a, e);
            self.arrow(b, -e);
        }
        self.text(center, angle, text, 4);
    }

    /// An angular dimension: extension lines along the rays where the
    /// measured feature stops short of the arc, the arc with arrows at its
    /// ends (turned round to point in when the arc is too short for them),
    /// and the angle at the arc's middle: in a break of the arc, or above
    /// it with text above (DIMTAD), or where the user put it.
    fn angular(&mut self, arc: &crate::model::AngularArc, placed: Option<Vec2>, text: &str) {
        let (c, r) = (arc.center, arc.radius);
        if r < 1e-12 || arc.sweep < 1e-12 {
            return;
        }
        let end_angle = arc.start + arc.sweep;
        for (i, ang) in [arc.start, end_angle].into_iter().enumerate() {
            let e = Vec2::from_angle(ang);
            let (near, far) = arc.legs[i];
            if r > far + 1e-9 {
                let from = far + self.sz.ext_offset;
                if from < r + self.sz.ext_extension {
                    self.line(Part::Extension, c + e * from, c + e * (r + self.sz.ext_extension));
                }
            } else if r < near - 1e-9 {
                let from = near - self.sz.ext_offset;
                let to = (r - self.sz.ext_extension).max(0.0);
                if from > to {
                    self.line(Part::Extension, c + e * from, c + e * to);
                }
            }
        }
        let asz = if self.sz.tick > 0.0 { 0.0 } else { self.sz.arrow };
        let inside = r * arc.sweep >= 2.0 * asz;
        let mid = arc.start + arc.sweep / 2.0;
        let em = Vec2::from_angle(mid);
        let tangent = em.perp();
        let angle = self.text_angle(tangent);
        let hh = self.sz.text / 2.0 + self.sz.gap;
        let above = self.above;
        let center = match placed {
            Some(p) => p,
            None if above => c + em * (r + hh),
            None => c + em * r,
        };
        // The arc, broken where the text sits on it.
        let on_arc = !text.is_empty() && (center.distance(c) - r).abs() < hh * (1.0 - 1e-9);
        let (mut a0, mut a1) = (arc.start, end_angle);
        if !inside {
            let ext = (2.0 * asz / r).min(std::f64::consts::FRAC_PI_2);
            a0 -= ext;
            a1 += ext;
        }
        let mut spans = vec![(a0, a1)];
        if on_arc {
            let ta = (center - c).angle();
            let rel = crate::geom::curve::normalize_angle(ta - a0);
            if rel <= a1 - a0 {
                let ha = self.half_extent(tangent, angle, text) / r;
                let tm = a0 + rel;
                spans = vec![(a0, tm - ha), (tm + ha, a1)];
            }
        }
        for (s, e) in spans {
            if e - s > 1e-9 {
                self.out.push((
                    Part::Line,
                    Shape::Arc {
                        center: c,
                        radius: r,
                        start: s,
                        end: e,
                    },
                ));
            }
        }
        let (ps, pe) = (c + Vec2::from_angle(arc.start) * r, c + Vec2::from_angle(end_angle) * r);
        let (ts, te) = (Vec2::from_angle(arc.start).perp(), Vec2::from_angle(end_angle).perp());
        if inside {
            self.arrow(ps, -ts);
            self.arrow(pe, te);
        } else {
            self.arrow(ps, ts);
            self.arrow(pe, -te);
        }
        self.text(center, angle, text, 4);
    }

    /// A leader to the feature at `tip`, coming in from outside along
    /// `out`, with a horizontal landing and the text after it.
    fn leader(&mut self, tip: Vec2, out: Vec2, text: &str) {
        let asz = self.sz.arrow;
        let base = tip + out * asz;
        let elbow = tip + out * (3.0 * asz);
        let side = if out.x >= -1e-9 { 1.0 } else { -1.0 };
        let landing = elbow + Vec2::new(side * asz, 0.0);
        self.arrow(tip, -out);
        self.line(Part::Line, base, elbow);
        self.line(Part::Line, elbow, landing);
        let at = landing + Vec2::new(side * self.sz.gap, 0.0);
        self.text(at, 0.0, text, if side > 0.0 { 0 } else { 2 });
    }

    /// A leader to the feature at `tip` (on a curve around `center`) from
    /// text the user placed at `at` (the text's middle). From outside the
    /// curve: an arrow pointing in, a straight leader, and a short
    /// horizontal landing into the side of the text facing the feature, as
    /// [`Self::leader`] draws it. From inside: an arrow pointing out at the
    /// curve and a straight line from the text.
    fn leader_to(&mut self, tip: Vec2, center: Vec2, at: Vec2, text: &str) {
        let asz = self.sz.arrow;
        let r = tip.distance(center);
        let radial = (tip - center).normalized();
        if at.distance(center) < r {
            self.arrow(tip, radial);
            let base = tip - radial * asz;
            let toward = (at - base).normalized();
            let t = self.half_extent(toward, 0.0, text);
            let reach = at - toward * t;
            if (reach - base).dot(toward) > 0.0 {
                self.line(Part::Line, base, reach);
            }
            self.text(at, 0.0, text, 4);
            return;
        }
        self.arrow(tip, -radial);
        let base = tip + radial * asz;
        let side = if at.x >= tip.x { -1.0 } else { 1.0 };
        let half = if text.is_empty() {
            0.0
        } else {
            font::text_width(text) * self.sz.text / 2.0 + self.sz.gap
        };
        let end = at + Vec2::new(side * half, 0.0);
        let elbow = end + Vec2::new(side * asz, 0.0);
        self.line(Part::Line, base, elbow);
        self.line(Part::Line, elbow, end);
        self.text(at, 0.0, text, 4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm_style() -> DimStyle {
        standards::dim_style(Unit::Millimeter)
    }

    fn texts(parts: &[(Part, Shape)]) -> Vec<(String, Vec2)> {
        parts
            .iter()
            .filter_map(|(_, s)| match s {
                Shape::Text { text, position, .. } => Some((text.clone(), *position)),
                _ => None,
            })
            .collect()
    }

    fn arrows(parts: &[(Part, Shape)]) -> Vec<Vec<Vec2>> {
        parts
            .iter()
            .filter_map(|(_, s)| match s {
                Shape::Fill { rings } => Some(rings[0].clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_horizontal_dimension_with_room() {
        let dm = Dimension::linear(
            Vec2::new(0.0, 0.0),
            Vec2::new(200.0, 0.0),
            Vec2::new(0.0, 20.0),
            0.0,
        );
        let parts = graphics(&dm, &mm_style(), Unit::Millimeter).unwrap();
        // Text "200.00" centred on the dimension line, in a break.
        let t = texts(&parts);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].0, "200.00");
        assert!(t[0].1.distance(Vec2::new(100.0, 20.0)) < 1e-9);
        let dim_lines: Vec<(Vec2, Vec2)> = parts
            .iter()
            .filter_map(|(p, s)| match (p, s) {
                (Part::Line, Shape::Line { a, b }) => Some((*a, *b)),
                _ => None,
            })
            .collect();
        assert_eq!(dim_lines.len(), 2, "broken around the text");
        assert!(dim_lines[0].1.x < 100.0 && dim_lines[1].0.x > 100.0);
        // Extension lines: 1.5 gap at the part, 2 past the dimension line.
        let ext: Vec<(Vec2, Vec2)> = parts
            .iter()
            .filter_map(|(p, s)| match (p, s) {
                (Part::Extension, Shape::Line { a, b }) => Some((*a, *b)),
                _ => None,
            })
            .collect();
        assert_eq!(ext.len(), 2);
        assert!(ext[0].0.distance(Vec2::new(0.0, 1.5)) < 1e-9);
        assert!(ext[0].1.distance(Vec2::new(0.0, 22.0)) < 1e-9);
        // Arrows: 3 long, 1 wide (3:1), tips on the extension lines,
        // pointing out.
        let a = arrows(&parts);
        assert_eq!(a.len(), 2);
        assert!(a[0][0].distance(Vec2::new(0.0, 20.0)) < 1e-9);
        assert!((a[0][1].distance(a[0][2]) - 1.0).abs() < 1e-9);
        assert!(((a[0][1] + a[0][2]) * 0.5).x > 0.0);
    }

    #[test]
    fn crowded_dimensions_move_text_then_arrows_outside() {
        // 10 mm vertical: "10.00" does not fit between the arrows.
        let dm = Dimension::linear(
            Vec2::new(200.0, 0.0),
            Vec2::new(200.0, 10.0),
            Vec2::new(210.0, 0.0),
            std::f64::consts::FRAC_PI_2,
        );
        let parts = graphics(&dm, &mm_style(), Unit::Millimeter).unwrap();
        let t = texts(&parts);
        assert!(t[0].1.y > 10.0, "text moved past the end: {:?}", t[0].1);
        // 4 mm: arrows turn round too.
        let dm = Dimension::linear(
            Vec2::new(0.0, 0.0),
            Vec2::new(4.0, 0.0),
            Vec2::new(0.0, -10.0),
            0.0,
        );
        let parts = graphics(&dm, &mm_style(), Unit::Millimeter).unwrap();
        let a = arrows(&parts);
        let base0 = (a[0][1] + a[0][2]) * 0.5;
        assert!(
            base0.x < 0.0,
            "first arrow comes in from the left: {base0:?}"
        );
    }

    #[test]
    fn diameters_and_radii() {
        // A big circle: the diameter line runs across with the text inside.
        let dm = Dimension::diameter(Vec2::new(50.0, 50.0), 40.0, 0.0);
        let parts = graphics(&dm, &mm_style(), Unit::Millimeter).unwrap();
        let t = texts(&parts);
        assert_eq!(t[0].0, "⌀80.00");
        assert!(t[0].1.distance(Vec2::new(50.0, 50.0)) < 1e-9);
        assert_eq!(arrows(&parts).len(), 2);
        // A small hole with a callout: a leader from outside.
        let dm = Dimension {
            user_text: "4X <> THRU".into(),
            ..Dimension::diameter(Vec2::ZERO, 6.5, std::f64::consts::FRAC_PI_4)
        };
        let parts = graphics(&dm, &mm_style(), Unit::Millimeter).unwrap();
        let t = texts(&parts);
        assert_eq!(t[0].0, "4X ⌀13.00 THRU");
        assert!(t[0].1.x > 6.5 && t[0].1.y > 6.5);
        assert_eq!(arrows(&parts).len(), 1);
        // A radius: leader outside, "R" prefix.
        let dm = Dimension::radius(Vec2::ZERO, 10.0, 0.0);
        let parts = graphics(&dm, &mm_style(), Unit::Millimeter).unwrap();
        assert_eq!(texts(&parts)[0].0, "R10.00");
        let tip = arrows(&parts)[0][0];
        assert!(tip.distance(Vec2::new(10.0, 0.0)) < 1e-9);
        // Ordinate dimensions are not drawn yet; a degenerate angular one
        // draws nothing.
        let dm = Dimension {
            kind: DimensionKind::Ordinate,
            ..Default::default()
        };
        assert!(graphics(&dm, &mm_style(), Unit::Millimeter).is_none());
        let dm = Dimension {
            kind: DimensionKind::Angular3Pt,
            ..Default::default()
        };
        assert_eq!(graphics(&dm, &mm_style(), Unit::Millimeter), Some(Vec::new()));
    }

    #[test]
    fn angular_dimensions_measure_the_side_they_are_placed() {
        use std::f64::consts::{FRAC_PI_2, PI};
        let st = mm_style();
        // A right angle at the origin, arc inside it.
        let dm = Dimension::angular(Vec2::ZERO, Vec2::new(50.0, 0.0), Vec2::new(0.0, 50.0), Vec2::new(20.0, 20.0));
        assert!((dm.measurement - FRAC_PI_2).abs() < 1e-12);
        let parts = graphics(&dm, &st, Unit::Millimeter).unwrap();
        assert_eq!(texts(&parts)[0].0, "90°");
        assert_eq!(arrows(&parts).len(), 2);
        // The arc stops at the legs: no extension lines needed inside them.
        assert!(!parts.iter().any(|(p, _)| *p == Part::Extension));
        // Placed outside the angle: the rest of the turn.
        let dm = Dimension::angular(Vec2::ZERO, Vec2::new(50.0, 0.0), Vec2::new(0.0, 50.0), Vec2::new(-20.0, -20.0));
        assert!((dm.measurement - 1.5 * PI).abs() < 1e-12);
        assert_eq!(texts(&graphics(&dm, &st, Unit::Millimeter).unwrap())[0].0, "270°");
        // Arc beyond the short legs: extension lines reach out to it.
        let dm = Dimension::angular(Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(30.0, 10.0));
        let parts = graphics(&dm, &st, Unit::Millimeter).unwrap();
        assert_eq!(texts(&parts)[0].0, "45°");
        assert_eq!(parts.iter().filter(|(p, _)| *p == Part::Extension).count(), 2);
        // Two lines: the angle of the quadrant the arc is in.
        let a = (Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0));
        let b = (Vec2::new(0.0, 0.0), Vec2::new(30.0, 30.0 * 3f64.sqrt()));
        let dm = Dimension::angular_lines(a.0, a.1, b.0, b.1, Vec2::new(20.0, 5.0)).unwrap();
        assert!((dm.measurement - PI / 3.0).abs() < 1e-9, "{}", dm.measurement);
        let dm = Dimension::angular_lines(a.0, a.1, b.0, b.1, Vec2::new(-20.0, 5.0)).unwrap();
        assert!((dm.measurement - 2.0 * PI / 3.0).abs() < 1e-9, "{}", dm.measurement);
        assert_eq!(texts(&graphics(&dm, &st, Unit::Millimeter).unwrap())[0].0, "120°");
        assert!(Dimension::angular_lines(a.0, a.1, Vec2::new(0.0, 5.0), Vec2::new(9.0, 5.0), Vec2::new(1.0, 1.0)).is_none());
        // Half degrees show as such.
        let dm = Dimension::angular(Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::from_angle(22.5f64.to_radians()) * 10.0, Vec2::new(8.0, 1.0));
        assert_eq!(texts(&graphics(&dm, &st, Unit::Millimeter).unwrap())[0].0, "22.5°");
    }

    fn dim_lines(parts: &[(Part, Shape)]) -> Vec<(Vec2, Vec2)> {
        parts
            .iter()
            .filter_map(|(p, s)| match (p, s) {
                (Part::Line, Shape::Line { a, b }) => Some((*a, *b)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn placed_text_gets_a_leader_with_a_landing() {
        use std::f64::consts::PI;
        let st = mm_style();
        let sz = sizes(&st, Unit::Millimeter);
        // A hole callout set down and to the left of the hole.
        let at = Vec2::new(-40.0, -20.0);
        let dm = Dimension {
            user_text: "4X <> THRU".into(),
            text_midpoint: at,
            ..Dimension::diameter(Vec2::ZERO, 6.5, 1.25 * PI)
        };
        let parts = graphics(&dm, &st, Unit::Millimeter).unwrap();
        assert_eq!(texts(&parts), vec![("4X ⌀13.00 THRU".to_string(), at)]);
        // One arrow, on the circle at the end nearer the text, coming in
        // from outside.
        let a = arrows(&parts);
        assert_eq!(a.len(), 1);
        assert!(a[0][0].distance(Vec2::from_angle(1.25 * PI) * 6.5) < 1e-9);
        let base = (a[0][1] + a[0][2]) * 0.5;
        assert!(base.length() > 6.5);
        // A straight leader, then a horizontal landing one arrow long that
        // stops a gap short of the text's right side.
        let lines = dim_lines(&parts);
        assert_eq!(lines.len(), 2);
        let (elbow, end) = lines[1];
        assert!(lines[0].0.distance(base) < 1e-9 && lines[0].1.distance(elbow) < 1e-9);
        assert!((elbow.y - at.y).abs() < 1e-9 && (end.y - at.y).abs() < 1e-9);
        assert!((elbow.x - end.x - sz.arrow).abs() < 1e-9);
        let half = font::text_width("4X ⌀13.00 THRU") * sz.text / 2.0;
        assert!((end.x - (at.x + half + sz.gap)).abs() < 1e-9);
        // Text placed inside a big circle stays on the diameter line.
        let dm = Dimension {
            text_midpoint: Vec2::new(10.0, 0.0),
            ..Dimension::diameter(Vec2::ZERO, 40.0, 0.0)
        };
        let parts = graphics(&dm, &st, Unit::Millimeter).unwrap();
        assert_eq!(arrows(&parts).len(), 2);
        assert_eq!(dim_lines(&parts).len(), 2, "broken around the text");
        // A radius with its text to the right: the landing comes in from
        // the left.
        let at = Vec2::new(30.0, 15.0);
        let dm = Dimension {
            text_midpoint: at,
            ..Dimension::radius(Vec2::ZERO, 10.0, 0.5)
        };
        let (elbow, end) = dim_lines(&graphics(&dm, &st, Unit::Millimeter).unwrap())[1];
        assert!(elbow.x < end.x && end.x < at.x);
    }

    #[test]
    fn text_above_follows_the_dimtad_rules() {
        use std::f64::consts::FRAC_PI_2;
        let above = DimStyle {
            text_above: 1,
            ..mm_style()
        };
        // A vertical line with horizontal text: in a break, not struck
        // through.
        let dm = Dimension::linear(
            Vec2::ZERO,
            Vec2::new(0.0, 100.0),
            Vec2::new(20.0, 0.0),
            FRAC_PI_2,
        );
        let parts = graphics(&dm, &above, Unit::Millimeter).unwrap();
        assert!((texts(&parts)[0].1.x - 20.0).abs() < 1e-9);
        assert_eq!(dim_lines(&parts).len(), 2, "broken around the text");
        // Text along the line: lifted to its left, the line whole.
        let aligned = DimStyle {
            text_inside_horizontal: false,
            ..above.clone()
        };
        let parts = graphics(&dm, &aligned, Unit::Millimeter).unwrap();
        assert!(texts(&parts)[0].1.x < 20.0);
        assert_eq!(dim_lines(&parts).len(), 1);
        // Too short for its text: above the line past the end, with the
        // line run on beneath it.
        let dm = Dimension::linear(Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 20.0), 0.0);
        let parts = graphics(&dm, &above, Unit::Millimeter).unwrap();
        let (s, p) = texts(&parts)[0].clone();
        assert!(p.y > 20.0 && p.x > 10.0, "{p:?}");
        let lines = dim_lines(&parts);
        assert_eq!(lines.len(), 1);
        let half = font::text_width(&s) * sizes(&above, Unit::Millimeter).text / 2.0;
        assert!(lines[0].1.x >= p.x + half, "{lines:?}");
    }

    #[test]
    fn styles_change_the_look() {
        let dm = Dimension::linear(
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 0.0),
            Vec2::new(0.0, 20.0),
            0.0,
        );
        // Text above the line, no break.
        let above = DimStyle {
            text_above: 1,
            ..mm_style()
        };
        let parts = graphics(&dm, &above, Unit::Millimeter).unwrap();
        assert!(texts(&parts)[0].1.y > 20.0);
        let lines = parts
            .iter()
            .filter(|(p, s)| *p == Part::Line && matches!(s, Shape::Line { .. }))
            .count();
        assert_eq!(lines, 1);
        // Ticks instead of arrows.
        let ticks = DimStyle {
            tick_size: 1.5,
            ..mm_style()
        };
        let parts = graphics(&dm, &ticks, Unit::Millimeter).unwrap();
        assert!(arrows(&parts).is_empty());
        // Dual units and scale.
        let dual = DimStyle {
            alt_units: true,
            scale: 2.0,
            ..mm_style()
        };
        let parts = graphics(&dm, &dual, Unit::Millimeter).unwrap();
        assert_eq!(texts(&parts)[0].0, "100.00 [3.937]");
        let h = parts.iter().find_map(|(_, s)| match s {
            Shape::Text { height, .. } => Some(*height),
            _ => None,
        });
        assert_eq!(h, Some(7.0));
        // An empty imported style falls back to the standard sizes.
        let empty = DimStyle::default();
        assert_eq!(sizes(&empty, Unit::Inch).text, 0.125);
    }
}

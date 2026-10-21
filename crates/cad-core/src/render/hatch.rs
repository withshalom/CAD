//! Hatch patterns: the pattern library, and the lines a pattern draws
//! inside a boundary.
//!
//! A pattern is a set of line families. Each family is lines at an angle
//! through a base point, every next line moved by an offset, dashed by a
//! dash list (positive dash, negative gap, zero dot), the way DXF and DWG
//! store a hatch's pattern (groups 53, 43–46, 79, 49). The library holds
//! the definitions in pattern units, an inch for the ANSI-named patterns;
//! [`pattern_lines`] scales and turns them into the stored form for a
//! drawing's units.
//!
//! The definitions are this project's own: plain geometric line sets
//! under the names drafting standards and other CAD programs use for the
//! same materials (ANSI31 iron and general section lining, ANSI32 steel,
//! ANSI37 lead and zinc …), so a drawing hatched here reads as intended
//! elsewhere.

use crate::geom::{Vec2, EPS};
use crate::model::HatchPatternLine;
use crate::units::Unit;

/// One line family in pattern units, as a `.pat` file writes it: angle in
/// degrees, base point, offset (along the line, then across it), dashes.
pub struct FamilyDef {
    pub angle: f64,
    pub base: (f64, f64),
    pub offset: (f64, f64),
    pub dashes: &'static [f64],
}

pub struct PatternDef {
    pub name: &'static str,
    pub description: &'static str,
    pub families: &'static [FamilyDef],
}

const fn fam(angle: f64, base: (f64, f64), offset: (f64, f64), dashes: &'static [f64]) -> FamilyDef {
    FamilyDef {
        angle,
        base,
        offset,
        dashes,
    }
}

/// The patterns this program draws by name. SOLID fills; USER is lines at
/// the hatch's angle spaced by its scale (doubled at 90° for a double
/// hatch).
pub const PATTERNS: &[PatternDef] = &[
    PatternDef {
        name: "ANSI31",
        description: "Iron, brick, stone; general section lining",
        families: &[fam(45.0, (0.0, 0.0), (0.0, 0.125), &[])],
    },
    PatternDef {
        name: "ANSI32",
        description: "Steel",
        families: &[
            fam(45.0, (0.0, 0.0), (0.0, 0.375), &[]),
            fam(45.0, (0.176_776_695, 0.0), (0.0, 0.375), &[]),
        ],
    },
    PatternDef {
        name: "ANSI33",
        description: "Bronze, brass, copper",
        families: &[
            fam(45.0, (0.0, 0.0), (0.0, 0.25), &[]),
            fam(45.0, (0.176_776_695, 0.0), (0.0, 0.25), &[0.125, -0.0625]),
        ],
    },
    PatternDef {
        name: "ANSI34",
        description: "Plastic, rubber",
        families: &[
            fam(45.0, (0.0, 0.0), (0.0, 0.75), &[]),
            fam(45.0, (0.176_776_695, 0.0), (0.0, 0.75), &[]),
            fam(45.0, (0.353_553_391, 0.0), (0.0, 0.75), &[]),
            fam(45.0, (0.530_330_086, 0.0), (0.0, 0.75), &[]),
        ],
    },
    PatternDef {
        name: "ANSI35",
        description: "Fire brick, refractory material",
        families: &[
            fam(45.0, (0.0, 0.0), (0.0, 0.25), &[]),
            fam(45.0, (0.176_776_695, 0.0), (0.0, 0.25), &[0.3125, -0.0625, 0.0, -0.0625]),
        ],
    },
    PatternDef {
        name: "ANSI36",
        description: "Marble, slate, glass",
        families: &[fam(45.0, (0.0, 0.0), (0.21875, 0.125), &[0.3125, -0.0625, 0.0, -0.0625])],
    },
    PatternDef {
        name: "ANSI37",
        description: "Lead, zinc, magnesium; sound, heat and electrical insulation",
        families: &[
            fam(45.0, (0.0, 0.0), (0.0, 0.125), &[]),
            fam(135.0, (0.0, 0.0), (0.0, 0.125), &[]),
        ],
    },
    PatternDef {
        name: "ANSI38",
        description: "Aluminium",
        families: &[
            fam(45.0, (0.0, 0.0), (0.0, 0.125), &[]),
            fam(135.0, (0.0, 0.0), (0.25, 0.125), &[0.3125, -0.1875]),
        ],
    },
    PatternDef {
        name: "LINE",
        description: "Parallel horizontal lines",
        families: &[fam(0.0, (0.0, 0.0), (0.0, 0.125), &[])],
    },
    PatternDef {
        name: "NET",
        description: "Horizontal and vertical grid",
        families: &[
            fam(0.0, (0.0, 0.0), (0.0, 0.125), &[]),
            fam(90.0, (0.0, 0.0), (0.0, 0.125), &[]),
        ],
    },
    PatternDef {
        name: "NET3",
        description: "Network of lines at 0°, 60° and 120°",
        families: &[
            fam(0.0, (0.0, 0.0), (0.0, 0.125), &[]),
            fam(60.0, (0.0, 0.0), (0.0, 0.125), &[]),
            fam(120.0, (0.0, 0.0), (0.0, 0.125), &[]),
        ],
    },
    PatternDef {
        name: "DASH",
        description: "Dashed lines",
        families: &[fam(0.0, (0.0, 0.0), (0.125, 0.125), &[0.125, -0.125])],
    },
    PatternDef {
        name: "DOTS",
        description: "A series of dots",
        families: &[fam(0.0, (0.0, 0.0), (0.03125, 0.0625), &[0.0, -0.0625])],
    },
    PatternDef {
        name: "SQUARE",
        description: "Small aligned squares",
        families: &[
            fam(0.0, (0.0, 0.0), (0.0, 0.125), &[0.125, -0.125]),
            fam(90.0, (0.0, 0.0), (0.0, 0.125), &[0.125, -0.125]),
        ],
    },
    PatternDef {
        name: "CROSS",
        description: "A series of crosses",
        families: &[
            fam(0.0, (0.0, 0.0), (0.25, 0.25), &[0.125, -0.375]),
            fam(90.0, (0.0625, -0.0625), (0.25, 0.25), &[0.125, -0.375]),
        ],
    },
    PatternDef {
        name: "BRICK",
        description: "Brick or masonry",
        families: &[
            fam(0.0, (0.0, 0.0), (0.0, 0.25), &[]),
            fam(90.0, (0.0, 0.0), (0.25, 0.25), &[0.25, -0.25]),
            fam(90.0, (0.25, 0.0), (0.25, 0.25), &[-0.25, 0.25]),
        ],
    },
    PatternDef {
        name: "ANGLE",
        description: "Angle steel",
        families: &[
            fam(0.0, (0.0, 0.0), (0.0, 0.275), &[0.2, -0.075]),
            fam(90.0, (0.0, 0.0), (0.0, 0.275), &[0.2, -0.075]),
        ],
    },
    PatternDef {
        name: "EARTH",
        description: "Earth or ground",
        families: &[
            fam(0.0, (0.0, 0.0), (0.25, 0.25), &[0.25, -0.25]),
            fam(0.0, (0.0, 0.09375), (0.25, 0.25), &[0.25, -0.25]),
            fam(0.0, (0.0, 0.1875), (0.25, 0.25), &[0.25, -0.25]),
            fam(90.0, (0.03125, 0.21875), (0.25, 0.25), &[0.25, -0.25]),
            fam(90.0, (0.125, 0.21875), (0.25, 0.25), &[0.25, -0.25]),
            fam(90.0, (0.21875, 0.21875), (0.25, 0.25), &[0.25, -0.25]),
        ],
    },
    PatternDef {
        name: "HONEY",
        description: "Honeycomb",
        families: &[
            fam(0.0, (0.0, 0.0), (0.1875, 0.108_253_175), &[0.125, -0.25]),
            fam(120.0, (0.0, 0.0), (0.1875, 0.108_253_175), &[0.125, -0.25]),
            fam(60.0, (0.125, 0.0), (0.1875, 0.108_253_175), &[0.125, -0.25]),
        ],
    },
];

/// Every name [`pattern_lines`] knows, SOLID and USER first.
pub fn names() -> Vec<&'static str> {
    let mut v = vec!["SOLID", "USER"];
    v.extend(PATTERNS.iter().map(|p| p.name));
    v
}

pub fn find(name: &str) -> Option<&'static PatternDef> {
    PATTERNS.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// Drawing units per pattern unit: the library is in inches, and metric
/// drawings draw it 25.4 times larger, as ISO pattern files do (in any
/// metric unit, as other CAD programs pick the file by the drawing's
/// measurement system, not its exact unit).
fn unit_factor(unit: Unit) -> f64 {
    if unit.is_metric() {
        25.4
    } else {
        1.0
    }
}

/// The line families the library gives a hatch, in the drawing's units:
/// the writers (`cad-dwg`, `cad-dxf`) store them for a pattern hatch that
/// holds none, so other programs draw the same pattern. Empty for SOLID
/// and for a pattern the library does not know.
pub fn library_lines(d: &crate::model::Drawing, h: &crate::model::Hatch) -> Vec<HatchPatternLine> {
    pattern_lines(&h.pattern_name, h.pattern_scale, h.pattern_angle, h.pattern_double, Unit::of(d)).unwrap_or_default()
}

/// The stored line families of a library pattern, `scale` and `angle`
/// (radians) applied, in drawing units; `None` for an unknown name. USER
/// gives lines at `angle` spaced `scale` apart, and a second set at 90°
/// when `double`. SOLID has none.
pub fn pattern_lines(name: &str, scale: f64, angle: f64, double: bool, unit: Unit) -> Option<Vec<HatchPatternLine>> {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    if name.eq_ignore_ascii_case("SOLID") {
        return Some(Vec::new());
    }
    if name.eq_ignore_ascii_case("USER") || name.is_empty() {
        let mut v = vec![family(angle, Vec2::ZERO, Vec2::new(0.0, scale), &[], angle)];
        if double {
            let a = angle + std::f64::consts::FRAC_PI_2;
            v.push(family(a, Vec2::ZERO, Vec2::new(0.0, scale), &[], a));
        }
        return Some(v);
    }
    let def = find(name)?;
    let k = scale * unit_factor(unit);
    Some(
        def.families
            .iter()
            .map(|f| {
                let a = f.angle.to_radians() + angle;
                let base = rotate(Vec2::new(f.base.0, f.base.1) * k, angle);
                let dashes: Vec<f64> = f.dashes.iter().map(|d| d * k).collect();
                family(a, base, Vec2::new(f.offset.0, f.offset.1) * k, &dashes, a)
            })
            .collect(),
    )
}

fn rotate(v: Vec2, a: f64) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// A family with its offset turned from the line's own axes (along,
/// across) into the hatch's.
fn family(angle: f64, base: Vec2, offset_along_across: Vec2, dashes: &[f64], line_angle: f64) -> HatchPatternLine {
    HatchPatternLine {
        angle,
        base,
        offset: rotate(offset_along_across, line_angle),
        dashes: dashes.to_vec(),
    }
}

/// Most pattern segments one hatch draws; past this the pattern is too
/// fine for its area (a scale typed wrong, usually) and only the boundary
/// is drawn.
pub const MAX_SEGMENTS: usize = 200_000;

/// The segments of `lines` inside `rings` (even-odd: holes and islands
/// alternate), in the hatch's coordinates. `None` when the pattern would
/// take more than [`MAX_SEGMENTS`].
pub fn fill_lines(rings: &[Vec<Vec2>], lines: &[HatchPatternLine]) -> Option<Vec<(Vec2, Vec2)>> {
    let mut out = Vec::new();
    let edges: Vec<(Vec2, Vec2)> = rings
        .iter()
        .filter(|r| r.len() >= 3)
        .flat_map(|r| (0..r.len()).map(move |i| (r[i], r[(i + 1) % r.len()])))
        .filter(|(a, b)| a.distance(*b) > 0.0)
        .collect();
    if edges.is_empty() {
        return Some(out);
    }
    for l in lines {
        let u = Vec2::from_angle(l.angle);
        let n = u.perp();
        let step = l.offset.dot(n);
        if !step.is_finite() || step.abs() < EPS || !l.base.is_finite() {
            continue;
        }
        let shift = l.offset.dot(u);
        // Range of line numbers that can meet the boundary.
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (a, _) in &edges {
            let s = (*a - l.base).dot(n) / step;
            lo = lo.min(s);
            hi = hi.max(s);
        }
        let (k0, k1) = (lo.floor() as i64, hi.ceil() as i64);
        if k1 - k0 > MAX_SEGMENTS as i64 {
            return None;
        }
        let period: f64 = l.dashes.iter().map(|d| d.abs()).sum();
        let dashed = !l.dashes.is_empty() && period > EPS && l.dashes.iter().any(|d| *d < 0.0);
        let mut ts: Vec<f64> = Vec::new();
        for k in k0..=k1 {
            let origin = l.base + l.offset * k as f64;
            // Where the line crosses the edges, as distances along u from
            // its origin. Each edge counts on one side only (half-open in
            // the normal direction), so a line through a vertex crosses
            // once.
            ts.clear();
            for (a, b) in &edges {
                let sa = (*a - origin).dot(n);
                let sb = (*b - origin).dot(n);
                if (sa > 0.0) == (sb > 0.0) {
                    continue;
                }
                let t = sa / (sa - sb);
                let p = *a + (*b - *a) * t;
                ts.push((p - origin).dot(u));
            }
            if ts.len() < 2 {
                continue;
            }
            ts.sort_by(f64::total_cmp);
            for pair in ts.chunks_exact(2) {
                let (ta, tb) = (pair[0], pair[1]);
                if tb - ta <= EPS {
                    continue;
                }
                if !dashed {
                    out.push((origin + u * ta, origin + u * tb));
                } else {
                    // The dash pattern starts at the line's origin, moved
                    // along by the family's shift on every line.
                    let phase = (shift * k as f64).rem_euclid(period);
                    let first = ((ta - phase) / period).floor() as i64;
                    let last = ((tb - phase) / period).ceil() as i64;
                    if (last - first) as usize * l.dashes.len() > MAX_SEGMENTS {
                        return None;
                    }
                    for c in first..=last {
                        let mut at = phase + c as f64 * period;
                        for d in &l.dashes {
                            let len = d.abs();
                            if *d >= 0.0 {
                                let (s, e) = (at.max(ta), (at + len).min(tb));
                                if e > s || (len == 0.0 && at >= ta && at <= tb) {
                                    out.push((origin + u * s, origin + u * e.max(s)));
                                }
                            }
                            at += len;
                        }
                    }
                }
                if out.len() > MAX_SEGMENTS {
                    return None;
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: f64) -> Vec<Vec<Vec2>> {
        vec![vec![Vec2::new(0.0, 0.0), Vec2::new(s, 0.0), Vec2::new(s, s), Vec2::new(0.0, s)]]
    }

    #[test]
    fn ansi31_in_inches_and_millimetres() {
        let l = pattern_lines("ANSI31", 1.0, 0.0, false, Unit::Inch).unwrap();
        assert_eq!(l.len(), 1);
        assert!((l[0].angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        // Spacing across the lines is the pattern's 1/8".
        let n = Vec2::from_angle(l[0].angle).perp();
        assert!((l[0].offset.dot(n) - 0.125).abs() < 1e-12);
        let m = pattern_lines("ansi31", 2.0, 0.0, false, Unit::Millimeter).unwrap();
        let n = Vec2::from_angle(m[0].angle).perp();
        assert!((m[0].offset.dot(n) - 2.0 * 3.175).abs() < 1e-9);
        assert!(pattern_lines("NOPE", 1.0, 0.0, false, Unit::Inch).is_none());
        assert!(pattern_lines("SOLID", 1.0, 0.0, false, Unit::Inch).unwrap().is_empty());
    }

    #[test]
    fn lines_fill_a_square_and_skip_a_hole() {
        let lines = pattern_lines("LINE", 1.0, 0.0, false, Unit::Inch).unwrap();
        let segs = fill_lines(&square(1.0), &lines).unwrap();
        // Horizontal lines every 1/8" across a 1" square.
        assert!((7..=9).contains(&segs.len()), "{}", segs.len());
        assert!(segs.iter().all(|(a, b)| (a.y - b.y).abs() < 1e-12 && (b.x - a.x - 1.0).abs() < 1e-9));
        // A hole in the middle splits the lines through it.
        let mut rings = square(1.0);
        rings.push(vec![Vec2::new(0.4, 0.4), Vec2::new(0.6, 0.4), Vec2::new(0.6, 0.6), Vec2::new(0.4, 0.6)]);
        let with_hole = fill_lines(&rings, &lines).unwrap();
        assert!(with_hole.len() > segs.len());
        assert!(with_hole.iter().all(|(a, b)| {
            let m = (*a + *b) * 0.5;
            !(m.x > 0.4 && m.x < 0.6 && m.y > 0.4 && m.y < 0.6)
        }));
    }

    #[test]
    fn dashes_and_dots_follow_the_pattern() {
        let lines = pattern_lines("DASH", 1.0, 0.0, false, Unit::Inch).unwrap();
        let segs = fill_lines(&square(1.0), &lines).unwrap();
        assert!(segs.iter().all(|(a, b)| a.distance(*b) <= 0.125 + 1e-9));
        let dots = fill_lines(&square(0.5), &pattern_lines("DOTS", 1.0, 0.0, false, Unit::Inch).unwrap()).unwrap();
        assert!(!dots.is_empty() && dots.iter().all(|(a, b)| a.distance(*b) < 1e-12));
        // A scale far too small for the area gives up instead of hanging.
        let fine = pattern_lines("ANSI31", 1e-6, 0.0, false, Unit::Inch).unwrap();
        assert!(fill_lines(&square(1000.0), &fine).is_none());
    }
}

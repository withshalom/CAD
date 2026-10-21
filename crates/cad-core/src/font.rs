//! The app's own single-stroke engineering font, and text layout.
//!
//! Every glyph is a few strokes (open polylines), drawn for this project
//! on a grid where the cap height is 12 units, the x-height 8 and the
//! descender 4. Text height in CAD is the cap height, so a glyph scales by
//! `height / 12`. Single strokes draw the same in every output (screen,
//! SVG, PNG, plotters) and engrave or mark cleanly, and DWG's SHX fonts
//! cannot be shipped (DESIGN.md §6): text that asks for any SHX font is
//! drawn with this one.
//!
//! The glyphs are written in a small path language, one string per glyph:
//!
//! - `Mx,y`: start a stroke at (x, y);
//! - `Lx,y`: line to (x, y);
//! - `Acx,cy,rx,ry,a0,a1`: elliptical arc around (cx, cy) from angle a0 to
//!   a1 (degrees; counter-clockwise when a1 > a0), continuing the stroke;
//! - `Dx,y`: a dot (a tiny closed stroke) at (x, y).
//!
//! Covered: printable ASCII, the symbols drawings use (° ± ⌀ × ÷ µ ² ³ ·
//! – — € and curly quotes), accented Latin letters (composed from a base
//! letter and an accent) and Ø ø. Anything else draws as a box.

use crate::geom::Vec2;
use std::sync::OnceLock;

/// Cap height in glyph units.
pub const CAP: f64 = 12.0;
/// Descender depth in glyph units.
pub const DESCENT: f64 = 4.0;
/// Space after each glyph's ink, in glyph units.
const SPACING: f64 = 3.0;
/// Advance of digits (all the same, so columns of numbers line up).
const DIGIT_ADVANCE: f64 = 10.0;
/// Advance of a space.
const SPACE_ADVANCE: f64 = 7.0;

/// The glyphs: character and path.
#[rustfmt::skip]
const GLYPHS: &[(char, &str)] = &[
    // Capitals.
    ('A', "M0,0 L4,12 L8,0 M1.333,4 L6.667,4"),
    ('B', "M0,6 L5,6 A5,3,3,3,90,-90 L0,0 L0,12 L4.5,12 A4.5,9,3,3,90,-90"),
    ('C', "A4,8,4,4,40,180 L0,4 A4,4,4,4,180,320"),
    ('D', "M0,0 L0,12 L3.5,12 A3.5,7.5,4.5,4.5,90,0 L8,4.5 A3.5,4.5,4.5,4.5,0,-90 L0,0"),
    ('E', "M7,12 L0,12 L0,0 L7,0 M0,6 L5.5,6"),
    ('F', "M7,12 L0,12 L0,0 M0,6 L5.5,6"),
    ('G', "A4,8,4,4,40,180 L0,4 A4,4,4,4,180,360 L8,6 L4.5,6"),
    ('H', "M0,0 L0,12 M8,12 L8,0 M0,6 L8,6"),
    ('I', "M0,0 L0,12"),
    ('J', "M7,12 L7,3.5 A3.5,3.5,3.5,3.5,0,-180"),
    ('K', "M0,0 L0,12 M8,12 L0,4 M2.5,6.5 L8,0"),
    ('L', "M0,12 L0,0 L7,0"),
    ('M', "M0,0 L0,12 L5,3 L10,12 L10,0"),
    ('N', "M0,0 L0,12 L8,0 L8,12"),
    ('O', "M0,4 L0,8 A4,8,4,4,180,0 L8,4 A4,4,4,4,0,-180"),
    ('P', "M0,0 L0,12 L4.5,12 A4.5,8.5,3.5,3.5,90,-90 L0,5"),
    ('Q', "M0,4 L0,8 A4,8,4,4,180,0 L8,4 A4,4,4,4,0,-180 M5,3 L8.5,-0.5"),
    ('R', "M0,0 L0,12 L4.5,12 A4.5,8.5,3.5,3.5,90,-90 L0,5 M4,5 L8,0"),
    ('S', "A4,9,3.8,3,20,270 A4,3,4,3,90,-160"),
    ('T', "M0,12 L8,12 M4,12 L4,0"),
    ('U', "M0,12 L0,4 A4,4,4,4,180,360 L8,12"),
    ('V', "M0,12 L4,0 L8,12"),
    ('W', "M0,12 L2.5,0 L5,10 L7.5,0 L10,12"),
    ('X', "M0,12 L8,0 M8,12 L0,0"),
    ('Y', "M0,12 L4,6 L8,12 M4,6 L4,0"),
    ('Z', "M0,12 L8,12 L0,0 L8,0"),
    // Digits (equal advance).
    ('0', "M0,3.5 L0,8.5 A3.5,8.5,3.5,3.5,180,0 L7,3.5 A3.5,3.5,3.5,3.5,0,-180"),
    ('1', "M1.5,10 L3.5,12 L3.5,0"),
    ('2', "A3.5,8.5,3.5,3.5,165,-30 L0,0 L7,0"),
    ('3', "A3.5,9,3,3,150,-90 A3.5,3,3.5,3,90,-150 M2,6 L3.5,6"),
    ('4', "M5.5,0 L5.5,12 L0,3.5 L7,3.5"),
    ('5', "M6.5,12 L1.2,12 L0.819,5.914 A3.5,3.6,3.5,3.6,140,-150"),
    ('6', "A3.5,8.5,3.5,3.5,50,180 L0,3.5 A3.5,3.5,3.5,3.5,180,540"),
    ('7', "M0,12 L7,12 L2.5,0"),
    ('8', "M3.5,6 A3.5,3,3.5,3,90,450 A3.5,9,3,3,270,630"),
    ('9', "A3.5,3.5,3.5,3.5,230,360 L7,8.5 A3.5,8.5,3.5,3.5,0,360"),
    // Lower case.
    ('a', "M6,8 L6,0 M6,4 A3,4,3,4,0,360"),
    ('b', "M0,12 L0,0 M0,4 A3,4,3,4,180,-180"),
    ('c', "A3,4,3,4,40,320"),
    ('d', "M6,12 L6,0 M6,4 A3,4,3,4,0,360"),
    ('e', "M0,4 L6,4 A3,4,3,4,0,315"),
    ('f', "A4.5,10,2,2,45,180 L2.5,0 M0.5,8 L5,8"),
    ('g', "M6,8 L6,-1.5 A3,-1.5,3,2.5,0,-160 M6,4 A3,4,3,4,0,360"),
    ('h', "M0,12 L0,0 M0,5 A3,5,3,3,180,0 L6,0"),
    ('i', "M0,8 L0,0 D0,11"),
    ('j', "M2,8 L2,-2 A0,-2,2,2,0,-150 D2,11"),
    ('k', "M0,12 L0,0 M5.5,8 L0,3 M2,4.818 L6,0"),
    ('l', "M0,12 L0,0"),
    ('m', "M0,8 L0,0 M0,5 A2.5,5,2.5,3,180,0 L5,0 M5,5 A7.5,5,2.5,3,180,0 L10,0"),
    ('n', "M0,8 L0,0 M0,5 A3,5,3,3,180,0 L6,0"),
    ('o', "A3,4,3,4,0,360"),
    ('p', "M0,8 L0,-4 M0,4 A3,4,3,4,180,-180"),
    ('q', "M6,8 L6,-4 M6,4 A3,4,3,4,0,360"),
    ('r', "M0,8 L0,0 M0,4.5 A3.5,4.5,3.5,3.5,180,70"),
    ('s', "A3,6,2.8,2,20,270 A3,2,3,2,90,-160"),
    ('t', "M2,12 L2,1.5 A3.5,1.5,1.5,1.5,180,300 M0,8 L5,8"),
    ('u', "M0,8 L0,3 A3,3,3,3,180,360 M6,8 L6,0"),
    ('v', "M0,8 L3,0 L6,8"),
    ('w', "M0,8 L2.25,0 L4.5,6.5 L6.75,0 L9,8"),
    ('x', "M0,8 L6,0 M6,8 L0,0"),
    ('y', "M0,8 L3.2,0 M6,8 L1.8,-4"),
    ('z', "M0,8 L6,8 L0,0 L6,0"),
    ('ı', "M0,8 L0,0"),
    // Punctuation and symbols.
    ('!', "M0,12 L0,3.5 D0,0.4"),
    ('"', "M0,12 L0,9 M2.5,12 L2.5,9"),
    ('#', "M2.5,0 L3.5,12 M5.5,0 L6.5,12 M0.5,4 L8,4 M1,8 L8.5,8"),
    ('$', "A4,8.7,3.8,2.7,20,270 A4,3.3,4,2.7,90,-160 M4,13 L4,-1"),
    ('%', "M8,12 L0,0 M2,12 A2,10,1.8,2,90,450 M6,4 A6,2,1.8,2,90,450"),
    ('&', "M8,0 L2.12,7.9 A3.6,9.6,2.1,2.4,225,-60 L1.75,5.97 A3.5,3.2,3.5,3.2,120,390 L8,6.5"),
    ('\'', "M0,12 L0,9"),
    ('(', "A5,6,5,7.5,120,240"),
    (')', "A-2.5,6,5,7.5,60,-60"),
    ('*', "M3,12 L3,6 M0.4,10.5 L5.6,7.5 M0.4,7.5 L5.6,10.5"),
    ('+', "M4,2 L4,10 M0,6 L8,6"),
    (',', "M1,1 L1,0 L0,-2"),
    ('-', "M0,6 L5,6"),
    ('.', "D0.4,0.4"),
    ('/', "M0,0 L7,12"),
    (':', "D0.4,0.4 D0.4,7.6"),
    (';', "D0.8,7.6 M1,1 L1,0 L0,-2"),
    ('<', "M8,10 L0,6 L8,2"),
    ('=', "M0,8 L8,8 M0,4 L8,4"),
    ('>', "M0,10 L8,6 L0,2"),
    ('?', "A3.5,8.5,3.5,3.5,150,-45 L3.5,4.5 L3.5,3 D3.5,0.4"),
    ('@', "A5,6,2,2,0,360 M7,8 L7,4.5 A8.25,4.5,1.25,1.25,180,360 L9.5,6 A5,6,4.5,6,0,300"),
    ('[', "M3,12.5 L0,12.5 L0,-0.5 L3,-0.5"),
    ('\\', "M0,12 L7,0"),
    (']', "M0,12.5 L3,12.5 L3,-0.5 L0,-0.5"),
    ('^', "M0,8 L3.5,12 L7,8"),
    ('_', "M0,-1.5 L8,-1.5"),
    ('`', "M0,12 L1.5,10"),
    ('{', "M3.5,12.5 L2.5,12 L2,11 L2,7.2 L0,6 L2,4.8 L2,1 L2.5,0 L3.5,-0.5"),
    ('|', "M0,13 L0,-1"),
    ('}', "M0,12.5 L1,12 L1.5,11 L1.5,7.2 L3.5,6 L1.5,4.8 L1.5,1 L1,0 L0,-0.5"),
    ('~', "M0,5.5 L1,6.5 L2.5,6.5 L4.5,5.5 L6,5.5 L7,6.5"),
    ('°', "A1.6,10.4,1.6,1.6,0,360"),
    ('±', "M3,5 L3,11 M0,8 L6,8 M0,2.5 L6,2.5"),
    ('⌀', "M0,0 L8,12 M4,10 A4,6,4,4,90,450"),
    ('×', "M0,3 L6,9 M6,3 L0,9"),
    ('÷', "M0,6 L7,6 D3.5,9 D3.5,3"),
    ('µ', "M0,8 L0,-4 M0,3 A3,3,3,3,180,360 M6,8 L6,0"),
    ('·', "D0.4,6"),
    ('–', "M0,6 L8,6"),
    ('—', "M0,6 L12,6"),
    ('€', "A5.5,8,4,4,40,180 L1.5,4 A5.5,4,4,4,180,320 M0,7 L6,7 M0,5 L6,5"),
    ('Ø', "M0,4 L0,8 A4,8,4,4,180,0 L8,4 A4,4,4,4,0,-180 M0,-0.5 L8,12.5"),
    ('ø', "A3,4,3,4,0,360 M0,-0.5 L6,8.5"),
];

/// Characters drawn with another character's glyph.
const SAME_AS: &[(char, char)] = &[
    ('\u{2018}', '\''),
    ('\u{2019}', '\''),
    ('\u{201C}', '"'),
    ('\u{201D}', '"'),
    ('\u{00A0}', ' '),
    ('\u{2212}', '-'),
    ('\u{00D7}', '×'),
    ('\u{2205}', '⌀'),
];

/// Accents, drawn centred on the base glyph: above the letter, or below
/// for the cedilla. Coordinates relative to the anchor.
const ACUTE: &str = "M-0.6,0 L1,1.8";
const GRAVE: &str = "M-1,1.8 L0.6,0";
const CIRCUMFLEX: &str = "M-1.8,0 L0,1.6 L1.8,0";
const DIAERESIS: &str = "D-1.4,0.6 D1.4,0.6";
const TILDE: &str = "M-2,0.3 L-1,1.2 L0,0.6 L1,0 L2,0.9";
const RING: &str = "A0,1,1,1,0,360";
const CEDILLA: &str = "M0,0 L0,-1 L1,-1.6 L0,-2.6";

/// Composed letters: (character, base, accent).
#[rustfmt::skip]
const COMPOSED: &[(char, char, &str)] = &[
    ('À', 'A', GRAVE), ('Á', 'A', ACUTE), ('Â', 'A', CIRCUMFLEX), ('Ã', 'A', TILDE),
    ('Ä', 'A', DIAERESIS), ('Å', 'A', RING), ('Ç', 'C', CEDILLA),
    ('È', 'E', GRAVE), ('É', 'E', ACUTE), ('Ê', 'E', CIRCUMFLEX), ('Ë', 'E', DIAERESIS),
    ('Ì', 'I', GRAVE), ('Í', 'I', ACUTE), ('Î', 'I', CIRCUMFLEX), ('Ï', 'I', DIAERESIS),
    ('Ñ', 'N', TILDE),
    ('Ò', 'O', GRAVE), ('Ó', 'O', ACUTE), ('Ô', 'O', CIRCUMFLEX), ('Õ', 'O', TILDE),
    ('Ö', 'O', DIAERESIS),
    ('Ù', 'U', GRAVE), ('Ú', 'U', ACUTE), ('Û', 'U', CIRCUMFLEX), ('Ü', 'U', DIAERESIS),
    ('Ý', 'Y', ACUTE),
    ('à', 'a', GRAVE), ('á', 'a', ACUTE), ('â', 'a', CIRCUMFLEX), ('ã', 'a', TILDE),
    ('ä', 'a', DIAERESIS), ('å', 'a', RING), ('ç', 'c', CEDILLA),
    ('è', 'e', GRAVE), ('é', 'e', ACUTE), ('ê', 'e', CIRCUMFLEX), ('ë', 'e', DIAERESIS),
    ('ì', 'ı', GRAVE), ('í', 'ı', ACUTE), ('î', 'ı', CIRCUMFLEX), ('ï', 'ı', DIAERESIS),
    ('ñ', 'n', TILDE),
    ('ò', 'o', GRAVE), ('ó', 'o', ACUTE), ('ô', 'o', CIRCUMFLEX), ('õ', 'o', TILDE),
    ('ö', 'o', DIAERESIS),
    ('ù', 'u', GRAVE), ('ú', 'u', ACUTE), ('û', 'u', CIRCUMFLEX), ('ü', 'u', DIAERESIS),
    ('ý', 'y', ACUTE), ('ÿ', 'y', DIAERESIS),
];

/// Superscript digits: (character, digit).
const SUPERSCRIPT: &[(char, char)] = &[('¹', '1'), ('²', '2'), ('³', '3')];

/// Box drawn for characters the font does not have.
const MISSING: &str = "M0,0 L0,12 L8,12 L8,0 L0,0";

/// One glyph: its strokes in glyph units and how far it moves the pen.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub advance: f64,
    pub strokes: Vec<Vec<Vec2>>,
}

impl Glyph {
    fn x_range(&self) -> Option<(f64, f64)> {
        let mut r: Option<(f64, f64)> = None;
        for p in self.strokes.iter().flatten() {
            r = Some(match r {
                None => (p.x, p.x),
                Some((a, b)) => (a.min(p.x), b.max(p.x)),
            });
        }
        r
    }
}

/// Points of an elliptical arc, `a0` to `a1` in degrees.
fn arc_points(c: Vec2, rx: f64, ry: f64, a0: f64, a1: f64) -> Vec<Vec2> {
    let sweep = a1 - a0;
    let n = ((sweep.abs() / 10.0).ceil() as usize).max(2);
    (0..=n)
        .map(|i| {
            let a = (a0 + sweep * i as f64 / n as f64).to_radians();
            Vec2::new(c.x + rx * a.cos(), c.y + ry * a.sin())
        })
        .collect()
}

/// Radius of a dot.
const DOT: f64 = 0.4;

/// Parse a glyph path. `None` if it does not follow the path language
/// (the tests check every built-in path).
fn parse(path: &str) -> Option<Vec<Vec<Vec2>>> {
    let mut strokes: Vec<Vec<Vec2>> = Vec::new();
    let mut cur: Vec<Vec2> = Vec::new();
    let flush = |cur: &mut Vec<Vec2>, strokes: &mut Vec<Vec<Vec2>>| {
        if cur.len() >= 2 {
            strokes.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    for tok in path.split_whitespace() {
        let mut chars = tok.chars();
        let cmd = chars.next()?;
        let nums: Vec<f64> = chars
            .as_str()
            .split(',')
            .map(|v| v.parse::<f64>().ok())
            .collect::<Option<Vec<f64>>>()?;
        match (cmd, nums.as_slice()) {
            ('M', &[x, y]) => {
                flush(&mut cur, &mut strokes);
                cur.push(Vec2::new(x, y));
            }
            ('L', &[x, y]) if !cur.is_empty() => cur.push(Vec2::new(x, y)),
            ('A', &[cx, cy, rx, ry, a0, a1]) => {
                let pts = arc_points(Vec2::new(cx, cy), rx, ry, a0, a1);
                let skip = cur.last().is_some_and(|p| p.distance(pts[0]) < 1e-6) as usize;
                cur.extend_from_slice(&pts[skip..]);
            }
            ('D', &[x, y]) => {
                flush(&mut cur, &mut strokes);
                strokes.push(arc_points(Vec2::new(x, y), DOT, DOT, 0.0, 360.0));
            }
            _ => return None,
        }
    }
    flush(&mut cur, &mut strokes);
    Some(strokes)
}

fn make(strokes: Vec<Vec<Vec2>>, advance: Option<f64>) -> Glyph {
    let mut g = Glyph {
        advance: 0.0,
        strokes,
    };
    g.advance = advance.unwrap_or_else(|| {
        g.x_range()
            .map(|(_, hi)| hi.max(0.0) + SPACING)
            .unwrap_or(SPACE_ADVANCE)
    });
    g
}

fn build() -> Vec<(char, Glyph)> {
    let mut out: Vec<(char, Glyph)> = Vec::new();
    for &(c, path) in GLYPHS {
        let strokes = parse(path).unwrap_or_default();
        let advance = c.is_ascii_digit().then_some(DIGIT_ADVANCE);
        out.push((c, make(strokes, advance)));
    }
    out.push((
        ' ',
        Glyph {
            advance: SPACE_ADVANCE,
            strokes: Vec::new(),
        },
    ));
    let find =
        |out: &[(char, Glyph)], c: char| out.iter().find(|(k, _)| *k == c).map(|(_, g)| g.clone());
    let mut extra: Vec<(char, Glyph)> = Vec::new();
    for &(c, base) in SAME_AS {
        if let Some(g) = find(&out, base) {
            extra.push((c, g));
        }
    }
    for &(c, base, accent) in COMPOSED {
        let (Some(g), Some(marks)) = (find(&out, base), parse(accent)) else {
            continue;
        };
        let (lo, hi) = g.x_range().unwrap_or((0.0, 0.0));
        let below = accent == CEDILLA;
        let top = if base.is_uppercase() { CAP } else { 8.0 };
        let anchor = Vec2::new((lo + hi) / 2.0, if below { 0.0 } else { top + 1.2 });
        let mut strokes = g.strokes.clone();
        strokes.extend(
            marks
                .into_iter()
                .map(|s| s.into_iter().map(|p| p + anchor).collect::<Vec<Vec2>>()),
        );
        extra.push((c, make(strokes, Some(g.advance))));
    }
    for &(c, digit) in SUPERSCRIPT {
        if let Some(g) = find(&out, digit) {
            let k = 0.55;
            let strokes = g
                .strokes
                .iter()
                .map(|s| {
                    s.iter()
                        .map(|p| Vec2::new(p.x * k, p.y * k + 6.6))
                        .collect()
                })
                .collect();
            extra.push((c, make(strokes, None)));
        }
    }
    out.extend(extra);
    out.sort_by_key(|(c, _)| *c);
    out.dedup_by_key(|(c, _)| *c);
    out
}

fn table() -> &'static [(char, Glyph)] {
    static TABLE: OnceLock<Vec<(char, Glyph)>> = OnceLock::new();
    TABLE.get_or_init(build)
}

fn missing() -> &'static Glyph {
    static BOX: OnceLock<Glyph> = OnceLock::new();
    BOX.get_or_init(|| make(parse(MISSING).unwrap_or_default(), None))
}

/// The glyph for a character, if the font has one.
pub fn glyph(c: char) -> Option<&'static Glyph> {
    let t = table();
    t.binary_search_by_key(&c, |(k, _)| *k)
        .ok()
        .map(|i| &t[i].1)
}

/// The glyph a character is drawn with: its own, a space for control
/// characters, a box for characters the font lacks.
fn glyph_or_box(c: char) -> &'static Glyph {
    let c = if c.is_control() { ' ' } else { c };
    glyph(c).unwrap_or_else(missing)
}

/// Width of single-line text at height 1 (cap height) and width factor 1:
/// from the left of the first glyph to the right of the last one's ink.
pub fn text_width(text: &str) -> f64 {
    let total: f64 = text.chars().map(|c| glyph_or_box(c).advance).sum();
    if total == 0.0 {
        0.0
    } else {
        (total - SPACING).max(0.0) / CAP
    }
}

/// How single-line text is placed: DXF's TEXT alignment groups 72 and 73.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// The alignment point (the insertion point for left/baseline text;
    /// the first point for aligned and fit text).
    pub position: Vec2,
    /// The second point, for aligned (72 = 3) and fit (72 = 5) text.
    pub fit_to: Option<Vec2>,
    /// Cap height.
    pub height: f64,
    pub width_factor: f64,
    /// Oblique angle, radians (slants glyphs to the right when positive).
    pub oblique: f64,
    /// Rotation of the baseline, radians.
    pub rotation: f64,
    /// 0 left, 1 centre, 2 right, 3 aligned, 4 middle, 5 fit.
    pub h_align: u16,
    /// 0 baseline, 1 bottom, 2 middle, 3 top.
    pub v_align: u16,
}

impl Placement {
    /// Left-aligned text on its baseline at `position`.
    pub fn at(position: Vec2, height: f64) -> Placement {
        Placement {
            position,
            fit_to: None,
            height,
            width_factor: 1.0,
            oblique: 0.0,
            rotation: 0.0,
            h_align: 0,
            v_align: 0,
        }
    }
}

/// The frame text is drawn in: where glyph-unit (0, 0) lands, the
/// direction of the baseline and the "up" direction, both scaled to world
/// units per glyph unit.
struct Frame {
    origin: Vec2,
    along: Vec2,
    up: Vec2,
}

fn frame(text: &str, p: &Placement) -> Frame {
    let w1 = text_width(text);
    let mut height = p.height;
    let mut wf = if p.width_factor > 0.0 {
        p.width_factor
    } else {
        1.0
    };
    let mut rotation = p.rotation;
    let mut start = p.position;
    let mut h = p.h_align;
    if let (3 | 5, Some(end)) = (p.h_align, p.fit_to) {
        let d = end - p.position;
        let len = d.length();
        if len > 1e-12 && w1 > 0.0 {
            rotation = d.angle();
            if p.h_align == 3 {
                // Aligned: the height follows the width.
                height = len / (w1 * wf);
            } else {
                // Fit: the width factor follows.
                wf = len / (w1 * height.max(1e-12));
            }
        }
        h = 0;
    }
    let width = w1 * height * wf;
    let dx = match h {
        1 | 4 => -width / 2.0,
        2 => -width,
        _ => 0.0,
    };
    let dy = match (h, p.v_align) {
        (4, _) | (_, 2) => -height / 2.0,
        (_, 1) => height * DESCENT / CAP,
        (_, 3) => -height,
        _ => 0.0,
    };
    let dir = Vec2::from_angle(rotation);
    let perp = dir.perp();
    start = start + dir * dx + perp * dy;
    let k = height / CAP;
    let slant = if p.oblique.is_finite() {
        p.oblique.tan().clamp(-10.0, 10.0)
    } else {
        0.0
    };
    Frame {
        origin: start,
        along: dir * (k * wf),
        up: perp * k + dir * (k * slant),
    }
}

/// The strokes of single-line text, in world coordinates.
pub fn layout(text: &str, p: &Placement) -> Vec<Vec<Vec2>> {
    let f = frame(text, p);
    let mut out = Vec::new();
    let mut pen = 0.0;
    for c in text.chars() {
        let g = glyph_or_box(c);
        for s in &g.strokes {
            out.push(
                s.iter()
                    .map(|q| f.origin + f.along * (q.x + pen) + f.up * q.y)
                    .collect(),
            );
        }
        pen += g.advance;
    }
    out
}

/// The corners of the box text occupies (from the descender to the cap
/// height), counter-clockwise from the bottom left.
pub fn text_box(text: &str, p: &Placement) -> [Vec2; 4] {
    let f = frame(text, p);
    let w = text_width(text) * CAP;
    let corner = |x: f64, y: f64| f.origin + f.along * x + f.up * y;
    [
        corner(0.0, -DESCENT),
        corner(w, -DESCENT),
        corner(w, CAP),
        corner(0.0, CAP),
    ]
}

/// Break a paragraph into lines no wider than `max_width` (at height 1),
/// at spaces. A word longer than the width gets a line of its own.
pub fn wrap(paragraph: &str, max_width: f64) -> Vec<String> {
    if max_width <= 0.0 || !max_width.is_finite() {
        return vec![paragraph.to_string()];
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in paragraph.split(' ') {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && text_width(&candidate) > max_width {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = candidate;
        }
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_parses_and_stays_in_its_box() {
        for &(c, path) in GLYPHS {
            let strokes = parse(path).unwrap_or_else(|| panic!("glyph {c:?} does not parse"));
            assert!(!strokes.is_empty(), "glyph {c:?} has no strokes");
            for p in strokes.iter().flatten() {
                assert!(
                    (-2.0..=12.5).contains(&p.x) && (-4.01..=13.01).contains(&p.y),
                    "glyph {c:?} point {p:?} outside the design box"
                );
            }
        }
        for accent in [
            ACUTE, GRAVE, CIRCUMFLEX, DIAERESIS, TILDE, RING, CEDILLA, MISSING,
        ] {
            assert!(parse(accent).is_some(), "{accent}");
        }
        assert!(parse("Q1,2").is_none());
        assert!(parse("M1").is_none());
        assert!(parse("L1,2").is_none(), "a line needs a start");
    }

    #[test]
    fn printable_ascii_and_drawing_symbols_are_covered() {
        for c in (0x20u8..0x7F).map(char::from) {
            assert!(glyph(c).is_some(), "{c:?} missing");
        }
        for c in "°±⌀×÷µ²³·–—€ØøÉéÑñÇçÜüÅå’”".chars() {
            assert!(glyph(c).is_some(), "{c:?} missing");
        }
        assert!(glyph('Ж').is_none());
        // Unknown characters still take room (a box).
        assert!(text_width("Ж") > 0.0);
    }

    #[test]
    fn widths_and_digits() {
        // Digits share one advance, so numbers line up in columns.
        for c in '0'..='9' {
            assert_eq!(glyph(c).unwrap().advance, DIGIT_ADVANCE, "{c}");
        }
        assert_eq!(text_width(""), 0.0);
        assert!((text_width("I") - 0.0).abs() < 1e-12);
        assert!((text_width("H") - 8.0 / CAP).abs() < 1e-12);
        assert!((text_width("HH") - 19.0 / CAP).abs() < 1e-12);
        assert!(text_width("WIDE TEXT") > text_width("WIDE"));
    }

    #[test]
    fn placement_and_alignment() {
        let close = |a: Vec2, b: Vec2| a.distance(b) < 1e-9;
        let text = "HH";
        let w = text_width(text) * 10.0;
        // Left, baseline: the box starts at the point.
        let b = text_box(text, &Placement::at(Vec2::new(5.0, 5.0), 10.0));
        assert!(close(b[0], Vec2::new(5.0, 5.0 - 10.0 * DESCENT / CAP)));
        assert!(close(b[2], Vec2::new(5.0 + w, 15.0)));
        // Centre/middle: centred on the point both ways (cap height).
        let p = Placement {
            h_align: 4,
            ..Placement::at(Vec2::ZERO, 10.0)
        };
        let b = text_box(text, &p);
        assert!(close(b[2], Vec2::new(w / 2.0, 5.0)));
        // Right/top.
        let p = Placement {
            h_align: 2,
            v_align: 3,
            ..Placement::at(Vec2::ZERO, 10.0)
        };
        let b = text_box(text, &p);
        assert!(close(b[2], Vec2::ZERO));
        // Rotated 90°: the baseline runs up.
        let p = Placement {
            rotation: std::f64::consts::FRAC_PI_2,
            ..Placement::at(Vec2::ZERO, 10.0)
        };
        let b = text_box(text, &p);
        assert!(close(b[1], Vec2::new(10.0 * DESCENT / CAP, w)));
        // Fit: stretched between two points at its own height.
        let p = Placement {
            h_align: 5,
            fit_to: Some(Vec2::new(100.0, 0.0)),
            ..Placement::at(Vec2::ZERO, 10.0)
        };
        let b = text_box(text, &p);
        assert!(close(b[2], Vec2::new(100.0, 10.0)));
        // Aligned: scaled to fit, height follows.
        let p = Placement {
            h_align: 3,
            fit_to: Some(Vec2::new(0.0, 2.0 * w)),
            ..Placement::at(Vec2::ZERO, 10.0)
        };
        let b = text_box(text, &p);
        assert!(close(b[2], Vec2::new(-20.0, 2.0 * w)));
        // Strokes stay inside the box.
        let strokes = layout("Rp|", &Placement::at(Vec2::ZERO, 12.0));
        for q in strokes.iter().flatten() {
            assert!(q.y >= -4.0 - 1e-9 && q.y <= 13.0 + 1e-9, "{q:?}");
        }
    }

    #[test]
    fn oblique_slants_to_the_right() {
        let p = Placement {
            oblique: 15f64.to_radians(),
            ..Placement::at(Vec2::ZERO, 12.0)
        };
        let strokes = layout("I", &p);
        let top = strokes[0].iter().fold(f64::MIN, |a, q| a.max(q.x));
        assert!((top - 12.0 * 15f64.to_radians().tan()).abs() < 1e-9);
    }

    #[test]
    fn wrapping() {
        let lines = wrap("BREAK ALL SHARP EDGES", text_width("BREAK ALL"));
        assert_eq!(lines, ["BREAK ALL", "SHARP", "EDGES"]);
        assert_eq!(wrap("ONE", 0.0), ["ONE"]);
        assert_eq!(wrap("VERYLONGWORD X", 1.0), ["VERYLONGWORD", "X"]);
    }
}

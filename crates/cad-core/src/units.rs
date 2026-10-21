//! Units: what a drawing's numbers mean, typed input, and display.
//!
//! A drawing stores coordinates in its own unit (`$INSUNITS`) and nothing
//! is converted on save. Units convert only at the edges: when a value is
//! typed with a unit (`10mm`, `3/8"` or `1'-2 1/2"` in a drawing of another
//! unit), when a value is displayed, and when one drawing is inserted into
//! another. See `docs/DESIGN.md` §8.
//!
//! Display formats are DXF's `$LUNITS` (1 scientific, 2 decimal, 3
//! engineering, 4 architectural, 5 fractional) with `$LUPREC` precision,
//! and `$AUNITS` / `$AUPREC` for angles.

use crate::model::Drawing;
use std::f64::consts::PI;
use std::fmt;

/// A length unit, as DWG and DXF number them in `$INSUNITS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    Unitless,
    Inch,
    Foot,
    Mile,
    Millimeter,
    Centimeter,
    Meter,
    Kilometer,
    Microinch,
    Mil,
    Yard,
    Angstrom,
    Nanometer,
    Micron,
    Decimeter,
    Decameter,
    Hectometer,
    Gigameter,
    AstronomicalUnit,
    LightYear,
    Parsec,
    UsSurveyFoot,
}

/// Units in `$INSUNITS` order.
const BY_CODE: [Unit; 22] = [
    Unit::Unitless,
    Unit::Inch,
    Unit::Foot,
    Unit::Mile,
    Unit::Millimeter,
    Unit::Centimeter,
    Unit::Meter,
    Unit::Kilometer,
    Unit::Microinch,
    Unit::Mil,
    Unit::Yard,
    Unit::Angstrom,
    Unit::Nanometer,
    Unit::Micron,
    Unit::Decimeter,
    Unit::Decameter,
    Unit::Hectometer,
    Unit::Gigameter,
    Unit::AstronomicalUnit,
    Unit::LightYear,
    Unit::Parsec,
    Unit::UsSurveyFoot,
];

/// Which base a unit is defined from. Conversions inside a family use the
/// family's own factors, so inch fractions stay exact between inches,
/// feet and yards.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    /// Factors in inches.
    Imperial,
    /// Factors in millimetres.
    Metric,
}

impl Unit {
    /// The unit with this `$INSUNITS` code (0–21).
    pub fn from_insunits(code: u16) -> Option<Unit> {
        BY_CODE.get(code as usize).copied()
    }

    /// The `$INSUNITS` code.
    pub fn insunits(self) -> u16 {
        BY_CODE.iter().position(|u| *u == self).unwrap_or(0) as u16
    }

    /// The unit a drawing is in: `$INSUNITS` when set, otherwise inches or
    /// millimetres from `$MEASUREMENT` (0 imperial, 1 metric), otherwise
    /// unitless.
    pub fn of(d: &Drawing) -> Unit {
        let h = &d.header;
        if let Some(u) = h.insunits.and_then(Unit::from_insunits) {
            if u != Unit::Unitless {
                return u;
            }
        }
        match h.measurement {
            Some(0) => Unit::Inch,
            Some(1) => Unit::Millimeter,
            _ => h
                .insunits
                .and_then(Unit::from_insunits)
                .unwrap_or(Unit::Unitless),
        }
    }

    /// Plural name, as the DXF reference names the `$INSUNITS` values.
    pub fn name(self) -> &'static str {
        crate::model::units_name(self.insunits())
    }

    /// Short symbol: `mm`, `in`, `ft` …; empty when unitless.
    pub fn symbol(self) -> &'static str {
        match self {
            Unit::Unitless => "",
            Unit::Inch => "in",
            Unit::Foot => "ft",
            Unit::Mile => "mi",
            Unit::Millimeter => "mm",
            Unit::Centimeter => "cm",
            Unit::Meter => "m",
            Unit::Kilometer => "km",
            Unit::Microinch => "µin",
            Unit::Mil => "mil",
            Unit::Yard => "yd",
            Unit::Angstrom => "Å",
            Unit::Nanometer => "nm",
            Unit::Micron => "µm",
            Unit::Decimeter => "dm",
            Unit::Decameter => "dam",
            Unit::Hectometer => "hm",
            Unit::Gigameter => "Gm",
            Unit::AstronomicalUnit => "au",
            Unit::LightYear => "ly",
            Unit::Parsec => "pc",
            Unit::UsSurveyFoot => "ft (US survey)",
        }
    }

    fn family_factor(self) -> Option<(Family, f64)> {
        use Family::*;
        Some(match self {
            Unit::Unitless => return None,
            Unit::Inch => (Imperial, 1.0),
            Unit::Foot => (Imperial, 12.0),
            Unit::Yard => (Imperial, 36.0),
            Unit::Mile => (Imperial, 63_360.0),
            Unit::Mil => (Imperial, 0.001),
            Unit::Microinch => (Imperial, 1e-6),
            // 1200/3937 m.
            Unit::UsSurveyFoot => (Imperial, 1_200_000.0 / 3937.0 / 25.4),
            Unit::Millimeter => (Metric, 1.0),
            Unit::Centimeter => (Metric, 10.0),
            Unit::Decimeter => (Metric, 100.0),
            Unit::Meter => (Metric, 1000.0),
            Unit::Decameter => (Metric, 1e4),
            Unit::Hectometer => (Metric, 1e5),
            Unit::Kilometer => (Metric, 1e6),
            Unit::Gigameter => (Metric, 1e12),
            Unit::Micron => (Metric, 1e-3),
            Unit::Nanometer => (Metric, 1e-6),
            Unit::Angstrom => (Metric, 1e-7),
            Unit::AstronomicalUnit => (Metric, 1.495_978_707e14),
            // 9 460 730 472 580 800 m.
            Unit::LightYear => (Metric, 9.460_730_472_580_8e18),
            // 648 000 / π au.
            Unit::Parsec => (Metric, 1.495_978_707e14 * 648_000.0 / PI),
        })
    }

    /// Millimetres in one of this unit; `None` when unitless.
    pub fn mm(self) -> Option<f64> {
        self.family_factor().map(|(f, k)| match f {
            Family::Imperial => k * 254.0 / 10.0,
            Family::Metric => k,
        })
    }

    /// True for inch-based units.
    pub fn is_imperial(self) -> bool {
        matches!(self.family_factor(), Some((Family::Imperial, _)))
    }

    /// True for metre-based units.
    pub fn is_metric(self) -> bool {
        matches!(self.family_factor(), Some((Family::Metric, _)))
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Convert a length between units. Converting between inches, feet and
/// yards, or between metric units, never goes through 25.4. `None` when
/// either unit is unitless (and they differ).
pub fn convert(value: f64, from: Unit, to: Unit) -> Option<f64> {
    if from == to {
        return Some(value);
    }
    let (ff, kf) = from.family_factor()?;
    let (ft, kt) = to.family_factor()?;
    Some(match (ff, ft) {
        _ if ff == ft => value * kf / kt,
        // × 254 / 10 rather than × 25.4 (and × 10 / 254 rather than
        // / 25.4): 25.4 has no exact binary value, so 12 × 25.4 is not
        // 304.8, while 12 × 254 / 10 is, and 76.2 mm comes back as 3 in.
        (Family::Imperial, _) => value * kf * 254.0 / 10.0 / kt,
        (Family::Metric, _) => value * kf * 10.0 / 254.0 / kt,
    })
}

// ---------------------------------------------------------------------------
// Typed input
// ---------------------------------------------------------------------------

/// Why typed input was not understood. The message is for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError(pub String);

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InputError {}

fn bad(msg: impl Into<String>) -> InputError {
    InputError(msg.into())
}

/// A tiny scanner over the typed text.
struct Scan<'a> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
}

impl<'a> Scan<'a> {
    fn new(text: &'a str) -> Self {
        Scan {
            s: text.as_bytes(),
            text,
            i: 0,
        }
    }
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn at_end(&mut self) -> bool {
        self.skip_ws();
        self.i >= self.s.len()
    }
    fn rest(&self) -> &'a str {
        &self.text[self.i..]
    }

    /// An unsigned decimal number: `12`, `12.5`, `.5`, `1e-3`.
    fn decimal(&mut self) -> Option<f64> {
        let start = self.i;
        let mut digits = 0;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
            digits += 1;
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            self.i = start;
            return None;
        }
        // Exponent, only when digits follow (so "2e" is not eaten).
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let save = self.i;
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
            } else {
                self.i = save;
            }
        }
        self.text[start..self.i].parse().ok()
    }

    /// An unsigned integer (for fractions).
    fn integer(&mut self) -> Option<f64> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        if self.i == start {
            return None;
        }
        self.text[start..self.i].parse().ok()
    }

    /// `N/D` at the current position, or nothing (position unchanged).
    fn fraction(&mut self) -> Result<Option<f64>, InputError> {
        let start = self.i;
        let Some(n) = self.integer() else {
            return Ok(None);
        };
        self.skip_ws();
        if !self.eat(b'/') {
            self.i = start;
            return Ok(None);
        }
        self.skip_ws();
        let d = self
            .integer()
            .ok_or_else(|| bad("a fraction needs a whole number below the line"))?;
        if d == 0.0 {
            return Err(bad("a fraction cannot have 0 below the line"));
        }
        Ok(Some(n / d))
    }

    /// An unsigned amount: `12.5`, `3/8`, `1 1/2` or `1-1/2`.
    fn amount(&mut self) -> Result<Option<f64>, InputError> {
        self.skip_ws();
        if let Some(f) = self.fraction()? {
            return Ok(Some(f));
        }
        let start = self.i;
        let Some(whole) = self.decimal() else {
            return Ok(None);
        };
        // A whole number can be followed by a fraction, after a space or a
        // hyphen: `1 1/2`, `1-1/2`.
        if self.text[start..self.i].bytes().all(|c| c.is_ascii_digit()) {
            let save = self.i;
            let spaced = self.peek().is_some_and(|c| c.is_ascii_whitespace());
            self.skip_ws();
            if spaced || self.eat(b'-') {
                self.skip_ws();
                if let Some(f) = self.fraction()? {
                    return Ok(Some(whole + f));
                }
            }
            self.i = save;
        }
        Ok(Some(whole))
    }

    /// A unit name or mark after a number, if any.
    fn unit(&mut self) -> Option<Unit> {
        self.skip_ws();
        if self.eat(b'"') {
            return Some(Unit::Inch);
        }
        if self.eat(b'\'') {
            return Some(Unit::Foot);
        }
        let rest = self.rest();
        let word: String = rest
            .chars()
            .take_while(|c| c.is_alphabetic() || *c == 'µ')
            .collect();
        if word.is_empty() {
            return None;
        }
        let u = match word.to_lowercase().as_str() {
            "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => Unit::Millimeter,
            "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => Unit::Centimeter,
            "dm" => Unit::Decimeter,
            "m" | "meter" | "meters" | "metre" | "metres" => Unit::Meter,
            "km" => Unit::Kilometer,
            "um" | "µm" | "micron" | "microns" => Unit::Micron,
            "nm" => Unit::Nanometer,
            "å" | "angstrom" | "angstroms" => Unit::Angstrom,
            "dam" => Unit::Decameter,
            "hm" => Unit::Hectometer,
            "gm" => Unit::Gigameter,
            "au" => Unit::AstronomicalUnit,
            "ly" => Unit::LightYear,
            "pc" => Unit::Parsec,
            "in" | "inch" | "inches" => Unit::Inch,
            "ft" | "foot" | "feet" => Unit::Foot,
            "usft" => Unit::UsSurveyFoot,
            "yd" | "yard" | "yards" => Unit::Yard,
            "mi" | "mile" | "miles" => Unit::Mile,
            "mil" | "mils" | "thou" => Unit::Mil,
            "µin" | "uin" | "microinch" | "microinches" => Unit::Microinch,
            _ => return None,
        };
        self.i += word.len();
        // The US survey foot as the display marks it: `ft (US survey)`.
        if u == Unit::Foot {
            let rest = self.rest();
            let after = rest.trim_start();
            if after
                .get(..11)
                .is_some_and(|t| t.eq_ignore_ascii_case("(us survey)"))
            {
                self.i += rest.len() - after.len() + 11;
                return Some(Unit::UsSurveyFoot);
            }
        }
        Some(u)
    }
}

/// Read a typed length and return it in the drawing's unit `drawing`.
///
/// Accepted forms: a plain number (`12.5`, `.5`, `1e-3`), a fraction
/// (`3/8`), a mixed number (`1 1/2` or `1-1/2`), any of those followed by a
/// unit (`10mm`, `2.5 cm`, `3/8"`, `1.5in`, `2ft`), and feet and inches
/// (`1'`, `1'-2"`, `1' 2-1/2"`, `1'2.5`). A leading `-` makes the whole
/// value negative. A number without a unit is in the drawing's unit.
pub fn parse_length(input: &str, drawing: Unit) -> Result<f64, InputError> {
    let mut s = Scan::new(input);
    s.skip_ws();
    let negative = if s.eat(b'-') {
        true
    } else {
        s.eat(b'+');
        false
    };
    let first = s
        .amount()?
        .ok_or_else(|| bad(format!("not a length: {:?}", input.trim())))?;
    let unit = s.unit();
    let (value, unit) = match unit {
        Some(Unit::Foot) => {
            // Feet, then optionally inches: `1'-2 1/2"`, `1' 2"`, `1'2`.
            s.skip_ws();
            let save = s.i;
            let dash = s.eat(b'-');
            let inches = match s.amount()? {
                Some(v) => {
                    // The inch mark is optional after feet.
                    s.skip_ws();
                    if !s.eat(b'"') && !matches!(s.unit(), Some(Unit::Inch) | None) {
                        return Err(bad("after feet, only inches can follow"));
                    }
                    v
                }
                None => {
                    if dash {
                        return Err(bad("a number of inches should follow the '-'"));
                    }
                    s.i = save;
                    0.0
                }
            };
            (first * 12.0 + inches, Unit::Inch)
        }
        Some(u) => (first, u),
        None => (first, drawing),
    };
    if !s.at_end() {
        return Err(bad(format!(
            "could not read {:?} in {:?}",
            s.rest().trim(),
            input.trim()
        )));
    }
    let value = if negative { -value } else { value };
    let finite = |v: f64| {
        if v.is_finite() {
            Ok(v)
        } else {
            Err(bad(format!("{:?} is too large a length", input.trim())))
        }
    };
    // A unitless drawing shows feet and inches as if its unit were the
    // inch, so what it shows can be typed back.
    if unit == drawing || (drawing == Unit::Unitless && unit == Unit::Inch) {
        return finite(value);
    }
    if drawing == Unit::Unitless {
        return Err(bad(
            "the drawing has no unit, so a unit cannot be converted; type a plain number",
        ));
    }
    let v = convert(value, unit, drawing)
        .ok_or_else(|| bad(format!("cannot convert {unit} to {drawing}")))?;
    finite(v)
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

/// Linear display format, as `$LUNITS` and `DIMLUNIT` number them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinearFormat {
    /// `1.5500E+01`
    Scientific,
    /// `15.50`
    Decimal,
    /// Feet and decimal inches: `1'-3.50"`
    Engineering,
    /// Feet and fractional inches: `1'-3 1/2"`
    Architectural,
    /// Fractions: `15 1/2`
    Fractional,
}

impl LinearFormat {
    /// From a `$LUNITS` value; anything unknown is decimal (6, "Windows
    /// desktop", included).
    pub fn from_lunits(v: u16) -> LinearFormat {
        match v {
            1 => LinearFormat::Scientific,
            3 => LinearFormat::Engineering,
            4 => LinearFormat::Architectural,
            5 => LinearFormat::Fractional,
            _ => LinearFormat::Decimal,
        }
    }

    pub fn lunits(self) -> u16 {
        match self {
            LinearFormat::Scientific => 1,
            LinearFormat::Decimal => 2,
            LinearFormat::Engineering => 3,
            LinearFormat::Architectural => 4,
            LinearFormat::Fractional => 5,
        }
    }
}

/// How lengths are shown.
#[derive(Debug, Clone, PartialEq)]
pub struct LengthFormat {
    pub format: LinearFormat,
    /// Decimal places (0–8), or for fractions the power of two of the
    /// smallest denominator (4 = sixteenths, 6 = sixty-fourths).
    pub precision: u16,
    /// Append the unit (` mm`, `"`): decimal, scientific and fractional
    /// formats only; feet-and-inches formats always carry their marks.
    pub unit_mark: bool,
    pub decimal_separator: char,
    /// Drop zeros after the decimal point (`12.50` → `12.5`, `3.00` → `3`).
    pub trim_zeros: bool,
}

impl Default for LengthFormat {
    fn default() -> Self {
        LengthFormat {
            format: LinearFormat::Decimal,
            precision: 4,
            unit_mark: false,
            decimal_separator: '.',
            trim_zeros: false,
        }
    }
}

impl LengthFormat {
    /// The drawing's own display format (`$LUNITS`, `$LUPREC`).
    pub fn of(d: &Drawing) -> LengthFormat {
        LengthFormat {
            format: LinearFormat::from_lunits(d.header.lunits.unwrap_or(2)),
            precision: d.header.luprec.unwrap_or(4).min(8),
            ..Default::default()
        }
    }
}

/// Fixed-point text with `places` decimals, `-0` shown as `0`.
fn fixed(v: f64, places: u16, sep: char, trim: bool) -> String {
    let places = places.min(16) as usize;
    let mut s = format!("{:.*}", places, v);
    if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
        s.remove(0);
    }
    if trim && s.contains('.') {
        let t = s.trim_end_matches('0').trim_end_matches('.');
        s = t.to_string();
    }
    if sep != '.' {
        s = s.replace('.', &sep.to_string());
    }
    s
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// `|v|` as a whole number and a reduced fraction of at most
/// 1/2^`precision`, rounded to the nearest one; `None` when `v` is too
/// large for fractions to mean anything (beyond 2^53 of the smallest one).
fn split_fraction(v: f64, precision: u16) -> Option<(u64, u64, u64)> {
    let den = 1u64 << precision.min(8);
    let n = (v.abs() * den as f64).round();
    if !(n.is_finite() && n <= 9_007_199_254_740_992.0) {
        return None;
    }
    let n = n as u64;
    let whole = n / den;
    let rem = n % den;
    if rem == 0 {
        return Some((whole, 0, 1));
    }
    let g = gcd(rem, den);
    Some((whole, rem / g, den / g))
}

/// `12 1/2`, `3/8`, `2`.
fn fraction_text(whole: u64, num: u64, den: u64) -> String {
    match (whole, num) {
        (w, 0) => w.to_string(),
        (0, n) => format!("{n}/{den}"),
        (w, n) => format!("{w} {n}/{den}"),
    }
}

/// `1.5500E+01`. Rust's own exponent formatting rounds (9.99995 →
/// 1.0000E+01) and handles the tiniest numbers.
fn scientific(v: f64, places: u16, sep: char) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    let text = format!("{:.*e}", places.min(16) as usize, v);
    let (mant, exp) = text.split_once('e').unwrap_or((&text, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut mant = mant.to_string();
    if mant.starts_with('-') && mant[1..].chars().all(|c| c == '0' || c == '.') {
        mant.remove(0);
    }
    if sep != '.' {
        mant = mant.replace('.', &sep.to_string());
    }
    format!("{mant}E{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
}

/// Format a length given in the drawing's unit `drawing`.
///
/// Engineering and architectural formats show feet and inches: the value
/// is converted to inches first (as is, when the drawing is unitless).
pub fn format_length(value: f64, drawing: Unit, f: &LengthFormat) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let mark = |s: String| -> String {
        if !f.unit_mark || drawing == Unit::Unitless {
            return s;
        }
        match drawing {
            Unit::Inch => format!("{s}\""),
            Unit::Foot => format!("{s}'"),
            u => format!("{s} {}", u.symbol()),
        }
    };
    match f.format {
        LinearFormat::Decimal => mark(fixed(value, f.precision, f.decimal_separator, f.trim_zeros)),
        LinearFormat::Scientific => mark(scientific(value, f.precision, f.decimal_separator)),
        LinearFormat::Fractional => match split_fraction(value, f.precision) {
            Some((w, n, d)) => {
                let body = fraction_text(w, n, d);
                let sign = if body == "0" { "" } else { sign };
                mark(format!("{sign}{body}"))
            }
            // Too large for fractions: whole units.
            None => mark(fixed(value, 0, f.decimal_separator, false)),
        },
        LinearFormat::Architectural | LinearFormat::Engineering => {
            let inches = if drawing == Unit::Unitless {
                value
            } else {
                convert(value, drawing, Unit::Inch).unwrap_or(value)
            };
            let a = inches.abs();
            // Too large to split into feet and inches: decimal inches.
            if !(a.is_finite() && a < 9e15) {
                return format!("{}\"", fixed(inches, 0, f.decimal_separator, false));
            }
            let (feet, rest) = if f.format == LinearFormat::Architectural {
                let Some((w, n, d)) = split_fraction(a, f.precision) else {
                    return format!("{}\"", fixed(inches, 0, f.decimal_separator, false));
                };
                let (feet, inch_whole) = (w / 12, w % 12);
                // Architectural text always shows the whole inches: 1'-0 3/8".
                let inch = if n == 0 {
                    inch_whole.to_string()
                } else {
                    format!("{inch_whole} {n}/{d}")
                };
                (feet, format!("{inch}\""))
            } else {
                // Round once, then split, so 11.9999 becomes 1'-0.00".
                let places = f.precision.min(8);
                let scale = 10f64.powi(places as i32);
                let total = (a * scale).round() / scale;
                let feet = (total / 12.0).floor();
                let inch = total - feet * 12.0;
                (
                    feet as u64,
                    format!("{}\"", fixed(inch, places, f.decimal_separator, false)),
                )
            };
            let zero = feet == 0
                && rest
                    .trim_end_matches('"')
                    .chars()
                    .all(|c| "0. ,".contains(c));
            let sign = if zero { "" } else { sign };
            format!("{sign}{feet}'-{rest}")
        }
    }
}

// ---------------------------------------------------------------------------
// Angles
// ---------------------------------------------------------------------------

/// Angle display format, as `$AUNITS` numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AngleFormat {
    /// `45.50°`
    Degrees,
    /// `45°30'00"`
    DegMinSec,
    /// `50.556g`
    Grads,
    /// `0.7854r`
    Radians,
}

impl AngleFormat {
    /// From `$AUNITS`; surveyor's units (4) show as degrees for now.
    pub fn from_aunits(v: u16) -> AngleFormat {
        match v {
            1 => AngleFormat::DegMinSec,
            2 => AngleFormat::Grads,
            3 => AngleFormat::Radians,
            _ => AngleFormat::Degrees,
        }
    }
}

/// Format an angle given in radians. `precision` is decimal places (for
/// degrees, minutes and seconds: 0 degrees only, 1–2 with minutes, 3–4
/// with seconds, more with decimal seconds).
pub fn format_angle(radians: f64, format: AngleFormat, precision: u16) -> String {
    match format {
        AngleFormat::Degrees => format!("{}°", fixed(radians.to_degrees(), precision, '.', false)),
        AngleFormat::Grads => format!("{}g", fixed(radians * 200.0 / PI, precision, '.', false)),
        AngleFormat::Radians => format!("{}r", fixed(radians, precision, '.', false)),
        AngleFormat::DegMinSec => {
            let deg = radians.to_degrees();
            let a = deg.abs();
            // The sign only when something non-zero is shown: no "-0°".
            let sign = |shown: f64| if deg < 0.0 && shown > 0.0 { "-" } else { "" };
            match precision {
                0 => format!("{}{}°", sign(a.round()), a.round()),
                1 | 2 => {
                    let total_min = (a * 60.0).round();
                    let (d, m) = ((total_min / 60.0).floor(), total_min % 60.0);
                    format!("{}{d}°{m:02}'", sign(total_min))
                }
                p => {
                    let places = p.saturating_sub(4);
                    let scale = 10f64.powi(places as i32);
                    let total_s = (a * 3600.0 * scale).round() / scale;
                    let sign = sign(total_s);
                    let d = (total_s / 3600.0).floor();
                    let m = ((total_s - d * 3600.0) / 60.0).floor();
                    let s = total_s - d * 3600.0 - m * 60.0;
                    let s = fixed(s, places, '.', false);
                    let s = if s.split('.').next().is_some_and(|w| w.len() < 2) {
                        format!("0{s}")
                    } else {
                        s
                    };
                    format!("{sign}{d}°{m:02}'{s}\"")
                }
            }
        }
    }
}

/// Read a typed angle and return radians. Degrees by default (`45`,
/// `45.5`, `45d`, `45°`, `45d30'15"`, `45°30'`); `0.5r` radians, `50g`
/// grads. A leading `-` makes it negative.
pub fn parse_angle(input: &str) -> Result<f64, InputError> {
    let mut s = Scan::new(input);
    s.skip_ws();
    let negative = s.eat(b'-');
    if !negative {
        s.eat(b'+');
    }
    s.skip_ws();
    let a = s
        .decimal()
        .ok_or_else(|| bad(format!("not an angle: {:?}", input.trim())))?;
    s.skip_ws();
    let rest = s.rest();
    let value = if let Some(r) = rest.strip_prefix('°').or_else(|| rest.strip_prefix('d')) {
        s.i = s.text.len() - r.len();
        // Optional minutes and seconds.
        let mut deg = a;
        s.skip_ws();
        if let Some(m) = s.decimal() {
            s.skip_ws();
            if !s.eat(b'\'') {
                return Err(bad("minutes need a ' mark"));
            }
            deg += m / 60.0;
            s.skip_ws();
            if let Some(sec) = s.decimal() {
                s.skip_ws();
                if !s.eat(b'"') {
                    return Err(bad("seconds need a \" mark"));
                }
                deg += sec / 3600.0;
            }
        }
        deg.to_radians()
    } else if s.eat(b'r') {
        a
    } else if s.eat(b'g') {
        a * PI / 200.0
    } else {
        a.to_radians()
    };
    if !s.at_end() {
        return Err(bad(format!(
            "could not read {:?} in {:?}",
            s.rest().trim(),
            input.trim()
        )));
    }
    if !value.is_finite() {
        return Err(bad(format!("{:?} is too large an angle", input.trim())));
    }
    Ok(if negative { -value } else { value })
}

// ---------------------------------------------------------------------------
// Dimension text
// ---------------------------------------------------------------------------

/// How a dimension shows its measurement (from a dimension style).
#[derive(Debug, Clone, PartialEq)]
pub struct DimTextFormat {
    pub format: LinearFormat,
    /// Decimal places, or the fraction precision (see [`LengthFormat`]).
    pub decimals: u16,
    /// Round measurements to a multiple of this (0: no rounding).
    pub rounding: f64,
    pub decimal_separator: char,
    /// DIMZIN bit 4: drop the zero before the decimal point (`.50`).
    pub suppress_leading_zero: bool,
    /// DIMZIN bit 8: drop zeros after the decimal point (`12.5`).
    pub suppress_trailing_zeros: bool,
    /// Alternate units shown in brackets: factor and decimal places.
    pub alternate: Option<(f64, u16)>,
    /// DIMPOST: text around the value, `<>` standing for it; without
    /// `<>` it is appended.
    pub post: String,
}

impl Default for DimTextFormat {
    fn default() -> Self {
        DimTextFormat {
            format: LinearFormat::Decimal,
            decimals: 2,
            rounding: 0.0,
            decimal_separator: '.',
            suppress_leading_zero: false,
            suppress_trailing_zeros: false,
            alternate: None,
            post: String::new(),
        }
    }
}

impl DimTextFormat {
    /// The format a dimension style describes.
    pub fn from_style(s: &crate::model::DimStyle) -> DimTextFormat {
        DimTextFormat {
            format: LinearFormat::from_lunits(s.unit_format),
            decimals: s.decimals.min(8),
            rounding: s.rounding.max(0.0),
            decimal_separator: s.decimal_separator,
            suppress_leading_zero: s.zero_suppression & 4 != 0,
            suppress_trailing_zeros: s.zero_suppression & 8 != 0,
            alternate: s.alt_units.then_some((s.alt_factor, s.alt_decimals.min(8))),
            post: s.post_text.clone(),
        }
    }
}

fn dim_number(v: f64, decimals: u16, f: &DimTextFormat, drawing: Unit) -> String {
    match f.format {
        LinearFormat::Decimal => {
            let mut s = fixed(v, decimals, f.decimal_separator, f.suppress_trailing_zeros);
            if f.suppress_leading_zero {
                if let Some(rest) = s.strip_prefix('0') {
                    if rest.starts_with(f.decimal_separator) {
                        s = rest.to_string();
                    }
                } else if let Some(rest) = s.strip_prefix("-0") {
                    if rest.starts_with(f.decimal_separator) {
                        s = format!("-{rest}");
                    }
                }
            }
            s
        }
        other => format_length(
            v,
            drawing,
            &LengthFormat {
                format: other,
                precision: decimals,
                unit_mark: false,
                decimal_separator: f.decimal_separator,
                trim_zeros: f.suppress_trailing_zeros,
            },
        ),
    }
}

/// The text a dimension shows. `measurement` is the measured value in
/// drawing units (already multiplied by DIMLFAC), `prefix` is `R` for
/// radii and `⌀` for diameters, `user_text` is the dimension's own text:
/// empty for the measurement, `<>` inside it standing for the measurement,
/// a single space for no text.
pub fn dimension_text(
    measurement: f64,
    prefix: &str,
    user_text: &str,
    f: &DimTextFormat,
    drawing: Unit,
) -> String {
    let v = if f.rounding > 0.0 {
        (measurement / f.rounding).round() * f.rounding
    } else {
        measurement
    };
    let mut value = format!("{prefix}{}", dim_number(v, f.decimals, f, drawing));
    // DIMPOST belongs to the primary value; the alternate (from the
    // measurement itself: DIMRND rounds the primary only) follows it.
    if !f.post.is_empty() {
        value = if f.post.contains("<>") {
            f.post.replace("<>", &value)
        } else {
            format!("{value}{}", f.post)
        };
    }
    if let Some((factor, places)) = f.alternate {
        let alt = DimTextFormat {
            format: LinearFormat::Decimal,
            ..f.clone()
        };
        value = format!(
            "{value} [{}]",
            dim_number(measurement * factor, places, &alt, drawing)
        );
    }
    match user_text {
        "" => value,
        " " => String::new(),
        t if t.contains("<>") => t.replace("<>", &value),
        t => t.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn insunits_codes_and_factors() {
        for code in 0..=21 {
            assert_eq!(Unit::from_insunits(code).unwrap().insunits(), code);
        }
        assert_eq!(Unit::from_insunits(22), None);
        assert_eq!(Unit::Inch.mm(), Some(25.4));
        assert_eq!(Unit::Foot.mm(), Some(304.8));
        assert_eq!(Unit::Meter.mm(), Some(1000.0));
        assert_eq!(Unit::Unitless.mm(), None);
        assert!(close(
            Unit::UsSurveyFoot.mm().unwrap(),
            304.800_609_601_219_2
        ));
        assert_eq!(Unit::Millimeter.name(), "millimeters");
        assert!(Unit::Mil.is_imperial() && Unit::Micron.is_metric());
    }

    #[test]
    fn conversions_stay_exact_inside_a_family() {
        // Feet to inches and back never touches 25.4.
        assert_eq!(convert(1.5, Unit::Foot, Unit::Inch), Some(18.0));
        assert_eq!(convert(0.125, Unit::Inch, Unit::Foot), Some(0.125 / 12.0));
        assert_eq!(convert(3.0, Unit::Yard, Unit::Foot), Some(9.0));
        assert_eq!(convert(0.125, Unit::Inch, Unit::Millimeter), Some(3.175));
        assert_eq!(convert(25.4, Unit::Millimeter, Unit::Inch), Some(1.0));
        assert_eq!(convert(2.5, Unit::Meter, Unit::Millimeter), Some(2500.0));
        assert_eq!(convert(1.0, Unit::Unitless, Unit::Inch), None);
        assert_eq!(convert(7.0, Unit::Unitless, Unit::Unitless), Some(7.0));
        // Metric to imperial is exact too: whole 64ths of an inch typed in
        // millimetres come back as those 64ths, up to 20 ft.
        assert_eq!(convert(76.2, Unit::Millimeter, Unit::Inch), Some(3.0));
        assert_eq!(convert(9.525, Unit::Millimeter, Unit::Inch), Some(0.375));
        assert_eq!(convert(304.8, Unit::Millimeter, Unit::Foot), Some(1.0));
        for k in 0..=(20 * 12 * 64) {
            let inches = k as f64 / 64.0;
            let mm = convert(inches, Unit::Inch, Unit::Millimeter).unwrap();
            assert_eq!(convert(mm, Unit::Millimeter, Unit::Inch), Some(inches));
        }
    }

    #[test]
    fn typed_lengths() {
        let mm = Unit::Millimeter;
        let inch = Unit::Inch;
        let ok = |s: &str, u: Unit, v: f64| {
            let got = parse_length(s, u).unwrap_or_else(|e| panic!("{s:?}: {e}"));
            assert!(close(got, v), "{s:?} in {u}: {got} != {v}");
        };
        ok("12.5", mm, 12.5);
        ok(" .5 ", mm, 0.5);
        ok("1e-3", mm, 0.001);
        ok("-2", mm, -2.0);
        ok("3/8", inch, 0.375);
        ok("1 1/2", inch, 1.5);
        ok("1-1/2", inch, 1.5);
        ok("-1 1/2", inch, -1.5);
        ok("10mm", inch, 10.0 / 25.4);
        ok("10 mm", mm, 10.0);
        ok("2.5cm", mm, 25.0);
        ok("1m", mm, 1000.0);
        ok("3/8\"", mm, 9.525);
        ok("3/8in", mm, 9.525);
        ok("1.5 inches", mm, 38.1);
        ok("2ft", inch, 24.0);
        ok("1'", inch, 12.0);
        ok("1'-2\"", inch, 14.0);
        ok("1'-2 1/2\"", inch, 14.5);
        ok("1' 2-1/2\"", inch, 14.5);
        ok("1'2.5", inch, 14.5);
        ok("1'-0 3/8\"", inch, 12.375);
        ok("-1'-6\"", inch, -18.0);
        ok("1'-2 1/2\"", mm, 368.3);
        ok("5", Unit::Foot, 5.0);
        ok("6\"", Unit::Foot, 0.5);
        ok("1 mil", inch, 0.001);
        ok("+4", mm, 4.0);
        ok("7", Unit::Unitless, 7.0);
    }

    #[test]
    fn typed_lengths_that_are_refused() {
        for (s, u) in [
            ("", Unit::Millimeter),
            ("abc", Unit::Millimeter),
            ("1/0", Unit::Inch),
            ("3/", Unit::Inch),
            ("12 apples", Unit::Millimeter),
            ("1'-", Unit::Inch),
            ("1'-2mm", Unit::Inch),
            ("10mm", Unit::Unitless),
            ("1 2", Unit::Millimeter),
            ("--1", Unit::Millimeter),
            // Numbers too large to be lengths, in any form.
            ("1e999", Unit::Millimeter),
            ("1e308'", Unit::Inch),
            ("1e305km", Unit::Millimeter),
            (&format!("{0}/{0}", "9".repeat(400)), Unit::Inch),
        ] {
            assert!(parse_length(s, u).is_err(), "{s:?} should be refused");
        }
        assert!(parse_angle("1e999").is_err());
    }

    #[test]
    fn display_formats() {
        let f = |format, precision| LengthFormat {
            format,
            precision,
            ..Default::default()
        };
        let inch = Unit::Inch;
        use LinearFormat::*;
        assert_eq!(format_length(15.5, inch, &f(Decimal, 2)), "15.50");
        assert_eq!(format_length(-0.0001, inch, &f(Decimal, 2)), "0.00");
        assert_eq!(format_length(15.5, inch, &f(Scientific, 4)), "1.5500E+01");
        assert_eq!(format_length(0.00155, inch, &f(Scientific, 2)), "1.55E-03");
        assert_eq!(
            format_length(9.99996, inch, &f(Scientific, 4)),
            "1.0000E+01"
        );
        assert_eq!(format_length(14.5, inch, &f(Fractional, 4)), "14 1/2");
        assert_eq!(format_length(0.375, inch, &f(Fractional, 6)), "3/8");
        assert_eq!(format_length(-1.25, inch, &f(Fractional, 4)), "-1 1/4");
        assert_eq!(format_length(2.999, inch, &f(Fractional, 4)), "3");
        assert_eq!(
            format_length(14.5, inch, &f(Architectural, 4)),
            "1'-2 1/2\""
        );
        assert_eq!(format_length(6.0, inch, &f(Architectural, 4)), "0'-6\"");
        assert_eq!(format_length(12.0, inch, &f(Architectural, 4)), "1'-0\"");
        assert_eq!(format_length(11.999, inch, &f(Architectural, 4)), "1'-0\"");
        assert_eq!(format_length(-18.0, inch, &f(Architectural, 4)), "-1'-6\"");
        assert_eq!(
            format_length(12.375, inch, &f(Architectural, 4)),
            "1'-0 3/8\""
        );
        assert_eq!(format_length(14.5, inch, &f(Engineering, 2)), "1'-2.50\"");
        assert_eq!(
            format_length(11.9999, inch, &f(Engineering, 2)),
            "1'-0.00\""
        );
        // A millimetre drawing shown in feet and inches converts first.
        assert_eq!(
            format_length(368.3, Unit::Millimeter, &f(Architectural, 4)),
            "1'-2 1/2\""
        );
        let marked = LengthFormat {
            unit_mark: true,
            ..f(Decimal, 2)
        };
        assert_eq!(format_length(25.4, Unit::Millimeter, &marked), "25.40 mm");
        assert_eq!(format_length(1.0, inch, &marked), "1.00\"");
        let comma = LengthFormat {
            decimal_separator: ',',
            trim_zeros: true,
            ..f(Decimal, 3)
        };
        assert_eq!(format_length(12.5, Unit::Millimeter, &comma), "12,5");
        assert_eq!(format_length(3.0, Unit::Millimeter, &comma), "3");
        // Values beyond what a format can show are still shown.
        assert_eq!(
            format_length(-5e16, inch, &f(Fractional, 8)),
            "-50000000000000000"
        );
        assert_eq!(
            format_length(1e17, inch, &f(Architectural, 8)),
            "100000000000000000\""
        );
        assert_eq!(format_length(f64::INFINITY, inch, &f(Scientific, 4)), "inf");
        assert_eq!(format_length(f64::NAN, inch, &f(Decimal, 2)), "NaN");
        assert_eq!(format_length(1e-310, inch, &f(Scientific, 2)), "1.00E-310");
        assert_eq!(format_length(-0.0, inch, &f(Scientific, 2)), "0.00E+00");
    }

    #[test]
    fn typed_values_read_back_from_display() {
        // What is shown can be typed back to the same value (at the shown
        // precision).
        for v in [0.0, 0.5, 14.5, 12.375, 100.0625, 36.0] {
            for format in [
                LinearFormat::Decimal,
                LinearFormat::Fractional,
                LinearFormat::Architectural,
                LinearFormat::Engineering,
            ] {
                let f = LengthFormat {
                    format,
                    precision: 4,
                    ..Default::default()
                };
                let shown = format_length(v, Unit::Inch, &f);
                let back =
                    parse_length(&shown, Unit::Inch).unwrap_or_else(|e| panic!("{shown}: {e}"));
                assert!(close(back, v), "{v} shown as {shown} reads back as {back}");
                // A unitless drawing shows and reads feet and inches too.
                let shown = format_length(v, Unit::Unitless, &f);
                let back =
                    parse_length(&shown, Unit::Unitless).unwrap_or_else(|e| panic!("{shown}: {e}"));
                assert!(close(back, v), "unitless {v} as {shown} reads {back}");
            }
        }
        // Every unit's mark reads back as that unit.
        let marked = LengthFormat {
            unit_mark: true,
            ..Default::default()
        };
        for code in 1..=21 {
            let u = Unit::from_insunits(code).unwrap();
            let shown = format_length(2.5, u, &marked);
            let back = parse_length(&shown, u).unwrap_or_else(|e| panic!("{u}: {shown}: {e}"));
            assert!(close(back, 2.5), "{u}: {shown} reads back as {back}");
        }
    }

    #[test]
    fn angles() {
        let d = |deg: f64| deg.to_radians();
        assert_eq!(format_angle(d(45.5), AngleFormat::Degrees, 2), "45.50°");
        assert_eq!(format_angle(d(45.5), AngleFormat::DegMinSec, 2), "45°30'");
        assert_eq!(
            format_angle(
                d(45.0 + 30.0 / 60.0 + 15.0 / 3600.0),
                AngleFormat::DegMinSec,
                4
            ),
            "45°30'15\""
        );
        assert_eq!(format_angle(PI / 2.0, AngleFormat::Grads, 1), "100.0g");
        assert_eq!(format_angle(1.0, AngleFormat::Radians, 4), "1.0000r");
        for (s, v) in [
            ("45", 45.0),
            ("45.5", 45.5),
            ("45d", 45.0),
            ("45°", 45.0),
            ("45d30'15\"", 45.0 + 30.0 / 60.0 + 15.0 / 3600.0),
            ("45°30'", 45.5),
            ("-90", -90.0),
            ("100g", 90.0),
        ] {
            let got = parse_angle(s).unwrap_or_else(|e| panic!("{s}: {e}"));
            assert!(close(got, d(v)), "{s}: {got}");
        }
        assert!(close(parse_angle("0.5r").unwrap(), 0.5));
        assert!(parse_angle("north").is_err());
        assert!(parse_angle("45d30").is_err());
        // No "-0" once rounded away (0.36" rounds to no seconds), but a
        // sign on what still shows.
        for p in [0, 2, 4] {
            let shown = format_angle(d(-0.0001), AngleFormat::DegMinSec, p);
            assert!(!shown.starts_with('-'), "{shown}");
        }
        assert_eq!(
            format_angle(d(-0.001), AngleFormat::DegMinSec, 4),
            "-0°00'04\""
        );
        assert_eq!(format_angle(d(-30.5), AngleFormat::DegMinSec, 2), "-30°30'");
    }

    #[test]
    fn dimension_texts() {
        let mm = Unit::Millimeter;
        let f = DimTextFormat::default();
        assert_eq!(dimension_text(25.4, "", "", &f, mm), "25.40");
        assert_eq!(dimension_text(13.0, "⌀", "", &f, mm), "⌀13.00");
        assert_eq!(dimension_text(6.5, "R", "", &f, mm), "R6.50");
        assert_eq!(
            dimension_text(13.0, "⌀", "4X <> THRU", &f, mm),
            "4X ⌀13.00 THRU"
        );
        assert_eq!(dimension_text(13.0, "", " ", &f, mm), "");
        assert_eq!(dimension_text(13.0, "", "SEE NOTE", &f, mm), "SEE NOTE");
        // Dual units, as in DESIGN.md: 25.40 [1.000].
        let dual = DimTextFormat {
            alternate: Some((1.0 / 25.4, 3)),
            ..Default::default()
        };
        assert_eq!(dimension_text(25.4, "", "", &dual, mm), "25.40 [1.000]");
        let zin = DimTextFormat {
            suppress_leading_zero: true,
            suppress_trailing_zeros: true,
            decimals: 3,
            ..Default::default()
        };
        assert_eq!(dimension_text(0.5, "", "", &zin, Unit::Inch), ".5");
        assert_eq!(dimension_text(2.0, "", "", &zin, Unit::Inch), "2");
        let rounded = DimTextFormat {
            rounding: 0.5,
            post: "<> TYP".into(),
            ..Default::default()
        };
        assert_eq!(dimension_text(12.3, "", "", &rounded, mm), "12.50 TYP");
        // DIMPOST follows the primary value only; the alternate comes from
        // the measurement, not from the rounded primary.
        let both = DimTextFormat {
            rounding: 5.0,
            post: "<> mm".into(),
            alternate: Some((1.0 / 25.4, 3)),
            ..Default::default()
        };
        assert_eq!(dimension_text(12.3, "", "", &both, mm), "10.00 mm [0.484]");
        let arch = DimTextFormat {
            format: LinearFormat::Architectural,
            decimals: 4,
            ..Default::default()
        };
        assert_eq!(
            dimension_text(14.5, "", "", &arch, Unit::Inch),
            "1'-2 1/2\""
        );
    }
}

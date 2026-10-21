//! Lineweights.
//!
//! From R2000, DWG stores a lineweight as an index into a fixed table of
//! standard weights, plus three special values (by layer, by block, the
//! drawing's default). DXF writes the weight itself, in hundredths of a
//! millimetre (group 370), with -1, -2 and -3 for the special values.
//!
//! The model keeps the DWG index (the *code*), exactly as read, in
//! [`EntityCommon::lineweight`](super::EntityCommon::lineweight),
//! [`Layer::lineweight`](super::Layer::lineweight) and the dimension style.
//! [`Lineweight`] interprets it.
//!
//! The standard weights are the ones the DXF reference lists for group 370.
//! That DWG stores an index follows from the specification: a layer keeps
//! its lineweight in five bits of its flag word (ODA spec §20.4.54, mask
//! 0x03E0), too few for the weights themselves. The order of the index
//! table and the codes of the special values are from knowledge, not from
//! the specification text, which does not list them. VERIFY against real
//! files.

use std::fmt;

/// The standard weights in hundredths of a millimetre, in index order.
pub const STANDARD: [u16; 24] = [
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200,
    211,
];

/// A lineweight as the drawing means it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lineweight {
    ByLayer,
    ByBlock,
    /// The drawing's default weight (`$LWDEFAULT`, 0.25 mm unless set).
    Default,
    /// One of the [`STANDARD`] weights, in hundredths of a millimetre.
    Hundredths(u16),
}

impl Lineweight {
    /// DWG code for "by layer".
    pub const CODE_BY_LAYER: u8 = 29;
    /// DWG code for "by block".
    pub const CODE_BY_BLOCK: u8 = 30;
    /// DWG code for "the drawing's default".
    pub const CODE_DEFAULT: u8 = 31;
    /// The weight `Default` stands for: AutoCAD's own default for
    /// `$LWDEFAULT`, 0.25 mm.
    pub const DEFAULT_HUNDREDTHS: u16 = 25;

    /// Interpret a DWG code. Codes outside the table (24–28, 32 and up)
    /// are read as the default weight; the code itself stays in the model
    /// unchanged.
    pub fn from_code(code: u8) -> Lineweight {
        match code {
            0..=23 => Lineweight::Hundredths(STANDARD[code as usize]),
            Self::CODE_BY_LAYER => Lineweight::ByLayer,
            Self::CODE_BY_BLOCK => Lineweight::ByBlock,
            _ => Lineweight::Default,
        }
    }

    /// The DWG code. A weight that is not one of the standard ones takes
    /// the code of the nearest standard weight.
    pub fn code(self) -> u8 {
        match self {
            Lineweight::ByLayer => Self::CODE_BY_LAYER,
            Lineweight::ByBlock => Self::CODE_BY_BLOCK,
            Lineweight::Default => Self::CODE_DEFAULT,
            Lineweight::Hundredths(h) => nearest_index(h as f64) as u8,
        }
    }

    /// Interpret a DXF group 370 value: -1 by layer, -2 by block, -3
    /// default, otherwise hundredths of a millimetre (rounded to the
    /// nearest standard weight).
    pub fn from_dxf(v: i16) -> Lineweight {
        match v {
            -1 => Lineweight::ByLayer,
            -2 => Lineweight::ByBlock,
            v if v >= 0 => Lineweight::Hundredths(STANDARD[nearest_index(v as f64)]),
            _ => Lineweight::Default,
        }
    }

    /// The DXF group 370 value.
    pub fn dxf(self) -> i16 {
        match self {
            Lineweight::ByLayer => -1,
            Lineweight::ByBlock => -2,
            Lineweight::Default => -3,
            Lineweight::Hundredths(h) => STANDARD[nearest_index(h as f64)] as i16,
        }
    }

    /// The standard weight nearest to `mm` millimetres.
    pub fn from_mm(mm: f64) -> Lineweight {
        let h = if mm.is_finite() { mm * 100.0 } else { 0.0 };
        Lineweight::Hundredths(STANDARD[nearest_index(h)])
    }

    /// The weight in millimetres, if it is a weight (not by layer, by
    /// block or default).
    pub fn mm(self) -> Option<f64> {
        match self {
            Lineweight::Hundredths(h) => Some(h as f64 / 100.0),
            _ => None,
        }
    }

    /// Millimetres to draw with: `Default` (and an unresolved by-layer or
    /// by-block weight) counts as 0.25 mm.
    pub fn effective_mm(self) -> f64 {
        self.mm().unwrap_or(Self::DEFAULT_HUNDREDTHS as f64 / 100.0)
    }

    /// Replace "by layer" and "by block" with what they refer to.
    pub fn resolve(self, layer: Lineweight, block: Lineweight) -> Lineweight {
        let pick = |w: Lineweight| match w {
            Lineweight::ByLayer | Lineweight::ByBlock => Lineweight::Default,
            w => w,
        };
        match self {
            Lineweight::ByLayer => pick(layer),
            Lineweight::ByBlock => pick(block),
            w => w,
        }
    }
}

/// Index of the standard weight nearest to `hundredths` (ties go to the
/// heavier weight).
fn nearest_index(hundredths: f64) -> usize {
    let mut best = 0;
    let mut best_d = f64::INFINITY;
    for (i, &w) in STANDARD.iter().enumerate() {
        let d = (w as f64 - hundredths).abs();
        if d <= best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

impl fmt::Display for Lineweight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lineweight::ByLayer => f.write_str("ByLayer"),
            Lineweight::ByBlock => f.write_str("ByBlock"),
            Lineweight::Default => f.write_str("Default"),
            Lineweight::Hundredths(h) => write!(f, "{}.{:02} mm", h / 100, h % 100),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for code in 0u8..=23 {
            assert_eq!(Lineweight::from_code(code).code(), code);
        }
        for code in [29u8, 30, 31] {
            assert_eq!(Lineweight::from_code(code).code(), code);
        }
        assert_eq!(Lineweight::from_code(11), Lineweight::Hundredths(50));
        assert_eq!(Lineweight::from_code(7), Lineweight::Hundredths(25));
        assert_eq!(Lineweight::from_code(26), Lineweight::Default);
        assert_eq!(Lineweight::from_code(255), Lineweight::Default);
    }

    #[test]
    fn dxf_values_and_millimetres() {
        assert_eq!(Lineweight::from_dxf(-1), Lineweight::ByLayer);
        assert_eq!(Lineweight::from_dxf(-2), Lineweight::ByBlock);
        assert_eq!(Lineweight::from_dxf(-3), Lineweight::Default);
        assert_eq!(Lineweight::from_dxf(50).dxf(), 50);
        // Not a standard weight: the nearest one.
        assert_eq!(Lineweight::from_dxf(48), Lineweight::Hundredths(50));
        assert_eq!(Lineweight::from_mm(0.5), Lineweight::Hundredths(50));
        assert_eq!(Lineweight::from_mm(0.6), Lineweight::Hundredths(60));
        assert_eq!(Lineweight::from_mm(0.3), Lineweight::Hundredths(30));
        assert_eq!(Lineweight::from_mm(9.0), Lineweight::Hundredths(211));
        assert_eq!(Lineweight::from_mm(f64::NAN), Lineweight::Hundredths(0));
        assert_eq!(Lineweight::Hundredths(50).to_string(), "0.50 mm");
        assert_eq!(Lineweight::Hundredths(211).to_string(), "2.11 mm");
        assert_eq!(Lineweight::Default.effective_mm(), 0.25);
    }

    #[test]
    fn resolving() {
        let l = Lineweight::Hundredths(50);
        let b = Lineweight::Hundredths(35);
        assert_eq!(Lineweight::ByLayer.resolve(l, b), l);
        assert_eq!(Lineweight::ByBlock.resolve(l, b), b);
        assert_eq!(
            Lineweight::Hundredths(18).resolve(l, b),
            Lineweight::Hundredths(18)
        );
        assert_eq!(
            Lineweight::ByLayer.resolve(Lineweight::ByBlock, b),
            Lineweight::Default
        );
    }
}

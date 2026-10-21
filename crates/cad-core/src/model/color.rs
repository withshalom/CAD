//! Colors: AutoCAD Color Index (ACI) values, true colors and by-layer /
//! by-block references.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    ByLayer,
    ByBlock,
    /// ACI 1–255.
    Index(u8),
    /// True color.
    Rgb(u8, u8, u8),
    /// Explicitly no color (rare; some table defaults).
    None,
}

impl Color {
    /// Interpret an ACI number: 0 = BYBLOCK, 256 = BYLAYER, 1–255 = index.
    /// Negative values (a switched-off layer's color) use the magnitude.
    pub fn from_aci(value: i16) -> Color {
        match value {
            0 => Color::ByBlock,
            256 | 257 => Color::ByLayer,
            v if v < 0 => Color::from_aci(v.checked_neg().unwrap_or(7)),
            v if (1..=255).contains(&v) => Color::Index(v as u8),
            _ => Color::ByLayer,
        }
    }

    /// Decode the 32-bit "color method + RGB" word used by R2004+ CmColor:
    /// the high byte selects the method (0xC0 by layer, 0xC1 by block,
    /// 0xC2 true color, 0xC3 ACI in the low byte, 0xC8 none).
    pub fn from_method_word(word: u32, fallback_index: i16) -> Color {
        let method = (word >> 24) as u8;
        match method {
            0xC0 => Color::ByLayer,
            0xC1 => Color::ByBlock,
            0xC2 => Color::Rgb((word >> 16) as u8, (word >> 8) as u8, word as u8),
            0xC3 => Color::from_aci((word & 0xFF) as i16),
            0xC8 => Color::None,
            _ => Color::from_aci(fallback_index),
        }
    }

    /// The "color method + RGB" word ([`Color::from_method_word`] reads it
    /// back): 0xC0 by layer, 0xC1 by block, 0xC2 true color, 0xC3 index,
    /// 0xC8 none. Index 0 is ACI's "by block" and is written as such.
    pub fn to_method_word(self) -> u32 {
        match self {
            Color::ByLayer => 0xC000_0000,
            Color::ByBlock | Color::Index(0) => 0xC100_0000,
            Color::Rgb(r, g, b) => 0xC200_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32,
            Color::Index(i) => 0xC300_0000 | i as u32,
            Color::None => 0xC800_0000,
        }
    }

    /// Resolve to RGB for display, given the color the ByLayer/ByBlock
    /// reference points at.
    pub fn resolve(self, by_layer: Color, by_block: Color) -> (u8, u8, u8) {
        match self {
            Color::ByLayer => match by_layer {
                Color::ByLayer | Color::ByBlock | Color::None => aci_rgb(7),
                c => c.resolve(Color::Index(7), Color::Index(7)),
            },
            Color::ByBlock => match by_block {
                Color::ByLayer | Color::ByBlock | Color::None => aci_rgb(7),
                c => c.resolve(Color::Index(7), Color::Index(7)),
            },
            Color::Index(i) => aci_rgb(i),
            Color::Rgb(r, g, b) => (r, g, b),
            Color::None => aci_rgb(7),
        }
    }

    /// Nearest ACI index, for formats without true color.
    pub fn to_aci(self) -> i16 {
        match self {
            Color::ByLayer => 256,
            Color::ByBlock => 0,
            Color::Index(i) => i as i16,
            Color::Rgb(r, g, b) => nearest_aci(r, g, b) as i16,
            Color::None => 7,
        }
    }
}

fn hsv_to_rgb(h_deg: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let v255 = v * 255.0;
    let c = v255 * s;
    let hp = (h_deg / 60.0) % 6.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v255 - c;
    let f = |t: f64| (t + m).floor().clamp(0.0, 255.0) as u8;
    (f(r1), f(g1), f(b1))
}

/// RGB for an ACI index. Indices 10–249 are generated from the standard
/// layout: 24 hues 15° apart, five brightness levels, each followed by a
/// half-saturated tint.
pub fn aci_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0 => (0, 0, 0),
        1 => (255, 0, 0),
        2 => (255, 255, 0),
        3 => (0, 255, 0),
        4 => (0, 255, 255),
        5 => (0, 0, 255),
        6 => (255, 0, 255),
        7 => (255, 255, 255),
        8 => (128, 128, 128),
        9 => (192, 192, 192),
        10..=249 => {
            let i = index as u32 - 10;
            let hue = (i / 10) as f64 * 15.0;
            let k = i % 10;
            let level = [1.0, 0.8, 0.6, 0.5, 0.3][(k / 2) as usize];
            let sat = if k & 1 == 0 { 1.0 } else { 0.5 };
            hsv_to_rgb(hue, sat, level)
        }
        250 => (51, 51, 51),
        251 => (91, 91, 91),
        252 => (132, 132, 132),
        253 => (173, 173, 173),
        254 => (214, 214, 214),
        255 => (255, 255, 255),
    }
}

pub fn nearest_aci(r: u8, g: u8, b: u8) -> u8 {
    let mut best = 7u8;
    let mut best_d = u32::MAX;
    for i in 1..=255u8 {
        let (ar, ag, ab) = aci_rgb(i);
        let d = (ar as i32 - r as i32).pow(2) as u32
            + (ag as i32 - g as i32).pow(2) as u32
            + (ab as i32 - b as i32).pow(2) as u32;
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_spot_checks() {
        assert_eq!(aci_rgb(10), (255, 0, 0));
        assert_eq!(aci_rgb(11), (255, 127, 127));
        assert_eq!(aci_rgb(12), (204, 0, 0));
        assert_eq!(aci_rgb(15), (153, 76, 76));
        assert_eq!(aci_rgb(18), (76, 0, 0));
        assert_eq!(nearest_aci(250, 5, 5), 1);
    }

    #[test]
    fn method_word() {
        assert_eq!(
            Color::from_method_word(0xC2FF8000, 0),
            Color::Rgb(255, 128, 0)
        );
        assert_eq!(Color::from_method_word(0xC3000005, 0), Color::Index(5));
        assert_eq!(Color::from_method_word(0xC0000000, 3), Color::ByLayer);
        assert_eq!(Color::from_aci(-3), Color::Index(3));
        for c in [
            Color::ByLayer,
            Color::ByBlock,
            Color::Rgb(1, 2, 3),
            Color::Index(250),
            Color::None,
        ] {
            assert_eq!(Color::from_method_word(c.to_method_word(), 7), c);
        }
        // Index 0 is ACI's "by block", as group 62 writes it.
        assert_eq!(Color::Index(0).to_method_word(), 0xC100_0000);
    }
}

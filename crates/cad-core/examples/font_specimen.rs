//! Write an SVG specimen of the built-in single-stroke font, for checking
//! glyph shapes by eye.
//!
//!     cargo run -p cad-core --example font_specimen -- /tmp/font.svg

use cad_core::font::{layout, Placement};
use cad_core::geom::Vec2;
use std::fmt::Write;

fn main() -> std::io::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "font.svg".into());
    let lines = [
        "ABCDEFGHIJKLM",
        "NOPQRSTUVWXYZ",
        "abcdefghijklm",
        "nopqrstuvwxyz",
        "0123456789 1234567890",
        "!\"#$%&'()*+,-./:;<=>?@",
        "[\\]^_`{|}~ °±⌀×÷µ²³·–—€",
        "ÀÁÂÃÄÅÇÈÉÊËÑÖØÜ àáâãäåçèéêëñöøü",
        "PLATE P-1042  4X ⌀13.00 THRU  R6.50",
        "MATERIAL: A36 STEEL, 10 THK  45°  ±0.05",
        "Break all sharp edges 0.5 x 45°",
        "1'-2 1/2\"  25.40 [1.000]  3/8\"",
    ];
    let height = 12.0;
    let pitch = 24.0;
    let width = 1400.0;
    let total = pitch * (lines.len() as f64 + 1.0);
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}"><rect width="100%" height="100%" fill="#0D1013"/><g transform="translate(20 0) scale(2.2)" fill="none" stroke="#FFFFFF" stroke-width="0.9" stroke-linecap="round" stroke-linejoin="round">"##,
        width,
        total * 2.2,
        width,
        total * 2.2
    );
    for (i, line) in lines.iter().enumerate() {
        let y = pitch * (i as f64 + 1.0);
        // SVG is y-down: lay out at the baseline, then flip y.
        for stroke in layout(line, &Placement::at(Vec2::new(0.0, 0.0), height)) {
            let pts: Vec<String> = stroke
                .iter()
                .map(|p| format!("{:.3},{:.3}", p.x, y - p.y))
                .collect();
            let _ = writeln!(svg, r#"<polyline points="{}"/>"#, pts.join(" "));
        }
    }
    let _ = writeln!(svg, "</g></svg>");
    std::fs::write(&path, svg)?;
    println!("wrote {path}");
    Ok(())
}

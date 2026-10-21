//! DWG files, read and written by the `cad-dwg` crate.
//!
//! Everything `cad_dwg` offers is here under the same names. Writing from
//! here ([`write::to_bytes`]) adds what the core draws and the DWG crate
//! leaves to the program: graphics blocks for dimensions, line families
//! for pattern hatches, MTEXT extents in the core's own font, a preview
//! of model space, and BlackCAD's name and version.

pub use cad_dwg::*;

pub mod write {
    //! The DWG writer (`cad_dwg::write`), and [`to_bytes`] as BlackCAD
    //! writes.

    pub use cad_dwg::write::*;

    use crate::error::Result;
    use crate::model::Drawing;
    use cad_dwg::nongraphic::{self, Vport};

    /// The drawing as an AutoCAD 2018 (AC1032) DWG file, as BlackCAD
    /// writes it. Dimensions drawn from their definition points get
    /// graphics blocks first, so that other programs draw them.
    pub fn to_bytes(d: &Drawing) -> Result<DwgOutput> {
        let d = &*crate::export::dimblocks::with_dimension_blocks(d);
        let png = preview_png(d);
        let opts = WriteOptions {
            app_name: crate::PRODUCT_NAME,
            app_version: crate::VERSION,
            preview_png: Some(&png),
            text_width: Some(&crate::font::text_width),
            hatch_lines: Some(&crate::render::hatch::library_lines),
        };
        to_bytes_with(d, &opts)
    }

    /// AutoCAD's model-space background, the canvas of its previews.
    const PREVIEW_BACKGROUND: (u8, u8, u8) = (0x21, 0x28, 0x30);

    /// The drawing's `*Active` viewport, which AutoCAD draws the preview
    /// from.
    fn active_viewport(d: &Drawing) -> Option<Vport> {
        d.objects
            .values()
            .filter(|o| o.type_name == "VPORT")
            .filter_map(|o| nongraphic::vport(o, &d.classes, d.codepage)?.ok())
            .find(|v| v.name.eq_ignore_ascii_case("*Active"))
    }

    /// The preview image as AutoCAD writes it: 256 pixels wide, the shape
    /// and view of the active viewport, model space on AutoCAD's
    /// background, as an indexed-colour PNG (the background, then the
    /// AutoCAD colour index).
    fn preview_png(d: &Drawing) -> Vec<u8> {
        let scene = crate::render::model_space(d);
        let b = scene.bounds;
        let width = 256u32;
        let (height, view) = match active_viewport(d) {
            Some(v) if v.view_height > 0.0 && v.aspect > 0.0 => {
                let h = ((width as f64 / v.aspect).round() as u32).clamp(16, 256);
                let view = crate::raster::View {
                    center: v.center,
                    scale: h as f64 / v.view_height,
                };
                (h, Some(view))
            }
            _ => {
                let h = if b.is_empty() || b.width() <= 0.0 || b.height() <= 0.0 {
                    192
                } else {
                    ((width as f64 * b.height() / b.width()).round() as u32).clamp(64, 256)
                };
                (h, None)
            }
        };
        let opts = crate::raster::RasterOptions {
            width,
            height,
            view,
            grid: false,
            axis_icon: false,
            threads: 1,
            ..Default::default()
        };
        let img = crate::raster::render(&scene, &opts);
        let mut palette = vec![PREVIEW_BACKGROUND];
        palette.extend((1..=255u8).map(crate::model::color::aci_rgb));
        let canvas = crate::standards::canvas::BACKGROUND;
        let dist = |a: (u8, u8, u8), b: (u8, u8, u8)| {
            let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
            d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
        };
        let mut cache = std::collections::HashMap::new();
        let pixels: Vec<u8> = img
            .rgba
            .chunks(4)
            .map(|p| {
                let c = (p[0], p[1], p[2]);
                *cache.entry(c).or_insert_with(|| {
                    if dist(c, canvas) < 3 * 24 * 24 {
                        return 0;
                    }
                    (1..palette.len()).min_by_key(|&i| dist(c, palette[i])).unwrap_or(0) as u8
                })
            })
            .collect();
        crate::raster::png::encode_indexed(img.width, img.height, &palette, &pixels)
    }
}

//! Layouts: sheets of paper holding the title block and notes, with
//! viewports onto model space at a scale (LAYOUT, MVIEW, VPORTS, page
//! setup).
//!
//! Drawings keep their layouts as DWG files do: a LAYOUT object per sheet
//! in the `ACAD_LAYOUT` dictionary of the named-object dictionary, each
//! pointing at its block (`*Model_Space`, `*Paper_Space`, then
//! `*Paper_Space0`, `*Paper_Space1` …), and a first viewport on every
//! sheet that is the sheet's own view (id 1). Drawings started here get
//! that structure the first time a layout is used, so their handles stay
//! as they were until then.
//!
//! Paper space is measured in millimetres for metric drawings and inches
//! for imperial ones, its origin at the paper's lower-left corner (the
//! sheets are "full bleed": no printer margins).

use crate::geom::{BBox, Vec2, Vec3};
use crate::model::*;
use crate::units::Unit;

type R<T> = Result<T, String>;

fn err(e: EditError) -> String {
    e.to_string()
}

/// A paper size: its name, the media name page setups record, and its
/// size in millimetres, landscape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paper {
    pub name: &'static str,
    pub media: &'static str,
    pub width_mm: f64,
    pub height_mm: f64,
}

/// The paper sizes offered, ISO then ANSI.
pub const PAPERS: [Paper; 9] = [
    Paper { name: "A4", media: "ISO_full_bleed_A4_(297.00_x_210.00_MM)", width_mm: 297.0, height_mm: 210.0 },
    Paper { name: "A3", media: "ISO_full_bleed_A3_(420.00_x_297.00_MM)", width_mm: 420.0, height_mm: 297.0 },
    Paper { name: "A2", media: "ISO_full_bleed_A2_(594.00_x_420.00_MM)", width_mm: 594.0, height_mm: 420.0 },
    Paper { name: "A1", media: "ISO_full_bleed_A1_(841.00_x_594.00_MM)", width_mm: 841.0, height_mm: 594.0 },
    Paper { name: "A0", media: "ISO_full_bleed_A0_(1189.00_x_841.00_MM)", width_mm: 1189.0, height_mm: 841.0 },
    Paper { name: "Letter", media: "ANSI_full_bleed_A_(11.00_x_8.50_Inches)", width_mm: 279.4, height_mm: 215.9 },
    Paper { name: "Tabloid", media: "ANSI_full_bleed_B_(17.00_x_11.00_Inches)", width_mm: 431.8, height_mm: 279.4 },
    Paper { name: "ANSI C", media: "ANSI_full_bleed_C_(22.00_x_17.00_Inches)", width_mm: 558.8, height_mm: 431.8 },
    Paper { name: "ANSI D", media: "ANSI_full_bleed_D_(34.00_x_22.00_Inches)", width_mm: 863.6, height_mm: 558.8 },
];

/// The media name a page setup records for a paper, landscape or
/// portrait: "ISO_full_bleed_A3_(420.00_x_297.00_MM)".
pub fn media_name(p: &Paper, portrait: bool) -> String {
    if !portrait {
        return p.media.to_string();
    }
    let (prefix, unit, k) = if p.media.contains("Inches") { (p.media.split('(').next().unwrap_or(""), "Inches", 25.4) } else { (p.media.split('(').next().unwrap_or(""), "MM", 1.0) };
    format!("{prefix}({:.2}_x_{:.2}_{unit})", p.height_mm / k, p.width_mm / k)
}

/// A paper size by name ("A3", "a3", "Tabloid", "ANSI B", "Letter" …).
pub fn paper(name: &str) -> Option<Paper> {
    let n = name.trim().to_ascii_uppercase().replace(['_', '-'], " ");
    let n = match n.as_str() {
        "ANSI A" => "LETTER".to_string(),
        "ANSI B" | "LEDGER" | "B" => "TABLOID".to_string(),
        "C" => "ANSI C".to_string(),
        "D" => "ANSI D".to_string(),
        _ => n,
    };
    PAPERS.iter().copied().find(|p| p.name.eq_ignore_ascii_case(&n))
}

/// The paper new layouts start on: A3 for metric drawings, ANSI B
/// (tabloid) for imperial ones, landscape.
pub fn default_paper(unit: Unit) -> Paper {
    if unit.is_imperial() {
        PAPERS[6]
    } else {
        PAPERS[1]
    }
}

/// Millimetres per paper unit: paper space is in inches for imperial
/// drawings, millimetres otherwise.
pub fn paper_unit_mm(unit: Unit) -> f64 {
    if unit.is_imperial() {
        25.4
    } else {
        1.0
    }
}

/// A sheet as the app shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// The LAYOUT object (null for a sheet the drawing has no object for
    /// yet: model space, or the first layout of a drawing started here).
    pub layout: Handle,
    /// The block its objects are in.
    pub block: Handle,
    pub name: String,
    pub tab_order: u32,
    /// Paper size in paper units (none for model space).
    pub paper: Option<(f64, f64)>,
    pub paper_name: String,
}

/// Model space and the layouts, in tab order. A drawing without layout
/// objects lists model space and its paper space as "Layout1".
pub fn sheets(d: &Drawing) -> Vec<Sheet> {
    let unit = Unit::of(d);
    let k = paper_unit_mm(unit);
    let mut out: Vec<Sheet> = d
        .objects
        .values()
        .filter_map(|o| match &o.kind {
            ObjectKind::NonGraphic(NonGraphic::Layout(l)) if d.block_header(l.block).is_some() => {
                let model = l.block == d.model_space();
                let (w, h) = (l.plot.paper_width, l.plot.paper_height);
                let (w, h) = if l.plot.rotation % 2 == 1 { (h, w) } else { (w, h) };
                let paper = (!model && w > 0.0 && h > 0.0).then(|| (w / k, h / k));
                let paper = if model { None } else { paper.or(Some(default_size(unit))) };
                Some(Sheet {
                    layout: o.handle,
                    block: l.block,
                    name: l.name.clone(),
                    tab_order: if model { 0 } else { l.tab_order.max(1) },
                    paper,
                    paper_name: paper_label(&l.plot.paper_size),
                })
            }
            _ => None,
        })
        .collect();
    out.sort_by_key(|s| (s.tab_order, s.name.to_ascii_lowercase()));
    let ms = d.model_space();
    if !out.iter().any(|s| s.block == ms) {
        out.insert(
            0,
            Sheet {
                layout: Handle::NULL,
                block: ms,
                name: "Model".into(),
                tab_order: 0,
                paper: None,
                paper_name: String::new(),
            },
        );
    }
    let ps = d.paper_space();
    if !ps.is_null() && !out.iter().any(|s| s.block == ps) {
        let p = default_paper(unit);
        out.insert(
            1,
            Sheet {
                layout: Handle::NULL,
                block: ps,
                name: "Layout1".into(),
                tab_order: 1,
                paper: Some(default_size(unit)),
                paper_name: p.name.into(),
            },
        );
    }
    out
}

fn default_size(unit: Unit) -> (f64, f64) {
    let p = default_paper(unit);
    let k = paper_unit_mm(unit);
    (p.width_mm / k, p.height_mm / k)
}

/// A short name for a media name: "A3" for "ISO_full_bleed_A3_(…)".
fn paper_label(media: &str) -> String {
    PAPERS
        .iter()
        .find(|p| p.media.eq_ignore_ascii_case(media) || p.name.eq_ignore_ascii_case(media))
        .map(|p| p.name.to_string())
        .unwrap_or_else(|| media.to_string())
}

/// The sheet named `name` (any case), "Model" included.
pub fn sheet(d: &Drawing, name: &str) -> Option<Sheet> {
    sheets(d).into_iter().find(|s| s.name.eq_ignore_ascii_case(name.trim()))
}

fn dictionary(d: &Drawing, h: Handle) -> Option<&Dictionary> {
    match &d.get(h)?.kind {
        ObjectKind::NonGraphic(NonGraphic::Dictionary(x)) => Some(x),
        _ => None,
    }
}

/// The entry `name` of dictionary `owner`, made (an empty dictionary)
/// when missing.
fn ensure_entry(d: &mut Drawing, owner: Handle, name: &str) -> Handle {
    if let Some(h) = dictionary(d, owner).and_then(|x| x.entries.iter().find(|(k, _)| k == name).map(|(_, h)| *h)) {
        if d.get(h).is_some() {
            return h;
        }
    }
    let h = d.add_object(
        "DICTIONARY",
        0x2A,
        owner,
        ObjectKind::NonGraphic(NonGraphic::Dictionary(Dictionary {
            cloning: 1,
            hard_owner: 0,
            entries: Vec::new(),
            default: None,
        })),
    );
    if let Some(o) = d.objects.get_mut(&h) {
        o.reactors = vec![owner];
    }
    if let Some(ObjectKind::NonGraphic(NonGraphic::Dictionary(x))) = d.objects.get_mut(&owner).map(|o| &mut o.kind) {
        x.entries.retain(|(k, _)| k != name);
        x.entries.push((name.to_string(), h));
    }
    h
}

/// The named-object dictionary, with its layout and group dictionaries,
/// made where missing. The layout dictionary's handle.
fn ensure_dictionaries(d: &mut Drawing) -> Handle {
    let nod = d.header.named_objects_dict;
    let nod = if dictionary(d, nod).is_some() {
        nod
    } else {
        let h = d.add_object(
            "DICTIONARY",
            0x2A,
            Handle::NULL,
            ObjectKind::NonGraphic(NonGraphic::Dictionary(Dictionary {
                cloning: 1,
                hard_owner: 0,
                entries: Vec::new(),
                default: None,
            })),
        );
        d.header.named_objects_dict = h;
        d.set_header_text("DICTIONARY_NAMED_OBJECTS", &format!("#{:X}", h.0));
        h
    };
    let groups = ensure_entry(d, nod, "ACAD_GROUP");
    d.set_header_text("DICTIONARY_ACAD_GROUP", &format!("#{:X}", groups.0));
    let layouts = ensure_entry(d, nod, "ACAD_LAYOUT");
    d.set_header_text("DICTIONARY_LAYOUTS", &format!("#{:X}", layouts.0));
    layouts
}

/// Plot settings for a sheet: a layout plotted at 1:1 on `paper`, or
/// model space plotted to fit it.
fn plot_settings(paper: &Paper, unit: Unit, model: bool) -> PlotSettings {
    PlotSettings {
        page_setup_name: String::new(),
        printer: "none_device".into(),
        flags: if model { 1024 | 512 | 128 | 32 | 16 | 4 } else { 512 | 128 | 32 | 16 },
        margins: [0.0; 4],
        paper_width: paper.width_mm,
        paper_height: paper.height_mm,
        paper_size: paper.media.into(),
        origin: Vec2::ZERO,
        paper_units: if unit.is_imperial() { 0 } else { 1 },
        rotation: 0,
        plot_type: if model { 1 } else { 5 },
        window_min: Vec2::ZERO,
        window_max: Vec2::ZERO,
        real_world_units: 1.0,
        drawing_units: 1.0,
        style_sheet: "monochrome.ctb".into(),
        scale_type: if model { 0 } else { 16 },
        scale_factor: 1.0,
        image_origin: Vec2::ZERO,
        shade_plot_mode: 0,
        shade_plot_res_level: 2,
        shade_plot_custom_dpi: 300,
        plot_view: Handle::NULL,
        visual_style: Handle::NULL,
    }
}

fn new_layout_object(d: &mut Drawing, dict: Handle, name: &str, block: Handle, tab_order: u32, paper: &Paper) -> Handle {
    let unit = Unit::of(d);
    let model = block == d.model_space();
    let k = paper_unit_mm(unit);
    let (w, h) = (paper.width_mm / k, paper.height_mm / k);
    let layout = Layout {
        plot: plot_settings(paper, unit, model),
        name: name.into(),
        tab_order,
        flags: 1,
        ucs_origin: Vec3::ZERO,
        limmin: Vec2::ZERO,
        limmax: if model { d.header.limmax.unwrap_or(Vec2::new(w, h)) } else { Vec2::new(w, h) },
        insbase: Vec3::ZERO,
        ucs_x: Vec3::X,
        ucs_y: Vec3::Y,
        elevation: 0.0,
        ortho_type: 0,
        extmin: Vec3::ZERO,
        extmax: Vec3::new(w, h, 0.0),
        block,
        last_viewport: Handle::NULL,
        base_ucs: Handle::NULL,
        named_ucs: Handle::NULL,
        viewports: Vec::new(),
    };
    let lh = d.add_object("LAYOUT", 0x52, dict, ObjectKind::NonGraphic(NonGraphic::Layout(Box::new(layout))));
    if let Some(o) = d.objects.get_mut(&lh) {
        o.reactors = vec![dict];
    }
    if let Some(ObjectKind::NonGraphic(NonGraphic::Dictionary(x))) = d.objects.get_mut(&dict).map(|o| &mut o.kind) {
        x.entries.push((name.to_string(), lh));
    }
    if let Some(ObjectKind::BlockHeader(b)) = d.objects.get_mut(&block).map(|o| &mut o.kind) {
        b.layout = lh;
    }
    if !model {
        let vp = sheet_view(d, block, w, h);
        if let Some(ObjectKind::NonGraphic(NonGraphic::Layout(l))) = d.objects.get_mut(&lh).map(|o| &mut o.kind) {
            l.last_viewport = vp;
        }
    }
    lh
}

/// The sheet's own view (viewport 1), first in its block.
fn sheet_view(d: &mut Drawing, block: Handle, w: f64, h: f64) -> Handle {
    if let Some(o) = d.block_entities(block).into_iter().find(|o| matches!(&o.kind, ObjectKind::PaperViewport(v) if v.id == 1)) {
        return o.handle;
    }
    let c = Vec2::new(w / 2.0, h / 2.0);
    let mut vp = PaperViewport::new(c, w * 1.1, h * 1.1, c, 1.0, 1);
    vp.view_height = h * 1.1;
    let layer0 = d.layer_by_name("0").map(|(l, _)| l).unwrap_or(Handle::NULL);
    d.add_entity_in(block, layer0, ObjectKind::PaperViewport(Box::new(vp))).unwrap_or(Handle::NULL)
}

/// Give the drawing its layout objects where it has none: "Model" for
/// model space and "Layout1" for paper space, in a layout dictionary.
pub fn ensure_layouts(d: &mut Drawing) -> R<()> {
    d.ensure_model_space();
    let dict = ensure_dictionaries(d);
    let unit = Unit::of(d);
    let have: Vec<Handle> = sheets(d).iter().filter(|s| !s.layout.is_null()).map(|s| s.block).collect();
    let ms = d.model_space();
    if !have.contains(&ms) {
        new_layout_object(d, dict, "Model", ms, 0, &default_paper(unit));
    }
    let ps = d.paper_space();
    if !ps.is_null() && !have.contains(&ps) {
        let name = if sheets(d).iter().any(|s| s.name.eq_ignore_ascii_case("Layout1") && s.block != ps) { "Layout0" } else { "Layout1" };
        new_layout_object(d, dict, name, ps, 1, &default_paper(unit));
    }
    Ok(())
}

fn check_layout_name(d: &Drawing, name: &str, except: Handle) -> R<String> {
    let name = name.trim();
    check_name(name).map_err(err)?;
    if name.eq_ignore_ascii_case("Model") {
        return Err("Model is model space's name".into());
    }
    if name.chars().count() > 255 {
        return Err("a layout name can have at most 255 characters".into());
    }
    if sheets(d).iter().any(|s| s.name.eq_ignore_ascii_case(name) && s.layout != except) {
        return Err(format!("there is a layout named {name} already"));
    }
    Ok(name.to_string())
}

/// A new layout on `paper` (by name; the drawing's default when empty),
/// last in the tabs. Its block's handle.
pub fn new_layout(d: &mut Drawing, name: &str, paper_name: &str) -> R<Handle> {
    ensure_layouts(d)?;
    let name = check_layout_name(d, name, Handle::NULL)?;
    let p = if paper_name.trim().is_empty() {
        default_paper(Unit::of(d))
    } else {
        paper(paper_name).ok_or_else(|| format!("there is no paper size {:?}; one of {}", paper_name.trim(), PAPERS.map(|p| p.name).join(", ")))?
    };
    let mut n = 0;
    while d.block_by_name(&format!("*Paper_Space{n}")).is_some() {
        n += 1;
    }
    let block = d.new_block_record(&format!("*Paper_Space{n}"), Vec3::ZERO).map_err(err)?;
    let dict = ensure_dictionaries(d);
    let tab = sheets(d).iter().map(|s| s.tab_order).max().unwrap_or(0) + 1;
    new_layout_object(d, dict, &name, block, tab, &p);
    Ok(block)
}

fn layout_mut(d: &mut Drawing, h: Handle) -> Option<&mut Layout> {
    match d.objects.get_mut(&h).map(|o| &mut o.kind) {
        Some(ObjectKind::NonGraphic(NonGraphic::Layout(l))) => Some(l),
        _ => None,
    }
}

/// The LAYOUT object of a sheet's block, made if the drawing has none.
fn layout_of_block(d: &mut Drawing, block: Handle) -> R<Handle> {
    ensure_layouts(d)?;
    sheets(d)
        .into_iter()
        .find(|s| s.block == block && !s.layout.is_null())
        .map(|s| s.layout)
        .ok_or_else(|| "that is not a layout".to_string())
}

/// Rename a layout.
pub fn rename_layout(d: &mut Drawing, old: &str, new: &str) -> R<()> {
    let s = sheet(d, old).ok_or_else(|| format!("there is no layout {old}"))?;
    if s.block == d.model_space() {
        return Err("model space cannot be renamed".into());
    }
    let lh = layout_of_block(d, s.block)?;
    let name = check_layout_name(d, new, lh)?;
    let dict = d.get(lh).map(|o| o.owner).unwrap_or(Handle::NULL);
    if let Some(l) = layout_mut(d, lh) {
        l.name = name.clone();
    }
    if let Some(ObjectKind::NonGraphic(NonGraphic::Dictionary(x))) = d.objects.get_mut(&dict).map(|o| &mut o.kind) {
        for e in &mut x.entries {
            if e.1 == lh {
                e.0 = name.clone();
            }
        }
    }
    Ok(())
}

/// Delete a layout and what is on it. The last layout stays.
pub fn delete_layout(d: &mut Drawing, name: &str) -> R<()> {
    let s = sheet(d, name).ok_or_else(|| format!("there is no layout {name}"))?;
    if s.block == d.model_space() {
        return Err("model space cannot be deleted".into());
    }
    if sheets(d).iter().filter(|x| x.block != d.model_space()).count() <= 1 {
        return Err("a drawing keeps at least one layout".into());
    }
    if s.block == d.paper_space() {
        return Err("this layout holds the drawing's paper space: delete another one".into());
    }
    let lh = layout_of_block(d, s.block)?;
    let dict = d.get(lh).map(|o| o.owner).unwrap_or(Handle::NULL);
    let ents: Vec<Handle> = d.block_entities(s.block).iter().map(|o| o.handle).collect();
    for e in ents {
        d.erase_entity(e).map_err(err)?;
    }
    if let Some(ObjectKind::NonGraphic(NonGraphic::Dictionary(x))) = d.objects.get_mut(&dict).map(|o| &mut o.kind) {
        x.entries.retain(|e| e.1 != lh);
    }
    let (begin, end) = d.block_header(s.block).map(|b| (b.block_entity, b.endblk)).unwrap_or_default();
    for h in [lh, begin, end, s.block] {
        d.objects.remove(&h);
    }
    let ctl = d.header.block_control;
    if let Some(ObjectKind::Control(c)) = d.objects.get_mut(&ctl).map(|o| &mut o.kind) {
        c.entries.retain(|h| *h != s.block);
    }
    d.rebuild_index();
    Ok(())
}

/// Put a layout on another paper size, landscape or portrait.
pub fn set_paper(d: &mut Drawing, block: Handle, paper_name: &str, portrait: bool) -> R<()> {
    let p = paper(paper_name).ok_or_else(|| format!("there is no paper size {:?}; one of {}", paper_name.trim(), PAPERS.map(|p| p.name).join(", ")))?;
    let lh = layout_of_block(d, block)?;
    let unit = Unit::of(d);
    let model = block == d.model_space();
    let k = paper_unit_mm(unit);
    let (w, h) = if portrait { (p.height_mm, p.width_mm) } else { (p.width_mm, p.height_mm) };
    let media = media_name(&p, portrait);
    if let Some(l) = layout_mut(d, lh) {
        l.plot.paper_width = w;
        l.plot.paper_height = h;
        l.plot.paper_size = media;
        l.plot.rotation = 0;
        if !model {
            l.limmax = Vec2::new(w / k, h / k);
            l.extmax = Vec3::new(w / k, h / k, 0.0);
        }
    }
    if !model {
        if let Some(o) = d.block_entities(block).into_iter().find(|o| matches!(&o.kind, ObjectKind::PaperViewport(v) if v.id == 1)) {
            let vh = o.handle;
            if let Some(ObjectKind::PaperViewport(v)) = d.objects.get_mut(&vh).map(|o| &mut o.kind) {
                let c = Vec2::new(w / k / 2.0, h / k / 2.0);
                v.center = c.extend(0.0);
                v.view_center = c;
                v.width = w / k * 1.1;
                v.height = h / k * 1.1;
                v.view_height = v.height;
            }
        }
    }
    Ok(())
}

/// Plot with colours or in black (`monochrome.ctb`), as a sheet records.
pub fn set_monochrome(d: &mut Drawing, block: Handle, monochrome: bool) -> R<()> {
    let lh = layout_of_block(d, block)?;
    if let Some(l) = layout_mut(d, lh) {
        l.plot.style_sheet = if monochrome { "monochrome.ctb".into() } else { String::new() };
    }
    Ok(())
}

/// Whether a sheet plots in black: its plot style table is monochrome
/// (the default for sheets with no page setup).
pub fn monochrome(d: &Drawing, block: Handle) -> bool {
    sheets(d)
        .iter()
        .find(|s| s.block == block && !s.layout.is_null())
        .and_then(|s| match &d.get(s.layout)?.kind {
            ObjectKind::NonGraphic(NonGraphic::Layout(l)) => Some(l.plot.style_sheet.to_ascii_lowercase().contains("monochrome")),
            _ => None,
        })
        .unwrap_or(true)
}

/// The paper a sheet plots on, in millimetres (model space: its page
/// setup's paper, or the default).
pub fn paper_mm(d: &Drawing, block: Handle) -> (f64, f64) {
    let unit = Unit::of(d);
    let found = sheets(d).into_iter().find(|s| s.block == block && !s.layout.is_null()).and_then(|s| match &d.get(s.layout)?.kind {
        ObjectKind::NonGraphic(NonGraphic::Layout(l)) if l.plot.paper_width > 0.0 && l.plot.paper_height > 0.0 => {
            Some(if l.plot.rotation % 2 == 1 { (l.plot.paper_height, l.plot.paper_width) } else { (l.plot.paper_width, l.plot.paper_height) })
        }
        _ => None,
    });
    found.unwrap_or_else(|| {
        let p = default_paper(unit);
        (p.width_mm, p.height_mm)
    })
}

/// How much of model space there is: the drawn extents, or the drawing
/// limits when nothing is drawn.
fn model_extents(d: &Drawing) -> BBox {
    let b = crate::render::model_space(d).bounds;
    if !b.is_empty() && (b.width() > 0.0 || b.height() > 0.0) {
        return b;
    }
    BBox {
        min: d.header.limmin.unwrap_or(Vec2::ZERO),
        max: d.header.limmax.unwrap_or(Vec2::new(420.0, 297.0)),
    }
}

/// A viewport on the layout `block` between corners `a` and `b` (MVIEW),
/// showing model space at `scale` (paper units per model unit) around
/// `view_center`; without a scale, model space's extents fit it.
pub fn add_viewport(d: &mut Drawing, block: Handle, a: Vec2, b: Vec2, scale: Option<f64>, view_center: Option<Vec2>) -> R<Handle> {
    if block == d.model_space() || d.block_header(block).is_none() {
        return Err("viewports go on a layout: switch to one first".into());
    }
    let (w, h) = ((b.x - a.x).abs(), (b.y - a.y).abs());
    if w < 1e-9 || h < 1e-9 {
        return Err("the viewport has no size".into());
    }
    let lh = layout_of_block(d, block)?;
    let (pw, ph) = sheets(d).iter().find(|s| s.layout == lh).and_then(|s| s.paper).unwrap_or((w, h));
    sheet_view(d, block, pw, ph);
    let c = Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let ext = model_extents(d);
    let (k, vc) = match scale {
        Some(k) => {
            if !(k.is_finite() && k > 0.0) {
                return Err("the scale must be more than zero".into());
            }
            (k, view_center.unwrap_or(ext.center()))
        }
        None => {
            let (ew, eh) = (ext.width().max(1e-9), ext.height().max(1e-9));
            ((w / ew).min(h / eh) * 0.9, view_center.unwrap_or(ext.center()))
        }
    };
    let id = d.block_entities(block).iter().filter(|o| matches!(o.kind, ObjectKind::PaperViewport(_))).count() as i16 + 1;
    let vp = PaperViewport::new(c, w, h, vc, k, id.max(2));
    let vh = d.add_entity_in(block, Handle::NULL, ObjectKind::PaperViewport(Box::new(vp))).map_err(err)?;
    Ok(vh)
}

/// The corners of a viewport filling layout `block`'s paper inside a
/// margin (MVIEW Fit): 10 mm, or ½ in on an inch sheet.
pub fn fit_corners(d: &Drawing, block: Handle) -> R<(Vec2, Vec2)> {
    if block == d.model_space() {
        return Err("viewports go on a layout: switch to one first".into());
    }
    let (w, h) = sheets(d)
        .into_iter()
        .find(|s| s.block == block)
        .and_then(|s| s.paper)
        .ok_or("this layout has no paper size: set one with page setup")?;
    let m = if Unit::of(d).is_imperial() { 0.5 } else { 10.0 };
    if w <= 2.0 * m || h <= 2.0 * m {
        return Err("the paper is too small for a margin".into());
    }
    Ok((Vec2::new(m, m), Vec2::new(w - m, h - m)))
}

/// Change what a viewport shows: its scale (paper units per model unit)
/// and the model point at its centre.
pub fn set_view(d: &mut Drawing, viewport: Handle, scale: Option<f64>, view_center: Option<Vec2>) -> R<()> {
    crate::ops::check_unlocked(d, viewport)?;
    let Some(ObjectKind::PaperViewport(v)) = d.objects.get_mut(&viewport).map(|o| &mut o.kind) else {
        return Err("that is not a viewport".into());
    };
    if v.id == 1 {
        return Err("that is the sheet's own view".into());
    }
    if let Some(k) = scale {
        if !(k.is_finite() && k > 0.0) {
            return Err("the scale must be more than zero".into());
        }
        v.view_height = v.height / k;
    }
    if let Some(c) = view_center {
        v.view_center = c;
    }
    Ok(())
}

/// Read a scale as drafters write it: "1:2", "2:1", "1/4\"=1'-0\"", "0.5".
/// Paper units per model unit.
pub fn parse_scale(text: &str, unit: Unit) -> Option<f64> {
    let t = text.trim();
    if let Some((p, m)) = t.split_once(':') {
        let (p, m): (f64, f64) = (p.trim().parse().ok()?, m.trim().parse().ok()?);
        return (p > 0.0 && m > 0.0).then_some(p / m);
    }
    if let Some((p, m)) = t.split_once('=') {
        let p = crate::units::parse_length(p.trim(), unit).ok()?;
        let m = crate::units::parse_length(m.trim(), unit).ok()?;
        return (p > 0.0 && m > 0.0).then_some(p / m);
    }
    let v: f64 = t.parse().ok()?;
    (v > 0.0 && v.is_finite()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standards::{new_drawing, Template};

    #[test]
    fn layouts_are_made_when_first_used() {
        let mut d = new_drawing(Template::Metric);
        let before = d.header.handseed;
        let s = sheets(&d);
        assert_eq!(s.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Model", "Layout1"]);
        assert_eq!(s[1].paper, Some((420.0, 297.0)));
        assert_eq!(d.header.handseed, before);
        d.add_entity(Handle::NULL, ObjectKind::Circle(Circle::new(Vec2::new(100.0, 50.0), 25.0))).unwrap();
        let ps = d.paper_space();
        let vp = add_viewport(&mut d, ps, Vec2::new(20.0, 20.0), Vec2::new(220.0, 170.0), Some(0.5), Some(Vec2::new(100.0, 50.0))).unwrap();
        let s = sheets(&d);
        assert_eq!(s.len(), 2);
        assert!(!s[1].layout.is_null());
        // The sheet's own view came first.
        let vps: Vec<i16> = d.block_entities(ps).iter().filter_map(|o| match &o.kind { ObjectKind::PaperViewport(v) => Some(v.id), _ => None }).collect();
        assert_eq!(vps, [1, 2]);
        // The circle shows on the layout at half size, in the viewport.
        let scene = crate::render::layout(&d, ps);
        let circle = scene.items.iter().find(|i| i.source == vp && matches!(i.shape, crate::render::Shape::Circle { .. })).unwrap();
        assert!(matches!(circle.shape, crate::render::Shape::Circle { radius, center } if (radius - 12.5).abs() < 1e-9 && center.distance(Vec2::new(120.0, 95.0)) < 1e-9));
        let b = new_layout(&mut d, "Details", "A4").unwrap();
        assert_eq!(sheet(&d, "details").unwrap().paper, Some((297.0, 210.0)));
        assert!(new_layout(&mut d, "details", "").is_err());
        rename_layout(&mut d, "Details", "Sheet 2").unwrap();
        assert_eq!(sheets(&d)[2].block, b);
        set_paper(&mut d, b, "A3", true).unwrap();
        assert_eq!(sheet(&d, "Sheet 2").unwrap().paper, Some((297.0, 420.0)));
        delete_layout(&mut d, "Sheet 2").unwrap();
        assert_eq!(sheets(&d).len(), 2);
        assert!(delete_layout(&mut d, "Layout1").is_err());
        // Saved and read back, the layouts are there.
        for bytes in [
            crate::native::to_bytes(&d, &crate::native::WriteOptions::default()),
            crate::dwg::write::to_bytes(&d).unwrap().bytes,
            crate::dxf::to_bytes(&d, crate::dxf::DxfFormat::Ascii).bytes,
        ] {
            let back = crate::open_bytes(&bytes).unwrap();
            let names: Vec<String> = sheets(&back).iter().map(|s| s.name.clone()).collect();
            assert_eq!(names, ["Model", "Layout1"]);
            let scene = crate::render::layout(&back, back.paper_space());
            assert!(scene.items.iter().any(|i| matches!(i.shape, crate::render::Shape::Circle { .. })));
        }
        assert_eq!(parse_scale("1:2", Unit::Millimeter), Some(0.5));
        assert_eq!(parse_scale("1/4\"=1'", Unit::Inch), Some(0.25 / 12.0));
    }
}

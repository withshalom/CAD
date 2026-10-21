//! Plan-view display geometry.
//!
//! Walks model space (expanding block references, dimensions' anonymous
//! blocks and polyline vertices), resolves by-layer and by-block colours,
//! linetypes and lineweights and visibility, applies object coordinate
//! systems and block transforms, and produces a flat list of 2D shapes in
//! world coordinates. Curves stay analytic (lines, arcs, circles, bulge
//! polylines) whenever the transform keeps them circular, so exporters can
//! write true arcs.
//!
//! Each shape carries its resolved style: colour, lineweight, linetype
//! name and dash pattern. Shapes are not split into dashes here; outputs
//! that draw dashes call [`dash::dash`] (DXF writes the linetype instead).
//! Dimensions without a graphics block are drawn by [`dim`].

pub mod clip;
pub mod dash;
pub mod dim;
pub mod hatch;

use crate::geom::curve::{self, ArcSeg, DEFAULT_STEP};
use crate::geom::{BBox, Transform, Vec2, Vec3};
use crate::model::*;
use crate::units::Unit;
use std::collections::BTreeMap;
use std::f64::consts::TAU;

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line {
        a: Vec2,
        b: Vec2,
    },
    Circle {
        center: Vec2,
        radius: f64,
    },
    /// Counter-clockwise from `start` to `end` (radians).
    Arc {
        center: Vec2,
        radius: f64,
        start: f64,
        end: f64,
    },
    /// Vertices with the bulge of the segment that leaves each vertex.
    Polyline {
        vertices: Vec<(Vec2, f64)>,
        closed: bool,
    },
    /// Already-tessellated curve.
    Path {
        points: Vec<Vec2>,
        closed: bool,
    },
    /// Filled region; rings are even-odd.
    Fill {
        rings: Vec<Vec<Vec2>>,
    },
    Point {
        at: Vec2,
    },
    /// A construction line through `origin` along `direction` (a unit
    /// vector): infinite both ways, or from `origin` on when `ray`. It has
    /// no extent: outputs cut it to what they show.
    Ray {
        origin: Vec2,
        direction: Vec2,
        ray: bool,
    },
    /// Separate straight segments drawn as one shape: a hatch's pattern.
    Lines {
        segments: Vec<(Vec2, Vec2)>,
    },
    /// Single-line text, placed as [`crate::font::Placement`] describes.
    Text {
        /// The alignment point (the first point for aligned and fit text).
        position: Vec2,
        /// Cap height.
        height: f64,
        rotation: f64,
        width_factor: f64,
        /// Oblique angle, radians.
        oblique: f64,
        text: String,
        /// DXF 72: 0 left, 1 centre, 2 right, 3 aligned, 4 middle, 5 fit.
        h_align: u16,
        /// DXF 73: 0 baseline, 1 bottom, 2 middle, 3 top.
        v_align: u16,
        /// The second point of aligned and fit text.
        fit_to: Option<Vec2>,
    },
}

impl Shape {
    /// Where the font puts this text.
    pub fn placement(&self) -> Option<crate::font::Placement> {
        match self {
            Shape::Text {
                position,
                height,
                rotation,
                width_factor,
                oblique,
                h_align,
                v_align,
                fit_to,
                ..
            } => Some(crate::font::Placement {
                position: *position,
                fit_to: *fit_to,
                height: *height,
                width_factor: *width_factor,
                oblique: *oblique,
                rotation: *rotation,
                h_align: *h_align,
                v_align: *v_align,
            }),
            _ => None,
        }
    }
}

/// How a shape is drawn, resolved from its entity, layer and enclosing
/// blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// True colour of [`Style::color`].
    pub rgb: (u8, u8, u8),
    /// ACI number for formats without true color.
    pub aci: i16,
    /// The colour, resolved: an index or a true colour.
    pub color: Color,
    pub layer: String,
    /// The lineweight, resolved: a weight, or the drawing default.
    pub lineweight: Lineweight,
    /// Linetype name, resolved; empty for continuous lines.
    pub linetype: String,
    /// Dash pattern in world units (positive dash, negative gap, zero
    /// dot), with every linetype scale applied; empty for continuous.
    pub pattern: Vec<f64>,
}

impl Style {
    /// Millimetres the line is drawn with (the drawing default counts as
    /// 0.25 mm).
    pub fn weight_mm(&self) -> f64 {
        self.lineweight.effective_mm()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub shape: Shape,
    pub style: Style,
    /// Top-level entity this came from (the INSERT for block contents).
    pub source: Handle,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderStats {
    pub entities_drawn: usize,
    /// Entity types that are not drawn yet, with counts.
    pub skipped: BTreeMap<String, usize>,
    pub failed: usize,
    pub hidden: usize,
    pub depth_limited: usize,
    /// Entities not drawn because the drawing asked for more than one
    /// drawing can hold ([`MAX_VISITS`]): blocks nested both deep and
    /// wide, or arrays of millions of copies (damage, usually).
    pub over_budget: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub items: Vec<Item>,
    pub bounds: BBox,
    pub stats: RenderStats,
    /// Each item's bounds, kept with the items: hit tests and patches
    /// read them instead of laying out text and arcs again.
    pub(crate) boxes: Vec<BBox>,
}

impl Scene {
    /// Bounds of item `i`.
    pub fn item_box(&self, i: usize) -> BBox {
        match self.boxes.get(i) {
            Some(b) if self.boxes.len() == self.items.len() => *b,
            _ => {
                let mut b = BBox::EMPTY;
                shape_bounds(&self.items[i].shape, &mut b);
                b
            }
        }
    }

    /// Fill in the item bounds when items were set some other way.
    fn ensure_boxes(&mut self) {
        if self.boxes.len() != self.items.len() {
            self.boxes = self
                .items
                .iter()
                .map(|it| {
                    let mut b = BBox::EMPTY;
                    shape_bounds(&it.shape, &mut b);
                    b
                })
                .collect();
        }
    }

    fn bounds_from_boxes(&mut self) {
        let mut b = BBox::EMPTY;
        for x in &self.boxes {
            b.union(x);
        }
        self.bounds = b;
    }
}

/// Inherited context when drawing inside a block reference.
#[derive(Clone)]
struct Inherit {
    /// Layer of the enclosing INSERT (for entities on layer "0").
    layer: Handle,
    /// Resolved color of the enclosing INSERT (for BYBLOCK).
    block_color: Color,
    /// Resolved lineweight of the enclosing INSERT (for BYBLOCK).
    block_weight: Lineweight,
    /// Linetype of the enclosing INSERT (for BYBLOCK): its name and its
    /// pattern before scaling, which each entity scales for itself.
    block_linetype: (String, Vec<f64>),
    source: Handle,
}

impl Inherit {
    fn root() -> Inherit {
        Inherit {
            layer: Handle::NULL,
            block_color: Color::Index(7),
            block_weight: Lineweight::Default,
            block_linetype: (String::new(), Vec::new()),
            source: Handle::NULL,
        }
    }
}

const MAX_DEPTH: usize = 16;

/// Most entities one rendering visits, counting each copy of a block's
/// contents. The depth limit alone does not bound the work: a block of
/// ten inserts nested ten deep is ten billion entities.
pub const MAX_VISITS: usize = 2_000_000;

struct Walker<'a> {
    d: &'a Drawing,
    scene: Scene,
    unit: Unit,
    ltscale: f64,
    /// Blocks being drawn, outermost first: a block met again inside
    /// itself (a damaged file) is not entered.
    open: Vec<Handle>,
    visits: usize,
}

impl<'a> Walker<'a> {
    fn new(d: &'a Drawing) -> Walker<'a> {
        let ltscale = d
            .header
            .ltscale
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(1.0);
        Walker {
            d,
            scene: Scene::default(),
            unit: Unit::of(d),
            ltscale,
            open: Vec::new(),
            visits: 0,
        }
    }
}

/// Render model space.
pub fn model_space(d: &Drawing) -> Scene {
    let mut w = Walker::new(d);
    let root = Inherit::root();
    for obj in d.model_space_entities() {
        let inh = Inherit {
            source: obj.handle,
            ..root.clone()
        };
        w.entity(obj, &Transform::IDENTITY, &inh, 0);
    }
    w.finish()
}

/// The top-level model-space entities a change touched, when it touched
/// nothing else that is drawn: no layer, style, block definition or
/// drawing-wide setting. Then [`update_model_space`] can bring a scene up
/// to date by drawing those entities alone; otherwise (`None`) the scene
/// is made again.
pub fn entities_changed(d: &Drawing, c: &Change) -> Option<Vec<Handle>> {
    let ms = d.model_space();
    let top = |o: &Object| {
        o.entity.as_ref().is_some_and(|e| e.space == Space::Model)
            && !matches!(o.kind, ObjectKind::BlockBegin { .. } | ObjectKind::BlockEnd)
    };
    let mut out = Vec::new();
    for (h, t) in &c.objects.touched {
        if *h == ms && !ms.is_null() {
            // Its entity list changed one place at a time (an entity
            // added or erased, which is in this change too); a record
            // kept whole may have been changed some other way.
            if t.prior.is_some() {
                return None;
            }
            continue;
        }
        let now = d.get(*h);
        let was = match &t.prior {
            Some(p) => p.as_ref(),
            None => now,
        };
        if now.is_some_and(|o| !top(o)) || was.is_some_and(|o| !top(o)) {
            return None;
        }
        if now.is_some() || was.is_some() {
            out.push(*h);
        }
    }
    if let Some(before) = c.header_before() {
        // Making entities moves the handle seed and may change the
        // current layer; neither is drawn.
        let mut b = before.clone();
        b.handseed = d.header.handseed;
        b.clayer = d.header.clayer;
        b.raw = d.header.raw.clone();
        if b != d.header {
            return None;
        }
    }
    Some(out)
}

impl RenderStats {
    fn add(&mut self, o: &RenderStats) {
        self.entities_drawn += o.entities_drawn;
        self.failed += o.failed;
        self.hidden += o.hidden;
        self.depth_limited += o.depth_limited;
        self.over_budget += o.over_budget;
        for (k, n) in &o.skipped {
            *self.skipped.entry(k.clone()).or_default() += n;
        }
    }

    fn sub(&mut self, o: &RenderStats) {
        self.entities_drawn = self.entities_drawn.saturating_sub(o.entities_drawn);
        self.failed = self.failed.saturating_sub(o.failed);
        self.hidden = self.hidden.saturating_sub(o.hidden);
        self.depth_limited = self.depth_limited.saturating_sub(o.depth_limited);
        self.over_budget = self.over_budget.saturating_sub(o.over_budget);
        for (k, n) in &o.skipped {
            if let Some(v) = self.skipped.get_mut(k) {
                *v = v.saturating_sub(*n);
                if *v == 0 {
                    self.skipped.remove(k);
                }
            }
        }
    }
}

/// Top-level entities drawn on their own, as [`model_space`] draws them.
fn draw_alone<'a>(d: &Drawing, objs: impl Iterator<Item = &'a Object>) -> Scene {
    let mut w = Walker::new(d);
    for obj in objs {
        let inh = Inherit {
            source: obj.handle,
            ..Inherit::root()
        };
        w.entity(obj, &Transform::IDENTITY, &inh, 0);
    }
    w.scene
}

/// Bring `scene`, made by [`model_space`], up to date after a change that
/// touched only the model-space entities `touched` (from
/// [`entities_changed`]); `before` gives each as it was (`None`: it did
/// not exist). Draws those entities alone and puts their shapes where
/// [`model_space`] would: the result is the scene drawn again. The usual
/// edits (entities added at the end, changed in place, erased) cost what
/// they touch, apart from moving the shapes after them along.
pub fn update_model_space<'a>(
    scene: &mut Scene,
    d: &Drawing,
    touched: &[Handle],
    before: impl Fn(Handle) -> Option<&'a Object>,
) {
    if touched.is_empty() {
        return;
    }
    scene.ensure_boxes();
    // Counts: take out what the old versions added, add the new ones.
    let old = draw_alone(d, touched.iter().filter_map(|h| before(*h)));
    let new = draw_alone(d, touched.iter().filter_map(|h| d.get(*h)));
    scene.stats.sub(&old.stats);
    scene.stats.add(&new.stats);
    let mut fresh: BTreeMap<Handle, Vec<(Item, BBox)>> = BTreeMap::new();
    for (it, b) in new.items.into_iter().zip(new.boxes) {
        fresh.entry(it.source).or_default().push((it, b));
    }

    // Entities that are all new and last in drawing order: add them.
    if touched.iter().all(|h| before(*h).is_none()) {
        if let Some(tail) = new_at_end(d, &fresh) {
            for h in tail {
                for (it, b) in fresh.remove(&h).expect("listed") {
                    scene.bounds.union(&b);
                    scene.items.push(it);
                    scene.boxes.push(b);
                }
            }
            return;
        }
    }

    // Where the touched entities' shapes are: one run each.
    let set: std::collections::BTreeSet<Handle> = touched.iter().copied().collect();
    let mut runs: Vec<(usize, usize, Handle)> = Vec::new();
    for (i, it) in scene.items.iter().enumerate() {
        if !set.contains(&it.source) {
            continue;
        }
        match runs.last_mut() {
            Some(r) if r.1 == i && r.2 == it.source => r.1 += 1,
            _ => runs.push((i, i + 1, it.source)),
        }
    }
    let one_run_each = {
        let mut seen = std::collections::BTreeSet::new();
        runs.iter().all(|r| seen.insert(r.2))
    };
    // Changed in place, each into as many shapes as before.
    if one_run_each
        && fresh.keys().all(|h| runs.iter().any(|r| r.2 == *h))
        && runs.iter().all(|(a, b, h)| fresh.get(h).map_or(0, Vec::len) == b - a)
    {
        for (a, _, h) in &runs {
            for (k, (it, b)) in fresh.remove(h).unwrap_or_default().into_iter().enumerate() {
                scene.items[a + k] = it;
                scene.boxes[a + k] = b;
            }
        }
        scene.bounds_from_boxes();
        return;
    }
    // Only shapes taken away (entities erased, or hidden).
    if fresh.is_empty() {
        if runs.len() <= 8 {
            for (a, b, _) in runs.iter().rev() {
                scene.items.drain(a..b);
                scene.boxes.drain(a..b);
            }
        } else {
            let mut keep = runs.iter().flat_map(|(a, b, _)| *a..*b).peekable();
            let mut i = 0;
            let mut w = 0;
            let n = scene.items.len();
            while i < n {
                if keep.peek() == Some(&i) {
                    keep.next();
                } else {
                    scene.items.swap(w, i);
                    scene.boxes.swap(w, i);
                    w += 1;
                }
                i += 1;
            }
            scene.items.truncate(w);
            scene.boxes.truncate(w);
        }
        scene.bounds_from_boxes();
        return;
    }
    // Otherwise lay the shapes out again in drawing order, moving the
    // unchanged ones and drawing nothing again.
    let order: Vec<Handle> = d.model_space_entities().iter().map(|o| o.handle).collect();
    let old_items = std::mem::take(&mut scene.items);
    let old_boxes = std::mem::take(&mut scene.boxes);
    let mut rest = old_items
        .into_iter()
        .zip(old_boxes)
        .filter(|(it, _)| !set.contains(&it.source))
        .peekable();
    let mut out: Vec<(Item, BBox)> = Vec::new();
    for h in order {
        if let Some(f) = fresh.remove(&h) {
            out.extend(f);
            continue;
        }
        while rest.peek().is_some_and(|(it, _)| it.source == h) {
            out.push(rest.next().expect("peeked"));
        }
    }
    // Anything left was drawn from an entity no longer in the order; it
    // stays, as before the change.
    out.extend(rest);
    let (items, boxes) = out.into_iter().unzip();
    scene.items = items;
    scene.boxes = boxes;
    scene.bounds_from_boxes();
}

/// When every entity in `fresh` comes after every other entity drawn in
/// model space, those entities in drawing order.
fn new_at_end(d: &Drawing, fresh: &BTreeMap<Handle, Vec<(Item, BBox)>>) -> Option<Vec<Handle>> {
    let ms = d.model_space();
    let order: &[Handle] = match d.block_header(ms) {
        Some(bh) if !bh.entities.is_empty() => &bh.entities,
        _ => d.index.owned.get(&ms).map(Vec::as_slice).unwrap_or(&[]),
    };
    let mut need = fresh.len();
    let mut tail = Vec::with_capacity(need);
    for h in order.iter().rev() {
        if need == 0 {
            break;
        }
        if fresh.contains_key(h) {
            tail.push(*h);
            need -= 1;
        } else if d.get(*h).is_some_and(|o| {
            o.is_entity() && !matches!(o.kind, ObjectKind::BlockBegin { .. } | ObjectKind::BlockEnd)
        }) {
            return None;
        }
    }
    if need > 0 {
        return None;
    }
    tail.reverse();
    Some(tail)
}

/// Render the contents of one block definition at identity.
pub fn block(d: &Drawing, block: Handle) -> Scene {
    let mut w = Walker::new(d);
    for obj in d.block_entities(block) {
        let inh = Inherit {
            source: obj.handle,
            ..Inherit::root()
        };
        w.entity(obj, &Transform::IDENTITY, &inh, 0);
    }
    w.finish()
}

fn layer_name(d: &Drawing, h: Handle) -> String {
    d.layer(h)
        .map(|l| l.name.clone())
        .unwrap_or_else(|| "0".to_string())
}

/// A colour that can be drawn: by-layer, by-block or no colour left
/// unresolved fall back to white (index 7).
fn solid_color(c: Color) -> Color {
    match c {
        Color::ByLayer | Color::ByBlock | Color::None => Color::Index(7),
        c => c,
    }
}

/// A plan shape mapped by an affine transform (identity is common, and
/// returned as is).
fn map_shape(shape: &Shape, xf: &Transform) -> Shape {
    if *xf == Transform::IDENTITY {
        return shape.clone();
    }
    let p = |v: Vec2| xf.apply_xy(v, 0.0);
    let similarity = xf.plane_similarity(Vec3::X, Vec3::Y);
    match shape {
        Shape::Line { a, b } => Shape::Line { a: p(*a), b: p(*b) },
        Shape::Point { at } => Shape::Point { at: p(*at) },
        Shape::Ray {
            origin,
            direction,
            ray,
        } => Shape::Ray {
            origin: p(*origin),
            direction: xf.apply_vector(direction.extend(0.0)).xy().normalized(),
            ray: *ray,
        },
        Shape::Lines { segments } => Shape::Lines {
            segments: segments.iter().map(|(a, b)| (p(*a), p(*b))).collect(),
        },
        Shape::Fill { rings } => Shape::Fill {
            rings: rings
                .iter()
                .map(|r| r.iter().map(|v| p(*v)).collect())
                .collect(),
        },
        Shape::Path { points, closed } => Shape::Path {
            points: points.iter().map(|v| p(*v)).collect(),
            closed: *closed,
        },
        Shape::Text {
            position,
            height,
            rotation,
            width_factor,
            oblique,
            text,
            h_align,
            v_align,
            fit_to,
        } => {
            let dir = xf
                .apply_vector(Vec2::from_angle(*rotation).extend(0.0))
                .xy();
            let up = xf
                .apply_vector(Vec2::from_angle(*rotation).perp().extend(0.0))
                .xy();
            let (dl, ul) = (dir.length().max(1e-12), up.length().max(1e-12));
            Shape::Text {
                position: p(*position),
                height: height * ul,
                rotation: dir.angle(),
                width_factor: width_factor * dl / ul,
                oblique: *oblique,
                text: text.clone(),
                h_align: *h_align,
                v_align: *v_align,
                fit_to: fit_to.map(p),
            }
        }
        Shape::Circle { center, radius } => match similarity {
            Some((k, _)) => Shape::Circle {
                center: p(*center),
                radius: radius * k,
            },
            None => map_shape(&as_path(shape), xf),
        },
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => match similarity {
            Some((k, mirrored)) => {
                let c = p(*center);
                let a0 = (p(*center + Vec2::from_angle(*start) * *radius) - c).angle();
                let a1 = (p(*center + Vec2::from_angle(*end) * *radius) - c).angle();
                let (start, end) = if mirrored { (a1, a0) } else { (a0, a1) };
                Shape::Arc {
                    center: c,
                    radius: radius * k,
                    start,
                    end,
                }
            }
            None => map_shape(&as_path(shape), xf),
        },
        Shape::Polyline { vertices, closed } => match similarity {
            Some((_, mirrored)) => Shape::Polyline {
                vertices: vertices
                    .iter()
                    .map(|(v, b)| (p(*v), if mirrored { -*b } else { *b }))
                    .collect(),
                closed: *closed,
            },
            None => map_shape(&as_path(shape), xf),
        },
    }
}

/// A curve as a tessellated path.
fn as_path(shape: &Shape) -> Shape {
    match shape {
        Shape::Circle { center, radius } => Shape::Path {
            points: curve::tessellate_arc(
                &ArcSeg {
                    center: *center,
                    radius: *radius,
                    start: 0.0,
                    end: TAU,
                },
                DEFAULT_STEP,
            ),
            closed: true,
        },
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => Shape::Path {
            points: curve::tessellate_arc(
                &ArcSeg {
                    center: *center,
                    radius: *radius,
                    start: *start,
                    end: *end,
                },
                DEFAULT_STEP,
            ),
            closed: false,
        },
        Shape::Polyline { vertices, closed } => Shape::Path {
            points: curve::tessellate_bulge_polyline(vertices, *closed, DEFAULT_STEP),
            closed: *closed,
        },
        other => other.clone(),
    }
}

impl<'a> Walker<'a> {
    fn finish(mut self) -> Scene {
        self.scene.bounds_from_boxes();
        self.scene
    }

    fn skip(&mut self, name: &str) {
        *self
            .scene
            .stats
            .skipped
            .entry(name.to_string())
            .or_default() += 1;
    }

    /// Effective layer and style for an entity, or None if hidden, with
    /// its linetype pattern before scaling (for blocks it holds).
    fn style(
        &mut self,
        obj: &Object,
        xf: &Transform,
        inh: &Inherit,
    ) -> Option<(Style, Handle, Vec<f64>)> {
        let ent = obj.entity.as_ref()?;
        if ent.invisible {
            return None;
        }
        let d = self.d;
        let mut layer_h = ent.layer;
        let on_layer_zero = d
            .layer(layer_h)
            .map(|l| l.name == "0")
            .unwrap_or(layer_h.is_null());
        if on_layer_zero && !inh.layer.is_null() {
            layer_h = inh.layer;
        }
        let layer = d.layer(layer_h);
        if let Some(l) = layer {
            if l.frozen || l.off {
                return None;
            }
        }
        let layer_color = layer.map(|l| l.color).unwrap_or(Color::Index(7));
        let color = solid_color(match ent.color {
            Color::ByLayer => layer_color,
            Color::ByBlock => inh.block_color,
            c => c,
        });
        let layer_weight = layer.map(|l| l.weight()).unwrap_or(Lineweight::Default);
        let lineweight = ent.weight().resolve(layer_weight, inh.block_weight);
        let xf_scale = xf
            .plane_similarity(Vec3::X, Vec3::Y)
            .map(|(s, _)| s)
            .unwrap_or(1.0);
        let (linetype, base) = self.linetype(ent, layer, inh);
        // $LTSCALE × the entity's own scale × any block scale.
        let ent_scale = if ent.linetype_scale.is_finite() && ent.linetype_scale > 0.0 {
            ent.linetype_scale
        } else {
            1.0
        };
        let k = self.ltscale * ent_scale * xf_scale;
        let pattern = base.iter().map(|v| v * k).collect();
        Some((
            Style {
                rgb: color.resolve(Color::Index(7), Color::Index(7)),
                aci: color.to_aci(),
                color,
                layer: layer_name(d, layer_h),
                lineweight,
                linetype,
                pattern,
            },
            layer_h,
            base,
        ))
    }

    /// The linetype name an entity is drawn with and its dash pattern as
    /// defined, before any scaling (empty for a continuous line).
    fn linetype(
        &self,
        ent: &EntityCommon,
        layer: Option<&Layer>,
        inh: &Inherit,
    ) -> (String, Vec<f64>) {
        let by_layer = || layer.map(|l| l.linetype).unwrap_or(Handle::NULL);
        let record = match ent.linetype {
            LinetypeRef::ByLayer => by_layer(),
            LinetypeRef::ByBlock => return inh.block_linetype.clone(),
            LinetypeRef::Continuous => return (String::new(), Vec::new()),
            LinetypeRef::Handle(h) => h,
        };
        let name = self.d.record_name(record).unwrap_or("");
        let record = if name.eq_ignore_ascii_case("ByLayer") {
            by_layer()
        } else if name.eq_ignore_ascii_case("ByBlock") {
            return inh.block_linetype.clone();
        } else {
            record
        };
        let name = self.d.record_name(record).unwrap_or("").to_string();
        let base = match self.d.linetype(record) {
            Some(lt) => lt.dashes(),
            // Name only (a DWG record the reader could not decode): the
            // project's own pattern for the standard names, if it has one.
            None => crate::standards::pattern_by_name(&name, self.unit).unwrap_or_default(),
        };
        if !dash::is_dashed(&base) {
            return (
                if name.eq_ignore_ascii_case(crate::standards::CONTINUOUS) {
                    String::new()
                } else {
                    name
                },
                Vec::new(),
            );
        }
        (name, base)
    }

    fn push(&mut self, shape: Shape, style: &Style, source: Handle) {
        let mut b = BBox::EMPTY;
        shape_bounds(&shape, &mut b);
        self.scene.boxes.push(b);
        self.scene.items.push(Item {
            shape,
            style: style.clone(),
            source,
        });
    }

    fn entity(&mut self, obj: &Object, xf: &Transform, inh: &Inherit, depth: usize) {
        if depth > MAX_DEPTH {
            self.scene.stats.depth_limited += 1;
            return;
        }
        if self.visits >= MAX_VISITS {
            self.scene.stats.over_budget += 1;
            return;
        }
        self.visits += 1;
        match &obj.kind {
            ObjectKind::Failed(_) | ObjectKind::Damaged(_) => {
                self.scene.stats.failed += 1;
                return;
            }
            ObjectKind::BlockBegin { .. }
            | ObjectKind::BlockEnd
            | ObjectKind::SeqEnd
            | ObjectKind::Vertex(_) => return,
            // Attribute definitions show their tag where they stand, but
            // not inside a block: its inserts show the values.
            ObjectKind::Attribute(a) if a.is_definition && depth > 0 => return,
            ObjectKind::PaperViewport(_) => return,
            _ => {}
        }
        let Some((style, layer_h, lt_base)) = self.style(obj, xf, inh) else {
            self.scene.stats.hidden += 1;
            return;
        };
        let src = if inh.source.is_null() {
            obj.handle
        } else {
            inh.source
        };
        let before = self.scene.items.len();
        match &obj.kind {
            ObjectKind::Line(l) => {
                let a = xf.apply(l.start).xy();
                let b = xf.apply(l.end).xy();
                self.push(Shape::Line { a, b }, &style, src);
            }
            ObjectKind::Circle(c) => {
                let full = xf.after(&Transform::ocs_to_wcs(c.extrusion));
                self.circle_or_arc(&full, c.center, c.radius, None, &style, src);
            }
            ObjectKind::Arc(a) => {
                let full = xf.after(&Transform::ocs_to_wcs(a.extrusion));
                self.circle_or_arc(
                    &full,
                    a.center,
                    a.radius,
                    Some((a.start_angle, a.end_angle)),
                    &style,
                    src,
                );
            }
            ObjectKind::LwPolyline(p) => {
                let full = xf.after(&Transform::ocs_to_wcs(p.extrusion));
                let verts: Vec<(Vec2, f64)> = p
                    .points
                    .iter()
                    .enumerate()
                    .map(|(i, pt)| (*pt, p.bulge(i)))
                    .collect();
                self.bulge_polyline(&full, &verts, p.elevation, p.is_closed(), &style, src);
            }
            ObjectKind::Polyline(p) => self.old_polyline(obj, p, xf, &style, src),
            ObjectKind::Point(p) => {
                // POINT coordinates are WCS.
                let at = xf.apply(p.position).xy();
                self.push(Shape::Point { at }, &style, src);
            }
            ObjectKind::Ellipse(e) => {
                let n = e.extrusion.normalized();
                let n = if n.length() < 0.5 { Vec3::Z } else { n };
                let minor = n.cross(e.major_axis) * e.axis_ratio;
                let pts = curve::tessellate_ellipse(
                    e.center,
                    e.major_axis,
                    minor,
                    e.start_param,
                    e.end_param,
                    DEFAULT_STEP,
                );
                let full_turn =
                    (curve::normalize_sweep(e.end_param - e.start_param) - TAU).abs() < 1e-9;
                let points: Vec<Vec2> = pts.iter().map(|p| xf.apply(*p).xy()).collect();
                self.push(
                    Shape::Path {
                        points,
                        closed: full_turn,
                    },
                    &style,
                    src,
                );
            }
            ObjectKind::Spline(sp) => {
                let pts3 = if sp.control_points.len() >= 2 && !sp.knots.is_empty() {
                    curve::tessellate_nurbs(
                        sp.degree as usize,
                        &sp.knots,
                        &sp.control_points,
                        &sp.weights,
                    )
                } else {
                    sp.fit_points.clone()
                };
                if pts3.len() >= 2 {
                    let points: Vec<Vec2> = pts3.iter().map(|p| xf.apply(*p).xy()).collect();
                    self.push(
                        Shape::Path {
                            points,
                            closed: sp.closed,
                        },
                        &style,
                        src,
                    );
                }
            }
            ObjectKind::Text(t) => self.text(t, xf, &style, src),
            ObjectKind::Attribute(a) => {
                if a.is_definition {
                    let mut t = a.text.clone();
                    t.value = a.tag.clone();
                    self.text(&t, xf, &style, src);
                } else if a.flags & 1 == 0 {
                    self.text(&a.text, xf, &style, src);
                }
            }
            ObjectKind::Xline(x) | ObjectKind::Ray(x) => {
                let origin = xf.apply(x.base).xy();
                let direction = xf.apply_vector(x.direction).xy().normalized();
                if direction.length() > 0.5 && origin.is_finite() {
                    self.push(
                        Shape::Ray {
                            origin,
                            direction,
                            ray: matches!(obj.kind, ObjectKind::Ray(_)),
                        },
                        &style,
                        src,
                    );
                }
            }
            ObjectKind::Leader(l) => self.leader(l, xf, &style, src),
            ObjectKind::MText(m) => self.mtext(m, xf, &style, src),
            ObjectKind::Solid(s) => {
                let full = xf.after(&Transform::ocs_to_wcs(s.extrusion));
                let c = s.corners;
                let mut ring: Vec<Vec2> = [c[0], c[1], c[3], c[2]]
                    .iter()
                    .map(|p| full.apply_xy(*p, s.elevation))
                    .collect();
                if ring[2].distance(ring[3]) < 1e-12 {
                    ring.pop();
                }
                self.push(Shape::Fill { rings: vec![ring] }, &style, src);
            }
            ObjectKind::Face3d(f) => {
                let points: Vec<Vec2> = f.corners.iter().map(|p| xf.apply(*p).xy()).collect();
                self.push(
                    Shape::Path {
                        points,
                        closed: true,
                    },
                    &style,
                    src,
                );
            }
            ObjectKind::Hatch(h) => self.hatch(h, xf, &style, src),
            ObjectKind::Insert(ins) => {
                self.insert(obj, ins, xf, layer_h, (&style, &lt_base), depth);
            }
            ObjectKind::Dimension(dm) => {
                if self.open.contains(&dm.block) {
                    self.scene.stats.depth_limited += 1;
                } else if self.d.block_header(dm.block).is_some() {
                    self.open.push(dm.block);
                    let child = Inherit {
                        layer: layer_h,
                        block_color: style.color,
                        block_weight: style.lineweight,
                        block_linetype: (style.linetype.clone(), lt_base.clone()),
                        source: src,
                    };
                    let ents: Vec<&Object> = self.d.block_entities(dm.block);
                    for e in ents {
                        self.entity(e, xf, &child, depth + 1);
                    }
                    self.open.pop();
                } else {
                    self.dimension(dm, xf, layer_h, &style, src);
                }
            }
            _ => {
                self.skip(&obj.type_name);
                return;
            }
        }
        if depth == 0
            && (self.scene.items.len() > before || matches!(obj.kind, ObjectKind::Insert(_)))
        {
            self.scene.stats.entities_drawn += 1;
        }
    }

    /// A dimension with no graphics block, drawn from its definition
    /// points and its dimension style.
    fn dimension(
        &mut self,
        dm: &Dimension,
        xf: &Transform,
        layer_h: Handle,
        style: &Style,
        src: Handle,
    ) {
        let d = self.d;
        let st = d
            .dim_style(dm.dimstyle)
            .or_else(|| d.dim_style(d.header.dimstyle))
            .cloned()
            .unwrap_or_else(|| crate::standards::dim_style(self.unit));
        let Some(parts) = dim::graphics(dm, &st, self.unit) else {
            self.skip(match dm.kind {
                DimensionKind::Ordinate => "DIMENSION (ordinate)",
                _ => "DIMENSION (angular)",
            });
            return;
        };
        // By block: the dimension's own colour and weight; by layer: its
        // layer's.
        let layer = d.layer(layer_h);
        let layer_color = layer.map(|l| l.color).unwrap_or(Color::Index(7));
        let layer_weight = layer.map(|l| l.weight()).unwrap_or(Lineweight::Default);
        let part_style = |color: Color, weight: u8| {
            let color = solid_color(match color {
                Color::ByBlock => style.color,
                Color::ByLayer => layer_color,
                c => c,
            });
            Style {
                rgb: color.resolve(Color::Index(7), Color::Index(7)),
                aci: color.to_aci(),
                color,
                layer: style.layer.clone(),
                lineweight: Lineweight::from_code(weight).resolve(layer_weight, style.lineweight),
                linetype: String::new(),
                pattern: Vec::new(),
            }
        };
        let line = part_style(st.line_color, st.line_weight);
        let ext = part_style(st.ext_color, st.ext_weight);
        let text = part_style(st.text_color, Lineweight::CODE_BY_BLOCK);
        for (part, shape) in parts {
            let s = match part {
                dim::Part::Line => &line,
                dim::Part::Extension => &ext,
                dim::Part::Text => &text,
            };
            self.push(map_shape(&shape, xf), s, src);
        }
    }

    fn circle_or_arc(
        &mut self,
        full: &Transform,
        center: Vec3,
        radius: f64,
        angles: Option<(f64, f64)>,
        style: &Style,
        src: Handle,
    ) {
        if radius.is_nan() || radius <= 0.0 || !radius.is_finite() {
            return;
        }
        if let Some((scale, mirrored)) = full.plane_similarity(Vec3::X, Vec3::Y) {
            let c = full.apply(center).xy();
            let r = radius * scale;
            match angles {
                None => self.push(
                    Shape::Circle {
                        center: c,
                        radius: r,
                    },
                    style,
                    src,
                ),
                Some((s, e)) => {
                    let ps = full
                        .apply(center + Vec3::new(s.cos(), s.sin(), 0.0) * radius)
                        .xy();
                    let pe = full
                        .apply(center + Vec3::new(e.cos(), e.sin(), 0.0) * radius)
                        .xy();
                    let (a0, a1) = ((ps - c).angle(), (pe - c).angle());
                    let (start, end) = if mirrored { (a1, a0) } else { (a0, a1) };
                    self.push(
                        Shape::Arc {
                            center: c,
                            radius: r,
                            start,
                            end,
                        },
                        style,
                        src,
                    );
                }
            }
            return;
        }
        // General affine: tessellate in OCS, then map.
        let (s, e) = angles.unwrap_or((0.0, TAU));
        let arc = ArcSeg {
            center: center.xy(),
            radius,
            start: s,
            end: e,
        };
        let pts = curve::tessellate_arc(&arc, DEFAULT_STEP);
        let points: Vec<Vec2> = pts.iter().map(|p| full.apply_xy(*p, center.z)).collect();
        self.push(
            Shape::Path {
                points,
                closed: angles.is_none(),
            },
            style,
            src,
        );
    }

    fn bulge_polyline(
        &mut self,
        full: &Transform,
        verts: &[(Vec2, f64)],
        elevation: f64,
        closed: bool,
        style: &Style,
        src: Handle,
    ) {
        if verts.len() < 2 {
            if let Some((p, _)) = verts.first() {
                self.push(
                    Shape::Point {
                        at: full.apply_xy(*p, elevation),
                    },
                    style,
                    src,
                );
            }
            return;
        }
        if let Some((_, mirrored)) = full.plane_similarity(Vec3::X, Vec3::Y) {
            let vertices = verts
                .iter()
                .map(|(p, b)| {
                    (
                        full.apply_xy(*p, elevation),
                        if mirrored { -*b } else { *b },
                    )
                })
                .collect();
            self.push(Shape::Polyline { vertices, closed }, style, src);
        } else {
            let pts = curve::tessellate_bulge_polyline(verts, closed, DEFAULT_STEP);
            let points = pts.iter().map(|p| full.apply_xy(*p, elevation)).collect();
            self.push(Shape::Path { points, closed }, style, src);
        }
    }

    fn old_polyline(
        &mut self,
        obj: &Object,
        p: &Polyline,
        xf: &Transform,
        style: &Style,
        src: Handle,
    ) {
        let verts = self.d.polyline_vertices(obj);
        match p.kind {
            PolylineKind::Polyline2d => {
                let full = xf.after(&Transform::ocs_to_wcs(p.extrusion));
                // Skip spline-frame control points (vertex flag 16).
                let vs: Vec<(Vec2, f64)> = verts
                    .iter()
                    .filter(|v| v.flags & 16 == 0)
                    .map(|v| (v.point.xy(), v.bulge))
                    .collect();
                self.bulge_polyline(&full, &vs, p.elevation, p.is_closed(), style, src);
            }
            PolylineKind::Polyline3d => {
                let points: Vec<Vec2> = verts
                    .iter()
                    .filter(|v| v.flags & 16 == 0)
                    .map(|v| xf.apply(v.point).xy())
                    .collect();
                if points.len() >= 2 {
                    self.push(
                        Shape::Path {
                            points,
                            closed: p.is_closed(),
                        },
                        style,
                        src,
                    );
                }
            }
            PolylineKind::PolyfaceMesh => {
                let positions: Vec<Vec2> = verts
                    .iter()
                    .filter(|v| v.kind == VertexKind::PfaceVertex)
                    .map(|v| xf.apply(v.point).xy())
                    .collect();
                for f in verts.iter().filter(|v| v.kind == VertexKind::PfaceFace) {
                    let idx: Vec<i16> = f.face.iter().copied().filter(|&i| i != 0).collect();
                    for k in 0..idx.len() {
                        let a = idx[k];
                        let b = idx[(k + 1) % idx.len()];
                        if a < 0 {
                            continue; // invisible edge
                        }
                        let pa = positions.get(a.unsigned_abs() as usize - 1);
                        let pb = positions.get(b.unsigned_abs() as usize - 1);
                        if let (Some(&pa), Some(&pb)) = (pa, pb) {
                            self.push(Shape::Line { a: pa, b: pb }, style, src);
                        }
                    }
                }
            }
            PolylineKind::PolygonMesh => self.skip("POLYLINE_MESH"),
        }
    }

    fn text(&mut self, t: &Text, xf: &Transform, style: &Style, src: Handle) {
        let full = xf.after(&Transform::ocs_to_wcs(t.extrusion));
        // Aligned and fit text run from the insertion point to the
        // alignment point; other alignments hang on the alignment point.
        let two_points = matches!(t.h_align, 3 | 5);
        let anchor = if (t.h_align == 0 && t.v_align == 0) || two_points {
            t.insertion
        } else {
            t.alignment
        };
        let position = full.apply_xy(anchor, t.elevation);
        let fit_to = two_points.then(|| full.apply_xy(t.alignment, t.elevation));
        let dir = full
            .apply_vector(Vec3::new(t.rotation.cos(), t.rotation.sin(), 0.0))
            .xy();
        let up = full
            .apply_vector(Vec3::new(-t.rotation.sin(), t.rotation.cos(), 0.0))
            .xy();
        let height = t.height * up.length().max(1e-12);
        let text = crate::text::expand_percent_codes(&t.value);
        if text.trim().is_empty() {
            return;
        }
        self.push(
            Shape::Text {
                position,
                height,
                rotation: dir.angle(),
                width_factor: if t.width_factor > 0.0 {
                    t.width_factor
                } else {
                    1.0
                },
                oblique: if t.oblique.is_finite() {
                    t.oblique
                } else {
                    0.0
                },
                text,
                h_align: t.h_align,
                v_align: t.v_align,
                fit_to,
            },
            style,
            src,
        );
    }

    fn mtext(&mut self, m: &MText, xf: &Transform, style: &Style, src: Handle) {
        let plain = crate::text::strip_mtext(&m.contents);
        let h = if m.text_height > 0.0 {
            m.text_height
        } else {
            1.0
        };
        // Paragraphs wrap at the reference width, when there is one.
        let wrap_at = if m.rect_width > 0.0 && m.rect_width.is_finite() {
            m.rect_width / h
        } else {
            0.0
        };
        let wrapped: Vec<String> = plain
            .lines()
            .flat_map(|p| crate::font::wrap(p, wrap_at))
            .collect();
        let lines: Vec<&str> = wrapped.iter().map(String::as_str).collect();
        if lines.is_empty() {
            return;
        }
        let n = m.extrusion.normalized();
        let n = if n.length() < 0.5 { Vec3::Z } else { n };
        let mut x_axis = m.x_axis.normalized();
        if x_axis.length() < 0.5 {
            x_axis = crate::geom::arbitrary_axes(n).0;
        }
        let y_axis = n.cross(x_axis).normalized();
        let spacing = 1.666_666
            * h
            * if m.linespacing_factor > 0.0 {
                m.linespacing_factor
            } else {
                1.0
            };
        let total = spacing * (lines.len() as f64 - 1.0) + h;
        // Attachment 1–9: rows top/middle/bottom, columns left/center/right.
        let att = m.attachment.clamp(1, 9);
        let col = (att - 1) % 3;
        let row = (att - 1) / 3;
        let top_offset = match row {
            0 => 0.0,
            1 => total / 2.0,
            _ => total,
        };
        let h_align = col; // 0 left, 1 center, 2 right (TEXT codes)
        let dir2 = xf.apply_vector(x_axis).xy();
        let up2 = xf.apply_vector(y_axis).xy();
        for (i, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            // Baseline of line i measured from the attachment point.
            let dy = top_offset - h - spacing * i as f64;
            let p3 = m.insertion + y_axis * dy;
            self.push(
                Shape::Text {
                    position: xf.apply(p3).xy(),
                    height: h * up2.length().max(1e-12),
                    rotation: dir2.angle(),
                    width_factor: 1.0,
                    oblique: 0.0,
                    text: line.to_string(),
                    h_align,
                    v_align: 0,
                    fit_to: None,
                },
                style,
                src,
            );
        }
    }

    fn hatch(&mut self, h: &Hatch, xf: &Transform, style: &Style, src: Handle) {
        let full = xf.after(&Transform::ocs_to_wcs(h.extrusion));
        let mut rings: Vec<Vec<Vec2>> = Vec::new();
        let mut ocs_rings: Vec<Vec<Vec2>> = Vec::new();
        for path in &h.paths {
            let pts2: Vec<Vec2> = match path {
                // Boundary polylines are always closed loops.
                HatchPath::Polyline { vertices, .. } => {
                    curve::tessellate_bulge_polyline(vertices, true, DEFAULT_STEP)
                }
                HatchPath::Edges { edges, .. } => {
                    let mut pts = Vec::new();
                    for e in edges {
                        let seg: Vec<Vec2> = match e {
                            HatchEdge::Line { a, b } => vec![*a, *b],
                            HatchEdge::Arc {
                                center,
                                radius,
                                start,
                                end,
                                ccw,
                            } => {
                                let major = Vec2::new(*radius, 0.0);
                                edge_arc_points(*center, major, 1.0, *start, *end, *ccw)
                            }
                            HatchEdge::Ellipse {
                                center,
                                major,
                                ratio,
                                start,
                                end,
                                ccw,
                            } => edge_arc_points(*center, *major, *ratio, *start, *end, *ccw),
                            HatchEdge::Spline {
                                degree,
                                knots,
                                control_points,
                                weights,
                            } => {
                                let cp: Vec<Vec3> =
                                    control_points.iter().map(|p| p.extend(0.0)).collect();
                                curve::tessellate_nurbs(*degree as usize, knots, &cp, weights)
                                    .iter()
                                    .map(|p| p.xy())
                                    .collect()
                            }
                        };
                        pts.extend(seg);
                    }
                    pts
                }
            };
            if pts2.len() >= 2 {
                rings.push(
                    pts2.iter()
                        .map(|p| full.apply_xy(*p, h.elevation))
                        .collect(),
                );
                ocs_rings.push(pts2);
            }
        }
        if rings.is_empty() {
            return;
        }
        if h.solid_fill {
            self.push(Shape::Fill { rings }, style, src);
            return;
        }
        // The pattern: as stored, or from the library by name for hatches
        // saved without their line families.
        let lines = if !h.pattern_lines.is_empty() {
            Some(h.pattern_lines.clone())
        } else {
            hatch::pattern_lines(&h.pattern_name, h.pattern_scale, h.pattern_angle, h.pattern_double, self.unit)
        };
        let segments = lines.and_then(|lines| {
            // Lines are made in the hatch's plane, then mapped.
            let ocs_rings: Vec<Vec<Vec2>> = ocs_rings.iter().map(|r| r.to_vec()).collect();
            hatch::fill_lines(&ocs_rings, &lines)
        });
        match segments {
            Some(segs) => {
                if !segs.is_empty() {
                    let segments = segs
                        .into_iter()
                        .map(|(a, b)| (full.apply_xy(a, h.elevation), full.apply_xy(b, h.elevation)))
                        .collect();
                    self.push(Shape::Lines { segments }, style, src);
                }
            }
            // No pattern to draw (an unknown name, or one too fine for its
            // area): the boundary shows where the hatch is.
            None => {
                for r in rings {
                    self.push(
                        Shape::Path {
                            points: r,
                            closed: true,
                        },
                        style,
                        src,
                    );
                }
            }
        }
    }

    /// A leader: its line (or a curve through its vertices), the arrow at
    /// the first vertex and the hook line into the annotation.
    fn leader(&mut self, l: &Leader, xf: &Transform, style: &Style, src: Handle) {
        let pts: Vec<Vec2> = l.vertices.iter().map(|p| xf.apply(*p).xy()).collect();
        if pts.len() < 2 {
            return;
        }
        let d = self.d;
        let st = d
            .dim_style(l.dimstyle)
            .or_else(|| d.dim_style(d.header.dimstyle))
            .cloned()
            .unwrap_or_else(|| crate::standards::dim_style(self.unit));
        let sz = dim::sizes(&st, self.unit);
        let line: Vec<Vec2> = if l.spline && pts.len() > 2 {
            leader_curve(&pts)
        } else {
            pts.clone()
        };
        self.push(
            Shape::Path {
                points: line,
                closed: false,
            },
            style,
            src,
        );
        if l.arrowhead {
            let dir = (pts[0] - pts[1]).normalized();
            if dir.length() > 0.5 && sz.arrow > 0.0 {
                let base = pts[0] - dir * sz.arrow;
                let w = dir.perp() * (sz.arrow / (2.0 * crate::standards::ARROW_LENGTH_TO_WIDTH));
                self.push(
                    Shape::Fill {
                        rings: vec![vec![pts[0], base + w, base - w]],
                    },
                    style,
                    src,
                );
            }
        }
        if l.hookline {
            let last = pts[pts.len() - 1];
            let prev = pts[pts.len() - 2];
            let x = xf.apply_vector(l.x_direction).xy().normalized();
            let x = if x.length() < 0.5 { Vec2::new(1.0, 0.0) } else { x };
            let side = if (last - prev).dot(x) >= 0.0 { 1.0 } else { -1.0 };
            self.push(
                Shape::Line {
                    a: last,
                    b: last + x * (side * sz.arrow),
                },
                style,
                src,
            );
        }
    }

    fn insert(
        &mut self,
        obj: &Object,
        ins: &Insert,
        xf: &Transform,
        layer_h: Handle,
        (style, lt_base): (&Style, &[f64]),
        depth: usize,
    ) {
        let d = self.d;
        let Some(bh) = d.block_header(ins.block_header) else {
            self.skip("INSERT (missing block)");
            return;
        };
        if bh.is_xref {
            self.skip("INSERT (external reference)");
            return;
        }
        let ocs = Transform::ocs_to_wcs(ins.extrusion);
        let base = Transform::translation(-bh.base_point);
        let scale = Transform::scale(ins.scale);
        let rot = Transform::rotation_z(ins.rotation);
        let (cols, rows, dx, dy) = ins.array.unwrap_or((1, 1, 0.0, 0.0));
        let cols = cols.clamp(1, 1000);
        let rows = rows.clamp(1, 1000);
        let child = Inherit {
            layer: layer_h,
            block_color: style.color,
            block_weight: style.lineweight,
            block_linetype: (style.linetype.clone(), lt_base.to_vec()),
            source: obj.handle,
        };
        if self.open.contains(&ins.block_header) {
            // The block holds an insert of itself: drawn once, not forever.
            self.scene.stats.depth_limited += 1;
            return;
        }
        self.open.push(ins.block_header);
        let ents: Vec<&Object> = d.block_entities(ins.block_header);
        'cells: for r in 0..rows {
            for c in 0..cols {
                if self.visits >= MAX_VISITS {
                    self.scene.stats.over_budget += 1;
                    break 'cells;
                }
                let cell = Transform::translation(Vec3::new(c as f64 * dx, r as f64 * dy, 0.0));
                let local = Transform::translation(ins.insertion)
                    .after(&rot)
                    .after(&cell)
                    .after(&scale)
                    .after(&base);
                let full = xf.after(&ocs).after(&local);
                for e in &ents {
                    if e.handle == obj.handle {
                        continue;
                    }
                    self.entity(e, &full, &child, depth + 1);
                }
            }
        }
        self.open.pop();
        // Attributes are stored in the insert's own coordinate system.
        let parent = Inherit {
            source: obj.handle,
            ..child
        };
        let parent = Inherit {
            layer: Handle::NULL,
            ..parent
        };
        for a in d.insert_attributes(obj) {
            self.entity(a, xf, &parent, depth + 1);
        }
    }
}

/// Points of a hatch boundary arc or elliptical arc edge. Clockwise edges
/// store their angles measured clockwise, so the geometric arc runs
/// counter-clockwise from -end to -start; the points are then reversed to
/// keep the edge's direction.
fn edge_arc_points(
    center: Vec2,
    major: Vec2,
    ratio: f64,
    start: f64,
    end: f64,
    ccw: bool,
) -> Vec<Vec2> {
    let maj = major.extend(0.0);
    let minor = Vec3::Z.cross(maj) * ratio;
    let (s, e) = if ccw { (start, end) } else { (-end, -start) };
    let mut pts: Vec<Vec2> = curve::tessellate_ellipse(Vec3::ZERO, maj, minor, s, e, DEFAULT_STEP)
        .iter()
        .map(|q| center + q.xy())
        .collect();
    if !ccw {
        pts.reverse();
    }
    pts
}

/// A smooth curve through a spline leader's vertices (Catmull–Rom),
/// tangent to the first segment at the arrow.
fn leader_curve(pts: &[Vec2]) -> Vec<Vec2> {
    let n = pts.len();
    let mut out = vec![pts[0]];
    for i in 0..n - 1 {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = if i + 2 < n { pts[i + 2] } else { pts[n - 1] };
        for k in 1..=12 {
            let t = k as f64 / 12.0;
            let (t2, t3) = (t * t, t * t * t);
            let p = (p1 * 2.0
                + (p2 - p0) * t
                + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
                + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3)
                * 0.5;
            out.push(p);
        }
    }
    out
}

/// The scene as it plots: without what lies on layers set not to plot,
/// nor on `DEFPOINTS`, which never plots.
pub fn plotted(d: &Drawing, scene: &Scene) -> Scene {
    let skip: std::collections::BTreeSet<String> = d
        .objects
        .values()
        .filter_map(|o| match &o.kind {
            ObjectKind::Layer(l) if !l.plot || l.name.eq_ignore_ascii_case("DEFPOINTS") => Some(l.name.to_ascii_uppercase()),
            _ => None,
        })
        .collect();
    if skip.is_empty() {
        return scene.clone();
    }
    let mut out = Scene {
        stats: scene.stats.clone(),
        bounds: BBox::EMPTY,
        ..Default::default()
    };
    for (i, it) in scene.items.iter().enumerate() {
        if skip.contains(&it.style.layer.to_ascii_uppercase()) {
            continue;
        }
        let b = scene.item_box(i);
        out.bounds.union(&b);
        out.items.push(it.clone());
        out.boxes.push(b);
    }
    out
}

/// A layout: the paper-space entities of its block, and in each viewport
/// the part of model space it shows, scaled onto the paper and cut at the
/// viewport's edges. Model space content takes the viewport as its
/// source, so picking it on the layout picks the viewport. Linetype dashes
/// keep their size on the paper whatever the viewport's scale, as
/// `$PSLTSCALE` 1 has it.
pub fn layout(d: &Drawing, block: Handle) -> Scene {
    let mut w = Walker::new(d);
    let ents = d.block_entities(block);
    let mut model: Option<Scene> = None;
    for obj in &ents {
        if let ObjectKind::PaperViewport(vp) = &obj.kind {
            if vp.id == 1 {
                continue;
            }
            let Some((style, _, _)) = w.style(obj, &Transform::IDENTITY, &Inherit::root()) else {
                continue;
            };
            let (c, hw, hh) = (vp.center.xy(), vp.width / 2.0, vp.height / 2.0);
            let rect = BBox {
                min: Vec2::new(c.x - hw, c.y - hh),
                max: Vec2::new(c.x + hw, c.y + hh),
            };
            if vp.is_on() && hw > 0.0 && hh > 0.0 {
                let m = model.get_or_insert_with(|| model_space(d));
                let k = vp.scale();
                let rot = Transform::rotation_z(-vp.twist);
                let xf = Transform::translation(c.extend(0.0))
                    .after(&Transform::scale(Vec3::new(k, k, 1.0)))
                    .after(&rot)
                    .after(&Transform::translation(-vp.view_center.extend(0.0)));
                let frozen: Vec<String> = vp.frozen_layers.iter().filter_map(|h| d.layer(*h).map(|l| l.name.clone())).collect();
                for it in &m.items {
                    if frozen.iter().any(|n| n.eq_ignore_ascii_case(&it.style.layer)) {
                        continue;
                    }
                    let mapped = map_shape(&it.shape, &xf);
                    // Text stays text when the viewport shows it whole.
                    let mapped = match (&it.shape, &mapped) {
                        (Shape::Text { height, .. }, Shape::Text { .. }) if *height <= 0.0 => continue,
                        _ => mapped,
                    };
                    for piece in clip::shape_to_rect(&mapped, &rect) {
                        w.push(piece, &it.style, obj.handle);
                    }
                }
            }
            // The viewport's border, on its own layer.
            let corners = vec![rect.min, Vec2::new(rect.max.x, rect.min.y), rect.max, Vec2::new(rect.min.x, rect.max.y)];
            w.push(
                Shape::Path {
                    points: corners,
                    closed: true,
                },
                &style,
                obj.handle,
            );
            w.scene.stats.entities_drawn += 1;
            continue;
        }
        let inh = Inherit {
            source: obj.handle,
            ..Inherit::root()
        };
        w.entity(obj, &Transform::IDENTITY, &inh, 0);
    }
    w.finish()
}

/// How far one entity reaches as drawn (an empty box when it is hidden
/// or draws nothing): where a selection sits, for arrays and the like.
pub fn entity_bounds(d: &Drawing, h: Handle) -> BBox {
    let Some(obj) = d.get(h) else {
        return BBox::EMPTY;
    };
    let mut w = Walker::new(d);
    let inh = Inherit {
        source: obj.handle,
        ..Inherit::root()
    };
    w.entity(obj, &Transform::IDENTITY, &inh, 0);
    w.finish().bounds
}

/// Extend a bounding box by a shape.
pub fn shape_bounds(shape: &Shape, b: &mut BBox) {
    match shape {
        Shape::Line { a, b: bb } => {
            b.add(*a);
            b.add(*bb);
        }
        Shape::Circle { center, radius } => {
            b.add(*center - Vec2::new(*radius, *radius));
            b.add(*center + Vec2::new(*radius, *radius));
        }
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let arc = ArcSeg {
                center: *center,
                radius: *radius,
                start: *start,
                end: *end,
            };
            for p in curve::tessellate_arc(&arc, DEFAULT_STEP * 4.0) {
                b.add(p);
            }
        }
        Shape::Polyline { vertices, closed } => {
            for p in curve::tessellate_bulge_polyline(vertices, *closed, DEFAULT_STEP * 4.0) {
                b.add(p);
            }
        }
        Shape::Path { points, .. } => points.iter().for_each(|p| b.add(*p)),
        Shape::Fill { rings } => rings.iter().flatten().for_each(|p| b.add(*p)),
        Shape::Point { at } => b.add(*at),
        // Infinite: no extent of its own, so zooming to the drawing's
        // extents ignores construction lines.
        Shape::Ray { .. } => {}
        Shape::Lines { segments } => {
            for (p, q) in segments {
                b.add(*p);
                b.add(*q);
            }
        }
        Shape::Text { text, .. } => {
            if let Some(p) = shape.placement() {
                for c in crate::font::text_box(text, &p) {
                    b.add(c);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dwg::DwgVersion;

    fn ent(space: Space, layer: Handle) -> EntityCommon {
        EntityCommon {
            space,
            layer,
            linetype_scale: 1.0,
            ..Default::default()
        }
    }

    fn obj(h: u64, kind: ObjectKind, entity: Option<EntityCommon>, owner: u64) -> Object {
        Object {
            handle: Handle(h),
            type_code: 0,
            type_name: "TEST".into(),
            owner: Handle(owner),
            reactors: vec![],
            xdictionary: Handle::NULL,
            entity,
            kind,
            offset: 0,
            size: 0,
            note: None,
            raw: None,
            xdata: Vec::new(),
            unknown_tags: Vec::new(),
        }
    }

    /// An in-memory drawing: a layer, model space with a line, an arc and a
    /// block reference (scaled ×2, rotated 90°) to a block holding a circle.
    fn sample() -> Drawing {
        let mut objects = BTreeMap::new();
        let layer = Handle(0x10);
        objects.insert(
            layer,
            obj(
                0x10,
                ObjectKind::Layer(Layer {
                    name: "0".into(),
                    color: Color::Index(1),
                    plot: true,
                    ..Default::default()
                }),
                None,
                0,
            ),
        );
        let ms = Handle(0x1F);
        objects.insert(
            ms,
            obj(
                0x1F,
                ObjectKind::BlockHeader(BlockHeader {
                    name: "*Model_Space".into(),
                    ..Default::default()
                }),
                None,
                0,
            ),
        );
        let blk = Handle(0x20);
        objects.insert(
            blk,
            obj(
                0x20,
                ObjectKind::BlockHeader(BlockHeader {
                    name: "HOLE".into(),
                    base_point: Vec3::new(1.0, 0.0, 0.0),
                    ..Default::default()
                }),
                None,
                0,
            ),
        );
        objects.insert(
            Handle(0x30),
            obj(
                0x30,
                ObjectKind::Line(Line {
                    start: Vec3::ZERO,
                    end: Vec3::new(10.0, 0.0, 0.0),
                    extrusion: Vec3::Z,
                    thickness: 0.0,
                }),
                Some(ent(Space::Model, layer)),
                0,
            ),
        );
        objects.insert(
            Handle(0x31),
            obj(
                0x31,
                ObjectKind::Arc(Arc {
                    center: Vec3::ZERO,
                    radius: 5.0,
                    extrusion: Vec3::new(0.0, 0.0, -1.0),
                    start_angle: 0.0,
                    end_angle: std::f64::consts::FRAC_PI_2,
                    thickness: 0.0,
                }),
                Some(ent(Space::Model, layer)),
                0,
            ),
        );
        objects.insert(
            Handle(0x32),
            obj(
                0x32,
                ObjectKind::Insert(Insert {
                    insertion: Vec3::new(100.0, 0.0, 0.0),
                    scale: Vec3::new(2.0, 2.0, 2.0),
                    rotation: std::f64::consts::FRAC_PI_2,
                    extrusion: Vec3::Z,
                    block_header: blk,
                    ..Default::default()
                }),
                Some(ent(Space::Model, layer)),
                0,
            ),
        );
        objects.insert(
            Handle(0x40),
            obj(
                0x40,
                ObjectKind::Circle(Circle {
                    center: Vec3::new(1.0, 0.0, 0.0),
                    radius: 1.0,
                    extrusion: Vec3::Z,
                    thickness: 0.0,
                }),
                Some(ent(Space::Owned, layer)),
                0x20,
            ),
        );
        let mut d = Drawing {
            version: DwgVersion::R2018,
            maintenance_version: 0,
            codepage: 30,
            header: HeaderVars::default(),
            classes: vec![],
            objects: objects.into(),
            sections: vec![],
            diagnostics: vec![],
            index: DrawingIndex::default(),
        };
        d.rebuild_index();
        d
    }

    #[test]
    fn renders_lines_mirrored_arcs_and_blocks() {
        let d = sample();
        let scene = model_space(&d);
        assert_eq!(scene.stats.entities_drawn, 3);
        // Line.
        assert!(scene
            .items
            .iter()
            .any(|i| matches!(i.shape, Shape::Line { .. })));
        // The arc with extrusion (0,0,-1) is mirrored: CCW 0..90° in its OCS
        // lands in the second quadrant of the plan.
        let arc = scene.items.iter().find_map(|i| match i.shape {
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => Some((center, radius, start, end)),
            _ => None,
        });
        let (c, r, s, e) = arc.unwrap();
        assert!(c.length() < 1e-9 && (r - 5.0).abs() < 1e-9);
        let mid = (s + curve::normalize_sweep(e - s) / 2.0).rem_euclid(TAU);
        assert!(
            (mid - 3.0 * std::f64::consts::FRAC_PI_4).abs() < 1e-9,
            "mid angle {mid}"
        );
        // The block's circle sits on the base point, so it lands on the
        // insertion point, radius doubled.
        let circ = scene.items.iter().find_map(|i| match i.shape {
            Shape::Circle { center, radius } => Some((center, radius)),
            _ => None,
        });
        let (c, r) = circ.unwrap();
        assert!((c.x - 100.0).abs() < 1e-9 && c.y.abs() < 1e-9 && (r - 2.0).abs() < 1e-9);
        // Layer color 1 (red) is inherited through BYLAYER.
        assert!(scene.items.iter().all(|i| i.style.rgb == (255, 0, 0)));
    }

    use crate::standards::{new_drawing, Template};

    fn first_item_of(scene: &Scene, src: Handle) -> &Item {
        scene.items.iter().find(|i| i.source == src).unwrap()
    }

    #[test]
    fn linetypes_and_lineweights_come_from_the_layer() {
        let mut d = new_drawing(Template::Metric);
        let layer = |d: &Drawing, n: &str| d.layer_by_name(n).unwrap().0;
        let line = || ObjectKind::Line(Line::new(Vec2::ZERO, Vec2::new(100.0, 0.0)));
        let visible = d.add_entity(layer(&d, "VISIBLE"), line()).unwrap();
        let hidden = d.add_entity(layer(&d, "HIDDEN"), line()).unwrap();
        let center = d.add_entity(layer(&d, "CENTER"), line()).unwrap();
        // An entity's own linetype scale and lineweight win over the layer.
        let own = d.add_entity(layer(&d, "HIDDEN"), line()).unwrap();
        {
            let e = d.objects.get_mut(&own).unwrap().entity.as_mut().unwrap();
            e.linetype_scale = 2.0;
            e.lineweight = Lineweight::Hundredths(35).code();
            e.color = Color::Index(5);
        }
        d.header.ltscale = Some(0.5);
        let scene = model_space(&d);
        let v = &first_item_of(&scene, visible).style;
        assert_eq!(v.lineweight, Lineweight::Hundredths(50));
        assert!(v.pattern.is_empty() && v.linetype.is_empty());
        assert_eq!(v.color, Color::Index(7));
        let h = &first_item_of(&scene, hidden).style;
        assert_eq!(h.linetype, "HIDDEN");
        assert_eq!(h.pattern, vec![1.5, -0.75], "pattern × LTSCALE 0.5");
        assert_eq!(h.lineweight, Lineweight::Hundredths(25));
        assert_eq!(h.color, Color::Index(1));
        let c = &first_item_of(&scene, center).style;
        assert_eq!(c.pattern, vec![6.0, -0.75, 1.5, -0.75]);
        let o = &first_item_of(&scene, own).style;
        assert_eq!(o.pattern, vec![3.0, -1.5], "× LTSCALE × entity scale");
        assert_eq!(o.lineweight, Lineweight::Hundredths(35));
        assert_eq!(o.color, Color::Index(5));
        // Changing a default layer changes its entities.
        let hl = layer(&d, "HIDDEN");
        d.set_layer_lineweight(hl, Lineweight::Hundredths(35))
            .unwrap();
        d.set_layer_color(hl, Color::Index(6)).unwrap();
        let scene = model_space(&d);
        let h = &first_item_of(&scene, hidden).style;
        assert_eq!(
            (h.lineweight, h.color),
            (Lineweight::Hundredths(35), Color::Index(6))
        );
    }

    #[test]
    fn by_block_linetypes_take_the_block_scale_and_their_own() {
        // A block inserted ×10 on HIDDEN, holding lines on layer 0 with a
        // linetype scale of 2: by layer and by block, both dash at the
        // HIDDEN pattern × 2 × 10.
        let mut d = new_drawing(Template::Metric);
        let zero = d.layer_by_name("0").unwrap().0;
        let hidden = d.layer_by_name("HIDDEN").unwrap().0;
        let blk = d.new_handle();
        d.objects.insert(
            blk,
            obj(
                blk.0,
                ObjectKind::BlockHeader(BlockHeader {
                    name: "PART".into(),
                    ..Default::default()
                }),
                None,
                0,
            ),
        );
        for lt in [LinetypeRef::ByLayer, LinetypeRef::ByBlock] {
            let h = d.new_handle();
            let e = EntityCommon {
                linetype: lt,
                linetype_scale: 2.0,
                ..ent(Space::Owned, zero)
            };
            let line = Line::new(Vec2::ZERO, Vec2::new(10.0, 0.0));
            d.objects
                .insert(h, obj(h.0, ObjectKind::Line(line), Some(e), blk.0));
        }
        let ins = d
            .add_entity(
                hidden,
                ObjectKind::Insert(Insert {
                    scale: Vec3::new(10.0, 10.0, 10.0),
                    extrusion: Vec3::Z,
                    block_header: blk,
                    ..Default::default()
                }),
            )
            .unwrap();
        d.rebuild_index();
        let scene = model_space(&d);
        let patterns: Vec<&Vec<f64>> = scene
            .items
            .iter()
            .filter(|i| i.source == ins)
            .map(|i| &i.style.pattern)
            .collect();
        assert_eq!(patterns, vec![&vec![60.0, -30.0]; 2]);
    }

    #[test]
    fn name_only_linetypes_from_dwg_use_the_standard_patterns() {
        // A DWG linetype record keeps only its name; CENTER still dashes.
        let mut d = new_drawing(Template::Imperial);
        let ctr = d.linetype_by_name("CENTER").unwrap();
        d.objects.get_mut(&ctr).unwrap().kind = ObjectKind::NamedRecord(NamedRecord {
            name: "CENTER".into(),
        });
        let l = d.layer_by_name("CENTER").unwrap().0;
        let h = d
            .add_entity(
                l,
                ObjectKind::Line(Line::new(Vec2::ZERO, Vec2::new(5.0, 0.0))),
            )
            .unwrap();
        let scene = model_space(&d);
        assert_eq!(
            first_item_of(&scene, h).style.pattern,
            vec![0.5, -0.0625, 0.125, -0.0625]
        );
    }

    #[test]
    fn dimensions_without_a_block_are_drawn_in_their_layer_colour() {
        let mut d = new_drawing(Template::Metric);
        let dl = d.layer_by_name("DIMENSION").unwrap().0;
        let h = d
            .add_entity(
                dl,
                ObjectKind::Dimension(Dimension::linear(
                    Vec2::ZERO,
                    Vec2::new(200.0, 0.0),
                    Vec2::new(0.0, 15.0),
                    0.0,
                )),
            )
            .unwrap();
        let scene = model_space(&d);
        let parts: Vec<&Item> = scene.items.iter().filter(|i| i.source == h).collect();
        assert!(parts.len() >= 6, "lines, arrows, extension lines, text");
        assert!(parts.iter().all(|i| i.style.color == Color::Index(3)));
        assert!(parts
            .iter()
            .all(|i| i.style.lineweight == Lineweight::Hundredths(25)));
        assert!(parts.iter().any(|i| matches!(
            &i.shape,
            Shape::Text { text, .. } if text == "200.00"
        )));
        assert_eq!(scene.stats.entities_drawn, 1);
        assert!(scene.stats.skipped.is_empty());
        // The text's real extent counts in the drawing's bounds.
        assert!(scene.bounds.max.y > 15.0 + 1.0);
    }
}

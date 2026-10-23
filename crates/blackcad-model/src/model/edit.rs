//! Editing the drawing database: handles, layers and new entities.
//!
//! Every layer can be changed, the default ones included: colour,
//! linetype, lineweight, name and state. The exception is layer `0`, which
//! DWG and DXF require: it cannot be renamed or deleted, though its
//! properties can change. Names follow DWG's rules and compare without
//! regard to case, as DWG compares them.
//!
//! Edits change the objects in memory and keep the lookup index current.
//! Saving is separate (`native::write_file`, or a change set with
//! `native::append_changes`).

use super::*;
use std::fmt;

/// Why an edit was refused. The message is for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The name is empty, too long, or has a character DWG does not allow.
    BadName(String),
    /// Another record of the same table already has this name.
    NameInUse(String),
    /// No object with that handle, or not of the kind needed.
    NotFound(Handle),
    /// A value the property cannot take (for example a layer colour of
    /// "by layer").
    BadValue(String),
    /// Layer `0` cannot be renamed or deleted.
    LayerZero,
    /// The current layer cannot be deleted or frozen.
    CurrentLayer,
    /// The record is still used by that many objects.
    InUse { name: String, count: usize },
    /// The drawing lacks a table the edit needs.
    MissingTable(&'static str),
    /// `add_entity` does not create this kind of object yet.
    Unsupported(String),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::BadName(m) => write!(f, "invalid name: {m}"),
            EditError::NameInUse(n) => write!(f, "the name {n:?} is already used"),
            EditError::NotFound(h) => write!(f, "no suitable object with handle {h}"),
            EditError::BadValue(m) => write!(f, "invalid value: {m}"),
            EditError::LayerZero => f.write_str("layer 0 cannot be renamed or deleted"),
            EditError::CurrentLayer => f.write_str("the current layer cannot be deleted or frozen"),
            EditError::InUse { name, count } => {
                write!(f, "{name:?} is still used by {count} objects")
            }
            EditError::MissingTable(t) => write!(f, "the drawing has no {t} table"),
            EditError::Unsupported(m) => write!(f, "not supported yet: {m}"),
        }
    }
}

impl std::error::Error for EditError {}

/// Visibility and protection of a layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerState {
    pub on: bool,
    pub frozen: bool,
    pub locked: bool,
    pub plot: bool,
}

/// Characters DWG does not allow in a symbol table name.
const FORBIDDEN: &str = "<>/\\\":;?*|,=`";

/// Check a table-record name (layer, linetype, style …) against DWG's
/// rules: not empty, at most 255 characters, no leading or trailing
/// spaces, none of `< > / \ " : ; ? * | , = `` ` `` and no control
/// characters.
pub fn check_name(name: &str) -> Result<(), EditError> {
    if name.is_empty() {
        return Err(EditError::BadName("a name cannot be empty".into()));
    }
    if name.chars().count() > 255 {
        return Err(EditError::BadName(
            "a name can have at most 255 characters".into(),
        ));
    }
    if name.trim() != name {
        return Err(EditError::BadName(
            "a name cannot start or end with a space".into(),
        ));
    }
    if let Some(c) = name
        .chars()
        .find(|c| c.is_control() || FORBIDDEN.contains(*c))
    {
        return Err(EditError::BadName(format!(
            "{c:?} is not allowed in a name"
        )));
    }
    Ok(())
}

/// DWG type name and number of an entity kind `add_entity` can create.
pub fn entity_type(kind: &ObjectKind) -> Option<(&'static str, u16)> {
    use ObjectKind as K;
    Some(match kind {
        K::Line(_) => ("LINE", 0x13),
        K::Circle(_) => ("CIRCLE", 0x12),
        K::Arc(_) => ("ARC", 0x11),
        K::Point(_) => ("POINT", 0x1B),
        K::Ellipse(_) => ("ELLIPSE", 0x23),
        K::LwPolyline(_) => ("LWPOLYLINE", 0x4D),
        K::Text(_) => ("TEXT", 0x01),
        K::MText(_) => ("MTEXT", 0x2C),
        K::Solid(s) if s.is_trace => ("TRACE", 0x20),
        K::Solid(_) => ("SOLID", 0x1F),
        K::Face3d(_) => ("3DFACE", 0x1C),
        K::Spline(_) => ("SPLINE", 0x24),
        K::Insert(i) if i.array.is_some() => ("MINSERT", 0x08),
        K::Insert(_) => ("INSERT", 0x07),
        K::Hatch(_) => ("HATCH", 0x4E),
        K::Xline(_) => ("XLINE", 0x29),
        K::Ray(_) => ("RAY", 0x28),
        K::Leader(_) => ("LEADER", 0x2D),
        K::Tolerance(_) => ("TOLERANCE", 0x2E),
        K::MLine(_) => ("MLINE", 0x2F),
        // Class-based: the number is the drawing's (`Drawing::class_number`).
        K::Image(_) => ("IMAGE", 0),
        K::Wipeout(_) => ("WIPEOUT", 0),
        K::PaperViewport(_) => ("VIEWPORT", 0x22),
        K::Attribute(a) if a.is_definition => ("ATTDEF", 0x03),
        K::Attribute(_) => ("ATTRIB", 0x02),
        K::Dimension(d) => match d.kind {
            DimensionKind::Ordinate => ("DIMENSION_ORDINATE", 0x14),
            DimensionKind::Linear => ("DIMENSION_LINEAR", 0x15),
            DimensionKind::Aligned => ("DIMENSION_ALIGNED", 0x16),
            DimensionKind::Angular3Pt => ("DIMENSION_ANG_3PT", 0x17),
            DimensionKind::Angular2Line => ("DIMENSION_ANG_2LN", 0x18),
            DimensionKind::Radius => ("DIMENSION_RADIUS", 0x19),
            DimensionKind::Diameter => ("DIMENSION_DIAMETER", 0x1A),
        },
        _ => return None,
    })
}

/// Classes the model can register in a drawing that lacks them (a DXF
/// without its CLASSES section, a new drawing): DXF name, C++ class name,
/// application, proxy flags, and whether it is an entity class. The names
/// are the ones the DXF reference and DXF files show for these classes.
pub const KNOWN_CLASSES: &[(&str, &str, &str, u16, bool)] = &[
    ("IMAGE", "AcDbRasterImage", "ISM", 127, true),
    ("WIPEOUT", "AcDbWipeout", "WipeOut", 127, true),
    ("IMAGEDEF", "AcDbRasterImageDef", "ISM", 0, false),
    ("IMAGEDEF_REACTOR", "AcDbRasterImageDefReactor", "ISM", 1, false),
    ("RASTERVARIABLES", "AcDbRasterVariables", "ISM", 0, false),
];

impl Drawing {
    /// The DWG type number of the class with this DXF name (any case),
    /// registering it first when the drawing lacks it and it is one of
    /// [`KNOWN_CLASSES`]. `None` for other unknown classes.
    pub fn class_number(&mut self, dxf_name: &str) -> Option<u16> {
        if let Some(c) = self.classes.iter().find(|c| c.dxf_name.eq_ignore_ascii_case(dxf_name)) {
            return Some(c.number);
        }
        let &(name, cpp, app, flags, entity) =
            KNOWN_CLASSES.iter().find(|k| k.0.eq_ignore_ascii_case(dxf_name))?;
        let number = self.classes.iter().map(|c| c.number).max().map_or(500, |n| n.saturating_add(1)).max(500);
        self.classes.push(ClassDef {
            number,
            proxy_flags: flags,
            app_name: app.into(),
            cpp_name: cpp.into(),
            dxf_name: name.into(),
            was_zombie: false,
            item_class_id: if entity { 0x1F2 } else { 0x1F3 },
            instance_count: 0,
            dwg_version: 0,
            maintenance_version: 0,
        });
        Some(number)
    }

    /// A handle no object uses yet, taken from `$HANDSEED`, which moves on.
    /// It is also kept above every handle in the drawing, so a seed that
    /// fell behind (a damaged or hand-made file) cannot hand out a used one.
    /// When the seed has reached the top of the range (a damaged file
    /// again), the lowest free handle is used instead.
    pub fn new_handle(&mut self) -> Handle {
        let above_used = self
            .objects
            .keys()
            .next_back()
            .map(|h| h.0.saturating_add(1))
            .unwrap_or(1);
        let seed = self.header.handseed.unwrap_or(1).max(above_used).max(1);
        if self.objects.contains_key(&Handle(seed)) {
            let mut free = 1u64;
            for k in self.objects.keys() {
                if k.0 > free {
                    break;
                }
                if k.0 == free {
                    free += 1;
                }
            }
            return Handle(free);
        }
        self.header.handseed = Some(seed.saturating_add(1));
        Handle(seed)
    }

    /// The layer table control object, from the header or by type.
    fn control(&self, from_header: Handle, type_name: &str) -> Option<Handle> {
        let ok = |h: Handle| {
            self.get(h).is_some_and(|o| {
                matches!(o.kind, ObjectKind::Control(_)) && o.type_name == type_name
            })
        };
        if ok(from_header) {
            return Some(from_header);
        }
        self.objects
            .values()
            .find(|o| o.type_name == type_name && matches!(o.kind, ObjectKind::Control(_)))
            .map(|o| o.handle)
    }

    fn control_entries(&mut self, control: Handle) -> Option<&mut Vec<Handle>> {
        match &mut self.objects.get_mut(&control)?.kind {
            ObjectKind::Control(c) => Some(&mut c.entries),
            _ => None,
        }
    }

    /// Every layer, in the order the layer table lists them (then any the
    /// table does not list, by handle).
    pub fn layers(&self) -> Vec<(Handle, &Layer)> {
        let mut out = self.layers_in_table();
        for o in self.objects.values() {
            if let ObjectKind::Layer(l) = &o.kind {
                if !out.iter().any(|(x, _)| *x == o.handle) {
                    out.push((o.handle, l));
                }
            }
        }
        out
    }

    /// The layers the layer table lists, in its order: [`Self::layers`]
    /// without the search for layers it does not list.
    pub fn layers_in_table(&self) -> Vec<(Handle, &Layer)> {
        let mut out: Vec<(Handle, &Layer)> = Vec::new();
        if let Some(ctl) = self.control(self.header.layer_control, "LAYER_CONTROL") {
            if let Some(ObjectKind::Control(c)) = self.get(ctl).map(|o| &o.kind) {
                for h in &c.entries {
                    if let Some(l) = self.layer(*h) {
                        if !out.iter().any(|(x, _)| x == h) {
                            out.push((*h, l));
                        }
                    }
                }
            }
        }
        out
    }

    /// A layer by name, compared without regard to case.
    pub fn layer_by_name(&self, name: &str) -> Option<(Handle, &Layer)> {
        self.objects.values().find_map(|o| match &o.kind {
            ObjectKind::Layer(l) if l.name.eq_ignore_ascii_case(name) => Some((o.handle, l)),
            _ => None,
        })
    }

    /// A linetype record by name (with its pattern or, from DWG, name
    /// only), compared without regard to case.
    pub fn linetype_by_name(&self, name: &str) -> Option<Handle> {
        self.objects.values().find_map(|o| match &o.kind {
            ObjectKind::Linetype(l) if l.name.eq_ignore_ascii_case(name) => Some(o.handle),
            ObjectKind::NamedRecord(r)
                if o.type_name == "LTYPE" && r.name.eq_ignore_ascii_case(name) =>
            {
                Some(o.handle)
            }
            _ => None,
        })
    }

    /// The current layer (`$CLAYER`), if it is a layer.
    pub fn current_layer(&self) -> Option<Handle> {
        let h = self.header.clayer;
        self.layer(h).map(|_| h)
    }

    fn layer_mut(&mut self, h: Handle) -> Result<&mut Layer, EditError> {
        match self.objects.get_mut(&h).map(|o| &mut o.kind) {
            Some(ObjectKind::Layer(l)) => Ok(l),
            _ => Err(EditError::NotFound(h)),
        }
    }

    fn check_layer_name_free(&self, name: &str, except: Handle) -> Result<(), EditError> {
        check_name(name)?;
        match self.layer_by_name(name) {
            Some((h, l)) if h != except => Err(EditError::NameInUse(l.name.clone())),
            _ => Ok(()),
        }
    }

    fn check_layer_color(color: Color) -> Result<(), EditError> {
        match color {
            Color::Index(1..=255) | Color::Rgb(..) => Ok(()),
            other => Err(EditError::BadValue(format!(
                "a layer needs a real colour, not {other:?}"
            ))),
        }
    }

    fn check_layer_weight(weight: Lineweight) -> Result<(), EditError> {
        match weight {
            Lineweight::ByLayer | Lineweight::ByBlock => Err(EditError::BadValue(format!(
                "a layer's lineweight cannot be {weight}"
            ))),
            _ => Ok(()),
        }
    }

    fn check_linetype_record(&self, linetype: Handle) -> Result<(), EditError> {
        if linetype.is_null() {
            return Ok(());
        }
        let o = self.get(linetype).ok_or(EditError::NotFound(linetype))?;
        let is_ltype = matches!(o.kind, ObjectKind::Linetype(_))
            || (o.type_name == "LTYPE" && matches!(o.kind, ObjectKind::NamedRecord(_)));
        if !is_ltype {
            return Err(EditError::NotFound(linetype));
        }
        let name = self.record_name(linetype).unwrap_or("");
        if name.eq_ignore_ascii_case("ByLayer") || name.eq_ignore_ascii_case("ByBlock") {
            return Err(EditError::BadValue(format!(
                "a layer's linetype cannot be {name}"
            )));
        }
        Ok(())
    }

    /// Add a layer to the layer table. `linetype` is an LTYPE record (or
    /// null for continuous).
    pub fn add_layer(
        &mut self,
        name: &str,
        color: Color,
        linetype: Handle,
        weight: Lineweight,
    ) -> Result<Handle, EditError> {
        self.check_layer_name_free(name, Handle::NULL)?;
        Self::check_layer_color(color)?;
        Self::check_layer_weight(weight)?;
        self.check_linetype_record(linetype)?;
        let control = self
            .control(self.header.layer_control, "LAYER_CONTROL")
            .ok_or(EditError::MissingTable("layer"))?;
        let h = self.new_handle();
        let mut layer = Layer {
            name: name.into(),
            color,
            linetype,
            lineweight: weight.code(),
            ..Default::default()
        };
        layer.flags = layer.dwg_flags();
        self.insert_object(Object::record(
            h,
            "LAYER",
            0x33,
            control,
            ObjectKind::Layer(layer),
        ));
        if let Some(entries) = self.control_entries(control) {
            entries.push(h);
        }
        Ok(h)
    }

    /// Rename a layer. Layer `0` keeps its name.
    pub fn rename_layer(&mut self, layer: Handle, name: &str) -> Result<(), EditError> {
        let current = self.layer(layer).ok_or(EditError::NotFound(layer))?;
        if current.name == "0" {
            return Err(EditError::LayerZero);
        }
        self.check_layer_name_free(name, layer)?;
        self.layer_mut(layer)?.name = name.into();
        Ok(())
    }

    /// Change a layer's colour (an index 1–255 or a true colour).
    pub fn set_layer_color(&mut self, layer: Handle, color: Color) -> Result<(), EditError> {
        Self::check_layer_color(color)?;
        self.layer_mut(layer)?.color = color;
        Ok(())
    }

    /// Change a layer's lineweight (a standard weight, or the default).
    pub fn set_layer_lineweight(
        &mut self,
        layer: Handle,
        weight: Lineweight,
    ) -> Result<(), EditError> {
        Self::check_layer_weight(weight)?;
        let l = self.layer_mut(layer)?;
        l.lineweight = weight.code();
        l.flags = l.dwg_flags();
        Ok(())
    }

    /// Change a layer's linetype (an LTYPE record, or null for
    /// continuous).
    pub fn set_layer_linetype(&mut self, layer: Handle, linetype: Handle) -> Result<(), EditError> {
        self.check_linetype_record(linetype)?;
        self.layer_mut(layer)?.linetype = linetype;
        Ok(())
    }

    /// The state of a layer.
    pub fn layer_state(&self, layer: Handle) -> Option<LayerState> {
        self.layer(layer).map(|l| LayerState {
            on: !l.off,
            frozen: l.frozen,
            locked: l.locked,
            plot: l.plot,
        })
    }

    /// Switch a layer on or off, freeze or thaw, lock or unlock it, and
    /// set whether it plots. The current layer cannot be frozen.
    pub fn set_layer_state(&mut self, layer: Handle, s: LayerState) -> Result<(), EditError> {
        if s.frozen && self.header.clayer == layer {
            return Err(EditError::CurrentLayer);
        }
        let l = self.layer_mut(layer)?;
        l.off = !s.on;
        l.frozen = s.frozen;
        l.locked = s.locked;
        l.plot = s.plot;
        l.flags = l.dwg_flags();
        Ok(())
    }

    /// Make a layer current: new objects go on it. A frozen layer cannot be
    /// made current.
    pub fn set_current_layer(&mut self, layer: Handle) -> Result<(), EditError> {
        let l = self.layer(layer).ok_or(EditError::NotFound(layer))?;
        if l.frozen {
            return Err(EditError::BadValue(format!(
                "layer {:?} is frozen; thaw it first",
                l.name
            )));
        }
        self.header.clayer = layer;
        Ok(())
    }

    /// How many objects refer to `h` as their layer (entities, and
    /// extended data naming the layer).
    pub fn layer_use_count(&self, h: Handle) -> usize {
        self.objects
            .values()
            .filter(|o| {
                o.entity.as_ref().is_some_and(|e| e.layer == h)
                    || o.xdata.iter().any(|x| x.items.contains(&XValue::Layer(h)))
            })
            .count()
    }

    /// How many entities are on layer `h`, as a layer list shows it: block
    /// start and end markers, polyline vertices and sequence ends are parts
    /// of other objects and not counted (unlike [`Self::layer_use_count`],
    /// which counts every reference).
    pub fn layer_entity_count(&self, h: Handle) -> usize {
        self.objects
            .values()
            .filter(|o| {
                o.entity.as_ref().is_some_and(|e| e.layer == h)
                    && !matches!(
                        o.kind,
                        ObjectKind::BlockBegin { .. }
                            | ObjectKind::BlockEnd
                            | ObjectKind::SeqEnd
                            | ObjectKind::Vertex(_)
                    )
            })
            .count()
    }

    /// [`Self::layer_entity_count`] for every layer at once, in one pass.
    pub fn entity_counts_by_layer(&self) -> BTreeMap<Handle, usize> {
        let mut out = BTreeMap::new();
        for o in self.objects.values() {
            if let Some(e) = &o.entity {
                if !matches!(
                    o.kind,
                    ObjectKind::BlockBegin { .. } | ObjectKind::BlockEnd | ObjectKind::SeqEnd | ObjectKind::Vertex(_)
                ) {
                    *out.entry(e.layer).or_default() += 1;
                }
            }
        }
        out
    }

    /// The layer table and its layer `0`, made when the drawing has none
    /// (a DXF of entities alone, say). Layer `0`'s handle. The current
    /// layer becomes `0` when it is not a layer.
    pub fn ensure_layer_zero(&mut self) -> Handle {
        let control = match self.control(self.header.layer_control, "LAYER_CONTROL") {
            Some(c) => c,
            None => {
                let c = self.new_handle();
                self.insert_object(Object::record(
                    c,
                    "LAYER_CONTROL",
                    0x32,
                    Handle::NULL,
                    ObjectKind::Control(Control::default()),
                ));
                c
            }
        };
        self.header.layer_control = control;
        let zero = match self.layer_by_name("0") {
            Some((h, _)) => h,
            None => {
                let h = self.new_handle();
                let mut layer = Layer {
                    name: "0".into(),
                    color: Color::Index(7),
                    ..Default::default()
                };
                layer.flags = layer.dwg_flags();
                self.insert_object(Object::record(h, "LAYER", 0x33, control, ObjectKind::Layer(layer)));
                if let Some(entries) = self.control_entries(control) {
                    entries.insert(0, h);
                }
                h
            }
        };
        if self.layer(self.header.clayer).is_none() {
            self.header.clayer = zero;
        }
        zero
    }

    /// Model space's block record, made when the drawing has none, with
    /// what a drawing always has beside it: the block table, the paper
    /// space record, and each one's BLOCK and ENDBLK entities on layer `0`
    /// (made too if missing).
    pub fn ensure_model_space(&mut self) -> Handle {
        let ms = self.model_space();
        if !ms.is_null() && self.block_header(ms).is_some() {
            return ms;
        }
        // There, but not found yet (the index not built, say).
        let named = |d: &Drawing, name: &str| d.block_by_name(name).map(|(h, _)| h);
        if let (Some(m), Some(p)) = (named(self, "*Model_Space"), named(self, "*Paper_Space")) {
            if self.header.model_space.is_null() {
                self.header.model_space = m;
            }
            if self.header.paper_space.is_null() {
                self.header.paper_space = p;
            }
            self.rebuild_index();
            return self.model_space();
        }
        let layer0 = self.ensure_layer_zero();
        let control = match self.control(self.header.block_control, "BLOCK_CONTROL") {
            Some(c) => c,
            None => {
                let c = self.new_handle();
                self.insert_object(Object::record(
                    c,
                    "BLOCK_CONTROL",
                    0x30,
                    Handle::NULL,
                    ObjectKind::Control(Control::default()),
                ));
                c
            }
        };
        self.header.block_control = control;
        let mut made = [Handle::NULL; 2];
        for (i, name) in ["*Model_Space", "*Paper_Space"].into_iter().enumerate() {
            if let Some((h, _)) = self.block_by_name(name) {
                made[i] = h;
                continue;
            }
            let h = self.new_handle();
            let begin = self.new_handle();
            let end = self.new_handle();
            self.insert_object(Object::record(
                h,
                "BLOCK_HEADER",
                0x31,
                control,
                ObjectKind::BlockHeader(BlockHeader {
                    name: name.into(),
                    insert_units: self.header.insunits.unwrap_or(0),
                    block_entity: begin,
                    endblk: end,
                    ..Default::default()
                }),
            ));
            for (eh, type_name, code, kind) in [
                (begin, "BLOCK", 0x04, ObjectKind::BlockBegin { name: name.into() }),
                (end, "ENDBLK", 0x05, ObjectKind::BlockEnd),
            ] {
                let mut o = Object::record(eh, type_name, code, h, kind);
                o.entity = Some(EntityCommon {
                    layer: layer0,
                    ..Default::default()
                });
                self.insert_object(o);
            }
            made[i] = h;
        }
        if let Some(ObjectKind::Control(c)) = self.objects.get_mut(&control).map(|o| &mut o.kind) {
            c.model_space = made[0];
            c.paper_space = made[1];
        }
        self.header.model_space = made[0];
        self.header.paper_space = made[1];
        self.rebuild_index();
        made[0]
    }

    /// Delete a layer nothing uses. Layer `0` and the current layer stay.
    pub fn delete_layer(&mut self, layer: Handle) -> Result<(), EditError> {
        let l = self.layer(layer).ok_or(EditError::NotFound(layer))?;
        if l.name == "0" {
            return Err(EditError::LayerZero);
        }
        if self.header.clayer == layer {
            return Err(EditError::CurrentLayer);
        }
        let count = self.layer_use_count(layer);
        if count > 0 {
            return Err(EditError::InUse {
                name: l.name.clone(),
                count,
            });
        }
        let owner = self.get(layer).map(|o| o.owner).unwrap_or_default();
        self.objects.remove(&layer);
        for ctl in [owner, self.header.layer_control] {
            if let Some(entries) = self.control_entries(ctl) {
                entries.retain(|h| *h != layer);
            }
        }
        self.rebuild_index();
        Ok(())
    }

    /// Add an entity to model space on `layer` (null: the current layer).
    /// Its colour, linetype and lineweight are by layer; its linetype scale
    /// is the drawing's `$CELTSCALE`.
    /// A drawing with no model space (a damaged file, or a DXF of entities
    /// alone that the reader could not complete) gets one first.
    pub fn add_entity(&mut self, layer: Handle, kind: ObjectKind) -> Result<Handle, EditError> {
        let ms = self.ensure_model_space();
        self.add_entity_in(ms, layer, kind)
    }

    /// Add an entity to a block definition (or to model or paper space,
    /// which are blocks too) on `layer` (null: the current layer).
    pub fn add_entity_in(
        &mut self,
        block: Handle,
        layer: Handle,
        kind: ObjectKind,
    ) -> Result<Handle, EditError> {
        let (type_name, mut type_code) = entity_type(&kind).ok_or_else(|| {
            EditError::Unsupported(format!("creating {:?} objects", variant_name(&kind)))
        })?;
        if type_code == 0 {
            type_code = self.class_number(type_name).ok_or_else(|| {
                EditError::Unsupported(format!("creating {type_name} objects"))
            })?;
        }
        if self.block_header(block).is_none() {
            return Err(EditError::NotFound(block));
        }
        let layer = if layer.is_null() {
            self.current_layer()
                .or_else(|| self.layer_by_name("0").map(|(h, _)| h))
                .ok_or(EditError::MissingTable("layer"))?
        } else {
            layer
        };
        if self.layer(layer).is_none() {
            return Err(EditError::NotFound(layer));
        }
        let space = if block == self.model_space() {
            Space::Model
        } else if block == self.paper_space() {
            Space::Paper
        } else {
            Space::Owned
        };
        let h = self.new_handle();
        let mut o = Object::record(
            h,
            type_name,
            type_code,
            if space == Space::Owned {
                block
            } else {
                Handle::NULL
            },
            kind,
        );
        let (color, linetype, lineweight) = self.current_properties();
        o.entity = Some(EntityCommon {
            space,
            layer,
            color,
            linetype,
            lineweight,
            linetype_scale: self
                .header
                .celtscale
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or(1.0),
            ..Default::default()
        });
        // A block that lists its entities (R2004+ files) must list this
        // one too, or it would not be drawn.
        let listed = matches!(
            self.objects.get(&block).map(|o| &o.kind),
            Some(ObjectKind::BlockHeader(b)) if !b.entities.is_empty()
        );
        if listed {
            self.objects.list_insert(block, None, h);
        }
        self.insert_object(o);
        Ok(h)
    }

    /// Erase an entity, with the objects it owns (a polyline's vertices,
    /// an insert's attributes).
    ///
    /// Entities that other objects depend on are refused for now: one
    /// with reactors (an associative dimension follows it, a group lists
    /// it) or an extension dictionary. Erasing those needs the dependants
    /// updated too, which is not built yet.
    pub fn erase_entity(&mut self, h: Handle) -> Result<(), EditError> {
        let o = self.objects.get(&h).ok_or(EditError::NotFound(h))?;
        let Some(e) = &o.entity else {
            return Err(EditError::BadValue(format!("{} is not an entity", o.type_name)));
        };
        if !o.reactors.is_empty() || !o.xdictionary.is_null() {
            return Err(EditError::Unsupported(format!(
                "erasing a {} that other objects depend on",
                o.type_name
            )));
        }
        let block = match e.space {
            Space::Model => self.index.model_space,
            Space::Paper => self.index.paper_space,
            _ => o.owner,
        };
        // What it owns, depth first.
        let mut gone = vec![h];
        let mut i = 0;
        while i < gone.len() {
            if let Some(kids) = self.index.owned.get(&gone[i]) {
                gone.extend(kids.iter().copied());
            }
            i += 1;
        }
        for g in &gone {
            self.objects.remove(g);
            self.index.owned.remove(g);
        }
        if let Some(list) = self.index.owned.get_mut(&block) {
            list.retain(|x| *x != h);
        }
        self.objects.list_remove(block, h);
        Ok(())
    }

    /// The colour, linetype and lineweight new entities get: `$CECOLOR`,
    /// `$CELTYPE` and `$CELWEIGHT` (by layer when unset).
    pub fn current_properties(&self) -> (Color, LinetypeRef, u8) {
        let raw = |name: &str| self.header.raw.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        let color = match raw("CECOLOR") {
            None | Some("BYLAYER") | Some("256") => Color::ByLayer,
            Some("BYBLOCK") | Some("0") => Color::ByBlock,
            Some(t) if t.starts_with("RGB(") => {
                let v: Vec<u8> = t[4..t.len().saturating_sub(1)].split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if v.len() == 3 {
                    Color::Rgb(v[0], v[1], v[2])
                } else {
                    Color::ByLayer
                }
            }
            Some(t) => match t.parse::<i16>() {
                Ok(i) if (1..=255).contains(&i) => Color::Index(i as u8),
                _ => Color::ByLayer,
            },
        };
        let linetype = match self.record_name(self.header.celtype) {
            None => LinetypeRef::ByLayer,
            Some(n) if n.eq_ignore_ascii_case("ByLayer") => LinetypeRef::ByLayer,
            Some(n) if n.eq_ignore_ascii_case("ByBlock") => LinetypeRef::ByBlock,
            Some(_) => LinetypeRef::Handle(self.header.celtype),
        };
        let lineweight = raw("FLAGS")
            .and_then(|t| t.parse::<i64>().ok())
            .map(|f| (f & 0x1F) as u8)
            .map(Lineweight::from_code)
            .map(|w| w.code())
            .unwrap_or(Lineweight::CODE_BY_LAYER);
        (color, linetype, lineweight)
    }

    /// Set a header variable kept as text (see `HeaderVars::raw`).
    pub fn set_header_text(&mut self, name: &str, value: &str) {
        match self.header.raw.iter_mut().find(|(k, _)| k == name) {
            Some(e) => e.1 = value.to_string(),
            None => self.header.raw.push((name.to_string(), value.to_string())),
        }
    }

    /// Make `color` the colour of new entities (`$CECOLOR`).
    pub fn set_current_color(&mut self, color: Color) -> Result<(), EditError> {
        let text = match color {
            Color::ByLayer => "BYLAYER".to_string(),
            Color::ByBlock => "BYBLOCK".to_string(),
            Color::Index(i) if i >= 1 => i.to_string(),
            Color::Rgb(r, g, b) => format!("RGB({r},{g},{b})"),
            other => return Err(EditError::BadValue(format!("{other:?} cannot be the current colour"))),
        };
        self.set_header_text("CECOLOR", &text);
        Ok(())
    }

    /// Make `linetype` (an LTYPE record, ByLayer or ByBlock included) the
    /// linetype of new entities (`$CELTYPE`).
    pub fn set_current_linetype(&mut self, linetype: Handle) -> Result<(), EditError> {
        let o = self.get(linetype).ok_or(EditError::NotFound(linetype))?;
        if !(matches!(o.kind, ObjectKind::Linetype(_)) || o.type_name == "LTYPE") {
            return Err(EditError::NotFound(linetype));
        }
        self.header.celtype = linetype;
        Ok(())
    }

    /// Make `weight` the lineweight of new entities (`$CELWEIGHT`, kept in
    /// the low bits of the header's `FLAGS` as DWG stores it).
    pub fn set_current_lineweight(&mut self, weight: Lineweight) -> Result<(), EditError> {
        let flags = self
            .header
            .raw
            .iter()
            .find(|(k, _)| k == "FLAGS")
            .and_then(|(_, v)| v.parse::<i64>().ok())
            .unwrap_or(0x2A1D);
        let flags = (flags & !0x1F) | (weight.code() as i64 & 0x1F);
        self.set_header_text("FLAGS", &flags.to_string());
        Ok(())
    }

    /// Add an object owned by another: an insert's attribute, an old-style
    /// polyline's vertex, or the SEQEND that closes their list. Entity
    /// properties `common` (if any) are kept, the space set to owned.
    pub fn add_owned(
        &mut self,
        owner: Handle,
        type_name: &str,
        type_code: u16,
        kind: ObjectKind,
        common: Option<EntityCommon>,
    ) -> Handle {
        let h = self.new_handle();
        let mut o = Object::record(h, type_name, type_code, owner, kind);
        o.entity = common.map(|c| EntityCommon {
            space: Space::Owned,
            ..c
        });
        self.insert_object(o);
        h
    }

    /// Add an empty block definition: its record in the block table, with
    /// the BLOCK and ENDBLK entities that open and close it, on layer `0`.
    pub fn add_block_record(&mut self, name: &str, base: Vec3) -> Result<Handle, EditError> {
        if name.starts_with('*') {
            return Err(EditError::BadName("block names starting with * are kept for the drawing's own blocks".into()));
        }
        self.new_block_record(name, base)
    }

    /// [`Drawing::add_block_record`] for any name, `*Paper_Space2` and the
    /// like included: the drawing's own blocks (layouts).
    pub fn new_block_record(&mut self, name: &str, base: Vec3) -> Result<Handle, EditError> {
        check_name(name.strip_prefix('*').unwrap_or(name))?;
        if self.block_by_name(name).is_some() {
            return Err(EditError::NameInUse(name.to_string()));
        }
        self.ensure_model_space();
        let control = self
            .control(self.header.block_control, "BLOCK_CONTROL")
            .ok_or(EditError::MissingTable("block"))?;
        let layer0 = self.ensure_layer_zero();
        let h = self.new_handle();
        let begin = self.new_handle();
        let end = self.new_handle();
        self.insert_object(Object::record(
            h,
            "BLOCK_HEADER",
            0x31,
            control,
            ObjectKind::BlockHeader(BlockHeader {
                name: name.into(),
                base_point: base,
                insert_units: self.header.insunits.unwrap_or(0),
                block_entity: begin,
                endblk: end,
                ..Default::default()
            }),
        ));
        for (eh, type_name, code, kind) in [
            (begin, "BLOCK", 0x04, ObjectKind::BlockBegin { name: name.into() }),
            (end, "ENDBLK", 0x05, ObjectKind::BlockEnd),
        ] {
            let mut o = Object::record(eh, type_name, code, h, kind);
            o.entity = Some(EntityCommon {
                layer: layer0,
                ..Default::default()
            });
            self.insert_object(o);
        }
        if let Some(entries) = self.control_entries(control) {
            entries.push(h);
        }
        Ok(h)
    }

    /// Add a non-graphical object (a dictionary, a layout) owned by `owner`.
    pub fn add_object(&mut self, type_name: &str, type_code: u16, owner: Handle, kind: ObjectKind) -> Handle {
        let h = self.new_handle();
        self.insert_object(Object::record(h, type_name, type_code, owner, kind));
        h
    }

    /// Add a linetype record of dashes (positive), gaps (negative) and
    /// dots (zero) to the linetype table.
    pub fn add_linetype(&mut self, name: &str, description: &str, pattern: &[f64]) -> Result<Handle, EditError> {
        check_name(name)?;
        if self.linetype_by_name(name).is_some() {
            return Err(EditError::NameInUse(name.to_string()));
        }
        if pattern.iter().any(|v| !v.is_finite()) {
            return Err(EditError::BadValue("a linetype's lengths must be numbers".into()));
        }
        let control = self
            .control(self.header.linetype_control, "LTYPE_CONTROL")
            .ok_or(EditError::MissingTable("linetype"))?;
        let h = self.new_handle();
        self.insert_object(Object::record(
            h,
            "LTYPE",
            0x39,
            control,
            ObjectKind::Linetype(Linetype::simple(name, description, pattern)),
        ));
        if let Some(entries) = self.control_entries(control) {
            entries.push(h);
        }
        Ok(h)
    }

    /// Insert an object and add it to the lookup index.
    fn insert_object(&mut self, o: Object) {
        let key = match o.entity.as_ref().map(|e| e.space) {
            Some(Space::Model) => self.index.model_space,
            Some(Space::Paper) => self.index.paper_space,
            _ => o.owner,
        };
        let h = o.handle;
        self.objects.insert(h, o);
        if !key.is_null() {
            let list = self.index.owned.entry(key).or_default();
            // New handles are the largest, so this keeps handle order.
            match list.last() {
                Some(last) if *last > h => {
                    let at = list.partition_point(|x| *x < h);
                    list.insert(at, h);
                }
                _ => list.push(h),
            }
        }
    }
}

fn variant_name(kind: &ObjectKind) -> String {
    let s = format!("{kind:?}");
    s.split(['(', ' ', '{']).next().unwrap_or("").to_string()
}

impl Object {
    /// A new object with no reactors, extended data or provenance.
    pub fn record(
        handle: Handle,
        type_name: &str,
        type_code: u16,
        owner: Handle,
        kind: ObjectKind,
    ) -> Object {
        Object {
            handle,
            type_code,
            type_name: type_name.into(),
            owner,
            reactors: Vec::new(),
            xdictionary: Handle::NULL,
            entity: None,
            kind,
            offset: 0,
            size: 0,
            note: None,
            raw: None,
            xdata: Vec::new(),
            unknown_tags: Vec::new(),
        }
    }
}

/// What an edit changed, to take it back (see [`Drawing::begin_change`]).
///
/// Holds the objects the edit touched as they were, and the header and
/// class list when the edit changed them: memory in proportion to the
/// edit, not to the drawing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Change {
    pub objects: Journal,
    /// The header before, when the edit changed it. Its `raw` variables
    /// are kept only when the edit changed them (`raw_kept`): adding an
    /// entity moves the handle seed, and copying every variable each time
    /// would cost more than the entity.
    header: Option<Box<HeaderVars>>,
    raw_kept: bool,
    classes: Option<Vec<ClassDef>>,
}

/// The parts of the drawing outside its objects, as an edit found them.
pub struct ChangeStart {
    header: Box<HeaderVars>,
    classes: Vec<ClassDef>,
}

impl Change {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.header.is_none() && self.classes.is_none()
    }

    /// Add a later change, so the two undo as one step.
    pub fn merge(&mut self, later: Change) {
        self.objects.merge(later.objects);
        match (&mut self.header, later.header) {
            (None, h) => {
                self.header = h;
                self.raw_kept = later.raw_kept;
            }
            // The earlier change left `raw` as it found it, so the later
            // one's record of it is the record from before both.
            (Some(mine), Some(theirs)) if !self.raw_kept && later.raw_kept => {
                mine.raw = theirs.raw;
                self.raw_kept = true;
            }
            _ => {}
        }
        if self.classes.is_none() {
            self.classes = later.classes;
        }
    }

    /// Bytes held, roughly.
    pub fn approx_bytes(&self) -> usize {
        self.objects.approx_bytes()
            + self.header.as_ref().map_or(0, |h| {
                std::mem::size_of::<HeaderVars>() + h.raw.iter().map(|(k, v)| 48 + k.len() + v.len()).sum::<usize>()
            })
            + self.classes.as_ref().map_or(0, |c| c.len() * 128)
    }

    /// Handles of the objects changed.
    pub fn handles(&self) -> impl Iterator<Item = Handle> + '_ {
        self.objects.touched.keys().copied()
    }

    /// Whether the header changed.
    pub fn changes_header(&self) -> bool {
        self.header.is_some()
    }

    /// Whether the header kept for undo includes its stored variables
    /// (`HeaderVars::raw`): only when the edit changed them.
    pub fn keeps_raw_header(&self) -> bool {
        self.raw_kept
    }

    /// The header as it was before the change, when the change made one.
    pub fn header_before(&self) -> Option<&HeaderVars> {
        self.header.as_deref()
    }
}

impl Drawing {
    /// Start recording an edit. Make the edit through the drawing's own
    /// methods or `objects`, then call [`Drawing::end_change`].
    pub fn begin_change(&mut self) -> ChangeStart {
        self.objects.start_journal();
        ChangeStart {
            header: Box::new(self.header.clone()),
            classes: self.classes.clone(),
        }
    }

    /// What changed since [`Drawing::begin_change`].
    pub fn end_change(&mut self, start: ChangeStart) -> Change {
        let mut header = (self.header != *start.header).then_some(start.header);
        let raw_kept = header.as_ref().is_some_and(|h| h.raw != self.header.raw);
        if let Some(h) = &mut header {
            if !raw_kept {
                h.raw = Vec::new();
            }
        }
        Change {
            objects: self.objects.take_journal(),
            header,
            raw_kept,
            classes: (self.classes != start.classes).then_some(start.classes),
        }
    }

    /// Take a change back; the change that makes it again. The lookup
    /// index is updated for the objects changed, not rebuilt.
    pub fn revert(&mut self, c: Change) -> Change {
        let key = |d: &Drawing, o: &Object| -> Handle {
            match o.entity.as_ref().map(|e| e.space) {
                Some(Space::Model) => d.index.model_space,
                Some(Space::Paper) => d.index.paper_space,
                _ => o.owner,
            }
        };
        let blocks_change = |d: &Drawing, c: &Journal| {
            c.touched.iter().any(|(h, t)| {
                let now = matches!(d.objects.get(h).map(|o| &o.kind), Some(ObjectKind::BlockHeader(_)));
                let was = matches!(&t.prior, Some(Some(o)) if matches!(o.kind, ObjectKind::BlockHeader(_)));
                // A block record made or removed can change which one is
                // model or paper space.
                now != was || (t.prior.is_some() && !d.objects.contains_key(h))
            })
        };
        let rebuild = blocks_change(self, &c.objects)
            || c.header.as_ref().is_some_and(|h| {
                h.model_space != self.header.model_space || h.paper_space != self.header.paper_space
            });
        let before: Vec<(Handle, Handle)> = c
            .objects
            .touched
            .keys()
            .map(|h| (*h, self.objects.get(h).map_or(Handle::NULL, |o| key(self, o))))
            .collect();
        let objects = self.objects.revert(c.objects);
        let header = c.header.map(|mut h| {
            if !c.raw_kept {
                h.raw = std::mem::take(&mut self.header.raw);
            }
            let mut now = std::mem::replace(&mut self.header, *h);
            if !c.raw_kept {
                now.raw = Vec::new();
            }
            Box::new(now)
        });
        let classes = c.classes.map(|cl| std::mem::replace(&mut self.classes, cl));
        if rebuild {
            self.rebuild_index();
        } else {
            for (h, was) in before {
                let now = self.objects.get(&h).map_or(Handle::NULL, |o| key(self, o));
                let listed_now = !now.is_null();
                if was == now && listed_now {
                    // Still listed under the same owner, unless it was
                    // removed and put back in one change.
                    let list = self.index.owned.entry(now).or_default();
                    if let Err(at) = list.binary_search(&h) {
                        list.insert(at, h);
                    }
                    continue;
                }
                if !was.is_null() {
                    if let Some(list) = self.index.owned.get_mut(&was) {
                        if let Ok(at) = list.binary_search(&h) {
                            list.remove(at);
                        }
                        if list.is_empty() {
                            self.index.owned.remove(&was);
                        }
                    }
                }
                if listed_now {
                    let list = self.index.owned.entry(now).or_default();
                    if let Err(at) = list.binary_search(&h) {
                        list.insert(at, h);
                    }
                }
            }
        }
        Change {
            objects,
            header,
            raw_kept: c.raw_kept,
            classes,
        }
    }
}

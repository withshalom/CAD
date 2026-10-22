//! Open drawings and the edits made to them, for apps that keep the core
//! in their own process (the browser workbench today; the desktop apps
//! can use it the same way).
//!
//! Everything is safe Rust over `cad-core`; [`crate::web`] only moves
//! bytes in and out. An app talks to a drawing through [`Session::exec`]:
//! one line of text per command, the same words a user types on the
//! command line, answered with JSON. Keeping the interface to bytes and
//! text means no bindings to generate and nothing to keep in step but
//! this file.
//!
//! Commands (numbers in drawing units; angles in degrees; handles in hex):
//!
//! | Command | Does |
//! |---|---|
//! | `line x1 y1 x2 y2` | adds a line |
//! | `circle cx cy r` | adds a circle |
//! | `arc cx cy r start end` | adds an arc, counter-clockwise |
//! | `arc-3p x1 y1 x2 y2 x3 y3` | adds the arc from the first point through the second to the third |
//! | `pline closed x y bulge …` | adds a polyline (closed 0 or 1) |
//! | `rect x1 y1 x2 y2` | adds a closed rectangular polyline |
//! | `text x y height rotation words…` | adds single-line text |
//! | `erase h …` | erases entities |
//! | `copy dx dy h …` | copies entities |
//! | `rotate cx cy degrees h …` | rotates entities about a point |
//! | `mirror x1 y1 x2 y2 keep h …` | mirrors entities across a line, keeping the originals when keep is 1 |
//! | `dim-linear x1 y1 x2 y2 lx ly [degrees]` | linear dimension, its line through (lx, ly) along the angle; without one, horizontal or vertical by where the line goes |
//! | `dim-aligned x1 y1 x2 y2 offset` · `dim-aligned x1 y1 x2 y2 lx ly` | aligned dimension, its line at the offset or through (lx, ly) |
//! | `dim-radius cx cy r degrees` · `dim-diameter cx cy r degrees` | radius and diameter dimensions |
//! | `dim-radius h lx ly` · `dim-diameter h lx ly` | the same of circle or arc h, the leader towards (lx, ly) |
//! | `move dx dy h …` | moves entities |
//! | `layer-current h` | makes a layer current |
//! | `layer-on h 0/1`, `layer-freeze h 0/1`, `layer-lock h 0/1`, `layer-plot h 0/1` | layer state |
//! | `layer-color h aci` | layer colour |
//! | `layer-add name` | adds a layer with default properties |
//! | `get h` | an entity's type, layer and geometry |
//! | `set h vN x y` | moves vertex N of a polyline |
//! | `set h property value` | changes one value: `cx`, `cy`, `radius`, `diameter`, `circumference`, `area` (circles); `start`, `end` angles (arcs); `x1` … `y2`, `length` (lines); `x`, `y`, `height`, `rotation`, `value` (text); `layer` for any entity |
//! | `undo`, `redo` | step through the history |
//! | `begin`, `end` | the edits between them undo as one step (a command that draws several segments is one operation, COMMANDS.md §1) |
//! | `cancel` | ends the group and takes back its edits |
//!
//! Edits answer `{"ok":true,"handles":["2A",…]}`; `get` answers the
//! entity. Errors come back as `Err` with a message for the user.

use cad_core::export::display::{display_list, display_patch, json_escape, json_num};
use cad_core::geom::{Vec2, Vec3};
use cad_core::model::*;
use cad_core::render;
use cad_core::{dwg, dxf, native, new_drawing, Template};
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;

/// Steps of undo kept per drawing, at most.
const UNDO_STEPS: usize = 1000;

/// Bytes of undo and redo kept per drawing, roughly; the oldest steps go
/// first. The latest step is kept whatever its size.
const HISTORY_BYTES: usize = 64 << 20;

/// Formats [`Session::save`] writes.
pub const SAVE_FORMATS: [&str; 6] = ["blk", "dwg", "dxf", "dxfb", "svg", "pdf"];

struct Doc {
    drawing: Drawing,
    /// What each edit changed, oldest first, with its size in bytes.
    undo: VecDeque<(Change, usize)>,
    redo: Vec<(Change, usize)>,
    /// Bytes `undo` and `redo` hold, roughly.
    history: usize,
    /// Edits since the last save.
    dirty: bool,
    /// What the drawing was opened from: "dwg", "dxf", "blk" or "new".
    source: &'static str,
    /// Model space rendered, kept between queries until the next edit.
    scene: Option<render::Scene>,
    /// The edits of the running command (`begin` … `end`), merged.
    group: Option<Change>,
    /// What changed since the app last read the display.
    redraw: Redraw,
    /// The sheet being worked on: model space (null) or a layout's block.
    /// Display, picking and new objects are there.
    space: Handle,
}

/// What an app's copy of the display needs.
#[derive(Default)]
enum Redraw {
    #[default]
    Nothing,
    /// The shapes of these entities.
    Some(std::collections::BTreeSet<Handle>),
    All,
}

impl Doc {
    /// The block of the sheet being worked on: the layout chosen, while
    /// it exists, otherwise model space.
    fn space(&self) -> Handle {
        let ms = self.drawing.model_space();
        if !self.space.is_null() && self.space != ms && self.drawing.block_header(self.space).is_some() {
            self.space
        } else {
            ms
        }
    }

    fn in_model(&self) -> bool {
        self.space() == self.drawing.model_space()
    }

    fn scene(&mut self) -> &render::Scene {
        let (model, space) = (self.in_model(), self.space());
        let d = &self.drawing;
        self.scene.get_or_insert_with(|| if model { render::model_space(d) } else { render::layout(d, space) })
    }

    /// The drawing changed by `c` (made or taken back; `c` holds what
    /// it touched as it was). The scene is patched where only entities
    /// changed, and made again on the next query otherwise.
    fn changed(&mut self, c: &Change) {
        self.dirty = true;
        if !self.in_model() {
            // A layout draws model space through its viewports: drawn
            // again whole.
            self.scene = None;
            self.redraw = Redraw::All;
            return;
        }
        let Doc {
            drawing,
            scene,
            redraw,
            ..
        } = self;
        let touched = render::entities_changed(drawing, c);
        match (scene.as_mut(), touched) {
            (Some(sc), Some(hs)) => {
                let before = |h: Handle| match c.objects.touched.get(&h) {
                    Some(Touch { prior: Some(p), .. }) => p.as_ref(),
                    _ => drawing.get(h),
                };
                render::update_model_space(sc, drawing, &hs, before);
                match redraw {
                    Redraw::All => {}
                    Redraw::Some(set) => set.extend(hs),
                    Redraw::Nothing => *redraw = Redraw::Some(hs.into_iter().collect()),
                }
            }
            _ => {
                *scene = None;
                *redraw = Redraw::All;
            }
        }
    }

    fn push_undo(&mut self, c: Change) {
        let n = c.approx_bytes();
        self.history += n;
        self.undo.push_back((c, n));
        self.trim();
    }

    /// Keep the history inside its limits, oldest first.
    fn trim(&mut self) {
        while self.undo.len() > UNDO_STEPS || (self.history > HISTORY_BYTES && self.undo.len() + self.redo.len() > 1) {
            if let Some((_, n)) = self.undo.pop_front() {
                self.history -= n;
            } else if !self.redo.is_empty() {
                // The step farthest from now.
                let (_, n) = self.redo.remove(0);
                self.history -= n;
            } else {
                break;
            }
        }
    }

    fn clear_redo(&mut self) {
        for (_, n) in self.redo.drain(..) {
            self.history -= n;
        }
    }

    /// Let the history go, to free memory.
    fn drop_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.history = 0;
    }

    /// End the running command: its edits become one step of undo.
    fn end_group(&mut self) {
        if let Some(g) = self.group.take() {
            if !g.is_empty() {
                self.clear_redo();
                self.push_undo(g);
            }
        }
    }
}

/// The drawings an app has open.
#[derive(Default)]
pub struct Session {
    /// Bytes in use, where the platform counts them (the browser build),
    /// and the most the session lets itself use before it lets history go
    /// and refuses to open or change drawings.
    heap: Option<(fn() -> usize, usize)>,
    docs: BTreeMap<u32, Doc>,
    next: u32,
}

fn unit_of(d: &Drawing) -> u16 {
    d.header.insunits.unwrap_or(0)
}

fn handle_hex(h: Handle) -> String {
    format!("{:X}", h.0)
}

pub(crate) fn parse_handle(s: &str) -> Result<Handle, String> {
    u64::from_str_radix(s.trim_start_matches("0x"), 16)
        .map(Handle)
        .map_err(|_| format!("{s:?} is not a handle"))
}

pub(crate) fn parse_num(s: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("{s:?} is not a number"))
}

pub(crate) fn nums(args: &[&str], n: usize) -> Result<Vec<f64>, String> {
    if args.len() != n {
        return Err(format!("expected {n} numbers, got {}", args.len()));
    }
    args.iter().map(|a| parse_num(a)).collect()
}

pub(crate) fn flag(s: &str) -> Result<bool, String> {
    match s {
        "1" | "on" | "true" | "yes" => Ok(true),
        "0" | "off" | "false" | "no" => Ok(false),
        _ => Err(format!("{s:?} is not 0 or 1")),
    }
}

const Z: Vec3 = Vec3 {
    x: 0.0,
    y: 0.0,
    z: 1.0,
};

fn ok_handles(hs: &[Handle]) -> String {
    let list: Vec<String> = hs.iter().map(|h| format!("\"{}\"", handle_hex(*h))).collect();
    format!("{{\"ok\":true,\"handles\":[{}]}}", list.join(","))
}

impl Session {
    pub fn new() -> Session {
        Session::default()
    }

    /// A session that watches memory: above `limit` bytes in use (as
    /// `in_use` counts them), it lets undo history go, and if that is not
    /// enough, refuses to open or change drawings until some are closed.
    pub fn with_heap_limit(in_use: fn() -> usize, limit: usize) -> Session {
        Session {
            heap: Some((in_use, limit)),
            ..Session::default()
        }
    }

    /// Room to open or change a drawing (see [`Session::with_heap_limit`]).
    fn room(&mut self) -> Result<(), String> {
        let Some((in_use, limit)) = self.heap else {
            return Ok(());
        };
        if in_use() <= limit {
            return Ok(());
        }
        for d in self.docs.values_mut() {
            d.drop_history();
            if in_use() <= limit {
                return Ok(());
            }
        }
        Err(format!(
            "memory is nearly full ({} MB in use, {} MB allowed): save your work and close drawings you are not using",
            in_use() >> 20,
            limit >> 20
        ))
    }

    fn add(&mut self, drawing: Drawing, source: &'static str) -> u32 {
        self.next += 1;
        self.docs.insert(
            self.next,
            Doc {
                drawing,
                undo: VecDeque::new(),
                redo: Vec::new(),
                history: 0,
                dirty: false,
                source,
                scene: None,
                group: None,
                redraw: Redraw::All,
                space: Handle::NULL,
            },
        );
        self.next
    }

    fn doc(&self, id: u32) -> Result<&Doc, String> {
        self.docs.get(&id).ok_or_else(|| format!("no drawing {id} is open"))
    }

    fn doc_mut(&mut self, id: u32) -> Result<&mut Doc, String> {
        self.docs.get_mut(&id).ok_or_else(|| format!("no drawing {id} is open"))
    }

    /// Open a drawing (native `.blk`, DXF or DWG, told apart by content).
    pub fn open(&mut self, bytes: &[u8]) -> Result<u32, String> {
        self.room()?;
        let source = if native::looks_native(bytes) {
            "blk"
        } else if dxf::looks_like_dxf(bytes) {
            "dxf"
        } else {
            "dwg"
        };
        let d = cad_core::open_bytes(bytes).map_err(|e| e.to_string())?;
        Ok(self.add(d, source))
    }

    /// Start a drawing from the metric (millimetre) or imperial (inch)
    /// template.
    pub fn new_drawing(&mut self, imperial: bool) -> Result<u32, String> {
        self.room()?;
        let t = if imperial {
            Template::Imperial
        } else {
            Template::Metric
        };
        Ok(self.add(new_drawing(t), "new"))
    }

    /// The sample plate from the workspace mockups (`cad sample`).
    pub fn sample(&mut self) -> Result<u32, String> {
        self.room()?;
        Ok(self.add(cad_core::samples::plate(), "new"))
    }

    pub fn close(&mut self, id: u32) {
        self.docs.remove(&id);
    }

    pub fn drawing(&self, id: u32) -> Option<&Drawing> {
        self.docs.get(&id).map(|d| &d.drawing)
    }

    /// Model space as a display list ([`cad_core::export::display`]).
    pub fn display(&mut self, id: u32) -> Result<Vec<u8>, String> {
        let doc = self.doc_mut(id)?;
        doc.redraw = Redraw::Nothing;
        Ok(display_list(doc.scene()))
    }

    /// What changed in the display since the app last read it: a patch
    /// ([`cad_core::export::display::display_patch`]) when only some
    /// entities changed, otherwise the whole display list.
    pub fn display_changes(&mut self, id: u32) -> Result<Vec<u8>, String> {
        let doc = self.doc_mut(id)?;
        let r = std::mem::take(&mut doc.redraw);
        let scene = doc.scene();
        Ok(match r {
            Redraw::All => display_list(scene),
            Redraw::Some(set) => display_patch(scene, &set.into_iter().collect::<Vec<_>>()),
            Redraw::Nothing => display_patch(scene, &[]),
        })
    }

    /// What the app shows about the drawing, as JSON: units, layers,
    /// history, and the plain-text summary the Android viewer shows.
    pub fn info(&mut self, id: u32) -> Result<String, String> {
        self.doc_mut(id)?.scene();
        let doc = self.doc(id)?;
        let d = &doc.drawing;
        let scene = doc.scene.as_ref().expect("rendered above");
        let mut j = String::from("{");
        let u = unit_of(d);
        let _ = write!(
            j,
            "\"source\":\"{}\",\"version\":\"{}\",\"insunits\":{u},\"units\":\"{}\",\"dirty\":{},\"undo\":{},\"redo\":{},",
            doc.source,
            json_escape(&d.version.to_string()),
            units_name(u),
            doc.dirty,
            doc.undo.len(),
            doc.redo.len()
        );
        let current = d.current_layer();
        let census = crate::census(d);
        j.push_str("\"layers\":[");
        for (i, (h, l)) in census.layers.iter().enumerate() {
            if i > 0 {
                j.push(',');
            }
            let (r, g, b) = cad_core::standards::screen_color(l.color);
            let aci = match l.color {
                Color::Index(i) => i,
                _ => 0,
            };
            let lt = d
                .get(l.linetype)
                .and_then(|o| match &o.kind {
                    ObjectKind::Linetype(t) => Some(t.name.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let _ = write!(
                j,
                "{{\"handle\":\"{}\",\"name\":\"{}\",\"color\":\"#{r:02x}{g:02x}{b:02x}\",\"aci\":{aci},\"linetype\":\"{}\",\"weight\":{},\"on\":{},\"frozen\":{},\"locked\":{},\"plot\":{},\"current\":{},\"count\":{}}}",
                handle_hex(*h),
                json_escape(&l.name),
                json_escape(&lt),
                json_num(Lineweight::from_code(l.lineweight).effective_mm()),
                !l.off,
                l.frozen,
                l.locked,
                l.plot,
                current == Some(*h),
                census.by_layer.get(h).copied().unwrap_or(0)
            );
        }
        let _ = write!(j, "],\"display\":\"{}\",", json_escape(&display_format(d)));
        // The sheet being worked on, the formats the Units dialog shows,
        // and what new objects get.
        let space = doc.space();
        let sheet = cad_core::layouts::sheets(d).into_iter().find(|s| s.block == space);
        let _ = write!(
            j,
            "\"space\":\"{}\",\"spaceHandle\":\"{}\",",
            json_escape(sheet.as_ref().map(|s| s.name.as_str()).unwrap_or("Model")),
            handle_hex(space)
        );
        match sheet.and_then(|s| s.paper) {
            Some((w, h)) => {
                let _ = write!(j, "\"paper\":[{},{}],", json_num(w), json_num(h));
            }
            None => j.push_str("\"paper\":null,"),
        }
        let (color, linetype, weight) = d.current_properties();
        let color = match color {
            Color::ByLayer => "ByLayer".to_string(),
            Color::ByBlock => "ByBlock".to_string(),
            Color::Index(i) => i.to_string(),
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            _ => "ByLayer".to_string(),
        };
        let linetype = match linetype {
            LinetypeRef::Handle(h) => d.record_name(h).unwrap_or("ByLayer").to_string(),
            LinetypeRef::ByBlock => "ByBlock".into(),
            LinetypeRef::Continuous => cad_core::standards::CONTINUOUS.into(),
            LinetypeRef::ByLayer => "ByLayer".into(),
        };
        let weight = match Lineweight::from_code(weight) {
            Lineweight::ByLayer => "ByLayer".to_string(),
            Lineweight::ByBlock => "ByBlock".to_string(),
            Lineweight::Default => "Default".to_string(),
            w => format!("{:.2}", w.effective_mm()),
        };
        let h = &d.header;
        let _ = write!(
            j,
            "\"lunits\":{},\"luprec\":{},\"aunits\":{},\"auprec\":{},\"ltscale\":{},\"textHeight\":{},\"currentColor\":\"{}\",\"currentLinetype\":\"{}\",\"currentLineweight\":\"{}\",",
            h.lunits.unwrap_or(2),
            h.luprec.unwrap_or(4),
            h.aunits.unwrap_or(0),
            h.auprec.unwrap_or(0),
            json_num(h.ltscale.unwrap_or(1.0)),
            json_num(cad_core::standards::text_height(cad_core::units::Unit::of(d))),
            json_escape(&color),
            json_escape(&linetype),
            json_escape(&weight)
        );
        let _ = write!(
            j,
            "\"summary\":\"{}\"}}",
            json_escape(&crate::summary_of(d, scene, doc.source, &census))
        );
        Ok(j)
    }

    /// The drawing in a file format: `cad`, `dwg` (AutoCAD 2018), `dxf`
    /// (ASCII), `dxfb` (binary DXF) or `svg`. `unix_seconds` and `user`
    /// stamp the save into the drawing, as a CAD program does. Returns
    /// the bytes and notes on anything the format could not hold.
    pub fn save(
        &mut self,
        id: u32,
        format: &str,
        unix_seconds: i64,
        user: &str,
    ) -> Result<(Vec<u8>, Vec<String>), String> {
        cad_core::set_time(unix_seconds);
        let doc = self.doc_mut(id)?;
        if format != "svg" && format != "pdf" {
            doc.drawing.stamp_save(user, unix_seconds);
        }
        let doc = &*doc;
        let d = &doc.drawing;
        let out = match format {
            "blk" => (native::to_bytes(d, &native::WriteOptions::default()), Vec::new()),
            "dwg" => {
                let o = dwg::write::to_bytes(d).map_err(|e| e.to_string())?;
                (o.bytes, o.notes)
            }
            "dxf" | "dxfb" => {
                let f = if format == "dxf" {
                    dxf::DxfFormat::Ascii
                } else {
                    dxf::DxfFormat::Binary
                };
                let o = dxf::to_bytes(d, f);
                (o.bytes, o.notes)
            }
            "svg" => {
                let svg = cad_core::export::svg::to_svg(
                    &render::model_space(d),
                    &cad_core::export::svg::SvgOptions::default(),
                );
                (svg.into_bytes(), Vec::new())
            }
            "pdf" => {
                use cad_core::export::pdf::{to_pdf, PdfOptions, Placement};
                use cad_core::layouts;
                let space = doc.space();
                let model = space == d.model_space();
                let unit = cad_core::units::Unit::of(d);
                let scene = render::plotted(d, &if model { render::model_space(d) } else { render::layout(d, space) });
                let opt = PdfOptions {
                    paper_mm: layouts::paper_mm(d, space),
                    monochrome: layouts::monochrome(d, space),
                    lineweights: true,
                    title: d.header.properties.title.clone(),
                };
                let place = if model {
                    Placement::Fit { window: None, margin_mm: 10.0 }
                } else {
                    Placement::Scale {
                        mm_per_unit: layouts::paper_unit_mm(unit),
                        origin: Vec2::ZERO,
                    }
                };
                (to_pdf(&scene, &place, &opt), Vec::new())
            }
            _ => return Err(format!("unknown format {format:?}; one of {}", SAVE_FORMATS.join(", "))),
        };
        if format == "blk" || format == "dwg" || format == "dxf" || format == "dxfb" {
            self.doc_mut(id)?.dirty = false;
        }
        Ok(out)
    }

    /// Run one command line (see the module documentation).
    pub fn exec(&mut self, id: u32, line: &str) -> Result<String, String> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some((&cmd, args)) = words.split_first() else {
            return Err("empty command".into());
        };
        let cmd = cmd.to_ascii_lowercase();
        if !matches!(
            cmd.as_str(),
            "begin" | "end" | "cancel" | "undo" | "u" | "redo" | "get" | "snap" | "pick" | "box" | "len" | "angle" | "fmt"
                | "space" | "layouts" | "blocks" | "linetypes" | "patterns" | "attribs" | "bounds" | "measure" | "grips"
                | "check-holes"
        ) {
            self.room()?;
        }
        let doc = self.doc_mut(id)?;
        match cmd.as_str() {
            "begin" => {
                if doc.group.is_none() {
                    doc.group = Some(Change::default());
                }
                return Ok("{\"ok\":true,\"handles\":[]}".into());
            }
            "end" => {
                doc.end_group();
                return Ok("{\"ok\":true,\"handles\":[]}".into());
            }
            "cancel" => {
                if let Some(g) = doc.group.take() {
                    if !g.is_empty() {
                        let back = doc.drawing.revert(g);
                        doc.changed(&back);
                    }
                }
                return Ok("{\"ok\":true,\"handles\":[]}".into());
            }
            "undo" | "u" => {
                doc.end_group();
                let (c, n) = doc.undo.pop_back().ok_or("nothing to undo")?;
                doc.history -= n;
                let again = doc.drawing.revert(c);
                doc.changed(&again);
                let n = again.approx_bytes();
                doc.history += n;
                doc.redo.push((again, n));
                doc.trim();
                return Ok("{\"ok\":true,\"handles\":[]}".into());
            }
            "redo" => {
                let (c, n) = doc.redo.pop().ok_or("nothing to redo")?;
                doc.history -= n;
                let back = doc.drawing.revert(c);
                doc.changed(&back);
                let n = back.approx_bytes();
                doc.history += n;
                doc.undo.push_back((back, n));
                doc.trim();
                return Ok("{\"ok\":true,\"handles\":[]}".into());
            }
            "get" => {
                if args.len() != 1 {
                    return Err("get takes one handle".into());
                }
                return describe(&doc.drawing, parse_handle(args[0])?);
            }
            "snap" | "pick" | "box" | "len" | "angle" | "fmt" | "check-holes" => return query(doc, &cmd, args),
            "space" => {
                let name = args.join(" ");
                let sheet = cad_core::layouts::sheet(&doc.drawing, &name).ok_or_else(|| format!("there is no layout {name:?}"))?;
                doc.space = sheet.block;
                doc.scene = None;
                doc.redraw = Redraw::All;
                return Ok(format!("{{\"ok\":true,\"handles\":[\"{}\"]}}", handle_hex(sheet.block)));
            }
            _ => {
                let space = doc.space();
                if let Some(r) = crate::commands::query(&doc.drawing, space, &cmd, args) {
                    return r;
                }
            }
        }
        // An edit, recorded as it is made; taken back if it fails part way.
        let space = doc.space();
        let start = doc.drawing.begin_change();
        let r = edit(&mut doc.drawing, space, &cmd, args);
        let c = doc.drawing.end_change(start);
        let handles = match r {
            Ok(h) => h,
            Err(e) => {
                if !c.is_empty() {
                    doc.drawing.revert(c);
                }
                return Err(e);
            }
        };
        if !c.is_empty() {
            doc.changed(&c);
            doc.clear_redo();
            match &mut doc.group {
                Some(g) => g.merge(c),
                None => doc.push_undo(c),
            }
        }
        Ok(ok_handles(&handles))
    }
}

/// Queries that change nothing: snaps, picking, selection boxes, and
/// reading and showing lengths in the drawing's units.
///
/// | Query | Answer |
/// |---|---|
/// | `snap x y radius kinds` | `{"snaps":[{"kind":1,"name":"Endpoint","x":…,"y":…,"handle":"2A"},…]}`, closest first ([`cad_core::snap::kind`] bits) |
/// | `pick x y radius` | `{"handle":"2A"}` or `{"handle":null}` |
/// | `box x1 y1 x2 y2 crossing` | `{"handles":[…]}` |
/// | `len text` | `{"value":12.7}`: a typed length (`1/2"`, `10mm`, `1'-2"`) in drawing units |
/// | `angle text` | `{"value":0.785…}`: a typed angle, in radians |
/// | `fmt v …` | `{"text":["12.7000",…]}`: lengths in the drawing's display format |
/// | `check-holes dia web bend` | `{"holes":5,"edges":…,"bends":…,"small":[…],"close":[…],"edge":[…],"near":[…]}`: holes on the sheet shown against the smallest diameter, web and hole-to-bend distance, in millimetres ([`cad_core::check`]) |
fn query(doc: &mut Doc, cmd: &str, args: &[&str]) -> Result<String, String> {
    use cad_core::snap;
    use cad_core::units::{format_length, parse_angle, parse_length, LengthFormat, Unit};
    match cmd {
        "snap" => {
            let v = nums(args, 4)?;
            let found = snap::snaps(doc.scene(), Vec2::new(v[0], v[1]), v[2], v[3] as u32);
            let list: Vec<String> = found
                .iter()
                .take(8)
                .map(|f| {
                    format!(
                        "{{\"kind\":{},\"name\":\"{}\",\"x\":{},\"y\":{},\"handle\":\"{}\"}}",
                        f.kind,
                        snap::kind::name(f.kind),
                        json_num(f.point.x),
                        json_num(f.point.y),
                        handle_hex(f.source)
                    )
                })
                .collect();
            Ok(format!("{{\"snaps\":[{}]}}", list.join(",")))
        }
        "pick" => {
            let v = nums(args, 3)?;
            Ok(match snap::pick(doc.scene(), Vec2::new(v[0], v[1]), v[2]) {
                Some(h) => format!("{{\"handle\":\"{}\"}}", handle_hex(h)),
                None => "{\"handle\":null}".into(),
            })
        }
        "box" => {
            let v = nums(args, 5)?;
            let hs = snap::in_box(doc.scene(), Vec2::new(v[0], v[1]), Vec2::new(v[2], v[3]), v[4] != 0.0);
            let list: Vec<String> = hs.iter().map(|h| format!("\"{}\"", handle_hex(*h))).collect();
            Ok(format!("{{\"handles\":[{}]}}", list.join(",")))
        }
        "len" => {
            let v = parse_length(&args.join(" "), Unit::of(&doc.drawing)).map_err(|e| e.to_string())?;
            Ok(format!("{{\"value\":{}}}", json_num(v)))
        }
        "angle" => {
            let v = parse_angle(&args.join(" ")).map_err(|e| e.to_string())?;
            Ok(format!("{{\"value\":{}}}", json_num(v)))
        }
        "check-holes" => {
            use cad_core::check::{check_holes, Limits};
            let v = nums(args, 3)?;
            if v.iter().any(|x| x.is_nan() || *x < 0.0) {
                return Err("the limits cannot be negative".into());
            }
            let limits = Limits { min_diameter: v[0], min_web: v[1], hole_to_bend: v[2] };
            doc.scene();
            let r = check_holes(&doc.drawing, doc.scene.as_ref().expect("rendered above"), &limits);
            let list = |set: &std::collections::BTreeSet<Handle>| set.iter().map(|h| format!("\"{}\"", handle_hex(*h))).collect::<Vec<_>>().join(",");
            Ok(format!(
                "{{\"holes\":{},\"edges\":{},\"bends\":{},\"small\":[{}],\"close\":[{}],\"edge\":[{}],\"near\":[{}],\"flagged\":[{}]}}",
                r.holes.len(),
                r.edges,
                r.bends,
                list(&r.too_small),
                list(&r.too_close),
                list(&r.near_edge),
                list(&r.near_bend),
                list(&r.flagged())
            ))
        }
        _ => {
            let v = nums(args, args.len())?;
            let unit = Unit::of(&doc.drawing);
            let f = LengthFormat::of(&doc.drawing);
            let list: Vec<String> = v
                .iter()
                .map(|x| format!("\"{}\"", json_escape(&format_length(*x, unit, &f))))
                .collect();
            Ok(format!("{{\"text\":[{}]}}", list.join(",")))
        }
    }
}

/// How the drawing shows lengths, as the Drawing panel names it:
/// "Decimal, 0.000", "Fractional, 1/64", "Architectural, 1/16".
fn display_format(d: &Drawing) -> String {
    use cad_core::units::{LengthFormat, LinearFormat as F};
    let f = LengthFormat::of(d);
    let p = f.precision.min(8);
    match f.format {
        F::Decimal => format!("Decimal, {}", if p == 0 { "0".into() } else { format!("0.{}", "0".repeat(p as usize)) }),
        F::Scientific => format!("Scientific, {p} places"),
        F::Engineering => format!("Engineering, {p} places"),
        F::Architectural => format!("Architectural, 1/{}", 1u32 << p),
        F::Fractional => format!("Fractional, 1/{}", 1u32 << p),
    }
}

fn current_text_style(d: &Drawing) -> Handle {
    if d.get(d.header.textstyle).is_some() {
        return d.header.textstyle;
    }
    d.objects
        .values()
        .find(|o| matches!(o.kind, ObjectKind::TextStyle(_)))
        .map(|o| o.handle)
        .unwrap_or(Handle::NULL)
}

fn layer_arg(d: &Drawing, s: &str) -> Result<Handle, String> {
    let h = parse_handle(s)?;
    d.layer(h).map(|_| h).ok_or_else(|| format!("{s} is not a layer"))
}

fn check_unlocked(d: &Drawing, h: Handle) -> Result<(), String> {
    let o = d.get(h).ok_or_else(|| format!("no object {}", handle_hex(h)))?;
    let layer = o.entity.as_ref().map(|e| e.layer).unwrap_or(Handle::NULL);
    if let Some(l) = d.layer(layer) {
        if l.locked {
            return Err(format!("layer {} is locked", l.name));
        }
    }
    Ok(())
}

fn edit(d: &mut Drawing, space: Handle, cmd: &str, args: &[&str]) -> Result<Vec<Handle>, String> {
    let add = |d: &mut Drawing, kind: ObjectKind| -> Result<Vec<Handle>, String> {
        d.add_entity_in(space, Handle::NULL, kind)
            .map(|h| vec![h])
            .map_err(|e| e.to_string())
    };
    match cmd {
        "line" => {
            let v = nums(args, 4)?;
            if v[0] == v[2] && v[1] == v[3] {
                return Err("the line has no length".into());
            }
            add(
                d,
                ObjectKind::Line(Line {
                    start: Vec3::new(v[0], v[1], 0.0),
                    end: Vec3::new(v[2], v[3], 0.0),
                    extrusion: Z,
                    ..Default::default()
                }),
            )
        }
        "circle" => {
            let v = nums(args, 3)?;
            if v[2] <= 0.0 {
                return Err("the radius must be more than zero".into());
            }
            add(
                d,
                ObjectKind::Circle(Circle {
                    center: Vec3::new(v[0], v[1], 0.0),
                    radius: v[2],
                    extrusion: Z,
                    ..Default::default()
                }),
            )
        }
        "arc" => {
            let v = nums(args, 5)?;
            if v[2] <= 0.0 {
                return Err("the radius must be more than zero".into());
            }
            add(
                d,
                ObjectKind::Arc(Arc {
                    center: Vec3::new(v[0], v[1], 0.0),
                    radius: v[2],
                    extrusion: Z,
                    start_angle: v[3].to_radians(),
                    end_angle: v[4].to_radians(),
                    ..Default::default()
                }),
            )
        }
        "pline" | "rect" => {
            let (closed, pts, bulges) = if cmd == "rect" {
                let v = nums(args, 4)?;
                if v[0] == v[2] || v[1] == v[3] {
                    return Err("the rectangle has no area".into());
                }
                let p = vec![
                    Vec2::new(v[0], v[1]),
                    Vec2::new(v[2], v[1]),
                    Vec2::new(v[2], v[3]),
                    Vec2::new(v[0], v[3]),
                ];
                (true, p, Vec::new())
            } else {
                let (first, rest) = args.split_first().ok_or("pline needs points")?;
                let closed = flag(first)?;
                if rest.len() % 3 != 0 || rest.len() < 6 {
                    return Err("pline takes closed, then x y bulge for two or more points".into());
                }
                let v = nums(rest, rest.len())?;
                let pts: Vec<Vec2> = v.chunks(3).map(|c| Vec2::new(c[0], c[1])).collect();
                let bulges: Vec<f64> = v.chunks(3).map(|c| c[2]).collect();
                let bulges = if bulges.iter().all(|b| *b == 0.0) {
                    Vec::new()
                } else {
                    bulges
                };
                (closed, pts, bulges)
            };
            add(
                d,
                ObjectKind::LwPolyline(LwPolyline {
                    flags: if closed { LwPolyline::FLAG_CLOSED } else { 0 },
                    extrusion: Z,
                    points: pts,
                    bulges,
                    ..Default::default()
                }),
            )
        }
        "text" => {
            if args.len() < 5 {
                return Err("text takes x y height rotation and the words".into());
            }
            let v = nums(&args[..4], 4)?;
            if v[2] <= 0.0 {
                return Err("the height must be more than zero".into());
            }
            let style = current_text_style(d);
            add(
                d,
                ObjectKind::Text(Text {
                    insertion: Vec2::new(v[0], v[1]),
                    extrusion: Z,
                    rotation: v[3].to_radians(),
                    height: v[2],
                    width_factor: 1.0,
                    value: args[4..].join(" "),
                    style,
                    ..Default::default()
                }),
            )
        }
        "erase" => {
            if args.is_empty() {
                return Err("erase takes handles".into());
            }
            let hs: Vec<Handle> = args.iter().map(|a| parse_handle(a)).collect::<Result<_, _>>()?;
            for h in &hs {
                check_unlocked(d, *h)?;
                d.erase_entity(*h).map_err(|e| e.to_string())?;
            }
            Ok(hs)
        }
        "layer-current" | "layer-on" | "layer-freeze" | "layer-lock" | "layer-plot" | "layer-color" => {
            let (h, rest) = args.split_first().ok_or("which layer?")?;
            let h = layer_arg(d, h)?;
            let err = |e: EditError| e.to_string();
            match cmd {
                "layer-current" => d.set_current_layer(h).map_err(err)?,
                "layer-color" => {
                    let aci: u8 = rest
                        .first()
                        .and_then(|s| s.parse().ok())
                        .filter(|v| *v >= 1)
                        .ok_or("colour index 1 to 255")?;
                    d.set_layer_color(h, Color::Index(aci)).map_err(err)?
                }
                _ => {
                    let on = flag(rest.first().ok_or("0 or 1?")?)?;
                    let mut s = d.layer_state(h).ok_or("not a layer")?;
                    match cmd {
                        "layer-on" => s.on = on,
                        "layer-freeze" => s.frozen = on,
                        "layer-plot" => s.plot = on,
                        _ => s.locked = on,
                    }
                    d.set_layer_state(h, s).map_err(err)?
                }
            }
            Ok(vec![h])
        }
        "dim-linear" | "dim-aligned" | "dim-radius" | "dim-diameter" => {
            use cad_core::construct;
            let nothing = || "nothing to measure: the points are the same".to_string();
            let dim = match (cmd, args.len()) {
                // Horizontal or vertical by where the line goes.
                ("dim-linear", 6) => {
                    let v = nums(args, 6)?;
                    construct::linear_dimension_at(Vec2::new(v[0], v[1]), Vec2::new(v[2], v[3]), Vec2::new(v[4], v[5])).ok_or_else(nothing)?
                }
                ("dim-linear", _) => {
                    let v = nums(args, 7)?;
                    Dimension::linear(Vec2::new(v[0], v[1]), Vec2::new(v[2], v[3]), Vec2::new(v[4], v[5]), v[6].to_radians())
                }
                // The line through a point.
                ("dim-aligned", 6) => {
                    let v = nums(args, 6)?;
                    construct::aligned_dimension_at(Vec2::new(v[0], v[1]), Vec2::new(v[2], v[3]), Vec2::new(v[4], v[5])).ok_or_else(nothing)?
                }
                ("dim-aligned", _) => {
                    let v = nums(args, 5)?;
                    Dimension::aligned(Vec2::new(v[0], v[1]), Vec2::new(v[2], v[3]), v[4])
                }
                // A picked circle or arc, its leader towards a point.
                (_, 3) => {
                    let h = parse_handle(args[0])?;
                    let at = nums(&args[1..], 2)?;
                    construct::radial_dimension(d, h, Vec2::new(at[0], at[1]), Some(cmd == "dim-diameter"))?
                }
                ("dim-radius", _) => {
                    let v = nums(args, 4)?;
                    Dimension::radius(Vec2::new(v[0], v[1]), v[2], v[3].to_radians())
                }
                _ => {
                    let v = nums(args, 4)?;
                    Dimension::diameter(Vec2::new(v[0], v[1]), v[2], v[3].to_radians())
                }
            };
            if dim.measurement <= 0.0 {
                return Err("nothing to measure: the points are the same".into());
            }
            let dimstyle = cad_core::ops::current_dimstyle(d);
            // Dimensions go on the DIMENSION layer when the drawing has one
            // (DESIGN.md §3), otherwise on the current layer.
            let layer = cad_core::ops::dimension_layer(d);
            let h = d
                .add_entity_in(space, layer, ObjectKind::Dimension(Dimension { dimstyle, ..dim }))
                .map_err(|e| e.to_string())?;
            Ok(vec![h])
        }
        "set" => {
            if args.len() < 3 {
                return Err("set takes a handle, a property and a value".into());
            }
            let h = parse_handle(args[0])?;
            check_unlocked(d, h)?;
            let field = args[1].to_ascii_lowercase();
            if field == "layer" {
                let l = layer_arg(d, args[2])?;
                let o = d.objects.get_mut(&h).ok_or("no such object")?;
                o.entity.as_mut().ok_or("not an entity")?.layer = l;
                return Ok(vec![h]);
            }
            let raw = args[2..].join(" ");
            if let Some(r) = crate::commands::set(d, h, &field, &raw) {
                r?;
                return Ok(vec![h]);
            }
            let o = d.objects.get_mut(&h).ok_or_else(|| format!("no object {}", handle_hex(h)))?;
            set_field(&mut o.kind, &field, &raw)?;
            Ok(vec![h])
        }
        "layer-add" => {
            let name = args.join(" ");
            let continuous = d
                .linetype_by_name(cad_core::standards::CONTINUOUS)
                .unwrap_or(Handle::NULL);
            let h = d
                .add_layer(&name, Color::Index(7), continuous, Lineweight::Default)
                .map_err(|e| e.to_string())?;
            Ok(vec![h])
        }
        _ => crate::commands::edit(d, space, cmd, args).unwrap_or_else(|| Err(format!("unknown command {cmd:?}"))),
    }
}

/// Change one value of an entity. Lengths are numbers in drawing units
/// (the app reads typed lengths with `len` first); angles are degrees.
fn set_field(kind: &mut ObjectKind, field: &str, raw: &str) -> Result<(), String> {
    let v = || parse_num(raw.trim());
    let positive = || {
        let x = v()?;
        if x > 0.0 {
            Ok(x)
        } else {
            Err("the value must be more than zero".to_string())
        }
    };
    let unknown = || Err(format!("{field:?} is not a property that can be set here"));
    match kind {
        ObjectKind::Circle(c) => match field {
            "cx" => c.center.x = v()?,
            "cy" => c.center.y = v()?,
            "radius" => c.radius = positive()?,
            "diameter" => c.radius = positive()? / 2.0,
            "circumference" => c.radius = positive()? / std::f64::consts::TAU,
            "area" => c.radius = (positive()? / std::f64::consts::PI).sqrt(),
            _ => return unknown(),
        },
        ObjectKind::Arc(a) => match field {
            "cx" => a.center.x = v()?,
            "cy" => a.center.y = v()?,
            "radius" => a.radius = positive()?,
            "start" => a.start_angle = v()?.to_radians(),
            "end" => a.end_angle = v()?.to_radians(),
            _ => return unknown(),
        },
        ObjectKind::Line(l) => {
            let len = || ((l.end.x - l.start.x).powi(2) + (l.end.y - l.start.y).powi(2)).sqrt();
            match field {
                "x1" => l.start.x = v()?,
                "y1" => l.start.y = v()?,
                "x2" => l.end.x = v()?,
                "y2" => l.end.y = v()?,
                "length" => {
                    // Keep the start and the direction.
                    let n = positive()?;
                    let old = len();
                    if old == 0.0 {
                        return Err("the line has no direction".into());
                    }
                    let k = n / old;
                    l.end.x = l.start.x + (l.end.x - l.start.x) * k;
                    l.end.y = l.start.y + (l.end.y - l.start.y) * k;
                }
                _ => return unknown(),
            }
        }
        ObjectKind::LwPolyline(p) if field.starts_with('v') => {
            // vN x y: move vertex N.
            let i: usize = field[1..].parse().map_err(|_| format!("{field:?} is not a vertex"))?;
            let xy: Vec<&str> = raw.split_whitespace().collect();
            let v = nums(&xy, 2)?;
            let q = p.points.get_mut(i).ok_or_else(|| format!("the polyline has no vertex {i}"))?;
            *q = Vec2::new(v[0], v[1]);
        }
        ObjectKind::Text(t) => match field {
            "x" => t.insertion.x = v()?,
            "y" => t.insertion.y = v()?,
            "height" => t.height = positive()?,
            "rotation" => t.rotation = v()?.to_radians(),
            "value" => {
                if raw.is_empty() {
                    return Err("text cannot be empty".into());
                }
                t.value = raw.to_string()
            }
            _ => return unknown(),
        },
        _ => return Err("this kind of object has no editable values here yet".into()),
    }
    Ok(())
}

fn describe(d: &Drawing, h: Handle) -> Result<String, String> {
    let o = d.get(h).ok_or_else(|| format!("no object {}", handle_hex(h)))?;
    let e = o.entity.as_ref().ok_or("not an entity")?;
    let lay = d.layer(e.layer);
    let layer = lay.map(|l| l.name.clone()).unwrap_or_default();
    let (r, g, b) = cad_core::standards::screen_color(lay.map(|l| l.color).unwrap_or(Color::Index(7)));
    let lt_name = |h: Handle| {
        d.get(h)
            .and_then(|o| match &o.kind {
                ObjectKind::Linetype(t) => Some(t.name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| cad_core::standards::CONTINUOUS.to_string())
    };
    let layer_lt = lay.map(|l| lt_name(l.linetype)).unwrap_or_default();
    let linetype = match &e.linetype {
        LinetypeRef::ByLayer => format!("ByLayer ({layer_lt})"),
        LinetypeRef::ByBlock => "ByBlock".to_string(),
        LinetypeRef::Continuous => cad_core::standards::CONTINUOUS.to_string(),
        LinetypeRef::Handle(t) => lt_name(*t),
    };
    let layer_w = lay.map(|l| Lineweight::from_code(l.lineweight)).unwrap_or(Lineweight::Default);
    let w = e.weight();
    let lineweight = match w {
        Lineweight::ByLayer => format!("ByLayer ({:.2} mm)", layer_w.effective_mm()),
        other => format!("{other}"),
    };
    let color = match e.color {
        Color::ByLayer => "ByLayer".to_string(),
        Color::ByBlock => "ByBlock".to_string(),
        Color::Index(i) => format!("Index {i}"),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        ref other => format!("{other:?}"),
    };
    let mut j = format!(
        "{{\"handle\":\"{}\",\"type\":\"{}\",\"layer\":\"{}\",\"layerHandle\":\"{}\",\"layerColor\":\"#{r:02x}{g:02x}{b:02x}\",\"color\":\"{}\",\"linetype\":\"{}\",\"lineweight\":\"{}\"",
        handle_hex(h),
        json_escape(&o.type_name),
        json_escape(&layer),
        handle_hex(e.layer),
        json_escape(&color),
        json_escape(&linetype),
        json_escape(&lineweight)
    );
    let n = json_num;
    match &o.kind {
        ObjectKind::Line(l) => {
            let m = cad_core::construct::measure(l.start.xy(), l.end.xy());
            let _ = write!(
                j,
                ",\"start\":[{},{}],\"end\":[{},{}],\"length\":{},\"angle\":{}",
                n(l.start.x),
                n(l.start.y),
                n(l.end.x),
                n(l.end.y),
                n(m.distance),
                n(m.angle.to_degrees())
            );
        }
        ObjectKind::Circle(c) => {
            use std::f64::consts::{PI, TAU};
            let _ = write!(
                j,
                ",\"center\":[{},{}],\"radius\":{},\"diameter\":{},\"circumference\":{},\"area\":{}",
                n(c.center.x),
                n(c.center.y),
                n(c.radius),
                n(2.0 * c.radius),
                n(TAU * c.radius),
                n(PI * c.radius * c.radius)
            );
        }
        ObjectKind::Arc(a) => {
            let sweep = cad_core::geom::curve::normalize_sweep(a.end_angle - a.start_angle);
            let _ = write!(
                j,
                ",\"center\":[{},{}],\"radius\":{},\"start\":{},\"end\":{},\"sweep\":{},\"arcLength\":{}",
                n(a.center.x),
                n(a.center.y),
                n(a.radius),
                n(a.start_angle.to_degrees()),
                n(a.end_angle.to_degrees()),
                n(sweep.to_degrees()),
                n(sweep * a.radius)
            );
        }
        ObjectKind::LwPolyline(p) => {
            let pts: Vec<String> = p.points.iter().map(|q| format!("[{},{}]", n(q.x), n(q.y))).collect();
            let _ = write!(
                j,
                ",\"vertices\":{},\"closed\":{},\"points\":[{}]",
                p.points.len(),
                p.is_closed(),
                pts.join(",")
            );
        }
        ObjectKind::Text(t) => {
            let _ = write!(
                j,
                ",\"insertion\":[{},{}],\"height\":{},\"value\":\"{}\"",
                n(t.insertion.x),
                n(t.insertion.y),
                n(t.height),
                json_escape(&t.value)
            );
        }
        _ => {}
    }
    crate::commands::describe(d, o, &mut j);
    j.push('}');
    Ok(j)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handles(json: &str) -> Vec<String> {
        let at = json.find("\"handles\":[").unwrap() + 11;
        let end = json[at..].find(']').unwrap() + at;
        json[at..end]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_matches('"').to_string())
            .collect()
    }

    #[test]
    fn draw_undo_redo_and_save() {
        let mut s = Session::new();
        let id = s.new_drawing(false).unwrap();
        let r = s.exec(id, "line 0 0 100 50").unwrap();
        let h = handles(&r);
        assert_eq!(h.len(), 1);
        s.exec(id, "circle 50 25 10").unwrap();
        s.exec(id, "rect 0 0 100 50").unwrap();
        s.exec(id, "arc 0 0 20 0 90").unwrap();
        s.exec(id, "pline 0 0 0 1 10 0 0 20 5 0").unwrap();
        s.exec(id, "text 5 5 3.5 0 PLATE 1").unwrap();
        let info = s.info(id).unwrap();
        assert!(info.contains("\"undo\":6"), "{info}");
        assert!(info.contains("\"name\":\"VISIBLE\""), "{info}");
        assert!(s.exec(id, &format!("get {}", h[0])).unwrap().contains("\"length\":111.80"));

        s.exec(id, "undo").unwrap();
        s.exec(id, "undo").unwrap();
        assert_eq!(render::model_space(s.drawing(id).unwrap()).items.len(), 4);
        s.exec(id, "redo").unwrap();
        assert_eq!(render::model_space(s.drawing(id).unwrap()).items.len(), 5);

        s.exec(id, &format!("move 10 10 {}", h[0])).unwrap();
        assert!(s.exec(id, &format!("get {}", h[0])).unwrap().contains("\"start\":[10,10]"));
        s.exec(id, &format!("erase {}", h[0])).unwrap();
        assert!(s.exec(id, &format!("get {}", h[0])).is_err());

        for f in SAVE_FORMATS {
            let (bytes, _) = s.save(id, f, 1_790_000_000, "tester").unwrap();
            assert!(!bytes.is_empty(), "{f}");
            if f != "svg" && f != "pdf" {
                let back = s.open(&bytes).unwrap();
                assert_eq!(
                    render::model_space(s.drawing(back).unwrap()).items.len(),
                    render::model_space(s.drawing(id).unwrap()).items.len(),
                    "{f}"
                );
            }
        }
        assert!(s.info(id).unwrap().contains("\"dirty\":false"));
        let disp = s.display(id).unwrap();
        assert_eq!(&disp[..4], b"DSPL");
    }

    #[test]
    fn queries_snap_pick_and_read_lengths() {
        let mut s = Session::new();
        let id = s.new_drawing(true).unwrap();
        s.exec(id, "line 0 0 10 0").unwrap();
        s.exec(id, "line 4 -5 4 3").unwrap();
        let r = s.exec(id, "snap 4.1 0.1 0.5 255").unwrap();
        assert!(r.starts_with("{\"snaps\":[{\"kind\":16,\"name\":\"Intersection\",\"x\":4,\"y\":0"), "{r}");
        assert!(s.exec(id, "pick 2 0.05 0.2").unwrap().contains("\"handle\":\""));
        assert_eq!(s.exec(id, "pick 2 3 0.2").unwrap(), "{\"handle\":null}");
        let b = s.exec(id, "box -1 -1 11 1 1").unwrap();
        assert_eq!(b.matches(',').count(), 1, "{b}");
        assert_eq!(s.exec(id, "len 1'-2 1/2\"").unwrap(), "{\"value\":14.5}");
        assert_eq!(s.exec(id, "len 25.4mm").unwrap(), "{\"value\":1}");
        assert!(s.exec(id, "fmt 1.5").unwrap().starts_with("{\"text\":[\"1.5"));
        // Queries are not edits.
        assert!(s.info(id).unwrap().contains("\"undo\":2"));
    }

    #[test]
    fn values_are_edited_in_place() {
        let mut s = Session::new();
        let id = s.new_drawing(false).unwrap();
        let c = handles(&s.exec(id, "circle 15 15 6.5").unwrap())[0].clone();
        s.exec(id, &format!("set {c} diameter 20")).unwrap();
        let g = s.exec(id, &format!("get {c}")).unwrap();
        assert!(g.contains("\"radius\":10"), "{g}");
        assert!(g.contains("\"linetype\":\"ByLayer (Continuous)\""), "{g}");
        assert!(g.contains("\"lineweight\":\"ByLayer (0.50 mm)\""), "{g}");
        let l = handles(&s.exec(id, "line 0 0 3 4").unwrap())[0].clone();
        s.exec(id, &format!("set {l} length 10")).unwrap();
        assert!(s.exec(id, &format!("get {l}")).unwrap().contains("\"end\":[6,8]"));
        assert!(s.exec(id, &format!("set {c} radius -1")).is_err());
        s.exec(id, &format!("set {c} colour 3")).unwrap();
        assert!(s.exec(id, &format!("get {c}")).unwrap().contains("\"color\":\"Index 3\""));
        assert!(s.exec(id, &format!("set {c} colour 300")).is_err());
        assert!(s.exec(id, &format!("set {c} weight 3")).is_err());
        let t = handles(&s.exec(id, "text 0 0 3.5 0 HELLO").unwrap())[0].clone();
        s.exec(id, &format!("set {t} value P-1042 PLATE")).unwrap();
        assert!(s.exec(id, &format!("get {t}")).unwrap().contains("P-1042 PLATE"));
        s.exec(id, "undo").unwrap();
        assert!(s.exec(id, &format!("get {t}")).unwrap().contains("HELLO"));
    }

    #[test]
    fn copy_rotate_mirror_and_dimensions() {
        let mut s = Session::new();
        let id = s.new_drawing(false).unwrap();
        let l = handles(&s.exec(id, "line 0 0 10 0").unwrap())[0].clone();
        let c = handles(&s.exec(id, &format!("copy 0 5 {l}")).unwrap())[0].clone();
        assert_ne!(c, l);
        assert!(s.exec(id, &format!("get {c}")).unwrap().contains("\"start\":[0,5],\"end\":[10,5]"));
        s.exec(id, &format!("rotate 0 0 90 {l}")).unwrap();
        let g = s.exec(id, &format!("get {l}")).unwrap();
        assert!(g.contains(",10],\"length\":10"), "{g}");
        let a = handles(&s.exec(id, "arc 0 0 5 0 90").unwrap())[0].clone();
        let m = handles(&s.exec(id, &format!("mirror 0 0 0 1 1 {a}")).unwrap())[0].clone();
        let g = s.exec(id, &format!("get {m}")).unwrap();
        assert!(g.contains("\"start\":90") && g.contains("\"end\":180"), "{g}");
        let dl = handles(&s.exec(id, "dim-linear 0 0 200 0 0 20 0").unwrap())[0].clone();
        let g = s.exec(id, &format!("get {dl}")).unwrap();
        assert!(g.contains("\"layer\":\"DIMENSION\""), "{g}");
        s.exec(id, "dim-diameter 50 50 6.5 45").unwrap();
        assert!(s.exec(id, "dim-linear 1 1 1 1 0 5 0").is_err());
        // Everything still saves as DWG and reads back.
        let (bytes, _) = s.save(id, "dwg", 1_790_000_000, "").unwrap();
        let back = s.open(&bytes).unwrap();
        assert_eq!(
            render::model_space(s.drawing(back).unwrap()).items.len(),
            render::model_space(s.drawing(id).unwrap()).items.len()
        );
    }

    #[test]
    fn a_group_undoes_as_one_step() {
        let mut s = Session::new();
        let id = s.new_drawing(false).unwrap();
        s.exec(id, "circle 0 0 5").unwrap();
        s.exec(id, "begin").unwrap();
        for i in 0..3 {
            s.exec(id, &format!("line {i} 0 {} 0", i + 1)).unwrap();
        }
        s.exec(id, "end").unwrap();
        assert!(s.info(id).unwrap().contains("\"undo\":2"));
        s.exec(id, "undo").unwrap();
        assert_eq!(render::model_space(s.drawing(id).unwrap()).items.len(), 1);

        s.exec(id, "begin").unwrap();
        s.exec(id, "line 0 0 9 9").unwrap();
        s.exec(id, "line 9 9 0 9").unwrap();
        s.exec(id, "cancel").unwrap();
        assert_eq!(render::model_space(s.drawing(id).unwrap()).items.len(), 1);
        assert!(s.info(id).unwrap().contains("\"undo\":1"));
    }

    /// Items by entity: (style, operations as bits), in order.
    type Items = BTreeMap<u64, Vec<(String, Vec<u64>)>>;

    /// A display list's items by entity, with the patch's handles when it
    /// is one.
    fn read_display(b: &[u8]) -> (Items, Option<Vec<u64>>) {
        let n = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        let head = std::str::from_utf8(&b[12..12 + n]).unwrap();
        let styles: Vec<String> = head
            .split("\"styles\":[")
            .nth(1)
            .unwrap()
            .split("],\"items\"")
            .next()
            .unwrap()
            .split("},{")
            .map(|t| t.trim_matches(|c| c == '{' || c == '}').to_string())
            .collect();
        let patch = head.split("\"patch\":[").nth(1).map(|t| {
            t.split(']')
                .next()
                .unwrap()
                .split(',')
                .filter(|x| !x.is_empty())
                .map(|x| u64::from_str_radix(x.trim_matches('"'), 16).unwrap())
                .collect()
        });
        let start = (12 + n).div_ceil(8) * 8;
        let ops: Vec<f64> = b[start..].chunks(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect();
        let mut items: Items = BTreeMap::new();
        let mut i = 0;
        let mut cur = 0;
        while i < ops.len() {
            if ops[i] == 1.0 {
                cur = ops[i + 1] as u64;
                let style = styles[ops[i + 2] as usize].clone();
                items.entry(cur).or_default().push((style, Vec::new()));
                i += 3;
            } else {
                let len = match ops[i] as u32 {
                    2 => 5,
                    3 => 6,
                    4 => 3 + 2 * ops[i + 1] as usize,
                    5 => {
                        let mut j = i + 2;
                        for _ in 0..ops[i + 1] as usize {
                            j += 1 + 2 * ops[j] as usize;
                        }
                        j - i
                    }
                    6 => 3,
                    7 => 7 + ops[i + 6] as usize,
                    c => panic!("operation {c}"),
                };
                let item = items.get_mut(&cur).unwrap().last_mut().unwrap();
                item.1.extend(ops[i..i + len].iter().map(|v| v.to_bits()));
                i += len;
            }
        }
        (items, patch)
    }

    #[test]
    fn the_scene_is_patched_not_redrawn() {
        let mut s = Session::new();
        let id = s.sample().unwrap();
        let mut client = read_display(&s.display(id).unwrap()).0;
        let check = |s: &mut Session, client: &mut Items, step: &str| {
            let doc = s.docs.get(&id).unwrap();
            if let Some(sc) = &doc.scene {
                assert_eq!(sc, &render::model_space(&doc.drawing), "scene after {step}");
            }
            let mut fresh = doc.drawing.clone();
            fresh.rebuild_index();
            assert_eq!(fresh.index, doc.drawing.index, "index after {step}");
            let (items, patch) = read_display(&s.display_changes(id).unwrap());
            match patch {
                Some(hs) => {
                    for h in hs {
                        client.remove(&h);
                    }
                    client.extend(items);
                }
                None => *client = items,
            }
            let (whole, _) = read_display(&s.display(id).unwrap());
            assert_eq!(client, &whole, "client after {step}");
        };
        let mut made = Vec::new();
        for (i, cmd) in [
            "line 0 0 30 40",
            "circle 100 50 8",
            "text 10 10 3 0 HELLO",
            "rect 0 0 5 5",
            "begin",
            "line 1 1 2 2",
            "line 2 2 3 1",
            "end",
            "undo",
            "redo",
            "begin",
            "circle 0 0 1",
            "cancel",
        ]
        .iter()
        .enumerate()
        {
            let r = s.exec(id, cmd).unwrap();
            made.extend(handles(&r));
            check(&mut s, &mut client, &format!("{i}: {cmd}"));
        }
        let (a, b) = (made[0].clone(), made[1].clone());
        for cmd in [
            format!("move 5 5 {a} {b}"),
            format!("set {a} x1 -10"),
            format!("erase {b}"),
            "undo".into(),
            format!("copy 0 20 {a}"),
            format!("rotate 0 0 30 {a}"),
            format!("mirror 0 0 0 1 1 {a}"),
            "undo".into(),
            "undo".into(),
            "undo".into(),
            "redo".into(),
            // A layer change reaches every entity on it: drawn again.
            format!("layer-on {} 0", s.info(id).unwrap().split("\"handle\":\"").nth(2).unwrap().split('"').next().unwrap()),
            "undo".into(),
        ] {
            s.exec(id, &cmd).unwrap();
            check(&mut s, &mut client, &cmd);
        }
    }

    #[test]
    fn a_dxf_of_entities_alone_opens_drawn_and_editable() {
        // Hand-written from the DXF reference: no header, tables or handles.
        let text = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n10.0\n21\n10.0\n31\n0.0\n\
                    0\nCIRCLE\n8\nHOLES\n10\n5.0\n20\n5.0\n30\n0.0\n40\n2.0\n0\nENDSEC\n0\nEOF\n";
        let mut s = Session::new();
        let id = s.open(text.as_bytes()).unwrap();
        let info = s.info(id).unwrap();
        assert!(info.contains("Model space: 2 entities drawn"), "{info}");
        assert!(info.contains("\"name\":\"HOLES\""), "{info}");
        let (items, _) = read_display(&s.display(id).unwrap());
        assert_eq!(items.len(), 2);
        s.exec(id, "line 0 10 10 0").unwrap();
        assert!(s.info(id).unwrap().contains("Model space: 3 entities drawn"));
    }

    #[test]
    fn units_read_the_same_after_every_save() {
        let mut s = Session::new();
        let id = s.sample().unwrap();
        for f in ["blk", "dwg", "dxf", "dxfb"] {
            let (bytes, _) = s.save(id, f, 0, "").unwrap();
            let back = s.open(&bytes).unwrap();
            let info = s.info(back).unwrap();
            assert!(info.contains("\"insunits\":4,\"units\":\"millimeters\""), "{f}: {info}");
            assert!(info.contains("Units: millimeters"), "{f}: summary {info}");
            s.close(back);
        }
    }

    #[test]
    fn bad_commands_change_nothing() {
        let mut s = Session::new();
        let id = s.new_drawing(true).unwrap();
        for bad in ["", "line 0 0 0 0", "circle 1 1 -2", "line a b c d", "fly 1 2", "erase 99999"] {
            assert!(s.exec(id, bad).is_err(), "{bad:?}");
        }
        assert!(s.info(id).unwrap().contains("\"undo\":0"));
        assert!(s.exec(id, "undo").is_err());
        assert!(s.exec(99, "line 0 0 1 1").is_err());
    }

    /// Every drafting tool the lessons use, run as an app runs it, then
    /// saved in every format and read back.
    #[test]
    fn the_drafting_tools_draw_edit_and_save() {
        let mut s = Session::new();
        let id = s.new_drawing(false).unwrap();
        let one = |s: &mut Session, c: &str| -> String {
            let r = s.exec(id, c).unwrap_or_else(|e| panic!("{c}: {e}"));
            handles(&r).first().cloned().unwrap_or_default()
        };
        // An outline with a hole, drawn and edited.
        let base = one(&mut s, "line 0 0 100 0");
        let right = one(&mut s, "line 100 0 100 60");
        let top = one(&mut s, "line 100 60 0 60");
        let left = one(&mut s, "line 0 60 0 0");
        let hole = one(&mut s, "circle 30 30 10");
        let off = one(&mut s, &format!("offset 30 50 40 {base}"));
        assert!(s.exec(id, &format!("get {off}")).unwrap().contains("\"start\":[0,30]"));
        let r = s.exec(id, &format!("trim 30 30 {off} {hole}")).unwrap();
        assert_eq!(handles(&r).len(), 2);
        let short = one(&mut s, "line 10 45 50 45");
        s.exec(id, &format!("extend 49 45 {short} {right}")).unwrap();
        assert!(s.exec(id, &format!("get {short}")).unwrap().contains("\"end\":[100,45]"));
        let r = s.exec(id, &format!("fillet 8 90 0 {base} 100 10 {right}")).unwrap();
        assert_eq!(handles(&r).len(), 3);
        let r = s.exec(id, &format!("chamfer 5 5 10 60 {top} 0 50 {left}")).unwrap();
        assert_eq!(handles(&r).len(), 3);
        let arr = s.exec(id, &format!("array-rect 2 3 15 20 {hole}")).unwrap();
        assert_eq!(handles(&arr).len(), 5);
        let polar = s.exec(id, &format!("array-polar 200 30 6 360 1 {hole}")).unwrap();
        assert_eq!(handles(&polar).len(), 5);
        s.exec(id, &format!("scale 30 30 0.5 {hole}")).unwrap();
        assert!(s.exec(id, &format!("get {hole}")).unwrap().contains("\"radius\":5"));
        s.exec(id, &format!("stretch 90 -10 120 70 10 0 {right}")).unwrap();
        let pl = one(&mut s, "rect 300 0 340 30");
        s.exec(id, &format!("fillet-pline 4 {pl}")).unwrap();
        assert!(s.exec(id, &format!("get {pl}")).unwrap().contains("\"vertices\":8"));
        // Construction lines, hatch, notes, leaders, dimensions.
        let x = one(&mut s, "xline 0 0 1 1");
        let ray = one(&mut s, "ray 0 0 -1 0");
        assert!(s.exec(id, &format!("get {x}")).unwrap().contains("\"angle\":45"));
        let rect = one(&mut s, "rect 400 0 500 50");
        let c2 = one(&mut s, "circle 450 25 10");
        let hatch = one(&mut s, "hatch ANSI31 1 0 0 420 10");
        assert!(s.exec(id, &format!("get {hatch}")).unwrap().contains("\"loops\":2"));
        let solid = one(&mut s, &format!("hatch-objects SOLID 1 0 0 {c2}"));
        s.exec(id, &format!("set {solid} pattern ANSI37")).unwrap();
        let mt = one(&mut s, "mtext 0 100 80 3.5 0 FIRST LINE\\PSECOND LINE");
        assert!(s.exec(id, &format!("get {mt}")).unwrap().contains("FIRST LINE\\\\PSECOND LINE"));
        let r = s.exec(id, "leader 3.5 30 30 50 80 | 4X %%C10 THRU").unwrap();
        assert_eq!(handles(&r).len(), 2);
        let dl = one(&mut s, "dim-linear 0 0 100 0 0 -15 0");
        let b1 = one(&mut s, &format!("dim-baseline {dl} 150 0"));
        assert!(s.exec(id, &format!("get {b1}")).unwrap().contains("\"measurement\":150"));
        let c1 = one(&mut s, &format!("dim-continue {dl} 130 0"));
        assert!(s.exec(id, &format!("get {c1}")).unwrap().contains("\"measurement\":30"));
        let ang = one(&mut s, "dim-angular 0 0 50 0 0 50 20 20");
        assert!(s.exec(id, &format!("get {ang}")).unwrap().contains("\"measurement\":90"));
        let l1 = one(&mut s, "line 600 0 700 0");
        let l2 = one(&mut s, "line 600 0 650 50");
        let ang2 = one(&mut s, &format!("dim-angular-lines {l1} {l2} 640 10"));
        assert!(s.exec(id, &format!("get {ang2}")).unwrap().contains("\"measurement\":45"));
        let arc = one(&mut s, "arc 800 0 20 0 120");
        one(&mut s, &format!("dim-angular-arc {arc} 800 30"));
        // A block with attributes, inserted with values, edited, exploded.
        let frame = one(&mut s, "rect 0 -100 180 -60");
        let t1 = one(&mut s, "attdef 5 -75 5 0 | TITLE | Drawing title | PLATE");
        let r = s.exec(id, &format!("block 0 -100 convert {frame} {t1} | TITLE BLOCK")).unwrap();
        assert_eq!(handles(&r).len(), 2);
        let ins = one(&mut s, "insert 300 -100 1 0 | TITLE BLOCK | TITLE=BRACKET");
        assert!(s.exec(id, &format!("get {ins}")).unwrap().contains("\"value\":\"BRACKET\""));
        s.exec(id, &format!("attedit {ins} | title | BASE PLATE")).unwrap();
        assert!(s.exec(id, "blocks").unwrap().contains("\"name\":\"TITLE BLOCK\""));
        assert!(s.exec(id, &format!("attribs {ins}")).unwrap().contains("BASE PLATE"));
        let copy = one(&mut s, &format!("copy 0 -60 {ins}"));
        s.exec(id, &format!("rotate 300 -160 90 {copy}")).unwrap();
        s.exec(id, &format!("mirror 300 -200 400 -200 1 {copy}")).unwrap();
        s.exec(id, &format!("explode {copy}")).unwrap();
        // Linetypes, lineweights, colours.
        s.exec(id, "linetype-load PHANTOM DASHED2").unwrap();
        assert!(s.exec(id, "linetypes").unwrap().contains("\"name\":\"PHANTOM\""));
        s.exec(id, "linetype-current DASHED2").unwrap();
        s.exec(id, "lineweight-current 0.35").unwrap();
        s.exec(id, "color-current 1").unwrap();
        let dashed = one(&mut s, "line 0 200 100 200");
        let g = s.exec(id, &format!("get {dashed}")).unwrap();
        assert!(g.contains("\"linetype\":\"DASHED2\"") && g.contains("\"color\":\"Index 1\"") && g.contains("0.35"), "{g}");
        s.exec(id, "linetype-current ByLayer").unwrap();
        s.exec(id, "lineweight-current ByLayer").unwrap();
        s.exec(id, "color-current ByLayer").unwrap();
        s.exec(id, &format!("set {dashed} linetype CENTER")).unwrap();
        s.exec(id, &format!("set {dashed} ltscale 2")).unwrap();
        s.exec(id, "ltscale 0.5").unwrap();
        let lay = s.info(id).unwrap().split("\"name\":\"CENTER\"").next().unwrap().rsplit("\"handle\":\"").next().unwrap().split('"').next().unwrap().to_string();
        s.exec(id, &format!("layer-linetype {lay} PHANTOM2")).unwrap();
        s.exec(id, &format!("layer-lineweight {lay} 0.18")).unwrap();
        // Layouts and viewports.
        s.exec(id, "layout-new A4 Details").unwrap();
        s.exec(id, "space Details").unwrap();
        let info = s.info(id).unwrap();
        assert!(info.contains("\"space\":\"Details\"") && info.contains("\"paper\":[297,210]"), "{info}");
        let vp = one(&mut s, "mview 10 10 287 200 fit");
        one(&mut s, "text 20 15 3.5 0 SHEET 2");
        let (items, _) = read_display(&s.display(id).unwrap());
        assert!(items.contains_key(&u64::from_str_radix(&vp, 16).unwrap()));
        s.exec(id, &format!("vp-scale {vp} 1:4")).unwrap();
        assert!(s.exec(id, &format!("get {vp}")).unwrap().contains("\"scale\":0.25"));
        s.exec(id, "page-setup A4 landscape 1").unwrap();
        let (pdf, _) = s.save(id, "pdf", 1_790_000_000, "").unwrap();
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(s.exec(id, "layouts").unwrap().contains("\"name\":\"Details\",\"block\""));
        s.exec(id, "space Model").unwrap();
        let (pdf, _) = s.save(id, "pdf", 1_790_000_000, "").unwrap();
        assert!(pdf.len() > 1000);
        // Undo walks every step back.
        let steps = s.info(id).unwrap().split("\"undo\":").nth(1).unwrap().split(',').next().unwrap().parse::<usize>().unwrap();
        assert!(steps > 50, "{steps}");
        // Every format keeps every object.
        let (items, _) = read_display(&s.display(id).unwrap());
        let count = render::model_space(s.drawing(id).unwrap()).items.len();
        for f in ["blk", "dwg", "dxf", "dxfb"] {
            let (bytes, notes) = s.save(id, f, 1_790_000_000, "tester").unwrap();
            assert!(notes.is_empty(), "{f}: {notes:?}");
            let back = s.open(&bytes).unwrap();
            let d = s.drawing(back).unwrap();
            let again = render::model_space(d);
            assert_eq!(again.items.len(), count, "{f}");
            assert!(again.stats.skipped.is_empty(), "{f}: {:?}", again.stats.skipped);
            let names: Vec<String> = cad_core::layouts::sheets(d).iter().map(|x| x.name.clone()).collect();
            assert_eq!(names, ["Model", "Layout1", "Details"], "{f}");
            s.exec(back, "space Details").unwrap();
            let (lay, _) = read_display(&s.display(back).unwrap());
            assert!(lay.len() >= 2, "{f}");
            s.close(back);
        }
        assert!(items.contains_key(&u64::from_str_radix(&ray, 16).unwrap()));
        for _ in 0..steps {
            s.exec(id, "undo").unwrap();
        }
        assert_eq!(render::model_space(s.drawing(id).unwrap()).items.len(), 0);
        assert_eq!(cad_core::layouts::sheets(s.drawing(id).unwrap()).len(), 2);
        let _ = (rect, hatch);
    }

    #[test]
    fn units_convert_the_drawing() {
        let mut s = Session::new();
        let id = s.new_drawing(false).unwrap();
        let l = handles(&s.exec(id, "line 0 0 254 0").unwrap())[0].clone();
        s.exec(id, "units 1 4 4 0 0 1").unwrap();
        let info = s.info(id).unwrap();
        assert!(info.contains("\"insunits\":1") && info.contains("\"lunits\":4"), "{info}");
        assert!(s.exec(id, &format!("get {l}")).unwrap().contains("\"length\":10"));
        s.exec(id, "undo").unwrap();
        assert!(s.exec(id, &format!("get {l}")).unwrap().contains("\"length\":254"));
        s.exec(id, "units 6 2 3 0 0 0").unwrap();
        assert!(s.exec(id, &format!("get {l}")).unwrap().contains("\"length\":254"));
        assert!(s.exec(id, "units 99 2 3 0 0 0").is_err());
    }

    #[test]
    fn layers_from_commands() {
        let mut s = Session::new();
        let id = s.new_drawing(false).unwrap();
        s.exec(id, "layer-add PLATES").unwrap();
        let d = s.drawing(id).unwrap();
        let (h, _) = d.layer_by_name("PLATES").unwrap();
        let hx = format!("{:X}", h.0);
        s.exec(id, &format!("layer-current {hx}")).unwrap();
        s.exec(id, "line 0 0 10 0").unwrap();
        s.exec(id, &format!("layer-color {hx} 3")).unwrap();
        s.exec(id, &format!("layer-lock {hx} 1")).unwrap();
        let line = s.drawing(id).unwrap().model_space_entities()[0].handle;
        assert!(s.exec(id, &format!("erase {:X}", line.0)).unwrap_err().contains("locked"));
        // The current layer cannot be frozen.
        assert!(s.exec(id, &format!("layer-freeze {hx} 1")).is_err());
        let info = s.info(id).unwrap();
        assert!(info.contains("\"name\":\"PLATES\",\"color\":\"#3ddc84\",\"aci\":3"), "{info}");
    }

    /// What the app used to work out itself is the core's: arcs and
    /// dimensions from picked points, text boxes, construction lines at an
    /// angle, measuring, grips, array spacing and the hole check.
    #[test]
    fn picked_points_become_geometry_in_the_core() {
        let mut s = Session::new();
        let id = s.new_drawing(false).unwrap();
        let num = |json: &str, key: &str| -> f64 {
            let at = json.find(&format!("\"{key}\":")).unwrap_or_else(|| panic!("{key} in {json}")) + key.len() + 3;
            let end = json[at..].find([',', '}', ']']).unwrap() + at;
            json[at..end].parse().unwrap()
        };
        // ARC by three points, picked clockwise.
        let a = handles(&s.exec(id, "arc-3p -10 0 0 10 10 0").unwrap())[0].clone();
        let g = s.exec(id, &format!("get {a}")).unwrap();
        assert_eq!((num(&g, "radius"), num(&g, "start"), num(&g, "end")), (10.0, 0.0, 180.0));
        assert!((num(&g, "arcLength") - 10.0 * std::f64::consts::PI).abs() < 1e-9);
        assert!(s.exec(id, "arc-3p 0 0 1 1 2 2").is_err());
        // DIMLINEAR chooses vertical when the line is placed to the side.
        let v = handles(&s.exec(id, "dim-linear 0 0 100 40 130 20").unwrap())[0].clone();
        assert!((num(&s.exec(id, &format!("get {v}")).unwrap(), "measurement") - 40.0).abs() < 1e-9);
        let h = handles(&s.exec(id, "dim-linear 0 0 100 40 50 -20").unwrap())[0].clone();
        assert_eq!(num(&s.exec(id, &format!("get {h}")).unwrap(), "measurement"), 100.0);
        let al = handles(&s.exec(id, "dim-aligned 0 0 30 40 0 50").unwrap())[0].clone();
        assert_eq!(num(&s.exec(id, &format!("get {al}")).unwrap(), "measurement"), 50.0);
        // Radius and diameter of a picked circle.
        let c = handles(&s.exec(id, "circle 200 0 12").unwrap())[0].clone();
        let g = s.exec(id, &format!("get {c}")).unwrap();
        assert_eq!((num(&g, "diameter"), num(&g, "area")), (24.0, std::f64::consts::PI * 144.0));
        let r = handles(&s.exec(id, &format!("dim-radius {c} 230 0")).unwrap())[0].clone();
        assert!(s.exec(id, &format!("get {r}")).unwrap().contains("\"dimension\":\"radius\",\"measurement\":12"));
        let dd = handles(&s.exec(id, &format!("dim-diameter {c} 200 40")).unwrap())[0].clone();
        assert!(s.exec(id, &format!("get {dd}")).unwrap().contains("\"measurement\":24"));
        assert!(s.exec(id, &format!("dim-radius {v} 0 0")).is_err());
        // MTEXT from two corners, XLINE at an angle.
        let m = handles(&s.exec(id, "mtext-box 80 -40 0 -20 3.5 0 FIRST\\PSECOND").unwrap())[0].clone();
        let g = s.exec(id, &format!("get {m}")).unwrap();
        assert!(g.contains("\"insertion\":[0,-20]") && g.contains("\"width\":80"), "{g}");
        let x = handles(&s.exec(id, "xline-angle 5 5 90").unwrap())[0].clone();
        assert!((num(&s.exec(id, &format!("get {x}")).unwrap(), "angle") - 90.0).abs() < 1e-9);
        // DIST.
        let d = s.exec(id, "measure 0 0 3 4").unwrap();
        assert_eq!(num(&d, "distance"), 5.0);
        assert!(d.contains("Distance = 5"), "{d}");
        // Grips: list them, drag one.
        let gr = s.exec(id, &format!("grips {c}")).unwrap();
        assert_eq!(gr.matches("\"kind\":\"radius\"").count(), 4, "{gr}");
        s.exec(id, &format!("grip {c} radius - 212 0 220 0")).unwrap();
        assert_eq!(num(&s.exec(id, &format!("get {c}")).unwrap(), "radius"), 20.0);
        s.exec(id, &format!("grip {c} center - 200 0 210 0")).unwrap();
        assert!(s.exec(id, &format!("get {c}")).unwrap().contains("\"center\":[210,0]"));
        assert!(s.exec(id, &format!("grip {c} vertex 2 0 0 1 1")).is_err());
        // SCALE Reference and ARRAY's spacing.
        s.exec(id, &format!("scale-ref 210 0 20 10 {c}")).unwrap();
        assert_eq!(num(&s.exec(id, &format!("get {c}")).unwrap(), "radius"), 10.0);
        let b = s.exec(id, &format!("bounds {c}")).unwrap();
        assert!(b.contains("\"box\":[200,-10,220,10]") && num(&b, "rowGap") == 30.0, "{b}");
        // A hole too close to the plate's edge.
        s.exec(id, "rect 0 100 100 150").unwrap();
        let hole = handles(&s.exec(id, "circle 4 140 3").unwrap())[0].clone();
        let chk = s.exec(id, "check-holes 2 2 6").unwrap();
        assert!(chk.contains(&format!("\"edge\":[\"{hole}\"]")), "{chk}");
        assert!(s.exec(id, "check-holes 2 -1 6").is_err());
        // A layout filled by one viewport.
        s.exec(id, "layout-new A3 Sheet 1").unwrap();
        s.exec(id, "space Sheet 1").unwrap();
        let vp = handles(&s.exec(id, "mview fit 1:2").unwrap())[0].clone();
        let g = s.exec(id, &format!("get {vp}")).unwrap();
        assert_eq!((num(&g, "width"), num(&g, "height"), num(&g, "scale")), (400.0, 277.0, 0.5));
        s.exec(id, "space Model").unwrap();
        assert!(s.exec(id, "mview fit").is_err());
        // A sheet ready to plot, its viewport on a layer that does not plot.
        let made = handles(&s.exec(id, "sheet A3 | - | PART NAME=PLATE | SCALE=").unwrap());
        assert_eq!(made.len(), 3);
        let t = s.exec(id, &format!("get {}", made[1])).unwrap();
        assert!(t.contains("\"tag\":\"PART_NAME\",\"value\":\"PLATE\""), "{t}");
        s.exec(id, "space Sheet 1").unwrap();
        let (pdf, _) = s.save(id, "pdf", 0, "").unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        s.exec(id, "space Model").unwrap();
        assert!(s.exec(id, "sheet A3 | Sheet 1").is_err());
        assert!(s.info(id).unwrap().contains("\"textHeight\":3.5"));
    }
}

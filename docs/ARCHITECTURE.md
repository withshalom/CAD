# Architecture

## Names and versions

Two things, named and numbered separately.

- **The software** has no final name yet and calls itself CAD (the code
  holds the name in one constant, `cad_core::PRODUCT_NAME`, and the README
  title; it is also what a written DWG names as the program that saved it). Its version
  is semver `MAJOR.MINOR.PATCH` and stays below 1.0.0 until release:
  the minor number goes up for any public API change, new feature set or new
  `.cad` format version; the patch number for fixes only. 1.0.0 waits for the
  format freeze, which waits for the real-DWG corpus pass
  (`FORMAT-ROADMAP.md` §4). It is the Cargo workspace version, shown by
  `cad version` and recorded in each file's `$CAD_WRITER`.
- **The file format** is `.cad` (written "the `.cad` format, version 2.0").
  Its two-part version says only which readers can open a file (`FORMAT.md`).
  One program build can read and write several format versions.

Crates, the CLI binary and the project folder keep their `cad-` names: they
name the format and its code, not the product.

## Goals

- **Own native format, fluent in DWG.** Drawings are saved in the native
  `.cad` format (see `FORMAT.md`): binary-DXF-style tagged records in a
  DWG-style sectioned container, plus a change journal and `.cdl` locks.
  DWG is read on open and, later, written on export. The in-memory
  database mirrors DWG's own object graph (handles, owners, reactors,
  extension dictionaries), so DWG maps onto it without loss, and objects
  the core cannot decode yet keep their original DWG bytes.
- **One core, every platform.** The core is a plain Rust library using only
  the standard library: no crates, no `unsafe`, no platform code. Each app
  (desktop, Android, iOS) is a thin shell around it. The one piece that
  needs `unsafe`, the bridge to the apps (`crates/cad-ffi`), is small and
  separate.
- **Open formats, open to others.** The drawing database and the DWG and
  DXF readers and writers are crates of their own (`cad-model`,
  `cad-dwg`, `cad-dxf`), licensed MIT OR Apache-2.0 so that other programs
  can read and write DWG with them; `cad-core` builds on them and keeps
  everything else. They take nothing from the core: where a file needs
  something only the program has (its name, a preview, its font's text
  widths, its hatch pattern library), the writer takes it as an option.
- **Never crash on a file.** Malformed input produces an error, or a single
  object marked as failed. The rest of the drawing still loads.

## Layers

```
            ┌─────────────────────────────────────────────────────────┐
 apps       │ cad-cli · web workbench · Android viewer · later: iOS   │
            └──────────────────────┬──────────────────────────────────┘
                                   │ cad_core API: open() · save() · new_drawing()
 ┌─────────────────────────────────▼──────────────────────────────────┐
 │ raster    scene → anti-aliased image on every core; PNG            │
 │ export    SVG, DXF (R12), display list for app canvases            │
 │ snap      object snaps, picking, selection boxes                   │
 │ takeoff   length, contours, area and weight of a part (Proposed)   │
 │ render    model space → plan-view shapes with resolved style;      │
 │           dash patterns (dash), dimension graphics (dim)           │
 │ font      own single-stroke font, text layout                      │
 │ standards default layers, linetypes, styles, colours, templates    │
 │ units     unit conversion, typed lengths and angles, display       │
 │ native    .cad read/write, journal (incremental save), .cdl locks  │
 │ codec     DEFLATE and zlib (RFC 1950/1951), SHA-256 (FIPS 180-4)    │
 └─────────────────────────────────┬──────────────────────────────────┘
          cad-core, above; the crates below are MIT OR Apache-2.0
 ┌─────────────────────────────────▼──────────────────────────────────┐
 │ cad-dxf   DXF read and write, ASCII and binary                     │
 │ cad-dwg   bits → container → object map → objects → header; writer │
 │ cad-model model: Drawing, objects by handle, layers, blocks,       │
 │           header, edits · geom: Vec2/Vec3, transforms, OCS,        │
 │           bulges, NURBS · text: code pages, MTEXT codes            │
 └────────────────────────────────────────────────────────────────────┘

 open(path) sniffs the first bytes: native magic → native reader, DXF
 text or sentinel → DXF reader, else DWG.
 save(drawing, path) always writes native .cad.
 new_drawing(Metric | Imperial) starts one from a template.
```

## DWG read pipeline (`crates/cad-dwg`)

1. **Version** (`version.rs`): the six-byte tag, AC1012–AC1032.
2. **Container**, split by version:
   - `r2000.rs`: R13–R15. A table of section locators points at the header,
     classes and object map. Objects are addressed by absolute file offset.
   - `r2004.rs`: R2004, R2010, R2013, R2018. Decrypts the file header
     (LCG XOR), reads the page map and section map (system pages), then
     rebuilds each section from its pages, decompressing with `lz77.rs`.
     Objects are addressed by offset into the AcDb:AcDbObjects section.
   - R2007 has its own container and is refused for now.
3. **Classes** (`classes.rs`): custom types, object codes 500 and up.
4. **Object map** (`objmap.rs`): handle → offset. Both plausible readings
   are parsed; the one whose entries match the objects' own handles wins.
5. **Objects** (`object.rs`): each object is split into its main data,
   string stream (R2007+) and handle stream (`streams.rs`), then decoded by
   type (`entities.rs`, `tables.rs`). A failure or panic is contained to
   that object. Every object's extended data is decoded by `eed.rs`; once
   all objects are loaded, each application is named from its APPID
   record.
6. **Header** (`header.rs`): the variable list is a table, easy to correct.
   Values count as verified only when the control-object and space handles
   point at objects of the right type.
7. **Index**: owner → children, plus the model/paper space block records,
   built once after loading (`Drawing::rebuild_index`).
8. **Records** (`nongraphic.rs`): dictionaries, extension records,
   layouts, scales, multiline styles, placeholders, dictionary variables,
   and the VPORT and DIMSTYLE records are decoded into the model's own
   kinds (`model/objects.rs`); the style objects become their DXF groups
   (`styles.rs`, R2013 and later); objects that do not decode keep their
   bytes (`Object::raw`).

## DWG writer (`crates/cad-dwg/src/write`)

AC1032 (AutoCAD 2018) only. `objects.rs` encodes each object the model
holds whole, in the R2010+ object format, making every choice the format
leaves open (compressed forms, handle codes, padding) as AutoCAD 2027
does; objects kept as raw records of the same version are written as
they were. `sections.rs` builds the header, classes, object map,
auxiliary header, template, free-space, revision history, summary
information, preview (an indexed PNG of the active viewport on AutoCAD's
background), application information and its history (naming this
program) and the xref manifest, each in AutoCAD's layout. `../styles.rs`
converts the style objects the model keeps as DXF groups (visual styles,
materials, table, multileader, section and detail view styles, dimension
associations) to and from their DWG records; objects only DXF has (the
cell style maps AutoCAD adds to table styles in DXF) are left out of the
DWG. `container.rs` packs them into the R2004-family
paged container: data pages and their masked headers and checksums, the
section and page maps, the encrypted file header and its second copy.
Given AutoCAD's own compressed pages it rebuilds AutoCAD's files byte for
byte, so what a written file can differ in is the section content and
what `lz77::compress` writes; the compressor keeps to the stream forms
AutoCAD's files use (ending `11 00 00`, no copy under four bytes, no far
opcodes 0x17–0x1F), which the specification's reading allows more of.
`write::to_bytes` returns the file and notes on anything left out;
`write::to_bytes_with` takes `WriteOptions`: the program's name and
version for the application information, the preview PNG, a text-width
function for MTEXT extents and a hatch pattern library. Without them the
crate names itself, stores a one-pixel preview, estimates widths and
writes hatches with the lines they hold. `cad_core::dwg::write::to_bytes`
passes BlackCAD's own (the preview rendered by `render` and `raster`)
and first adds graphics blocks to dimensions that have none
(`export::dimblocks`).

## DXF (`crates/cad-dxf`)

`encode.rs` holds group values and both encodings (ASCII and binary) in
both directions. `write.rs` turns the whole drawing database into the
groups of an AC1032 DXF, record by record as AutoCAD lays them out;
`read.rs` is its mirror, turning a DXF (AC1015 and later, ASCII or
binary) into the drawing: names resolved to handles, and the DWG-only
parts (flag words, entity modes, control lists, header handle variables)
rebuilt the way a DWG from AutoCAD has them. `cad_core::open` recognises
DXF by its content. `WriteOptions` carries a hatch pattern library, as
for DWG; `cad_core::dxf::to_bytes` passes BlackCAD's and adds dimension
blocks. `export/dxf.rs` is the older R12 export of a rendered scene.

## Native format (`crates/cad-core/src/native`)

The byte-level spec is `FORMAT.md`. Modules:

- `tags.rs`: tags (DXF group code + explicit type + value) and varints;
  writers that leave out a field equal to its default, and readers that
  fill it back in.
- `schema.rs`: every record kind's fields, types, defaults and meanings.
  Written into each file as the `Schema` section, printed as FORMAT.md
  §8.2 (`cad schema`), and checked against `records.rs` by a test.
- `split.rs`: a section of records split into four streams (structure,
  floats, integers and handle differences, text) and joined again.
- `container.rs`: file header and trailer copies, two copies of the
  (compressed) section directory, entry flags, per-section DEFLATE
  (`codec`) on every core, DWG LZ77 for 1.x files, and CRC-32.
- `recovery.rs`: XOR parity over blocks of the snapshot, and the repair
  readers use when the snapshot is damaged.
- `records.rs`: the drawing database ↔ records mapping. Codes below 2000
  keep their DXF meaning (1000–1071 for extended data); 2000+ are the
  format's own. Fields, header variables and object kinds from a newer
  revision are kept as encoded tags and written back unchanged.
- `mod.rs`: full save (write beside, then rename), read, `NativeFile`
  (open once; one object via the object map, one page decompressed;
  header, properties, preview, schema, hash chain), change sets
  (`append_changes`, `apply`), the SHA-256 chain, `inspect`.
- `lock.rs`: `.cdl` locks with heartbeat and stale takeover (the `.dwl`
  equivalent), and `presence()` for "who has this open".
- `audit.rs`: the structural audit behind `cad inspect`: byte map of the
  whole file and a PASS/FAIL check for every validity rule in FORMAT.md
  §15, plus tag dumps for reading a file by eye, each field named from
  the file's schema.

Measured size and speed, and the plan for the format, are in
`FORMAT-ROADMAP.md`.

Saving has two modes. A **full save** writes a snapshot. An **incremental
save** appends a change set (put / delete / header operations) and a new
directory to the end of the file, then updates the header; either the old
or the new state is readable after a crash at any point. Opening applies
change sets on top of the snapshot; `cad compact` folds them back in.
Change sets are also the natural unit for live collaboration later: send
the same records between copies of the app.

## Model (`crates/cad-model`)

`Drawing` holds every object in a `BTreeMap<Handle, Object>`. An `Object`
has its handle, type, owner, reactors, extension dictionary, the common
entity properties (layer, color, linetype, lineweight, space) when it is an
entity, and a typed `ObjectKind` (`Line`, `Insert`, `Layer`,
`BlockHeader`, …). Types that aren't decoded yet are still present as
`Unsupported`, so counts, ownership and handles stay complete. Those
objects (and ones that failed to decode) also keep their original DWG
bytes in `Object::raw`, which the native format stores and a DWG writer
can emit unchanged. Every object also carries its extended data
(`Object::xdata`: blocks of `XValue`s per application, DXF groups
1000–1071) and any fields a newer `.cad` writer added
(`Object::unknown_tags`, kept as encoded tags).

Table records the core uses fully have their own kinds: `Layer`,
`Linetype` (with its dash pattern), `TextStyle` and `DimStyle` (the
dimension variables the renderer uses). Records it only names (DIMSTYLE
from DWG among them, for now) are `NamedRecord`. Lineweights stay as DWG's index codes; `Lineweight`
interprets them (standard weights, by layer, by block, default).

**Editing** (`model/edit.rs`): new handles from `$HANDSEED`; layers added,
renamed, recoloured, given a linetype or lineweight, switched on or off,
frozen, locked, made current or deleted, with DWG's naming rules (names
compare without regard to case; layer `0` is never renamed or deleted, a
layer in use is never deleted); entities added to model space or any
block, by layer in everything. Edits change the objects in memory; saving
is separate.

## Standards, units and templates

`standards` holds the drafting defaults DESIGN.md specifies: the default
layers, the `CENTER` and `HIDDEN` patterns in millimetres and inches, the
text and dimension styles for each unit, the canvas and interaction
colours, how colours show on screen and on paper, and how many pixels a
lineweight draws. `new_drawing` builds a drawing from the Metric or
Imperial template. Tests read DESIGN.md's tables and fail when the code
and the document disagree.

`units` converts between DWG's units (inches, feet and yards convert
among themselves without passing through 25.4), reads typed lengths
(`10mm`, `3/8"`, `1'-2 1/2"`) and angles, and formats lengths in the five
DXF display formats and dimension text (with alternate units and
DIMPOST).

## Font and text

`font` is the app's own single-stroke font, drawn for this project: ASCII,
drawing symbols and accented Latin letters, on a 12-unit cap-height grid,
written in a small path language and checked by tests. It lays out
single-line text with DXF's alignments (left, centre, right, middle,
aligned, fit; baseline, bottom, middle, top), width factor and oblique
angle, measures text exactly, and wraps MTEXT paragraphs. Every output
draws text through it, so text looks the same everywhere and needs no
installed fonts.

## Render (`render/`)

`render::model_space` walks model space and produces `Scene` items in world
coordinates:

- Object coordinate systems via the arbitrary axis algorithm. A mirrored
  OCS (extrusion 0,0,−1) flips arc direction correctly.
- Block references: base point, scale, rotation, OCS, MINSERT arrays,
  nesting up to 16 levels, attributes, layer "0" and ByBlock inheritance
  of colour, linetype and lineweight.
- Each item's `Style` carries the resolved colour, lineweight, linetype
  name and dash pattern (multiplied by `$LTSCALE`, the entity's linetype
  scale and any block scale).
- `dash` splits shapes into a pattern's dashes, centred on each feature
  (DESIGN.md §4). Outputs that draw call it; DXF writes linetypes instead.
- Dimensions draw their anonymous `*D` block when they have one (DWG);
  otherwise `dim` draws them from their definition points and dimension
  style: extension lines, filled 3:1 arrowheads or ticks, text in a break
  of the line, moved outside when it does not fit, leaders for radii and
  small diameters.
- Circles, arcs and bulge polylines stay analytic when the transform is a
  similarity; otherwise they are tessellated.

## Raster (`raster/`)

`raster::render` draws a scene as the canvas shows it (dark background,
grid, axis icon, screen colours, 1–3 pixel lines by lineweight) or as it
prints (black on white, or in colour). It builds one display list in
pixel space, then splits the image into horizontal bands of 64 rows,
handed round the cores in turn; each core reuses one band's working
buffers, so memory stays near the size of the finished image, and the
output is identical for any number of threads. Lines are
anti-aliased capsules (round ends and joins), fills use four sub-rows per
pixel with exact horizontal coverage. `raster::png` writes the image with
the core's DEFLATE (`codec`, dynamic Huffman codes) and zlib framing.
This is the renderer for screenshots and previews now, and the fallback
for platforms without a GPU path.

## Porting plan

| Target | How |
|---|---|
| Windows, macOS, Linux | `cargo build --release`. The desktop app will call the core directly. |
| Android | `cargo xtask android` builds `crates/cad-ffi` as `libcad_ffi.so` with the NDK's clang as the linker; the Java app in `apps/android` calls it through JNI. See below. |
| iOS | `cargo build --target aarch64-apple-ios` into a static library, wrapped as an XCFramework and called from Swift through the same entry points. |
| Web (PWA) | `cargo xtask web` builds `crates/cad-ffi` for `wasm32-unknown-unknown` and puts `apps/web` together in `dist/web/`; `cargo xtask serve` serves it locally. Decided 2026-10-08: the GUI is built and tested first as an installable PWA (the workbench, below). The native apps stay the goal, so the core keeps no browser-only code: everything the workbench does goes through `session.rs`, which a desktop or Android shell can drive the same way. |

## Browser workbench (`apps/web`)

The GUI, as the workspace mockups and DESIGN.md §10–§11 lay it out, built
as a Progressive Web App so it can be worked on and tested in any browser
and installed on Windows, Linux, macOS and Android from one build.

**Layers.**

```
apps/web/app.js      layout, input, commands (generators that ask for points, lengths, text)
apps/web/core.js     loads core.wasm, copies bytes in and out (the only glue)
cad-ffi/src/web.rs   wasm entry points: numbers in, bytes out (the only unsafe)
cad-ffi/src/session.rs   open drawings, undo, the command language: safe Rust
cad-core             everything else: render, display list, snaps, editing, .cad
cad-model, cad-dwg, cad-dxf   the drawing database, DWG and DXF
```

**No dependencies, as everywhere else.** No npm packages, no frameworks,
no `wasm-bindgen`: the module imports nothing (`xtask` refuses a build
that does), and the page is plain HTML, CSS and JavaScript modules.

**The session.** `session.rs` keeps the open drawings, an undo history per
drawing, and the scene rendered for queries. The history holds changes,
not drawings: the drawing's object map (`model/store.rs`) records, while
an edit runs, each object it touches as it was (a block record's entity
list one position at a time), and `Drawing::revert` puts them back and
returns the change that redoes it. An edit costs memory in proportion to
what it touched; `begin`/`end` merge a command's edits into one step
(COMMANDS.md §1). The history keeps at most 1,000 steps and about 64 MB
per drawing, oldest first. The scene is patched, not drawn again, when an
edit touched only model-space entities (`render::update_model_space`),
and the page asks for only those entities' items (`display_patch`). In
the browser the core counts the memory it holds; past 1 GiB it lets the
history go, then refuses to open or change drawings with a message, and
the page restarts the core and reopens drawings as last saved if it
still stops (a trap, a failed allocation, an address it cannot read). Apps drive it with one line of text per command
(`line 0 0 100 0`, `set 2A radius 6.5`, `snap x y r kinds`) and get JSON
back. The same interface serves the command line and the GUI, and will
serve the desktop and Android shells.

**Drawing on the canvas.** `export::display` turns the rendered scene into
a display list: lines, true arcs, polylines, fills and dots in world
coordinates, with dashes made and text stroked in the app's font, plus a
style table (screen colour, print colour, pixel width, layer). The page
draws it at any zoom, batched by style, and redraws selection, hover,
grips and previews over it. Picking, window and crossing selection, and
object snaps come from `snap.rs` in the core.

**Files.** Opening reads the file's bytes; saving writes them back to the
same file where the browser offers file handles (Chromium on desktop),
or downloads them otherwise. The installed app registers for `.dwg`,
`.dxf` and `.cad` (manifest `file_handlers`), so a file can be opened
with it from the operating system.

**Offline.** The service worker caches the app and `core.wasm` and serves
them when there is no network; it fetches first, so a new build is
picked up on the next load. `xtask web` stamps the cache name with a hash
of the build.

**What the browser limits.** One thread, so the core renders previews on
one (raster bands are drawn in place when only one thread is available).
No clock the standard library can read, so the page passes the time to the
core before each save (`cad_core::set_time`). No file paths, so no `.cdl`
lock file: presence shows "this browser" until the desktop shell exists.
A panic stops the module (WebAssembly builds use `panic=abort`); the page
keeps the panic message, starts a fresh module and reopens the drawings
from their files, and unsaved edits are lost.

## Platform bridge and the Android app

`crates/cad-ffi` is the only crate with `unsafe` code, and it keeps it at
the boundary: exported entry points, and taking ownership of the file
descriptors the app hands over.

**Only integers cross it.** The Android app (`apps/android`: Java, Android's
own APIs, no libraries) opens the drawing and two output files and passes
their file descriptors to `Native.view(drawingFd, infoFd, svgFd)`. The
library reads the drawing, writes a plain-text summary and model space as
SVG, closes all three, and returns a status code. No JNI objects are
involved, so there are no JNI bindings and no third-party crate, and the
same functions run on Linux, where the tests drive them through files and
pipes. The interface has a version number (`ABI_VERSION`) that the app
checks at startup.

**Build.** `cargo xtask android` finds the NDK and builds cad-ffi for each
ABI, with the NDK's clang as the linker (`--target=<abi><api level>`). It
aligns the library for 16 KB memory pages, gives it a SONAME, checks the
ELF headers of the result, and copies it to
`apps/android/app/src/main/jniLibs/<abi>/`, where Gradle packages it. CI
does the same, then checks the APK: 16 KB zip alignment, and no classes
from outside the app.

**Later.** Richer viewing (layers, measuring, panning without re-rendering)
needs more than one SVG: a compact display list the app draws itself,
passed the same way. iOS can use the same entry points, with file
descriptors from `FileHandle`.

### Open design point: saving on Android

Viewing only reads. Saving is where Android differs. The file picker, and
"Open with", give the app a content URI for one file. The app can read that
file through a descriptor, and often write it, but it cannot see the
folder. Two parts of the native format assume a folder:

- a full save writes a temporary file next to the drawing and renames it
  over the original, so a failed save never damages the file;
- the `.cdl` lock is a separate file next to the drawing.

Incremental saves (append a change set, then rewrite the header) need only
a seekable read-write descriptor. Local storage gives one; some cloud
providers do not.

Options:

1. **Work on a copy** in the app's own storage, where paths, renames and
   locks all work, and write the result back through the URI on save.
   Simple, but other people see no `.cdl` presence.
2. **Ask for the folder** (`ACTION_OPEN_DOCUMENT_TREE`) and create the
   temporary file and the lock through the document provider. This keeps
   presence, but document providers offer no atomic replace, and the save
   and lock code would need a small storage interface in place of
   `std::fs` paths.

iOS's document picker raises the same question. The likely answer is 1 for
single files and 2 for shared project folders; it needs deciding before
the editor.

## Roadmap

1. Verify the DWG reader against real files, and correct chapters
   9/10/20/23 against the spec PDF (see TEST-PLAN.md and PROVENANCE.md).
   Started: the first real file reads cleanly after two fixes
   (TEST-PLAN.md §7).
   Run `cad verify` on every corpus file: DWG → `.cad` → identical model.
2. Keep more of each decoded DWG object in the model (the remaining
   fields; extended data is done) so `.cad` holds everything a DWG writer
   needs.
3. AC1021 (2007) container.
4. Paper space and layouts; hatch patterns; TrueType fonts. (Linetypes,
   lineweights, the own single-stroke font for SHX text, dimension
   graphics and the drafting defaults are built: DESIGN.md §13. DWG
   linetypes and text styles are read; dimension styles are still to do.)
5. Drawing aids in the core: snaps with a spatial index, tracking, ortho,
   polar, UCS (DESIGN.md §9); selection drawing (§5). Started 2026-10-08:
   `snap.rs` (endpoint, midpoint, center, quadrant, intersection,
   nearest, node, insertion; picking; window and crossing boxes, by a
   straight pass), with ortho, polar and grid stepping in the workbench.
6. Editing commands and undo, saving through change sets (the command
   set and its shortcuts: `COMMANDS.md`). Started 2026-10-08 in
   `session.rs`: draw (line, polyline, circle, arc, rectangle, text),
   dimensions, move, copy, rotate, mirror, erase, property edits, layer
   commands, undo and redo. Offset, trim and fillet are next (the
   2026-10-08 advisory, P3-2, asked that they stay on this list).
7. DWG export (writer), then round-trip tests DWG → `.cad` → DWG.
8. Desktop app shell with `.cdl` presence. The GUI is built first as the
   browser workbench (`apps/web`, above). Android has a test viewer
   (`apps/android`); its editor waits on the storage decision above. Then
   iOS.
9. Live collaboration: exchange change sets between sessions.

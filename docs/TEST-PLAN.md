# Test plan for the next pass

The draft core has only been checked with unit tests and generated data.
The next pass proves it against real DWG files.

## 1. Build a corpus

Use your own fictional parts only, never customer drawings (the MosaicIron
naming works well). Aim for 6–10 small drawings that between them contain:

- lines, arcs, circles, closed and open polylines with arcs (bulges)
- a block inserted several times: scaled, rotated and mirrored
- nested blocks, and a block with attributes
- text and multiline text, linear and radius dimensions
- a solid-filled hatch and a pattern hatch
- an ellipse and a spline
- several layers, one frozen and one off, plus true-color entities
- extended data: a dimension with a style override (for example a changed
  arrow size), which AutoCAD keeps as extended data, and an annotative
  text or dimension

Save **each drawing in every version**: 2000, 2004, 2010, 2013 and 2018.
AutoCAD's SAVEAS does this; Autodesk's free DWG TrueView (DWG Convert) can
also convert a DWG between versions. Also save a **DXF of each drawing**
from the same program as the reference.

Suggested layout:

```
corpus/
  mosaiciron-bracket/
    bracket-r2000.dwg  bracket-r2004.dwg  bracket-r2010.dwg
    bracket-r2013.dwg  bracket-r2018.dwg  bracket.dxf
```

## 2. Run it

```
cad info    corpus/mosaiciron-bracket/bracket-r2018.dwg
cad objects corpus/mosaiciron-bracket/bracket-r2018.dwg --failed
cad svg     corpus/mosaiciron-bracket/bracket-r2018.dwg bracket.svg
cad verify  corpus/mosaiciron-bracket/bracket-r2018.dwg
cad save    corpus/mosaiciron-bracket/bracket-r2018.dwg bracket.blc
cad inspect bracket.blc
cad svg     bracket.blc bracket-from-blk.svg
```

## 3. What to look for

| Check | Good result |
|---|---|
| `info` → Header | "verified against object map" |
| `info` → Diagnostics | object map "N/N sampled handles match" |
| `info` → failed column | 0 for the decoded types |
| `svg` | matches the drawing on screen; mirrored blocks face the right way |
| Units and extents | match the reference DXF's `$INSUNITS`, `$EXTMIN`, `$EXTMAX` |
| `verify` | "OK: lossless round trip" for every file |
| `inspect` | "all checks passed"; the references check shows 0 dangling handles (any are DWG decode gaps) |
| `bracket-from-blk.svg` | identical to `bracket.svg` |
| `.blc` size | noticeably smaller than the text DXF; compare with the DWG |
| `info` → XDATA | the applications you expect (ACAD for dimension overrides); values match the reference DXF's 1000–1071 groups |
| `info` → Diagnostics | no warning about extended data: layer references (1003) that miss their layer, or blocks only partly decoded, point at the handle byte order or string reading in `eed.rs` |

Anything off points at a specific module: container problems show up
before objects load; object-layout problems show up as failed types or
wrong geometry for one entity type. The fix goes into the module, and the
correction is noted in `PROVENANCE.md` with the spec section it came from.

## 4. Turn the corpus into automated tests

Once the corpus reads cleanly, add a `tests/corpus.rs` integration test
that loads every file and compares entity counts and extents across the
versions of the same drawing. Every later change gets checked against it.

## 5. Native format checks already automated

`cargo test` covers, on generated drawings that use every object kind:
lossless round trip with and without compression; files with many object
pages and single-object reads through the object map; change sets appended,
reloaded and compacted; interrupted saves at several points (old or new
state must load); a torn file header (trailer copy used); thousands of
truncated and bit-flipped files (must never panic); fields, header
variables, value types and object kinds from a newer writer (must be kept
and written back byte for byte); extended data of every value type, in
snapshots and change sets, and malformed extended data (kept whole and
reported); DWG extended data built byte by byte from the spec (both string
forms, several applications, bytes that do not decode kept); `.bcl` lock
acquire, hold, heartbeat, stale takeover and release. Format 2.0 adds: the
schema checked against the record code field by field, for every kind
(required fields only, and every default written out, must decode to the
same object and re-encode to the same bytes), and FORMAT.md §8.2 checked
against the schema; a file written by the 1.1 code (`tests/fixtures`)
read in full, by object and through its change set, with a change set
appended the 1.x way, and every one of its records written again in the
1.x form byte for byte as 1.1 wrote it (checked once by hand too: change
sets appended by 2.0 read cleanly in the 1.1 build, and its audit
passes); split and DEFLATE sections at every level; sections
with unknown codecs, unknown critical flags or the encrypted flag skipped
and reported; either directory copy damaged, and a copy cut short
anywhere after the first directory copy; runs of damage rebuilt from the
recovery data, including when its parity is damaged too (and too much
damage reported, not invented); the hash
chain over snapshot and change sets, including a section edited with its
CRC fixed up; drawing properties and part data through change sets; the
DEFLATE codec against Python-generated vectors and hostile input; SHA-256
against FIPS and Python-generated vectors; takeoff of plates with holes,
chained lines and arcs, islands and inch units. `cargo run -p cad-core --example native_demo`
exercises the same path end to end. Every file the tests write, and
thousands of damaged ones, also go through the structural audit
(`native::audit`, `cad inspect`). `--example native_bench` measures size
and speed on 200,000 entities; results are recorded in
`FORMAT-ROADMAP.md`.

Still to do by hand: two copies of a future app on two machines sharing a
network folder, to confirm `.bcl` behaviour on SMB and cloud-sync folders
(Dropbox, OneDrive), which may delay or reorder file operations.

## 5a. Drafting standards and drawing (Stage 1), automated

`cargo test` covers:

- **The design document.** Tests read DESIGN.md's tables (default layers,
  linetypes, canvas and interaction colours, dimension style, text height)
  and compare them with `cad_core::standards`. Changing a value in the
  document without the code, or the reverse, fails a test that names the
  value; a lineweight checks to the exact hundredth and a colour's name
  must match its number.
- **Templates and layers:** new Metric and Imperial drawings have the six
  default layers with their colours, linetypes and lineweights, round-trip
  through `.blc` exactly; layers can be added, renamed, recoloured,
  re-weighted, given a linetype, switched, made current and deleted, with
  DWG's naming rules; layer `0` is never renamed or deleted; a layer in
  use is never deleted. New handles never repeat, even from a damaged
  `$HANDSEED` at the top of the range; new entities take `$CELTSCALE`;
  the layer list counts entities, not block markers.
- **Units:** conversions (inch fractions stay exact both ways: every 64th
  of an inch to 20 ft goes to millimetres and back unchanged), every typed
  form (`10mm`, `3/8"`, `1'-2 1/2"`, mixed numbers), refusals (including
  numbers too large to be lengths), the five display formats and values
  beyond them, angles without `-0`, dimension text with dual units (the
  alternate from the measurement, DIMPOST on the primary), the alternate
  factor per drawing unit, and that what is shown can be typed back, for
  unitless drawings and every unit's mark too.
- **Linetypes:** centre lines across a hole cross at the short dash and
  end in long dashes; hidden lines start and end with dashes; short
  features are solid; circles fit whole periods; polyline segments dash
  separately; over-dense patterns draw solid, and so do patterns holding a
  length that is not a number (they never hang); dashes on nearly straight
  bulge segments come out as chords, not circles; by-layer, by-block and
  entity linetypes, `$LTSCALE`, linetype scales and block scales resolve
  (by block too).
- **Dimensions:** text in a break of the line, extension line gaps, 3:1
  arrowheads; text, then arrows, moved outside when there is no room; text
  above the line, the DIMTAD exception for horizontal text on vertical
  lines, and text above past the end with the line under it; ticks; dual
  units; diameters inside or with a leader; text placed by hand gets a
  leader with a landing; radii; colours by block and by layer.
- **Font:** every glyph parses and stays in its box; ASCII and the drawing
  symbols are covered; alignment, rotation, fit and aligned text, oblique,
  wrapping.
- **Raster and PNG:** thin and thick lines land on the right pixel rows;
  dashes; fills; paper colours; identical images for 1 and 7 threads; the
  grid, and no grid at zooms where its spacing would overflow; the PNG's
  chunks and CRCs, its zlib stream (decoded by the tests' own inflate) and
  every pixel; empty and short images still make valid files. SVG text
  escapes to characters XML allows.
- **DWG tables:** STYLE decodes the ODA specification's own example; LTYPE
  patterns, text elements (flag 2) and shape elements (flag 4) from
  hand-built R2000 and R2010 records, the referenced and xref-dependent
  flags, and records that fail the checks keep their names.
- **Native format:** a dimension style coloured index 0 reopens as by
  block; a 1.0 file of an R13/R14 drawing reads lineweight 0 as unset.
- **DXF:** linetype tables and a `$LTSCALE` that is always a positive
  number.
- **The sample plate** draws on every default layer with nothing skipped,
  and draws the same after a `.blc` round trip.

**By hand:** `cad sample plate.blc`, then `cad png plate.blc plate.png`
and `cad png plate.blc paper.png --light`: compare with the mockups and
DESIGN.md. With real drawings: `cad layers`, `cad png` and `cad svg` on
the corpus files, checking that linetypes read from DWG draw as in
AutoCAD.

## 6. Android test viewer

**Automated:**

- `cargo test` drives the bridge's entry points (`crates/cad-ffi`) on Linux
  through real file descriptors. It covers:
  - a drawing from a file, and from a pipe (the way some storage providers
    hand files over);
  - bytes that are not a drawing, a drawing that cannot be read, and an
    output that cannot be written;
  - negative descriptors, and the same descriptor passed twice.

  Every test also checks that each output descriptor ends up closed.
- `cargo test` also covers `cargo xtask`: options, NDK discovery, and the
  ELF checks (machine, 16 KB alignment).
- CI builds the APK, then checks its 16 KB zip alignment and that it holds
  only the app's own classes.

**By hand, on a phone and on the emulator:**

| Check | Good result |
|---|---|
| `adb install -r app-debug.apk` | "CAD Viewer" appears among the apps |
| Open a corpus `.blc` and `.dwg` with the Open button | The summary shows format, object counts, units and extents; the drawing matches `cad svg` of the same file |
| "Open with" from a file manager | The same |
| Pinch zoom, pan, rotate the phone | The drawing stays sharp and stays open |
| Open a photo or a text file | "This is not a drawing this version can open", with the reason |
| A large drawing | The app stays responsive while it opens; no "isn't responding" |
| An Android 15+ emulator image with 16 KB pages | The app starts (the library loads) |
| A 32-bit phone, if one is at hand (`cargo xtask android --abi all`) | The same results as on 64-bit. This is the core's first run on a 32-bit system. |

## 6a. Browser workbench (`apps/web`)

Automated: `cargo test` covers the session (`crates/cad-ffi/src/session.rs`:
drawing, undo groups, property edits, copy/rotate/mirror, dimensions, snaps
and picking, saving every format and reading it back), the display list
and snaps in the core, and the server and module checks in `xtask`,
including that the single file still assembles from `apps/web`. After the
WebAssembly build, the workflow's `web` job runs
`apps/web/tests/core.test.mjs`: the built core through the page's own glue
(`core.js`) in Node, from its version to the corpus and damaged input
(`apps/web/README.md`, Tests).

In a browser (`cargo xtask serve`, then Chrome or Edge, desktop and phone
sizes):

- The start screen opens a new metric or imperial drawing, a file, and
  the sample plate; dropping a file on the window opens it.
- The sample looks like the workspace mockup: layers in their colours,
  dimensions green, centre lines yellow and dashed, notes cyan.
- Each tool and alias runs; typed points (`x,y`, `@dx,dy`, `@50<30`, a
  length along the cursor) and typed lengths in other units (`1/2"`,
  `25.4mm`) land where they should; Esc cancels; Enter repeats.
- Snap markers and tags appear at the right points; Tab cycles; `END`,
  `CEN` … force one pick; ORTHO and POLAR steer the cursor.
- Selection: click, Shift-click, window (left to right), crossing (right
  to left); Delete erases; grips drag; properties edit in place.
- Save writes back to the opened file (Chromium) or downloads; the saved
  DWG opens in AutoCAD or TrueView; the app installs and opens files from
  the file manager; it loads with the network off.
- Phone: chips, tool sheet, value fields with the number pad, pinch zoom,
  two-finger pan, the magnifier while placing a point.

## 6b. Edit cost, memory and damaged input (automated)

- `cargo test` runs `crates/cad-ffi/tests/damaged_inputs.rs`: 1,200
  damaged copies (cut short, bits flipped, runs overwritten, repeated or
  dropped) of the core's own files in every format it writes, plus a DXF
  of entities alone, and 200 random buffers, each opened, shown, edited
  and saved; no panic allowed, every refusal with a message. Cases that
  once failed are kept in `REGRESSIONS`. Longer runs:
  `CAD_FUZZ_CASES=20000 cargo test --release -p cad-ffi --test damaged_inputs`.
- `cargo run --release -p cad-ffi --example history_bench [objects] [edits]`
  measures memory and time per edit on a large drawing.
- Session tests check that the patched scene equals the scene drawn again
  after every step of a mixed run (edits, groups, undo, redo, cancel, a
  layer change), and that a client applying display patches holds the same
  items as the whole list.

## 7. Results so far

### 2026-10-02: splice plate, AC1032 (AutoCAD LT 2026)

The owner's first real file: a 7 × 6.375 in plate with eight 3/8 in holes,
with a DXF and a FastCAM file of the same drawing as references. The files
stay out of the repository.

| Check | First run | After fixes |
|---|---|---|
| Container, sections, object map, header | Read; header verified; 87/87 sampled handles match | Same |
| Geometry against the DXF | All 12 lines and 12 circles, same coordinates, same order | Same |
| Classes | Failed to parse (0 classes) | 17 classes |
| Objects failing to decode | 14 (every LAYER, APPID and BLOCK_HEADER) | 0 |
| Model space | Found only through the object owners | Its block record lists the 24 entities |
| `verify` (DWG → `.blc` → identical model) | OK | OK |
| `inspect` of the saved `.blc` | All checks pass | All 25 pass |
| SVG from the DWG and from the `.blc` | Identical | Identical |
| `.blc` size | 29.6 KB against the DWG's 32.9 KB | Same |
| `takeoff` | 12 contours, no holes, 1.325 in² (wrong) | 1 outline and 8 holes, 43.741 in², cut length 36.175 in; 8 repeated or overlapping curves (6.212 in) set aside |

What was wrong, and is now fixed:

- **Classes:** from R2007 a class's DWG and maintenance versions are BL
  (spec §5.8), not BS (§10.2).
- **Table records:** from R2007 (at least in R2018) a table record has no
  64-flag and no xref-dependent bit, only the xref index. The specification
  does not say so; the file does. Each record now ends exactly where its
  strings begin, and the reader reports any LAYER, APPID or BLOCK_HEADER
  that does not.
- **Takeoff:** the drawing holds four circles twice and four short lines
  lying on the plate's sides (the FastCAM file has the same repeats, so they
  came from upstream). Takeoff now sets such curves aside and reports them,
  instead of failing to close the outline and counting them twice.
  Measuring a 150,000-entity drawing takes about 2 s (it took minutes).

Not checked by this file: arcs, polylines, blocks, text, dimensions,
hatches, layers other than `0`, R2000 to R2013 files.

### 2026-10-02: MI-201 bent clip plate, R14 to 2018 (`corpus/mosaiciron-mi201`)

The owner's own part, saved by AutoCAD 2027 as R14, 2000, 2004, 2007, 2010,
2013 and 2018, with a DXF of each (and an R12 DXF). It is in the
repository; `cargo test` (`tests/corpus.rs`) checks every DWG against its
DXF: no failed objects, notes or warnings, a verified header, all 34
model-space entities (lines, arcs, circles, text, dimensions) by handle,
layer, geometry and text, and an unchanged `.blc` round trip.

| Version | First run | After fixes |
|---|---|---|
| R14 (AC1014) | 0 failed; 36 extended-data blocks partly decoded | Clean; matches the R12 DXF |
| 2000 (AC1015) | 0 failed; 28 blocks partly decoded | Clean; matches its DXF |
| 2004 (AC1018) | Classes failed (0 read); 26 blocks partly decoded | 11 classes; clean; matches |
| 2007 (AC1021) | Not readable: its container is not implemented | Same (next) |
| 2010 (AC1024) | Clean | Clean; matches |
| 2013 (AC1027) | Clean | Clean; matches |
| 2018 (AC1032) | Clean | Clean; matches |

R14 and 2018 render to identical images. Fixed:

- **Classes, 2004:** BL for a class's version numbers holds from R2004, not
  only R2007 (§10.2 says BS for both; neither file parses that way).
- **Extended-data strings, R13 to 2004:** the code page in front of each
  string is stored most significant byte first (00 1E for code page 30);
  it was read the other way, so every string was set aside.

Settled by these files:

- The table-record change (no 64-flag or xref-dependent bit) holds for
  2010, 2013 and 2018; 2004 and earlier use the specification's layout.
  2007 waits on its container.
- Handles in extended data (1005) are stored most significant byte first
  (2 of 2 resolve in every version).
- Anonymous blocks are stored as `*D`; the number DXF shows (`*D0`, `*D1`)
  is not in the DWG.

### 2026-10-02: DXF and DWG writers against the MI-201 corpus

- **Binary DXF:** AutoCAD 2027's binary save of our ASCII DXF
  (`mi201-r2018-binary.dxf`) reads to its last byte (6,756 groups) and
  writes back byte for byte; our header's variables, codes and order are
  AutoCAD's. Our first binary DXF stored groups 280–289 in one byte and
  AutoCAD refused it; with two bytes it opens, and AutoCAD's re-save of it
  holds every entity and block unchanged.
- **DXF round trip:** every corpus drawing, written as ASCII and as binary
  DXF and read back, writes the same DXF again byte for byte
  (`tests/corpus.rs`).
- **DWG objects:** 156 of the 158 objects of the 2018 file the model holds
  whole encode to AutoCAD's bytes; the two MTEXT differ only in the text
  extents, which come from this program's fonts. The classes and object
  map are AutoCAD's bytes; the header matches up to 69 trailing bits
  AutoCAD writes after the handles.
- **DXF to DWG:** the drawing read back from our DXF encodes to the same
  150 DWG objects as the drawing read from the DWG (arcs to within units
  in the last place, from the angles' trip through degrees).
- **Layer state:** layers that AutoCAD's DXF shows on are on here; bit
  0x02 of the R2000+ layer flag word marks a layer that is off (the
  specification calls it "on").

### 2026-10-08: why AutoCAD refused every DWG we wrote

AutoCAD 2027 called all of our AC1032 files invalid, including seven made
to isolate the cause: AutoCAD's own sections in our container (T0), then
our header, objects and small sections in its place one at a time, and
one with every page stored as a single literal run (T6). Since T0 failed
with nothing of ours in it but the packing, the fault was in the packing.

- **Container:** given AutoCAD's own compressed pages, `container.rs` now
  rebuilds AutoCAD's three 2018 files of the corpus byte for byte: file
  header, encrypted block, page headers, checksums, padding, both maps
  and the second header (`container_rebuilds_autocads_files`). The two
  new files are AutoCAD's DWG saves of our ASCII and binary DXFs.
- **Compression:** every compressed stream in AutoCAD's 2004 to 2018
  files (81 streams) ends `11 00 00`; ours ended `11`, so a reader that
  takes two more bytes after the end opcode, as AutoCAD's evidently does,
  read into the padding. AutoCAD also never writes a copy shorter than
  four bytes, nor the far opcodes 0x17–0x1F (it writes far copies of 11
  and 12 bytes with 0x10 and a count, where the specification's reading
  of 0x19 and 0x1A would be shorter). The compressor now writes only
  those forms, and a test holds AutoCAD's streams and ours to them
  (`compressed_streams_keep_to_autocads_forms`). T6 ended `11` as well,
  which is why it failed too.
- **In AutoCAD 2027 (owner, 2026-10-08):** with the fixed compressor every
  file opens and shows the drawing correctly: the MI-201 DWG written from
  the `.blc` and T0–T6. AutoCAD's notice that the file was not saved by
  Autodesk software comes with every file that holds our header (T1, T5,
  the MI-201 DWG) or our small sections, among them the application
  information naming this program (T4); the files with AutoCAD's header
  and small sections open without it (T0, T2, T3, T6), whoever wrote the
  objects or the compressed streams. Our header differs from AutoCAD's
  mainly in the bits AutoCAD writes after the handles, which we omit. The notice is true of every file this program
  writes, and the application information will not claim otherwise
  (PROVENANCE.md), so it is not treated as a fault.

### 2026-10-08: the written DWG laid out as AutoCAD lays it out

Compared with AutoCAD's save of the same drawing, section by section and
object by object:

- **Sections:** AcDb:AppInfoHistory and AcDb:XrefManifest are written as
  AutoCAD writes them; the auxiliary header, summary information,
  application information history and xref manifest match AutoCAD's
  first save of our DXF byte for byte where AutoCAD names itself and we
  name this program (`small_sections_follow_autocads`). The preview is an
  indexed PNG of the active viewport on AutoCAD's background.
- **Header:** six undocumented longs and one the spec gives as 24 take
  the values AutoCAD writes in every file; with the objects below present,
  the material and visual style handles are set as AutoCAD sets them.
- **Objects:** visual styles, materials, the table, multileader, section
  and detail view styles and the dimension associations were dropped by
  our DXF reader and kept only as bytes from DWG; they are now in the
  model (`.blc` 2.3). All 33 of AutoCAD's decode to the groups of its DXF
  (`style_objects_decode_to_autocads_dxf_groups`), and every object of
  the 2018 file but the two MTEXT encodes to AutoCAD's bytes, 189 of 191
  (`objects_encode_to_autocads_bytes`). AutoCAD's own DXF, read and
  written as DWG, gives AutoCAD's objects apart from values its DXF
  rounds to 16 digits and the DXF-only cell style map.
- **Still different, besides the authentication data:** the order of the
  objects in AcDb:AcDbObjects (AutoCAD's follows its own traversal of
  the drawing, which puts the first few children of each container after
  the rest; ours is handle order) and so the object map's bytes, and the
  approximate object count in AcDb:ObjFreeSpace.

**Wanted next:** the AC1021 container (spec chapter 5) so the 2007 file
reads; then drawings with blocks and attributes, polylines, hatches, splines,
ellipses, MTEXT, leaders, frozen and off layers, and true colours (§1).

### 2026-10-08: the browser workbench, first pass

Headless Chromium against `dist/web` at the mockup's size (1600 × 1000)
and a phone's (412 × 915): the sample plate renders as the workspace
mockups show it, on desktop and phone. A scripted session (keyboard and
mouse, as a user works) passed all 18 checks with no page errors: LINE
with Close as one undo step; circle, rectangle, arc and text; DIMLINEAR
and DIMDIAMETER on the DIMENSION layer; click selection; a radius typed
as `1/2"` in a millimetre drawing becomes 12.7; MOVE, COPY, ROTATE,
MIRROR; Delete and Ctrl+Z; a new layer; turning DIMENSION off; dragging a
line's end grip; saving `.blc`, DWG, DXF and SVG. Found and fixed on the
way: filled arrowheads reset the batch they were drawn in, so dimension
lines and texts were missing; COPY left its objects selected, unlike the
other modify commands. Not yet tried: a real browser on Windows and
Android, installing, and opening a workbench-written DWG in AutoCAD.

### 2026-10-08: the single-file test page

`dist/cad-test.html` (2.5 MB) opened from disk (`file://`) in headless
Chromium, desktop (1600 × 1000) and phone (390 × 844): the core loads from
the embedded base64 in about 0.2 s; the sample renders as in the app; a
circle drawn with a typed radius adds one object and Ctrl+Z removes it;
saves to DWG, DXF, `.blc` and SVG produce bytes; no page errors. The
served app (`dist/web`) still passes the 18-check session after the
loader change.

### 2026-10-08: the outside advisory (black-box review of 0.5.0)

Every item answered; measured on the WebAssembly build through the page's
own glue, unless noted.

| Item | Before (advisory) | Now |
|---|---|---|
| P1-1 DXF of entities alone | opened empty, edits failed | both entities drawn, layer HOLES made, edits work; tables-without-owners and R12 polylines too; "found, none placed" reported |
| P1-2 memory, 20k objects | ≈18 MB per edit, ≈1.2 GB | 500 edits: 27 MB held, 115 MB memory (from loading) |
| P1-2 time per edit, 20k | ≈60–127 ms | 0.12 ms; with patch and info 0.9 ms |
| P1-2 2,000 lines in one command | 1,789 MB, 12.6 s | +3.5 MB, 15 ms |
| P1-2 6,000 separate lines | 347 MB, 11 s | +10.9 MB, 37 ms |
| P1-3 results past 2 GiB | RangeError, page stuck | read at 2,419,507,760; soft limit refuses edits with a message; forced out-of-memory restarts the core, reopens both drawings, memory back to 2 MB |
| P2-1 pick at 20k | ≈23 ms | 0.15 ms (snap 0.17 ms) |
| P2-2 refresh after an edit | whole display list | patch of the changed entities; matches the whole list after every step |
| P2-3 units after DWG round trip | "Units: unknown" | millimeters in the summary and the info record, every format |
| P3-1 damaged input | 1,200 cases, 0 panics | now permanent in `cargo test`; it found two failures the advisory's run missed, both fixed: a damaged binary DXF whose dimension blocks contain each other drew without end (memory without limit; the renderer now skips a block met inside itself and stops at 2 million entity visits), and a DXF cut off just after `SECTION` panicked. 20,200 cases: 0 panics |
| P3-2 offset, trim, fillet | not built | still on the roadmap (ARCHITECTURE.md item 6) |

Browser, headless Chromium: the 18-check session still passes, and ten
checks for the items above pass with no page errors.

Found on the way, not fixed yet: the DWG files the core writes give no
VIEW, UCS, VPORT or APPID table (the new-drawing template has none), so
their header handles for those tables are null and our own reader marks
the header "not verified". AutoCAD may well refuse or repair such a file;
it should be checked against AutoCAD and the template completed.

### 2026-10-21: 0.9.0, the web app built and published by CI

The workbench in `apps/web` is now the one that was published at
cad.mosaiciron.com, restored from that page; `xtask` assembles it back into
that page byte for byte apart from this release's changes. Checked on the
build CI made from `main` (`6189f29`) and published to `gh-pages`:

- `apps/web/tests/core.test.mjs` on CI's `core.wasm`, in CI and again
  locally: all checks pass. The core reports BlackCAD 0.9.0; new metric
  and imperial drawings; the sample; LINE, CIRCLE, ARC, PLINE, RECT, TEXT,
  linear, aligned, radius and diameter dimensions, COPY, MOVE, ROTATE,
  MIRROR and ERASE, a `begin`…`end` group undone as one step, everything
  undone and redone; every query the page makes; saved as `.blc`, DWG,
  DXF, binary DXF, SVG and PDF, and the first four read back with the
  same model space; the corpus: 15 files at 34 entities each, the 2007
  DWG refused with a message; 52 damaged or random inputs refused without
  a crash.
- `BlackCAD.html` from `gh-pages` is byte for byte what `cargo xtask web
  --core` assembles from `main` around CI's core. Opened from disk in
  headless Chromium at 1600 × 1000 and 390 × 844: the core loads in about
  0.3 s and the start screen says "0.9.0 · single-file test build"; the
  sample opens; CIRCLE and LINE typed at the command line draw; Ctrl+Z and
  Ctrl+Y work; Save as .dwg downloads an AC1032 file; `mi201-r2018.dwg`
  opens and draws; the Blackboard opens the Bend allowance lesson and
  steps through it; no page errors.
- The published folder served over HTTP (`cargo xtask serve --no-build`):
  the service worker installs and caches the whole app shell, which it
  could not do before because the PNG icons it lists were missing; the
  manifest names the app BlackCAD and all four icons load.

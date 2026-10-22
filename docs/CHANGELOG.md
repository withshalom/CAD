# Changelog

## 0.17.0 (2026-10-22)
The native format's container and tagged records are public, so CADCraft's
`.cad` can keep its own records in the same container. No change to the
format (2.10) or to what BlackCAD writes.

- `cad-native`: the `container` and `tags` modules are public.
  `Builder::for_records` marks a file as holding another program's
  records (`RECORDS_CADCRAFT`); `tags::put_record` and `tags::records`
  frame and split a records section; `TagReader::next` is `next_tag`, so
  it isn't mistaken for an iterator's. A container that isn't native
  reports "not a native file (.blc or .cad)".
- Fixed from 0.16.0: three places said CADCraft's files are `.blc`; they
  are `.cad` (the doc of `EXTENSION`, and the message refusing a
  CADCraft file).
- Docs: the crate's README shows the container used from outside; FORMAT
  says which sections are shared and which are BlackCAD's.
- Tests: another program's records go through the container (compressed
  or not, with or without recovery data) and are read back from outside
  the crate; BlackCAD refuses them, naming them a CADCraft drawing (.cad);
  a damaged front header still says whose records a file holds.

## 0.16.0 (2026-10-22)
BlackCAD's native format is `.blc`, and its lock file `.bcl`. The names
`.cad` and `.cdl` go to CADCraft's native format (developed in the
owner's CADCraft fork), which shares this format's container, encoding
and structure but holds CADCraft's own records. The format is 2.10.

- `cad-native`: `EXTENSION` is `blc`. Header bytes 40–41 (reserved
  before) say whose records a file holds: 0 BlackCAD's, 1 CADCraft's
  (`RECORDS_BLACKCAD`, `RECORDS_CADCRAFT`, `records_of`). Earlier files
  have 0 there and read as before; a CADCraft file is refused with a
  message saying what it is. The `lock` module writes either kind
  (`LockKind::BLACKCAD`: `.bcl` beside `.blc`; `LockKind::CADCRAFT`:
  `.cdl` beside `.cad`) and reads both; the functions without a kind are
  BlackCAD's.
- Programs: the web app, the Android viewer and `cad` save and open
  `.blc` (the web app's save format is `blc`); they still open older
  BlackCAD files named `.cad`, telling formats apart by content as before.
- Docs: FORMAT (title, header, lock file, 2.10), README, ARCHITECTURE,
  DESIGN, COMMANDS, TEST-PLAN, FORMAT-ROADMAP, the app READMEs. The 1.1
  test fixture is `sample-1.1.blc`.
- Tests: the header says whose records a file holds, through either copy;
  CADCraft's records are refused; each lock kind writes its own name and
  reads both; lock paths of each kind.

## 0.15.0 (2026-10-22)
The drawing model holds multilines. MLINE was kept only as DWG bytes and
dropped from DXF; now it's read, written and stored with every vertex,
miter and element parameter, through DWG, DXF and `.cad`, as the CADCraft
fork models it. The `.cad` format is 2.9.

- `cad-model`: `MLine` and `MLineVertex` (`ObjectKind::MLine`): style,
  scale, justification, start point, extrusion, flags (open, closed,
  caps suppressed), the style's element count, and per vertex its point,
  direction, miter and each element's segment and fill parameters.
- `cad-dwg`: read and written (ODA specification §20.4.50, checked
  against the text); multilines older `.cad` files kept as DWG bytes are
  decoded when read.
- `cad-dxf`: MLINE read and written in the DXF reference's group order; a
  multiline that names its style without a handle finds it by name.
- `cad-native`: format 2.9, object kind 45 MLINE. A 2.8 reader keeps it
  as unknown data.
- `cad-core`: keeps multilines and writes them back; it does not draw
  them yet.
- Tests: the specification's MLINE example (an R14 object) decodes to a
  three-vertex STANDARD multiline with unit directions and miters; a
  closed three-element multiline with breaks and fill parameters
  survives DWG, DXF and DXF by style name; `.cad` round trips.

## 0.14.0 (2026-10-22)
The drawing model holds raster images and wipeouts, and the objects an
image needs. IMAGE and WIPEOUT were kept only as DWG bytes and dropped
from DXF, as were image definitions; now they're read, written and stored
as what they are, through DWG, DXF and `.cad`, as the CADCraft fork models
them. The `.cad` format is 2.8.

- `cad-model`: `RasterImage` (`ObjectKind::Image`, `ObjectKind::Wipeout`):
  placement by pixel vectors, size, display flags, brightness, contrast,
  fade, the clip boundary and mode, and the image's definition and
  reactor. IMAGEDEF, IMAGEDEF_REACTOR and RASTERVARIABLES are kept as
  their DXF groups (`Groups`). `Drawing::class_number` registers the
  classes these types need when a drawing lacks them (`KNOWN_CLASSES`),
  so `add_entity` can make images and wipeouts.
- `cad-dwg`: IMAGE and WIPEOUT read and written (ODA specification
  §20.4.80, checked against the text; WIPEOUT has IMAGE's record), and
  the three objects (§20.4.81, §20.4.82, §20.4.91) in every version. An
  image with no clip boundary is written with the whole-image rectangle.
  Tolerances, images and wipeouts that older `.cad` files kept as DWG
  bytes are decoded when read.
- `cad-dxf`: IMAGE and WIPEOUT read and written; a DXF without a CLASSES
  section still gives these types, from the classes the model knows.
- `cad-native`: format 2.8, object kinds 43 IMAGE and 44 WIPEOUT, with
  DXF's group codes. A 2.7 reader keeps them as unknown data.
- `cad-core`: keeps images and wipeouts and writes them back; it does not
  draw them yet.
- Tests: the specification's IMAGE, IMAGEDEF, IMAGEDEF_REACTOR and
  RASTERVARIABLES examples (R14 objects) decode to a 128 × 128 image with
  its definition, reactor and settings; an image with its definition and
  reactor and a wipeout survive DWG, DXF, and DXF without classes; `.cad`
  round trips; a tolerance an older reader kept as bytes is decoded.

## 0.13.0 (2026-10-22)
The drawing model holds geometric tolerances. TOLERANCE is the first of the
entity types the CADCraft fork models and BlackCAD kept only as DWG bytes
(and dropped from DXF): from now on it is read, written and stored as what
it is, so the two programs carry the same tolerance frames through DWG,
DXF and `.cad`. The `.cad` format is 2.7.

- `cad-model`: `Tolerance` (`ObjectKind::Tolerance`): insertion point, X
  direction, extrusion, frame text in the DXF reference's GD&T codes, the
  dimension style, and the short, text height and gap that only R13 and
  R14 DWG files store.
- `cad-dwg`: read and written (ODA specification §20.4.49, checked against
  the specification text). Like the other types decoded since 0.6, a
  record that doesn't fit the layout is still kept as its DWG bytes.
- `cad-dxf`: read from and written to TOLERANCE (AcDbFcf: 3, 10, 1, 210,
  11). Before, DXF tolerances were left out with a note.
- `cad-native`: format 2.7, object kind 42 TOLERANCE, private codes
  2191–2193 for the R13/R14 fields. A 2.6 reader keeps the new kind as
  unknown data and writes it back.
- `cad-core`: keeps tolerances and writes them back; it does not draw or
  edit them yet.
- Tests: the specification's own TOLERANCE example (§20.4.49.1, an R14
  object) decodes to its frame text, DIMTXT and DIMGAP; a tolerance
  survives DWG and DXF; `.cad` round trips with and without the R14
  fields.

## 0.12.0 (2026-10-22)
Hatches keep the objects their boundaries were made from. An associative
hatch in DWG lists, path by path, the objects it follows, and those carry
the hatch as a persistent reactor, so a stretched boundary drags the fill
along in AutoCAD. BlackCAD skipped the list when it read DWG, wrote none,
and ignored DXF's 97 and 330 groups, so every associative hatch it passed
on came out detached. Found by the CADCraft fork's hatch associativity
test once its saves went through BlackCAD. The `.cad` format is 2.6.

- `cad-model`: `Hatch::sources`, the boundary objects of each path.
- `cad-dwg`: read and written, path by path (soft pointers after the
  common handles).
- `cad-dxf`: read from and written to each path's 97 and 330 groups.
- `cad-native`: format 2.6 ends HATCH records with the lists (2189, 97,
  2190), written only when a path has boundary objects.
- `cad-core`: hatches the core makes are not associative and list none.
- Tests: an associative hatch with a boundary on one path and none on the
  other survives DWG and DXF, with the boundary's reactor; the format's
  sample hatch is associative.

## 0.11.1 (2026-10-22)
The DWG reader takes MEASUREMENT (imperial or metric) from AcDb:Template,
the section DWG keeps it in. It never did, although the DXF reader's
comments said so, so every DWG BlackCAD opened was imperial in the DXF it
wrote and in what it handed on: a metric DWG saved by BlackCAD and opened
again came back imperial. Found by the CADCraft fork's template test once
its saves went through BlackCAD.

- `cad-dwg`: MEASUREMENT is the section's last short (0 or 1; anything
  else is not believed). R13 to 2000 files have no such section and keep
  the default.
- Test: AutoCAD's MI-201 reads as imperial; a metric drawing written and
  read back is metric, in the model and in its DXF.

## 0.11.0 (2026-10-22)
Attributes keep Lock position. AutoCAD 2007 and later store it with every
attribute; BlackCAD read it from DWG and dropped it, wrote "unlocked" to
every DWG, and neither read nor wrote it in DXF or `.cad`. It showed when
the CADCraft fork's 2018 saves started going through BlackCAD (0.10.1):
the fork's own test of attribute modes failed. The `.cad` format is 2.5.

- `cad-model`: `Attribute::lock_position`. A reference's attributes take
  it from their definitions (`cad-core`'s INSERT).
- `cad-dwg`: read and written for AutoCAD 2007 and later.
- `cad-dxf`: read from the 280 after the flags (the 280 before the tag is
  the subclass version) and written there when set.
- `cad-native`: format 2.5 adds lock position (280) at the end of ATTRIB /
  ATTDEF records, written only when set, so records without it are as 2.4
  wrote them and a 2.4 reader keeps it as a field from a newer writer.
- Tests: a locked and an unlocked definition survive DWG and DXF; the
  format's sample has a locked definition, so every `.cad` round trip
  checks it.

## 0.10.1 (2026-10-22)
The DWG writer takes three objects in the DXF reference's layout as well as
AutoCAD's. The CADCraft fork writes them that way, and BlackCAD's writer
refused them, so the fork's 2018 DWG saves went to its second engine
(acadrust) and came back with 29 more objects BlackCAD could keep only as
raw bytes. Found by the fork's round-trip test on the MI-201 corpus.

`cad-dwg`:
- TABLESTYLE: the version group (280) is optional (0 without it, as
  AutoCAD writes it); each row's groups are read in any order, so the
  borders may come by kind (274–279, 284–289, 64–69) as the reference lists
  them or interleaved as AutoCAD writes them; the cell data type, unit
  type and format (90, 91, 1), which the older layout has none of, take
  AutoCAD's values (512, 0, none). A group a row doesn't have is still an
  error.
- MLEADERSTYLE: the last group (298) is optional (0 without it).
- DIMASSOC: each point reference may carry a 301, which must be empty (no
  intersection or xref data).
- Test: AutoCAD's three objects from the corpus, rewritten in the
  reference's layout, encode to AutoCAD's bytes.

## 0.10.0 (2026-10-22)
The native `.cad` format is open source: it moves out of the core into a
crate of its own, `cad-native`, licensed MIT OR Apache-2.0 like
`cad-model`, `cad-dwg` and `cad-dxf`, so other programs can read and write
`.cad` without the rest of BlackCAD. The first is the owner's fork of
CADCraft. The format itself (2.4) is unchanged: files written before and
after read the same.

`cad-native`:
- The `.cad` reader and writer, the change journal, `.cdl` locks, `audit`
  and `schema`, and the DEFLATE, zlib and SHA-256 code they use
  (`cad_native::codec`), moved unchanged from `cad_core::native` and
  `cad_core::codec`.
- What a file needs from the program that writes it is an option:
  `WriteOptions::writer` (the name recorded as `$CAD_WRITER`; without one,
  `cad-native` and its version) and `WriteOptions::preview_svg` (the
  preview, as an SVG the program drew already or a function that draws
  one; without it, files have no preview).
  `lock::acquire_with` names the program in lock files.
- `set_time` and `now_unix`: the clock for save times and change sets,
  set by the page in a browser, as the core's was.
- The panic-free lint the other open crates carry. Four places that could
  only panic on a broken invariant now return an error or cannot fail: an
  unknown section codec, the directory placement loop, the object page in
  use, and the fixed DEFLATE tables; SHA-256 blocks are taken without a
  conversion that could panic.

`cad-core`:
- `cad_core::native` re-exports `cad_native` under the same names and
  fills in the core's preview of model space and its name
  (`cad-core <version>`) when the caller leaves them out, so files the core
  writes are the same as before. `cad_core::codec`, `now_unix` and
  `set_time` are `cad_native`'s.

CI: the APK check no longer fails at random (`echo | grep -q` under
`pipefail` broke its own pipe when grep matched early).

Tests: the format's 64 tests move with it; two that started from the core's
templates now start from the format's own sample drawing. New tests check
that the core still stores its preview and name, that the crate alone
stores neither, and that both templates round-trip.

## 0.9.0 (2026-10-21)
The repository builds and publishes BlackCAD in the browser: on every push
CI compiles the core to WebAssembly, puts the workbench together and checks
the core through the page's own glue, and from `main` it publishes the app
and a single-file build, `BlackCAD.html`. The core, the open crates and the
`.cad` format (2.4) are unchanged from 0.8.0; their version numbers move
with the workspace.

Web app (`apps/web`):
- The workbench people used at cad.mosaiciron.com (BlackCAD with the
  Blackboard, built on core 0.6.0) had been changed after its sources were
  archived, so `apps/web` held an older page with the old name. Its files
  are restored from the published page: `index.html`, `app.js`, `core.js`,
  `icons.js` and `style.css`. Put together by `xtask`, they reproduce that
  page byte for byte, apart from the changes below.
- The installed app is named BlackCAD (manifest).
- The single file says what it is: "single-file test build" on the start
  screen and "(test build)" in the window title, and it no longer registers
  the service worker when it is served beside the app.
- PNG icons (192, 512 and maskable 512 pixels), which the manifest, the
  service worker and iOS ask for, rendered from `icon.svg`. The service
  worker could not install without them, so the app can now be installed
  and work offline.
- `apps/web/README.md`: the files, how the page talks to the core,
  building, publishing and tests.

Build and CI:
- `cargo xtask web` writes `dist/BlackCAD.html` (was `dist/cad-test.html`).
  `--core FILE` puts the app together around a core built before (one
  from the `gh-pages` branch, say), for working on the page without the
  WebAssembly target installed; `serve` takes it too.
- `apps/web/tests/core.test.mjs` runs the core through `core.js` in Node:
  its version against `Cargo.toml`, new drawings, the sample, drawing and
  editing with undo and redo, the queries the page makes, saving in every
  format and reading back, the corpus (every DWG but 2007 and every DXF,
  34 entities each), and damaged input. The `web` job runs it after the
  build; an `xtask` test checks that the single file still assembles from
  `apps/web`.
- The `pages` job publishes `dist/web` and `BlackCAD.html` to the
  `gh-pages` branch from `main` once every job has passed (one commit,
  replaced each time, with `CNAME` and `.nojekyll`); GitHub Pages serves
  that branch at cad.mosaiciron.com. Each run keeps `blackcad-web` and
  `BlackCAD.html` (not zipped) as artifacts.
- The workflow runs for tags as before, and not for `gh-pages`.

Documents:
- `docs/blackboard/`: the Blackboard's curriculum, the shop advisory the
  lessons were built from, and every lesson's sources and sign-off.
- `docs/reviews/2026-10-08-advisory-0.5.0.md`: the outside advisory that
  `TEST-PLAN.md` answers item by item, kept as received.
- `PROVENANCE.md`: the restored web app and the Blackboard's lessons, the
  icon renderer, and the fork of CADCraft that now uses `cad-dwg` and
  `cad-dxf` (one way: nothing comes back).

## 0.8.0 (2026-10-21)
The drawing database and the DWG and DXF readers and writers become crates
of their own, open source under MIT OR Apache-2.0, so other programs can
read and write DWG with them. BlackCAD itself reads, writes and draws as
0.7.0 did: the `cad` binary built before and after the move writes
byte-identical SVG, PNG, DXF and DWG files for every corpus drawing. The
`.cad` format stays at 2.4.

Licensing:
- `crates/cad-model`, `crates/cad-dwg` and `crates/cad-dxf` are licensed
  under either of the MIT License or the Apache License, Version 2.0; each
  holds both texts. Everything else stays proprietary, and `LICENSE` names
  the three crates as the exception.

Build:
- `cad-model`: the drawing database (`model`), `geom`, `text`, `error` and
  the DWG version codes. `cad-dwg`: the DWG reader and writer. `cad-dxf`:
  the DXF reader and writer. All three use the standard library only.
- `cad-core` depends on them and keeps every path: `cad_core::model`,
  `geom`, `text`, `error`, `dwg` and `dxf` are re-exports.

API (0.x: a minor version):
- The writers take what they used to fetch from the core as options:
  `cad_dwg::write::to_bytes_with(d, &WriteOptions)` (program name and
  version, preview PNG, text widths, hatch pattern library),
  `cad_dxf::to_bytes_with` and `groups_with` (hatch pattern library).
  `cad_core::dwg::write::to_bytes`, `cad_core::dxf::to_bytes` and
  `cad_core::dxf::groups` pass BlackCAD's own, as before.
- `DimVars::from_header` is now `cad_dwg::nongraphic::dim_vars_from_header`;
  `cad_dwg::hvars::Hv` and `nongraphic::{XrType, xr_type}` are public;
  new `model::HatchLibrary`, `model::Change::keeps_raw_header`,
  `render::hatch::library_lines`.
- `DwgOutput` and `DxfOutput` gain `left_out`: the objects the file does
  not hold, by type name, so a program can tell a whole conversion from a
  partial one. The DXF reader's note on record types it leaves out is now
  a warning, and the DXF writer's note says why objects were left out
  (it said AutoCAD recreates them, which is not true of every one).
- `cad_dwg`'s container packer returns an error instead of panicking when
  the sections and the section map disagree.

Fixes:
- DXF: a HATCH claiming trillions of paths, vertices, edges or knots made
  the reader ask for 96 TB and abort the process. Its counts are now
  bounded by the groups the file holds.
- DWG writer: a style object read from DXF whose list count claimed two
  billion items aborted the save; bounded the same way.
- The open crates make no panicking calls outside their tests; clippy
  denies `unwrap`, `expect`, `panic!` and `unreachable!` there.

Tests:
- Tests that need BlackCAD's templates or renderer moved to cad-core's
  `tests/` (`model_edit`, `dxf_minimal_files`, `dwg_header_vars`); the DWG
  writer's tests read AutoCAD's DXF through cad-dxf as a dev-dependency.
- New: the hostile hatch counts (2), the style list count (1), and what
  the writers and the DXF reader leave out (4).
- 1 + 163 + 6 + 2 + 4 + 5 + 58 + 9 + 26 + 1 + 16 + 12 tests and 1
  doc-test pass, 20 ignored (cad-cli; cad-core and its tests corpus,
  dwg_header_vars, dxf_minimal_files, model_edit; cad-dwg; cad-dxf;
  cad-ffi and damaged inputs; cad-model; xtask).

Docs:
- README, ARCHITECTURE, CONTRIBUTING and PROVENANCE describe the three
  crates; each has a README.

## 0.7.0 (2026-10-21)
Rust 2024. No change to what the program reads, writes or draws: built
from the same sources under the 2021 and the 2024 edition, the `cad` binary
writes byte-identical SVG, PNG, DXF and DWG files for every corpus drawing.
The `.cad` format stays at 2.4.

Build:
- The workspace uses the 2024 edition, resolver 3 and `rust-version = "1.87"`
  (the newest standard-library API in use is from 1.87).
- `apps/cad-cli` and `xtask` are workspace members again; 0.6.0 had lost them,
  so `cargo xtask` and the `cad` binary did not build.
- The WebAssembly exports are marked `#[unsafe(no_mangle)]`.
- CI denies clippy warnings, checks the declared minimum Rust version, and
  lints the WebAssembly build of cad-ffi.

CI (its first runs on GitHub Actions):
- Files are checked out exactly as committed on every platform
  (`.gitattributes`): Windows had converted line endings, and its tests
  failed on files they compare byte for byte.
- The APK check allows what D8 adds to the APK (stubs of platform classes,
  android.* and dalvik.*, and its desugaring helpers); any other code from
  outside the app still fails it.
- A failing step shows the last 6 KB of its output as an annotation.

Code:
- clippy's 13 warnings fixed. API change: `boundary::hatch`,
  `ops::hatch_at` and `ops::hatch_objects` take the pattern as one
  `boundary::HatchPattern` (name, scale, angle, double).

Tests:
- `small_sections_follow_autocads` expected the product name CAD; it now
  reads `PRODUCT_NAME`. It had been passing only where the corpus was missing.
- New `the_corpus_is_present`: the corpus tests no longer pass silently
  without their files.
- AutoCAD's binary DXF of MI-201 is not in the corpus (see its README).
- 1 + 249 + 6 + 26 + 1 + 12 tests and 1 doc-test pass (cad-cli, cad-core,
  corpus, cad-ffi, damaged inputs, xtask). The WebAssembly module is built
  by CI only.

Docs:
- README names the program BlackCAD; new CONTRIBUTING.md.

## 0.6.0 (2026-10-15)
Engine (cad-core / cad-ffi):
- All drafting tools taught by the lessons now live in the core: modify, arrays, blocks, units and scale, views (XLINE/RAY), sections and hatch, dimension commands, dimension rules, layers (plot flag), layouts and the sheet builder.
- Saved files carry the 0.6.0 stamp; DWG/DXF output now includes the synthesized dimension blocks.
- 249 + 5 + 26 + 1 + 1 tests pass; wasm has 0 imports, 19 exports.
Page:
- Drafting tutorials finished: 16 lessons in 7 units, with checked exercises. The page only prompts, previews and sends command lines to the core.

## 0.5.1
Earlier build (version number was not bumped for later engine work).

# Changelog

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

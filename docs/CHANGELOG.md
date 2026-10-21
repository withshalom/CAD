# Changelog

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

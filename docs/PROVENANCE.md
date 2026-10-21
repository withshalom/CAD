# Provenance and clean-room record

This file records how the code was produced, so the project can show it
was written independently and carries no third-party license obligations.
It is a factual record, not legal advice. Have an IP attorney review it
before commercial release.

## Rules this project follows

1. **No third-party code.** No crates, no vendored sources, no copied
   snippets. `Cargo.toml` files declare no dependencies; `cargo tree` shows
   only this workspace. The Android app uses only Android's own platform
   APIs (no AndroidX, Kotlin or other libraries), and CI fails the build if
   its APK contains any class from outside the app. Build tools (Gradle,
   the Android Gradle plugin, the Android SDK and NDK) build it but are not
   part of it, and the Gradle wrapper's jar is deliberately not kept in the
   repository. The browser workbench (`apps/web`) is plain HTML, CSS and
   JavaScript with no packages, frameworks or fonts from elsewhere, and its
   WebAssembly module imports nothing.
2. **Format knowledge comes from published specifications**, not from other
   implementations.
3. **Not consulted, at any point:** LibreDWG, libdxfrw, ACadSharp, acadrust
   (see "Third-party code considered and rejected"), the ODA
   Drawings SDK or its headers, Teigha, ezdxf, and the DWG import code in
   the separate KAD project (which uses LibreDWG). For completeness: in the
   same working session, before this project started, the AI read KAD's
   planning notes about integrating LibreDWG. Those notes cover library
   choice, licensing and API names, not the DWG file format.
4. Every module names the specification sections it implements and whether
   the code was checked against the specification text.

## Sources used

| Source | Status | Notes |
|---|---|---|
| Open Design Alliance, *Open Design Specification for .dwg files*, v5.4.1 (free public download from opendesign.com) | Chapters 2–4 read directly while writing. Chapter 28 (extended data), and the parts of chapter 20 it depends on, checked against the full text | © ODA, offered "for informational use". The spec is ODA's reverse-engineered description of the format; ODA is not affiliated with Autodesk. On 2026-09-29 the owner added the PDF to the CAD project and supplied a text export of it, which is what was checked. |
| Same document, chapters 9, 10, 20 (apart from the parts above), 23 | Written from knowledge of the spec, **not yet checked against the text** | The tool first used to read the spec could only reach the first chapters. The full text is now available, so these parts can be checked (see below). |
| AutoCAD DXF reference (openly published by Autodesk) | General knowledge; the extended-data page read directly on 2026-09-29 | DXF group codes for the DXF writer and the native format; the arbitrary axis algorithm for object coordinate systems. Extended data: "About Extended Data (DXF)" (help.autodesk.com, AutoCAD 2018 DXF reference) for the 1000–1071 codes, how each value follows edits, and the string, name and chunk limits. |
| AutoLISP reference, `xdroom` (documentation.help mirror of Autodesk's AutoLISP functions reference) | Read directly on 2026-09-29 | The 16 KB limit on extended data per object. |
| General mathematics | — | Vectors, affine transforms, B-spline evaluation (de Boor), CRC and Adler-style checksums, scanline polygon filling and anti-aliasing by coverage. |
| RFC 1950 (zlib), RFC 1951 (DEFLATE), W3C PNG specification | General knowledge of the published standards | `codec/`: the DEFLATE encoder (hash-chain matching, stored, fixed and dynamic Huffman blocks, length-limited codes) and decoder, and zlib framing; the PNG writer's chunk layout and filters. |
| FIPS PUB 180-4 (Secure Hash Standard) | General knowledge of the published standard; checked against its published example hashes | `codec/sha256.rs`, used by the native format's hash chain. |
| ASME Y14.2, ISO 128 (drafting line conventions) | General knowledge | Line proportions and widths chosen in DESIGN.md; numbers, not text, are used. |
| Java Native Interface specification (Oracle, public) and Android's platform API reference | General knowledge | How native methods are named and called (`Java_<package>_<class>_<method>`, the environment and class arguments), and the platform classes the Android app uses. |
| Android developer documentation: "Support 16 KB page sizes", Google Play target API requirements | Read directly on 2026-09-29 | 16 KB alignment of native libraries (NDK r28+ default, the linker flags for older NDKs, `zipalign -P 16`), target SDK 36. |
| Common drafting vocabulary (command names such as LINE, OFFSET, TRIM; snap names such as END, MID, CEN) | General knowledge | `COMMANDS.md`. The aliases were assigned by the project's own rule (first letter for the most-used command, more letters for less-used ones); where that produces the same short alias other CAD programs use, the familiarity is intended. No other program's alias or menu file was copied or consulted. |
| The owner's MI-201 corpus (2026-10-02): one MosaicIron part saved by AutoCAD 2027 in every DWG version from R14 to 2018, with DXFs | Read with this project's own tools only | `corpus/mosaiciron-mi201`, in the repository with the owner's permission; checked by `tests/corpus.rs` (TEST-PLAN.md §7). |
| One real DWG supplied by the owner (2026-10-02): a splice plate, AC1032, saved by AutoCAD LT 2026, with a DXF and a FastCAM file of the same drawing as references | Read with this project's own tools only | Used to check the reader (TEST-PLAN.md §7). The files are kept out of the repository and the project. Three generic records from the DWG (layer "0", application "ACAD", the "*Paper_Space" block record: no names, geometry or job data) are test bytes in `dwg/object.rs`. The FastCAM file was looked at only for its general layout. |
| AutoCAD 2027's own outputs of the MI-201 drawing: its DWG files, its DXF of each, and its binary DXF of a DXF this project wrote (2026-10-02), and its DWG saves of the ASCII and binary DXFs this project wrote (2026-10-08), all supplied by the owner | Read with this project's own tools as black-box observations | How AutoCAD fills what the specification leaves open, compared bit for bit: object encodings, handle codes, the container's page order, checksums and padding, the forms of its compressed streams, the small sections it adds (application information history, xref manifest) and the constants in them, the small sections (auxiliary header, template, free space, revision history, summary information, preview, application information), and the binary DXF value sizes (280–289 two bytes, 290–299 one byte, 310 a length byte). |
| ELF format (System V ABI) | General knowledge | The file and program headers that `cargo xtask` reads to check a library's machine and segment alignment. |

## Per-module record

| Module | Implements | Checked against spec text? |
|---|---|---|
| `dwg/bits.rs` | §2.1–§2.13 bit codes, modular chars/shorts, handles | Yes. Unit tests use the spec's own worked examples. One open question: the length prefix of BITLONGLONG (§2.4). |
| `dwg/crc.rs` | §2.14 CRCs, §4.2 page checksum | Yes. Tables are generated and tested against the spec's printed prefixes. |
| `dwg/lz77.rs` | §4.7 compression | Yes. Tests use the spec's literal-length example. |
| `dwg/r2000.rs` | §3 R13–R15 file header and section locators | Yes. |
| `dwg/r2004.rs` | §4.1–§4.6 R2004 header, page map, section map, masked page headers | Yes. |
| `dwg/section.rs` | Header/classes section framing and sentinels | Partly. The file-header sentinel is checked; the header and classes sentinels are from knowledge. |
| `dwg/header.rs` | Chapter 9 header variables | **No.** It checks itself at load: values are only trusted when the control-object handles it reads point to objects of the right type. |
| `dwg/classes.rs` | Chapter 10 classes | Partly. The R2004+ layout (§10.2) checked against the text and against a real AC1032 file. A class's DWG and maintenance versions are BL from R2004, as §5.8 gives them for R2007, not BS as §10.2 does: real AC1018 and AC1032 files parse only that way (a maintenance number of 377 shows it). |
| `dwg/objmap.rs` | Chapter 23 object map | **No.** It also checks itself: it keeps whichever of two readings matches the handles stored in the objects. |
| `dwg/object.rs`, `streams.rs` | Chapter 20 common object/entity format, R2007+ string stream | **No**, except the fields up to and including the extended data (§20.1, §20.4.1), which were checked. A real AC1032 file confirms the string-stream rule as implemented (a flag bit ending the data, a 16-bit size in bits before it; §20.1 says "16 bytes", which the file contradicts) and the R2013+ data-store bit in objects and entities. |
| `dwg/eed.rs` | Chapter 28 extended data (EED); APPID names (§20.4.66) | Yes. Real R14 to 2018 files settle two open points: 8-byte handles are most significant byte first, and the code page before an R13–R2004 string is too (00 1E for code page 30). Where the spec leaves a point open, the code does not guess silently: the byte order of the 8-byte handles in items 3 and 5 is not settled by the text (it says to read them "as usual for handles", the §2.13 order, but §2 also says all data is little-endian unless stated otherwise), so the reader tries both and keeps the order under which the drawing's 1003 and 1005 values point at existing layers and objects, and reports which. No string terminator is described, so strings are kept whole. 8-bit strings are only decoded in ANSI_1252; other code pages, and anything else that does not decode, are kept as bytes. |
| `dwg/entities.rs`, `tables.rs` | Chapter 20 per-type layouts | **No**, except: LTYPE (§20.4.58) and STYLE (§20.4.56), written on 2026-10-01 from the specification text in the CAD project; STYLE decodes the specification's own example (§20.4.56.1) to the values it shows. LTYPE's shape flag (DXF 74) is read with the DXF reference's meaning (2 text, 4 shape), which the specification's R2007 text-area rule also uses; its per-bit note gives the two the other way round. The R2000+ LAYER flag word (§20.4.54: frozen 1, on 2, frozen in new viewports 4, locked 8, plotting 16, lineweight 0x03E0) checked against the text. AutoCAD 2027's files leave bit 2 clear on every layer their DXF shows on, so the bit is read as *off*, the opposite of the text's name for it. Fields marked "uncertain" in the code are the likeliest to need fixes. **Found in a real AC1032 file, not in the specification:** from R2007 (at least in R2018) a table record stores no 64-flag and no xref-dependent bit, only the xref index; read with the two bits, every LAYER, APPID and BLOCK_HEADER in the file failed, and without them each ends exactly where its strings begin. APPID's undocumented RC (DXF 71) is there. The MI-201 corpus shows it in AC1024, AC1027 and AC1032 and not in AC1018; AC1021 is still to be read (TEST-PLAN.md §7). |
| `model/`, `geom/`, `render/`, `text.rs`, `standards.rs`, `units.rs`, `samples.rs` | Original design | Not format-specific. Lineweight codes (`model/lineweight.rs`): the standard weights from the DXF reference; that DWG stores an index follows from the five-bit layer field (§20.4.54); the index order and the special codes are from knowledge. Unit factors are the exact definitions (1 in = 25.4 mm, US survey foot = 1200/3937 m). Drafting values (ASME Y14.2 line proportions, ISO 128 line widths) are the standards' published numbers, used as design choices in DESIGN.md. |
| `font.rs` | The app's single-stroke font | Original: every glyph drawn for this project as coordinates on a 12-unit grid. No font file, Hershey data or SHX font was used or consulted. |
| `raster/` (rasteriser, `png.rs`) | Software rendering, PNG output | Original. PNG from the W3C PNG specification, compressed with `codec`; CRC-32 shared with the native format. Tests decode the output with the core's inflate. |
| `codec/` (`deflate.rs`, `inflate.rs`, `sha256.rs`, zlib in `mod.rs`) | DEFLATE both ways, zlib, SHA-256 | Original, from RFC 1951, RFC 1950 and FIPS 180-4. Test vectors for DEFLATE (a fixed and a dynamic-Huffman stream) and for SHA-256 at the padding boundaries were generated with Python's `zlib` and `hashlib`, used only to produce those bytes; FIPS 180-4's own examples are tested too. Every level's output is also decoded by the core's inflate, and was checked once against Python's zlib (all streams decode). |
| `takeoff.rs` | Lengths, contours, areas and weights of a part | Original. Shoelace areas, ray-casting containment, arc lengths from bulges; nominal material densities are standard reference values. |
| `export/dxf.rs` | DXF R12 group codes | From the public DXF reference. |
| `dxf/` (`encode.rs`, `write.rs`, `read.rs`) | AC1032 DXF, ASCII and binary, both directions | Group codes and value types from the public DXF reference ("Group Code Value Types", "About Binary DXF Files"); record order and optional groups as AutoCAD 2027's own DXF files show them. Binary value sizes the reference leaves open (booleans, binary chunks) from AutoCAD's binary DXF. |
| `dwg/styles.rs` | Visual styles, materials, table styles, multileader, section and detail view styles, dimension associations (DWG ↔ the DXF groups the model keeps) | MLEADERSTYLE and TABLESTYLE from the specification text (§20.4.87, §20.4.101); the others are not in the text, so their layouts come from AutoCAD 2027's MI-201 files: each record read field by field against the DXF AutoCAD wrote of the same drawing, and checked by tests that decode every one of the 33 records to AutoCAD's DXF groups and encode them back to AutoCAD's bytes. A material is handled only as AutoCAD's default materials are, whose records hold AutoCAD's default values between the map transforms that DXF gives. |
| `dwg/write/` (`objects.rs`, `sections.rs`, `container.rs`), `dwg/nongraphic.rs` | AC1032 DWG writing; non-graphical objects, VPORT and DIMSTYLE records | Chapters 4, 9, 10, 13, 14, 16, 18, 20–23 and 27 of the specification text, mirrored from the reader; every choice the text leaves open taken from AutoCAD 2027's MI-201 files and checked by tests that compare bytes. The application information names this program and makes no claim to be an Autodesk application. The bits AutoCAD writes after the header's handles, on which (with its own application information) its notice that a file was not saved by Autodesk software depends, are left out and not analysed: they mark AutoCAD's own saves and are not drawing data. |
| `dwg/lz77.rs` `compress()` | §4.7 compression (encoder side) | Original. Written to produce the opcode forms the §4.7 decoder reads, limited to the forms AutoCAD's own streams use as observed in its files (the `11 00 00` ending, no copy under four bytes, no far opcodes 0x17–0x1F); every test output is checked by decompressing it. |
| `native/` (`tags`, `schema`, `split`, `container`, `recovery`, `records`, `mod`, `lock`, `audit`), `examples/native_demo.rs`, `examples/native_bench.rs` | The native `.cad` format and `.cdl` locks | Original design, specified in `docs/FORMAT.md`. Borrows ideas, not code: group-code meanings, including extended data, from the public DXF reference; named, compressed, CRC-checked sections and a handle map from ODA spec chapter 4; splitting values into streams from the R2007+ object layout the spec describes (chapter 5); XOR parity as in RAID. No other file format implementation was consulted. The 1.1 test fixture (`tests/fixtures/sample-1.1.cad`) was written by this project's own 1.1 code. |
| `crates/cad-ffi` | Bridge to the platform apps: JNI entry points that pass file descriptors, and the WebAssembly entry points (`web.rs`) | Original. The only crate with `unsafe` (exporting the entry points, taking ownership of descriptors), with a `SAFETY` note on each block. |
| `xtask` | `cargo xtask android`: builds cad-ffi with the NDK's clang, checks ELF headers, copies to `jniLibs` | Original. NDK layout (`toolchains/llvm/prebuilt/<host>`, `source.properties`, `meta/platforms.json`) from the NDK itself and Android's documentation. |
| `export/display.rs` | Display list for app canvases | Original. Built on the core's own render, dash and font code. |
| `snap.rs` | Object snaps, picking, window and crossing selection | Original. Snap kinds and markers from DESIGN.md §9; intersections are textbook line/circle geometry; box clipping is the Liang–Barsky method (a published algorithm, written from the description). |
| `model/edit.rs` `erase_entity`, `lib.rs` `set_time`/`now_unix`, `raster` single-thread path | Erasing, an app-supplied clock, rendering without threads | Original. |
| `crates/cad-ffi/src/session.rs` | Open drawings, undo, the command language apps drive | Original. Command names and option letters follow COMMANDS.md. Mirroring keeps text readable, as CAD programs' MIRRTEXT 0 does (general knowledge). |
| `model/store.rs`, `model/edit.rs` (`Change`, `revert`, `ensure_model_space`, `ensure_layer_zero`), `render` (`entities_changed`, `update_model_space`, item boxes, block-cycle and visit limits), `export/display.rs` (`display_patch`), `dxf/read.rs` (files with no handles, tables or subclass markers) | Undo by recorded changes; patched scenes and displays; minimal DXF files | Original. The 2026-10-08 advisory (an outside black-box review of this build's own behaviour, stating that no third-party code or other program's output was used) described symptoms and required outcomes only; the designs here are this project's. That a DXF may hold only an ENTITIES section, without handles or subclass markers, is from the public DXF reference. |
| `crates/cad-ffi/tests/damaged_inputs.rs`, `crates/cad-ffi/examples/history_bench.rs` | Damaged-input test (seeded from the core's own files) and the edit-cost benchmark | Original. |
| `crates/cad-ffi/src/web.rs` | WebAssembly entry points | Original. Plain `extern "C"` exports over linear memory; no `wasm-bindgen` or other crate. |
| `apps/web` | The browser workbench (PWA): `index.html`, `style.css`, `app.js`, `core.js`, `icons.js`, `sw.js`, `manifest.webmanifest`, icons | Original. Layout and colours from the project's own workspace mockups (2026-09-30) and DESIGN.md; icons drawn for this project. Browser APIs (canvas, pointer events, service workers, the web app manifest, File System Access) from their public specifications and MDN-style reference knowledge. The point-entry grammar (`x,y`, `@dx,dy`, `@d<a`) and the command-line conventions are common CAD practice, as COMMANDS.md records. |
| `xtask/src/web.rs` | `cargo xtask web` / `serve`: build and assemble the workbench, a local web server | Original. HTTP/1.1 from its public specification; the WebAssembly import-section check from the WebAssembly core specification. |
| `apps/android` | CAD Viewer, the Android test app | Original, Java, Android platform APIs only. Build files follow the documented Android Gradle plugin setup. |
| `tools/compaction_lab.py`, `examples/lz77_segments.rs` | Measurements of record layouts and codecs (`FORMAT-ROADMAP.md` §5.2) | Original. The lab calls DEFLATE and LZMA from Python's standard library to measure what an entropy-coding codec would add; they are yardsticks only, not part of the build, and no code from them is used. |

## The Rust standard library for WebAssembly

The workbench is built with Rust's own standard library for
`wasm32-unknown-unknown`, as every Rust program for that target is. It is
part of the Rust toolchain (MIT or Apache-2.0, like the library on every
other target), not a dependency of this project. Normally it comes with
`rustup target add wasm32-unknown-unknown`. On 2026-10-08 the build
workspace could not reach Rust's download server, so the same library was
built there from the Rust 1.95.0 sources (`library/` from
github.com/rust-lang/rust at tag 1.95.0, plus the crates that library
itself uses at the versions its `Cargo.lock` pins: cfg-if, libc,
hashbrown, rustc-demangle, dlmalloc, backtrace). None of those sources are
in this repository, none of this project's code was taken from them, and
a build on any machine with normal internet access uses rustup's copy.

## Third-party code considered and rejected

**acadrust** (crates.io, MPL-2.0, a Rust DWG/DXF reader and writer) was
evaluated on 2026-09-28 as a possible base or dependency for DWG reading.
It was **not adopted**, and none of its code was read. To check its
lineage, the repository was cloned to a scratch folder and searched for
the words "LibreDWG" and "ACadSharp"; only the matching comment lines and
file names were viewed, then the clone was deleted. Findings:

- Its source comments state that field layouts "follow LibreDWG
  `dwg2.spec`" / "`dwg.spec`" in several object readers and writers, and
  its Reed-Solomon tests embed tables copied from LibreDWG's
  `reedsolomon.c`. LibreDWG is GPLv3.
- Its README names ACadSharp (MIT) as an inspiration.
- It depends on about 15 third-party crates.

Using it would bring a possible GPL lineage question and third-party
dependencies into a project whose charter is to avoid both. Revisit only
with an IP attorney's opinion. It may still serve later as a black-box
comparison tool (run its binary on our own test files and compare output,
without reading its source), recorded here if that happens.

## A caveat about how the code was written

The code was written by an AI model (Anthropic's Claude) working for the
project owner. The model was trained on large amounts of public text and
code, which almost certainly included open-source DWG readers such as
LibreDWG. The model did not look at any such code while writing this
project, and the code was written from the specification, in a different
language and with its own structure. Still, the model cannot certify a
strict clean room the way a team that never saw other implementations
could.

Two things help here:

- The per-module table above ties each piece to specification sections.
- The work to verify chapters 9, 10, 20 and 23 against the specification
  text (below) makes the specification, not memory, the recorded source
  for those parts.

## Open items

1. Check chapters 9, 10, 20 and 23 line by line against `header.rs`,
   `classes.rs`, `objmap.rs`, `object.rs`, `streams.rs`, `entities.rs` and
   `tables.rs`, now that the full spec text is available. Note each
   correction here.
2. Build a test corpus from the owner's own fictional parts (never customer
   drawings). See `TEST-PLAN.md`. The first real file is checked (§7
   there); the same drawing saved as 2000, 2004, 2010 and 2013 would settle
   where the table-record change begins.
3. Before release, have counsel review this record, the ODA
   specification's terms, and naming (for example, use of "DWG" in product
   names and marketing).
4. Decide whether to publish `docs/FORMAT.md`. Publishing it does not
   affect the clean-room record; it only changes whether others may
   implement readers.

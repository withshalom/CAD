# BlackCAD — clean-room CAD software with its own `.blc` format and DWG fluency (draft)

*The program's name lives in this title, in one constant in the core
(`cad_core::PRODUCT_NAME`, the name it writes into DWG files as the program
that saved them) and in the web app's page, manifest and window title
(`apps/web`); the single-file build is named for it too (`xtask`).
Elsewhere in these docs **CAD** means the program and **`.blc`** its file format.*

A clean-room CAD engine written from scratch in Rust. Drawings are saved
in the project's own **native `.blc` format** (binary-DXF-style records in
a DWG-style compressed, sectioned container, with incremental saves and
`.bdl` edit locks). It opens **.dwg** files and AutoCAD 2018 **DXF**
(ASCII and binary) directly, and writes DXF (ASCII and binary) and
AutoCAD 2018 (AC1032) .dwg files. The core has **no
third-party dependencies** (only the Rust standard library) and no `unsafe`
code, so it carries no outside license terms and builds for every target
Rust supports: Windows, macOS, Linux, Android and iOS. A small Android test
app already runs on it.

This is the **first draft of the core**. It compiles, its unit tests pass,
and it rejects malformed input without crashing. It is checked against
**real DWG files**: a corpus of one part saved by AutoCAD in every version
from R14 to 2018 (`corpus/`, tested by `cargo test`), which it reads with no
failed objects and matches entity for entity against AutoCAD's own DXFs,
except 2007 (AC1021), whose container is not implemented yet.
See [Status](#status).

## What's here

```
crates/cad-model/    the drawing database, geometry and text code pages      (MIT OR Apache-2.0)
crates/cad-dwg/      the DWG reader and writer                                (MIT OR Apache-2.0)
crates/cad-dxf/      the DXF reader and writer                                (MIT OR Apache-2.0)
crates/cad-native/   the native .blc format, and its DEFLATE and SHA-256 code (MIT OR Apache-2.0)
crates/cad-core/     the core library on those four: drafting standards, editing commands,
                     units, font, rendering, raster and vector output
crates/cad-ffi/      bridge to the platform apps: the session (open drawings, undo, commands),
                     WebAssembly entry points for the browser, JNI entry points for Android
apps/web/            the BlackCAD workbench as a browser app (PWA), installable on desktop and Android;
                     published at https://cad.mosaiciron.com/ (apps/web/README.md)
apps/cad-cli/        `cad`, a command-line test build
apps/android/        CAD Viewer, an Android test app (Java, Android's own APIs only)
xtask/               `cargo xtask web`/`serve`: builds and serves the browser app;
                     `cargo xtask android`: builds the core for the Android app
docs/                ARCHITECTURE, DESIGN (look and behaviour), COMMANDS (commands and shortcuts), FORMAT (.blc spec), FORMAT-ROADMAP, PROVENANCE (clean-room record), TEST-PLAN;
                     blackboard/ (the lessons' curriculum and sources), reviews/ (outside reviews)
corpus/              real drawings (DWG with AutoCAD's DXF of each) that `cargo test` reads and checks
scripts/             build helpers for Linux and Windows
tools/               compaction_lab.py: format-layout measurements; render_icons.py: the web app's PNG
                     icons (Python, not part of the build)
.github/workflows/   CI: builds and tests on Linux, Windows and macOS, builds the Android APK and the web app,
                     and publishes the web app from main (the gh-pages branch)
```

## Build

Install Rust from <https://rustup.rs>: Rust 1.87 or later, since the
workspace uses the 2024 edition and declares `rust-version = "1.87"`.
Nothing else is downloaded: the project has no dependencies. Then, from
this folder:

| Platform | Command | Output |
|---|---|---|
| Linux | `scripts/build-linux.sh` (or `cargo build --release`) | `dist/linux-x86_64/cad` |
| Windows | `scripts\build-windows.ps1` (or `cargo build --release`) | `dist\windows-x86_64\cad.exe` |
| macOS | `scripts/build-linux.sh` | `dist/darwin-arm64/cad` |

The scripts run the tests first (`cargo test --release`), then build.

**Windows needs a linker.** Either:
- keep rustup's default MSVC toolchain, for which it asks for the Visual
  Studio Build Tools ("Desktop development with C++"), a large download; or
- use the GNU toolchain instead: in `rustup-init.exe` choose to customize
  the installation and set the default host to `x86_64-pc-windows-gnu`
  (or run `rustup-init.exe --default-host x86_64-pc-windows-gnu`). The
  rustup book says basic use needs no other software, and with no
  dependencies that is all this project needs.

**One Linux file that runs anywhere.** `cargo build --release` links
against the build machine's glibc. A statically linked binary runs on any
64-bit Linux:

```
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu
# → target/x86_64-unknown-linux-gnu/release/cad
```

**Without installing anything:** push this folder to a GitHub repository.
`.github/workflows/build.yml` lints, tests and builds on Linux, Windows
and macOS on every push, and the binaries are downloadable from the run.

## The workspace in a browser (PWA)

`apps/web` is the program's GUI: the drafting workspace from the design
mockups (tool rail, model space, properties, layers, command line, status
bar; on phones the title bar, aid chips and tool sheet), with the
Blackboard of drafting and shop lessons beside it. It runs the whole core
in the browser as WebAssembly, so it opens and saves DWG, DXF and `.blc`
itself, with no server. It is a Progressive Web App: open it once and it
can be installed like any program, and it keeps working offline.
`apps/web/README.md` describes its files and how the page talks to the
core.

**Use it now:** <https://cad.mosaiciron.com/>, built by the workflow from
the latest `main` (below), or the single file
<https://cad.mosaiciron.com/BlackCAD.html>.

**Run it on this computer:**

```
rustup target add wasm32-unknown-unknown    # once
cargo xtask serve                           # builds, then serves http://127.0.0.1:8080/
```

Open <http://127.0.0.1:8080/> in Chrome or Edge. To install it, use the
install button in the address bar (or "Install app" in the top bar):
it then has its own window, Start menu or app-launcher entry, and can
open `.dwg`, `.dxf` and `.blc` files from the file manager.

**On an Android phone,** with the phone plugged in and USB debugging on:
`adb reverse tcp:8080 tcp:8080`, then open <http://localhost:8080/> in
Chrome on the phone and choose "Install app" (or "Add to Home screen").
Browsers install apps and run their service workers only from
`localhost` or HTTPS, which is why the port is forwarded rather than
opened by the computer's network address.

**How it is published:** the workflow's `web` job builds the core for
WebAssembly and puts the app together on every push, then checks the core
through the page's own glue (`apps/web/tests`); every run keeps the app
(artifact `blackcad-web`) and the single file (`BlackCAD.html`). When every
job has passed on `main`, the `pages` job replaces the `gh-pages` branch
with that build (one commit, with `CNAME` and `.nojekyll`), and GitHub
Pages serves the branch at the custom domain (Settings → Pages: deploy from
the branch `gh-pages`, folder `/`). A site published this way is public,
even from a private repository.

**Host it** anywhere else that serves static files over HTTPS: `cargo
xtask web` puts the finished app in `dist/web/`, and those files are all
there is.

**One file for testing:** `cargo xtask web` also writes
`dist/BlackCAD.html`, the whole workbench in a single file (about 4 MB:
scripts, styles and the core inlined, the core as base64). Open it by
double-clicking, or send it to a phone and open it in the browser; it
needs no server. It runs the same code as the app, but has no service
worker, so it cannot be installed and does not open DWG files from the
file manager. The start screen says "single-file test build", and the
window title "(test build)". The newest one is on the `gh-pages` branch.

**What it does:** opens and saves `.blc`, DWG 2018 and ASCII and binary
DXF, and exports SVG and PDF. Its commands (typed, with AutoCAD's usual
aliases, or picked from the tools): LINE, PLINE, CIRCLE, ARC, RECTANGLE,
TEXT, MTEXT, XLINE, RAY, HATCH; DIM (one command for every kind),
DIMLINEAR, DIMALIGNED, DIMRADIUS, DIMDIAMETER, DIMANGULAR, DIMBASELINE,
DIMCONTINUE, LEADER, QLEADER, MLEADER; MOVE, COPY, ROTATE, MIRROR, SCALE,
STRETCH, OFFSET, TRIM, EXTEND, FILLET, CHAMFER, ARRAY (rows and columns,
or polar), EXPLODE, ERASE; BLOCK, INSERT, ATTDEF, ATTEDIT; UNITS, LINETYPE,
LTSCALE, LWEIGHT, COLOR; LAYOUT, MVIEW, PAGESETUP, PLOT; DIST, ID, ZOOM,
PAN, undo and redo. Object snaps, ORTHO, POLAR, grid and grid stepping;
window and crossing selection, grips that edit by dragging; values edited
in place in the properties panel (lengths in any unit: `1/2"`, `10mm`);
layers: current, on/off, lock, colour, new. Typed points follow
COMMANDS.md §17 (`x,y`, `@dx,dy`, `@len<angle`, a length along the
cursor). The Blackboard (BOARD, F1) teaches drafting, shop practice and
electronics on figures drawn from live numbers, and SHOPCHECK checks a
drawing's holes against the shop's rules. A sample drawing (the P-1042
plate from the mockups) is on the start screen.

## Android test viewer

`apps/android` is CAD Viewer, a small Android app for trying the core on a
phone or tablet. Open a `.blc` or DWG file from the file picker, or with
"Open with" from another app, and it shows a short summary and model space,
with pinch zoom. It is written in Java against Android's own APIs, with no
libraries, so the APK contains no third-party code. All the drawing work
happens in the core, through `crates/cad-ffi`.

**Easiest: GitHub.** The `android` job in the workflow builds a debug APK
on every push. Download `cad-viewer-debug-apk` from the run.

**On your own computer:**

1. Install [Android Studio](https://developer.android.com/studio). In its
   SDK Manager, add SDK Platform 36 and, under SDK Tools, "NDK (Side by
   side)", version r28 or later.
2. Add Rust's Android targets:
   `rustup target add aarch64-linux-android x86_64-linux-android`
3. From this folder, run `cargo xtask android`. It finds the NDK, builds
   `libcad_ffi.so` for phones (arm64-v8a) and the emulator (x86_64),
   checks it, and puts it where the app picks it up. `--abi all` adds older
   32-bit phones (add the `armv7-linux-androideabi` target first).
4. Open `apps/android` in Android Studio and press Run, or build with
   Gradle 9.6 or later: `gradle -p apps/android assembleDebug`, which writes
   `apps/android/app/build/outputs/apk/debug/app-debug.apk`.

**Install it:** turn on USB debugging on the phone (on most phones:
Settings → About phone, tap "Build number" seven times, then Developer
options → USB debugging), connect it, and run `adb install -r app-debug.apk`.
Or copy the APK to the phone and open it there. Each GitHub run signs the
APK with a new debug key, so before installing one over an earlier build,
uninstall the old one (`adb uninstall com.withshalom.cad`). Details, and
what a release build would need, are in `apps/android/README.md`.

## Use the test build

```
cad info    part.dwg                 # version, sections, object counts, diagnostics
cad objects part.dwg --type LINE     # list objects with a one-line summary
cad objects part.dwg --failed        # only objects that failed to decode
cad dump    part.dwg 2F              # one object in full (handle in hex)
cad header  part.dwg                 # header variables as read
cad svg     part.dwg part.svg        # render model space (--light: black on white paper)
cad png     part.dwg part.png        # draw it as the canvas shows it (--light [--color],
                                     #   --no-grid, --width N, --height N)
cad dxf     part.blc part.dxf        # AutoCAD 2018 DXF of the whole drawing (--binary for binary DXF)
cad dxf     part.dwg part.dxf --r12  # model space as R12 DXF, blocks exploded
cad dwg     part.blc part.dwg        # AutoCAD 2018 (AC1032) .dwg file
cad layers  part.dwg                 # layers: colour, linetype, lineweight, state, use

cad new     part.blc [--imperial]    # a new drawing from the Metric or Imperial template
cad sample  plate.blc                # the sample plate, drawn on the default layers
cad length  "1'-2 1/2\"" --unit mm   # read a typed length and show it
                                     #   (--format decimal|fractional|architectural|…)

cad save    part.dwg part.blc        # save as native .blc (from .dwg, .dxf or .blc)
cad verify  part.dwg                 # .blc round trip in memory; reports any difference
cad info    part.blc                 # every command above also takes .blc files
cad compact part.blc                 # fold incremental change sets into a new snapshot
cad inspect part.blc --tags 2        # byte map, structure checks (audit), tag dump
cad preview part.blc thumb.svg       # the preview stored in the file
cad presence part.blc                # who has it open (.bdl lock), and whether it is stale
cad props   part.blc --set title="Bracket" --set author=Shalom
                                     # drawing properties, saved as a change set
cad part    part.blc --set material=Steel --set thickness=6mm --set quantity=10
cad takeoff part.blc                 # length, contours, holes, net area, weight (Proposed)
cad schema                           # the record schema every .blc file carries
```

## Use the core from Rust

```rust
let drawing = cad_core::open("part.dwg")?;          // .dwg, .dxf or .blc, detected by content
println!("{}: {} objects", drawing.version, drawing.objects.len());
cad_core::save(&drawing, "part.blc")?;              // native format, atomic replace

let scene = cad_core::render::model_space(&drawing);
let svg = cad_core::export::svg::to_svg(&scene, &Default::default());
let dxf = cad_core::dxf::to_bytes(&drawing, cad_core::dxf::DxfFormat::Binary).bytes;
let dwg = cad_core::dwg::write::to_bytes(&drawing)?.bytes; // AC1032
let png = cad_core::raster::render(&scene, &Default::default()).to_png();

// A new drawing on the default layers, with something on it.
use cad_core::model::{Circle, ObjectKind};
let mut d = cad_core::new_drawing(cad_core::Template::Metric);
let visible = d.layer_by_name("VISIBLE").unwrap().0;
d.add_entity(visible, ObjectKind::Circle(Circle::new(Default::default(), 6.5)))?;
let hidden = d.layer_by_name("HIDDEN").unwrap().0;
d.set_layer_lineweight(hidden, cad_core::model::Lineweight::from_mm(0.35))?;
let mm = cad_core::units::parse_length("3/8\"", cad_core::units::Unit::of(&d))?; // 9.525
```

## Status

**Reads (draft; checked against real files from R14 to 2018, not 2007:
TEST-PLAN.md §7):**

- Containers: R13, R14, R2000 (AC1012–AC1015) and the R2004 family: 2004,
  2010, 2013, 2018+ (AC1018, AC1024, AC1027, AC1032). AC1032 is still
  AutoCAD's current format.
- Objects: LINE, CIRCLE, ARC, POINT, ELLIPSE, SPLINE, LWPOLYLINE, POLYLINE
  2D/3D/polyface with VERTEX, TEXT, MTEXT, ATTRIB/ATTDEF, INSERT/MINSERT,
  SOLID/TRACE, 3DFACE, all seven DIMENSION types, HATCH, BLOCK/ENDBLK/SEQEND,
  BLOCK_HEADER, LAYER, LTYPE (with its pattern), STYLE, table control
  objects and the names of other table records. Everything else is kept
  with its handle, type and owner and reported as "not decoded yet".
- Header variables, cross-checked against the object map before they're
  trusted.
- Non-graphical objects (dictionaries, extension records, layouts,
  scales, multiline styles, placeholders, dictionary variables) and the
  VPORT and DIMSTYLE records whole.
- DXF, ASCII and binary (AC1015 and later): everything the DXF writer
  writes reads back into the same drawing, so DXF → `.blc` → DXF gives the
  same bytes (TEST-PLAN.md §7).

**Writes:**

- DXF, ASCII and binary, AutoCAD 2018 (AC1032): every table, block,
  entity and the objects above, as AutoCAD lays them out. Binary DXF is
  checked against a binary DXF AutoCAD 2027 saved (corpus): it reads to
  its last byte and writes back byte for byte.
- DWG, AutoCAD 2018 (AC1032): objects encoded bit for bit as AutoCAD 2027
  encodes them (every object of the corpus drawing the model holds whole;
  MTEXT extents are measured with this program's fonts), the header,
  classes and object map, and the paged R2004-family container with its
  checksums, which rebuilds AutoCAD's own files byte for byte from their
  pages. AutoCAD 2027 opens the files written and shows the drawing
  correctly, with its notice that the file was not saved by Autodesk
  software.
  The small sections, the style objects (visual styles, materials, table
  and multileader styles, view styles) and the header follow AutoCAD's
  layout; what still differs from AutoCAD's own save is the
  authentication data AutoCAD adds to its saves, the order of objects in
  the file and an approximate count. Objects the model does not decode
  are written back as their original bytes when the drawing came from a
  2018 DWG.

**Drafting (DESIGN.md, Stage 1):** new drawings from Metric and Imperial
templates with the default layers (`0`, `VISIBLE`, `CENTER`, `HIDDEN`,
`DIMENSION`, `NOTES`), every one customizable; `CENTER` and `HIDDEN`
linetypes, centred on each feature; lineweights drawn 1–3 pixels; units
(typed `10mm`, `3/8"`, `1'-2 1/2"`; decimal, fractional, architectural,
engineering and scientific display); dimension and text styles; the app's
own single-stroke font. Tests compare the code with DESIGN.md's tables.

**Renders and exports:** model space in plan view, with blocks, nested
blocks, MINSERT arrays, dimension blocks (and dimensions drawn from their
style when they have none), mirrored/rotated object coordinate systems,
layer visibility and ByLayer/ByBlock colours, linetypes and lineweights.
Arcs stay true arcs where the transform allows. Output: PNG from the
core's own multi-core rasteriser (the dark canvas, or paper), SVG, and R12
DXF with linetypes.

**Native format (`.blc` 2.3, spec in `docs/FORMAT.md`):** full save and
load of everything the model holds, lossless (tested on every object
kind); records that leave out default values and a schema in every file
that describes them; sections split into streams and compressed with the
core's own DEFLATE, on every core, with CRC-32 per section; object map
with 64 KiB pages so one object can be read without loading the rest
(`NativeFile`); incremental saves as appended change sets that survive a
crash at any point; two header copies and two directory copies; a
SHA-256 hash chain over the snapshot and every change set; recovery data
that rebuilds damaged bytes; drawing properties; objects the core cannot
decode yet keep their original DWG bytes; extended data (XDATA) with
DXF's group codes, decoded from DWG; fields and object kinds from newer
versions of the format kept and written back unchanged; SVG preview;
`.bdl` lock files with heartbeat and stale-lock takeover (the `.dwl`
equivalent); a structural audit (`cad inspect`). Files of format 1.x
still open. See `cargo run -p cad-core --example native_demo`,
and `--example native_bench` for size and speed on 200,000 entities
(results and roadmap in `docs/FORMAT-ROADMAP.md`).

**Not yet:**

- Real-file validation. This is the next step; see `docs/TEST-PLAN.md`.
- AC1021 (AutoCAD 2007–2009) container. Those files are refused with a
  clear message.
- Writing DWG versions other than 2018; writing visual styles, materials
  and table/leader styles from the model. Password-protected drawings and
  signatures (the format reserves the flags). Paper-space drafting.
- Apps: the Android viewer only views; saving from it needs the storage
  design in `docs/ARCHITECTURE.md`. No desktop or iOS app yet.
- Unmodelled fields of decoded DWG objects are not kept in `.blc` yet.
  Hatch patterns (outlines only), TrueType fonts, angular and ordinate
  dimensions without a block, dimension styles from DWG.
- Snaps, tracking, ortho and polar (DESIGN.md §9): next.

## License

BlackCAD is proprietary (see [`LICENSE`](LICENSE)), except the drawing
database and the file formats: the crates [`cad-model`](crates/cad-model),
[`cad-dwg`](crates/cad-dwg), [`cad-dxf`](crates/cad-dxf) and
[`cad-native`](crates/cad-native) are licensed under either of the MIT
License or the Apache License, Version 2.0, at your option. Other programs
can use them to read and write DWG, DXF and `.blc` without the rest of
BlackCAD.

## Working on it

How changes reach `main` (branches, commits, pull requests, releases) is
in [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Clean-room record

`docs/PROVENANCE.md` records what each part was built from, what was
deliberately not consulted, and what still needs checking against the
specification. Keep it current as the code changes; it's the evidence for
the clean-room claim.

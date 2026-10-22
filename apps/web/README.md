# BlackCAD in the browser (`apps/web`)

The BlackCAD workbench as a web app: the drafting workspace (tool rail,
model space, properties, layers, command line, status bar; on phones the
title bar, aid chips and tool sheet) and the Blackboard beside it. The
whole Rust core runs in the page as WebAssembly, so the app opens and saves
DWG, DXF and `.cad` itself, with no server: the files here are the entire
front end, and `core.wasm` (built from `crates/cad-ffi`) is the rest.

Published from `main` at <https://cad.mosaiciron.com/> (see
[Publishing](#publishing)).

## The files

| File | What it is |
|---|---|
| `index.html` | The page: every panel's markup, and the Blackboard's frame. Loads `style.css` and `app.js`. |
| `style.css` | The look: colour tokens on `:root`, desktop and phone layouts, the Blackboard's chalk. |
| `app.js` | The shell: boots the core, draws its display list on the canvas, routes pointer, touch and keyboard input, runs commands as prompt sequences (as a CAD command line does), and drives the panels and files. Its last section is the Blackboard: lessons in drafting, shop practice and electronics, each figure drawn from the numbers its formulas use. Sections are marked `// ----`. |
| `core.js` | The only glue to the core: loads `core.wasm`, wraps its entry points (`crates/cad-ffi/src/web.rs`) and reads its display list. Restarts the core after a crash. |
| `icons.js` | The tool and button icons, drawn as SVG paths for this project. |
| `sw.js` | The service worker: caches the app so it works offline after one visit; the build stamps its cache name. |
| `manifest.webmanifest` | Makes the app installable, and lets the installed app open `.dwg`, `.dxf` and `.cad` files from the file manager. |
| `icons/icon.svg` | The app icon. `icon-192.png`, `icon-512.png` and `icon-maskable-512.png` are renderings of it (`tools/render_icons.py`). |
| `tests/core.test.mjs` | Checks a built core through `core.js` in Node, as the page uses it (see [Tests](#tests)). Not shipped. |

The page has no build step of its own and no dependencies: plain HTML, CSS
and JavaScript modules, written for this project. `app.js` imports only
`core.js` and `icons.js`.

## How the page talks to the core

`core.wasm` imports nothing and exports plain functions; only numbers
cross between them. Bytes go in through memory the page asks for
(`cad_alloc`, `cad_free`); whatever a call produces (JSON, a display list,
a file) waits in one result buffer (`cad_result_ptr`, `cad_result_len`).
Calls that can fail return 0 and leave the message in that buffer.

| Entry point | Used for |
|---|---|
| `cad_abi_version`, `cad_version` | Checking the module is the one `core.js` expects (ABI 1), and the version on the start screen |
| `cad_new`, `cad_sample`, `cad_open`, `cad_close` | Drawings: a new one, the sample plate, a file's bytes |
| `cad_info` | Units, layers, undo state and the summary, as JSON |
| `cad_display`, `cad_display_changes` | Model or paper space as a display list (`crates/cad-core/src/export/display.rs`), whole or as a patch of what changed |
| `cad_exec` | Every command and query, as one line of text (`crates/cad-ffi/src/session.rs` lists them), answered with JSON |
| `cad_save`, `cad_save_notes` | A file in any format (`cad`, `dwg`, `dxf`, `dxfb`, `svg`, `pdf`), and what the save left out |
| `cad_heap_bytes`, `cad_panic_message` | The memory warning, and the message when the core stops on a bug |

## Build and run

You need Rust (<https://rustup.rs>) with the WebAssembly target:

```
rustup target add wasm32-unknown-unknown    # once
cargo xtask serve                           # builds, then serves http://127.0.0.1:8080/
```

`cargo xtask web` builds without serving. Either one writes:

- `dist/web/`: the app as it is published, these files plus `core.wasm`;
- `dist/BlackCAD.html`: the same app in one file (styles, scripts and the
  core inlined, the core as base64), for testing by opening it from disk.
  It has no service worker, so it does not install or work offline, and it
  says "single-file test build" on its start screen.

Working on the page without the WebAssembly target installed: take a
`core.wasm` that CI built (the `gh-pages` branch has the latest from
`main`) and run `cargo xtask serve --core path/to/core.wasm`.

`serve` serves the assembled copy in `dist/web/`, so after editing these
files run it again; `--core dist/web/core.wasm` reuses the core already
built instead of compiling it again.

## Publishing

The workflow's `web` job builds the app on every push. After every job
passes on `main`, the `pages` job puts `dist/web/` and `BlackCAD.html` on
the `gh-pages` branch (one commit, replaced each time), which GitHub Pages
serves at the custom domain. The newest single-file build is therefore
always at `BlackCAD.html` on that branch, and at
<https://cad.mosaiciron.com/BlackCAD.html>. Every run also keeps both as
artifacts (`blackcad-web`, `BlackCAD.html`).

## Tests

- `cargo test -p xtask` assembles the single file from these files and
  fails if `index.html` no longer has what the assembly replaces.
- `tests/core.test.mjs`, run by CI after the WebAssembly build:
  `node apps/web/tests/core.test.mjs dist/web/core.wasm corpus/mosaiciron-mi201`
  loads the core through `core.js` and checks its version against
  `Cargo.toml`, new drawings, the sample, the drawing and editing commands
  with undo and redo, the queries the page makes, saving in every format
  and reading back, the corpus, and damaged input.
- Sessions in a browser (pointer, keyboard, phone layout) are recorded in
  `docs/TEST-PLAN.md`.

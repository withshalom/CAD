// CAD workspace: the browser shell around the core (core.js).
//
// Laid out as the workspace mockups show it (desktop: tool rail, model
// space, properties/layers/drawing panel, command line, status bar; phone:
// title bar, aid chips, drawing, bottom sheet) and behaving as
// docs/DESIGN.md §2–§11 and docs/COMMANDS.md describe. Every drawing
// operation, snap, measurement and file format is the core's; this file
// draws the display list, routes input, and runs commands as generators
// that ask for points, lengths and text the way a CAD command line does.

import { Core, CoreCrash, opLength } from "./core.js";
import { icon } from "./icons.js";

// ---------------------------------------------------------------- constants

const COLORS = {
  space: "#0d1013",
  gridMinor: "#151a1f", // DESIGN.md §2
  gridMajor: "#1d242b",
  select: "#3d8bff", // DESIGN.md §5
  snap: "#ff4fd8",
  rubber: "rgba(230, 237, 243, .85)",
  cross: "rgba(160, 172, 184, .45)",
  ucs: "rgba(205, 214, 222, .8)",
};
// On a layout the sheet is white paper on grey: lines in their printed
// colours, and the rubber band, crosshairs and axes dark.
const DARK = { ...COLORS };
const PAPER = { ...COLORS, rubber: "rgba(25, 32, 40, .85)", cross: "rgba(40, 50, 60, .55)", ucs: "rgba(40, 50, 60, .75)", space: "#3a4048" };
const onPaper = () => !!doc?.info?.paper;
const APERTURE = { mouse: 10, touch: 22 }; // px, DESIGN.md §9
const GRIP = 8; // px, DESIGN.md §5
const GRID_SPACING = 10; // drawing units (standards::canvas::GRID_SPACING)
const SNAP_KINDS = { END: 1, MID: 2, CEN: 4, QUA: 8, INT: 16, NEA: 32, NOD: 64, INS: 128 };
const SNAP_DEFAULT = 1 | 2 | 4 | 8 | 16 | 64 | 128;
const POLAR_STEP = 15; // degrees
const TAU = Math.PI * 2;
const PHONE = matchMedia("(max-width: 760px)");
// The single-file test page (`cargo xtask web`: dist/BlackCAD.html) carries
// the core inline; it says so on the start screen and in the window title.
const SINGLE_FILE = typeof globalThis.CAD_CORE_WASM === "string";
const APP_TITLE = SINGLE_FILE ? "BlackCAD (test build)" : "BlackCAD";

// ---------------------------------------------------------------- elements

const $ = (s) => document.querySelector(s);
const app = $("#app");
const canvas = $("#canvas");
const ctx = canvas.getContext("2d");
const cmdInput = $("#cmd-input");

// ---------------------------------------------------------------- state

let core = null;
const docs = []; // {id, name, format, fileHandle, orig, imperial, disp, info, view}
let doc = null;
const selection = new Set();
let grips = []; // [{x, y, handle, kind, field}]
let hover = null;
const aids = loadAids();
const cursor = { sx: 0, sy: 0, wx: 0, wy: 0, inside: false, touch: false, snap: null, snaps: [], snapIndex: 0 };
let runner = null;
let lastCommand = null;
let pointerMode = null;
const pointers = new Map();
let needsDraw = true;
const recall = [];
let recallAt = -1;
let suggestAt = -1;
let editingField = null; // the properties row being edited

// ---------------------------------------------------------------- boot

async function boot() {
  buildChrome();
  bindInput();
  try {
    core = await Core.load("core.wasm");
    $("#welcome-version").textContent = core.version.replace(/^\S+\s+/, "") + (SINGLE_FILE ? " · single-file test build" : "");
  } catch (e) {
    $("#welcome-version").textContent = `The core could not be loaded: ${e.message}`;
    document.querySelectorAll(".welcome-actions button").forEach((b) => (b.disabled = true));
    return;
  }
  // The single file installs nothing, even when served beside the app.
  if (!SINGLE_FILE && "serviceWorker" in navigator && location.protocol.startsWith("http")) {
    navigator.serviceWorker.register("sw.js").catch(() => {});
  }
  if ("launchQueue" in window) {
    // Files opened with the installed app (manifest file_handlers).
    window.launchQueue.setConsumer(async (p) => {
      for (const h of p.files || []) await openFile(await h.getFile(), h);
    });
  }
  setPrompt();
  renderAll();
  frame();
}

function loadAids() {
  const d = { grid: true, snap: false, ortho: false, polar: true, osnap: true, dyn: true };
  try {
    return { ...d, ...JSON.parse(localStorage.getItem("cad.aids") || "{}") };
  } catch (_) {
    return d;
  }
}
function saveAids() {
  try {
    localStorage.setItem("cad.aids", JSON.stringify(aids));
  } catch (_) {}
}

// ---------------------------------------------------------------- chrome

// The rail as in the mockup: select · draw · annotate · modify.
const RAIL = [
  ["select", "Select", "SELECT"],
  "-",
  ["line", "Line (L)", "LINE"],
  ["pline", "Polyline (P)", "PLINE"],
  ["circle", "Circle (C)", "CIRCLE"],
  ["arc", "Arc (A)", "ARC"],
  ["rect", "Rectangle (R)", "RECTANGLE"],
  ["text", "Text (TE)", "TEXT"],
  "-",
  ["hatch", "Hatch (H)", "HATCH"],
  "-",
  ["dim", "Dimension (D)", "DIM"],
  ["leader", "Leader (LE)", "LEADER"],
  ["measure", "Measure (DI)", "DIST"],
  "-",
  ["move", "Move (M)", "MOVE"],
  ["copy", "Copy (CO)", "COPY"],
  ["rotate", "Rotate (RO)", "ROTATE"],
  ["mirror", "Mirror (MI)", "MIRROR"],
  ["scale", "Scale (SC)", "SCALE"],
  "-",
  ["offset", "Offset (O)", "OFFSET"],
  ["trim", "Trim (T)", "TRIM"],
  ["fillet", "Fillet (F)", "FILLET"],
  ["array", "Array (AR)", "ARRAY"],
  ["erase", "Erase (E, Delete)", "ERASE"],
];
// The phone's tool strip, with labels.
const STRIP = [
  ["select", "Select", "SELECT"],
  ["line", "Line", "LINE"],
  ["pline", "Pline", "PLINE"],
  ["circle", "Circle", "CIRCLE"],
  ["arc", "Arc", "ARC"],
  ["dim", "Dim", "DIM"],
  ["rect", "Rect", "RECTANGLE"],
  ["text", "Text", "TEXT"],
  ["move", "Move", "MOVE"],
  ["copy", "Copy", "COPY"],
  ["rotate", "Rotate", "ROTATE"],
  ["mirror", "Mirror", "MIRROR"],
  ["erase", "Erase", "ERASE"],
  ["offset", "Offset", "OFFSET"],
  ["trim", "Trim", "TRIM"],
  ["fillet", "Fillet", "FILLET"],
  ["scale", "Scale", "SCALE"],
  ["array", "Array", "ARRAY"],
  ["hatch", "Hatch", "HATCH"],
  ["leader", "Leader", "LEADER"],
  ["measure", "Measure", "DIST"],
  ["keyboard", "Type", "TYPE"],
];

function toolClick(cmd) {
  if (cmd === "SELECT") return cancelCommand(true);
  if (cmd === "TYPE") {
    app.classList.toggle("typing");
    if (app.classList.contains("typing")) cmdInput.focus();
    return;
  }
  startCommand(cmd);
}

function buildChrome() {
  $("#mark").innerHTML = icon("mark");
  $("#wmark").innerHTML = icon("mark");
  $("#new-btn").innerHTML = icon("plus");
  $("#undo-btn").innerHTML = icon("undo");
  $("#redo-btn").innerHTML = icon("redo");
  $("#pmenu").innerHTML = icon("menu");
  $("#pundo").innerHTML = icon("undo");
  $("#predo").innerHTML = icon("redo");
  $("#players").innerHTML = icon("layers");
  $("#layer-add").innerHTML = icon("plus");

  const rail = $("#rail");
  for (const t of RAIL) {
    if (t === "-") {
      rail.insertAdjacentHTML("beforeend", '<div class="sep"></div>');
      continue;
    }
    const [ic, title, cmd] = t;
    const b = document.createElement("button");
    b.className = "tool";
    b.dataset.tool = cmd;
    b.title = title;
    b.setAttribute("aria-label", title);
    b.innerHTML = icon(ic);
    b.onclick = () => toolClick(cmd);
    rail.append(b);
  }
  const strip = $("#sheet-tools");
  for (const [ic, label, cmd] of STRIP) {
    const b = document.createElement("button");
    b.className = "pt";
    b.dataset.tool = cmd;
    b.innerHTML = `${icon(ic)}<span>${label}</span>`;
    b.onclick = () => toolClick(cmd);
    strip.append(b);
  }

  const views = $("#views");
  for (const [ic, title, fn] of [
    ["extents", "Zoom extents (double-click the wheel)", () => zoomExtents()],
    ["zoomin", "Zoom in", () => zoomBy(1.5)],
    ["zoomout", "Zoom out", () => zoomBy(1 / 1.5)],
    ["pan", "Pan (PA, or drag with the wheel)", () => startCommand("PAN")],
  ]) {
    const b = document.createElement("button");
    b.className = "iconbtn";
    b.title = title;
    b.setAttribute("aria-label", title);
    b.innerHTML = icon(ic);
    b.onclick = fn;
    views.append(b);
  }

  // Status bar toggles (desktop) and aid chips (phone).
  for (const [k, label, key] of [
    ["grid", "GRID", "F7"],
    ["snap", "SNAP", "F9"],
    ["ortho", "ORTHO", "F8"],
    ["polar", "POLAR", "F10"],
    ["osnap", "OSNAP", "F3"],
    ["dyn", "DYN", "F12"],
  ]) {
    const b = document.createElement("button");
    b.className = "tg";
    b.dataset.aid = k;
    b.textContent = label;
    b.title = `${label} (${key})`;
    b.onclick = () => toggleAid(k);
    $("#aids").append(b);
  }
  for (const [k, label] of [["snap", "SNAP"], ["ortho", "ORTHO"], ["grid", "GRID"]]) {
    const b = document.createElement("button");
    b.className = "c";
    b.dataset.aid = k;
    b.textContent = label;
    b.onclick = () => toggleAid(k);
    $("#chip-aids").append(b);
  }
  syncAids();

  document.querySelectorAll("[data-act]").forEach((b) => {
    b.onclick = () => {
      const a = b.dataset.act;
      if (a === "new-metric") newDrawing(false);
      else if (a === "new-imperial") newDrawing(true);
      else if (a === "sample") openSample();
      else openDialog();
    };
  });
  $("#mark").onclick = (e) => showMenu(e.currentTarget, fileMenu());
  $("#pmenu").onclick = (e) => showMenu(e.currentTarget, fileMenu());
  $("#new-btn").onclick = (e) => showMenu(e.currentTarget, newMenu());
  $("#undo-btn").onclick = $("#pundo").onclick = () => runLine("UNDO");
  $("#redo-btn").onclick = $("#predo").onclick = () => runLine("REDO");
  $("#players").onclick = () => $("#side").classList.toggle("open");
  $("#who").onclick = () => startCommand("SAVEDBY");
  $("#layer-add").onclick = () => addLayerRow();
  $("#file-input").onchange = async (e) => {
    for (const f of e.target.files) await openFile(f, null);
    e.target.value = "";
  };

  let deferredInstall = null;
  addEventListener("beforeinstallprompt", (e) => {
    e.preventDefault();
    deferredInstall = e;
    $("#install-btn").hidden = false;
  });
  $("#install-btn").onclick = async () => {
    if (!deferredInstall) return;
    deferredInstall.prompt();
    await deferredInstall.userChoice;
    deferredInstall = null;
    $("#install-btn").hidden = true;
  };
  addEventListener("appinstalled", () => ($("#install-btn").hidden = true));
}

function toggleAid(k) {
  aids[k] = !aids[k];
  // ORTHO and POLAR exclude each other (DESIGN.md §9).
  if (k === "ortho" && aids.ortho) aids.polar = false;
  if (k === "polar" && aids.polar) aids.ortho = false;
  saveAids();
  syncAids();
  log(`<${k.toUpperCase()} ${aids[k] ? "on" : "off"}>`);
  updateCursor();
}
function syncAids() {
  document.querySelectorAll("[data-aid]").forEach((b) => b.classList.toggle("on", !!aids[b.dataset.aid]));
  needsDraw = true;
}

function newMenu() {
  return [
    ["New drawing, metric (mm)", "Ctrl+N", () => newDrawing(false)],
    ["New drawing, imperial (in)", "", () => newDrawing(true)],
    ["Sample: P-1042 plate", "", () => openSample()],
  ];
}

function fileMenu() {
  const has = !!doc;
  return [
    ["New metric drawing", "Ctrl+N", () => newDrawing(false)],
    ["New imperial drawing", "", () => newDrawing(true)],
    ["Open…", "Ctrl+O", () => openDialog()],
    ["Sample: P-1042 plate", "", () => openSample()],
    ["Blackboard: lessons", "F1", () => Board.open()],
    ["Blackboard: shop settings", "", () => Board.open("settings")],
    ["Check my drawing against shop rules", "", () => Board.check(), !has],
    "-",
    ["Save", "Ctrl+S", () => save(), !has],
    ["Save as .cad…", "Ctrl+Shift+S", () => saveAs("cad"), !has],
    ["Save as .dwg (2018 format)…", "", () => saveAs("dwg"), !has],
    ["Export DXF…", "", () => saveAs("dxf"), !has],
    ["Export binary DXF…", "", () => saveAs("dxfb"), !has],
    ["Export SVG…", "", () => saveAs("svg"), !has],
    ["Plot to PDF…", "Ctrl+P", () => startCommand("PLOT"), !has],
    ["Print…", "", () => startCommand("PRINT"), !has],
    ["Page setup…", "", () => startCommand("PAGESETUP"), !has],
    "-",
    ...(has ? (core.query(doc.id, "layouts")?.layouts || []).map((l) => [`${l.current ? "● " : ""}${esc(l.name)}${l.paperName ? ` · ${esc(l.paperName)}` : ""}`, l.model ? "Model" : "Layout", () => switchSheet(l.name)]) : []),
    ["New layout…", "", () => startCommand("LAYOUT", (c) => layoutNew(c)), !has],
    "-",
    ["Close drawing", "Ctrl+W", () => closeDoc(doc), !has],
    "-",
    ["Your name on saves…", "", () => startCommand("SAVEDBY")],
  ];
}

function showMenu(anchor, items) {
  const m = $("#menu");
  m.innerHTML = "";
  for (const it of items) {
    if (it === "-") {
      m.append(document.createElement("hr"));
      continue;
    }
    const [label, key, fn, disabled] = it;
    const b = document.createElement("button");
    b.setAttribute("role", "menuitem");
    b.innerHTML = `<span>${label}</span><kbd>${esc(key || "")}</kbd>`;
    b.disabled = !!disabled;
    b.onclick = () => {
      hideMenu();
      fn();
    };
    m.append(b);
  }
  m.hidden = false;
  const r = anchor.getBoundingClientRect();
  m.style.left = Math.max(8, Math.min(r.left, innerWidth - m.offsetWidth - 8)) + "px";
  m.style.top = Math.min(r.bottom + 4, innerHeight - m.offsetHeight - 8) + "px";
  setTimeout(() => document.addEventListener("pointerdown", outsideMenu), 0);
}
function outsideMenu(e) {
  if (!$("#menu").contains(e.target)) hideMenu();
}
function hideMenu() {
  $("#menu").hidden = true;
  document.removeEventListener("pointerdown", outsideMenu);
}

let toastTimer = 0;
function toast(msg, err = false) {
  const t = $("#toast");
  t.textContent = msg;
  t.classList.toggle("err", err);
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), err ? 6000 : 3000);
}

function esc(s) {
  return String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

/** The short unit mark for lengths ("mm", "in"). */
function unitMark() {
  const u = { 1: "in", 2: "ft", 4: "mm", 5: "cm", 6: "m" }[doc?.info.insunits];
  return u || "";
}

// ---------------------------------------------------------------- drawings

function newDrawing(imperial) {
  if (!core) return;
  const n = docs.filter((d) => /^Drawing\d+/.test(d.name)).length + 1;
  let id;
  try {
    id = core.newDrawing(imperial);
  } catch (e) {
    return handleError(e);
  }
  addDoc({ id, name: `Drawing${n}.cad`, format: "cad", fileHandle: null, orig: null, imperial });
}

function openSample() {
  if (!core) return;
  let id;
  try {
    id = core.sample();
  } catch (e) {
    return handleError(e);
  }
  addDoc({ id, name: "P-1042 plate.cad", format: "cad", fileHandle: null, orig: null, sample: true });
}

async function openFile(file, handle) {
  if (!core) return;
  const bytes = new Uint8Array(await file.arrayBuffer());
  let id;
  try {
    id = core.open(bytes);
  } catch (e) {
    if (e instanceof CoreCrash) return crashed(e);
    toast(`${file.name}: ${e.message}`, true);
    return;
  }
  const ext = (file.name.split(".").pop() || "").toLowerCase();
  addDoc({ id, name: file.name, format: ext === "dwg" || ext === "dxf" ? ext : "cad", fileHandle: handle, orig: bytes });
}

function addDoc(d) {
  d.view = null;
  try {
    d.disp = prepare(core.display(d.id));
    d.info = core.info(d.id);
  } catch (e) {
    return handleError(e);
  }
  docs.push(d);
  activate(d);
  zoomExtents();
  log(`${d.name}: ${d.info.summary.split("\n")[0]}`);
}

function activate(d) {
  if (runner) cancelCommand(false);
  doc = d;
  selection.clear();
  grips = [];
  hover = null;
  app.classList.toggle("no-drawing", !doc);
  renderAll();
  needsDraw = true;
}

function closeDoc(d) {
  if (!d) return;
  if (d.info.dirty && !confirm(`${d.name} has unsaved changes. Close it anyway?`)) return;
  core.close(d.id);
  docs.splice(docs.indexOf(d), 1);
  activate(docs[docs.length - 1] || null);
}

/** Read the drawing again after it changed: only the entities that
 *  changed, when the core can say which (crates/cad-ffi/src/session.rs). */
function refresh(d = doc) {
  if (!d) return;
  try {
    const p = core.displayChanges(d.id);
    if (p.patch && d.disp) patchDisplay(d.disp, p);
    else d.disp = prepare(p);
    d.info = core.info(d.id);
  } catch (e) {
    if (e instanceof CoreCrash) return crashed(e);
    throw e;
  }
  memoryCheck();
  if (d === doc) {
    for (const h of [...selection]) if (!d.disp.byHandle.has(h)) selection.delete(h);
    updateGrips();
    renderAll();
  }
  needsDraw = true;
}

/** Replace the items of the entities a patch lists with the patch's own.
 *  Removed items are marked dead and skipped; the list is packed again
 *  once they are many. */
function patchDisplay(disp, p) {
  for (const h of p.patch) {
    for (const k of disp.byHandle.get(h) || []) {
      disp.items[k].dead = true;
      disp.dead++;
      disp.live--;
    }
    disp.byHandle.delete(h);
  }
  boxes(p.items);
  const styleIndex = new Map(disp.styles.map((st, i) => [styleKey(st), i]));
  const remap = p.styles.map((st) => {
    const key = styleKey(st);
    if (!styleIndex.has(key)) {
      styleIndex.set(key, disp.styles.length);
      disp.styles.push(st);
      disp.byStyle.push([]);
    }
    return styleIndex.get(key);
  });
  for (const it of p.items) {
    it.style = remap[it.style];
    const k = disp.items.length;
    disp.items.push(it);
    disp.live++;
    if (!disp.byHandle.has(it.handle)) disp.byHandle.set(it.handle, []);
    disp.byHandle.get(it.handle).push(k);
    disp.byStyle[it.style].push(k);
  }
  disp.bounds = p.bounds;
  disp.skipped = p.skipped;
  if (disp.dead > 1000 && disp.dead > disp.live) pack(disp);
}

function styleKey(st) {
  return `${st.screen}|${st.print}|${st.px}|${st.layer}|${st.linetype}`;
}

/** Drop dead items and rebuild the lookups. */
function pack(disp) {
  disp.items = disp.items.filter((it) => !it.dead);
  index(disp);
}

function index(disp) {
  disp.byHandle = new Map();
  disp.items.forEach((it, k) => {
    if (!disp.byHandle.has(it.handle)) disp.byHandle.set(it.handle, []);
    disp.byHandle.get(it.handle).push(k);
  });
  disp.byStyle = disp.styles.map(() => []);
  disp.items.forEach((it, k) => disp.byStyle[it.style].push(k));
  disp.dead = 0;
  disp.live = disp.items.length;
}

/** Bounds for each display item, and items grouped by style. */
function prepare(disp) {
  boxes(disp.items);
  index(disp);
  return disp;
}

/** Each item's bounds, from its operations. */
function boxes(items) {
  for (const it of items) {
    const { ops } = it;
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    const add = (x, y) => {
      if (x < x0) x0 = x;
      if (x > x1) x1 = x;
      if (y < y0) y0 = y;
      if (y > y1) y1 = y;
    };
    for (let i = it.start; i < it.end; i += opLength(ops, i)) {
      const c = ops[i];
      if (c === 2) { add(ops[i + 1], ops[i + 2]); add(ops[i + 3], ops[i + 4]); }
      else if (c === 3) { const r = ops[i + 3]; add(ops[i + 1] - r, ops[i + 2] - r); add(ops[i + 1] + r, ops[i + 2] + r); }
      else if (c === 4) { for (let k = 0; k < ops[i + 1]; k++) add(ops[i + 3 + 2 * k], ops[i + 4 + 2 * k]); }
      else if (c === 5) {
        let j = i + 2;
        for (let r = 0; r < ops[i + 1]; r++) { const n = ops[j]; for (let k = 0; k < n; k++) add(ops[j + 1 + 2 * k], ops[j + 2 + 2 * k]); j += 1 + 2 * n; }
        it.fill = true;
      } else if (c === 6) add(ops[i + 1], ops[i + 2]);
      else if (c === 7) {
        // A construction line reaches everywhere; it has no extent of its own.
        x0 = y0 = -Infinity;
        x1 = y1 = Infinity;
        it.ray = true;
      }
    }
    it.box = [x0, y0, x1, y1];
  }
}

let memoryWarned = false;
/** Warn once when the core holds a lot: past the core's own limit it
 *  refuses new drawings and edits (crates/cad-ffi/src/web.rs). */
function memoryCheck() {
  const mb = core.liveBytes() / 2 ** 20;
  if (mb > Core.SOFT_LIMIT_MB * 0.75 && !memoryWarned) {
    memoryWarned = true;
    toast(`Memory is getting full (${Math.round(mb)} MB). Save your work and close drawings you are not using.`, true);
  } else if (mb < Core.SOFT_LIMIT_MB * 0.5) memoryWarned = false;
}

function renderAll() {
  renderTabs();
  renderSheets();
  updateChrome();
  renderProps();
  renderLayers();
  renderDrawing();
  renderSheet();
}

function renderTabs() {
  const nav = $("#tabs");
  nav.innerHTML = "";
  for (const d of docs) {
    const t = document.createElement("div");
    t.className = "tab" + (d === doc ? " on" : "");
    t.innerHTML = `${d === doc ? `<span class="dot${d.info?.dirty ? " dirty" : ""}"></span>` : ""}<span>${esc(d.name)}</span><button class="x" aria-label="Close ${esc(d.name)}">${icon("close")}</button>`;
    t.title = d.info?.dirty ? "Unsaved changes" : "Saved";
    t.onclick = (e) => {
      if (e.target.closest(".x")) closeDoc(d);
      else if (d !== doc) activate(d);
    };
    nav.append(t);
  }
}

/** Model and the layouts, as tabs in the status bar (LAYOUT). */
function renderSheets() {
  const el = $("#sheets");
  if (!el) return;
  el.innerHTML = "";
  if (!doc) return;
  let list = [];
  try {
    list = core.query(doc.id, "layouts")?.layouts || [];
  } catch (_) {}
  for (const sh of list) {
    const b = document.createElement("button");
    b.className = "st" + (sh.current ? " on" : "");
    b.textContent = sh.name;
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", sh.current ? "true" : "false");
    b.title = sh.model ? "Model space: draw full size" : `Layout ${sh.name}: ${sh.paperName || ""} paper; double-click to rename`;
    b.onclick = () => switchSheet(sh.name);
    if (!sh.model) b.ondblclick = () => startCommand("LAYOUT", (c) => layoutRename(c, sh.name));
    el.append(b);
  }
  const add = document.createElement("button");
  add.className = "st add";
  add.textContent = "+";
  add.title = "New layout";
  add.onclick = () => startCommand("LAYOUT", (c) => layoutNew(c));
  el.append(add);
}

/** Work on model space or a layout: the display, picks and new objects
 *  follow (crates/cad-ffi/src/session.rs, `space`). */
function switchSheet(name) {
  if (!doc) return;
  if (runner) cancelCommand(false);
  switchSheetNow(name);
}

/** [`switchSheet`] from inside a running command (LAYOUT). */
function switchSheetNow(name) {
  try {
    core.exec(doc.id, `space ${name}`);
  } catch (e) {
    return handleError(e);
  }
  selection.clear();
  doc.views = doc.views || {};
  if (doc.view) doc.views[doc.sheet || "Model"] = doc.view;
  doc.sheet = name;
  refresh(doc);
  doc.view = doc.views[name] || null;
  if (!doc.view) zoomExtents();
  needsDraw = true;
}

function updateChrome() {
  const i = doc?.info;
  for (const id of ["#undo-btn", "#pundo"]) $(id).disabled = !i || !i.undo;
  for (const id of ["#redo-btn", "#predo"]) $(id).disabled = !i || !i.redo;
  const saved = $("#saved");
  saved.textContent = !i ? "" : i.dirty ? "Unsaved" : "Saved";
  saved.classList.toggle("dirty", !!i?.dirty);
  $("#crumb").innerHTML = i ? (i.paper ? `<b>${esc(i.space)}</b> · paper · ${esc(i.units)}` : `<b>Model</b> · 1:1 · ${esc(i.units)}`) : "";
  $("#ptitle").textContent = doc ? doc.name : "BlackCAD";
  $("#psub").textContent = i ? `${i.space || "Model"} · ${unitMark() || i.units} · ${i.dirty ? "Unsaved" : "Saved"}` : "";
  const u = user();
  $("#who-av").textContent = u ? u.split(/\s+/).map((w) => w[0]).join("").slice(0, 2).toUpperCase() : "·";
  $("#who-text").textContent = u ? `${u} · this browser` : "Editing in this browser";
  document.title = doc ? `${doc.name}${i.dirty ? " •" : ""} — ${APP_TITLE}` : APP_TITLE;
}

// ---------------------------------------------------------------- view

function size() {
  return { w: canvas.clientWidth, h: canvas.clientHeight };
}
function toScreen(x, y, v = doc.view) {
  const { w, h } = size();
  return [(x - v.cx) * v.scale + w / 2, h / 2 - (y - v.cy) * v.scale];
}
function toWorld(sx, sy, v = doc.view) {
  const { w, h } = size();
  return [(sx - w / 2) / v.scale + v.cx, (h / 2 - sy) / v.scale + v.cy];
}

function zoomExtents() {
  if (!doc) return;
  const b = doc.info.paper ? [0, 0, doc.info.paper[0], doc.info.paper[1]] : doc.disp.bounds;
  const { w, h } = size();
  if (!w || !h) return;
  if (!b) {
    // An empty drawing: the template's sheet, A3 or ANSI B (DESIGN.md §8).
    const [sw, sh] = doc.info.insunits === 1 ? [17, 11] : [420, 297];
    doc.view = { cx: sw / 2, cy: sh / 2, scale: Math.min(w / sw, h / sh) * 0.9 };
  } else {
    const bw = Math.max(b[2] - b[0], 1e-9), bh = Math.max(b[3] - b[1], 1e-9);
    doc.view = { cx: (b[0] + b[2]) / 2, cy: (b[1] + b[3]) / 2, scale: Math.min(w / bw, h / bh) * 0.82 };
  }
  needsDraw = true;
}
function zoomBy(f, sx, sy) {
  if (!doc?.view) return;
  const { w, h } = size();
  if (sx === undefined) [sx, sy] = [w / 2, h / 2];
  const [wx, wy] = toWorld(sx, sy);
  const v = doc.view;
  v.scale = Math.min(Math.max(v.scale * f, 1e-9), 1e9);
  const [nx, ny] = toWorld(sx, sy);
  v.cx += wx - nx;
  v.cy += wy - ny;
  needsDraw = true;
  updateCursor();
}
function zoomWindow(a, b) {
  const { w, h } = size();
  const bw = Math.abs(b[0] - a[0]), bh = Math.abs(b[1] - a[1]);
  if (bw <= 0 || bh <= 0) return;
  doc.view = { cx: (a[0] + b[0]) / 2, cy: (a[1] + b[1]) / 2, scale: Math.min(w / bw, h / bh) };
  needsDraw = true;
}

// ---------------------------------------------------------------- drawing

function frame() {
  if (needsDraw) {
    needsDraw = false;
    draw();
  }
  requestAnimationFrame(frame);
}

/** A rotation or reflection plus a move, for previews of COPY, ROTATE, MIRROR. */
function xfMove(dx, dy) {
  return { a: 1, b: 0, c: 0, d: 1, e: dx, f: dy };
}
function xfRotate(cx, cy, ang) {
  const k = Math.cos(ang), s = Math.sin(ang);
  return { a: k, b: s, c: -s, d: k, e: cx - (k * cx - s * cy), f: cy - (s * cx + k * cy) };
}
function xfMirror(p, q) {
  const L = Math.hypot(q[0] - p[0], q[1] - p[1]) || 1;
  const ux = (q[0] - p[0]) / L, uy = (q[1] - p[1]) / L;
  const a = 2 * ux * ux - 1, b = 2 * ux * uy, c = 2 * ux * uy, d = 2 * uy * uy - 1;
  return { a, b, c, d, e: p[0] - (a * p[0] + c * p[1]), f: p[1] - (b * p[0] + d * p[1]) };
}
const xfPoint = (m, x, y) => [m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f];

function draw() {
  const dpr = window.devicePixelRatio || 1;
  const { w, h } = size();
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  Object.assign(COLORS, onPaper() ? PAPER : DARK);
  ctx.fillStyle = COLORS.space;
  ctx.fillRect(0, 0, w, h);
  if (!doc) return;
  if (!doc.view) zoomExtents();
  if (!doc.view) return;
  const paper = doc.info.paper;
  if (paper) {
    // The sheet, with a shadow.
    const [ax, ay] = toScreen(0, paper[1]);
    const [bx, by] = toScreen(paper[0], 0);
    ctx.fillStyle = "rgba(0, 0, 0, .35)";
    ctx.fillRect(ax + 5, ay + 5, bx - ax, by - ay);
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(ax, ay, bx - ax, by - ay);
  } else if (aids.grid) drawGrid(w, h);

  const disp = doc.disp;
  const [vx0, vy1] = toWorld(0, 0);
  const [vx1, vy0] = toWorld(w, h);
  const visible = (it) => it.box[2] >= vx0 && it.box[0] <= vx1 && it.box[3] >= vy0 && it.box[1] <= vy1;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  disp.byStyle.forEach((list, s) => {
    const st = disp.styles[s];
    const color = paper ? st.print : st.screen;
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.lineWidth = st.px;
    const fills = [];
    ctx.beginPath();
    for (const k of list) {
      const it = disp.items[k];
      if (it.dead || !visible(it)) continue;
      if (it.fill) fills.push(it);
      pathItem(it);
    }
    ctx.stroke();
    // Fills (arrowheads, solid hatches) after the batch, which a fill's
    // own path would otherwise reset.
    for (const it of fills) fillItem(it);
  });

  // Hover glow, then the selection in blue with its grips (DESIGN.md §5).
  if (hover && !selection.has(hover)) {
    drawHandles([hover], "rgba(61, 139, 255, .35)", 6);
    drawHandles([hover], "rgba(190, 215, 255, .9)", 0);
  }
  if (selection.size) drawHandles([...selection], COLORS.select, 1);
  const ghost = runner?.req?.ghost;
  if (ghost?.xf) drawHandles(ghost.handles, COLORS.rubber, 0, ghost.xf, 0.75);
  drawGrips();
  drawPreview();
  drawBox();
  drawUcs(h);
  drawCursor(w, h);
  drawLoupe(dpr);
}

function fillItem(it) {
  const { ops } = it;
  ctx.save();
  ctx.beginPath();
  for (let i = it.start; i < it.end; i += opLength(ops, i)) {
    if (ops[i] !== 5) continue;
    let j = i + 2;
    for (let r = 0; r < ops[i + 1]; r++) {
      const n = ops[j];
      for (let k = 0; k < n; k++) {
        const [sx, sy] = toScreen(ops[j + 1 + 2 * k], ops[j + 2 + 2 * k]);
        k ? ctx.lineTo(sx, sy) : ctx.moveTo(sx, sy);
      }
      ctx.closePath();
      j += 1 + 2 * n;
    }
  }
  ctx.fill("evenodd");
  ctx.restore();
}

function pathItem(it, m = null) {
  const { ops } = it;
  const v = doc.view;
  const T = m ? (x, y) => toScreen(...xfPoint(m, x, y), v) : (x, y) => toScreen(x, y, v);
  const flips = m && m.a * m.d - m.b * m.c < 0;
  const mapAngle = (t) => {
    if (!m) return t;
    const dx = m.a * Math.cos(t) + m.c * Math.sin(t), dy = m.b * Math.cos(t) + m.d * Math.sin(t);
    return Math.atan2(dy, dx);
  };
  for (let i = it.start; i < it.end; i += opLength(ops, i)) {
    const c = ops[i];
    if (c === 2) {
      const [ax, ay] = T(ops[i + 1], ops[i + 2]);
      const [bx, by] = T(ops[i + 3], ops[i + 4]);
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
    } else if (c === 3) {
      const r = ops[i + 3];
      let a0 = ops[i + 4], a1 = ops[i + 5];
      const full = Math.abs(a1 - a0 - TAU) < 1e-9;
      if (m && !full) {
        const s = mapAngle(a0), e = mapAngle(a1);
        [a0, a1] = flips ? [e, s] : [s, e];
        if (a1 < a0) a1 += TAU;
      }
      const [sx, sy] = T(ops[i + 1], ops[i + 2]);
      const rs = r * v.scale;
      if (rs < 0.3) continue;
      ctx.moveTo(sx + rs * Math.cos(a0), sy - rs * Math.sin(a0));
      ctx.arc(sx, sy, rs, -a0, -a1, true);
    } else if (c === 4) {
      const n = ops[i + 1];
      for (let k = 0; k < n; k++) {
        const [px, py] = T(ops[i + 3 + 2 * k], ops[i + 4 + 2 * k]);
        k ? ctx.lineTo(px, py) : ctx.moveTo(px, py);
      }
      if (n === 1) {
        const [px, py] = T(ops[i + 3], ops[i + 4]);
        ctx.lineTo(px + 0.01, py);
      }
      if (ops[i + 2]) ctx.closePath();
    } else if (c === 6) {
      const [px, py] = T(ops[i + 1], ops[i + 2]);
      ctx.moveTo(px + 1.5, py);
      ctx.arc(px, py, 1.5, 0, TAU);
    } else if (c === 7) rayPath(ops, i, m);
  }
}

/** A construction line (display operation 7): cut to the screen and
 *  dashed by its linetype, the dashes anchored at its base point so they
 *  stay put while the view moves. */
function rayPath(ops, i, m) {
  const v = doc.view;
  let ox = ops[i + 1], oy = ops[i + 2], dx = ops[i + 3], dy = ops[i + 4];
  if (m) {
    [ox, oy] = xfPoint(m, ox, oy);
    const ex = m.a * dx + m.c * dy, ey = m.b * dx + m.d * dy;
    const L = Math.hypot(ex, ey) || 1;
    [dx, dy] = [ex / L, ey / L];
  }
  const ray = ops[i + 5] === 1;
  const n = ops[i + 6];
  const { w, h } = size();
  // The screen in world units, a little larger.
  const [x0, y1] = toWorld(-20, -20), [x1, y0] = toWorld(w + 20, h + 20);
  let t0 = ray ? 0 : -Infinity, t1 = Infinity;
  for (const [p, d, lo, hi] of [[ox, dx, x0, x1], [oy, dy, y0, y1]]) {
    if (Math.abs(d) < 1e-15) {
      if (p < lo || p > hi) return;
      continue;
    }
    let a = (lo - p) / d, b = (hi - p) / d;
    if (a > b) [a, b] = [b, a];
    t0 = Math.max(t0, a);
    t1 = Math.min(t1, b);
  }
  if (!(t1 > t0)) return;
  const seg = (ta, tb) => {
    const [ax, ay] = toScreen(ox + dx * ta, oy + dy * ta, v);
    const [bx, by] = toScreen(ox + dx * tb, oy + dy * tb, v);
    ctx.moveTo(ax, ay);
    ctx.lineTo(bx === ax && by === ay ? bx + 0.01 : bx, by);
  };
  const pat = [];
  for (let k = 0; k < n; k++) pat.push(ops[i + 7 + k]);
  const period = pat.reduce((s, x) => s + Math.abs(x), 0);
  // Too fine to see, or too many dashes: drawn solid.
  if (!n || period * v.scale < 3 || (t1 - t0) / period > 4000) return seg(t0, t1);
  let t = Math.floor(t0 / period) * period;
  while (t < t1) {
    for (const x of pat) {
      const len = Math.abs(x);
      if (x >= 0) {
        const a = Math.max(t, t0), b = Math.min(t + len, t1);
        if (b >= a && a <= t1) seg(a, b);
      }
      t += len;
      if (t >= t1) break;
    }
  }
}

function drawHandles(handles, color, extra, m = null, alpha = 1) {
  const disp = doc.disp;
  ctx.save();
  ctx.globalAlpha = alpha;
  ctx.strokeStyle = color;
  for (const h of handles) {
    for (const k of disp.byHandle.get(h) || []) {
      const it = disp.items[k];
      ctx.lineWidth = disp.styles[it.style].px + extra;
      ctx.beginPath();
      pathItem(it, m);
      ctx.stroke();
    }
  }
  ctx.restore();
}

function drawGrips() {
  if (!grips.length) return;
  ctx.save();
  ctx.fillStyle = COLORS.select;
  ctx.strokeStyle = "#0d1013";
  ctx.lineWidth = 1;
  for (const g of grips) {
    const [sx, sy] = toScreen(g.x, g.y);
    const hot = cursor.grip === g;
    const s = hot ? GRIP + 2 : GRIP;
    ctx.fillStyle = hot ? "#9cc8ff" : COLORS.select;
    ctx.fillRect(Math.round(sx - s / 2), Math.round(sy - s / 2), s, s);
    ctx.strokeRect(Math.round(sx - s / 2) + 0.5, Math.round(sy - s / 2) + 0.5, s - 1, s - 1);
  }
  ctx.restore();
}

/** Grid spacing on screen: steps of five, lines 8 to 40 px apart (DESIGN.md §2). */
function gridStep() {
  let g = GRID_SPACING;
  const s = doc.view.scale;
  while (g * s < 8) g *= 5;
  while (g * s >= 40) g /= 5;
  return g;
}

function drawGrid(w, h) {
  const g = gridStep();
  const [x0, y1] = toWorld(0, 0);
  const [x1, y0] = toWorld(w, h);
  if ((x1 - x0) / g > 600 || (y1 - y0) / g > 600) return;
  for (const major of [false, true]) {
    ctx.beginPath();
    const step = major ? g * 5 : g;
    for (let x = Math.ceil(x0 / step) * step; x <= x1; x += step) {
      if (!major && Math.abs(Math.round(x / g)) % 5 === 0) continue;
      const sx = Math.round(toScreen(x, 0)[0]) + 0.5;
      ctx.moveTo(sx, 0);
      ctx.lineTo(sx, h);
    }
    for (let y = Math.ceil(y0 / step) * step; y <= y1; y += step) {
      if (!major && Math.abs(Math.round(y / g)) % 5 === 0) continue;
      const sy = Math.round(toScreen(0, y)[1]) + 0.5;
      ctx.moveTo(0, sy);
      ctx.lineTo(w, sy);
    }
    ctx.strokeStyle = major ? COLORS.gridMajor : COLORS.gridMinor;
    ctx.lineWidth = 1;
    ctx.stroke();
  }
}

/** The axis icon, bottom left (DESIGN.md §2). */
function drawUcs(h) {
  const ox = 30.5, oy = h - 30.5, L = 52;
  ctx.save();
  ctx.strokeStyle = ctx.fillStyle = COLORS.ucs;
  ctx.lineWidth = 1.4;
  ctx.beginPath();
  ctx.moveTo(ox, oy - L);
  ctx.lineTo(ox, oy);
  ctx.lineTo(ox + L, oy);
  ctx.moveTo(ox - 4, oy - L + 7);
  ctx.lineTo(ox, oy - L);
  ctx.lineTo(ox + 4, oy - L + 7);
  ctx.moveTo(ox + L - 7, oy - 4);
  ctx.lineTo(ox + L, oy);
  ctx.lineTo(ox + L - 7, oy + 4);
  ctx.stroke();
  ctx.font = "11px ui-monospace, monospace";
  ctx.fillText("Y", ox + 6, oy - L + 6);
  ctx.fillText("X", ox + L + 4, oy - 5);
  ctx.restore();
}

function drawPreview() {
  const req = runner?.req;
  if (!req?.preview || (!cursor.inside && !cursor.touch)) return;
  const p = currentPoint();
  if (!p) return;
  let shapes;
  try {
    shapes = req.preview(p) || [];
  } catch (_) {
    return;
  }
  ctx.save();
  ctx.strokeStyle = COLORS.rubber;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (const s of shapes) {
    if (s.t === "line") {
      const [ax, ay] = toScreen(...s.a);
      const [bx, by] = toScreen(...s.b);
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
    } else if (s.t === "circle" || s.t === "arc") {
      const [sx, sy] = toScreen(...s.c);
      const r = s.r * doc.view.scale;
      const a0 = s.t === "arc" ? s.a0 : 0, a1 = s.t === "arc" ? s.a1 : TAU;
      ctx.moveTo(sx + r * Math.cos(a0), sy - r * Math.sin(a0));
      ctx.arc(sx, sy, r, -a0, -a1, true);
    } else if (s.t === "poly") {
      s.pts.forEach((q, k) => {
        const [sx, sy] = toScreen(...q);
        k ? ctx.lineTo(sx, sy) : ctx.moveTo(sx, sy);
      });
      if (s.closed) ctx.closePath();
    }
  }
  ctx.stroke();
  ctx.restore();
}

function drawBox() {
  if (pointerMode?.kind !== "box") return;
  const { x0, y0, x1, y1 } = pointerMode;
  const crossing = x1 < x0;
  ctx.save();
  ctx.fillStyle = crossing ? "rgba(61, 220, 132, .08)" : "rgba(61, 139, 255, .10)";
  ctx.strokeStyle = crossing ? "#3ddc84" : COLORS.select;
  ctx.setLineDash(crossing ? [5, 4] : []);
  ctx.lineWidth = 1;
  const x = Math.min(x0, x1), y = Math.min(y0, y1), bw = Math.abs(x1 - x0), bh = Math.abs(y1 - y0);
  ctx.fillRect(x, y, bw, bh);
  ctx.strokeRect(x + 0.5, y + 0.5, bw, bh);
  ctx.restore();
}

function drawCursor(w, h) {
  const snap = cursor.snap;
  // On touch screens the marker shows only while a finger is placing a point.
  const showing = cursor.touch ? pointerMode?.kind === "place" : cursor.inside;
  if (snap && showing) {
    const [sx, sy] = toScreen(snap.x, snap.y);
    drawSnapMarker(snap.kind, sx, sy);
  }
  showTip();
  if (!cursor.inside) return;
  // On touch the crosshairs follow the one finger that is down.
  if (cursor.touch && !(pointers.size === 1 && pointerMode && pointerMode.kind !== "pan")) return;
  const pickBox = !runner?.req || runner.req.type === "select";
  const [cx, cy] = [Math.round(cursor.sx) + 0.5, Math.round(cursor.sy) + 0.5];
  ctx.save();
  ctx.strokeStyle = COLORS.cross;
  ctx.lineWidth = 1;
  if (pointerMode?.kind !== "pan") {
    ctx.beginPath();
    ctx.moveTo(0, cy);
    ctx.lineTo(w, cy);
    ctx.moveTo(cx, 0);
    ctx.lineTo(cx, h);
    ctx.stroke();
  }
  if (pickBox) {
    ctx.strokeStyle = "rgba(220, 228, 235, .85)";
    ctx.strokeRect(cx - 4, cy - 4, 8, 8);
  }
  ctx.restore();
}

/** The marker's shape tells the snap kind (DESIGN.md §9). */
function drawSnapMarker(kind, x, y) {
  const r = 7;
  ctx.save();
  ctx.strokeStyle = COLORS.snap;
  ctx.lineWidth = 1.8;
  ctx.beginPath();
  switch (kind) {
    case 1: ctx.rect(x - r, y - r, 2 * r, 2 * r); break;
    case 2: ctx.moveTo(x, y - r); ctx.lineTo(x + r, y + r * 0.8); ctx.lineTo(x - r, y + r * 0.8); ctx.closePath(); break;
    case 4: ctx.arc(x, y, r, 0, TAU); ctx.moveTo(x - 4, y); ctx.lineTo(x + 4, y); ctx.moveTo(x, y - 4); ctx.lineTo(x, y + 4); break;
    case 8: ctx.moveTo(x, y - r); ctx.lineTo(x + r, y); ctx.lineTo(x, y + r); ctx.lineTo(x - r, y); ctx.closePath(); break;
    case 16: ctx.moveTo(x - r, y - r); ctx.lineTo(x + r, y + r); ctx.moveTo(x + r, y - r); ctx.lineTo(x - r, y + r); break;
    case 32: ctx.moveTo(x - r, y - r); ctx.lineTo(x + r, y - r); ctx.lineTo(x - r, y + r); ctx.lineTo(x + r, y + r); ctx.closePath(); break;
    case 64: ctx.arc(x, y, r, 0, TAU); ctx.moveTo(x - 5, y - 5); ctx.lineTo(x + 5, y + 5); ctx.moveTo(x + 5, y - 5); ctx.lineTo(x - 5, y + 5); break;
    case 128: ctx.rect(x - r, y - r, r, r); ctx.rect(x, y, r, r); break;
    default: ctx.arc(x, y, 3, 0, TAU);
  }
  ctx.stroke();
  ctx.restore();
}

/** On touch screens, a magnifier above the finger keeps the snap in view (DESIGN.md §11). */
function drawLoupe(dpr) {
  if (pointerMode?.kind !== "place" || !cursor.touch) return;
  const R = 59, k = 2;
  const sx = cursor.sx, sy = cursor.sy;
  const lx = sx, ly = Math.max(R + 6, sy - 120);
  ctx.save();
  // The finger.
  ctx.beginPath();
  ctx.arc(sx, sy, 22, 0, TAU);
  ctx.fillStyle = "rgba(255, 79, 216, .10)";
  ctx.fill();
  ctx.strokeStyle = "rgba(255, 79, 216, .7)";
  ctx.lineWidth = 2;
  ctx.stroke();
  // The magnified spot.
  ctx.beginPath();
  ctx.arc(lx, ly, R, 0, TAU);
  ctx.clip();
  ctx.fillStyle = COLORS.space;
  ctx.fillRect(lx - R, ly - R, 2 * R, 2 * R);
  const src = R / k;
  ctx.drawImage(canvas, (sx - src) * dpr, (sy - src) * dpr, 2 * src * dpr, 2 * src * dpr, lx - R, ly - R, 2 * R, 2 * R);
  ctx.restore();
  ctx.save();
  ctx.beginPath();
  ctx.arc(lx, ly, R, 0, TAU);
  ctx.strokeStyle = COLORS.snap;
  ctx.lineWidth = 2;
  ctx.stroke();
  ctx.restore();
}

/** The snap tag and the coordinate boxes at the cursor (DYN). */
function showTip() {
  const tip = $("#tip");
  const tag = $("#snaptag");
  const dyn = $("#dyn");
  const req = runner?.req;
  const showing = (cursor.inside && !cursor.touch) || (cursor.touch && pointerMode?.kind === "place");
  if (!doc || !showing || pointerMode?.kind === "pan") {
    tip.hidden = true;
    return;
  }
  const snap = cursor.snap;
  tag.hidden = !snap;
  if (snap) tag.textContent = snap.name;
  const want = aids.dyn && req && (req.type === "point" || req.type === "distance" || req.type === "angle");
  if (want) {
    const p = currentPoint();
    let boxes;
    if (req.base) {
      const dx = p[0] - req.base[0], dy = p[1] - req.base[1];
      let ang = (Math.atan2(dy, dx) * 180) / Math.PI;
      if (ang < 0) ang += 360;
      boxes = [fmt(Math.hypot(dx, dy)), `${ang.toFixed(1)}°`];
    } else boxes = [fmt(p[0]), fmt(p[1])];
    dyn.innerHTML = boxes.map((b) => `<span>${esc(b)}</span>`).join("");
  } else dyn.innerHTML = "";
  if (!snap && !want) {
    tip.hidden = true;
    return;
  }
  let [x, y] = snap ? toScreen(snap.x, snap.y) : [cursor.sx, cursor.sy];
  if (cursor.touch) y = Math.max(140, y - 120) + 70;
  tip.hidden = false;
  // Keep it on screen: flip to the left of the point near the right edge.
  const { w } = size();
  const left = x + 14 + tip.offsetWidth > w - 8 ? x - 14 - tip.offsetWidth : x + 14;
  tip.style.left = Math.max(4, left) + "px";
  tip.style.top = y + 14 + "px";
}

// ---------------------------------------------------------------- cursor

function aperturePx() {
  return cursor.touch ? APERTURE.touch : APERTURE.mouse;
}

/** Where a click would put a point: snap, then grid step, ortho or polar. */
function currentPoint() {
  if (cursor.snap) return [cursor.snap.x, cursor.snap.y];
  let p = [cursor.wx, cursor.wy];
  const req = runner?.req;
  if (req?.pick) return p;
  if (aids.snap) {
    const g = gridStep();
    p = [Math.round(p[0] / g) * g, Math.round(p[1] / g) * g];
  }
  const base = req?.base;
  if (!base) return p;
  const dx = p[0] - base[0], dy = p[1] - base[1];
  const d = Math.hypot(dx, dy);
  if (req.angleLock != null) {
    const t = dx * Math.cos(req.angleLock) + dy * Math.sin(req.angleLock);
    return [base[0] + t * Math.cos(req.angleLock), base[1] + t * Math.sin(req.angleLock)];
  }
  if (aids.ortho) return Math.abs(dx) >= Math.abs(dy) ? [p[0], base[1]] : [base[0], p[1]];
  if (aids.polar && d > 0) {
    const ang = Math.atan2(dy, dx);
    const step = (POLAR_STEP * Math.PI) / 180;
    const near = Math.round(ang / step) * step;
    // Within 4 px of the polar ray: track along it.
    if (Math.abs(Math.sin(ang - near)) * d * doc.view.scale < 4) {
      const t = dx * Math.cos(near) + dy * Math.sin(near);
      return [base[0] + t * Math.cos(near), base[1] + t * Math.sin(near)];
    }
  }
  return p;
}

let cursorQueued = false;
function updateCursor() {
  if (cursorQueued) return;
  cursorQueued = true;
  requestAnimationFrame(() => {
    cursorQueued = false;
    if (!doc?.view || !core) return;
    [cursor.wx, cursor.wy] = toWorld(cursor.sx, cursor.sy);
    const req = runner?.req;
    const r = aperturePx() / doc.view.scale;
    cursor.snap = null;
    cursor.snaps = [];
    cursor.grip = null;
    if ((req?.type === "point" && !req.pick) || req?.type === "distance" || req?.type === "angle") {
      const kinds = req.forceSnap ?? (aids.osnap ? SNAP_DEFAULT : 0);
      if (kinds) {
        const res = core.query(doc.id, `snap ${cursor.wx} ${cursor.wy} ${r} ${kinds}`);
        cursor.snaps = res?.snaps || [];
        if (cursor.snaps.length) cursor.snap = cursor.snaps[cursor.snapIndex % cursor.snaps.length];
      }
    }
    if ((!req || req.type === "select" || req.pick) && !pointerMode) {
      cursor.grip = !req ? gripAt(cursor.sx, cursor.sy) : null;
      hover = cursor.grip ? null : core.query(doc.id, `pick ${cursor.wx} ${cursor.wy} ${r}`)?.handle ?? null;
    } else hover = null;
    showCoords();
    needsDraw = true;
  });
}

function gripAt(sx, sy) {
  for (const g of grips) {
    const [gx, gy] = toScreen(g.x, g.y);
    if (Math.abs(gx - sx) <= GRIP && Math.abs(gy - sy) <= GRIP) return g;
  }
  return null;
}

function showCoords() {
  if (!doc) return;
  const p = currentPoint();
  const t = `${fmt(p[0])}, ${fmt(p[1])}`;
  const u = unitMark();
  $("#coords").textContent = u ? `${t} ${u}` : t;
  $("#ppos").textContent = t;
}

/** A length in the drawing's display format. */
function fmt(v) {
  if (!doc || !core) return String(v);
  return core.query(doc.id, `fmt ${v}`)?.text[0] ?? (+v).toFixed(4);
}

// ---------------------------------------------------------------- pointer input

function bindInput() {
  canvas.addEventListener("pointerdown", onDown);
  canvas.addEventListener("pointermove", onMove);
  canvas.addEventListener("pointerup", onUp);
  canvas.addEventListener("pointercancel", onUp);
  canvas.addEventListener("pointerleave", () => {
    cursor.inside = false;
    hover = null;
    needsDraw = true;
  });
  canvas.addEventListener("pointerenter", () => (cursor.inside = true));
  canvas.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    // Right-click is Enter (COMMANDS.md §18).
    submit("");
  });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    zoomBy(Math.exp(-e.deltaY * (e.deltaMode === 1 ? 0.05 : 0.0015)), e.offsetX, e.offsetY);
  }, { passive: false });
  canvas.addEventListener("auxclick", (e) => e.button === 1 && e.preventDefault());
  let lastMiddle = 0;
  canvas.addEventListener("mousedown", (e) => {
    if (e.button !== 1) return;
    const now = performance.now();
    if (now - lastMiddle < 350) zoomExtents();
    lastMiddle = now;
  });
  new ResizeObserver(() => {
    needsDraw = true;
    if (doc && !doc.view) zoomExtents();
  }).observe(canvas);

  let dragDepth = 0;
  addEventListener("dragenter", (e) => {
    if (![...(e.dataTransfer?.types || [])].includes("Files")) return;
    dragDepth++;
    $("#drop-hint").hidden = false;
  });
  addEventListener("dragleave", () => {
    if (--dragDepth <= 0) {
      dragDepth = 0;
      $("#drop-hint").hidden = true;
    }
  });
  addEventListener("dragover", (e) => e.preventDefault());
  addEventListener("drop", async (e) => {
    e.preventDefault();
    dragDepth = 0;
    $("#drop-hint").hidden = true;
    for (const item of e.dataTransfer.items) {
      if (item.kind !== "file") continue;
      const handle = item.getAsFileSystemHandle ? await item.getAsFileSystemHandle().catch(() => null) : null;
      await openFile(item.getAsFile(), handle?.kind === "file" ? handle : null);
    }
  });

  addEventListener("keydown", onKey);
  cmdInput.addEventListener("input", onTyping);
  cmdInput.addEventListener("keydown", onCmdKey);
}

function setCursorFrom(e) {
  const r = canvas.getBoundingClientRect();
  cursor.sx = e.clientX - r.left;
  cursor.sy = e.clientY - r.top;
  cursor.touch = e.pointerType === "touch" || e.pointerType === "pen";
  cursor.inside = true;
}

function onDown(e) {
  if (!doc) return;
  canvas.focus({ preventScroll: true });
  canvas.setPointerCapture(e.pointerId);
  pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
  setCursorFrom(e);
  if (pointers.size === 2) {
    const [a, b] = [...pointers.values()];
    pointerMode = { kind: "pinch", d: Math.hypot(a.x - b.x, a.y - b.y), mx: (a.x + b.x) / 2, my: (a.y + b.y) / 2 };
    return;
  }
  const req = runner?.req;
  if (e.button === 1 || (e.button === 0 && runner?.name === "PAN")) {
    pointerMode = { kind: "pan", x: e.clientX, y: e.clientY };
    e.preventDefault();
    return;
  }
  if (e.button !== 0) return;
  if (req && req.type !== "select") {
    // A point is placed when the button or finger comes up, so a finger
    // can slide to its snap first (DESIGN.md §11).
    pointerMode = { kind: "place" };
    updateCursor();
    needsDraw = true;
    return;
  }
  if (!req) {
    const g = gripAt(cursor.sx, cursor.sy);
    if (g) {
      startGrip(g);
      pointerMode = { kind: "place", grip: true, x0: cursor.sx, y0: cursor.sy };
      return;
    }
  }
  pointerMode = { kind: "press", x0: cursor.sx, y0: cursor.sy, x1: cursor.sx, y1: cursor.sy, shift: e.shiftKey, touch: cursor.touch };
  needsDraw = true;
}

function onMove(e) {
  if (pointers.has(e.pointerId)) pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
  setCursorFrom(e);
  const m = pointerMode;
  if (m?.kind === "pinch" && pointers.size >= 2) {
    const [a, b] = [...pointers.values()];
    const d = Math.hypot(a.x - b.x, a.y - b.y);
    const mx = (a.x + b.x) / 2, my = (a.y + b.y) / 2;
    const r = canvas.getBoundingClientRect();
    doc.view.cx -= (mx - m.mx) / doc.view.scale;
    doc.view.cy += (my - m.my) / doc.view.scale;
    if (m.d > 0) zoomBy(d / m.d, mx - r.left, my - r.top);
    Object.assign(m, { d, mx, my });
    needsDraw = true;
    return;
  }
  if (m?.kind === "pan") {
    doc.view.cx -= (e.clientX - m.x) / doc.view.scale;
    doc.view.cy += (e.clientY - m.y) / doc.view.scale;
    m.x = e.clientX;
    m.y = e.clientY;
    needsDraw = true;
  } else if (m?.kind === "press") {
    m.x1 = cursor.sx;
    m.y1 = cursor.sy;
    if (Math.hypot(m.x1 - m.x0, m.y1 - m.y0) > 6) {
      // One finger draws the selection box, as the mouse does; two
      // fingers pan and pinch.
      pointerMode = { ...m, kind: "box" };
    }
  } else if (m?.kind === "box") {
    m.x1 = cursor.sx;
    m.y1 = cursor.sy;
    needsDraw = true;
  }
  updateCursor();
}

function hoverAt(sx, sy) {
  const [x, y] = toWorld(sx, sy);
  return core.query(doc.id, `pick ${x} ${y} ${aperturePx() / doc.view.scale}`)?.handle ?? null;
}

function onUp(e) {
  pointers.delete(e.pointerId);
  const m = pointerMode;
  if (m?.kind === "pinch") {
    if (pointers.size === 0) pointerMode = null;
    return;
  }
  pointerMode = null;
  if (!m || !doc) return;
  if (m.kind === "place") {
    // A grip clicked without dragging stays hot until the next click.
    if (m.grip && Math.hypot(cursor.sx - m.x0, cursor.sy - m.y0) < 4) return updateCursor();
    [cursor.wx, cursor.wy] = toWorld(cursor.sx, cursor.sy);
    answerPoint(currentPoint());
  } else if (m.kind === "press") {
    const h = hoverAt(m.x0, m.y0);
    if (h) {
      if (m.shift && selection.has(h)) selection.delete(h);
      else selection.add(h);
    } else if (!m.shift) selection.clear();
    selectionChanged();
  } else if (m.kind === "box") {
    const a = toWorld(m.x0, m.y0), b = toWorld(m.x1, m.y1);
    const res = core.query(doc.id, `box ${a[0]} ${a[1]} ${b[0]} ${b[1]} ${m.x1 < m.x0 ? 1 : 0}`);
    for (const h of res?.handles || []) m.shift ? selection.delete(h) : selection.add(h);
    selectionChanged();
  }
  needsDraw = true;
  updateCursor();
}

function selectionChanged() {
  updateGrips();
  renderProps();
  renderSheet();
  if (runner?.req?.type === "select") setPrompt();
  needsDraw = true;
}

// ---------------------------------------------------------------- grips

/** Grips on the selected objects' defining points (DESIGN.md §5), as the
 *  core lists them (cad_core::grips). */
function updateGrips() {
  grips = [];
  if (!doc || selection.size > 40) return;
  const hs = [...selection];
  if (!hs.length) return;
  let list = [];
  try {
    list = core.query(doc.id, `grips ${hs.join(" ")}`)?.grips || [];
  } catch (_) {
    return;
  }
  // The rubber band while dragging pivots on the object's centre, or on
  // a line's other end.
  const at = (h, kind, field) => list.find((g) => g.handle === h && g.kind === kind && (!field || g.field === field));
  for (const gr of list) {
    const cen = at(gr.handle, "center");
    const other = gr.kind === "end" ? at(gr.handle, "end", gr.field === "1" ? "2" : "1") : null;
    grips.push({ ...gr, center: cen && [cen.x, cen.y], other: other && [other.x, other.y] });
  }
}

/** Dragging a grip edits the object: one command, one undo step. */
function startGrip(gr) {
  startCommand("GRIP", function* (c) {
    const base = [gr.x, gr.y];
    const req = c.point("Specify stretch point", { base });
    const cen = gr.center;
    if (gr.kind === "radius" && cen) req.preview = (q) => [{ t: "circle", c: cen, r: Math.hypot(q[0] - cen[0], q[1] - cen[1]) }];
    else if (gr.kind === "center" || gr.kind === "move") {
      req.ghost = { handles: [gr.handle], xf: null };
      req.preview = (q) => {
        req.ghost.xf = xfMove(q[0] - base[0], q[1] - base[1]);
        return [{ t: "line", a: base, b: q }];
      };
    } else if (gr.kind === "end" && gr.other) req.preview = (q) => [{ t: "line", a: gr.other, b: q }];
    else req.preview = (q) => [{ t: "line", a: base, b: q }];
    const q = yield req;
    if (q) c.edit(`grip ${gr.handle} ${gr.kind} ${gr.field} ${P(base)} ${P(q)}`);
  });
}

// ---------------------------------------------------------------- keyboard

function onKey(e) {
  if (e.target instanceof HTMLInputElement && e.target !== cmdInput) return;
  const k = e.key;
  if (!$("#numpad").hidden) return;
  if (e.ctrlKey || e.metaKey) {
    const lower = k.toLowerCase();
    const map = { z: e.shiftKey ? "REDO" : "UNDO", y: "REDO", s: e.shiftKey ? "SAVEAS" : "SAVE", o: "OPEN", n: "NEW", a: "SELECTALL", p: "PLOT", 1: "PROPERTIES", 2: "LAYER" };
    if (lower === "w" && doc) {
      e.preventDefault();
      closeDoc(doc);
    } else if (map[lower]) {
      e.preventDefault();
      runLine(map[lower]);
    }
    return;
  }
  const fkeys = { F3: "osnap", F7: "grid", F8: "ortho", F9: "snap", F10: "polar", F12: "dyn" };
  if (fkeys[k]) {
    e.preventDefault();
    toggleAid(fkeys[k]);
    return;
  }
  if (e.target === cmdInput) return;
  if (k === "Escape") return escape();
  if (k === "Delete" && selection.size && !runner) return runLine("ERASE");
  if (k === "Tab" && cursor.snaps.length > 1) {
    e.preventDefault();
    return cycleSnap();
  }
  // Typing goes to the command line from anywhere (COMMANDS.md §1).
  if (k.length === 1 && !e.altKey) {
    e.preventDefault();
    if (PHONE.matches) app.classList.add("typing");
    cmdInput.focus();
    if (k !== " ") {
      cmdInput.value += k;
      onTyping();
    } else submit(cmdInput.value);
  } else if (k === "Enter") submit(cmdInput.value);
}

function cycleSnap() {
  cursor.snapIndex++;
  cursor.snap = cursor.snaps[cursor.snapIndex % cursor.snaps.length];
  needsDraw = true;
}

function onCmdKey(e) {
  const k = e.key;
  const textReq = runner?.req?.type === "text";
  if (k === "Escape") {
    e.preventDefault();
    if (cmdInput.value) {
      cmdInput.value = "";
      onTyping();
    } else escape();
    canvas.focus({ preventScroll: true });
  } else if (k === "Enter" || (k === " " && !textReq)) {
    e.preventDefault();
    const list = $("#suggest");
    if (!list.hidden && suggestAt >= 0 && list.children[suggestAt]) cmdInput.value = list.children[suggestAt].dataset.name;
    submit(cmdInput.value);
  } else if (k === "ArrowUp" || k === "ArrowDown") {
    e.preventDefault();
    const list = $("#suggest");
    if (!list.hidden && list.children.length) {
      suggestAt = (suggestAt + (k === "ArrowDown" ? 1 : -1) + list.children.length) % list.children.length;
      [...list.children].forEach((li, i) => li.classList.toggle("on", i === suggestAt));
    } else if (recall.length) {
      recallAt = Math.max(0, Math.min(recall.length - 1, recallAt + (k === "ArrowUp" ? 1 : -1)));
      cmdInput.value = recall[recall.length - 1 - recallAt] || "";
    }
  } else if (k === "Tab" && cursor.snaps.length > 1) {
    e.preventDefault();
    cycleSnap();
  }
}

function onTyping() {
  const v = cmdInput.value.trim().toUpperCase();
  const list = $("#suggest");
  suggestAt = -1;
  if (!v || runner?.req) {
    list.hidden = true;
    return;
  }
  const m = COMMANDS.filter((c) => !c.hidden && (c.name.startsWith(v) || c.aliases.some((a) => a.startsWith(v))))
    .sort((a, b) => (b.aliases.includes(v) ? 1 : 0) - (a.aliases.includes(v) ? 1 : 0))
    .slice(0, 8);
  list.innerHTML = m.map((c) => `<li data-name="${c.name}"><b>${c.aliases[0] || c.name}</b><span>${c.name} · ${esc(c.desc)}</span></li>`).join("");
  [...list.children].forEach((li) => (li.onclick = () => submit(li.dataset.name)));
  list.hidden = m.length === 0;
}

function escape() {
  if (Board.isOpen()) return Board.close();
  if (!$("#numpad").hidden) return closeNumpad();
  if (runner) cancelCommand(true);
  else if (selection.size) {
    selection.clear();
    selectionChanged();
  }
  $("#suggest").hidden = true;
  cmdInput.value = "";
  app.classList.remove("typing");
  $("#side").classList.remove("open");
}

// ---------------------------------------------------------------- command line

function log(text, cls = "") {
  const h = $("#history");
  const div = document.createElement("div");
  if (cls) div.className = cls;
  div.textContent = text;
  h.append(div);
  while (h.children.length > 200) h.firstChild.remove();
  h.scrollTop = h.scrollHeight;
}
function logError(msg) {
  log(msg, "err");
  if (PHONE.matches) toast(msg, true);
}

function promptHtml() {
  const req = runner?.req;
  if (!runner) return "Command:";
  const opts = (req?.options || []).map((o) => `<span class="opt" data-opt="${o}">${o}</span>`).join(" ");
  let text = esc(req?.prompt || "");
  if (req?.type === "select" && selection.size) text += ` (${selection.size} selected)`;
  return `<span class="p">${runner.name}</span> ${text}${opts ? ` [${opts}]` : ""}${req?.def != null ? ` <span class="def">&lt;${esc(req.def)}&gt;</span>` : ""}`;
}

function setPrompt() {
  const p = $("#prompt");
  p.innerHTML = promptHtml();
  p.querySelectorAll(".opt").forEach((o) => (o.onclick = () => submit(o.dataset.opt)));
  const active = runner ? (runner.name === "GRIP" ? "" : runner.name) : "SELECT";
  document.querySelectorAll("[data-tool]").forEach((t) => t.classList.toggle("on", t.dataset.tool === active));
  renderSheet();
}

/** Enter on the command line (or Space, or a right-click). */
function submit(raw) {
  const text = raw.trim();
  cmdInput.value = "";
  $("#suggest").hidden = true;
  recallAt = -1;
  if (text) recall.push(text);
  if (runner?.req) {
    log(`${$("#prompt").textContent} ${text}`);
    answerText(runner.req.type === "text" ? raw : text);
    return;
  }
  if (!text) {
    if (lastCommand) startCommand(lastCommand);
    return;
  }
  runLine(text);
}

function runLine(text) {
  const word = text.split(/\s+/)[0].toUpperCase();
  const c = COMMANDS.find((c) => c.name === word || c.aliases.includes(word));
  if (!c) return logError(`Unknown command "${word}". Type a name or alias; matches show as you type.`);
  startCommand(c.name);
}

// ---------------------------------------------------------------- commands

/**
 * A running command is a generator that yields requests and gets back
 * the answers. A request is {type: "point"|"distance"|"angle"|"text"|
 * "select"|"option", prompt, options, def, base, preview, ghost}. `null`
 * answers Enter.
 */
function startCommand(name, gen = null) {
  hideMenu();
  const c = gen ? { name, run: gen, needsDoc: true, repeat: false } : COMMANDS.find((c) => c.name === name);
  if (!c) return;
  if (c.needsDoc && !doc) return logError("Open or start a drawing first.");
  if (c.instant) {
    log(name, "ok");
    try {
      c.run();
    } catch (e) {
      handleError(e);
    }
    return;
  }
  if (runner) cancelCommand(false);
  if (name !== "GRIP") log(`Command: ${name}`, "ok");
  if (c.repeat !== false) lastCommand = name;
  runner = { name, gen: c.run(helpers()), req: null };
  step(undefined);
}

function step(answer) {
  if (!runner) return;
  const r = runner;
  let res;
  try {
    res = r.gen.next(answer);
  } catch (e) {
    runner = null;
    handleError(e);
    finishCommand();
    return;
  }
  if (runner !== r) return;
  if (res.done) {
    runner = null;
    finishCommand();
    return;
  }
  r.req = res.value;
  cursor.snapIndex = 0;
  setPrompt();
  updateCursor();
}

function finishCommand() {
  setPrompt();
  if (doc) refresh(doc);
  needsDraw = true;
  updateCursor();
}

function cancelCommand(announce) {
  if (!runner) return setPrompt();
  const r = runner;
  runner = null;
  try {
    r.gen.return();
  } catch (e) {
    handleError(e);
  }
  if (announce) log("*Cancel*");
  finishCommand();
}

function handleError(e) {
  if (e instanceof CoreCrash) return crashed(e);
  logError(e.message || String(e));
}

let restarting = null;
/** The core stopped: start a fresh one and reopen every drawing from its
 *  last saved bytes (or as it began, if never saved). */
function crashed(e) {
  if (!restarting) restarting = recover(e).finally(() => (restarting = null));
  return restarting;
}

async function recover(e) {
  const why = e.outOfMemory ? "ran out of memory" : "stopped on a bug";
  logError(`The core ${why}: ${e.message}`);
  toast(`The core ${why} and was restarted. Unsaved changes were lost; drawings were reopened as last saved.`, true);
  runner = null;
  try {
    await core.restart();
  } catch (err) {
    logError(`The core could not be restarted: ${err.message}. Reload the page.`);
    return;
  }
  for (const d of [...docs]) {
    try {
      d.id = d.orig ? core.open(d.orig) : d.sample ? core.sample() : core.newDrawing(!!d.imperial);
      d.disp = prepare(core.display(d.id));
      d.info = core.info(d.id);
    } catch (_) {
      docs.splice(docs.indexOf(d), 1);
    }
  }
  activate(docs.includes(doc) ? doc : docs[0] || null);
}

function answerPoint(p) {
  const req = runner?.req;
  if (!req) return;
  p.snap = cursor.snap?.kind || 0;
  if (req.type === "point") {
    req.forceSnap = undefined;
    step(p);
  } else if (req.type === "distance" && req.base) step(Math.hypot(p[0] - req.base[0], p[1] - req.base[1]));
  else if (req.type === "angle" && req.base) step(Math.atan2(p[1] - req.base[1], p[0] - req.base[0]));
}

/** Typed input at a prompt: options, snap keywords, points, lengths, text. */
function answerText(text) {
  const req = runner.req;
  const up = text.trim().toUpperCase();
  if (req.type === "text") return step(text.length ? text : null);
  if (!up) return step(null);
  for (const o of req.options || []) {
    const key = o.match(/[A-Z0-9]/)?.[0];
    if (up === o.toUpperCase() || up === key) return step({ option: o });
  }
  if (req.type === "select") return logError("Pick objects, or press Enter to finish selecting.");
  if (req.type === "point" && SNAP_KINDS[up] !== undefined) {
    req.forceSnap = SNAP_KINDS[up];
    log(`(${up} for the next pick)`);
    return updateCursor();
  }
  if (req.type === "point" && up === "NON") {
    req.forceSnap = 0;
    return updateCursor();
  }
  if (req.type === "option") return logError(`Choose ${req.options.join(", ")}.`);
  if (req.type === "angle") {
    const a = core.query(doc.id, `angle ${text}`);
    return a ? step(a.value) : logError(`"${text}" is not an angle.`);
  }
  if (req.type === "distance") {
    const v = length(text);
    return v === null ? logError(`"${text}" is not a length.`) : step(v);
  }
  const p = parsePoint(text, req);
  if (p === undefined) return;
  if (!p) return logError(`"${text}" is not a point. Type x,y or @dx,dy or @length<angle, or a length along the cursor.`);
  step(p);
}

function length(t) {
  const r = core.query(doc.id, `len ${t}`);
  return r ? r.value : null;
}

/** COMMANDS.md §17: x,y · @dx,dy · @len<angle · <angle · a length along the cursor. */
function parsePoint(text, req) {
  const t = text.trim();
  const base = req.base || null;
  if (t.startsWith("<")) {
    const a = core.query(doc.id, `angle ${t.slice(1)}`);
    if (!a) return null;
    req.angleLock = a.value;
    log(`Angle locked at ${t.slice(1)}°`);
    updateCursor();
    return undefined;
  }
  const rel = t.startsWith("@");
  const body = rel ? t.slice(1) : t;
  if (rel && !base) return null;
  const polar = body.split("<");
  if (polar.length === 2) {
    const d = length(polar[0]);
    const a = core.query(doc.id, `angle ${polar[1]}`);
    if (d === null || !a) return null;
    const o = rel ? base : [0, 0];
    return [o[0] + d * Math.cos(a.value), o[1] + d * Math.sin(a.value)];
  }
  const parts = body.split(",");
  if (parts.length === 2) {
    const x = length(parts[0]), y = length(parts[1]);
    if (x === null || y === null) return null;
    return rel ? [base[0] + x, base[1] + y] : [x, y];
  }
  if (parts.length === 1 && base) {
    const d = length(body);
    if (d === null) return null;
    const p = currentPoint();
    const dx = p[0] - base[0], dy = p[1] - base[1];
    const n = Math.hypot(dx, dy);
    if (n === 0) return null;
    return [base[0] + (dx / n) * d, base[1] + (dy / n) * d];
  }
  return null;
}

function helpers() {
  const exec = (line) => core.exec(doc.id, line);
  return {
    point: (prompt, o = {}) => ({ type: "point", prompt, ...o }),
    distance: (prompt, o = {}) => ({ type: "distance", prompt, ...o }),
    angle: (prompt, o = {}) => ({ type: "angle", prompt, ...o }),
    text: (prompt, o = {}) => ({ type: "text", prompt, ...o }),
    option: (prompt, options, o = {}) => ({ type: "option", prompt, options, ...o }),
    select: (prompt = "Select objects") => ({ type: "select", prompt, options: [] }),
    exec,
    /** An edit; on failure the message is shown and null returned. */
    edit: (line) => {
      try {
        const r = exec(line);
        refresh(doc);
        return r;
      } catch (e) {
        if (e instanceof CoreCrash) throw e;
        logError(e.message.replace(/^./, (c) => c.toUpperCase()));
        return null;
      }
    },
    log,
    fmt,
  };
}

const N = (v) => +(+v).toPrecision(15);
const P = (p) => `${N(p[0])} ${N(p[1])}`;
const deg = (r) => N((r * 180) / Math.PI);

/** The selection, or ask for one. */
function* selected(c) {
  if (!selection.size) {
    for (;;) if ((yield c.select()) === null) break;
  }
  return [...selection];
}

function* cmdLine(c) {
  const first = yield c.point("Specify first point");
  if (!first) return;
  c.exec("begin");
  try {
    const pts = [first];
    const made = [];
    for (;;) {
      const last = pts[pts.length - 1];
      const r = yield c.point("Specify next point", { base: last, options: pts.length >= 3 ? ["Close", "Undo"] : ["Undo"], preview: (q) => [{ t: "line", a: last, b: q }] });
      if (r === null) break;
      if (r.option === "Undo") {
        if (made.length) {
          c.edit(`erase ${made.pop()}`);
          pts.pop();
        }
        continue;
      }
      if (r.option === "Close") {
        c.edit(`line ${P(last)} ${P(pts[0])}`);
        break;
      }
      const res = c.edit(`line ${P(last)} ${P(r)}`);
      if (res) {
        made.push(res.handles[0]);
        pts.push(r);
      }
    }
  } finally {
    c.exec("end");
  }
}

function* cmdPline(c) {
  const first = yield c.point("Specify start point");
  if (!first) return;
  const pts = [first];
  let closed = false;
  for (;;) {
    const last = pts[pts.length - 1];
    const r = yield c.point("Specify next point", { base: last, options: pts.length >= 3 ? ["Close", "Undo"] : ["Undo"], preview: (q) => [{ t: "poly", pts: [...pts, q] }] });
    if (r === null) break;
    if (r.option === "Undo") {
      if (pts.length > 1) pts.pop();
      continue;
    }
    if (r.option === "Close") {
      closed = true;
      break;
    }
    pts.push(r);
  }
  if (pts.length < 2) return;
  c.edit(`pline ${closed ? 1 : 0} ${pts.map((p) => `${P(p)} 0`).join(" ")}`);
}

function* cmdCircle(c) {
  const center = yield c.point("Specify center point");
  if (!center) return;
  let diameter = false;
  for (;;) {
    const r = yield c.distance(diameter ? "Specify diameter" : "Specify radius", {
      base: center,
      options: diameter ? ["Radius"] : ["Diameter"],
      preview: (q) => {
        const d = Math.hypot(q[0] - center[0], q[1] - center[1]);
        return [{ t: "circle", c: center, r: diameter ? d / 2 : d }, { t: "line", a: center, b: q }];
      },
    });
    if (r === null) return;
    if (r.option) {
      diameter = r.option === "Diameter";
      continue;
    }
    c.edit(`circle ${P(center)} ${N(diameter ? r / 2 : r)}`);
    return;
  }
}

/** The arc from a over b to p, for the rubber band only: the core makes
 *  the arc itself (arc-3p, cad_core::construct::arc_through). */
function arc3(a, b, p) {
  const d = 2 * (a[0] * (b[1] - p[1]) + b[0] * (p[1] - a[1]) + p[0] * (a[1] - b[1]));
  if (Math.abs(d) < 1e-12) return null;
  const s = (q) => q[0] * q[0] + q[1] * q[1];
  const ux = (s(a) * (b[1] - p[1]) + s(b) * (p[1] - a[1]) + s(p) * (a[1] - b[1])) / d;
  const uy = (s(a) * (p[0] - b[0]) + s(b) * (a[0] - p[0]) + s(p) * (b[0] - a[0])) / d;
  const r = Math.hypot(a[0] - ux, a[1] - uy);
  const ang = (q) => Math.atan2(q[1] - uy, q[0] - ux);
  const ccw = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) > 0;
  const [a0, a1] = ccw ? [ang(a), ang(p)] : [ang(p), ang(a)];
  return { c: [ux, uy], r, a0, a1: a1 < a0 ? a1 + TAU : a1 };
}

function* cmdArc(c) {
  const a = yield c.point("Specify start point");
  if (!a) return;
  const b = yield c.point("Specify second point", { base: a, preview: (q) => [{ t: "line", a, b: q }] });
  if (!b) return;
  const p = yield c.point("Specify end point", {
    base: b,
    preview: (q) => {
      const arc = arc3(a, b, q);
      return arc ? [{ t: "arc", ...arc }] : [{ t: "poly", pts: [a, b, q] }];
    },
  });
  if (p) c.edit(`arc-3p ${P(a)} ${P(b)} ${P(p)}`);
}

function* cmdRect(c) {
  const a = yield c.point("Specify first corner");
  if (!a) return;
  const b = yield c.point("Specify other corner", { base: a, preview: (q) => [{ t: "poly", pts: [a, [q[0], a[1]], q, [a[0], q[1]]], closed: true }] });
  if (b) c.edit(`rect ${P(a)} ${P(b)}`);
}

function* cmdText(c) {
  const at = yield c.point("Specify start point");
  if (!at) return;
  const defH = textHeight();
  let h = yield c.distance("Specify height", { base: at, def: fmt(defH), preview: (q) => [{ t: "line", a: at, b: q }] });
  if (h === null) h = defH;
  let rot = yield c.angle("Specify rotation angle", { base: at, def: "0" });
  if (rot === null) rot = 0;
  const words = yield c.text("Enter text");
  if (words) c.edit(`text ${P(at)} ${N(h)} ${deg(rot)} ${words}`);
}

function ghostReq(c, prompt, base, hs, make) {
  const req = c.point(prompt, { base, ghost: { handles: hs, xf: null } });
  req.preview = (q) => {
    req.ghost.xf = make(q);
    return [{ t: "line", a: base, b: q }];
  };
  return req;
}

function* cmdMove(c) {
  const hs = yield* selected(c);
  if (!hs.length) return;
  const base = yield c.point("Specify base point");
  if (!base) return;
  const to = yield ghostReq(c, "Specify second point", base, hs, (q) => xfMove(q[0] - base[0], q[1] - base[1]));
  if (to && c.edit(`move ${N(to[0] - base[0])} ${N(to[1] - base[1])} ${hs.join(" ")}`)) selection.clear();
}

function* cmdCopy(c) {
  const hs = yield* selected(c);
  if (!hs.length) return;
  const base = yield c.point("Specify base point");
  if (!base) return;
  c.exec("begin");
  try {
    for (;;) {
      const to = yield ghostReq(c, "Specify second point", base, hs, (q) => xfMove(q[0] - base[0], q[1] - base[1]));
      if (!to) break;
      c.edit(`copy ${N(to[0] - base[0])} ${N(to[1] - base[1])} ${hs.join(" ")}`);
    }
  } finally {
    c.exec("end");
    selection.clear();
  }
}

function* cmdRotate(c) {
  const hs = yield* selected(c);
  if (!hs.length) return;
  const base = yield c.point("Specify base point");
  if (!base) return;
  const req = c.angle("Specify rotation angle", { base, ghost: { handles: hs, xf: null } });
  req.preview = (q) => {
    req.ghost.xf = xfRotate(base[0], base[1], Math.atan2(q[1] - base[1], q[0] - base[0]));
    return [{ t: "line", a: base, b: q }];
  };
  const a = yield req;
  if (a !== null && c.edit(`rotate ${P(base)} ${deg(a)} ${hs.join(" ")}`)) selection.clear();
}

function* cmdMirror(c) {
  const hs = yield* selected(c);
  if (!hs.length) return;
  const a = yield c.point("Specify first point of mirror line");
  if (!a) return;
  const req = c.point("Specify second point of mirror line", { base: a, ghost: { handles: hs, xf: null } });
  req.preview = (q) => {
    if (Math.hypot(q[0] - a[0], q[1] - a[1]) > 0) req.ghost.xf = xfMirror(a, q);
    return [{ t: "line", a, b: q }];
  };
  const b = yield req;
  if (!b) return;
  const r = yield c.option("Erase source objects?", ["Yes", "No"], { def: "No" });
  const keep = !(r && r.option === "Yes");
  if (c.edit(`mirror ${P(a)} ${P(b)} ${keep ? 1 : 0} ${hs.join(" ")}`)) selection.clear();
}

function* cmdErase(c) {
  const hs = yield* selected(c);
  if (!hs.length) return;
  if (c.edit(`erase ${hs.join(" ")}`)) {
    selection.clear();
    c.log(`${hs.length} erased`);
  }
}

/** The circle or arc under a picked point, if any. */
function circleAt(p) {
  const r = aperturePx() / doc.view.scale;
  const h = core.query(doc.id, `pick ${p[0]} ${p[1]} ${r}`)?.handle;
  const g = h && core.query(doc.id, `get ${h}`);
  return g && (g.type === "CIRCLE" || g.type === "ARC") ? g : null;
}

/** A dimension's look while it is placed: extension lines and the dimension line. */
function dimPreview(p1, p2, loc, aligned) {
  let u = aligned ? [p2[0] - p1[0], p2[1] - p1[1]] : linearDir(p1, p2, loc);
  const L = Math.hypot(u[0], u[1]) || 1;
  u = [u[0] / L, u[1] / L];
  const proj = (p) => {
    const t = (p[0] - loc[0]) * u[0] + (p[1] - loc[1]) * u[1];
    return [loc[0] + u[0] * t, loc[1] + u[1] * t];
  };
  const a = proj(p1), b = proj(p2);
  return [{ t: "line", a: p1, b: a }, { t: "line", a: p2, b }, { t: "line", a, b }];
}

/** Horizontal or vertical, by where the dimension line goes, for the
 *  rubber band; the core decides for the dimension itself
 *  (cad_core::construct::linear_is_vertical). */
function linearDir(p1, p2, loc) {
  const minX = Math.min(p1[0], p2[0]), maxX = Math.max(p1[0], p2[0]);
  const minY = Math.min(p1[1], p2[1]), maxY = Math.max(p1[1], p2[1]);
  const outY = loc[1] > maxY ? loc[1] - maxY : loc[1] < minY ? minY - loc[1] : 0;
  const outX = loc[0] > maxX ? loc[0] - maxX : loc[0] < minX ? minX - loc[0] : 0;
  return outX > outY ? [0, 1] : [1, 0];
}

/** Smart DIM: a circle or arc gives a diameter or radius, two points a linear dimension. */
function* cmdDim(c, mode = "smart") {
  const first = yield c.point(mode === "radius" || mode === "diameter" ? "Select arc or circle" : "Specify first point or select a circle");
  if (!first) return;
  const circ = mode === "linear" || mode === "aligned" ? null : first.snap && first.snap !== 32 ? null : circleAt(first);
  if (circ || mode === "radius" || mode === "diameter") {
    if (!circ) return c.log("That is not an arc or a circle.");
    const cen = circ.center;
    const radius = mode === "radius" || (mode === "smart" && circ.type === "ARC");
    const loc = yield c.point("Specify dimension line location", {
      base: cen,
      preview: (q) => {
        const t = Math.atan2(q[1] - cen[1], q[0] - cen[0]);
        const far = [cen[0] + circ.radius * Math.cos(t), cen[1] + circ.radius * Math.sin(t)];
        const near = radius ? cen : [cen[0] - circ.radius * Math.cos(t), cen[1] - circ.radius * Math.sin(t)];
        return [{ t: "line", a: near, b: far }];
      },
    });
    if (!loc) return;
    const r = c.edit(`dim-${radius ? "radius" : "diameter"} ${circ.handle} ${P(loc)}`);
    if (r) doc.lastDim = r.handles[0];
    return;
  }
  const second = yield c.point("Specify second point", { base: first, preview: (q) => [{ t: "line", a: first, b: q }] });
  if (!second) return;
  let aligned = mode === "aligned";
  for (;;) {
    const loc = yield c.point("Specify dimension line location", {
      base: second,
      options: aligned ? ["Linear"] : ["Aligned"],
      preview: (q) => dimPreview(first, second, q, aligned),
    });
    if (!loc) return;
    if (loc.option) {
      aligned = loc.option === "Aligned";
      continue;
    }
    const r = c.edit(`dim-${aligned ? "aligned" : "linear"} ${P(first)} ${P(second)} ${P(loc)}`);
    if (r) doc.lastDim = r.handles[0];
    return;
  }
}

function* cmdDist(c) {
  const a = yield c.point("Specify first point");
  if (!a) return;
  const b = yield c.point("Specify second point", { base: a, preview: (q) => [{ t: "line", a, b: q }] });
  if (!b) return;
  const msg = core.query(doc.id, `measure ${P(a)} ${P(b)}`).text;
  c.log(msg);
  if (PHONE.matches) toast(msg);
}

function* cmdId(c) {
  const p = yield c.point("Specify point");
  if (p) c.log(`X = ${fmt(p[0])}   Y = ${fmt(p[1])}`);
}

function* cmdZoom(c) {
  const r = yield c.option("Specify an option", ["Extents", "Window", "In", "Out"], { def: "Extents" });
  if (r === null || r.option === "Extents") return zoomExtents();
  if (r.option === "In") return zoomBy(2);
  if (r.option === "Out") return zoomBy(0.5);
  const a = yield c.point("Specify first corner");
  if (!a) return;
  const b = yield c.point("Specify opposite corner", { base: a, preview: (q) => [{ t: "poly", pts: [a, [q[0], a[1]], q, [a[0], q[1]]], closed: true }] });
  if (b) zoomWindow(a, b);
}

function* cmdPan(c) {
  yield c.option("Drag to pan; Esc or Enter to exit", []);
}

function* cmdSavedBy(c) {
  const name = yield c.text('Your name, recorded as "last saved by"', { def: user() || "none" });
  if (name === null) return;
  try {
    localStorage.setItem("cad.user", name.trim());
  } catch (_) {}
  c.log(`Saves will record "${name.trim()}".`);
  updateChrome();
}

// ---------------------------------------------------------------- drafting tools
// Modify, construction, annotation, blocks, settings, layouts and plotting.
// Each asks for points, objects and values as AutoCAD's command line does
// and hands the work to the core in one line (crates/cad-ffi/src/commands.rs).

/** A request to pick an object: no snaps, and what would be picked glows. */
function pickReq(c, prompt, o = {}) {
  return c.point(prompt, { pick: true, ...o });
}

/** The object at a picked point, if any. */
function objectAt(p) {
  const r = aperturePx() / doc.view.scale;
  return core.query(doc.id, `pick ${p[0]} ${p[1]} ${r}`)?.handle ?? null;
}

function getObj(h) {
  return h ? core.query(doc.id, `get ${h}`) : null;
}

/** A whole number typed at a prompt, or the default. */
function* askInt(c, prompt, def, min = 1) {
  for (;;) {
    const t = yield c.text(prompt, { def: String(def) });
    if (t === null || !t.trim()) return def;
    const v = Number(t.trim());
    if (Number.isInteger(v) && v >= min) return v;
    c.log(`Type a whole number, ${min} or more.`);
  }
}

/** A plain number typed at a prompt (a scale factor), or the default. */
function* askNumber(c, prompt, def, o = {}) {
  for (;;) {
    const r = yield c.distance(prompt, { def: String(def), ...o });
    if (r === null) return def;
    if (r.option) return r;
    if (r > 0) return r;
    c.log("Type a number more than zero.");
  }
}

function xfScale(cx, cy, k) {
  return { a: k, b: 0, c: 0, d: k, e: cx - k * cx, f: cy - k * cy };
}

const setting = (key, def) => (doc[key] === undefined ? def : doc[key]);

function* cmdOffset(c) {
  let through = setting("offsetThrough", false);
  let dist = setting("offsetDist", null);
  const r = yield c.distance("Specify offset distance", { options: ["Through"], def: through || dist === null ? "Through" : fmt(dist) });
  if (r === null) {
    if (!through && dist === null) through = true;
  } else if (r.option === "Through") through = true;
  else {
    through = false;
    dist = r;
  }
  if (!through && !(dist > 0)) return c.log("The offset distance must be more than zero.");
  Object.assign(doc, { offsetThrough: through, offsetDist: dist });
  c.exec("begin");
  try {
    for (;;) {
      const p = yield pickReq(c, "Select object to offset", { options: ["Exit"] });
      if (!p || p.option) break;
      const h = objectAt(p);
      if (!h) {
        c.log("No object there.");
        continue;
      }
      const side = yield c.point(through ? "Specify through point" : "Specify point on side to offset", { base: p });
      if (!side) break;
      c.edit(through ? `offset-through ${P(side)} ${h}` : `offset ${N(dist)} ${P(side)} ${h}`);
    }
  } finally {
    c.exec("end");
  }
}

/** TRIM and EXTEND: every visible object is an edge (quick mode), or the
 *  edges chosen with the Edges option. */
function* cmdTrimExtend(c, extend) {
  let edges = [];
  const verb = extend ? "extend" : "trim";
  c.exec("begin");
  try {
    for (;;) {
      const p = yield pickReq(c, `Select object to ${verb}${edges.length ? ` (${edges.length} edges)` : ", all objects are edges"}`, { options: ["Edges"] });
      if (!p) break;
      if (p.option === "Edges") {
        selection.clear();
        selectionChanged();
        for (;;) if ((yield c.select(`Select ${extend ? "boundary" : "cutting"} edges`)) === null) break;
        edges = [...selection];
        selection.clear();
        selectionChanged();
        continue;
      }
      const h = objectAt(p);
      if (!h) {
        c.log("No object there.");
        continue;
      }
      c.edit(`${verb} ${P(p)} ${h} ${edges.join(" ")}`);
    }
  } finally {
    c.exec("end");
  }
}

function* cmdFillet(c, chamfer) {
  let r = setting("filletRadius", 0);
  let d = setting("chamferDist", [0, 0]);
  const what = () => (chamfer ? `distances ${fmt(d[0])}, ${fmt(d[1])}` : `radius ${fmt(r)}`);
  for (;;) {
    const p1 = yield pickReq(c, `Select first ${chamfer ? "line" : "object"} (${what()})`, { options: [chamfer ? "Distance" : "Radius", "Polyline"] });
    if (!p1) return;
    if (p1.option === "Radius") {
      const v = yield c.distance("Specify fillet radius", { def: fmt(r) });
      if (v !== null && !v.option) r = doc.filletRadius = v;
      continue;
    }
    if (p1.option === "Distance") {
      const a = yield c.distance("Specify first chamfer distance", { def: fmt(d[0]) });
      if (a === null) continue;
      const b = yield c.distance("Specify second chamfer distance", { def: fmt(a) });
      d = doc.chamferDist = [a, b === null ? a : b];
      continue;
    }
    if (p1.option === "Polyline") {
      const q = yield pickReq(c, "Select polyline");
      const h = q && objectAt(q);
      if (!h) return c.log("No polyline there.");
      if (c.edit(chamfer ? `chamfer-pline ${N(d[0])} ${N(d[1])} ${h}` : `fillet-pline ${N(r)} ${h}`)) c.log("Corners done.");
      return;
    }
    const h1 = objectAt(p1);
    if (!h1) {
      c.log("No object there.");
      continue;
    }
    selection.clear();
    selection.add(h1);
    selectionChanged();
    const p2 = yield pickReq(c, `Select second ${chamfer ? "line" : "object"}`);
    selection.clear();
    selectionChanged();
    if (!p2) return;
    const h2 = objectAt(p2);
    if (!h2) return c.log("No object there.");
    c.edit(chamfer ? `chamfer ${N(d[0])} ${N(d[1])} ${P(p1)} ${h1} ${P(p2)} ${h2}` : `fillet ${N(r)} ${P(p1)} ${h1} ${P(p2)} ${h2}`);
    return;
  }
}

function* cmdArray(c, kind = null) {
  const hs = yield* selected(c);
  if (!hs.length) return;
  if (!kind) {
    const r = yield c.option("Enter array type", ["Rectangular", "Polar"], { def: "Rectangular" });
    kind = r?.option === "Polar" ? "polar" : "rect";
  }
  if (kind === "rect") {
    const rows = yield* askInt(c, "Enter number of rows", 3);
    const cols = yield* askInt(c, "Enter number of columns", 4);
    // The core offers spacing that clears the selection.
    let { rowGap, columnGap: colGap } = core.query(doc.id, `bounds ${hs.join(" ")}`);
    if (rows > 1) {
      const v = yield c.distance("Specify the distance between rows", { def: fmt(rowGap) });
      if (v !== null) rowGap = v;
    }
    if (cols > 1) {
      const v = yield c.distance("Specify the distance between columns", { def: fmt(colGap) });
      if (v !== null) colGap = v;
    }
    if (c.edit(`array-rect ${rows} ${cols} ${N(rowGap)} ${N(colGap)} ${hs.join(" ")}`)) selection.clear();
    return;
  }
  const center = yield c.point("Specify center point of array");
  if (!center) return;
  const count = yield* askInt(c, "Enter number of items", 6, 2);
  const fillReq = c.angle("Specify the angle to fill (+ counter-clockwise, - clockwise)", { def: "360" });
  let fill = yield fillReq;
  fill = fill === null ? 360 : deg(fill);
  if (fill === 0) fill = 360;
  const r = yield c.option("Rotate arrayed items?", ["Yes", "No"], { def: "Yes" });
  const rotate = !(r && r.option === "No");
  if (c.edit(`array-polar ${P(center)} ${count} ${N(fill)} ${rotate ? 1 : 0} ${hs.join(" ")}`)) selection.clear();
}

function* cmdScale(c) {
  const hs = yield* selected(c);
  if (!hs.length) return;
  const base = yield c.point("Specify base point");
  if (!base) return;
  for (;;) {
    const req = c.distance("Specify scale factor", { base, options: ["Reference"], ghost: { handles: hs, xf: null } });
    req.preview = (q) => {
      const k = Math.hypot(q[0] - base[0], q[1] - base[1]);
      if (k > 0) req.ghost.xf = xfScale(base[0], base[1], k);
      return [{ t: "line", a: base, b: q }];
    };
    const k = yield req;
    if (k === null) return;
    if (k.option === "Reference") {
      const ref = yield c.distance("Specify reference length", { def: "1" });
      if (ref === null) return;
      const nl = yield c.distance("Specify new length", { base });
      if (nl === null) return;
      if (c.edit(`scale-ref ${P(base)} ${N(ref)} ${N(nl)} ${hs.join(" ")}`)) selection.clear();
      return;
    }
    if (c.edit(`scale ${P(base)} ${N(k)} ${hs.join(" ")}`)) selection.clear();
    return;
  }
}

function* cmdStretch(c) {
  c.log("Select the objects to stretch with a crossing window: what is inside moves, what crosses it stretches.");
  const a = yield c.point("Specify first corner of crossing window");
  if (!a) return;
  const b = yield c.point("Specify opposite corner", { base: a, preview: (q) => [{ t: "poly", pts: [a, [q[0], a[1]], q, [a[0], q[1]]], closed: true }] });
  if (!b) return;
  const hs = core.query(doc.id, `box ${a[0]} ${a[1]} ${b[0]} ${b[1]} 1`)?.handles || [];
  if (!hs.length) return c.log("Nothing crosses that window.");
  hs.forEach((h) => selection.add(h));
  selectionChanged();
  const base = yield c.point(`Specify base point (${hs.length} found)`);
  if (!base) return void selection.clear();
  const to = yield c.point("Specify second point", { base, preview: (q) => [{ t: "line", a: base, b: q }] });
  selection.clear();
  selectionChanged();
  if (to) c.edit(`stretch ${P(a)} ${P(b)} ${N(to[0] - base[0])} ${N(to[1] - base[1])} ${hs.join(" ")}`);
}

function* cmdExplode(c) {
  const hs = yield* selected(c);
  if (!hs.length) return;
  const r = c.edit(`explode ${hs.join(" ")}`);
  if (r) {
    selection.clear();
    c.log(`${r.handles.length} objects`);
  }
}

function* cmdXline(c, ray) {
  if (ray) {
    const a = yield c.point("Specify start point");
    if (!a) return;
    c.exec("begin");
    try {
      for (;;) {
        const b = yield c.point("Specify through point", { base: a, preview: (q) => [{ t: "line", a, b: [a[0] + (q[0] - a[0]) * 50, a[1] + (q[1] - a[1]) * 50] }] });
        if (!b) break;
        c.edit(`ray ${P(a)} ${P(b)}`);
      }
    } finally {
      c.exec("end");
    }
    return;
  }
  let mode = "point", angle = 0;
  let a = yield c.point("Specify a point", { options: ["Hor", "Ver", "Ang"] });
  if (!a) return;
  if (a.option === "Hor" || a.option === "Ver") mode = a.option;
  else if (a.option === "Ang") {
    const t = yield c.angle("Enter angle of xline", { def: "0" });
    angle = t === null ? 0 : t;
    mode = "Ang";
  }
  c.exec("begin");
  try {
    const long = (p, u) => [{ t: "line", a: [p[0] - u[0] * 1e4, p[1] - u[1] * 1e4], b: [p[0] + u[0] * 1e4, p[1] + u[1] * 1e4] }];
    for (;;) {
      if (mode === "point") {
        const b = yield c.point("Specify through point", { base: a, preview: (q) => { const L = Math.hypot(q[0] - a[0], q[1] - a[1]) || 1; return long(a, [(q[0] - a[0]) / L, (q[1] - a[1]) / L]); } });
        if (!b) break;
        c.edit(`xline ${P(a)} ${P(b)}`);
      } else {
        const a = mode === "Hor" ? 0 : mode === "Ver" ? Math.PI / 2 : angle;
        const u = [Math.cos(a), Math.sin(a)];
        const p = yield c.point("Specify through point", { preview: (q) => long(q, u) });
        if (!p) break;
        c.edit(`xline-angle ${P(p)} ${deg(a)}`);
      }
    }
  } finally {
    c.exec("end");
  }
}

function* cmdHatch(c) {
  let pattern = setting("hatchPattern", "ANSI31");
  let scale = setting("hatchScale", 1);
  let angle = setting("hatchAngle", 0);
  c.exec("begin");
  try {
    for (;;) {
      const p = yield pickReq(c, `Pick a point inside the area (${pattern}${pattern === "SOLID" ? "" : `, scale ${+scale.toPrecision(6)}, ${deg(angle)}°`})`, { options: ["Pattern", "Scale", "Angle", "Select objects"] });
      if (!p) break;
      if (p.option === "Pattern") {
        const names = (core.query(doc.id, "patterns")?.patterns || []).map((x) => x.name);
        c.log(`Patterns: ${names.join(", ")}`);
        const t = yield c.text("Enter a pattern name", { def: pattern });
        if (t && t.trim()) {
          const up = t.trim().toUpperCase();
          if (names.includes(up)) pattern = doc.hatchPattern = up;
          else c.log(`There is no pattern ${up}.`);
        }
        continue;
      }
      if (p.option === "Scale") {
        const v = yield* askNumber(c, pattern === "USER" ? "Specify the spacing of the lines" : "Specify the pattern scale", scale);
        if (typeof v === "number") scale = doc.hatchScale = v;
        continue;
      }
      if (p.option === "Angle") {
        const a = yield c.angle("Specify the pattern angle", { def: String(deg(angle)) });
        if (a !== null) angle = doc.hatchAngle = a;
        continue;
      }
      if (p.option === "Select objects") {
        selection.clear();
        selectionChanged();
        for (;;) if ((yield c.select("Select closed objects to hatch")) === null) break;
        const hs = [...selection];
        selection.clear();
        selectionChanged();
        if (hs.length) c.edit(`hatch-objects ${pattern} ${N(scale)} ${deg(angle)} 0 ${hs.join(" ")}`);
        continue;
      }
      c.edit(`hatch ${pattern} ${N(scale)} ${deg(angle)} 0 ${P(p)}`);
    }
  } finally {
    c.exec("end");
  }
}

function textHeight() {
  return doc.info.textHeight; // cad_core::standards::text_height
}

/** Lines of text: each Enter adds a line, an empty one finishes. */
function* askLines(c, prompt) {
  const lines = [];
  for (;;) {
    const t = yield c.text(lines.length ? "Enter the next line, or Enter to finish" : prompt);
    if (t === null || !t.length) break;
    lines.push(t.replace(/\|/g, "/"));
  }
  return lines;
}

function* cmdMtext(c) {
  const a = yield c.point("Specify first corner");
  if (!a) return;
  const b = yield c.point("Specify opposite corner", { base: a, options: ["Height"], preview: (q) => [{ t: "poly", pts: [a, [q[0], a[1]], q, [a[0], q[1]]], closed: true }] });
  if (!b) return;
  let h = setting("mtextHeight", textHeight());
  let corner = b;
  if (b.option === "Height") {
    const v = yield c.distance("Specify height", { base: a, def: fmt(h) });
    if (v !== null) h = doc.mtextHeight = v;
    corner = yield c.point("Specify opposite corner", { base: a, preview: (q) => [{ t: "poly", pts: [a, [q[0], a[1]], q, [a[0], q[1]]], closed: true }] });
    if (!corner) return;
  }
  const lines = yield* askLines(c, "Enter text");
  if (!lines.length) return;
  c.edit(`mtext-box ${P(a)} ${P(corner)} ${N(h)} 0 ${lines.join("\\P")}`);
}

function* cmdLeader(c, two = false) {
  const first = yield c.point(two ? "Specify leader arrowhead location" : "Specify leader start point");
  if (!first) return;
  const pts = [first];
  for (;;) {
    const last = pts[pts.length - 1];
    const q = yield c.point(two ? "Specify leader landing location" : pts.length === 1 ? "Specify next point" : "Specify next point, or Enter for the text", { base: last, preview: (p) => [{ t: "poly", pts: [...pts, p] }] });
    if (!q) break;
    pts.push(q);
    if (two) break;
  }
  if (pts.length < 2) return;
  const lines = yield* askLines(c, "Enter the note");
  if (!lines.length) return;
  c.edit(`leader ${N(textHeight())} ${pts.map(P).join(" ")} | ${lines.join("\\P")}`);
}

function* cmdDimAngular(c) {
  const first = yield pickReq(c, "Select arc, circle or line, or Enter to specify a vertex");
  const arcPreview = (cen, a0, a1) => (q) => {
    const r = Math.hypot(q[0] - cen[0], q[1] - cen[1]);
    return [{ t: "arc", c: cen, r, a0, a1: a1 < a0 ? a1 + TAU : a1 }];
  };
  const done = (r) => {
    if (r) doc.lastDim = r.handles[0];
  };
  if (first === null) {
    const v = yield c.point("Specify angle vertex");
    if (!v) return;
    const p1 = yield c.point("Specify first angle endpoint", { base: v, preview: (q) => [{ t: "line", a: v, b: q }] });
    if (!p1) return;
    const p2 = yield c.point("Specify second angle endpoint", { base: v, preview: (q) => [{ t: "line", a: v, b: p1 }, { t: "line", a: v, b: q }] });
    if (!p2) return;
    const a0 = Math.atan2(p1[1] - v[1], p1[0] - v[0]), a1 = Math.atan2(p2[1] - v[1], p2[0] - v[0]);
    const loc = yield c.point("Specify dimension arc line location", { base: v, preview: arcPreview(v, a0, a1) });
    if (loc) done(c.edit(`dim-angular ${P(v)} ${P(p1)} ${P(p2)} ${P(loc)}`));
    return;
  }
  const h = objectAt(first);
  const g = getObj(h);
  if (!g) return c.log("No object there.");
  if (g.type === "ARC") {
    const loc = yield c.point("Specify dimension arc line location", { base: g.center, preview: arcPreview(g.center, (g.start * Math.PI) / 180, (g.end * Math.PI) / 180) });
    if (loc) done(c.edit(`dim-angular-arc ${h} ${P(loc)}`));
    return;
  }
  if (g.type === "CIRCLE") {
    const p2 = yield c.point("Specify second angle endpoint", { base: g.center });
    if (!p2) return;
    const a0 = Math.atan2(first[1] - g.center[1], first[0] - g.center[0]), a1 = Math.atan2(p2[1] - g.center[1], p2[0] - g.center[0]);
    const loc = yield c.point("Specify dimension arc line location", { base: g.center, preview: arcPreview(g.center, a0, a1) });
    if (loc) done(c.edit(`dim-angular ${P(g.center)} ${P(first)} ${P(p2)} ${P(loc)}`));
    return;
  }
  if (g.type !== "LINE") return c.log("Pick a line, an arc or a circle, or press Enter to pick a vertex and two points.");
  const q = yield pickReq(c, "Select second line");
  const h2 = q && objectAt(q);
  const g2 = getObj(h2);
  if (!g2 || g2.type !== "LINE") return c.log("That is not a line.");
  const loc = yield c.point("Specify dimension arc line location");
  if (loc) done(c.edit(`dim-angular-lines ${h} ${h2} ${P(loc)}`));
}

function* cmdDimNext(c, baseline) {
  let base = doc.lastDim && getObj(doc.lastDim)?.type?.startsWith("DIMENSION") ? doc.lastDim : null;
  if (!base) {
    const p = yield pickReq(c, "Select base dimension");
    const h = p && objectAt(p);
    if (!h || !getObj(h)?.type?.startsWith("DIMENSION")) return c.log("That is not a dimension.");
    base = h;
  }
  c.exec("begin");
  try {
    for (;;) {
      const p = yield c.point("Specify a second extension line origin", { options: ["Select"] });
      if (!p) break;
      if (p.option === "Select") {
        const q = yield pickReq(c, "Select base dimension");
        const h = q && objectAt(q);
        if (h && getObj(h)?.type?.startsWith("DIMENSION")) base = h;
        else c.log("That is not a dimension.");
        continue;
      }
      const r = c.edit(`dim-${baseline ? "baseline" : "continue"} ${base} ${P(p)}`);
      if (r) base = doc.lastDim = r.handles[0];
    }
  } finally {
    c.exec("end");
  }
}

function* cmdBlock(c) {
  const name = yield c.text("Enter block name");
  if (!name || !name.trim()) return;
  const base = yield c.point("Specify insertion base point");
  if (!base) return;
  const hs = yield* selected(c);
  if (!hs.length) return;
  const r = c.edit(`block ${P(base)} convert ${hs.join(" ")} | ${name.trim()}`);
  if (r) {
    selection.clear();
    doc.lastBlock = name.trim();
    c.log(`Block ${name.trim()} made from ${hs.length} objects, and put back where they were.`);
  }
}

function* cmdInsert(c) {
  const list = core.query(doc.id, "blocks")?.blocks || [];
  if (!list.length) return c.log("The drawing has no blocks yet: make one with BLOCK.");
  c.log(`Blocks: ${list.map((b) => b.name).join(", ")}`);
  const def = doc.lastBlock && list.some((b) => b.name === doc.lastBlock) ? doc.lastBlock : list[0].name;
  const t = yield c.text("Enter block name", { def });
  const name = (t && t.trim()) || def;
  const blk = list.find((b) => b.name.toLowerCase() === name.toLowerCase());
  if (!blk) return c.log(`There is no block named ${name}.`);
  const at = yield c.point("Specify insertion point");
  if (!at) return;
  const s = yield* askNumber(c, "Specify scale factor", 1);
  if (typeof s !== "number") return;
  const rot = yield c.angle("Specify rotation angle", { base: at, def: "0" });
  const values = [];
  for (const a of blk.attributes) {
    const v = yield c.text(`${a.prompt || a.tag}`, { def: a.default });
    values.push(`${a.tag}=${(v === null ? a.default : v).replace(/\|/g, "/")}`);
  }
  const r = c.edit(`insert ${P(at)} ${N(s)} ${rot === null ? 0 : deg(rot)} | ${blk.name}${values.length ? " | " + values.join(" | ") : ""}`);
  if (r) doc.lastBlock = blk.name;
}

function* cmdAttdef(c) {
  const tag = yield c.text("Enter attribute tag (one word)");
  if (!tag || !tag.trim()) return;
  const prompt = yield c.text("Enter attribute prompt", { def: tag.trim() });
  const dflt = yield c.text("Enter default value", { def: "" });
  const at = yield c.point("Specify start point");
  if (!at) return;
  let h = yield c.distance("Specify height", { base: at, def: fmt(textHeight()) });
  if (h === null) h = textHeight();
  const rot = yield c.angle("Specify rotation angle", { base: at, def: "0" });
  c.edit(`attdef ${P(at)} ${N(h)} ${rot === null ? 0 : deg(rot)} | ${tag.trim().toUpperCase()} | ${(prompt || tag).replace(/\|/g, "/")} | ${(dflt || "").replace(/\|/g, "/")}`);
}

function* cmdAttedit(c) {
  const p = yield pickReq(c, "Select block reference");
  const h = p && objectAt(p);
  const g = getObj(h);
  if (!g || g.type !== "INSERT" || !g.attributes?.length) return c.log("That is not a block reference with attributes.");
  c.exec("begin");
  try {
    for (const a of g.attributes) {
      const v = yield c.text(`${a.tag}`, { def: a.value });
      if (v !== null && v !== a.value) c.edit(`attedit ${h} | ${a.tag} | ${v.replace(/\|/g, "/")}`);
    }
  } finally {
    c.exec("end");
  }
}

const LUNITS = [["Scientific", 1], ["Decimal", 2], ["Engineering", 3], ["Architectural", 4], ["Fractional", 5]];
const AUNITS = [["Degrees", 0], ["DMS", 1], ["Grads", 2], ["Radians", 3]];
const INSUNITS = [["Millimeters", 4], ["Inches", 1], ["Feet", 2], ["Centimeters", 5], ["Meters", 6], ["Unitless", 0]];

function* cmdUnits(c) {
  const i = doc.info;
  const pick = (list, v) => (list.find((x) => x[1] === v) || list[0])[0];
  const lt = yield c.option("Length type", LUNITS.map((x) => x[0]), { def: pick(LUNITS, i.lunits) });
  const lunits = lt ? LUNITS.find((x) => x[0] === lt.option)[1] : i.lunits;
  const fraction = lunits === 4 || lunits === 5;
  const luprec = yield* askInt(c, fraction ? "Precision: 0 whole, 1 halves, 2 quarters … 8 (1/256)" : "Decimal places (0 to 8)", i.luprec, 0);
  const at = yield c.option("Angle type", AUNITS.map((x) => x[0]), { def: pick(AUNITS, i.aunits) });
  const aunits = at ? AUNITS.find((x) => x[0] === at.option)[1] : i.aunits;
  const auprec = yield* askInt(c, "Angle decimal places (0 to 8)", i.auprec, 0);
  const ut = yield c.option("Drawing unit", INSUNITS.map((x) => x[0]), { def: pick(INSUNITS, i.insunits) });
  const ins = ut ? INSUNITS.find((x) => x[0] === ut.option)[1] : i.insunits;
  let scale = 0;
  if (ins !== i.insunits && ins !== 0 && i.insunits !== 0) {
    const r = yield c.option("Scale the drawing so everything keeps its real size?", ["Yes", "No"], { def: "Yes" });
    scale = r && r.option === "No" ? 0 : 1;
  }
  if (luprec > 8 || auprec > 8) return c.log("Precision is 0 to 8.");
  if (c.edit(`units ${ins} ${lunits} ${luprec} ${aunits} ${auprec} ${scale}`)) {
    c.log(`Units: ${pick(INSUNITS, ins).toLowerCase()}, ${pick(LUNITS, lunits).toLowerCase()} lengths${scale ? "; the drawing was scaled to keep its size" : ""}.`);
    if (scale) zoomExtents();
  }
}

function* cmdLinetype(c) {
  const lt = core.query(doc.id, "linetypes");
  c.log(`Loaded: ${lt.loaded.map((l) => l.name).join(", ")}. Current: ${lt.current}.`);
  const r = yield c.option("Enter an option", ["Load", "Set", "?"], { def: "Set" });
  if (r === null) return;
  if (r.option === "?") {
    for (const l of lt.loaded) c.log(`${l.name.padEnd(12)} ${l.description}`);
    c.log(`Can be loaded: ${lt.library.join(", ")}`);
    return;
  }
  if (r.option === "Load") {
    c.log(`Library: ${lt.library.join(", ")}`);
    const t = yield c.text("Linetypes to load (separate with spaces)");
    if (t && t.trim() && c.edit(`linetype-load ${t.trim()}`)) c.log("Loaded.");
    return;
  }
  const t = yield c.text("Linetype for new objects (ByLayer, ByBlock or a name)", { def: lt.current });
  if (t && t.trim() && c.edit(`linetype-current ${t.trim()}`)) c.log(`New objects: ${t.trim()}.`);
}

function* cmdLtscale(c) {
  const v = yield* askNumber(c, "Enter new linetype scale factor", doc.info.ltscale);
  if (typeof v === "number" && c.edit(`ltscale ${N(v)}`)) c.log(`Linetype scale ${v}.`);
}

function* cmdLweight(c) {
  c.log(`Current: ${doc.info.currentLineweight}. Weights: 0.13 0.18 0.25 0.35 0.50 0.70 1.00 mm.`);
  const t = yield c.text("Lineweight for new objects (ByLayer, ByBlock, Default or mm)", { def: doc.info.currentLineweight });
  if (t && t.trim() && c.edit(`lineweight-current ${t.trim()}`)) c.log(`New objects: ${t.trim()}.`);
}

function* cmdColor(c) {
  const t = yield c.text("Colour for new objects (ByLayer, ByBlock, 1 to 255 or #rrggbb)", { def: doc.info.currentColor });
  if (t && t.trim() && c.edit(`color-current ${t.trim()}`)) c.log(`New objects: ${t.trim()}.`);
}

function* layoutNew(c) {
  const lay = core.query(doc.id, "layouts");
  const n = lay.layouts.length;
  const name = yield c.text("Enter new layout name", { def: `Layout${n}` });
  const nm = (name && name.trim()) || `Layout${n}`;
  const paper = yield c.option("Paper size", lay.papers, { def: lay.defaultPaper });
  if (c.edit(`layout-new ${paper ? paper.option.replace(/\s/g, "_") : "-"} ${nm}`)) switchSheetNow(nm);
}

function* layoutRename(c, old) {
  const name = yield c.text(`Enter new name for ${old}`, { def: old });
  if (name && name.trim() && name.trim() !== old) c.edit(`layout-rename ${old} | ${name.trim()}`);
  renderSheets();
}

function* cmdLayout(c) {
  const r = yield c.option("Enter layout option", ["New", "Rename", "Delete", "Set", "?"], { def: "Set" });
  if (r === null) return;
  const lay = core.query(doc.id, "layouts");
  if (r.option === "?") return c.log(`Sheets: ${lay.layouts.map((l) => l.name + (l.paperName ? ` (${l.paperName})` : "")).join(", ")}`);
  if (r.option === "New") return yield* layoutNew(c);
  const names = lay.layouts.filter((l) => r.option === "Set" || !l.model).map((l) => l.name);
  c.log(`Sheets: ${names.join(", ")}`);
  const t = yield c.text(`Enter layout to ${r.option.toLowerCase()}`, { def: lay.layouts.find((l) => l.current)?.name });
  const name = t && t.trim();
  if (!name) return;
  if (r.option === "Set") return switchSheetNow(name);
  if (r.option === "Rename") return yield* layoutRename(c, name);
  if (c.edit(`layout-delete ${name}`)) renderSheets();
}

function* cmdMview(c) {
  const i = doc.info;
  if (!i.paper) return c.log("Viewports go on a layout: click a layout tab first (LAYOUT New makes one).");
  const a = yield c.point("Specify corner of viewport", { options: ["Fit"] });
  if (!a) return;
  // Fit: the core fills the paper inside its margin.
  let corners = "fit";
  if (!a.option) {
    const b = yield c.point("Specify opposite corner", { base: a, preview: (q) => [{ t: "poly", pts: [a, [q[0], a[1]], q, [a[0], q[1]]], closed: true }] });
    if (!b) return;
    corners = `${P(a)} ${P(b)}`;
  }
  const s = yield c.text("Scale: Fit, or paper:model as 1:2, 1:1, 2:1", { def: "Fit" });
  const scale = s && s.trim() ? s.trim() : "fit";
  if (c.edit(`mview ${corners} ${scale}`)) c.log("Viewport made. Select it to change its scale in Properties.");
}

function* cmdPageSetup(c) {
  const lay = core.query(doc.id, "layouts");
  const cur = lay.layouts.find((l) => l.current);
  const defPaper = cur?.paperName || lay.defaultPaper;
  const defOrient = cur?.paper && cur.paper[1] > cur.paper[0] ? "Portrait" : "Landscape";
  const defColor = cur?.monochrome === false ? "Color" : "Monochrome";
  const paper = (yield c.option("Paper size", lay.papers, { def: defPaper }))?.option || defPaper;
  const orient = (yield c.option("Orientation", ["Landscape", "Portrait"], { def: defOrient }))?.option || defOrient;
  const color = (yield c.option("Plot in", ["Monochrome", "Color"], { def: defColor }))?.option || defColor;
  const ok = c.edit(`page-setup ${paper.replace(/\s/g, "_")} ${orient === "Portrait" ? "portrait" : "landscape"} ${color === "Color" ? 0 : 1}`);
  if (ok) {
    if (doc.info.paper) zoomExtents();
    c.log(`${cur?.name || "This sheet"}: ${paper}${orient === "Portrait" ? " portrait" : ""}, ${color === "Color" ? "in colour" : "monochrome"}.`);
  }
}

/** PLOT and PRINT: the sheet as a PDF, set up first; PRINT opens it for
 *  the browser's print dialog. */
function* cmdPlot(c, print = false) {
  const r = yield c.option(`Plot ${doc.info.space || "Model"} to PDF`, ["Plot", "Setup"], { def: "Plot" });
  if (r?.option === "Setup") yield* cmdPageSetup(c);
  plotPdf(print);
}

function plotPdf(print = false) {
  let out;
  try {
    out = core.save(doc.id, "pdf", user());
  } catch (e) {
    return handleError(e);
  }
  const base = doc.name.replace(/\.(dwg|dxf|cad|svg)$/i, "");
  const name = `${base}${doc.info.paper ? `-${doc.info.space}` : ""}.pdf`;
  const url = URL.createObjectURL(new Blob([out.bytes], { type: "application/pdf" }));
  if (print) {
    const w = window.open(url, "_blank");
    if (!w) toast("Allow pop-ups to print; the PDF was saved instead.");
    else {
      log(`Opened ${name} to print`, "ok");
      setTimeout(() => URL.revokeObjectURL(url), 60000);
      return;
    }
  }
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
  log(`Plotted ${name}`, "ok");
  toast(`Plotted ${name}`);
}

const COMMANDS = [
  { name: "LINE", aliases: ["L"], desc: "line segments", run: cmdLine, needsDoc: true },
  { name: "PLINE", aliases: ["P", "PL"], desc: "polyline, one object", run: cmdPline, needsDoc: true },
  { name: "CIRCLE", aliases: ["C"], desc: "circle by centre and radius", run: cmdCircle, needsDoc: true },
  { name: "ARC", aliases: ["A"], desc: "arc through three points", run: cmdArc, needsDoc: true },
  { name: "RECTANGLE", aliases: ["R", "REC"], desc: "rectangle by two corners", run: cmdRect, needsDoc: true },
  { name: "TEXT", aliases: ["TE", "DT"], desc: "single-line text", run: cmdText, needsDoc: true },
  { name: "DIM", aliases: ["D"], desc: "smart dimension", run: (c) => cmdDim(c), needsDoc: true },
  { name: "DIMLINEAR", aliases: ["DL", "DLI"], desc: "horizontal or vertical dimension", run: (c) => cmdDim(c, "linear"), needsDoc: true },
  { name: "DIMALIGNED", aliases: ["DA", "DAL"], desc: "dimension along two points", run: (c) => cmdDim(c, "aligned"), needsDoc: true },
  { name: "DIMRADIUS", aliases: ["DR", "DRA"], desc: "radius of an arc or circle", run: (c) => cmdDim(c, "radius"), needsDoc: true },
  { name: "DIMDIAMETER", aliases: ["DD", "DDI"], desc: "diameter of a circle", run: (c) => cmdDim(c, "diameter"), needsDoc: true },
  { name: "MOVE", aliases: ["M"], desc: "move objects", run: cmdMove, needsDoc: true },
  { name: "COPY", aliases: ["CO", "CP"], desc: "copy objects", run: cmdCopy, needsDoc: true },
  { name: "ROTATE", aliases: ["RO"], desc: "rotate about a point", run: cmdRotate, needsDoc: true },
  { name: "MIRROR", aliases: ["MI"], desc: "mirror across a line", run: cmdMirror, needsDoc: true },
  { name: "ERASE", aliases: ["E"], desc: "remove objects", run: cmdErase, needsDoc: true },
  { name: "SCALE", aliases: ["SC"], desc: "resize about a point", run: cmdScale, needsDoc: true },
  { name: "STRETCH", aliases: ["S"], desc: "move what a crossing window takes in", run: cmdStretch, needsDoc: true },
  { name: "OFFSET", aliases: ["O"], desc: "parallel copy at a distance", run: cmdOffset, needsDoc: true },
  { name: "TRIM", aliases: ["T", "TR"], desc: "cut back to the objects it crosses", run: (c) => cmdTrimExtend(c, false), needsDoc: true },
  { name: "EXTEND", aliases: ["EX"], desc: "lengthen to the next object", run: (c) => cmdTrimExtend(c, true), needsDoc: true },
  { name: "FILLET", aliases: ["F"], desc: "round a corner", run: (c) => cmdFillet(c, false), needsDoc: true },
  { name: "CHAMFER", aliases: ["CHA"], desc: "bevel a corner", run: (c) => cmdFillet(c, true), needsDoc: true },
  { name: "ARRAY", aliases: ["AR"], desc: "rows and columns, or a circle, of copies", run: (c) => cmdArray(c), needsDoc: true },
  { name: "ARRAYRECT", aliases: [], desc: "copies in rows and columns", run: (c) => cmdArray(c, "rect"), needsDoc: true },
  { name: "ARRAYPOLAR", aliases: [], desc: "copies around a centre", run: (c) => cmdArray(c, "polar"), needsDoc: true },
  { name: "EXPLODE", aliases: ["X"], desc: "a block or polyline into its parts", run: cmdExplode, needsDoc: true },
  { name: "XLINE", aliases: ["XL"], desc: "construction line, endless both ways", run: (c) => cmdXline(c, false), needsDoc: true },
  { name: "RAY", aliases: ["RA"], desc: "construction line from a point", run: (c) => cmdXline(c, true), needsDoc: true },
  { name: "HATCH", aliases: ["H", "BHATCH", "BH"], desc: "fill an area with a pattern", run: cmdHatch, needsDoc: true },
  { name: "MTEXT", aliases: ["MT"], desc: "text of several lines", run: cmdMtext, needsDoc: true },
  { name: "LEADER", aliases: ["LE", "LEAD"], desc: "arrow and note", run: (c) => cmdLeader(c), needsDoc: true },
  { name: "QLEADER", aliases: ["QL"], desc: "arrow and note", run: (c) => cmdLeader(c), needsDoc: true },
  { name: "MLEADER", aliases: ["MLD"], desc: "arrow, landing and note", run: (c) => cmdLeader(c, true), needsDoc: true },
  { name: "DIMANGULAR", aliases: ["DAN"], desc: "angle between lines, of an arc, or at a vertex", run: cmdDimAngular, needsDoc: true },
  { name: "DIMBASELINE", aliases: ["DBA"], desc: "dimensions stacked from one datum", run: (c) => cmdDimNext(c, true), needsDoc: true },
  { name: "DIMCONTINUE", aliases: ["DCO"], desc: "dimensions in a chain", run: (c) => cmdDimNext(c, false), needsDoc: true },
  { name: "BLOCK", aliases: ["B"], desc: "make a block from objects", run: cmdBlock, needsDoc: true },
  { name: "INSERT", aliases: ["I"], desc: "place a block, filling its attributes", run: cmdInsert, needsDoc: true },
  { name: "ATTDEF", aliases: ["ATT"], desc: "attribute: a fill-in field for a block", run: cmdAttdef, needsDoc: true },
  { name: "ATTEDIT", aliases: ["ATE"], desc: "change a block's attribute values", run: cmdAttedit, needsDoc: true },
  { name: "UNITS", aliases: ["UN"], desc: "drawing unit, length and angle formats", run: cmdUnits, needsDoc: true },
  { name: "LINETYPE", aliases: ["LT", "LTYPE"], desc: "load linetypes; linetype of new objects", run: cmdLinetype, needsDoc: true },
  { name: "LTSCALE", aliases: ["LTS"], desc: "size of every dash pattern", run: cmdLtscale, needsDoc: true },
  { name: "LWEIGHT", aliases: ["LW"], desc: "lineweight of new objects", run: cmdLweight, needsDoc: true },
  { name: "COLOR", aliases: ["COL"], desc: "colour of new objects", run: cmdColor, needsDoc: true },
  { name: "LAYOUT", aliases: ["LO"], desc: "new, rename, delete or switch layouts", run: cmdLayout, needsDoc: true },
  { name: "MVIEW", aliases: ["MV", "VPORTS"], desc: "viewport on a layout", run: cmdMview, needsDoc: true },
  { name: "PAGESETUP", aliases: ["PAGE"], desc: "paper size, orientation, colours", run: cmdPageSetup, needsDoc: true },
  { name: "PLOT", aliases: [], desc: "the sheet as a PDF (Ctrl+P)", run: (c) => cmdPlot(c), needsDoc: true, repeat: false },
  { name: "PRINT", aliases: ["PRI"], desc: "open the sheet's PDF to print", run: (c) => cmdPlot(c, true), needsDoc: true, repeat: false },
  { name: "EXPORTPDF", aliases: [], desc: "save the sheet as a PDF", instant: true, needsDoc: true, run: () => plotPdf(false) },
  { name: "DIST", aliases: ["DI", "ME"], desc: "distance and angle between points", run: cmdDist, needsDoc: true },
  { name: "ID", aliases: ["ID"], desc: "coordinates of a point", run: cmdId, needsDoc: true },
  { name: "ZOOM", aliases: ["Z"], desc: "extents, window, in, out", run: cmdZoom, needsDoc: true, repeat: false },
  { name: "PAN", aliases: ["PA"], desc: "slide the view", run: cmdPan, needsDoc: true, repeat: false },
  { name: "SAVEDBY", aliases: [], desc: "your name on saves", run: cmdSavedBy, hidden: true, repeat: false },
  { name: "UNDO", aliases: ["U"], desc: "undo the last command", instant: true, needsDoc: true, run: () => history("undo") },
  { name: "REDO", aliases: [], desc: "redo", instant: true, needsDoc: true, run: () => history("redo") },
  { name: "LAYER", aliases: ["LA"], desc: "the layers panel", instant: true, run: () => focusSection("#sec-layers") },
  { name: "PROPERTIES", aliases: ["PR", "MO"], desc: "the properties panel", instant: true, run: () => focusSection("#sec-props") },
  { name: "SELECTALL", aliases: [], desc: "select everything", instant: true, needsDoc: true, run: selectAll },
  { name: "OSNAP", aliases: ["OS"], desc: "running object snaps (F3)", instant: true, run: () => toggleAid("osnap") },
  { name: "GRID", aliases: ["GR"], desc: "grid (F7)", instant: true, run: () => toggleAid("grid") },
  { name: "SNAP", aliases: ["SN"], desc: "step the cursor (F9)", instant: true, run: () => toggleAid("snap") },
  { name: "ORTHO", aliases: ["OR"], desc: "lock to X and Y (F8)", instant: true, run: () => toggleAid("ortho") },
  { name: "POLAR", aliases: ["POLA"], desc: "track 15° angles (F10)", instant: true, run: () => toggleAid("polar") },
  { name: "NEW", aliases: [], desc: "new drawing (Ctrl+N)", instant: true, run: () => newDrawing(false) },
  { name: "OPEN", aliases: [], desc: "open a drawing (Ctrl+O)", instant: true, run: () => openDialog() },
  { name: "SAVE", aliases: [], desc: "save (Ctrl+S)", instant: true, needsDoc: true, run: () => save() },
  { name: "SAVEAS", aliases: [], desc: "save as .cad (Ctrl+Shift+S)", instant: true, needsDoc: true, run: () => saveAs("cad") },
  { name: "EXPORT", aliases: [], desc: "DWG, DXF or SVG", instant: true, needsDoc: true, run: () => showMenu($("#mark"), fileMenu()) },
  { name: "BOARD", aliases: ["BB", "COACH"], desc: "the blackboard: lessons (F1)", instant: true, run: () => Board.toggle() },
  { name: "SHOPCHECK", aliases: ["CHECK", "CHK"], desc: "check holes against shop rules", instant: true, needsDoc: true, run: () => Board.check() },
  { name: "SHOPSET", aliases: ["SHOP"], desc: "shop settings: kerf, die, materials", instant: true, run: () => Board.open("settings") },
  { name: "CLOSE", aliases: [], desc: "close the drawing (Ctrl+W)", instant: true, needsDoc: true, run: () => closeDoc(doc) },
];

function history(which) {
  if (runner) cancelCommand(false);
  try {
    core.exec(doc.id, which);
  } catch (e) {
    if (e instanceof CoreCrash) throw e;
    return logError(e.message.replace(/^./, (c) => c.toUpperCase()));
  }
  selection.clear();
  refresh(doc);
}

function selectAll() {
  const locked = new Set(doc.info.layers.filter((l) => l.locked).map((l) => l.name));
  selection.clear();
  for (const it of doc.disp.items) if (!it.dead && !locked.has(doc.disp.styles[it.style].layer)) selection.add(it.handle);
  selectionChanged();
}

function focusSection(sel) {
  if (PHONE.matches) $("#side").classList.add("open");
  $(sel).scrollIntoView({ block: "start", behavior: "smooth" });
}

// ---------------------------------------------------------------- properties

/** Rows for an object: [label, value, unit, field] (field: editable through `set`). */
function propRows(g) {
  const u = unitMark();
  const pt = (p) => `${fmt(p[0])}, ${fmt(p[1])}`;
  const rows = [];
  if (g.type === "CIRCLE") {
    rows.push(["Center X", fmt(g.center[0]), u, "cx"], ["Center Y", fmt(g.center[1]), u, "cy"], ["Radius", fmt(g.radius), u, "radius"], ["Diameter", fmt(g.diameter), u, "diameter"], ["Circumference", fmt(g.circumference), u, "circumference"], ["Area", fmt(g.area), u && `${u}²`, "area"]);
  } else if (g.type === "ARC") {
    rows.push(["Center X", fmt(g.center[0]), u, "cx"], ["Center Y", fmt(g.center[1]), u, "cy"], ["Radius", fmt(g.radius), u, "radius"], ["Start angle", g.start.toFixed(2), "°", "start"], ["End angle", g.end.toFixed(2), "°", "end"], ["Arc length", fmt(g.arcLength), u]);
  } else if (g.type === "LINE") {
    rows.push(["Start X", fmt(g.start[0]), u, "x1"], ["Start Y", fmt(g.start[1]), u, "y1"], ["End X", fmt(g.end[0]), u, "x2"], ["End Y", fmt(g.end[1]), u, "y2"], ["Length", fmt(g.length), u, "length"], ["Angle", (+g.angle).toFixed(2), "°"]);
  } else if (g.type === "LWPOLYLINE") {
    rows.push(["Vertices", String(g.vertices), ""], ["Closed", g.closed ? "Yes" : "No", ""]);
  } else if (g.type === "TEXT") {
    rows.push(["Contents", g.value, "", "value"], ["Height", fmt(g.height), u, "height"], ["Position", pt(g.insertion), u]);
  } else if (g.type === "MTEXT") {
    rows.push(["Contents", g.value.replace(/\\P/g, " ¶ "), "", "value"], ["Height", fmt(g.height), u, "height"], ["Width", g.width ? fmt(g.width) : "none", g.width ? u : "", "width"], ["Rotation", (+g.rotation).toFixed(2), "°", "rotation"], ["Position", pt(g.insertion), u]);
  } else if (g.type === "HATCH") {
    rows.push(["Pattern", g.pattern, "", "pattern"]);
    if (!g.solid) rows.push([g.pattern === "USER" ? "Spacing" : "Scale", +(+g.scale).toPrecision(6) + "", "", "scale"], ["Angle", (+g.angle).toFixed(2), "°", "angle"]);
    rows.push(["Boundaries", String(g.loops), ""]);
  } else if (g.type === "XLINE" || g.type === "RAY") {
    rows.push(["Base X", fmt(g.base[0]), u, "x"], ["Base Y", fmt(g.base[1]), u, "y"], ["Angle", (+g.angle).toFixed(2), "°", "angle"]);
  } else if (g.type === "INSERT") {
    rows.push(["Block", g.block, ""], ["Position X", fmt(g.insertion[0]), u, "x"], ["Position Y", fmt(g.insertion[1]), u, "y"], ["Scale", +(+g.scale).toPrecision(6) + "", "", "scale"], ["Rotation", (+g.rotation).toFixed(2), "°", "rotation"]);
    for (const a of g.attributes || []) rows.push([a.tag, a.value, "", `attr:${a.tag}`]);
  } else if (g.type === "ATTDEF" || g.type === "ATTRIB") {
    rows.push(["Tag", g.tag, "", g.type === "ATTDEF" ? "tag" : ""], ...(g.type === "ATTDEF" ? [["Prompt", g.prompt, "", "prompt"], ["Default", g.value, "", "value"]] : [["Value", g.value, "", "value"]]), ["Height", fmt(g.height), u, "height"]);
  } else if (g.type === "VIEWPORT") {
    const k = g.scale;
    const ratio = k >= 1 ? `${+k.toPrecision(4)}:1` : `1:${+(1 / k).toPrecision(4)}`;
    rows.push(["Scale", ratio, "", "vpscale"], ["View centre X", fmt(g.viewCenter[0]), u, "cx"], ["View centre Y", fmt(g.viewCenter[1]), u, "cy"], ["Size", `${fmt(g.width)} × ${fmt(g.height)}`, u]);
  } else if (g.type?.startsWith("DIMENSION")) {
    rows.push(["Measured", g.dimension === "angular" ? `${(+g.measurement).toFixed(2)}°` : fmt(g.measurement), g.dimension === "angular" ? "" : u], ["Text override", g.value || "(measured)", "", "value"]);
  } else if (g.type === "LEADER") {
    rows.push(["Points", String(g.points.length), ""]);
  }
  return rows;
}

function title(t) {
  const names = { LWPOLYLINE: "Polyline", MTEXT: "Multiline text", DIMENSION: "Dimension", INSERT: "Block reference", XLINE: "Construction line", ATTDEF: "Attribute definition", ATTRIB: "Attribute" };
  if (t.startsWith("DIMENSION")) return { DIMENSION_ANG_3PT: "Angular dimension", DIMENSION_ANG_2LN: "Angular dimension", DIMENSION_RADIUS: "Radius dimension", DIMENSION_DIAMETER: "Diameter dimension", DIMENSION_ALIGNED: "Aligned dimension" }[t] || "Linear dimension";
  return names[t] || t.charAt(0) + t.slice(1).toLowerCase();
}

const WEIGHTS = ["ByLayer", "ByBlock", "Default", "0.13", "0.18", "0.25", "0.35", "0.50", "0.70", "1.00"];

/** Menus for an object's colour, linetype and lineweight. */
function propertyMenu(row, g, which) {
  const h = g.handle;
  if (which === "color") {
    return showMenu(row, [["ByLayer", "", () => applySet(h, "color", "ByLayer")], ["ByBlock", "", () => applySet(h, "color", "ByBlock")], ...LAYER_COLORS.map(([i, name]) => [`<span class="sw" style="background:${aciCss(i)};margin-right:8px"></span>${name}`, String(i), () => applySet(h, "color", i)])]);
  }
  if (which === "linetype") {
    const lt = core.query(doc.id, "linetypes");
    const names = ["ByLayer", "ByBlock", ...lt.loaded.map((l) => l.name), ...lt.library.filter((n) => !lt.loaded.some((l) => l.name === n)).slice(0, 12)];
    return showMenu(row, names.map((n) => [esc(n), lt.loaded.some((l) => l.name === n) || n.startsWith("By") ? "" : "load", () => applySet(h, "linetype", n)]));
  }
  return showMenu(row, WEIGHTS.map((w) => [w, /^\d/.test(w) ? "mm" : "", () => applySet(h, "lineweight", w)]));
}

function renderProps() {
  const el = $("#props");
  if (!doc) {
    el.innerHTML = '<p class="empty">No drawing open.</p>';
    return;
  }
  if (!selection.size) {
    const cur = doc.info.layers.find((l) => l.current);
    el.innerHTML = `<div class="obj"><span class="t">Nothing selected</span></div>
      <div class="kv"><span class="k">Current layer</span><span class="v"><span class="sw" style="background:${cur?.color || "#fff"}"></span>${esc(cur?.name || "")}</span></div>
      <p class="empty">Click an object, or drag a box: left to right takes what is inside, right to left what it crosses.</p>`;
    return;
  }
  if (selection.size > 1) {
    el.innerHTML = `<div class="obj"><span class="t">${selection.size} objects</span><span class="pill">${selection.size} selected</span></div><p class="empty">Move, Copy, Rotate, Mirror and Erase work on all of them.</p>`;
    return;
  }
  const h = [...selection][0];
  const g = core.query(doc.id, `get ${h}`);
  if (!g) {
    el.innerHTML = `<div class="obj"><span class="t">Object ${esc(h)}</span><span class="pill">1 selected</span></div>`;
    return;
  }
  const row = ([k, v, u, field]) =>
    `<div class="kv${field ? " ed" : ""}" ${field ? `data-field="${field}"` : ""}><span class="k">${k}</span><span class="v">${esc(v)}${u ? `<span class="u">${u}</span>` : ""}</span></div>`;
  el.innerHTML = `<div class="obj"><span class="t">${esc(title(g.type))}</span><span class="pill">1 selected</span></div>
    <div class="grp">General</div>
    <div class="kv ed" data-layer><span class="k">Layer</span><span class="v"><span class="sw" style="background:${g.layerColor}"></span>${esc(g.layer)}</span></div>
    <div class="kv ed" data-menu="color"><span class="k">Color</span><span class="v">${esc(g.color)}</span></div>
    <div class="kv ed" data-menu="linetype"><span class="k">Linetype</span><span class="v">${esc(g.linetype)}</span></div>
    <div class="kv ed" data-menu="lineweight"><span class="k">Lineweight</span><span class="v">${esc(g.lineweight)}</span></div>
    ${row(["Linetype scale", String(g.ltscale ?? 1), "", "ltscale"])}
    ${propRows(g).length ? `<div class="grp">Geometry</div>${propRows(g).map(row).join("")}` : ""}`;
  el.querySelectorAll("[data-field]").forEach((r) => (r.onclick = () => editRow(r, g)));
  el.querySelectorAll("[data-menu]").forEach((r) => (r.onclick = () => propertyMenu(r, g, r.dataset.menu)));
  const lay = el.querySelector("[data-layer]");
  lay.onclick = () =>
    showMenu(
      lay,
      doc.info.layers.map((l) => [`<span class="sw" style="background:${l.color};margin-right:8px"></span>${esc(l.name)}`, l.handle === g.layerHandle ? "●" : "", () => applySet(h, "layer", l.handle)])
    );
}

/** Edit a value in place (DESIGN.md §10). Lengths may be typed in any unit. */
function editRow(row, g) {
  if (editingField) return;
  const field = row.dataset.field;
  const v = row.querySelector(".v");
  const old = v.innerHTML;
  const raw = field === "value" ? g.value : field.startsWith("attr:") ? (g.attributes.find((a) => `attr:${a.tag}` === field)?.value ?? "") : field === "prompt" ? g.prompt : field === "tag" ? g.tag : v.firstChild.textContent;
  editingField = field;
  row.classList.add("edit");
  v.innerHTML = "";
  const input = document.createElement("input");
  input.value = raw;
  v.append(input);
  input.focus();
  input.select();
  const done = (commit) => {
    if (!editingField) return;
    editingField = null;
    row.classList.remove("edit");
    if (commit && input.value.trim() !== raw.trim()) applyValue(g.handle, field, input.value.trim());
    else v.innerHTML = old;
  };
  input.onkeydown = (e) => {
    e.stopPropagation();
    if (e.key === "Enter") done(true);
    if (e.key === "Escape") done(false);
  };
  input.onblur = () => done(true);
}

function applyValue(h, field, text) {
  if (field.startsWith("attr:")) {
    try {
      core.exec(doc.id, `attedit ${h} | ${field.slice(5)} | ${text.replace(/\|/g, "/")}`);
    } catch (e) {
      if (e instanceof CoreCrash) return crashed(e);
      logError(e.message.replace(/^./, (c) => c.toUpperCase()));
    }
    return refresh(doc);
  }
  if (field === "value" || field === "pattern" || field === "tag" || field === "prompt") return applySet(h, field, text);
  if (field === "vpscale") return applySet(h, "scale", text);
  if (field === "ltscale" || field === "scale") {
    const v = Number(text);
    if (!(v > 0)) {
      logError(`"${text}" is not a number more than zero.`);
      return renderProps();
    }
    return applySet(h, field, v);
  }
  const angle = field === "start" || field === "end" || field === "rotation" || field === "angle";
  const r = core.query(doc.id, `${angle ? "angle" : "len"} ${text}`);
  if (!r) {
    logError(`"${text}" is not ${angle ? "an angle" : "a length"}.`);
    return renderProps();
  }
  applySet(h, field, angle ? deg(r.value) : N(r.value));
}

function applySet(h, field, value) {
  try {
    core.exec(doc.id, `set ${h} ${field} ${value}`);
  } catch (e) {
    if (e instanceof CoreCrash) return crashed(e);
    logError(e.message.replace(/^./, (c) => c.toUpperCase()));
  }
  refresh(doc);
}

// ---------------------------------------------------------------- layers

/** A sample of the layer's linetype, drawn at its weight. */
function linetypeSample(l) {
  const n = (l.linetype || "").toUpperCase();
  const dash = /CENTER|CENTRE/.test(n) ? "10 3 3 3" : /HIDDEN|DASHED/.test(n) ? "4 3" : /PHANTOM/.test(n) ? "12 3 3 3 3 3" : /DOT/.test(n) ? "1 3" : "";
  const w = l.weight >= 0.4 ? 2.2 : 1.2;
  return `<svg viewBox="0 0 56 8" aria-hidden="true"><path d="M1 4h54" stroke="${l.color}" stroke-width="${w}" ${dash ? `stroke-dasharray="${dash}"` : ""}/></svg>`;
}

const LAYER_COLORS = [[1, "Red"], [2, "Yellow"], [3, "Green"], [4, "Cyan"], [5, "Blue"], [6, "Magenta"], [7, "White"], [8, "Dark grey"], [9, "Light grey"]];

function renderLayers() {
  const el = $("#layers");
  if (!doc) {
    el.innerHTML = '<p class="empty">No drawing open.</p>';
    return;
  }
  el.innerHTML = "";
  for (const l of doc.info.layers) {
    const row = document.createElement("div");
    row.className = "lay" + (l.current ? " cur" : "") + (l.on ? "" : " off");
    row.title = l.current ? `${l.name} is the current layer` : `Click to make ${l.name} current`;
    row.innerHTML = `<button class="sw" data-a="color" title="Colour" style="background:${l.color};width:12px;height:12px"></button>
      <span class="n">${esc(l.name)}${l.current ? '<span class="curtag">CURRENT</span>' : ""}${l.plot === false ? '<span class="curtag" title="This layer does not plot">NO PLOT</span>' : ""}</span>
      <button class="lt" data-a="lt" title="${esc(l.linetype || "Continuous")}, ${l.weight.toFixed(2)} mm: click to change">${linetypeSample(l)}</button>
      <span class="c">${l.count}</span>
      <button class="ib${l.locked ? "" : " dim"}" data-a="lock" title="${l.locked ? "Locked" : "Unlocked"}">${icon(l.locked ? "lock" : "unlock")}</button>
      <button class="ib${l.on ? "" : " dim"}" data-a="on" title="${l.on ? "On" : "Off"}">${icon(l.on ? "eye" : "eyeoff")}</button>`;
    row.onclick = (e) => {
      const a = e.target.closest("button")?.dataset.a;
      if (a === "lt") {
        const lt = core.query(doc.id, "linetypes");
        const names = ["Continuous", ...lt.loaded.map((x) => x.name).filter((n) => n !== "Continuous"), ...lt.library.filter((n) => !lt.loaded.some((x) => x.name === n)).slice(0, 12)];
        return showMenu(e.target.closest("button"), [
          ...names.map((n) => [`Linetype ${esc(n)}`, (l.linetype || "Continuous").toUpperCase() === n.toUpperCase() ? "●" : lt.loaded.some((x) => x.name === n) ? "" : "load", () => layerExec(`layer-linetype ${l.handle} ${n}`)]),
          "-",
          ...WEIGHTS.slice(2).map((w) => [`Lineweight ${w}${/^\d/.test(w) ? " mm" : ""}`, Math.abs(l.weight - Number(w)) < 0.005 ? "●" : "", () => layerExec(`layer-lineweight ${l.handle} ${w}`)]),
          "-",
          ["Plots on the sheet", l.plot ? "●" : "", () => layerExec(`layer-plot ${l.handle} ${l.plot ? 0 : 1}`)],
        ]);
      }
      if (a === "color") {
        return showMenu(
          e.target,
          LAYER_COLORS.map(([i, name]) => [`<span class="sw" style="background:${aciCss(i)};margin-right:8px"></span>${name}`, l.aci === i ? "●" : String(i), () => layerExec(`layer-color ${l.handle} ${i}`)])
        );
      }
      layerExec(a === "on" ? `layer-on ${l.handle} ${l.on ? 0 : 1}` : a === "lock" ? `layer-lock ${l.handle} ${l.locked ? 0 : 1}` : `layer-current ${l.handle}`);
    };
    el.append(row);
  }
}

function aciCss(i) {
  // Screen colours (standards::screen_color): 1–4 softened, the rest pure.
  return { 1: "#ff4d4d", 2: "#ffe14d", 3: "#3ddc84", 4: "#3fe0ff", 5: "#0000ff", 6: "#ff00ff", 7: "#ffffff", 8: "#808080", 9: "#c0c0c0" }[i];
}

function layerExec(cmd) {
  try {
    core.exec(doc.id, cmd);
  } catch (e) {
    if (e instanceof CoreCrash) return crashed(e);
    return toast(e.message.replace(/^./, (c) => c.toUpperCase()), true);
  }
  refresh(doc);
}

function addLayerRow() {
  if (!doc) return;
  const input = document.createElement("input");
  input.className = "newlayer";
  input.placeholder = "New layer name, then Enter";
  $("#layers").append(input);
  input.focus();
  input.onkeydown = (e) => {
    e.stopPropagation();
    if (e.key === "Escape") return renderLayers();
    if (e.key === "Enter" && input.value.trim()) layerExec(`layer-add ${input.value.trim()}`);
  };
  input.onblur = () => setTimeout(renderLayers, 100);
}

// ---------------------------------------------------------------- drawing section

function renderDrawing() {
  const el = $("#drawing");
  if (!doc) {
    el.innerHTML = '<p class="empty">No drawing open.</p>';
    return;
  }
  const i = doc.info;
  const cap = (s) => s.charAt(0).toUpperCase() + s.slice(1);
  el.innerHTML = `<div class="kv ed" data-cmd="UNITS" title="UNITS"><span class="k">Units</span><span class="v">${esc(cap(i.units))}</span></div>
    <div class="kv ed" data-cmd="UNITS" title="UNITS"><span class="k">Display</span><span class="v">${esc(i.display)}</span></div>
    <div class="kv ed" data-cmd="LTSCALE" title="LTSCALE"><span class="k">Linetype scale</span><span class="v">${esc(String(i.ltscale))}</span></div>
    <div class="kv ed" data-cmd="COLOR" title="COLOR"><span class="k">New objects</span><span class="v">${esc(i.currentColor)} · ${esc(i.currentLinetype)} · ${esc(i.currentLineweight)}</span></div>
    <div class="kv"><span class="k">Sheet</span><span class="v">${esc(i.space || "Model")}${i.paper ? ` · ${fmt(i.paper[0])} × ${fmt(i.paper[1])}` : " · 1:1"}</span></div>
    <div class="kv"><span class="k">Opened from</span><span class="v">${esc(i.source === "new" ? "New drawing" : i.source.toUpperCase())}</span></div>
    <details><summary class="grp" style="cursor:pointer">Summary</summary><pre class="summary">${esc(i.summary)}</pre></details>`;
  el.querySelectorAll("[data-cmd]").forEach((r) => (r.onclick = () => startCommand(r.dataset.cmd)));
}

// ---------------------------------------------------------------- phone sheet

/** The bottom sheet: the running prompt, then the selected object's key values. */
function renderSheet() {
  const el = $("#sheet-obj");
  if (!el || !doc) return;
  let html = "";
  if (runner?.req) {
    const opts = (runner.req.options || []).map((o) => `<button class="pill" data-opt="${o}">${o}</button>`).join(" ");
    html += `<div class="prompt-line"><b>${runner.name === "GRIP" ? "EDIT" : runner.name}</b> ${esc(runner.req.prompt)} ${opts}</div>`;
  }
  if (selection.size === 1) {
    const g = core.query(doc.id, `get ${[...selection][0]}`);
    if (g) {
      const rows = propRows(g);
      const fields =
        g.type === "CIRCLE"
          ? [["Center", `${fmt(g.center[0])}, ${fmt(g.center[1])}`, ""], rows[2], rows[3], rows[5]]
          : g.type === "LINE"
          ? [["Start", `${fmt(g.start[0])}, ${fmt(g.start[1])}`, ""], ["End", `${fmt(g.end[0])}, ${fmt(g.end[1])}`, ""], rows[4], rows[5]]
          : rows.slice(0, 4);
      html += `<div class="obj"><span class="t">${esc(title(g.type))}</span><span class="pill"><span class="sw" style="background:${g.layerColor}"></span>${esc(g.layer)}</span></div>
        <div class="grid2">${fields.filter(Boolean).map(([k, v, u, field]) => `<button class="f" ${field ? `data-field="${field}"` : "disabled"}><div class="k">${k}</div><div class="v">${esc(v)}${u ? `<span class="u">${u}</span>` : ""}</div></button>`).join("")}</div>`;
      el.innerHTML = html;
      el.querySelectorAll("[data-field]").forEach((b) => (b.onclick = () => openNumpad(g, b)));
      wireSheetOptions(el);
      return;
    }
  }
  if (!runner?.req) html += `<div class="prompt-line">${selection.size > 1 ? `${selection.size} selected` : "Tap an object to select it, or pick a tool."}</div>`;
  el.innerHTML = html;
  wireSheetOptions(el);
}

function wireSheetOptions(el) {
  el.querySelectorAll("[data-opt]").forEach((b) => (b.onclick = () => submit(b.dataset.opt)));
}

/** A number pad for a field (DESIGN.md §11). */
function openNumpad(g, btn) {
  const field = btn.dataset.field;
  if (field === "value") {
    const t = prompt("Text", g.value);
    if (t) applySet(g.handle, "value", t);
    return;
  }
  document.querySelectorAll(".f.edit").forEach((f) => f.classList.remove("edit"));
  btn.classList.add("edit");
  const pad = $("#numpad");
  let val = "";
  const keys = ["7", "8", "9", "⌫", "4", "5", "6", "/", "1", "2", "3", "-", "0", ".", '"', "Set"];
  pad.innerHTML = `<div class="np-head"><span class="np-k">${esc(btn.querySelector(".k").textContent)}</span><span class="np-k">${esc(unitMark())}</span></div><div class="np-v"></div><div class="np-keys">${keys.map((k) => `<button${k === "Set" ? ' class="ok"' : ""}>${k}</button>`).join("")}</div>`;
  const show = () => (pad.querySelector(".np-v").textContent = val || btn.querySelector(".v").firstChild.textContent);
  show();
  pad.querySelectorAll(".np-keys button").forEach(
    (b) =>
      (b.onclick = () => {
        const k = b.textContent;
        if (k === "⌫") val = val.slice(0, -1);
        else if (k === "Set") {
          closeNumpad();
          if (val) applyValue(g.handle, field, val);
          return;
        } else val += k;
        show();
      })
  );
  pad.hidden = false;
}
function closeNumpad() {
  $("#numpad").hidden = true;
  document.querySelectorAll(".f.edit").forEach((f) => f.classList.remove("edit"));
}

// ---------------------------------------------------------------- files

const TYPES = {
  cad: { description: "CAD drawing", accept: { "application/octet-stream": [".cad"] } },
  dwg: { description: "DWG drawing (2018 format)", accept: { "application/acad": [".dwg"] } },
  dxf: { description: "DXF drawing", accept: { "application/dxf": [".dxf"] } },
  dxfb: { description: "Binary DXF drawing", accept: { "application/dxf": [".dxf"] } },
  svg: { description: "SVG image", accept: { "image/svg+xml": [".svg"] } },
  pdf: { description: "PDF document", accept: { "application/pdf": [".pdf"] } },
};

async function openDialog() {
  if (!core) return;
  if (window.showOpenFilePicker) {
    try {
      const hs = await window.showOpenFilePicker({ multiple: true, types: [{ description: "Drawings", accept: { "application/octet-stream": [".dwg", ".dxf", ".cad"] } }] });
      for (const h of hs) await openFile(await h.getFile(), h);
    } catch (e) {
      if (e.name !== "AbortError") toast(e.message, true);
    }
  } else $("#file-input").click();
}

function user() {
  try {
    return localStorage.getItem("cad.user") || "";
  } catch (_) {
    return "";
  }
}

/** Save in the drawing's own format, to its own file when the browser allows. */
async function save() {
  if (!doc) return;
  if (doc.fileHandle?.createWritable) {
    try {
      const { bytes, notes } = core.save(doc.id, doc.format, user());
      const w = await doc.fileHandle.createWritable();
      await w.write(bytes);
      await w.close();
      return saved(doc, doc.name, notes, false, bytes);
    } catch (e) {
      if (e instanceof CoreCrash) return crashed(e);
      if (e.name === "AbortError") return;
      toast(`Saving to ${doc.name} failed (${e.message}); choose where to save.`, true);
    }
  }
  await saveAs(doc.format);
}

async function saveAs(format) {
  if (!doc) return;
  const ext = format === "dxfb" ? "dxf" : format;
  const name = `${doc.name.replace(/\.(dwg|dxf|cad|svg)$/i, "")}.${ext}`;
  let out;
  try {
    out = core.save(doc.id, format, user());
  } catch (e) {
    return handleError(e);
  }
  const keeps = format !== "svg";
  if (window.showSaveFilePicker) {
    try {
      const h = await window.showSaveFilePicker({ suggestedName: name, types: [TYPES[format]] });
      const w = await h.createWritable();
      await w.write(out.bytes);
      await w.close();
      if (keeps) Object.assign(doc, { fileHandle: h, format: ext, name: h.name });
      saved(doc, h.name, out.notes, !keeps, out.bytes);
    } catch (e) {
      if (e.name !== "AbortError") toast(e.message, true);
    }
    return;
  }
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([out.bytes], { type: "application/octet-stream" }));
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 10000);
  if (keeps) Object.assign(doc, { format: ext, name });
  saved(doc, name, out.notes, !keeps, out.bytes);
}

function saved(d, name, notes, exportOnly = false, bytes = null) {
  // What a restart reopens (a .cad, DWG or DXF the core reads back).
  if (!exportOnly && bytes) Object.assign(d, { orig: bytes, sample: false, imperial: false });
  refresh(d);
  log(`${exportOnly ? "Exported" : "Saved"} ${name}${notes.length ? ` (${notes[0]})` : ""}`, "ok");
  for (const n of notes.slice(1)) log(`  ${n}`);
  toast(`${exportOnly ? "Exported" : "Saved"} ${name}`);
}

addEventListener("beforeunload", (e) => {
  if (docs.some((d) => d.info?.dirty)) e.preventDefault();
});

// ---------------------------------------------------------------- blackboard

/**
 * The Blackboard: a pull-down classroom board beside the drawing. It is
 * organised like a school: subjects (tabs), units inside a subject, lessons
 * inside a unit, and steps inside a lesson. A step is a chalk figure, a few
 * short sentences, a worked example, the numbers the learner can change, and
 * the rule as a live check.
 *
 * Two rules hold everywhere:
 *  - Every figure is drawn from the same numbers the formulas use, so a
 *    figure can never disagree with the maths beside it.
 *  - Every threshold is a starting point the shop can override. They all live
 *    in Shop settings, and every step that uses one says what it depends on.
 *
 * Sheet-metal maths is the standard neutral-axis model:
 *   BA = θ (R + K T)      OSSB = tan(θ/2) (R + T)      BD = 2 OSSB − BA
 */
const Board = (() => {
  const root = $("#board");
  const frameEl = root.querySelector(".bb-frame");
  const mainEl = root.querySelector(".bb-main");
  const figEl = $("#bb-fig");
  const textEl = $("#bb-text");
  const stepsEl = $("#bb-steps");
  const tabsEl = $("#bb-tabs");
  const rad = (d) => (d * Math.PI) / 180;
  const f1 = (x) => +(+x).toFixed(1);
  const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
  const ok = (v, need) => v >= need - 1e-9;
  const col = (good) => (good ? "g" : "r");
  const cls = (good) => (good ? "ok" : "bad");

  // ---------------------------------------------------------------- the shop's own numbers
  // Starting points only. Every one can be changed in Shop settings, and the
  // lessons read them from there. Material and process tables are kept per
  // entry so a shop can tune one without touching the others.
  const MATERIALS = {
    crs: { name: "Mild steel, cold rolled", air: 0.15, across: 0.5, along: 1.0, rho: 7.85 },
    hrs: { name: "Mild steel, hot rolled", air: 0.12, across: 1.0, along: 1.5, rho: 7.85 },
    ss: { name: "Stainless 304", air: 0.2, across: 0.5, along: 1.0, rho: 7.9 },
    al: { name: "Aluminium 5052-H32", air: 0.15, across: 1.0, along: 1.5, rho: 2.68 },
  };
  const PROCESSES = {
    laser: { name: "Laser", kerf: 0.3, hole: 1.0, web: 1.0, lead: 0.5, leadMin: 1, pierce: 2, tol: 0.1, slot: 0.1, bevel: 0.5 },
    plasma: { name: "Plasma", kerf: 1.8, hole: 1.5, web: 2.0, lead: 1.0, leadMin: 3, pierce: 2.5, tol: 0.5, slot: 0.5, bevel: 2 },
  };
  const SHOP_DEF = { holeBend: 2.5, slotBend: 4, relW: 1, relD: 1, vRatio: 8, bendTol: 0.5, angTol: 1, edge: 1.5, kShop: 0, buttMax: 3, gapFrac: 0.5 };
  const copy = (o) => JSON.parse(JSON.stringify(o));
  const shopDefaults = () => ({ ...SHOP_DEF, mats: copy(MATERIALS), procs: copy(PROCESSES) });
  let shop = shopDefaults();

  // ---------------------------------------------------------------- the part on the board
  // The example part's numbers, kept in millimetres. The unit switch only
  // changes what is shown.
  const DEF = {
    T: 3, R: 3, K: 0.4, ang: 90, A: 50, B: 30, C: 40, W: 40, ai: 47, bi: 27,
    h: 8, f: 15, d: 6, g: 3, sl: 12, w: 2, dp: 5, cr: 8, jog: 20,
    phi: 90, rot: 0, sw: 1.5, e: 10, br: 3, tw: 6, tl: 40, tq: 0.2, Ln: 50,
    n: 5, pt: 25, tol: 0.2, nb: 1, fl: 100, gap: 1.5, inc: 60, rf: 1.5, tabw: 20, leg: 4,
    px: 30, py: 20, pdx: 40, pdy: 25, pl: 60, pa: 30, vL: 80, vW: 40, vH: 30, vd: 16, dec: 2, sL: 300, nseg: 4,
  };
  const s = { ...DEF, unit: "mm", proc: "laser", mat: "crs", wside: "arrow", proj: "third", sheet: "A3" };
  const PR = () => shop.procs[s.proc];
  const MAT = () => shop.mats[s.mat];

  // ---- the controls: each reads and writes the part, or a shop setting when it says so
  const CTL = {
    T: { sym: "T", label: "Thickness", mm: [0.5, 25, 0.1], in: [0.02, 1, 0.001] },
    R: { sym: "R", label: "Inside radius", mm: [0, 30, 0.5], in: [0, 1.2, 0.01] },
    K: { sym: "K", label: "K-factor", mm: [0.25, 0.5, 0.01], plain: true, show: (v) => v.toFixed(2) },
    ang: { sym: "θ", label: "Bend angle", mm: [20, 135, 1], plain: true, show: (v) => `${Math.round(v)}°` },
    A: { sym: "A", label: "Outside leg A", mm: [10, 200, 1], in: [0.4, 8, 0.05] },
    B: { sym: "B", label: "Outside leg B", mm: [10, 200, 1], in: [0.4, 8, 0.05] },
    C: { sym: "C", label: "Outside leg C", mm: [10, 200, 1], in: [0.4, 8, 0.05] },
    W: { sym: "W", label: "Part width", mm: [10, 200, 1], in: [0.4, 8, 0.05] },
    ai: { sym: "a", label: "Inside leg a", mm: [10, 200, 1], in: [0.4, 8, 0.05] },
    bi: { sym: "b", label: "Inside leg b", mm: [10, 200, 1], in: [0.4, 8, 0.05] },
    h: { sym: "h", label: "Hole edge to bend", mm: [0, 40, 0.5], in: [0, 1.6, 0.02] },
    f: { sym: "f", label: "Flange length", mm: [0, 60, 0.5], in: [0, 2.4, 0.02] },
    d: { sym: "d", label: "Hole diameter", mm: [0.5, 40, 0.5], in: [0.02, 1.6, 0.01] },
    g: { sym: "g", label: "Web between cuts", mm: [0, 20, 0.5], in: [0, 0.8, 0.02] },
    sl: { sym: "s", label: "Slot edge to bend", mm: [0, 50, 0.5], in: [0, 2, 0.02] },
    w: { sym: "w", label: "Relief width", mm: [0, 12, 0.5], in: [0, 0.5, 0.02] },
    dp: { sym: "p", label: "Relief depth", mm: [0, 30, 0.5], in: [0, 1.2, 0.02] },
    cr: { sym: "c", label: "Corner relief past the bend line", mm: [0, 30, 0.5], in: [0, 1.2, 0.02] },
    jog: { sym: "j", label: "Jog height", mm: [2, 80, 0.5], in: [0.1, 3, 0.02] },
    phi: { sym: "φ", label: "Bend line to grain", mm: [0, 90, 1], plain: true, show: (v) => `${Math.round(v)}°` },
    rot: { sym: "ψ", label: "Part turned on the sheet", mm: [0, 90, 1], plain: true, show: (v) => `${Math.round(v)}°` },
    sw: { sym: "w", label: "Slot width", mm: [0.1, 12, 0.1], in: [0.004, 0.5, 0.004] },
    e: { sym: "e", label: "Hole centre to edge", mm: [1, 60, 0.5], in: [0.04, 2.4, 0.02] },
    br: { sym: "b", label: "Bridge and stroke width", mm: [0.2, 12, 0.1], in: [0.01, 0.5, 0.005] },
    tw: { sym: "w", label: "Tab width", mm: [0.5, 40, 0.5], in: [0.02, 1.6, 0.02] },
    tl: { sym: "L", label: "Tab length", mm: [2, 150, 1], in: [0.1, 6, 0.05] },
    tq: { sym: "±", label: "Tolerance you need", mm: [0.01, 2, 0.01], in: [0.0005, 0.08, 0.0005] },
    Ln: { sym: "L", label: "Size being toleranced", mm: [1, 3000, 1], in: [0.04, 120, 0.04] },
    n: { sym: "n", label: "Holes in the row", mm: [2, 10, 1], plain: true, show: (v) => String(Math.round(v)) },
    pt: { sym: "p", label: "Hole pitch", mm: [8, 60, 1], in: [0.3, 2.4, 0.05] },
    tol: { sym: "±", label: "Tolerance on each dimension", mm: [0.05, 1, 0.05], in: [0.002, 0.04, 0.001] },
    fl: { sym: "L", label: "Flange length", mm: [5, 1000, 1], in: [0.2, 40, 0.05] },
    nb: { sym: "n", label: "Bends between the features", mm: [0, 4, 1], plain: true, show: (v) => String(Math.round(v)) },
    gap: { sym: "g", label: "Root gap", mm: [0, 6, 0.1], in: [0, 0.24, 0.005] },
    inc: { sym: "α", label: "Included groove angle", mm: [30, 90, 1], plain: true, show: (v) => `${Math.round(v)}°` },
    rf: { sym: "f", label: "Root face", mm: [0, 6, 0.1], in: [0, 0.24, 0.005] },
    tabw: { sym: "w", label: "Tab width", mm: [5, 80, 1], in: [0.2, 3, 0.05] },
    leg: { sym: "z", label: "Fillet weld leg", mm: [1, 20, 0.5], in: [0.04, 0.8, 0.02] },
    // drafting
    px: { sym: "x", label: "X of the point", mm: [-100, 100, 1], in: [-4, 4, 0.05] },
    py: { sym: "y", label: "Y of the point", mm: [-80, 90, 1], in: [-3, 3.5, 0.05] },
    pdx: { sym: "Δx", label: "Across from the last point", mm: [-80, 80, 1], in: [-3, 3, 0.05] },
    pdy: { sym: "Δy", label: "Up from the last point", mm: [-45, 80, 1], in: [-1.75, 3, 0.05] },
    pl: { sym: "L", label: "Length", mm: [5, 90, 1], in: [0.2, 3.5, 0.05] },
    pa: { sym: "θ", label: "Angle", mm: [0, 360, 1], plain: true, show: (v) => `${Math.round(v)}°` },
    vL: { sym: "L", label: "Length", mm: [20, 200, 1], in: [0.8, 8, 0.05] },
    vW: { sym: "W", label: "Depth", mm: [10, 120, 1], in: [0.4, 4.5, 0.05] },
    vH: { sym: "H", label: "Height", mm: [5, 100, 1], in: [0.2, 4, 0.05] },
    vd: { sym: "d", label: "Hole diameter", mm: [2, 60, 1], in: [0.1, 2.4, 0.02] },
    dec: { sym: "n", label: "Decimal places shown", mm: [0, 4, 1], plain: true, show: (v) => String(Math.round(v)) },
    sL: { sym: "L", label: "Part's longest size", mm: [20, 5000, 10], in: [1, 200, 0.5] },
    nseg: { sym: "n", label: "Steps along the part", mm: [2, 6, 1], plain: true, show: (v) => String(Math.round(v)) },
    // bound to shop settings
    k: { sym: "k", label: "Kerf width", mm: [0.05, 5, 0.05], in: [0.002, 0.2, 0.001], get: () => PR().kerf, set: (v) => (PR().kerf = v), shop: true },
    bev: { sym: "β", label: "Edge bevel", mm: [0, 10, 0.5], plain: true, show: (v) => `${v.toFixed(1)}°`, get: () => PR().bevel, set: (v) => (PR().bevel = v), shop: true },
    V: { sym: "V", label: "Die opening", mm: [2, 250, 0.5], in: [0.08, 10, 0.02], get: () => shop.vRatio * s.T, set: (v) => (shop.vRatio = v / s.T), shop: true },
  };
  const val = (k) => (CTL[k].get ? CTL[k].get() : s[k]);
  function setVal(k, v) {
    const c = CTL[k];
    if (c.get) {
      c.set(v);
      saveShop();
    } else s[k] = v;
  }

  // ---- views: a subject's index ("tab:shop"), "settings", or a lesson id
  let view = "tab:drafting";
  let step = 0;
  let open = false;
  let lastFocus = null;
  let hc = false;
  let done = new Set();
  let savedView = null;

  try {
    const o = JSON.parse(localStorage.getItem("cad.board") || "{}");
    for (const k of Object.keys(DEF)) {
      const v = o.v?.[k];
      if (Number.isFinite(v) && CTL[k]) s[k] = clamp(v, CTL[k].mm[0], CTL[k].mm[1]);
    }
    if (o.v?.unit === "in") s.unit = "in";
    if (o.v?.proc in PROCESSES) s.proc = o.v.proc;
    if (o.v?.mat in MATERIALS) s.mat = o.v.mat;
    if (o.v?.wside === "other") s.wside = "other";
    if (o.v?.proj === "first") s.proj = "first";
    if (["A4", "A3", "Letter", "Tabloid"].includes(o.v?.sheet)) s.sheet = o.v.sheet;
    savedView = o.view;
    if (Number.isInteger(o.step)) step = Math.max(0, o.step);
    hc = !!o.hc;
    done = new Set(Array.isArray(o.done) ? o.done : []);
  } catch (_) {}
  try {
    const o = JSON.parse(localStorage.getItem("cad.shop") || "{}");
    for (const k of Object.keys(SHOP_DEF)) if (Number.isFinite(o[k])) shop[k] = o[k];
    for (const [m, row] of Object.entries(o.mats || {})) if (shop.mats[m]) for (const k of Object.keys(shop.mats[m])) if (Number.isFinite(row[k])) shop.mats[m][k] = row[k];
    for (const [p, row] of Object.entries(o.procs || {})) if (shop.procs[p]) for (const k of Object.keys(shop.procs[p])) if (Number.isFinite(row[k])) shop.procs[p][k] = row[k];
  } catch (_) {}

  function persist() {
    try {
      localStorage.setItem("cad.board", JSON.stringify({ v: s, view, step, hc, done: [...done] }));
    } catch (_) {}
  }
  function saveShop() {
    try {
      const strip = (o) => Object.fromEntries(Object.entries(o).map(([k, row]) => [k, Object.fromEntries(Object.entries(row).filter(([, v]) => typeof v === "number"))]));
      localStorage.setItem("cad.shop", JSON.stringify({ ...Object.fromEntries(Object.keys(SHOP_DEF).map((k) => [k, shop[k]])), mats: strip(shop.mats), procs: strip(shop.procs) }));
    } catch (_) {}
  }

  // ---------------------------------------------------------------- formatting in the chosen unit
  const inch = () => s.unit === "in";
  const U = () => (inch() ? "in" : "mm");
  const num = (v) => (inch() ? (v / 25.4).toFixed(3) : (+v).toFixed(2));
  const len = (v) => `${num(v)} ${U()}`;
  const trim = (x) => String(+(+x).toFixed(3));
  const showK = (k) => {
    const c = CTL[k], v = val(k);
    return c.show ? c.show(v) : len(v);
  };
  /** Run fn with some part numbers swapped in: for worked examples. */
  function withVals(o, fn) {
    const keep = {};
    for (const k of Object.keys(o)) keep[k] = s[k];
    Object.assign(s, o);
    try {
      return fn();
    } finally {
      Object.assign(s, keep);
    }
  }

  // ---------------------------------------------------------------- the maths every lesson shares
  function calc(o = s) {
    const th = rad(o.ang);
    const BA = th * (o.R + o.K * o.T);
    const OSSB = Math.tan(th / 2) * (o.R + o.T);
    const ISSB = Math.tan(th / 2) * o.R;
    const BD = 2 * OSSB - BA;
    return { th, BA, OSSB, ISSB, BD, flat: o.A + o.B - BD, leg1: o.A - OSSB, leg2: o.B - OSSB };
  }
  /** K as shops often start: by inside radius over thickness, unless the shop measured its own. */
  function suggestK() {
    if (shop.kShop > 0) return shop.kShop;
    const q = s.R / s.T;
    return q < 1 ? 0.33 : q < 3 ? 0.4 : 0.5;
  }
  const rule = {
    holeBend: () => shop.holeBend * s.T + s.R,
    slotBend: () => shop.slotBend * s.T + s.R,
    relW: () => shop.relW * s.T,
    relD: () => s.R + shop.relD * s.T,
    minDia: () => PR().hole * s.T,
    minWeb: () => PR().web * s.T,
    V: () => shop.vRatio * s.T,
    dieR: () => MAT().air * shop.vRatio * s.T,
    /** The flat flange must still lie across the die shoulder: half the die
     *  opening from the middle of the bend, turned into an outside dimension. */
    minFlange: (V = shop.vRatio * s.T) => {
      const c = calc();
      return V / 2 + c.OSSB - c.BA / 2;
    },
    leadIn: () => Math.max(PR().leadMin, PR().lead * s.T),
    pierce: () => PR().pierce * PR().kerf,
    minR: (phiDeg) => {
      const m = MAT(), c2 = Math.cos(rad(phiDeg)) ** 2;
      return s.T * (m.across + (m.along - m.across) * c2);
    },
    slotW: () => s.T + 2 * PR().slot,
  };
  const procName = () => PR().name;

  // ---------------------------------------------------------------- chalk drawing helpers
  // Each call numbers its delay so a figure draws in order.
  let n = 0;
  const D = (g = 0.11) => `style="--d:${(n++ * g).toFixed(2)}s"`;
  const path = (d, c = "w", x = "") => `<path class="ch c-${c} ${x}" pathLength="1" ${D()} d="${d}"/>`;
  const dash = (d, c = "b") => `<path class="dsh c-${c}" ${D()} d="${d}"/>`;
  const fill = (d, c = "b") => `<path class="fl c-${c}f" style="--d:.7s" d="${d}"/>`;
  const hatch = (x, y, w, h, pat = "bb-hatch") => `<rect class="fl" style="--d:.8s" x="${f1(x)}" y="${f1(y)}" width="${f1(Math.max(w, 0))}" height="${f1(Math.max(h, 0))}" fill="url(#${pat})"/>`;
  const circle = (cx, cy, r, c = "w", x = "") => `<circle class="ch c-${c} ${x}" pathLength="1" ${D()} cx="${f1(cx)}" cy="${f1(cy)}" r="${f1(Math.max(r, 0.6))}"/>`;
  const rectD = (x, y, w, h) => `M ${f1(x)} ${f1(y)} h ${f1(w)} v ${f1(h)} h ${f1(-w)} Z`;
  const poly = (pts, closed = true) => pts.map((p, i) => `${i ? "L" : "M"} ${f1(p[0])} ${f1(p[1])}`).join(" ") + (closed ? " Z" : "");
  let words = [];
  const label = (x, y, t, c = "w", size = 22, anchor = "start") => {
    words.push(`<text class="lb c-${c}t" ${D()} x="${f1(x)}" y="${f1(y)}" font-size="${size}" text-anchor="${anchor}">${t}</text>`);
    return "";
  };
  const head = (x1, y1, x2, y2) => {
    const a = Math.atan2(y2 - y1, x2 - x1);
    const p = (da) => `${f1(x2 - 9 * Math.cos(a + da))} ${f1(y2 - 9 * Math.sin(a + da))}`;
    return ` M ${p(0.45)} L ${f1(x2)} ${f1(y2)} L ${p(-0.45)}`;
  };
  const arrow = (x1, y1, x2, y2, c = "w") => path(`M ${f1(x1)} ${f1(y1)} L ${f1(x2)} ${f1(y2)}${head(x1, y1, x2, y2)}`, c);
  const dim = (x1, y1, x2, y2, c = "w") => path(`M ${f1(x1)} ${f1(y1)} L ${f1(x2)} ${f1(y2)}${head(x1, y1, x2, y2)}${head(x2, y2, x1, y1)}`, c);
  /** A small chalk tag: a ✓ or ✗ beside a feature. */
  const mark = (x, y, good) => (good ? path(`M ${f1(x - 7)} ${f1(y)} l 5 6 l 10 -13`, "g") : path(`M ${f1(x - 6)} ${f1(y - 6)} l 12 12 M ${f1(x + 6)} ${f1(y - 6)} l -12 12`, "r"));

  /** Wrap a figure. Busy figures draw faster, so every one is finished in
   *  about a second and a half. */
  const svg = (inner, alt, vb) => {
    let out = `<svg viewBox="${vb}" role="img" aria-label="${alt}"><g filter="url(#chalk)">${inner}</g><g filter="url(#chalk-soft)">${words.join("")}</g></svg>`;
    words = [];
    const last = (n - 1) * 0.11;
    if (last > 1.2) {
      const k = 1.2 / last;
      out = out.replace(/--d:([0-9.]+)s/g, (_, d) => `--d:${(+d * k).toFixed(2)}s`);
    }
    return out;
  };

  /** The side view of one bend, in proportion: thickness T and radius R set the
   *  scale; legs are drawn at a fixed length. */
  function profile(o) {
    const th = rad(o.ang ?? s.ang);
    const sc = Math.min(16, (o.fit || 60) / (s.R + s.T));
    const T = s.T * sc;
    const R = s.R * sc;
    const Kt = s.K * s.T * sc;
    const { cx, y0, Lh, Lf } = o;
    const C = [cx, y0 - R];
    const pt = (r, a) => [C[0] + r * Math.sin(a), C[1] + r * Math.cos(a)];
    const d = [Math.cos(th), -Math.sin(th)];
    const add = (p, L) => [p[0] + d[0] * L, p[1] + d[1] * L];
    const a0 = pt(R, 0), a1 = pt(R, th), b0 = pt(R + T, 0), b1 = pt(R + T, th);
    const e1 = add(a1, Lf), e2 = add(b1, Lf);
    const xs = cx - Lh;
    const P = (p) => `${f1(p[0])} ${f1(p[1])}`;
    const body = `M ${xs} ${y0} L ${P(a0)} A ${f1(R)} ${f1(R)} 0 0 0 ${P(a1)} L ${P(e1)} L ${P(e2)} L ${P(b1)} A ${f1(R + T)} ${f1(R + T)} 0 0 1 ${P(b0)} L ${xs} ${f1(y0 + T)} Z`;
    const rn = R + Kt;
    const n0 = pt(rn, 0), n1 = pt(rn, th);
    const arcN = `M ${P(n0)} A ${f1(rn)} ${f1(rn)} 0 0 0 ${P(n1)}`;
    const neutral = `M ${xs + 12} ${f1(y0 + Kt)} L ${P(n0)} A ${f1(rn)} ${f1(rn)} 0 0 0 ${P(n1)} L ${P(add(n1, Lf - 12))}`;
    const hull = fill(body, "b") + path(body, "w");
    return { sc, T, R, Kt, C, pt, d, add, a0, a1, b0, b1, e1, e2, xs, y0, cx, th, body, hull, neutral, arcN, n0, n1, rn, P };
  }

  /** A flat strip: legs and bend zones in proportion, with the flat length above. */
  function strip(parts, y, x0 = 60, w = 520) {
    const total = parts.reduce((a, p) => a + Math.max(p.v, 0), 0) || 1;
    const pp = w / total;
    let x = x0, g = "";
    for (const p of parts) {
      const wd = Math.max(p.v, 0) * pp;
      if (p.bend) g += fill(rectD(x, y, wd, 28), "a") + path(rectD(x, y, wd, 28), "a");
      else g += path(rectD(x, y, wd, 28));
      if (p.t) g += label(x + wd / 2, y + (p.bend ? 96 : 54), p.t, p.bend ? "a" : "w", 19, "middle");
      if (p.t2) g += label(x + wd / 2, y + 74, p.t2, "w", 19, "middle");
      x += wd;
    }
    return g;
  }

  /** A plate seen from above: bend line at x = BL, the amber strip is the bend zone. */
  const plate = (x0, y0, x1, y1, BL) => path(`M ${x0} ${y0} H ${x1} V ${y1} H ${x0} Z`, "w") + fill(`M ${BL} ${y0} h 34 V ${y1} h -34 Z`, "a") + path(`M ${BL} ${y0 - 10} V ${y1 + 10}`, "a");

  const buttons = (act, entries, cur) =>
    entries.map(([k, name]) => `<button class="chalkbtn${cur === k ? " on" : ""}" data-act="${act}" data-v="${k}" aria-pressed="${cur === k}">${name}</button>`).join("");
  const procButtons = () => `<div class="bb-row">${buttons("proc", Object.entries(shop.procs).map(([k, p]) => [k, p.name]), s.proc)}</div>`;
  const matButtons = () => `<div class="bb-row">${buttons("mat", Object.entries(shop.mats).map(([k, m]) => [k, m.name]), s.mat)}</div>`;
  const ln = (h, c = "") => `<div class="ln${c ? " " + c : ""}">${h}</div>`;
  const V_ = (x) => `<span class="v">${x}</span>`;
  const verdict = (good, yes, no, detail = "") => ln(`<span class="${cls(good)}">${good ? yes : no}</span>${detail ? `: ${detail}` : ""}`);
  const warnLn = (t) => ln(`<span class="warn">${t}</span>`);

  // ---------------------------------------------------------------- worked examples
  // Round numbers in the chosen unit, worked by the same functions the sliders drive.
  const LENGTH_KEYS = new Set(Object.keys(CTL).filter((k) => !CTL[k].plain));
  function exVals(mm, inches) {
    if (!inch()) return mm;
    const o = {};
    for (const [k, v] of Object.entries(inches)) o[k] = LENGTH_KEYS.has(k) ? v * 25.4 : v;
    return o;
  }
  const exBox = (lines) => `<div class="ex"><b>Worked example</b>${lines.map((l) => `<div>${l}</div>`).join("")}</div>`;
  const nm = (v) => (inch() ? trim(v / 25.4) : trim(v)); // a round number as the learner would write it

  // ================================================================ FORMING

  // ---------------------------------------------------------------- lesson: bend allowance
  function fig1() {
    const p = profile({ cx: 340, y0: 262, Lh: 230, Lf: 130 });
    const m = p.th / 2;
    const rd = [Math.sin(m), Math.cos(m)];
    const tg = [Math.cos(m), -Math.sin(m)];
    const out = p.pt(p.R + p.T + 20, m);
    const inn = [p.C[0] - rd[0] * 26, p.C[1] - rd[1] * 26];
    let g = p.hull + dash(p.neutral, "a");
    g += arrow(out[0], out[1], out[0] + tg[0] * 22, out[1] + tg[1] * 22, "r") + arrow(out[0], out[1], out[0] - tg[0] * 22, out[1] - tg[1] * 22, "r");
    g += arrow(inn[0] + tg[0] * 24, inn[1] + tg[1] * 24, inn[0] + tg[0] * 5, inn[1] + tg[1] * 5, "b") + arrow(inn[0] - tg[0] * 24, inn[1] - tg[1] * 24, inn[0] - tg[0] * 5, inn[1] - tg[1] * 5, "b");
    const lo = p.pt(p.R + p.T + 58, m);
    g += label(lo[0] + 8, lo[1] - 4, "outside stretches", "r", 21);
    g += label(inn[0] - 150, inn[1] - 16, "inside squeezes", "b", 21);
    g += arrow(p.xs + 80, p.y0 + p.T + 38, p.xs + 80, p.y0 + p.Kt + 4, "a");
    g += label(p.xs + 80, p.y0 + p.T + 62, "neutral axis: keeps its length", "a", 21, "start");
    return svg(g, "Side view of a bent flange. The outside surface stretches, the inside squeezes, and a dashed neutral axis between them keeps its length.", "80 50 520 340");
  }

  function fig2() {
    const p = profile({ cx: 340, y0: 250, Lh: 230, Lf: 130 });
    const x1 = p.xs + 70, x2 = p.xs + 130;
    let g = p.hull + dash(p.neutral, "a");
    g += dim(x1, p.y0, x1, p.y0 + p.T, "w") + label(x1 - 10, p.y0 + p.T / 2 + 7, "T", "w", 22, "end");
    g += dim(x2, p.y0, x2, p.y0 + p.Kt, "a") + label(x2 + 10, p.y0 + p.Kt / 2 + 7, "K·T", "a", 22);
    g += label(p.xs + 4, p.y0 - 14, "inside surface", "b", 19);
    g += label(p.xs + 4, p.y0 + p.T + 28, "outside surface", "w", 19);
    return svg(g, "The same bend with thickness T and the distance K times T from the inside surface to the neutral axis marked.", "80 50 520 340");
  }

  function fig3() {
    const p = profile({ cx: 340, y0: 262, Lh: 230, Lf: 130 });
    const th = p.th;
    const ra = p.R + p.T + 30;
    const m = th / 2;
    const A0 = p.pt(ra, 0), A1 = p.pt(ra, th);
    let g = p.hull + dash(p.neutral, "a");
    g += dash(`M ${p.P(p.C)} L ${p.P(p.pt(ra + 8, 0))}`, "b") + dash(`M ${p.P(p.C)} L ${p.P(p.pt(ra + 8, th))}`, "b");
    g += path(`M ${p.P(A0)} A ${f1(ra)} ${f1(ra)} 0 0 0 ${p.P(A1)}`, "b");
    const tl = p.pt(ra + 22, m);
    g += label(tl[0] + 4, tl[1] + 6, "θ", "b", 26);
    g += path(p.arcN, "a", "thick");
    g += arrow(p.cx + 10, p.y0 + p.T + 44, p.n0[0] + 4, p.n0[1] + 4, "a");
    g += label(p.cx + 14, p.y0 + p.T + 66, "BA", "a", 26);
    return svg(g, "The bend with its angle theta marked and the neutral axis through the bend highlighted as the bend allowance.", "80 50 520 340");
  }

  function fig4() {
    const c = calc();
    const p = profile({ cx: 300, y0: 120, Lh: 200, Lf: 62 });
    const os = c.OSSB * p.sc;
    const V = [p.b0[0] + os, p.b0[1]];
    const nrm = [Math.sin(p.th), Math.cos(p.th)];
    const yA = p.y0 + p.T + 30;
    let g = p.hull;
    g += dash(`M ${p.P(p.b0)} L ${p.P(V)} L ${p.P(p.b1)}`, "b");
    g += dim(p.xs, yA, V[0], yA, "w") + label((p.xs + V[0]) / 2, yA - 8, "A", "w", 22, "middle");
    g += dim(p.b0[0], yA + 22, V[0], yA + 22, "a") + label(V[0] + 10, yA + 30, "OSSB", "a", 19);
    const bs = [V[0] + nrm[0] * 30, V[1] + nrm[1] * 30], be = [p.e2[0] + nrm[0] * 30, p.e2[1] + nrm[1] * 30];
    g += dim(bs[0], bs[1], be[0], be[1], "w") + label((bs[0] + be[0]) / 2 + 16, (bs[1] + be[1]) / 2 + 8, "B", "w", 22);
    const y = 308;
    g += dim(60, y - 18, 580, y - 18, "g") + label(320, y - 28, `flat length ${len(c.flat)}`, "g", 22, "middle");
    g += strip([{ v: c.leg1, t: "A − OSSB", t2: num(c.leg1) }, { v: c.BA, bend: true, t: `BA ${num(c.BA)}` }, { v: c.leg2, t: "B − OSSB", t2: num(c.leg2) }], y);
    return svg(g, "A bent part with outside legs A and B and the outside setback, above the same part unfolded into a flat strip.", "40 70 560 360");
  }

  function fig5() {
    const BL = 450, pp = 7, top = 56, bot = 236, hy = 146, r = 21;
    const need = rule.holeBend();
    const hx = BL - s.h * pp - r;
    const okH = ok(s.h, need);
    const cH = col(okH);
    let g = path(`M 56 ${top} H ${BL} V ${bot} H 56 Z`, "w");
    const bz = Math.min(calc().BA * 7, 70);
    g += fill(`M ${BL} ${top} h ${f1(bz)} V ${bot} h ${f1(-bz)} Z`, "a");
    g += path(`M ${BL} ${top - 12} V ${bot + 12}`, "a");
    g += label(BL + 6, top - 16, "bend line", "a", 18);
    const zx = BL - need * pp;
    g += hatch(zx, top, need * pp, bot - top);
    g += dash(`M ${f1(zx)} ${top} V ${bot}`, "r");
    g += circle(hx, hy, r, cH);
    const yh = hy + r + 24;
    g += dim(hx + r, yh, BL, yh, cH) + label(hx + r + (BL - hx - r) / 2, yh + 22, `h = ${num(s.h)}`, cH, 19, "middle");
    g += dim(zx, bot + 28, BL, bot + 28, "r") + label(zx, bot + 52, `needs ${num(need)}  (${trim(shop.holeBend)}·T + R)`, "r", 18);
    const fx = 56, fy = 356, fp = 7, fneed = rule.minFlange();
    const okF = ok(s.f, fneed);
    const cF = col(okF);
    g += label(fx, fy - 14, "flange length f", "w", 19);
    g += path(`M ${fx} ${fy} h ${f1(Math.max(s.f * fp, 2))} v 22 h ${f1(-Math.max(s.f * fp, 2))} Z`, cF);
    g += path(`M ${f1(fx + clamp(fneed, 0, 70) * fp)} ${fy - 8} v 38`, "a");
    g += label(fx + clamp(fneed, 0, 70) * fp + 8, fy + 48, `needs ${num(fneed)}  (die ${trim(shop.vRatio)}·T)`, "a", 18);
    return svg(g, "Top view of a flat plate with a hole near the bend line, and a bar comparing the flange length with the minimum.", "30 30 580 410");
  }

  const BEND = {
    id: "bend",
    title: "Bend allowance",
    q: "How long is the flat blank?",
    blurb: "How a flat pattern gets its length",
    src: "Neutral-axis model: BA = θ(R + K·T), OSSB = tan(θ/2)(R + T), BD = 2·OSSB − BA. Standard sheet-metal geometry found in any press-brake handbook.",
    steps: [
      {
        t: "Metal doesn't bend. It stretches and squeezes.",
        p: "Fold a flange and the outside surface stretches while the inside squeezes. One line between them keeps its original length. That line is the neutral axis, and the flat pattern is measured along it.",
        ctl: ["T", "R"],
        fig: fig1,
        math: () => ln(`${V_("neutral axis")} = the line that keeps its length`) + ln("Thicker or tighter bends move it further toward the inside."),
        note: "3 mm is about 11 gauge mild steel. 1/8 in is 3.175 mm.",
      },
      {
        t: "K-factor says where that line sits",
        p: "K is the distance from the inside surface to the neutral axis, divided by the thickness. K = 0.5 would put it dead centre. Real bends push it toward the inside, so most shops work between 0.33 and 0.50.",
        ctl: ["R", "K"],
        fig: fig2,
        math: () => ln(`K = K·T ÷ T = ${V_(num(s.K * s.T))} ÷ ${num(s.T)} = ${V_(s.K.toFixed(2))}`) + ln(`R ÷ T = ${(s.R / s.T).toFixed(2)}: ${shop.kShop > 0 ? "your shop's measured K is" : "shops often start near"} ${V_(suggestK().toFixed(2))}`, "dim"),
        basis: () => (shop.kShop > 0 ? "K from your own test bends (Shop settings)." : "Starting K by R ÷ T: under 1 → 0.33, 1 to 3 → 0.40, over 3 → 0.50. Measure a test bend and put your own K in Shop settings."),
        extra: () => `<button class="chalkbtn" data-act="suggest">Use K = ${suggestK().toFixed(2)}</button>`,
      },
      {
        t: "Bend allowance is the arc along it",
        p: "The bend allowance (BA) is the length of the neutral axis through the bend: the angle in radians times the radius of the neutral axis. θ is how far the flange turns, so a square corner is 90°.",
        ctl: ["T", "R", "K", "ang"],
        fig: fig3,
        ex: () => {
          const o = exVals({ T: 2, R: 2, K: 0.4, ang: 90 }, { T: 0.125, R: 0.125, K: 0.4, ang: 90 });
          const c = withVals(o, () => calc());
          return exBox([`T ${nm(o.T)}, R ${nm(o.R)}, K 0.40, θ 90° = 1.571 rad`, `BA = 1.571 × (${nm(o.R)} + 0.40 × ${nm(o.T)}) = <span class="v">${len(c.BA)}</span>`]);
        },
        math: () => {
          const c = calc();
          return ln("BA = θ × (R + K·T)", "dim") + ln(`= ${c.th.toFixed(3)} × (${num(s.R)} + ${s.K.toFixed(2)} × ${num(s.T)})`) + ln(`= ${V_(len(c.BA))}`);
        },
      },
      {
        t: "Flat length from the outside dimensions",
        p: "Drawings dimension to the outside corner. Pull back from that corner by the outside setback (OSSB) to find where each bend starts. The bend deduction (BD) is how much the two outside legs overshoot the flat.",
        ctl: ["A", "B", "W"],
        fig: fig4,
        ex: () => {
          const o = exVals({ T: 2, R: 2, K: 0.4, ang: 90, A: 50, B: 30 }, { T: 0.125, R: 0.125, K: 0.4, ang: 90, A: 2, B: 1 });
          const c = withVals(o, () => calc());
          return exBox([`T ${nm(o.T)}, R ${nm(o.R)}, K 0.40, 90°, A ${nm(o.A)}, B ${nm(o.B)}`, `OSSB = tan 45° × (${nm(o.R)} + ${nm(o.T)}) = ${num(c.OSSB)}`, `BD = 2 × ${num(c.OSSB)} − ${num(c.BA)} = ${num(c.BD)}`, `flat = ${nm(o.A)} + ${nm(o.B)} − ${num(c.BD)} = <span class="v">${len(c.flat)}</span>`]);
        },
        math: () => {
          const c = calc();
          const bad = c.leg1 < 0 || c.leg2 < 0;
          return ln(`OSSB = tan(θ÷2) × (R + T) = ${V_(len(c.OSSB))}`) + ln(`BD = 2 × OSSB − BA = ${V_(len(c.BD))}`) + ln(`flat = A + B − BD = <span class="${bad ? "bad" : "ok"}">${len(c.flat)}</span>`) + (bad ? ln("A or B is shorter than the setback.", "bad") : "");
        },
        place: { label: "Draw this flat on my drawing", ready: () => calc().leg1 >= 0 && calc().leg2 >= 0, run: () => placeFlat([s.A, s.B]) },
      },
      {
        t: "Check it before the shop does",
        p: "Two checks catch most scrapped bends: a hole too close to the bend, and a flange too short for the die. Both limits come from your Shop settings.",
        ctl: ["h", "f"],
        fig: fig5,
        math: () => {
          const okH = ok(s.h, rule.holeBend()), okF = ok(s.f, rule.minFlange());
          return verdict(okH, "Hole is clear", "Hole is too close", `${num(s.h)} of ${num(rule.holeBend())} needed`) + verdict(okF, "Flange holds", "Flange is too short", `${num(s.f)} of ${num(rule.minFlange())} needed`) + ln(`Relief cuts at least ${V_(num(rule.relW()))} wide where a bend meets an edge.`, "dim");
        },
        basis: () => `Hole: ${trim(shop.holeBend)}·T + R. Flange: half a ${trim(shop.vRatio)}·T die opening plus the setback. Your press brake's die chart has the final say.`,
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: bend deduction and the brake table
  function bdFig1() {
    const c = calc();
    const p = profile({ cx: 300, y0: 116, Lh: 200, Lf: 62 });
    const os = c.OSSB * p.sc;
    const Vx = [p.b0[0] + os, p.b0[1]];
    let g = p.hull + dash(`M ${p.P(p.b0)} L ${p.P(Vx)} L ${p.P(p.b1)}`, "b");
    g += label(Vx[0] + 12, Vx[1] + 6, "outside corner", "b", 18);
    const x0 = 60, W = 520, pp = W / Math.max(s.A + s.B, 1);
    const yA = 250, yF = 320;
    g += path(rectD(x0, yA, s.A * pp, 24)) + path(rectD(x0 + s.A * pp, yA, s.B * pp, 24));
    g += label(x0 + (s.A * pp) / 2, yA - 10, `A ${num(s.A)}`, "w", 19, "middle") + label(x0 + s.A * pp + (s.B * pp) / 2, yA - 10, `B ${num(s.B)}`, "w", 19, "middle");
    const fl = Math.max(c.flat, 0) * pp;
    g += fill(rectD(x0, yF, fl, 24), "b") + path(rectD(x0, yF, fl, 24), "g");
    g += label(x0 + fl / 2, yF + 50, `flat ${num(c.flat)}`, "g", 20, "middle");
    g += fill(rectD(x0 + fl, yF, Math.max(c.BD, 0) * pp, 24), "r") + path(rectD(x0 + fl, yF, Math.max(c.BD, 0) * pp, 24), "r");
    g += label(x0 + fl + (c.BD * pp) / 2, yF + 50, `BD ${num(c.BD)}`, "r", 20, "middle");
    g += dash(`M ${f1(x0 + fl)} ${yA + 24} V ${yF}`, "r");
    return svg(g, "A bent part, then two bars: the outside legs A and B laid end to end, and the shorter flat blank. The difference at the end is the bend deduction.", "40 50 560 340");
  }

  function bdFig2() {
    const c = calc();
    const p = profile({ cx: 300, y0: 170, Lh: 200, Lf: 110 });
    const is = c.ISSB * p.sc, os = c.OSSB * p.sc;
    const Vi = [p.a0[0] + is, p.y0];
    const Vo = [p.b0[0] + os, p.b0[1]];
    let g = p.hull;
    g += dash(`M ${p.P(p.a0)} L ${p.P(Vi)} L ${p.P(p.a1)}`, "a") + dash(`M ${p.P(p.b0)} L ${p.P(Vo)} L ${p.P(p.b1)}`, "b");
    g += dim(p.xs, p.y0 - 26, Vi[0], p.y0 - 26, "a") + label((p.xs + Vi[0]) / 2, p.y0 - 36, "a (inside)", "a", 20, "middle");
    const nrm = [-Math.sin(p.th), -Math.cos(p.th)];
    const s0 = [Vi[0] + nrm[0] * 26, Vi[1] + nrm[1] * 26], s1 = [p.e1[0] + nrm[0] * 26, p.e1[1] + nrm[1] * 26];
    g += dim(s0[0], s0[1], s1[0], s1[1], "a") + label((s0[0] + s1[0]) / 2 - 16, (s0[1] + s1[1]) / 2, "b", "a", 22, "end");
    g += dim(p.xs, p.y0 + p.T + 30, Vo[0], p.y0 + p.T + 30, "b") + label((p.xs + Vo[0]) / 2, p.y0 + p.T + 56, "A (outside)", "b", 20, "middle");
    return svg(g, "One bend with the inside mold lines meeting at the inside corner, and the inside leg a and b measured to it; the outside leg A is shown below for comparison.", "60 40 540 330");
  }

  function bdFig3() {
    const c = calc();
    const p = profile({ cx: 280, y0: 230, Lh: 190, Lf: 130 });
    const os = c.OSSB * p.sc;
    const Vo = [p.b0[0] + os, p.b0[1]];
    const ext = [Vo[0] + 120, Vo[1]];
    const fd = [Math.cos(p.th), -Math.sin(p.th)];
    let g = p.hull + dash(`M ${p.P(p.b0)} L ${p.P(Vo)} L ${p.P(p.b1)}`, "b");
    g += dash(`M ${f1(Vo[0])} ${f1(Vo[1])} L ${f1(ext[0])} ${f1(ext[1])}`, "w");
    const r1 = 70, r2 = 44;
    const A1 = [Vo[0] + r1 * fd[0], Vo[1] + r1 * fd[1]];
    g += path(`M ${f1(Vo[0] + r1)} ${f1(Vo[1])} A ${r1} ${r1} 0 0 0 ${f1(A1[0])} ${f1(A1[1])}`, "a");
    g += label(Vo[0] + r1 * Math.cos(p.th / 2) + 8, Vo[1] - r1 * Math.sin(p.th / 2), `θ ${Math.round(s.ang)}°`, "a", 21);
    const B1 = [Vo[0] + r2 * fd[0], Vo[1] + r2 * fd[1]];
    g += path(`M ${f1(B1[0])} ${f1(B1[1])} A ${r2} ${r2} 0 0 0 ${f1(Vo[0] - r2)} ${f1(Vo[1])}`, "b");
    const mid = (p.th + Math.PI) / 2;
    g += label(Vo[0] + (r2 + 16) * Math.cos(mid) - 10, Vo[1] - (r2 + 16) * Math.sin(mid), `${Math.round(180 - s.ang)}° included`, "b", 19, "end");
    return svg(g, "A bend showing the bend angle theta, measured from the straight-on line, and the included angle between the two legs.", "60 40 540 320");
  }

  /** A U-channel outline: outside legs A (left), B (base), C (right), in px. */
  function channel(x0, yb, A, B, C, t, r) {
    const ro = r + t;
    return `M ${f1(x0)} ${f1(yb - A)} V ${f1(yb - ro)} A ${f1(ro)} ${f1(ro)} 0 0 0 ${f1(x0 + ro)} ${f1(yb)} H ${f1(x0 + B - ro)} A ${f1(ro)} ${f1(ro)} 0 0 0 ${f1(x0 + B)} ${f1(yb - ro)} V ${f1(yb - C)} H ${f1(x0 + B - t)} V ${f1(yb - t - r)} A ${f1(r)} ${f1(r)} 0 0 1 ${f1(x0 + B - t - r)} ${f1(yb - t)} H ${f1(x0 + t + r)} A ${f1(r)} ${f1(r)} 0 0 1 ${f1(x0 + t)} ${f1(yb - t - r)} V ${f1(yb - A)} Z`;
  }

  function bdFig4() {
    const c = calc();
    const big = Math.max(s.A, s.C, s.B * 0.6, 1);
    const sc = Math.min(150 / big, 300 / s.B);
    const t = clamp(s.T * sc, 3, 14), r = clamp(s.R * sc, 0, 30);
    const x0 = 300 - (s.B * sc) / 2, yb = 200;
    const d = channel(x0, yb, s.A * sc, s.B * sc, s.C * sc, t, r);
    let g = fill(d, "b") + path(d, "w");
    g += dim(x0 - 22, yb, x0 - 22, yb - s.A * sc, "w") + label(x0 - 30, yb - (s.A * sc) / 2, "A", "w", 22, "end");
    g += dim(x0, yb + 22, x0 + s.B * sc, yb + 22, "w") + label(300, yb + 48, "B", "w", 22, "middle");
    g += dim(x0 + s.B * sc + 22, yb, x0 + s.B * sc + 22, yb - s.C * sc, "w") + label(x0 + s.B * sc + 30, yb - (s.C * sc) / 2, "C", "w", 22);
    const mid = s.B - 2 * c.OSSB;
    g += dim(60, 286, 580, 286, "g") + label(320, 276, `flat ${len(s.A + s.B + s.C - 2 * c.BD)}`, "g", 21, "middle");
    g += strip([{ v: c.leg1, t: "A − OSSB" }, { v: c.BA, bend: true, t: "BA" }, { v: mid, t: "B − 2·OSSB" }, { v: c.BA, bend: true, t: "BA" }, { v: s.C - c.OSSB, t: "C − OSSB" }], 300);
    return svg(g, "A U-shaped channel with outside legs A, B and C, above its flat blank with two bend zones.", "40 20 560 400");
  }

  const TABLE_T = { mm: [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10], in: [0.036, 0.048, 0.06, 0.075, 0.105, 0.135, 0.1875, 0.25, 0.3125, 0.375] };
  function brakeRows() {
    const q = s.R / s.T;
    return (inch() ? TABLE_T.in.map((v) => v * 25.4) : TABLE_T.mm).map((T) => {
      const o = { ...s, T, R: q * T };
      const c = calc(o);
      return { T, R: o.R, BA: c.BA, OSSB: c.OSSB, BD: c.BD };
    });
  }
  function bdFig5() {
    const rows = brakeRows();
    const cols = [["T", 70], ["R", 170], ["BA", 290], ["OSSB", 410], ["BD", 530]];
    let g = "";
    cols.forEach(([h, x]) => label(x, 70, h, h === "BD" ? "a" : "b", 22, "middle"));
    g += path("M 30 82 H 580", "w");
    const near = rows.reduce((b, r, i) => (Math.abs(r.T - s.T) < Math.abs(rows[b].T - s.T) ? i : b), 0);
    rows.forEach((r, i) => {
      const y = 112 + i * 29;
      const c = i === near ? "a" : "w";
      label(70, y, num(r.T), c, 19, "middle");
      label(170, y, num(r.R), c, 19, "middle");
      label(290, y, num(r.BA), c, 19, "middle");
      label(410, y, num(r.OSSB), c, 19, "middle");
      label(530, y, num(r.BD), i === near ? "a" : "g", 19, "middle");
    });
    g += path("M 120 50 V 400 M 230 50 V 400 M 350 50 V 400 M 470 50 V 400", "b");
    return svg(g, "A table of thickness, inside radius, bend allowance, outside setback and bend deduction for ten thicknesses.", "20 30 580 380");
  }

  const BD = {
    id: "deduct",
    title: "Bend deduction and the brake table",
    q: "What number do I give the brake operator?",
    blurb: "Outside, inside and included angles, more bends, your own table",
    src: "Same neutral-axis model as Bend allowance; ISSB = tan(θ/2)·R; outside = inside + T·tan(θ/2).",
    steps: [
      {
        t: "The number the operator subtracts",
        p: "Lay the two outside legs end to end and they come out longer than the flat blank. The overshoot is the bend deduction (BD). An operator who knows BD can check a blank with a tape: legs added up, minus BD.",
        ctl: ["T", "R", "K", "ang"],
        fig: bdFig1,
        ex: () => {
          const o = exVals({ T: 2, R: 2, K: 0.4, ang: 90, A: 50, B: 30 }, { T: 0.125, R: 0.125, K: 0.4, ang: 90, A: 2, B: 1 });
          const c = withVals(o, () => calc());
          return exBox([`T ${nm(o.T)}, R ${nm(o.R)}, K 0.40, 90°`, `BD = 2 × ${num(c.OSSB)} − ${num(c.BA)} = <span class="v">${len(c.BD)}</span>`, `A ${nm(o.A)} + B ${nm(o.B)} − BD = ${len(c.flat)}`]);
        },
        math: () => {
          const c = calc();
          return ln(`BD = 2·OSSB − BA = 2 × ${num(c.OSSB)} − ${num(c.BA)} = ${V_(len(c.BD))}`) + ln(`flat = A + B − BD = ${num(s.A)} + ${num(s.B)} − ${num(c.BD)} = ${V_(len(c.flat))}`);
        },
      },
      {
        t: "Dimensioned to the inside?",
        p: "Some drawings measure legs to the inside corner. Then pull back by the inside setback (ISSB) instead, and add the bend allowance once. Mixing the two methods is a classic scrap maker: say on the drawing which one you used.",
        ctl: ["ai", "bi", "T", "R"],
        fig: bdFig2,
        math: () => {
          const c = calc();
          const flat = s.ai + s.bi - 2 * c.ISSB + c.BA;
          const dx = s.T * Math.tan(c.th / 2);
          return ln(`ISSB = tan(θ÷2) × R = ${V_(len(c.ISSB))}`) + ln(`flat = a + b − 2·ISSB + BA = ${V_(len(flat))}`) + ln(`same part outside: A = a + T·tan(θ÷2) = ${num(s.ai + dx)}, B = ${num(s.bi + dx)}`, "dim");
        },
        note: "Both methods give the same blank for the same part. The legs just start from different corners.",
      },
      {
        t: "Bend angle or included angle?",
        p: "θ is how far the flange turns. The included angle is what's left between the legs: 180° − θ. A drawing that says \"45°\" could mean either, and the blank differs. Write which one you mean.",
        ctl: ["ang"],
        fig: bdFig3,
        math: () => {
          const a = calc(), b = calc({ ...s, ang: 180 - s.ang });
          const diff = Math.abs(a.BA - b.BA);
          return ln(`bend ${Math.round(s.ang)}° → BA ${V_(num(a.BA))}`) + ln(`read as included → bend ${Math.round(180 - s.ang)}°, BA ${V_(num(b.BA))}`) + (diff > 0.005 ? ln(`<span class="bad">Mix them up</span> and the bend is off by ${len(diff)}`) : ln("At 90° they agree.", "ok"));
        },
      },
      {
        t: "More bends, more deductions",
        p: "Every bend takes off one bend deduction. A channel has two bends, so its blank is A + B + C − 2·BD. The base between the bends also has to sit on the die for the second bend.",
        ctl: ["A", "B", "C"],
        fig: bdFig4,
        math: () => {
          const c = calc();
          const flat = s.A + s.B + s.C - 2 * c.BD;
          const fmin = rule.minFlange();
          return ln(`flat = A + B + C − 2·BD = ${V_(len(flat))}`) + verdict(s.B - 2 * c.OSSB >= 0, "Base has a flat between the bends", "Base is shorter than two setbacks") + verdict(ok(s.B, fmin), "Base sits on the die", "Base is short for the die", `${num(s.B)} of ${num(fmin)}`);
        },
        basis: () => `Base check uses the shortest flange for a ${trim(shop.vRatio)}·T die (see Minimum flange and die).`,
        place: { label: "Draw this channel flat", ready: () => calc().leg1 >= 0 && s.B - 2 * calc().OSSB >= 0, run: () => placeFlat([s.A, s.B, s.C]) },
      },
      {
        t: "Your brake table",
        p: "Shops keep a table: thickness down the side, the numbers the brake needs across. This one keeps your R ÷ T, your K and your angle. Print it and pin it by the brake.",
        ctl: ["R", "K", "ang"],
        fig: bdFig5,
        math: () => ln(`R ÷ T = ${V_((s.R / s.T).toFixed(2))} · K = ${V_(s.K.toFixed(2))} · θ = ${V_(`${Math.round(s.ang)}°`)}`) + ln(`row in amber: your thickness, ${num(s.T)}`, "dim"),
        basis: "Worked from the formulas, not measured. Replace K with the value from your own test bends and the table follows.",
        extra: () => `<button class="chalkbtn" data-act="print-table">Print this table</button>`,
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: minimum flange and die choice
  /** A V-die in section with a flat sheet across it. `flat` is how far the
   *  flange reaches past the middle of the bend, in mm. */
  function dieFig({ V = rule.V(), flat = null, punch = true } = {}) {
    const sc = clamp(230 / Math.max(V, 1), 1, 14);
    const cx = 300, top = 232, Vp = V * sc, Tp = clamp(s.T * sc, 3, 40);
    const depth = Math.min(Vp / 2, 110);
    let g = "";
    const die = `M 60 ${top} H ${f1(cx - Vp / 2)} L ${cx} ${f1(top + depth)} L ${f1(cx + Vp / 2)} ${top} H 560 V ${top + 140} H 60 Z`;
    g += fill(die, "b") + path(die, "w");
    const reach = flat === null ? 240 : clamp(flat * sc, 0, 260);
    const falls = flat !== null && flat < V / 2;
    const sheet = rectD(cx - 230, top - Tp, 230 + reach, Tp);
    g += path(sheet, falls ? "r" : "w");
    if (punch) {
      const pd = `M ${cx} ${f1(top - Tp - 2)} L ${cx - 30} ${f1(top - Tp - 90)} V 40 H ${cx + 30} V ${f1(top - Tp - 90)} Z`;
      g += path(pd, "w");
    }
    g += dim(cx - Vp / 2, top + 64, cx + Vp / 2, top + 64, "a") + label(cx, top + 92, `V ${num(V)}`, "a", 20, "middle");
    return { g, sc, cx, top, Vp, Tp, falls, reach };
  }

  function dieFig1() {
    const d = dieFig();
    let g = d.g;
    g += label(d.cx + 40, 70, "punch", "w", 19) + label(80, d.top + 124, "V-die", "b", 19);
    g += label(d.cx + d.Vp / 2 + 10, d.top - 12, "shoulder", "b", 18);
    return svg(g, "A punch above a sheet resting across the two shoulders of a V-shaped die. The die opening V is marked.", "40 20 560 400");
  }

  function dieFig2() {
    const dieR = rule.dieR();
    const big = Math.max(dieR, s.R, 0.5) + s.T;
    const sc = clamp(200 / big, 2, 60);
    const ox = 380, oy = 320;
    const T = s.T * sc;
    // the outside corner of an L, legs going left and up
    const L = (r, c, dsh) => {
      const ri = r * sc, ro = ri + T;
      const d = `M 80 ${oy} H ${f1(ox - ro)} A ${f1(ro)} ${f1(ro)} 0 0 0 ${ox} ${f1(oy - ro)} V 40`;
      const di = `M 80 ${f1(oy - T)} H ${f1(ox - T - ri)} ${ri > 0.5 ? `A ${f1(ri)} ${f1(ri)} 0 0 0 ${f1(ox - T)} ${f1(oy - T - ri)}` : ""} V 40`;
      return dsh ? dash(d, c) + dash(di, c) : path(d, c) + path(di, c);
    };
    let g = L(s.R, "w", false) + L(dieR, "a", true);
    g += label(90, oy + 34, `solid: drawn, R ${num(s.R)}`, "w", 20) + label(90, oy + 62, `dashed: die makes R ≈ ${num(dieR)}`, "a", 20);
    return svg(g, "Two L-shaped corners on top of each other: the inside radius as drawn, and the larger or smaller radius the die will actually make.", "60 20 520 390");
  }

  function dieFig3() {
    const c = calc();
    const flat = s.f - c.OSSB + c.BA / 2;
    const d = dieFig({ flat });
    let g = d.g;
    const x = d.cx + d.reach;
    g += dim(d.cx, d.top - d.Tp - 22, x, d.top - d.Tp - 22, d.falls ? "r" : "g");
    g += label((d.cx + x) / 2, d.top - d.Tp - 32, `flat ${num(flat)}`, d.falls ? "r" : "g", 19, "middle");
    g += label(d.cx + d.Vp / 2 + 8, d.top + 40, d.falls ? "drops into the V" : "stays on the shoulder", d.falls ? "r" : "g", 19);
    return svg(g, "A sheet across a V-die. The flange side reaches past the middle of the bend; if it is shorter than half the die opening it drops into the V.", "40 20 560 400");
  }

  function dieFig4() {
    const c = calc();
    const flat = s.f - c.OSSB + c.BA / 2;
    const Vmax = Math.max(2 * flat, 0.5);
    const d = dieFig({ V: Vmax, flat });
    let g = d.g + label(d.cx, 60, "largest die this flange can use", "a", 20, "middle");
    return svg(g, "The largest V-die opening that this flange can still sit across.", "40 20 560 400");
  }

  const DIE = {
    id: "die",
    title: "Minimum flange and die choice",
    q: "Can this flange be formed?",
    blurb: "The die sets the radius and the shortest flange",
    src: "Die opening 6–12 × T with 8 × T common; air-formed inside radius as a fraction of the die opening by material (e.g. about 15% for cold-rolled steel, 20% for 304). Published press-brake practice.",
    steps: [
      {
        t: "Air bending on a V-die",
        p: "Most press brakes air bend: the punch pushes the sheet down into a V-shaped die, and the sheet rests only on the two shoulders. The die opening (V) is the shop's choice, and it is usually a multiple of the thickness.",
        ctl: ["T", "V"],
        fig: dieFig1,
        math: () => {
          const r = shop.vRatio;
          return ln(`V = ${V_(trim(r.toFixed(1)))} × T = ${V_(len(rule.V()))}`) + (r < 6 || r > 12 ? warnLn(`${trim(r.toFixed(1))} × T is outside the usual 6 to 12 × T.`) : ln("Within the usual 6 to 12 × T.", "dim"));
        },
        basis: "Die opening is stored as a multiple of T in Shop settings, so it follows the thickness. 8 × T is a common start for light gauge; heavier plate often uses 10 × T.",
      },
      {
        t: "The die sets the radius",
        p: "In air bending you don't get the radius you drew. You get roughly a fixed fraction of the die opening, and the fraction depends on the material. Draw the radius the die makes, or ask for a die that makes the radius you drew.",
        ctl: ["T", "V", "R"],
        fig: dieFig2,
        extra: matButtons,
        math: () => {
          const m = MAT(), dr = rule.dieR();
          const off = Math.abs(dr - s.R) > Math.max(0.15 * dr, 0.1);
          return ln(`R ≈ ${V_(trim(m.air))} × V = ${trim(m.air)} × ${num(rule.V())} = ${V_(len(dr))}`) + (off ? ln(`<span class="warn">Drawn ${num(s.R)} differs.</span> For R ${num(s.R)} the die would be about ${V_(len(s.R / m.air))}`) : ln("Drawn radius matches the die.", "ok"));
        },
        basis: () => `${MAT().name}: radius ≈ ${trim(MAT().air)} × V, a published rule of thumb for air forming. Real radii vary with the batch; measure a test bend.`,
      },
      {
        t: "The shortest flange",
        p: "Through the whole bend, the flange has to stay on the far shoulder. In the flat that means reaching at least half the die opening past the middle of the bend. Turn that into an outside dimension and you have the shortest flange.",
        ctl: ["T", "V", "f"],
        fig: dieFig3,
        ex: () => {
          const o = exVals({ T: 2, R: 2, K: 0.4, ang: 90 }, { T: 0.125, R: 0.125, K: 0.4, ang: 90 });
          const Vx = shop.vRatio * o.T;
          const c = withVals(o, () => calc());
          return exBox([`T ${nm(o.T)}, R ${nm(o.R)}, 90°, V = ${trim(shop.vRatio)} × T = ${num(Vx)}`, `f min = V÷2 + OSSB − BA÷2 = ${num(Vx / 2)} + ${num(c.OSSB)} − ${num(c.BA / 2)} = <span class="v">${len(Vx / 2 + c.OSSB - c.BA / 2)}</span>`]);
        },
        math: () => {
          const c = calc(), need = rule.minFlange();
          return ln("f min = V÷2 + OSSB − BA÷2", "dim") + ln(`= ${num(rule.V() / 2)} + ${num(c.OSSB)} − ${num(c.BA / 2)} = ${V_(len(need))}`) + verdict(ok(s.f, need), "Flange can be formed", "Flange is too short", `${num(s.f)} of ${num(need)}`);
        },
        basis: "A geometric minimum with no margin. Die charts list the true minimum for each die; add a little so the edge doesn't slip.",
      },
      {
        t: "When the flange is too short",
        p: "Three ways out: a smaller die (smaller radius, more force, and a deeper mark), a longer flange that is trimmed after forming, or a different part. The figure shows the largest die your flange can still use.",
        ctl: ["T", "f"],
        fig: dieFig4,
        math: () => {
          const c = calc();
          const Vmax = 2 * (s.f - c.OSSB + c.BA / 2);
          const ratio = Vmax / s.T;
          return ln(`largest die = 2 × (f − OSSB + BA÷2) = ${V_(len(Math.max(Vmax, 0)))}`) + ln(`= ${V_(ratio > 0 ? ratio.toFixed(1) : "0")} × T`) + (ratio >= 6 ? ln("A standard die can do it.", "ok") : ratio > 0 ? warnLn("Below 6 × T: ask the shop, or lengthen and trim.") : ln("No die can form this. Lengthen the flange.", "bad"));
        },
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: grain direction
  /** Grain streaks across a sheet, with the rolling direction along x. */
  function grainSheet(x0, y0, w, h) {
    let g = path(rectD(x0, y0, w, h), "w");
    for (let y = y0 + 16; y < y0 + h - 8; y += 22) {
      let d = "";
      for (let x = x0 + 10 + ((y * 7) % 30); x < x0 + w - 40; x += 70) d += `M ${f1(x)} ${f1(y)} h ${f1(26 + ((x + y) % 18))} `;
      g += `<path class="dsh c-b" style="--d:.1s;opacity:.55" d="${d}"/>`;
    }
    return g;
  }
  function crackFlag(x, y, good) {
    if (good) return label(x, y, "bends clean", "g", 19, "middle");
    return path(`M ${f1(x - 30)} ${f1(y - 26)} l 8 8 l -4 6 l 9 6 M ${f1(x)} ${f1(y - 28)} l -5 9 l 7 5 l -3 8 M ${f1(x + 26)} ${f1(y - 26)} l 6 9 l -6 5 l 4 8`, "r") + label(x, y + 4, "may crack", "r", 19, "middle");
  }

  function grainFig1() {
    let g = grainSheet(150, 70, 400, 250);
    g += path("M 70 70 a 46 46 0 1 0 0.1 0", "w") + path("M 70 102 a 14 14 0 1 0 0.1 0", "w");
    g += path("M 70 70 L 150 70", "w");
    g += arrow(240, 352, 470, 352, "a") + label(355, 384, "rolling direction = grain", "a", 20, "middle");
    g += label(70, 196, "coil", "w", 19, "middle");
    return svg(g, "A coil unrolling into a sheet. Faint streaks run along the sheet in the rolling direction, which is the grain.", "20 40 560 360");
  }

  function grainFig2() {
    const cx = 330, cy = 190;
    let g = grainSheet(110, 60, 440, 260);
    const a = rad(s.phi);
    const L = 150;
    const dx = Math.cos(a) * L, dy = -Math.sin(a) * L;
    const need = rule.minR(s.phi), good = ok(s.R, need);
    g += path(`M ${f1(cx - dx)} ${f1(cy - dy)} L ${f1(cx + dx)} ${f1(cy + dy)}`, good ? "g" : "r", "thick");
    g += path(`M ${cx + 60} ${cy} A 60 60 0 0 0 ${f1(cx + 60 * Math.cos(a))} ${f1(cy - 60 * Math.sin(a))}`, "a");
    g += label(cx + 70 * Math.cos(a / 2) + 6, cy - 70 * Math.sin(a / 2), `${Math.round(s.phi)}°`, "a", 20);
    g += label(120, 50, "bend line", good ? "g" : "r", 19);
    g += crackFlag(330, 370, good);
    return svg(g, "A grained sheet with a bend line drawn at an angle to the grain. The bend is marked clean or likely to crack.", "60 20 520 380");
  }

  function grainFig3() {
    const cx = 330, cy = 200, psi = rad(s.rot);
    let g = grainSheet(90, 50, 480, 300);
    const rot = (x, y) => [cx + x * Math.cos(psi) - y * Math.sin(psi), cy + x * Math.sin(psi) + y * Math.cos(psi)];
    const outline = [[-110, -90], [70, -90], [70, -40], [110, -40], [110, 90], [-110, 90]].map(([x, y]) => rot(x, y));
    g += fill(poly(outline), "b") + path(poly(outline));
    const b1 = [rot(70, -40), rot(70, 90)], b2 = [rot(-110, -40), rot(70, -40)];
    const ang = (p, q) => {
      const t = Math.abs((Math.atan2(q[1] - p[1], q[0] - p[0]) * 180) / Math.PI) % 180;
      return t > 90 ? 180 - t : t;
    };
    const a1 = ang(...b1), a2 = ang(...b2);
    const g1 = ok(s.R, rule.minR(a1)), g2 = ok(s.R, rule.minR(a2));
    g += path(poly(b1, false), col(g1), "thick") + path(poly(b2, false), col(g2), "thick");
    g += label(b1[1][0] + 10, b1[1][1] + 20, `${Math.round(a1)}°`, col(g1), 19);
    g += label(b2[0][0] - 10, b2[0][1] - 10, `${Math.round(a2)}°`, col(g2), 19, "end");
    return svg(g, "A flat part with two bend lines at right angles, turned on a grained sheet. Each bend line shows its angle to the grain and whether it bends clean.", "60 20 540 360");
  }

  function grainFig4() {
    let g = path(rectD(100, 90, 400, 180), "w") + path("M 330 76 V 284", "a");
    g += label(338, 72, "BEND", "a", 18);
    g += arrow(150, 320, 330, 320, "b") + label(150, 350, "GRAIN", "b", 21);
    g += label(110, 130, "NOTE: BEND ACROSS GRAIN", "b", 19);
    return svg(g, "A flat pattern with a bend line, a grain arrow under it and a note saying to bend across the grain.", "60 40 520 340");
  }

  const GRAIN = {
    id: "grain",
    title: "Grain direction and bend radius",
    q: "Which way should this part sit on the sheet?",
    blurb: "Bend across the grain, and say so on the drawing",
    src: "Rolled sheet bends tighter across the rolling direction than along it; minimum radii as conservative multiples of T to be replaced from the supplier's datasheet.",
    steps: [
      {
        t: "Sheet has a grain",
        p: "Sheet and plate are rolled from thick slabs, which stretches the metal's crystals into long streaks in one direction. That rolling direction is the grain, and it changes how the sheet bends.",
        ctl: ["T"],
        fig: grainFig1,
        extra: matButtons,
        math: () => ln(`${MAT().name}: grain usually runs along the long side of the sheet.`) + ln("Look for faint lines or a mill print on the surface.", "dim"),
      },
      {
        t: "Bend across the grain",
        p: "A bend line at 90° to the grain can take a tight radius. Turn it toward the grain and the outside of the bend is more likely to crack. The tighter the radius, the more it matters.",
        ctl: ["T", "R", "phi"],
        fig: grainFig2,
        extra: matButtons,
        math: () => {
          const need = rule.minR(s.phi), m = MAT();
          return ln(`smallest R across ${V_(`${trim(m.across)} × T`)}, along ${V_(`${trim(m.along)} × T`)}`, "dim") + ln(`at ${Math.round(s.phi)}°: R ≥ ${V_(len(need))}`) + verdict(ok(s.R, need), "Bends clean", "May crack", `R ${num(s.R)}`);
        },
        basis: () => `${MAT().name}: conservative starting multiples; between across and along the limit is blended by angle. The supplier's datasheet gives the real minimum for each temper and thickness.`,
      },
      {
        t: "Two bends, one sheet",
        p: "A part with bends at right angles can't have both across the grain. Turn the part about 45° on the sheet and both bends share the risk. That costs material, so do it only when the radius is tight.",
        ctl: ["T", "R", "rot"],
        fig: grainFig3,
        extra: matButtons,
        math: () => {
          const a = 90 - (s.rot % 90), b = s.rot % 90;
          const worst = Math.max(rule.minR(a), rule.minR(b));
          return ln(`worst bend needs R ≥ ${V_(len(worst))}`) + verdict(ok(s.R, worst), "Both bends clean", "One bend may crack", `R ${num(s.R)}`) + ln(`at 45° both need ${num(rule.minR(45))}`, "dim");
        },
      },
      {
        t: "Say it on the drawing",
        p: "If the radius depends on grain, the shop has to know. Add a grain arrow to the flat pattern and a short note. Without it, the nesting program will turn the part whichever way saves the most sheet.",
        ctl: ["T", "R"],
        fig: grainFig4,
        math: () => ln(`note: ${V_("BEND ACROSS GRAIN")}`) + ln("Put it on the flat pattern, beside the grain arrow.", "dim"),
        place: { label: "Put a grain arrow and note on my drawing", run: () => placeGrainNote() },
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: holes, slots and reliefs
  function hfig1() {
    const BL = 400, pp = 7, need = rule.holeBend();
    const r = clamp((s.d / 2) * pp, 2, 60);
    const hx = BL - s.h * pp - r;
    const good = ok(s.h, need);
    const stretch = good ? 0 : Math.min(1, (need - s.h) / need) * 0.9;
    let g = label(60, 46, "flat pattern", "b", 21);
    g += plate(60, 56, BL, 176, BL);
    g += hatch(BL - need * pp, 56, need * pp, 120);
    g += circle(hx, 116, r, col(good));
    g += dim(hx + r, 150, BL, 150, col(good));
    g += label(hx + r + (BL - hx - r) / 2, 168, `h ${num(s.h)}`, col(good), 19, "middle");
    g += label(BL + 44, 126, "bend", "a", 19);
    g += label(60, 246, "after forming", "b", 21);
    g += plate(60, 256, BL, 376, BL);
    g += `<ellipse class="ch c-${col(good)}" pathLength="1" ${D()} cx="${f1(hx + stretch * r * 0.6)}" cy="316" rx="${f1(r * (1 + stretch))}" ry="${f1(r * (1 - 0.25 * stretch))}"/>`;
    g += label(BL + 44, 326, good ? "stays round" : "pulled oval", good ? "g" : "r", 19);
    return svg(g, "Two top views of a plate with a hole near a bend line: the flat pattern, and the hole after forming, which stays round if it is far enough away and stretches into an oval if it is not.", "30 20 540 380");
  }

  function hfig2() {
    const BL = 440, pp = 5, y0 = 70, y1 = 300, sw = 42, sr = sw / 2;
    const slotNeed = rule.slotBend(), holeNeed = rule.holeBend();
    const good = ok(s.sl, slotNeed);
    const x1 = BL - s.sl * pp, x0 = x1 - sw;
    let g = plate(60, y0, BL, y1, BL);
    g += hatch(BL - slotNeed * pp, y0, slotNeed * pp, y1 - y0);
    g += dash(`M ${f1(BL - holeNeed * pp)} ${y0} V ${y1}`, "b");
    g += path(`M ${f1(x0)} ${120 + sr} A ${sr} ${sr} 0 0 1 ${f1(x1)} ${120 + sr} V ${250 - sr} A ${sr} ${sr} 0 0 1 ${f1(x0)} ${250 - sr} Z`, col(good));
    g += dim(x1, 185, BL, 185, col(good));
    g += label((x1 + BL) / 2, 168, `s ${num(s.sl)}`, col(good), 19, "middle");
    g += label(BL - slotNeed * pp, y1 + 28, `slot  ${trim(shop.slotBend)}T + R = ${num(slotNeed)}`, "r", 19, "middle");
    g += label(BL - holeNeed * pp, y1 + 54, `hole  ${trim(shop.holeBend)}T + R = ${num(holeNeed)}`, "b", 19, "start");
    g += label(BL + 44, 186, "bend", "a", 19);
    return svg(g, "Top view of a plate with a long slot beside a bend line. The red band is how far a slot must stay from the bend, and the blue dashed line is the smaller distance that is enough for a hole.", "40 40 540 360");
  }

  function hfig3() {
    const pp = Math.min(9, 400 / (3 * s.d + 2 * s.g + 1)), x0 = 50, x1 = 590, y0 = 60, y1 = 280, cy = 170;
    const r = (s.d / 2) * pp;
    const goodD = ok(s.d, rule.minDia()), goodW = ok(s.g, rule.minWeb());
    let g = path(`M ${x0} ${y0} H ${x1} V ${y1} H ${x0} Z`, "w");
    const a = x0 + 40 + r, b = a + 2 * r + s.g * pp, e = x1 - s.g * pp - r;
    for (const x of [a, b, e]) g += circle(x, cy, r, col(goodD));
    g += dim(a + r, cy + r + 24, b - r, cy + r + 24, col(goodW));
    g += label((a + b) / 2, cy + r + 50, `web ${num(s.g)}`, col(goodW), 19, "middle");
    g += dim(e + r, cy + r + 24, x1, cy + r + 24, col(goodW));
    g += label(x1, cy + r + 50, `edge ${num(s.g)}`, col(goodW), 19, "end");
    g += dim(a - r, cy - r - 20, a + r, cy - r - 20, col(goodD));
    g += label(a, cy - r - 30, `dia ${num(s.d)}`, col(goodD), 19, "middle");
    g += label(x0, y1 + 36, `${procName()}: smallest hole ${num(rule.minDia())}, thinnest web ${num(rule.minWeb())}`, "a", 19);
    return svg(g, "Top view of a plate with three holes. The gap between two holes and the gap between a hole and the edge are marked as webs, and the hole diameter is marked.", "30 30 580 300");
  }

  function hfig4() {
    const BL = 420, pp = 10, T = 50, B = 400, y1 = 190, y2 = 250, tab = 130;
    const w = Math.max(s.w, 0) * pp, dp = Math.max(s.dp, 0) * pp;
    const goodW = ok(s.w, rule.relW()), goodD = ok(s.dp, rule.relD()), good = goodW && goodD;
    const o = `M 100 ${T} H ${BL} V ${f1(y1 - w)} H ${f1(BL - dp)} V ${y1} H ${BL + tab} V ${y2} H ${f1(BL - dp)} V ${f1(y2 + w)} H ${BL} V ${B} H 100 Z`;
    let g = fill(o, "b") + path(o, "w");
    g += fill(`M ${BL} ${y1} h ${tab} V ${y2} h ${-tab} Z`, "a");
    g += path(`M ${BL} ${y1} V ${y2}`, "a", "thick");
    g += dim(BL - dp, y1 - w - 22, BL, y1 - w - 22, col(goodD));
    g += label(BL - dp / 2, y1 - w - 32, `depth ${num(s.dp)}`, col(goodD), 19, "middle");
    g += dim(BL - dp - 24, y1 - w, BL - dp - 24, y1, col(goodW));
    g += label(BL - dp - 34, y1 - w / 2 + 6, `w ${num(s.w)}`, col(goodW), 19, "end");
    g += label(BL + tab / 2, (y1 + y2) / 2 + 6, "flange", "a", 21, "middle");
    g += label(BL + tab / 2, y2 + 44, good ? "room to move" : "may tear here", good ? "g" : "r", 20, "middle");
    return svg(g, "Top view of a plate with a narrower flange tab. A relief notch is cut into the plate at each end of the bend, and its width and depth are marked.", "40 20 560 400");
  }

  function hfig5() {
    const BL = 340;
    let g = plate(60, 70, BL, 300, BL);
    g += circle(92, 102, 5, "r") + circle(110, 200, 20, "r") + circle(156, 200, 20, "r") + circle(BL - 40, 150, 20, "r") + circle(290, 250, 20, "g");
    g += label(106, 108, "too small", "r", 19);
    g += label(133, 244, "too close together", "r", 19, "middle");
    g += label(BL - 66, 126, "too near the bend", "r", 18, "end");
    g += label(290, 322, "fine", "g", 19, "middle");
    g += label(BL + 44, 190, "BEND layer", "a", 19);
    return svg(g, "A plate with five circles. Three are marked as problems: one too small, two too close together and one too near the bend line on the BEND layer. One is marked fine.", "30 40 520 300");
  }

  const HOLES = {
    id: "holes",
    title: "Holes, slots and reliefs",
    q: "How close to a bend can a feature sit?",
    blurb: "Keep features out of the bend zone and give bends room to move",
    src: "Hole and slot distances to a bend as multiples of T plus R, relief width and depth: common sheet-metal design rules of thumb, kept as Shop settings.",
    steps: [
      {
        t: "A hole near a bend stretches out of round",
        p: "Metal near a bend flows as it folds. A hole inside that zone pulls into an oval, and the pin or bolt it was drawn for stops fitting. Keep the hole edge a safe distance from where the bend starts.",
        ctl: ["T", "R", "h"],
        fig: hfig1,
        math: () => {
          const good = ok(s.h, rule.holeBend());
          return ln(`limit = ${trim(shop.holeBend)}·T + R = ${V_(len(rule.holeBend()))}`, "dim") + verdict(good, "Clear", "Too close", `${num(s.h)} of ${num(rule.holeBend())} needed`);
        },
        basis: () => `${trim(shop.holeBend)} × T + R is a common starting rule. Thick plate and tight radii may need more.`,
        note: "The lower view exaggerates the stretch so you can see it.",
        place: { label: "Draw this plate on my drawing", run: () => placeHolePlate() },
      },
      {
        t: "Slots need more room than holes",
        p: "A slot is long, so more of it sits in the bend zone and it distorts more. Hold slot edges further from the bend than holes. A slot that crosses the bend line will distort: move it, or cut it after forming.",
        ctl: ["T", "R", "sl"],
        fig: hfig2,
        math: () => {
          const good = ok(s.sl, rule.slotBend());
          return ln(`hole limit = ${trim(shop.holeBend)}·T + R = ${V_(len(rule.holeBend()))}`, "dim") + ln(`slot limit = ${trim(shop.slotBend)}·T + R = ${V_(len(rule.slotBend()))}`, "dim") + verdict(good, "Slot is clear", "Slot is too close", `${num(s.sl)} of ${num(rule.slotBend())} needed`);
        },
        basis: () => `${trim(shop.slotBend)} × T + R for slots parallel to the bend; a starting rule you can change in Shop settings.`,
      },
      {
        t: "Smallest hole, thinnest web",
        p: "Cuts need a minimum size and a minimum wall between them, and the limit depends on the process. A web is the strip left between two cuts, or between a cut and the edge. Thin webs warp and burn through.",
        ctl: ["T", "d", "g"],
        fig: hfig3,
        math: () => {
          const gd = ok(s.d, rule.minDia()), gw = ok(s.g, rule.minWeb());
          return ln(`${procName()}: hole ≥ ${trim(PR().hole)} × T = ${V_(len(rule.minDia()))}`, "dim") + ln(`web ≥ ${trim(PR().web)} × T = ${V_(len(rule.minWeb()))}`, "dim") + verdict(gd, "Hole is big enough", "Hole is too small") + verdict(gw, "Web holds", "Web is too thin");
        },
        extra: procButtons,
        basis: "Starting multiples of thickness. Your machine's cut chart for each material and thickness has the final say.",
      },
      {
        t: "Bend reliefs let the corner move",
        p: "When a bend stops short of the edge, the metal at its end has nowhere to go and tears. A relief cut at each end of the bend gives it room.",
        ctl: ["T", "R", "w", "dp"],
        fig: hfig4,
        math: () => {
          const gw = ok(s.w, rule.relW()), gd = ok(s.dp, rule.relD());
          return ln(`width ≥ ${trim(shop.relW)} × T = ${V_(len(rule.relW()))} · depth ≥ R + ${trim(shop.relD)} × T = ${V_(len(rule.relD()))}`, "dim") + verdict(gw, "Width holds", "Too narrow", num(s.w)) + verdict(gd, "Depth holds", "Too shallow", num(s.dp));
        },
        basis: "Relief at least T wide and R + T deep past the bend line: a common starting rule.",
        place: { label: "Draw a relieved flange on my drawing", run: () => placeRelief() },
      },
      {
        t: "Check my drawing",
        p: "Put your bend lines on a layer named BEND. The check finds holes that are too small, too close to each other, too close to an edge or too close to a bend, using the thickness, radius and process set here.",
        ctl: ["T", "R"],
        fig: hfig5,
        math: () => checkMath(),
        extra: () => procButtons() + checkButtons(),
        note: "Draw this flat, in the bend allowance lesson, already puts its bend lines on BEND.",
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: where bends meet
  function cornerFig() {
    const c = calc();
    const pp = 7, x0 = 70, y1 = 390, xB = 360, yB = 170, F = 110;
    const cp = clamp(s.cr * pp, 0, 160), ba = clamp(c.BA * pp, 4, 60);
    const xN = xB - cp, yN = yB + cp;
    const good = ok(s.cr, rule.relD());
    const o = poly([[x0, y1], [x0, yB - F], [xN, yB - F], [xN, yN], [xB + F, yN], [xB + F, y1]]);
    let g = fill(o, "b") + path(o, "w");
    g += fill(rectD(x0, yB - ba, xN - x0, ba), "a") + fill(rectD(xB, yN, ba, y1 - yN), "a");
    g += dash(`M ${x0} ${yB} H ${xB} M ${xB} ${y1} V ${yB}`, "a");
    g += label(x0 + 8, yB - F - 12, "flange", "a", 19) + label(xB + F + 6, y1 - 8, "flange", "a", 19);
    g += label(x0 + 14, y1 - 20, "base", "b", 20);
    const need = rule.relD() * pp;
    g += hatch(xB - need, yB, need, need);
    g += dim(xN, yN + 24, xB, yN + 24, col(good)) + label((xN + xB) / 2, yN + 50, `c ${num(s.cr)}`, col(good), 19, "middle");
    g += mark(xN - 14, yN + 4, good);
    return svg(g, "The flat pattern of a box corner: a base with two flanges and a square notch where they meet. The notch goes past both bend lines by the corner relief distance c.", "40 40 540 380");
  }

  function jogFig() {
    const need = rule.minFlange();
    const good = ok(s.jog, need);
    const sc = clamp(160 / Math.max(s.jog, 1), 0.5, 10);
    const j = s.jog * sc;
    const yLow = 300, yHigh = yLow - j;
    const d = `M 80 ${yLow} H 290 V ${f1(yHigh)} H 520`;
    let g = path(d, col(good), "thick");
    g += dim(320, yLow, 320, yHigh, col(good)) + label(332, (yLow + yHigh) / 2 + 6, `j ${num(s.jog)}`, col(good), 20);
    g += label(80, yLow + 40, good ? "two bends, standard die" : "too short to sit on the die", col(good), 19);
    return svg(g, "A Z-shaped jog: two opposite bends with a step height j between the lower and upper legs.", "60 40 500 320");
  }

  function chanFig() {
    const sc = Math.min(150 / Math.max(s.A, s.C, 1), 300 / s.B);
    const t = clamp(s.T * sc, 3, 14), r = clamp(s.R * sc, 0, 30);
    const x0 = 300 - (s.B * sc) / 2, yb = 300;
    const d = channel(x0, yb, s.A * sc, s.B * sc, s.C * sc, t, r);
    const tall = Math.max(s.A, s.C) > s.B;
    let g = fill(d, "b") + path(d, tall ? "r" : "w");
    g += dim(x0, yb + 22, x0 + s.B * sc, yb + 22, "w") + label(300, yb + 48, `B ${num(s.B)}`, "w", 20, "middle");
    g += dim(x0 - 22, yb, x0 - 22, yb - s.A * sc, tall ? "r" : "w") + label(x0 - 30, yb - (s.A * sc) / 2, `A ${num(s.A)}`, tall ? "r" : "w", 20, "end");
    if (tall) g += label(300, 60, "first flange may hit the punch", "r", 20, "middle");
    return svg(g, "A U-channel with its base width and flange height marked.", "40 30 560 360");
  }

  const CORNERS = {
    id: "corners",
    title: "Where bends meet",
    q: "What happens where two bends come together?",
    blurb: "Corner reliefs, jogs and channels",
    src: "Corner relief follows the bend-relief rule (past each bend line by R + T); jog and channel limits follow from the shortest flange a die can hold.",
    steps: [
      {
        t: "Corner reliefs",
        p: "Where two flanges meet at a corner, both bends try to use the same metal. Cut the corner away past both bend lines so each bend has room. The same rule as a single bend relief applies, in both directions.",
        ctl: ["T", "R", "cr"],
        fig: cornerFig,
        math: () => ln(`c ≥ R + ${trim(shop.relD)} × T = ${V_(len(rule.relD()))}`) + verdict(ok(s.cr, rule.relD()), "Corner has room", "Corner may tear or bulge", `c ${num(s.cr)}`),
        basis: "Same starting rule as the bend relief, set in Shop settings. A round corner relief works too if it reaches as far.",
      },
      {
        t: "A jog: two bends close together",
        p: "A jog (or Z-bend) steps a flange sideways with two opposite bends. For the second bend, the short middle leg has to sit across the die like any other flange. Shorter jogs need special offset tooling.",
        ctl: ["T", "jog"],
        fig: jogFig,
        math: () => ln(`j ≥ shortest flange = ${V_(len(rule.minFlange()))}`) + verdict(ok(s.jog, rule.minFlange()), "Standard die can do it", "Needs offset tooling", `j ${num(s.jog)}`),
        basis: () => `Shortest flange for a ${trim(shop.vRatio)}·T die. Offset (jog) tooling can make much shorter steps in one hit; ask the shop.`,
      },
      {
        t: "Channels: flanges and base",
        p: "In a U-channel the first flange stands up while the second bend is made. If the flanges are taller than the channel is wide, the first one can hit the punch. Gooseneck punches help, but check with the shop.",
        ctl: ["A", "B", "C"],
        fig: chanFig,
        math: () => {
          const tall = Math.max(s.A, s.C) > s.B;
          return ln(`flange ${num(Math.max(s.A, s.C))} vs base ${num(s.B)}`) + (tall ? warnLn("Flange taller than the base is wide: ask about punch clearance.") : ln("Flanges clear a straight punch.", "ok")) + verdict(ok(s.B, rule.minFlange()), "Base sits on the die", "Base is short for the die");
        },
        basis: "Flange no taller than the channel is wide is a rough rule of thumb; the real limit depends on the punch shape and the order of bends.",
      },
    ],
  };

  // ================================================================ CUTTING

  // ---------------------------------------------------------------- lesson: kerf
  /** How much to blow kerf up so it can be seen; the figure says it is exaggerated. */
  const kx = () => clamp(PR().kerf * 22, 6, 40);

  function kerfFig1() {
    const kp = kx();
    let g = path(rectD(60, 70, 480, 150), "w");
    const x = 300;
    g += fill(rectD(x - kp / 2, 70, kp, 150), "r") + path(`M ${f1(x - kp / 2)} 70 V 220 M ${f1(x + kp / 2)} 70 V 220`, "r");
    g += dash(`M ${x} 50 V 240`, "a");
    g += dim(x - kp / 2, 46, x + kp / 2, 46, "w") + label(x + kp / 2 + 10, 52, `kerf ${num(PR().kerf)}`, "w", 20);
    g += label(x + 10, 268, "beam or arc follows this line", "a", 18);
    const Tp = clamp(s.T * 6, 8, 60), y = 310;
    g += path(rectD(60, y, x - kp / 2 - 60, Tp), "w") + path(rectD(x + kp / 2, y, 540 - x - kp / 2, Tp), "w");
    g += label(70, y + Tp + 28, "section through the cut", "b", 18);
    return svg(g, "Top view of a plate with one straight cut. The cut is a slot as wide as the kerf, centred on the path. Below, a section through the cut. Kerf is drawn larger than life.", "40 20 540 400");
  }

  /** The drawn part and hole, with the edges the cut leaves when the path is
   *  on the line (comp = false) or moved to the scrap side (comp = true). */
  function kerfPart(comp) {
    const kp = kx();
    const x0 = 130, y0 = 70, W = 340, H = 260, cx = 300, cy = 200, r = clamp(s.d * 6, 26, 90);
    let g = dash(rectD(x0, y0, W, H), "w") + `<circle class="dsh c-w" ${D()} cx="${cx}" cy="${cy}" r="${f1(r)}"/>`;
    if (!comp) {
      g += path(rectD(x0 + kp / 2, y0 + kp / 2, W - kp, H - kp), "r") + circle(cx, cy, r + kp / 2, "r");
      g += dash(rectD(x0, y0, W, H), "a");
      g += label(cx, cy + r + kp / 2 + 30, `hole ${num(s.d + PR().kerf)}`, "r", 20, "middle");
      g += label(x0 + W / 2, y0 + H + 34, `part ${num(s.W - PR().kerf)} wide`, "r", 20, "middle");
    } else {
      g += dash(rectD(x0 - kp / 2, y0 - kp / 2, W + kp, H + kp), "a") + `<circle class="dsh c-a" ${D()} cx="${cx}" cy="${cy}" r="${f1(Math.max(r - kp / 2, 2))}"/>`;
      g += path(rectD(x0, y0, W, H), "g") + circle(cx, cy, r, "g");
      g += label(cx, cy + r + 30, `hole ${num(s.d)}`, "g", 20, "middle");
      g += label(x0 + W / 2, y0 + H + 34, `part ${num(s.W)} wide`, "g", 20, "middle");
    }
    g += label(x0, 44, comp ? "path moved to the scrap side" : "path on the drawn line", "a", 19);
    return svg(g, comp ? "A square part with a hole. The cutting path runs half a kerf outside the part and half a kerf inside the hole, so the finished edges land on the drawn lines." : "A square part with a hole. The cutting path follows the drawn lines, so the hole comes out larger and the part comes out smaller, each by one kerf.", "60 20 520 400");
  }

  function kerfFig4() {
    const kp = kx();
    const need = Math.max(PR().kerf, rule.minDia());
    const good = ok(s.sw, need);
    let g = path("M 60 80 H 280 V 200 H 400 V 340 H 60 Z", "w");
    const r = kp / 2;
    g += path(`M 280 ${200 - 40} V ${f1(200 - r)} A ${f1(r)} ${f1(r)} 0 0 0 ${f1(280 + r)} 200 H 320`, "a", "thick");
    g += label(290, 150, `inside corner: R ≥ ${num(PR().kerf / 2)}`, "a", 18);
    const sp = clamp(s.sw * 10, 3, 60);
    g += path(rectD(470, 90, sp, 220), col(good));
    g += label(470 + sp / 2, 340, `slot ${num(s.sw)}`, col(good), 19, "middle");
    g += mark(470 + sp / 2, 70, good);
    return svg(g, "A part with an inside corner, which can be no sharper than half the kerf, and a narrow slot beside it.", "40 40 540 330");
  }

  const KERF = {
    id: "kerf",
    title: "Kerf and your dimensions",
    q: "Is the hole I drew the hole I'll get?",
    blurb: "Why the cut has a width, and who corrects for it",
    src: "Geometry of a cut of finite width centred on its path; cutter compensation offsets the path half a kerf toward the scrap. Kerf defaults are starting points to be measured.",
    steps: [
      {
        t: "The cut has a width",
        p: "A laser beam or a plasma arc doesn't cut a line. It melts out a slot, and the slot's width is the kerf. Laser kerf is a few tenths of a millimetre. Plasma kerf is several times wider.",
        ctl: ["T", "k"],
        fig: kerfFig1,
        extra: procButtons,
        math: () => ln(`${procName()} kerf = ${V_(len(PR().kerf))}`) + ln("Kerf grows with thickness and power.", "dim"),
        basis: "Kerf is stored per process in Shop settings. Measure it: cut a slot, measure the slot, use that.",
      },
      {
        t: "Cut on the line: holes grow, parts shrink",
        p: "If the path follows your drawn line, half the kerf eats into the part on each side. Every hole comes out one kerf larger, and every outside size comes out one kerf smaller.",
        ctl: ["k", "d", "W"],
        fig: () => kerfPart(false),
        extra: procButtons,
        ex: () => {
          const o = exVals({ d: 10 }, { d: 0.5 });
          const k = PR().kerf;
          return exBox([`${procName()} kerf ${num(k)}, hole drawn ${nm(o.d)}`, `hole cut = d + k = ${nm(o.d)} + ${num(k)} = <span class="v">${len(o.d + k)}</span>`]);
        },
        math: () => ln(`hole = d + k = ${num(s.d)} + ${num(PR().kerf)} = <span class="bad">${len(s.d + PR().kerf)}</span>`) + ln(`part = W − k = ${num(s.W)} − ${num(PR().kerf)} = <span class="bad">${len(s.W - PR().kerf)}</span>`),
        note: "Kerf is drawn larger than life so you can see it.",
      },
      {
        t: "The program moves the path, not you",
        p: "Cutting programs and machine controls offset the path half a kerf toward the scrap: outside the part, inside the holes. Then edges land on your lines. So draw everything true size, and never pre-shrink or pre-grow it yourself.",
        ctl: ["k", "d", "W"],
        fig: () => kerfPart(true),
        extra: procButtons,
        math: () => ln(`offset = k ÷ 2 = ${V_(len(PR().kerf / 2))}`) + ln(`hole = ${V_(len(s.d))}, part = ${V_(len(s.W))}`, "ok") + ln("If a drawing was ever pre-offset, say so in a note.", "dim"),
      },
      {
        t: "What the offset can't fix",
        p: "An inside corner can be no sharper than half the kerf. A slot narrower than the kerf can't be cut at all, and a closed slot also needs room to start the cut inside it.",
        ctl: ["k", "sw", "T"],
        fig: kerfFig4,
        extra: procButtons,
        math: () => {
          const need = Math.max(PR().kerf, rule.minDia());
          return ln(`inside corners: R ≥ k ÷ 2 = ${V_(len(PR().kerf / 2))}`) + ln(`slot ≥ larger of kerf and smallest hole = ${V_(len(need))}`) + verdict(ok(s.sw, need), "Slot can be cut", "Slot is too narrow", num(s.sw));
        },
        basis: () => `Smallest hole for ${procName()} is ${trim(PR().hole)} × T (Shop settings).`,
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: small features
  function smallFig1() {
    const mults = [0.5, 1, 1.5, 2];
    const need = rule.minDia();
    const sc = clamp(60 / Math.max(2 * s.T, s.d, 1), 2, 30);
    let g = path(rectD(40, 60, 540, 230), "w"), x = 90;
    for (const m of mults) {
      const d = m * s.T, r = (d * sc) / 2, good = ok(d, need);
      g += circle(x, 150, r, col(good));
      label(x, 230, `${m} T`, col(good), 19, "middle");
      x += Math.max(2 * r, 30) + 50;
    }
    const r = (s.d * sc) / 2, good = ok(s.d, need);
    g += circle(Math.min(x + r, 520), 150, r, col(good), "thick");
    label(Math.min(x + r, 520), 230, `d ${num(s.d)}`, col(good), 19, "middle");
    g += path(rectD(60, 316, s.T * sc, 10), "b") + label(70 + s.T * sc, 328, `T ${num(s.T)} to scale`, "b", 18);
    return svg(g, "A plate with holes of half, one, one and a half and two times the thickness, and your hole, each marked as cuttable or too small for the chosen process.", "30 40 560 310");
  }

  function smallFig3() {
    const sc = clamp(150 / Math.max(s.e + s.d, 1), 2, 20);
    const xE = 500, cx = xE - s.e * sc, cy = 190, r = (s.d * sc) / 2;
    const web = s.e - s.d / 2;
    const gW = ok(web, rule.minWeb()), gE = ok(s.e, shop.edge * s.d);
    let g = path(`M 60 60 H ${xE} V 320 H 60`, "w");
    g += hatch(xE - shop.edge * s.d * sc, 60, shop.edge * s.d * sc, 260);
    g += circle(cx, cy, r, col(gW && gE));
    g += dash(`M ${f1(cx)} ${cy - r - 20} V ${cy + r + 20}`, "b");
    g += dim(cx, cy + r + 40, xE, cy + r + 40, col(gE)) + label((cx + xE) / 2, cy + r + 66, `e ${num(s.e)}`, col(gE), 19, "middle");
    g += dim(cx + r, cy - r - 30, xE, cy - r - 30, col(gW)) + label((cx + r + xE) / 2, cy - r - 40, `web ${num(web)}`, col(gW), 19, "middle");
    return svg(g, "A hole near a plate edge. The edge distance e is measured to the hole centre; the web is what is left between the hole and the edge.", "40 30 520 330");
  }

  function smallFig4() {
    const good = ok(s.br, rule.minWeb());
    const bp = clamp(s.br * 4, 3, 30);
    let g = path(rectD(40, 60, 540, 260), "w");
    const ring = (cx) => `<ellipse class="ch c-w" pathLength="1" ${D()} cx="${cx}" cy="180" rx="80" ry="100"/>`;
    g += ring(170) + `<ellipse class="ch c-r" pathLength="1" ${D()} cx="178" cy="214" rx="40" ry="58"/>`;
    g += path("M 160 300 l 10 10 l 10 -10", "r");
    label(170, 350, "island falls out", "r", 19, "middle");
    g += ring(430) + `<ellipse class="ch c-${col(good)}" pathLength="1" ${D()} cx="430" cy="180" rx="40" ry="58"/>`;
    g += fill(rectD(430 - bp / 2, 80, bp, 42), "b") + path(rectD(430 - bp / 2, 80, bp, 42), col(good));
    g += fill(rectD(430 - bp / 2, 238, bp, 42), "b") + path(rectD(430 - bp / 2, 238, bp, 42), col(good));
    label(430, 350, good ? "bridges hold" : "bridges too thin", col(good), 19, "middle");
    return svg(g, "Two cut-out letter O shapes. On the left the centre island falls out. On the right two bridges hold the island in place.", "30 40 560 330");
  }

  const SMALL = {
    id: "small",
    title: "Smallest hole and narrowest web",
    q: "How small can I cut?",
    blurb: "Holes, webs, edge distance and lettering",
    src: "Smallest hole and web as process multiples of T (cut-chart starting points); bolt edge distance about 1.5 × hole diameter, with exact values in structural codes such as ANSI/AISC 360-22 Table J3.4.",
    steps: [
      {
        t: "The smallest hole",
        p: "Every cut starts with a pierce, and the pierce needs room. Below a certain size, holes come out tapered, out of round or not at all. The limit is a multiple of the thickness, and plasma's is larger than laser's.",
        ctl: ["T", "d"],
        fig: smallFig1,
        extra: procButtons,
        math: () => ln(`${procName()}: hole ≥ ${trim(PR().hole)} × T = ${V_(len(rule.minDia()))}`) + verdict(ok(s.d, rule.minDia()), "Cuttable", "Too small to cut well", `d ${num(s.d)}`) + ln("Smaller holes: drill or punch them after cutting.", "dim"),
        basis: "Starting multiples per process in Shop settings. Thin sheet on a good laser can often go smaller; your cut chart has the final say.",
      },
      {
        t: "The web between cuts",
        p: "The web is the strip left between two cuts, or between a cut and the part's edge. Heat from both sides meets in it. Too thin and it warps, burns through or breaks off.",
        ctl: ["T", "d", "g"],
        fig: hfig3,
        extra: procButtons,
        math: () => ln(`${procName()}: web ≥ ${trim(PR().web)} × T = ${V_(len(rule.minWeb()))}`) + verdict(ok(s.g, rule.minWeb()), "Web holds", "Web is too thin", `${num(s.g)}`),
      },
      {
        t: "Bolt holes near an edge",
        p: "A hole near an edge has two limits. The cutting limit is the web. If a bolt pulls on the hole, there's a strength limit too: keep the hole centre about one and a half hole diameters from the edge.",
        ctl: ["T", "d", "e"],
        fig: smallFig3,
        extra: procButtons,
        math: () => {
          const web = s.e - s.d / 2;
          return ln(`web = e − d÷2 = ${V_(len(web))}, needs ${num(rule.minWeb())}`) + verdict(ok(web, rule.minWeb()), "Cuts clean", "Web too thin to cut") + ln(`loaded hole: e ≥ ${trim(shop.edge)} × d = ${V_(len(shop.edge * s.d))}`) + verdict(ok(s.e, shop.edge * s.d), "Room for load", "Close for a loaded bolt");
        },
        basis: () => `${trim(shop.edge)} × d is a common starting point. Structural codes list exact edge distances by bolt size and edge type (for example ANSI/AISC 360-22, Table J3.4).`,
      },
      {
        t: "Letters and islands",
        p: "Cut a letter O through a plate and its centre falls out. Letters with closed shapes need bridges that tie the island to the plate. Each bridge is a web, so it follows the web rule.",
        ctl: ["T", "br"],
        fig: smallFig4,
        extra: procButtons,
        math: () => ln(`bridge ≥ web = ${V_(len(rule.minWeb()))}`) + verdict(ok(s.br, rule.minWeb()), "Bridges hold", "Bridges too thin", num(s.br)) + ln(`letter strokes ≥ smallest hole = ${V_(len(rule.minDia()))}`, "dim"),
        note: "Stencil fonts have bridges built in. Or etch the text instead of cutting through.",
      },
      {
        t: "Check my drawing",
        p: "The check measures every circle on your drawing: too small for the process, too close to another hole, or too close to an edge or a bend. Lines on a layer named BEND count as bends.",
        ctl: ["T"],
        fig: hfig3,
        math: () => checkMath(),
        extra: () => procButtons() + checkButtons(),
        place: { label: "Draw a test plate on my drawing", run: () => placeTestPlate() },
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: lead-ins and pierce points
  function leadFig1() {
    const L = rule.leadIn(), pd = rule.pierce();
    const sc = clamp(150 / Math.max(L + pd, 1), 4, 60);
    const yE = 230, x1 = 330;
    let g = path(`M 60 ${yE} H 540`, "w") + fill(rectD(60, yE, 480, 110), "b");
    label(70, yE + 40, "part", "b", 20);
    label(70, yE - 120, "scrap", "w", 20);
    const rL = L * sc;
    const px = x1 - rL, py = yE - rL;
    g += `<circle class="fl c-rf" style="--d:.5s" cx="${f1(px)}" cy="${f1(py)}" r="${f1((pd * sc) / 2)}"/>` + circle(px, py, (pd * sc) / 2, "r");
    g += path(`M ${f1(px)} ${f1(py)} A ${f1(rL)} ${f1(rL)} 0 0 0 ${x1} ${yE}`, "a", "thick");
    g += arrow(x1, yE, 520, yE, "a");
    label(px - 10, py - (pd * sc) / 2 - 12, `pierce ${num(pd)}`, "r", 19, "end");
    label(x1 + 10, yE - 18, `lead-in ${num(L)}`, "a", 19);
    return svg(g, "A cut starting with a pierce mark in the scrap, then a curved lead-in that joins the part's edge, then the cut running along the edge.", "40 60 540 300");
  }

  function leadFig2() {
    const o = "M 100 90 H 470 V 320 H 100 Z";
    let g = fill(o, "b") + path(o, "w");
    g += path("M 470 70 A 20 20 0 0 0 490 90 L 470 90", "a", "thick") + mark(505, 70, true);
    label(500, 110, "corner: hidden", "g", 18);
    g += path("M 285 60 A 30 30 0 0 1 285 90", "a", "thick") + mark(285, 40, false);
    label(300, 46, "show face: leaves a mark", "r", 18);
    label(120, 300, "NOTE: NO LEAD-IN ON TOP EDGE", "b", 18);
    return svg(g, "A rectangular part. A lead-in into a corner is marked good; a lead-in into the middle of the top edge, a show face, is marked as leaving a mark. A note on the drawing says no lead-in on the top edge.", "60 20 520 330");
  }

  function leadFig3() {
    const L = rule.leadIn(), pd = rule.pierce();
    const need = pd + 2 * L;
    const good = ok(s.d, need);
    const sc = clamp(240 / Math.max(s.d, need, 1), 1, 60);
    const cx = 300, cy = 190, r = (s.d * sc) / 2;
    let g = fill(rectD(60, 40, 480, 300), "b") + path(rectD(60, 40, 480, 300));
    g += `<circle class="fl c-wf" style="--d:.5s" cx="${cx}" cy="${cy}" r="${f1(r)}"/>` + circle(cx, cy, r, col(good));
    g += circle(cx, cy, (pd * sc) / 2, "r");
    const lr = Math.min(L * sc, r);
    g += path(`M ${cx} ${cy} A ${f1(lr / 2)} ${f1(lr / 2)} 0 0 1 ${f1(cx + lr)} ${cy}`, "a", "thick");
    label(cx, cy + r + 30, good ? "pierce and lead-in fit" : "pierce lands on the edge", col(good), 19, "middle");
    return svg(g, "A hole with the pierce in its centre and a short lead-in reaching out to the hole's edge.", "40 20 520 340");
  }

  const LEAD = {
    id: "lead",
    title: "Lead-ins and pierce points",
    q: "Where does the cut start?",
    blurb: "Pierce in the scrap, start where it won't show",
    src: "Pierce and lead-in placement as general thermal-cutting practice; pierce mark as a multiple of kerf and lead-in as a multiple of T with a minimum, both starting points to be measured.",
    steps: [
      {
        t: "Every cut starts with a pierce",
        p: "Before it can cut, the beam or arc has to burn through, and that leaves a crater wider than the kerf. So the cut starts in the scrap and runs in along a short lead-in. The part edge never sees the pierce.",
        ctl: ["T", "k"],
        fig: leadFig1,
        extra: procButtons,
        math: () => ln(`pierce ≈ ${trim(PR().pierce)} × kerf = ${V_(len(rule.pierce()))}`) + ln(`lead-in ≈ ${trim(PR().lead)} × T, at least ${num(PR().leadMin)} = ${V_(len(rule.leadIn()))}`),
        basis: "Both are starting points stored per process. Look at a few cut parts and set your own.",
      },
      {
        t: "Outside edges: say where it may start",
        p: "Where the lead-in joins the part it leaves a small nick. The program puts it somewhere sensible, usually a corner. If an edge shows, or seals, or slides, say so on the drawing so the nick goes somewhere else.",
        ctl: ["T"],
        fig: leadFig2,
        math: () => ln(`a note such as ${V_("NO LEAD-IN ON THIS EDGE")}`) + ln("Leave room around the part for the lead-in when you nest parts yourself.", "dim"),
      },
      {
        t: "Holes start in the middle",
        p: "A hole's cut starts inside the hole, and the slug drops out. The pierce and the lead-in have to fit inside, so a hole has a smallest size set by the pierce, not just by the thickness.",
        ctl: ["T", "d"],
        fig: leadFig3,
        extra: procButtons,
        math: () => {
          const need = rule.pierce() + 2 * rule.leadIn();
          return ln(`d ≥ pierce + 2 × lead-in = ${num(rule.pierce())} + 2 × ${num(rule.leadIn())} = ${V_(len(need))}`) + verdict(ok(s.d, need), "Fits", "Too small for a clean start", `d ${num(s.d)}`);
        },
        basis: "Smaller holes are still cut, with shorter lead-ins and a poorer start. When a small hole matters, ask for it drilled.",
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: heat and thin features
  function tabRisk() {
    const slender = (s.tl / Math.max(s.tw, 0.01)) * (s.proc === "plasma" ? 1.5 : 1);
    const thin = s.tw / s.T;
    return slender > 8 && thin < 2 ? 2 : slender > 4 && thin < 4 ? 1 : 0;
  }
  const RISK = [["g", "fine"], ["a", "watch it"], ["r", "likely to curl"]];

  function heatFig1() {
    const lvl = tabRisk();
    const sc = clamp(320 / Math.max(s.tl, 1), 1, 10);
    const tw = clamp(s.tw * sc, 3, 120), tl = s.tl * sc;
    const y = 200 - tw / 2;
    const curl = lvl === 2 ? 30 : lvl === 1 ? 10 : 0;
    const tab = poly([[200, y], [200 + tl, y - curl], [200 + tl, y + tw - curl], [200, y + tw]]);
    let g = path(poly([[200, y], [200, 80], [60, 80], [60, 320], [200, 320], [200, y + tw]], false), "w");
    g += `<path class="fl" style="--d:.6s;fill:${["rgba(143,224,166,.12)", "rgba(242,169,59,.28)", "rgba(255,143,163,.32)"][lvl]}" d="${tab}"/>`;
    g += path(poly([[200, y], [200 + tl, y - curl], [200 + tl, y + tw - curl], [200, y + tw]], false), RISK[lvl][0]);
    label(220, y + tw + 40, RISK[lvl][1], RISK[lvl][0], 21);
    return svg(g, "A part with a long thin tab. The tab is shaded by how likely it is to curl from the heat of cutting.", "40 40 540 320");
  }


  function holeHeat() {
    const w = rule.minWeb();
    return s.g >= 2 * w ? 0 : s.g >= w ? 1 : 2;
  }
  function heatFig2() {
    const lvl = holeHeat();
    const sc = clamp(70 / Math.max(s.d + s.g, 1), 2, 20);
    const r = (s.d * sc) / 2, p = (s.d + s.g) * sc;
    let g = path(rectD(60, 50, 480, 280), "w");
    const glow = ["rgba(143,224,166,.0)", "rgba(242,169,59,.22)", "rgba(255,143,163,.32)"][lvl];
    g += `<rect class="fl" style="--d:.6s;fill:${glow}" x="${f1(300 - 2 * p)}" y="${f1(190 - p)}" width="${f1(4 * p)}" height="${f1(2 * p)}" rx="${f1(p / 2)}"/>`;
    for (let i = -2; i <= 2; i++) for (let j = -1; j <= 1; j++) g += circle(300 + i * p, 190 + j * p, r, RISK[lvl][0]);
    label(80, 310, RISK[lvl][1].replace("likely to curl", "heat builds up"), RISK[lvl][0], 20);
    return svg(g, "A tight grid of small holes. The area between them is shaded by how much heat builds up while they are cut.", "40 30 540 320");
  }

  function heatFig3() {
    const hz = s.proc === "plasma" ? 26 : 8;
    let g = fill(rectD(100, 80, 360, 220), "b") + path(rectD(100, 80, 360, 220));
    g += `<rect class="fl" style="--d:.6s;fill:rgba(242,169,59,.3)" x="${460 - hz}" y="80" width="${hz}" height="220"/>`;
    g += circle(400, 140, 14, "w") + label(400, 186, "tapped", "w", 17, "middle");
    g += path("M 410 230 q 20 -20 50 0", "w") + label(380, 270, "bend", "w", 17, "middle");
    label(470, 70, s.proc === "plasma" ? "wider hard edge" : "thin hard edge", "a", 18);
    return svg(g, "A plate with a shaded band along its cut edge where the heat hardened it, with a tapped hole and a bend line near that edge.", "60 40 520 290");
  }

  const HEAT = {
    id: "heat",
    title: "Heat and thin features",
    q: "Why did this tab curl?",
    blurb: "Warnings for slender tabs, hole clusters and hard edges",
    src: "Qualitative thermal-cutting practice. The thresholds here are a warning heuristic, not a calculation.",
    steps: [
      {
        t: "Long thin tabs curl",
        p: "Cutting puts heat into the metal next to the cut. A long, narrow tab is heated from both sides and has little metal to hold it straight, so it bends up or twists as it cools.",
        ctl: ["T", "tw", "tl"],
        fig: heatFig1,
        extra: procButtons,
        math: () => {
          const lvl = tabRisk();
          return ln(`length ÷ width = ${V_((s.tl / s.tw).toFixed(1))} · width ÷ T = ${V_((s.tw / s.T).toFixed(1))}`) + ln(`<span class="${["ok", "warn", "bad"][lvl]}">${["Fine", "Watch it", "Likely to curl"][lvl]}</span>`) + ln("Wider tabs, shorter tabs, or tabs that are cut last all help.", "dim");
        },
        basis: "A rule of thumb, not a calculation: watch tabs longer than about four widths and narrower than about four thicknesses. Plasma puts in more heat than laser.",
      },
      {
        t: "Clusters of small holes",
        p: "Each hole adds a little heat, and a tight cluster adds it all in one place. The plate between them can warp, and the last holes cut come out worse. Give clusters a wider web than the minimum.",
        ctl: ["T", "d", "g"],
        fig: heatFig2,
        extra: procButtons,
        math: () => {
          const lvl = holeHeat();
          return ln(`web ${num(s.g)} vs minimum ${num(rule.minWeb())}`) + ln(`<span class="${["ok", "warn", "bad"][lvl]}">${["Fine", "Heat builds up: widen the web", "Below the minimum web"][lvl]}</span>`) + ln("Twice the minimum web is a comfortable starting point.", "dim");
        },
      },
      {
        t: "Cut edges can be hard",
        p: "The metal right at a cut edge was melted and cooled fast. On steel, especially from plasma, that skin can be hard. Taps break in it and tight bends can crack along it. Laser leaves a much thinner skin.",
        ctl: ["T"],
        fig: heatFig3,
        extra: procButtons,
        math: () => ln(s.proc === "plasma" ? "Plasma: keep tapped holes and tight bends a little away from cut edges, or ask for them drilled or laser cut." : "Laser: usually fine to tap and bend close to the edge.") + ln("Nothing to draw. It's a reason to choose the process.", "dim"),
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: edge quality and bevel
  function edgeFig1() {
    const b = rad(PR().bevel);
    const sc = clamp(200 / s.T, 4, 60), Tp = clamp(s.T * sc, 10, 200);
    const top = 120, bot = top + Tp, x = 300, kp = kx();
    const lean = Tp * Math.tan(b);
    const laser = s.proc === "laser";
    const pb = laser ? x - kp / 2 + lean / 2 : x - kp / 2;
    const sb = laser ? x + kp / 2 - lean / 2 : x + kp / 2 - lean;
    const part = poly([[80, top], [x - kp / 2, top], [pb, bot], [80, bot]]);
    const scrap = poly([[520, top], [x + kp / 2, top], [sb, bot], [520, bot]]);
    let g = fill(part, "b") + path(poly([[80, top], [x - kp / 2, top]], false)) + path(poly([[x - kp / 2, top], [pb, bot]], false), "g") + path(poly([[pb, bot], [80, bot]], false));
    g += path(poly([[520, top], [x + kp / 2, top]], false)) + path(poly([[x + kp / 2, top], [sb, bot]], false), "r") + path(poly([[sb, bot], [520, bot]], false));
    label(90, top - 14, "part", "b", 20);
    label(510, top - 14, "scrap", "w", 20, "end");
    label(x - kp / 2 - 10, bot + 30, laser ? "nearly square" : "square side", "g", 18, "end");
    label(x + kp / 2 + 10, bot + 30, `${trim(PR().bevel)}° lean`, "r", 18);
    g += dim(66, top, 66, bot, "w") + label(58, (top + bot) / 2, "T", "w", 20, "end");
    return svg(g, "A section through a cut. On the part side the edge is square or nearly square; on the scrap side it leans at the bevel angle.", "30 80 540 300");
  }


  function edgeFig2() {
    const o = "M 110 80 H 490 V 320 H 110 Z";
    let g = fill(o, "b") + path(o, "w");
    g += arrow(150, 64, 260, 64, "a") + arrow(506, 120, 506, 220, "a");
    label(150, 50, "outside: clockwise", "a", 19);
    g += circle(300, 200, 50, "w");
    g += path(`M 300 130 A 70 70 0 0 0 232 186${head(237, 171, 232, 186)}`, "a");
    label(300, 290, "holes: counter-clockwise", "a", 19, "middle");
    return svg(g, "A part with a hole. Arrows show the outside cut running clockwise and the hole cut running counter-clockwise.", "60 30 520 300");
  }

  function edgeFig3() {
    const b = rad(PR().bevel);
    const sc = clamp(260 / Math.max(s.d, 2 * s.T), 2, 40), Tp = clamp(s.T * sc, 10, 180), dp = s.d * sc;
    const lean = Tp * Math.tan(b);
    const top = 100, cx = 300;
    const bottom = s.d - 2 * s.T * Math.tan(b);
    let g = fill(`M 60 ${top} H ${f1(cx - dp / 2)} L ${f1(cx - dp / 2 + lean)} ${f1(top + Tp)} H 60 Z`, "b") + fill(`M 540 ${top} H ${f1(cx + dp / 2)} L ${f1(cx + dp / 2 - lean)} ${f1(top + Tp)} H 540 Z`, "b");
    g += path(`M 60 ${top} H ${f1(cx - dp / 2)} L ${f1(cx - dp / 2 + lean)} ${f1(top + Tp)} H 60`) + path(`M 540 ${top} H ${f1(cx + dp / 2)} L ${f1(cx + dp / 2 - lean)} ${f1(top + Tp)} H 540`);
    g += dim(cx - dp / 2, top - 22, cx + dp / 2, top - 22, "w") + label(cx, top - 32, `top ${num(s.d)}`, "w", 19, "middle");
    g += dim(cx - dp / 2 + lean, top + Tp + 22, cx + dp / 2 - lean, top + Tp + 22, "r") + label(cx, top + Tp + 48, `bottom ${num(Math.max(bottom, 0))}`, "r", 19, "middle");
    return svg(g, "A section through a tapered hole: wider at the top face than at the bottom face.", "40 50 540 330");
  }

  const EDGE = {
    id: "edge",
    title: "Edge quality and bevel",
    q: "Which face is the good face?",
    blurb: "Cut edges lean; direction decides where",
    src: "Plasma cut-edge bevel and the convention of cutting outside contours clockwise and holes counter-clockwise for a clockwise-swirl torch; laser edges near square.",
    steps: [
      {
        t: "Cut edges lean",
        p: "A plasma arc swirls, so one side of the cut comes out nearly square and the other side leans by a few degrees. Laser edges are close to square on both sides. The lean is small, but across a thick plate it adds up.",
        ctl: ["T", "bev"],
        fig: edgeFig1,
        extra: procButtons,
        math: () => ln(`lean across the thickness = T × tan β = ${V_(len(s.T * Math.tan(rad(PR().bevel))))}`),
        basis: "Bevel angle is stored per process. A few degrees on the bad side is typical for plasma; measure your own parts.",
      },
      {
        t: "Direction decides the good side",
        p: "With a clockwise-swirl torch, the square side is on the right of the direction of travel. So programs cut outside edges clockwise and holes counter-clockwise, which keeps the square side on the part. Most torches swirl clockwise.",
        ctl: [],
        fig: edgeFig2,
        math: () => ln("outside contours: clockwise") + ln("holes and cut-outs: counter-clockwise") + ln("That's the shop's job, but it's why the inside of a plasma hole can lean.", "dim"),
      },
      {
        t: "Tapered holes and bolt fit",
        p: "When the lean lands on the part, a hole is smaller at one face than the other. For a clearance hole that's usually fine. For a pin, a press fit or a tapped hole, note the critical face, or ask for the hole drilled or laser cut.",
        ctl: ["T", "d", "bev"],
        fig: edgeFig3,
        extra: procButtons,
        math: () => {
          const bottom = s.d - 2 * s.T * Math.tan(rad(PR().bevel));
          return ln(`bottom = d − 2·T·tan β = ${V_(len(bottom))}`) + ln(`${num(s.d - bottom)} smaller than the top`, bottom < s.d - 0.1 ? "warn" : "dim");
        },
      },
    ],
  };

  // ================================================================ DRAWINGS FOR THE SHOP

  // ---------------------------------------------------------------- lesson: dimensioning for fabrication
  function dimFig1() {
    const nH = Math.round(s.n);
    const span = (nH + 1) * s.pt;
    const pp = 460 / span, x0 = 60, cy = 150, r = clamp(s.pt * pp * 0.18, 4, 18);
    let g = path(rectD(x0, 90, span * pp, 120), "w");
    const xs = Array.from({ length: nH }, (_, i) => x0 + (i + 1) * s.pt * pp);
    xs.forEach((x) => (g += circle(x, cy, r, "w")));
    // chain, above
    let prev = x0;
    xs.forEach((x) => {
      g += dim(prev, 72, x, 72, "a");
      prev = x;
    });
    label(x0, 56, "chain", "a", 19);
    // baseline, below
    xs.forEach((x, i) => (g += dim(x0, 232 + i * 12, x, 232 + i * 12, "b")));
    label(x0, 248 + nH * 12, "baseline", "b", 19);
    // worst-case spread at the last hole, drawn larger than life
    const last = xs[xs.length - 1];
    const k = 60 / Math.max(nH * s.tol, 0.01);
    const ch = nH * s.tol * k, bl = s.tol * k;
    g += path(`M ${f1(last - ch / 2)} 40 H ${f1(last + ch / 2)}`, "r", "thick") + path(`M ${f1(last - bl / 2)} ${f1(248 + nH * 12 + 14)} H ${f1(last + bl / 2)}`, "g", "thick");
    label(last + ch / 2 + 8, 46, `±${num(nH * s.tol)}`, "r", 19);
    label(last + bl / 2 + 8, 252 + nH * 12 + 14, `±${num(s.tol)}`, "g", 19);
    return svg(g, "A plate with a row of holes, dimensioned two ways: as a chain of hole-to-hole dimensions above, and as baseline dimensions from one edge below. Bars at the last hole show how far it can wander each way.", "40 20 560 " + (300 + nH * 12));
  }

  function dimFig2() {
    const p = profile({ cx: 330, y0: 250, Lh: 220, Lf: 150 });
    let g = p.hull;
    const hx = p.xs + 90;
    g += dash(`M ${hx} ${p.y0 - 18} V ${f1(p.y0 + p.T + 18)}`, "b");
    const mid = p.add(p.b1, 90), mi = p.add(p.a1, 90);
    const nrm = [Math.sin(p.th), Math.cos(p.th)];
    g += dash(`M ${f1(mi[0] - nrm[0] * 18)} ${f1(mi[1] - nrm[1] * 18)} L ${f1(mid[0] + nrm[0] * 18)} ${f1(mid[1] + nrm[1] * 18)}`, "b");
    const stack = PR().tol + s.nb * shop.bendTol;
    g += dash(`M ${hx} ${p.y0 + p.T + 40} L ${f1(mid[0] + nrm[0] * 40)} ${f1(mid[1] + nrm[1] * 40)}`, s.nb > 0 ? "r" : "g");
    label(hx + 10, p.y0 + p.T + 70, `hole to hole ±${num(stack)}`, s.nb > 0 ? "r" : "g", 20);
    return svg(g, "A bent bracket with one hole on the base and one on the flange. The dimension between them crosses the bend, so it carries the bend's tolerance too.", "60 40 540 330");
  }

  function dimFig3() {
    const c = calc();
    const p = profile({ cx: 300, y0: 230, Lh: 210, Lf: 120 });
    const os = c.OSSB * p.sc;
    const Vo = [p.b0[0] + os, p.b0[1]];
    let g = p.hull + dash(`M ${p.P(p.b0)} L ${p.P(Vo)} L ${p.P(p.b1)}`, "b");
    g += dim(p.xs, p.y0 + p.T + 30, Vo[0], p.y0 + p.T + 30, "g") + label((p.xs + Vo[0]) / 2, p.y0 + p.T + 56, "to the outside face", "g", 19, "middle") + mark((p.xs + Vo[0]) / 2 + 110, p.y0 + p.T + 50, true);
    g += dash(`M ${f1(p.a0[0])} ${p.y0 - 60} V ${f1(p.y0 + p.T)}`, "r");
    g += dim(p.xs, p.y0 - 44, p.a0[0], p.y0 - 44, "r") + label(p.xs + 10, p.y0 - 56, "to where the bend starts", "r", 19) + mark(p.a0[0] + 18, p.y0 - 60, false);
    return svg(g, "A bend dimensioned two ways: to the outside face, which can be measured, and to the line where the bend starts, which cannot.", "60 60 540 300");
  }

  const DIMS = {
    id: "dims",
    title: "Dimensioning for fabrication",
    q: "What does the shop actually need?",
    blurb: "Baseline vs chain, bends in the stack, measurable features",
    src: "Worst-case tolerance stacking (sum of the tolerances in the chain); cut and bend tolerances as Shop settings starting points.",
    steps: [
      {
        t: "Chain or baseline",
        p: "A chain dimensions each hole from the one before. Every link carries its own tolerance, so errors add up toward the far end. Baseline dimensions measure every hole from one edge, and no hole can wander further than one tolerance.",
        ctl: ["n", "pt", "tol"],
        fig: dimFig1,
        ex: () => exBox([`5 holes, ±${num(inch() ? 0.254 : 0.2)} on each dimension`, `chain: last hole can be off ±5 × ${num(inch() ? 0.254 : 0.2)} = <span class="v">±${num(inch() ? 1.27 : 1)}</span>`, `baseline: ±${num(inch() ? 0.254 : 0.2)}`]),
        math: () => ln(`chain: ±${Math.round(s.n)} × ${num(s.tol)} = <span class="bad">±${len(Math.round(s.n) * s.tol)}</span> at the last hole`) + ln(`baseline: <span class="ok">±${len(s.tol)}</span> at every hole`),
        basis: "Worst case: every link at its limit the same way. Real parts usually do better, but worst case is what gets rejected.",
        place: { label: "Draw a baseline-dimensioned plate", run: () => placeDimPlate() },
      },
      {
        t: "Bends join the stack",
        p: "A cut edge holds a tight tolerance. A bend holds a looser one, because the brake, the material and the operator all play in. A dimension that crosses a bend carries both.",
        ctl: ["nb"],
        fig: dimFig2,
        extra: procButtons,
        math: () => {
          const stack = PR().tol + s.nb * shop.bendTol;
          return ln(`±${num(PR().tol)} cut + ${Math.round(s.nb)} × ±${num(shop.bendTol)} bend = ${V_(`±${len(stack)}`)}`) + (s.nb > 0 ? ln("Tight fits across a bend: use a slot, or put both features on one face.", "dim") : ln("Same face: only the cut tolerance.", "ok"));
        },
        basis: () => `Cut ±${num(PR().tol)} (${procName()}) and bend ±${num(shop.bendTol)} per bend are starting points in Shop settings.`,
      },
      {
        t: "Dimension what can be measured",
        p: "An inspector measures faces, edges and hole centres. A dimension to where a bend starts can't be measured on the finished part. Dimension formed parts to their outside faces, and say so if you use the inside.",
        ctl: ["T", "R"],
        fig: dimFig3,
        math: () => ln("to outside faces and hole centres", "ok") + ln("not to bend tangent lines or to the middle of a radius", "bad") + ln("Choose one face as the start and dimension from it.", "dim"),
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: tolerances
  // ISO 2768-1:1989, general tolerances for linear dimensions (classes m and c), mm.
  const ISO2768 = [
    [0.5, 3, 0.1, 0.2],
    [3, 6, 0.1, 0.3],
    [6, 30, 0.2, 0.5],
    [30, 120, 0.3, 0.8],
    [120, 400, 0.5, 1.2],
    [400, 1000, 0.8, 2],
    [1000, 2000, 1.2, 3],
    [2000, 4000, 2, 4],
  ];
  const isoRow = (L) => ISO2768.find(([a, b]) => L > a - 1e-9 && L <= b) || (L <= 0.5 ? null : ISO2768[ISO2768.length - 1]);

  function tolFig1() {
    const row = isoRow(s.Ln);
    let g = path(rectD(70, 50, 460, 300), "w") + path("M 70 92 H 530", "w") + path("M 300 92 V 350 M 410 92 V 350", "b");
    label(84, 80, "GENERAL TOLERANCES  ISO 2768-m", "w", 19);
    label(185, 118, "size (mm)", "b", 18, "middle") + label(355, 118, "m", "b", 18, "middle") + label(470, 118, "c", "b", 18, "middle");
    ISO2768.forEach((r, i) => {
      const y = 148 + i * 26, on = r === row;
      label(185, y, `${r[0]} to ${r[1]}`, on ? "a" : "w", 18, "middle");
      label(355, y, `±${r[2]}`, on ? "a" : "w", 18, "middle");
      label(470, y, `±${r[3]}`, on ? "a" : "w", 18, "middle");
    });
    return svg(g, "A title-block style table of general tolerances by size range, with the row for the current size highlighted.", "50 30 500 340");
  }

  function tolFig2() {
    const bars = [
      ["Laser cut", shop.procs.laser.tol],
      ["Plasma cut", shop.procs.plasma.tol],
      [`${procName()} + one bend`, PR().tol + shop.bendTol],
    ];
    const max = Math.max(...bars.map((b) => b[1]), s.tq) * 1.15;
    const px = (v) => 200 + (v / max) * 320;
    let g = "";
    bars.forEach(([name, v], i) => {
      const y = 90 + i * 70, good = ok(s.tq, v);
      label(190, y + 20, name, "w", 19, "end");
      g += fill(rectD(200, y, px(v) - 200, 28), good ? "b" : "r") + path(rectD(200, y, px(v) - 200, 28), col(good));
      label(px(v) + 8, y + 20, `±${num(v)}`, col(good), 18);
    });
    g += path(`M ${f1(px(s.tq))} 60 V 310`, "a", "thick");
    label(px(s.tq), 340, `you need ±${num(s.tq)}`, "a", 19, "middle");
    return svg(g, "Bars for what each process can hold, with a line for the tolerance you need. Bars shorter than the line can hold it.", "20 40 560 320");
  }

  function tolFig3() {
    const cx = 120, cy = 300, Lp = 360;
    const a = rad(shop.angTol);
    let g = path(`M 60 ${cy} H ${cx} V ${cy - Lp * 0.85}`, "w", "thick");
    const tip = (t) => [cx + Math.sin(t) * Lp * 0.85, cy - Math.cos(t) * Lp * 0.85];
    const p1 = tip(a * 6), p2 = tip(-a * 6);
    g += dash(`M ${cx} ${cy} L ${f1(p1[0])} ${f1(p1[1])} M ${cx} ${cy} L ${f1(p2[0])} ${f1(p2[1])}`, "r");
    g += path(`M ${f1(p2[0])} ${f1(p2[1])} L ${f1(p1[0])} ${f1(p1[1])}`, "r");
    label(p1[0] + 12, p1[1] + 6, `±${num(s.fl * Math.tan(a))} at the end`, "r", 19);
    label(cx + 14, cy - 40, `±${trim(shop.angTol)}°`, "a", 19);
    g += dim(cx + 30, cy, cx + 30, cy - Lp * 0.85, "w") + label(cx + 40, cy - (Lp * 0.85) / 2, `L ${num(s.fl)}`, "w", 19);
    return svg(g, "A flange with its angle tolerance drawn as a fan, exaggerated, showing how far the end of the flange can move.", "40 0 520 340");
  }

  const TOL = {
    id: "tol",
    title: "Tolerances the shop can hold",
    q: "Can they actually make it to this?",
    blurb: "Title blocks, process capability, angles that become lengths",
    src: "ISO 2768-1:1989 general tolerances for linear dimensions (classes m and c); process tolerances as Shop settings starting points; angle-to-length by L·tan(angle).",
    steps: [
      {
        t: "Read the title block",
        p: "Most dimensions carry no tolerance of their own. They take the general tolerance from the title block, which often points to a standard table by size. A tolerance written on a dimension overrides the block.",
        ctl: ["Ln"],
        fig: tolFig1,
        math: () => {
          const r = isoRow(s.Ln);
          return r ? ln(`${len(s.Ln)} is in ${r[0]} to ${r[1]} mm: ${V_(`m ±${r[2]} mm`)}, ${V_(`c ±${r[3]} mm`)}`) : ln("Under 0.5 mm: no general tolerance; write one on the dimension.", "warn");
        },
        basis: "ISO 2768-1:1989, classes m (medium) and c (coarse). Many shops in inch units use a decimal-places block instead, such as .X ±.03, .XX ±.01, .XXX ±.005. Check your own title block.",
      },
      {
        t: "What the process can hold",
        p: "A tolerance tighter than the process can hold means extra work: a slower cut, a second operation, or parts that get rejected. Ask for tight tolerances only where the part needs them.",
        ctl: ["tq"],
        fig: tolFig2,
        extra: procButtons,
        math: () => {
          const c = PR().tol, b = c + shop.bendTol;
          return verdict(ok(s.tq, c), `${procName()} cut holds it`, `Tighter than ${procName()} cutting`, `±${num(s.tq)} vs ±${num(c)}`) + verdict(ok(s.tq, b), "Holds across a bend", "Not across a bend", `±${num(b)}`) + (!ok(s.tq, c) ? ln("Drill, ream or machine that feature after cutting.", "dim") : "");
        },
        basis: "Process tolerances are starting points in Shop settings. A good fabricator will tell you their real numbers.",
      },
      {
        t: "Angles become lengths",
        p: "A bend angle tolerance looks small: ±1°. But at the end of a long flange it becomes a length. A flange that has to line up with something far away needs a fixture or a tighter angle.",
        ctl: ["fl"],
        fig: tolFig3,
        ex: () => {
          const L = inch() ? 4 * 25.4 : 100;
          return exBox([`±1° on a ${len(L)} flange`, `end moves ±${num(L)} × tan 1° = <span class="v">±${len(L * Math.tan(rad(1)))}</span>`]);
        },
        math: () => ln(`end moves ±L × tan(${trim(shop.angTol)}°) = ±${num(s.fl)} × ${Math.tan(rad(shop.angTol)).toFixed(4)} = ${V_(`±${len(s.fl * Math.tan(rad(shop.angTol)))}`)}`),
        basis: () => `Angle tolerance ±${trim(shop.angTol)}° from Shop settings.`,
      },
    ],
  };

  // ================================================================ WELD AND ASSEMBLY

  // ---------------------------------------------------------------- lesson: fit-up and gap
  function fitFig1() {
    const sc = clamp(150 / s.T, 3, 40), Tp = s.T * sc, gp = s.gap * sc;
    const top = 120, bot = top + Tp, x = 300;
    const thin = s.T <= shop.buttMax + 1e-9;
    const goodGap = s.gap <= shop.gapFrac * s.T + 1e-9;
    const c = thin ? col(goodGap) : "r";
    let g = fill(rectD(80, top, x - gp / 2 - 80, Tp), "b") + path(rectD(80, top, x - gp / 2 - 80, Tp), "w");
    g += fill(rectD(x + gp / 2, top, 520 - x - gp / 2, Tp), "b") + path(rectD(x + gp / 2, top, 520 - x - gp / 2, Tp), "w");
    g += dim(x - gp / 2, bot + 26, x + gp / 2, bot + 26, c) + label(x, bot + 54, `gap ${num(s.gap)}`, c, 19, "middle");
    g += dim(64, top, 64, bot, "w") + label(56, (top + bot) / 2, "T", "w", 20, "end");
    label(x, top - 20, thin ? "square butt" : "too thick for a square butt", thin ? "w" : "r", 20, "middle");
    return svg(g, "Two plates in section, end to end, with a gap between them: a square butt joint.", "30 70 540 280");
  }

  function grooveArea() {
    const half = rad(s.inc / 2);
    const h = Math.max(s.T - s.rf, 0);
    return s.gap * s.T + h * h * Math.tan(half);
  }
  function fitFig2() {
    const sc = clamp(180 / s.T, 3, 40), Tp = s.T * sc, gp = s.gap * sc, rfp = Math.min(s.rf * sc, Tp);
    const half = rad(s.inc / 2);
    const top = 110, bot = top + Tp, x = 300;
    const open = (Tp - rfp) * Math.tan(half);
    const L = [[80, top], [x - gp / 2 - open, top], [x - gp / 2, bot - rfp], [x - gp / 2, bot], [80, bot]];
    const R = [[520, top], [x + gp / 2 + open, top], [x + gp / 2, bot - rfp], [x + gp / 2, bot], [520, bot]];
    let g = fill(poly(L), "b") + path(poly(L)) + fill(poly(R), "b") + path(poly(R));
    const groove = poly([[x - gp / 2 - open, top], [x + gp / 2 + open, top], [x + gp / 2, bot - rfp], [x + gp / 2, bot], [x - gp / 2, bot], [x - gp / 2, bot - rfp]]);
    g += fill(groove, "a") + dash(groove, "a");
    g += path(`M ${f1(x - gp / 2 - open)} ${top - 30} L ${f1(x - gp / 2)} ${f1(bot - rfp)} L ${f1(x + gp / 2 + open)} ${top - 30}`, "b");
    label(x, top - 40, `${Math.round(s.inc)}°`, "b", 20, "middle");
    g += dim(x - gp / 2, bot + 26, x + gp / 2, bot + 26, "w") + label(x, bot + 52, `gap ${num(s.gap)}`, "w", 18, "middle");
    g += dim(x + gp / 2 + 30, bot - rfp, x + gp / 2 + 30, bot, "w") + label(x + gp / 2 + 40, bot - rfp / 2 + 6, `root face ${num(s.rf)}`, "w", 18);
    return svg(g, "Two thick plates in section with a V-shaped groove between them: the bevel angle, root face and root gap are marked, and the groove area that weld metal must fill is shaded.", "30 50 540 300");
  }

  function fitFig3() {
    const arrowSide = s.wside !== "other";
    let g = fill(rectD(80, 250, 200, 40), "b") + path(rectD(80, 250, 200, 40)) + fill(rectD(290, 250, 200, 40), "b") + path(rectD(290, 250, 200, 40));
    g += path("M 285 250 L 220 120", "w") + path(`M 285 250${head(250, 180, 285, 250)}`, "w");
    g += path("M 220 120 H 470", "w");
    const vy = arrowSide ? 120 : 120;
    g += arrowSide ? path(`M 330 ${vy} l 18 26 l 18 -26`, "a") : path(`M 330 ${vy} l 18 -26 l 18 26`, "a");
    g += path("M 470 120 l 22 -22 M 470 120 l 22 22", "w");
    label(220, 100, "reference line", "w", 18);
    label(348, arrowSide ? 176 : 80, arrowSide ? "V below: arrow side" : "V above: other side", "a", 19, "middle");
    label(500, 126, "tail", "w", 17);
    label(150, 330, arrowSide ? "groove faces the arrow" : "groove on the far side", "a", 18);
    return svg(g, "A weld symbol: an arrow pointing at a butt joint, a horizontal reference line, and a V groove symbol below or above the line.", "60 50 500 310");
  }

  const FITUP = {
    id: "fitup",
    title: "Fit-up and gap",
    q: "How should plate edges meet for welding?",
    blurb: "Square butts, V grooves and the weld symbol",
    src: "General welding-joint practice: square butt for thin material with a small gap; single-V around 60° included with a root face and root gap for thicker plate. Prequalified joint details are in standards such as AWS D1.1/D1.1M:2020 and ISO 9692-1:2013; weld symbols per AWS A2.4:2020.",
    steps: [
      {
        t: "Thin plate: a square butt",
        p: "Thin plates can be welded edge to edge with a small gap, so the weld gets through to the back. Too big a gap and the weld falls through. Past a certain thickness the weld can't reach the root at all.",
        ctl: ["T", "gap"],
        fig: fitFig1,
        math: () => {
          const thin = s.T <= shop.buttMax + 1e-9;
          return thin ? ln(`gap 0 to ${trim(shop.gapFrac)} × T = 0 to ${V_(len(shop.gapFrac * s.T))}`) + verdict(s.gap <= shop.gapFrac * s.T + 1e-9, "Gap is right", "Gap is too wide", num(s.gap)) : ln(`<span class="bad">Over ${num(shop.buttMax)}</span>: bevel the edges (next step).`);
        },
        basis: () => `Square butt up to ${num(shop.buttMax)} with a gap up to ${trim(shop.gapFrac)} × T: a starting point that depends on the process and on welding from one side or two. The welding procedure (WPS) decides.`,
      },
      {
        t: "Thicker plate: bevel it",
        p: "Thicker plate gets a groove so the weld can reach the bottom. A V with about 60° included, a small flat root face and a small gap is a common start. A wider angle is easier to weld but needs more weld metal.",
        ctl: ["T", "inc", "rf", "gap"],
        fig: fitFig2,
        extra: matButtons,
        math: () => {
          const A = grooveArea();
          return ln(`area = g·T + (T − f)² × tan(α÷2) = ${V_(`${A.toFixed(1)} mm²`)}`) + ln(`about ${V_(`${((A * MAT().rho) / 1000).toFixed(2)} kg`)} of weld metal per metre (${MAT().name})`) + ln("Wider angle, more metal, more heat, more distortion.", "dim");
        },
        basis: "Area of the groove only, without the cap or the root bead. Use it to compare joint shapes, not to order wire.",
      },
      {
        t: "Saying it on the drawing",
        p: "A weld symbol tells the welder what joint to make. The arrow points at the joint. A symbol under the reference line means the arrow side, over the line means the other side.",
        ctl: [],
        fig: fitFig3,
        extra: () => `<div class="bb-row">${buttons("wside", [["arrow", "Arrow side"], ["other", "Other side"]], s.wside === "other" ? "other" : "arrow")}</div>`,
        math: () => ln(`V groove ${s.wside === "other" ? "above" : "below"} the line → ${V_(s.wside === "other" ? "other side" : "arrow side")}`) + ln("This is the AWS convention. ISO 2553 marks sides with an extra dashed line.", "dim"),
      },
    ],
  };

  // ---------------------------------------------------------------- lesson: tab and slot, and distortion
  function tabFig1() {
    const c = PR().slot, sw = rule.slotW(), sl = s.tabw + 2 * c;
    const good = ok(sw, Math.max(rule.minDia(), PR().kerf));
    const sc = clamp(300 / sl, 2, 40), exag = 6;
    const slotW = s.T * sc + 2 * c * sc * exag, slotL = s.tabw * sc + 2 * c * sc * exag;
    const cx = 300, cy = 190;
    let g = fill(rectD(60, 60, 480, 260), "b") + path(rectD(60, 60, 480, 260));
    g += `<rect class="fl" style="--d:.5s;fill:var(--slate)" x="${f1(cx - slotL / 2)}" y="${f1(cy - slotW / 2)}" width="${f1(slotL)}" height="${f1(slotW)}"/>` + path(rectD(cx - slotL / 2, cy - slotW / 2, slotL, slotW), col(good));
    g += fill(rectD(cx - (s.tabw * sc) / 2, cy - (s.T * sc) / 2, s.tabw * sc, s.T * sc), "a") + path(rectD(cx - (s.tabw * sc) / 2, cy - (s.T * sc) / 2, s.tabw * sc, s.T * sc), "a");
    label(cx, cy - slotW / 2 - 16, `slot ${num(sl)} × ${num(sw)}`, col(good), 19, "middle");
    label(cx, cy + slotW / 2 + 30, `tab ${num(s.tabw)} × ${num(s.T)}`, "a", 19, "middle");
    label(70, 346, "clearance drawn larger than life", "b", 16);
    return svg(g, "Top view of a base plate with a rectangular slot, and the tab of an upright part sitting in it with clearance all round.", "40 30 520 330");
  }

  function weldPull() {
    const q = s.leg / s.T;
    return q < 0.5 ? 0 : q <= 1 ? 1 : 2;
  }
  function tabFig2() {
    const lvl = weldPull();
    const sc = clamp(26 / s.T, 2, 10), Tp = s.T * sc, z = s.leg * sc;
    const one = (x0, both) => {
      const base = rectD(x0, 250, 200, Tp);
      const lean = both ? 0 : [0, 8, 18][lvl];
      const up = poly([[x0 + 100 - Tp / 2, 250], [x0 + 100 - Tp / 2 + lean, 90], [x0 + 100 + Tp / 2 + lean, 90], [x0 + 100 + Tp / 2, 250]]);
      let g = fill(base, "b") + path(base) + fill(up, "b") + path(up);
      g += fill(poly([[x0 + 100 + Tp / 2, 250], [x0 + 100 + Tp / 2 + z, 250], [x0 + 100 + Tp / 2, 250 - z]]), "a") + path(poly([[x0 + 100 + Tp / 2, 250], [x0 + 100 + Tp / 2 + z, 250], [x0 + 100 + Tp / 2, 250 - z]]), "a");
      if (both) g += fill(poly([[x0 + 100 - Tp / 2, 250], [x0 + 100 - Tp / 2 - z, 250], [x0 + 100 - Tp / 2, 250 - z]]), "a") + path(poly([[x0 + 100 - Tp / 2, 250], [x0 + 100 - Tp / 2 - z, 250], [x0 + 100 - Tp / 2, 250 - z]]), "a");
      else if (lvl) g += arrow(x0 + 100 + 30, 110, x0 + 100 + 30 + 10 * lvl + 10, 104, "r");
      return g;
    };
    let g = one(60, false) + one(320, true);
    label(160, 310, ["one side: small pull", "one side: pulls over", "one side: pulls hard"][lvl], RISK[lvl][0], 18, "middle");
    label(420, 310, "both sides: balanced", "g", 18, "middle");
    return svg(g, "Two T-joints in section. On the left a fillet weld on one side pulls the upright plate toward it. On the right welds on both sides balance.", "40 60 520 280");
  }

  function tabFig3() {
    let g = fill(rectD(60, 200, 480, 120), "b") + path(rectD(60, 200, 480, 120));
    for (const x of [120, 280, 440]) g += path(rectD(x - 25, 250, 50, 12), "w");
    const up = `M 60 70 H 540 V 150 H 465 V 168 H 415 V 150 H 305 V 168 H 255 V 150 H 145 V 168 H 95 V 150 H 60 Z`;
    g += fill(up, "a") + path(up, "a");
    label(70, 60, "upright: tabs", "a", 19);
    label(70, 346, "base: slots in the same places", "b", 19);
    return svg(g, "Two flat parts: an upright with three tabs along its edge, and a base with three slots that line up with them.", "40 30 520 330");
  }

  const TAB = {
    id: "tab",
    title: "Tab-and-slot and distortion",
    q: "How do I make parts that line themselves up?",
    blurb: "Slots that locate, welds that pull, parts that fixture themselves",
    src: "Tab-and-slot sizing from thickness plus a clearance per side (Shop settings); weld shrinkage and angular distortion as general welding practice, shown as a warning, not a calculation.",
    steps: [
      {
        t: "Slots that locate parts",
        p: "Cut a tab on one part and a matching slot in the other, and they only go together one way, in the right place. The slot is the thickness plus a little clearance each side. It's still a cut, so it has to obey the smallest-hole rule.",
        ctl: ["T", "tabw"],
        fig: tabFig1,
        extra: procButtons,
        ex: () => {
          const o = exVals({ T: 3 }, { T: 0.125 });
          const c = PR().slot;
          return exBox([`${procName()}, T ${nm(o.T)}, clearance ${num(c)} each side`, `slot width = T + 2c = ${nm(o.T)} + ${num(2 * c)} = <span class="v">${len(o.T + 2 * c)}</span>`, `smallest hole = ${trim(PR().hole)} × T = ${len(PR().hole * o.T)}`]);
        },
        math: () => {
          const sw = rule.slotW(), need = Math.max(rule.minDia(), PR().kerf);
          return ln(`slot = T + 2 × ${num(PR().slot)} = ${V_(len(sw))} wide, tab + 2 × ${num(PR().slot)} = ${V_(len(s.tabw + 2 * PR().slot))} long`) + verdict(ok(sw, need), "Slot can be cut", "Slot is narrower than the smallest hole", `${num(sw)} of ${num(need)}`);
        },
        basis: () => `Clearance ${num(PR().slot)} per side for ${procName()} (Shop settings). Too tight and parts won't go together; too loose and they don't locate.`,
        place: { label: "Draw a tab-and-slot pair", run: () => placeTabSlot() },
      },
      {
        t: "Welds pull",
        p: "Weld metal shrinks as it cools, and it pulls the parts toward it. A weld on one side of a joint bends the joint toward that side. Welds on both sides, in turn, pull against each other.",
        ctl: ["T", "leg"],
        fig: tabFig2,
        math: () => {
          const lvl = weldPull();
          return ln(`weld leg ÷ T = ${V_((s.leg / s.T).toFixed(2))}`) + ln(`<span class="${["ok", "warn", "bad"][lvl]}">${["Small pull", "Expect some pull", "Big weld on thin plate: expect a lot"][lvl]}</span>`) + ln("Don't draw welds bigger than the joint needs.", "dim");
        },
        basis: "A warning scale, not a calculation: under half the thickness pulls little, more than the thickness pulls a lot.",
      },
      {
        t: "Let the parts fixture themselves",
        p: "Put tabs where they square the parts up, space them evenly, and make the part symmetric where you can. The fitter then needs no measuring, and welds can be laid in a balanced order.",
        ctl: ["T"],
        fig: tabFig3,
        math: () => ln("tabs at both ends and in the middle", "ok") + ln("one way to fit: make one tab a different width so the part can't go in backwards", "dim"),
        place: { label: "Draw a tab-and-slot pair", run: () => placeTabSlot() },
      },
    ],
  };


  // ================================================================ DRAFTING
  // Lessons written around BlackCAD itself: its commands, layers, panels and
  // keys. Every tool a lesson teaches is in the core (cad_core); a lesson
  // draws a picture, sets up an exercise, and reads the drawing back to check
  // it. Nothing here measures or edits geometry itself.

  // ---------------------------------------------------------------- what BlackCAD can do today
  const hasCmd = (name) => COMMANDS.some((c) => c.name === name || c.aliases.includes(name));
  const aliasOf = (name) => {
    const c = COMMANDS.find((c) => c.name === name);
    return c ? c.aliases[0] || c.name : name;
  };

  // ---------------------------------------------------------------- chalk for layer styles
  /** A full circle as a path: two half arcs, so the centre is never ambiguous. */
  const circD = (cx, cy, r) => `M ${f1(cx - r)} ${f1(cy)} A ${f1(r)} ${f1(r)} 0 1 0 ${f1(cx + r)} ${f1(cy)} A ${f1(r)} ${f1(r)} 0 1 0 ${f1(cx - r)} ${f1(cy)}`;
  const nu = (mm) => `${nt(mm)} ${U()}`;
  const layerChalk = (aci) => ({ 1: "r", 2: "y", 3: "g", 4: "c", 5: "b", 6: "r" })[aci] || "w";
  const ltKind = (name = "") => (/CENTER|CENTRE/i.test(name) ? "center" : /HIDDEN|DASH/i.test(name) ? "hidden" : /PHANTOM/i.test(name) ? "phantom" : "cont");
  /** A path in a linetype: dashed kinds fade in, continuous ones draw. */
  const lt = (d, c = "w", kind = "cont", w = 2.4) => {
    const da = { hidden: "9 6", center: "26 6 6 6", phantom: "26 6 6 6 6 6" }[kind];
    const delay = (n++ * 0.11).toFixed(2);
    return da ? `<path class="dsh c-${c}" style="--d:${delay}s;stroke-dasharray:${da};stroke-width:${w}" d="${d}"/>` : `<path class="ch c-${c}" pathLength="1" style="--d:${delay}s;stroke-width:${w}" d="${d}"/>`;
  };
  const DEFAULT_LAYERS = [
    { name: "0", aci: 7, linetype: "Continuous", weight: 0.25 },
    { name: "VISIBLE", aci: 7, linetype: "Continuous", weight: 0.5, current: true },
    { name: "CENTER", aci: 2, linetype: "CENTER", weight: 0.25 },
    { name: "HIDDEN", aci: 1, linetype: "HIDDEN", weight: 0.25 },
    { name: "DIMENSION", aci: 3, linetype: "Continuous", weight: 0.25 },
    { name: "NOTES", aci: 4, linetype: "Continuous", weight: 0.25 },
  ];
  const layerList = () => (doc?.info.layers?.length ? doc.info.layers : DEFAULT_LAYERS);
  const LAYER_MEANS = {
    0: "the base layer every drawing has; keep your own work off it",
    VISIBLE: "edges you can see: the part itself, drawn thick",
    CENTER: "axes of holes and round features",
    HIDDEN: "edges behind a surface, dashed",
    DIMENSION: "sizes; dimensions land here by themselves",
    NOTES: "words: notes, labels and the title block",
  };
  /** A snap marker, shaped as BlackCAD draws it. */
  function smark(kind, x, y, c = "a", r = 9) {
    const d = {
      END: `M ${x - r} ${y - r} h ${2 * r} v ${2 * r} h ${-2 * r} Z`,
      MID: `M ${x} ${y - r} L ${x + r} ${y + r * 0.8} L ${x - r} ${y + r * 0.8} Z`,
      CEN: `${circD(x, y, r)} M ${x - 5} ${y} h 10 M ${x} ${y - 5} v 10`,
      QUA: `M ${x} ${y - r} L ${x + r} ${y} L ${x} ${y + r} L ${x - r} ${y} Z`,
      INT: `M ${x - r} ${y - r} L ${x + r} ${y + r} M ${x + r} ${y - r} L ${x - r} ${y + r}`,
      NEA: `M ${x - r} ${y - r} H ${x + r} L ${x - r} ${y + r} H ${x + r} Z`,
      NOD: `${circD(x, y, r)} M ${x - 6} ${y - 6} l 12 12 M ${x + 6} ${y - 6} l -12 12`,
    }[kind];
    return path(d, c);
  }

  // ---------------------------------------------------------------- numbers as the learner types them
  /** A length shown as it would be typed, in the board's units. */
  const nt = (v) => String(+(inch() ? v / 25.4 : v).toFixed(inch() ? 3 : 1));
  /** The drawing's own units for exercises: the open drawing's, else the board's. */
  const du = () => (doc ? (doc.info.insunits === 1 ? "in" : "mm") : s.unit);
  const xv = (mm, inches) => (du() === "in" ? inches : mm);
  const xs = (v) => String(+(+v).toFixed(4));
  const xtol = () => (du() === "in" ? 0.0006 : 0.015);
  const xf = (v) => `${xs((+v).toFixed(du() === "in" ? 3 : 2))}${du() === "in" ? " in" : " mm"}`;
  const kbd = (t) => `<kbd>${t}</kbd>`;

  // ---------------------------------------------------------------- reading the open drawing
  function entities() {
    if (!doc) return [];
    const out = [];
    for (const [h, idxs] of doc.disp.byHandle) {
      if (idxs.every((k) => doc.disp.items[k]?.dead)) continue;
      const g = core.query(doc.id, `get ${h}`);
      if (g) out.push(g);
    }
    return out;
  }
  const same = (a, b, t = xtol()) => Math.abs(a - b) <= t;
  const samePt = (p, q, t = xtol()) => same(p[0], q[0], t) && same(p[1], q[1], t);
  const lay = (g) => String(g.layer || "").toUpperCase();
  /** Axis-aligned rectangles, from closed four-corner polylines or from four lines. */
  function rectsOf(es) {
    const t = xtol(), out = [];
    for (const g of es) {
      if (g.type !== "LWPOLYLINE" || !g.points) continue;
      let pts = g.points.slice();
      if (pts.length === 5 && samePt(pts[0], pts[4])) pts = pts.slice(0, 4);
      if (pts.length !== 4 || !(g.closed || pts.length === 4)) continue;
      const xs_ = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
      const x0 = Math.min(...xs_), x1 = Math.max(...xs_), y0 = Math.min(...ys), y1 = Math.max(...ys);
      if (!pts.every((p) => (same(p[0], x0) || same(p[0], x1)) && (same(p[1], y0) || same(p[1], y1)))) continue;
      if (!g.closed) continue;
      out.push({ x0, y0, x1, y1, w: x1 - x0, h: y1 - y0, layer: lay(g), handles: [g.handle] });
    }
    const lines = es.filter((g) => g.type === "LINE");
    const H = lines.filter((g) => same(g.start[1], g.end[1]) && !same(g.start[0], g.end[0])).map((g) => ({ g, y: g.start[1], a: Math.min(g.start[0], g.end[0]), b: Math.max(g.start[0], g.end[0]) }));
    const V = lines.filter((g) => same(g.start[0], g.end[0]) && !same(g.start[1], g.end[1])).map((g) => ({ g, x: g.start[0], a: Math.min(g.start[1], g.end[1]), b: Math.max(g.start[1], g.end[1]) }));
    for (let i = 0; i < H.length; i++)
      for (let j = i + 1; j < H.length; j++) {
        const p = H[i], q = H[j];
        if (!same(p.a, q.a, t) || !same(p.b, q.b, t) || same(p.y, q.y, t)) continue;
        const y0 = Math.min(p.y, q.y), y1 = Math.max(p.y, q.y);
        const side = (x) => V.find((v) => same(v.x, x, t) && same(v.a, y0, t) && same(v.b, y1, t));
        const l = side(p.a), r = side(p.b);
        if (l && r) out.push({ x0: p.a, y0, x1: p.b, y1, w: p.b - p.a, h: y1 - y0, layer: lay(p.g), handles: [p.g.handle, q.g.handle, l.g.handle, r.g.handle] });
      }
    return out;
  }
  const sizeIs = (r, w, h) => same(r.w, w) && same(r.h, h);

  // ---------------------------------------------------------------- exercises
  // A step's `task` asks the learner to draw something in BlackCAD, then reads
  // the drawing to check it. `setup` puts down the starting shapes and
  // remembers where; `show` draws a finished answer beside the work.
  const tasks = {};
  const taskKey = () => `${LESSON.id}:${step}`;
  const tstate = () => (tasks[taskKey()] ||= {});
  let printing = false;
  /** Where the exercise's starting shapes are; a printed sheet assumes 0,0. */
  const T0 = () => tstate().o || (printing ? [0, 0] : null);
  function runSetup(st) {
    const t = tstate();
    const o = st.task.setup.run();
    // what's there now is the starting point; anything newer is the learner's
    if (o) Object.assign(t, { o, before: new Set(doc.disp.byHandle.keys()), res: null });
  }
  function runTaskCheck(st) {
    const t = tstate();
    if (!doc) t.res = [{ ok: false, t: "Open or start a drawing first." }];
    else
      try {
        t.res = st.task.check(t);
      } catch (e) {
        t.res = [{ ok: false, t: `Couldn't read the drawing: ${esc(e.message || String(e))}` }];
      }
    if (t.res.every((r) => r.ok)) log(`Blackboard: ${LESSON.title}, exercise checked: all correct.`, "ok");
  }
  function taskHtml(st) {
    if (!st.task) return "";
    const t = tstate();
    const k = st.task;
    const steps = k.do().map((x) => `<li>${x}</li>`).join("");
    const btns =
      (k.setup ? `<button class="chalkbtn try" data-act="task-setup">${k.setup.label}</button>` : "") +
      `<button class="chalkbtn" data-act="task-check">Check my work</button>` +
      (k.show ? `<button class="chalkbtn" data-act="task-show">Show me</button>` : "");
    const res = t.res ? `<div class="res">${t.res.map((r) => `<div class="${r.ok ? "ok" : "bad"}">${r.ok ? "✓" : "✗"} ${r.t}</div>`).join("")}${t.res.every((r) => r.ok) ? `<div class="ok">Well drawn.</div>` : ""}</div>` : "";
    return `<div class="task"><b>Your turn${du() === "in" ? " · inches" : " · millimetres"}</b><ol>${steps}</ol><div class="bb-row">${btns}</div>${res}</div>`;
  }
  /** The new objects since the exercise was set up, or all of them. */
  const fresh = (t, es) => (t.before ? es.filter((g) => !t.before.has(g.handle)) : es);
  const at = (o, x, y) => `${xs(o[0] + x)},${xs(o[1] + y)}`;
  const needSetup = (label) => `Press ${kbd(label)} first; then the numbers to type appear here.`;

  // ================================================================ LESSON: THE COMMAND LINE

  function cmdFig1() {
    const m = COMMANDS.filter((c) => !c.hidden && (c.name.startsWith("C") || c.aliases.some((a) => a.startsWith("C")))).slice(0, 5);
    let g = path(rectD(50, 60, 500, 70)) + path(rectD(50, 140, 500, 30 + m.length * 34), "b");
    label(70, 104, "Command:", "b", 24);
    label(190, 104, "C", "a", 26);
    g += path("M 210 82 v 30", "a");
    m.forEach((c, i) => {
      label(72, 176 + i * 34, esc(c.aliases[0] || c.name), "a", 21);
      label(150, 176 + i * 34, `${c.name} · ${esc(c.desc)}`, "w", 18);
    });
    label(50, 140 + 30 + m.length * 34 + 34, "start typing anywhere · Enter or Space runs it", "b", 19);
    return svg(g, "The command line with the letter C typed and a list of matching commands below it.", `30 40 540 ${140 + m.length * 34 + 90}`);
  }

  function cmdFig2() {
    let g = path(rectD(40, 60, 520, 64));
    label(58, 101, "CIRCLE  Specify radius [Diameter]:", "w", 21);
    label(440, 101, "D", "a", 24);
    g += arrow(372, 150, 372, 116, "a");
    label(300, 172, "options: type the capital letter", "a", 18);
    const cx = 200, cy = 280, r = 70;
    g += circle(cx, cy, r, "w") + path(`M ${cx} ${cy} L ${cx + r} ${cy}`, "b") + smark("CEN", cx, cy, "b", 7);
    label(cx + r / 2, cy - 12, "radius", "b", 18, "middle");
    label(330, 250, "Enter · done, or take the default", "w", 18);
    label(330, 280, "Esc · cancel", "w", 18);
    label(330, 310, "right-click · Enter", "w", 18);
    return svg(g, "The prompt line asking for a radius with a Diameter option, and a circle drawn below it.", "20 40 560 330");
  }

  function cmdFig3() {
    const d = xv(40, 1.5);
    const cx = 300, cy = 200, r = 90;
    let g = path(`M ${cx - 150} ${cy} H ${cx + 150} M ${cx} ${cy - 130} V ${cy + 130}`, "b") + circle(cx, cy, r, "w");
    g += dim(cx - r * 0.707, cy + r * 0.707, cx + r * 0.707, cy - r * 0.707, "a");
    label(cx + r * 0.707 + 10, cy - r * 0.707 - 6, `Ø${xs(d)}`, "a", 22);
    label(cx + 10, cy + 24, "0,0", "b", 18);
    return svg(g, "A circle centred on the origin with its diameter marked.", "120 50 360 300");
  }

  const CMD = {
    id: "d-cmd",
    title: "The command line",
    q: "How do I tell BlackCAD what to draw?",
    blurb: "Typing commands, prompts, options, undo",
    src: "BlackCAD's own command line and command list.",
    steps: [
      {
        t: "Type a command, from anywhere",
        p: "You don't have to click into the command line first. Start typing and the letters go there, and matching commands show as you type. Enter or Space runs the one you want. Every tool button does the same as typing its name.",
        ctl: [],
        fig: cmdFig1,
        math: () => ["LINE", "PLINE", "CIRCLE", "ARC", "RECTANGLE", "TEXT", "DIM", "MOVE", "COPY", "ROTATE", "MIRROR", "ERASE", "DIST"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c}`)).join(""),
        basis: "The short names are the aliases BlackCAD knows today. Type the full name if you forget one.",
      },
      {
        t: "One question at a time",
        p: "A running command asks one thing at a time on the prompt line. Words in square brackets are options: type the capital letter to pick one. A value in angle brackets is the default, and Enter takes it. Esc cancels.",
        ctl: [],
        fig: cmdFig2,
        math: () => ln(`CIRCLE asks for a radius; type ${V_("D")} to give a diameter instead`) + ln("Space works like Enter, except while you type text") + ln("Enter on an empty command line repeats the last command", "dim"),
      },
      {
        t: "Your first part: a circle",
        p: "Draw a circle by typing every value, then let the board check it. If anything goes wrong, Undo (Ctrl+Z or U) takes back the whole command in one step, and Redo (Ctrl+Y) puts it back.",
        ctl: [],
        fig: cmdFig3,
        math: () => ln(`centre ${V_("0,0")} · diameter ${V_(xs(xv(40, 1.5)))}`) + ln("Start a Metric or Imperial drawing first, if none is open.", "dim"),
        task: {
          do: () => [`Type ${kbd("C")} and press Enter.`, `For the centre, type ${kbd("0,0")} and Enter.`, `Type ${kbd("D")} and Enter, then ${kbd(xs(xv(40, 1.5)))} and Enter.`],
          check: () => {
            const d = xv(40, 1.5);
            const cs = entities().filter((g) => g.type === "CIRCLE");
            if (!cs.length) return [{ ok: false, t: "No circle in the drawing yet." }];
            const good = cs.find((g) => samePt(g.center, [0, 0]) && same(2 * g.radius, d));
            if (good) return [{ ok: true, t: `Circle at 0,0` }, { ok: true, t: `Diameter ${xf(d)}` }];
            const atO = cs.find((g) => samePt(g.center, [0, 0]));
            if (atO) return [{ ok: true, t: "Circle at 0,0" }, { ok: false, t: `Diameter is ${xf(2 * atO.radius)}; it should be ${xf(d)}. Select it and type ${xs(d)} as its Diameter in Properties.` }];
            const right = cs.find((g) => same(2 * g.radius, d));
            if (right) return [{ ok: false, t: `The centre is at ${xs(right.center[0].toFixed(3))},${xs(right.center[1].toFixed(3))}; type 0,0 for it.` }, { ok: true, t: `Diameter ${xf(d)}` }];
            return [{ ok: false, t: `No circle of diameter ${xf(d)} at 0,0 yet.` }];
          },
          show: () => placeOnDrawing("circle", (d) => d.circle(0, 0, xv(40, 1.5)), { at: [0, 0], raw: true }),
        },
      },
    ],
  };

  // ================================================================ LESSON: POINTS BY THE NUMBERS

  const GX = 300, GY = 220, GK = 1.9;
  const gx = (x) => GX + x * GK, gy = (y) => GY - y * GK;
  function axes() {
    let g = `<path class="dsh c-b" style="--d:0s;opacity:.3;stroke-dasharray:2 7" d="${[-4, -3, -2, -1, 1, 2, 3, 4, 5].map((i) => `M ${f1(gx(i * 20))} ${f1(gy(-90))} V ${f1(gy(95))}`).join(" ")} ${[-4, -3, -2, -1, 1, 2, 3, 4].map((i) => `M ${f1(gx(-120))} ${f1(gy(i * 20))} H ${f1(gx(125))}`).join(" ")}"/>`;
    g += arrow(gx(-125), GY, gx(130), GY, "w") + arrow(GX, gy(-92), GX, gy(100), "w");
    label(gx(130) + 6, GY + 22, "X", "w", 20);
    label(GX + 10, gy(100) + 10, "Y", "w", 20);
    label(GX - 8, GY + 22, "0,0", "b", 17, "end");
    return g;
  }
  function coordFig1() {
    const P = [gx(s.px), gy(s.py)];
    let g = axes() + dash(`M ${f1(P[0])} ${GY} V ${f1(P[1])} M ${GX} ${f1(P[1])} H ${f1(P[0])}`, "b");
    g += circle(P[0], P[1], 6, "a");
    label(P[0] + (s.px >= 0 ? 12 : -12), P[1] + (s.py >= 0 ? -12 : 26), `${nt(s.px)},${nt(s.py)}`, "a", 24, s.px >= 0 ? "start" : "end");
    label(P[0], GY + (s.py >= 0 ? 22 : -10), `x ${nt(s.px)}`, "b", 17, "middle");
    label(GX + (s.px >= 0 ? -8 : 8), P[1] + 6, `y ${nt(s.py)}`, "b", 17, s.px >= 0 ? "end" : "start");
    return svg(g, "X and Y axes with a point marked; dashed lines show its X and Y distances from the origin.", "40 20 520 400");
  }
  const A_REL = [-60, -40];
  function coordFig2() {
    const A = [gx(A_REL[0]), gy(A_REL[1])], B = [gx(A_REL[0] + s.pdx), gy(A_REL[1] + s.pdy)];
    let g = axes() + dash(`M ${f1(A[0])} ${f1(A[1])} H ${f1(B[0])} V ${f1(B[1])}`, "b");
    g += circle(A[0], A[1], 6, "w") + arrow(A[0], A[1], B[0], B[1], "a") + circle(B[0], B[1], 5, "a");
    label(A[0] - 10, A[1] + (s.pdy >= 0 ? -12 : 26), "last point", "w", 18, "end");
    label(B[0] + (s.pdx >= 0 ? 12 : -12), B[1] + (s.pdy >= 0 ? -10 : 26), `@${nt(s.pdx)},${nt(s.pdy)}`, "a", 24, s.pdx >= 0 ? "start" : "end");
    label((A[0] + B[0]) / 2, A[1] + (s.pdy >= 0 ? 24 : -12), `Δx ${nt(s.pdx)}`, "b", 17, "middle");
    label(B[0] + (s.pdx >= 0 ? 10 : -10), (A[1] + B[1]) / 2 + 6, `Δy ${nt(s.pdy)}`, "b", 17, s.pdx >= 0 ? "start" : "end");
    return svg(g, "From the last point, an arrow to a new point; dashed legs show the distance across and up.", "40 20 520 400");
  }
  function coordFig3() {
    const a = rad(s.pa);
    const B = [GX + Math.cos(a) * s.pl * GK, GY - Math.sin(a) * s.pl * GK];
    let g = axes();
    const r = 46, large = s.pa % 360 > 180 ? 1 : 0;
    if (s.pa % 360 > 0.5) g += path(`M ${GX + r} ${GY} A ${r} ${r} 0 ${large} 0 ${f1(GX + r * Math.cos(a))} ${f1(GY - r * Math.sin(a))}`, "b");
    g += arrow(GX, GY, B[0], B[1], "a") + circle(B[0], B[1], 5, "a");
    label(GX + (r + 16) * Math.cos(a / 2), GY - (r + 16) * Math.sin(a / 2) + 6, `${Math.round(s.pa)}°`, "b", 19, "middle");
    label(B[0] + (Math.cos(a) >= 0 ? 12 : -12), B[1] + (Math.sin(a) >= 0 ? -10 : 26), `@${nt(s.pl)}&lt;${Math.round(s.pa)}`, "a", 24, Math.cos(a) >= 0 ? "start" : "end");
    label(gx(118), GY - 8, "0°", "b", 16, "end");
    label(GX - 10, gy(88), "90°", "b", 16, "end");
    label(gx(-118), GY - 8, "180°", "b", 16);
    label(GX + 8, gy(-86), "270°", "b", 16);
    return svg(g, "From the last point, an arrow at an angle measured counter-clockwise from the X direction.", "40 20 520 400");
  }
  function unitsFig() {
    const x0 = 60, k = 4.6;
    let g = path(`M ${x0} 120 H ${x0 + 100 * k}`, "w");
    for (let i = 0; i <= 100; i += 5) g += path(`M ${f1(x0 + i * k)} 120 v ${i % 10 ? -10 : -20}`, "w");
    for (let i = 0; i <= 100; i += 20) label(x0 + i * k, 90, String(i), "w", 17, "middle");
    label(x0 + 100 * k + 12, 126, "mm", "w", 18);
    g += path(`M ${x0} 190 H ${f1(x0 + 101.6 * k)}`, "a");
    for (let i = 0; i <= 16; i++) g += path(`M ${f1(x0 + i * 6.35 * k)} 190 v ${i % 4 ? 10 : 22}`, "a");
    for (let i = 0; i <= 4; i++) label(x0 + i * 25.4 * k, 236, String(i), "a", 17, "middle");
    label(x0 + 101.6 * k + 12, 196, "in", "a", 18);
    g += dash(`M ${x0 + 38.1 * k} 120 V 190`, "g");
    label(x0 + 38.1 * k + 8, 160, `1-1/2" = 38.1 mm`, "g", 18);
    label(x0, 300, "type the unit and BlackCAD converts:", "b", 19);
    label(x0, 330, `1"   1-1/2"   3/8in   1'2"   25.4mm   2.5cm`, "w", 19);
    return svg(g, "A millimetre ruler above an inch ruler, with one and a half inches marked at 38.1 millimetres.", "40 60 530 290");
  }
  function plateTypedFig() {
    const W = xv(100, 4), H = xv(50, 2);
    const x0 = 130, y0 = 290, k = 340 / W;
    const pts = [[x0, y0], [x0 + W * k, y0], [x0 + W * k, y0 - H * k], [x0, y0 - H * k]];
    let g = path(poly(pts));
    const tags = ["0,0", `@${xs(W)},0`, `@0,${xs(H)}`, `@-${xs(W)},0`];
    tags.forEach((t, i) => (g += circle(pts[(i + 0) % 4][0], pts[i % 4][1], 4, "a")));
    label(x0 - 10, y0 + 26, tags[0], "a", 20, "end");
    label(x0 + W * k + 10, y0 + 26, tags[1], "a", 20);
    label(x0 + W * k + 10, y0 - H * k - 10, tags[2], "a", 20);
    label(x0 - 10, y0 - H * k - 10, tags[3], "a", 20, "end");
    label(x0 + 10, y0 - (H * k) / 2 + 6, "C closes it", "b", 18);
    return svg(g, "A rectangle with the typed coordinates written at each corner, starting at 0,0.", "30 60 560 280");
  }

  const COORDS = {
    id: "d-coords",
    title: "Points by the numbers",
    q: "How do I draw something exactly 100 long?",
    blurb: "Absolute, relative and polar points, in any unit",
    src: "BlackCAD's point input: x,y, @dx,dy, @length<angle, <angle and a length along the cursor; lengths parsed by the core with units.",
    steps: [
      {
        t: "x,y: measured from the origin",
        p: "Every point has an X and a Y, measured from 0,0. Type them with a comma and no space, such as 30,20. X goes right and Y goes up; negative values go left and down. The status bar shows the cursor's X and Y as you move.",
        ctl: ["px", "py"],
        fig: coordFig1,
        math: () => ln(`type ${V_(`${nt(s.px)},${nt(s.py)}`)}`) + ln("Use it for the first point of a part, often 0,0.", "dim"),
      },
      {
        t: "@dx,dy: measured from the last point",
        p: "Usually you know how far to go, not where you'll end up. Put @ in front and the two numbers are measured from the last point instead of from 0,0: across, then up.",
        ctl: ["pdx", "pdy"],
        fig: coordFig2,
        math: () => ln(`type ${V_(`@${nt(s.pdx)},${nt(s.pdy)}`)}`) + ln(`from ${nt(A_REL[0])},${nt(A_REL[1])} that lands on ${nt(A_REL[0] + s.pdx)},${nt(A_REL[1] + s.pdy)}`, "dim"),
      },
      {
        t: "@length&lt;angle: a distance in a direction",
        p: "For a slanted line, give a length and an angle with < between them. Angles start at 0°, pointing right along X, and turn counter-clockwise: 90° is up, 180° left and 270° down.",
        ctl: ["pl", "pa"],
        fig: coordFig3,
        math: () => {
          const a = rad(s.pa);
          return ln(`type ${V_(`@${nt(s.pl)}&lt;${Math.round(s.pa)}`)}`) + ln(`the same as @${nt(s.pl * Math.cos(a))},${nt(s.pl * Math.sin(a))}`, "dim");
        },
        note: "A length on its own goes that far toward the cursor. Typing <45 at a prompt locks the next pick to 45°.",
      },
      {
        t: "Type in any unit",
        p: "A plain number is in the drawing's own units. Add a unit and BlackCAD converts it, so inches and fractions work in a metric drawing and millimetres work in an inch drawing. A space ends what you type, so write 1-1/2\", not 1 1/2\".",
        ctl: [],
        fig: unitsFig,
        math: () => ln(`${V_('1"')} = 25.4 mm · ${V_('1-1/2"')} = 38.1 mm · ${V_("3/8in")} = 9.525 mm`) + ln(`${V_(`1'2"`)} = 355.6 mm · ${V_("25.4mm")} = 1 in · ${V_("2.5cm")} = 25 mm`) + ln("Angles are in degrees unless you add r (radians) or g (grads).", "dim"),
      },
      {
        t: "Your turn: a plate by the numbers",
        p: "Draw a plate with LINE, typing the first corner as a point and every other corner from the last one. Close it with the C option. Then check it.",
        ctl: [],
        fig: plateTypedFig,
        math: () => ln(`${V_(xs(xv(100, 4)))} × ${V_(xs(xv(50, 2)))}, lower-left corner at 0,0`),
        task: {
          do: () => {
            const W = xs(xv(100, 4)), H = xs(xv(50, 2));
            return [`Type ${kbd("L")} and Enter.`, `First point: ${kbd("0,0")}.`, `Then ${kbd(`@${W},0`)}, ${kbd(`@0,${H}`)} and ${kbd(`@-${W},0`)}, each followed by Enter.`, `Type ${kbd("C")} and Enter to close.`];
          },
          check: () => {
            const W = xv(100, 4), H = xv(50, 2);
            const rs = rectsOf(entities());
            const good = rs.find((r) => sizeIs(r, W, H) && same(r.x0, 0) && same(r.y0, 0));
            if (good) return [{ ok: true, t: `${xf(W)} × ${xf(H)} plate` }, { ok: true, t: "Lower-left corner at 0,0" }];
            const sz = rs.find((r) => sizeIs(r, W, H));
            if (sz) return [{ ok: true, t: `${xf(W)} × ${xf(H)} plate` }, { ok: false, t: `Its lower-left corner is at ${xs(sz.x0.toFixed(3))},${xs(sz.y0.toFixed(3))}; start at 0,0.` }];
            const lines = entities().filter((g) => g.type === "LINE").length;
            return [{ ok: false, t: lines ? `${lines} line${lines === 1 ? "" : "s"} found, but they don't make a closed ${xf(W)} × ${xf(H)} rectangle yet.` : "No lines in the drawing yet." }];
          },
          show: () =>
            placeOnDrawing(
              "plate",
              (d) => {
                const W = xv(100, 4), H = xv(50, 2);
                d.line(0, 0, W, 0), d.line(W, 0, W, H), d.line(W, H, 0, H), d.line(0, H, 0, 0);
              },
              { at: [0, 0], raw: true }
            ),
        },
      },
    ],
  };

  // ================================================================ LESSON: SNAPS, ORTHO AND POLAR

  function snapFig1() {
    const a = [90, 290], b = [520, 110], c = [430, 215], r = 72;
    let g = path(`M ${a[0]} ${a[1]} L ${b[0]} ${b[1]}`) + circle(c[0], c[1], r);
    // where the line crosses the circle
    const d = [b[0] - a[0], b[1] - a[1]], f = [a[0] - c[0], a[1] - c[1]];
    const A = d[0] * d[0] + d[1] * d[1], B = 2 * (f[0] * d[0] + f[1] * d[1]), C = f[0] * f[0] + f[1] * f[1] - r * r;
    const disc = Math.sqrt(Math.max(B * B - 4 * A * C, 0));
    const ints = [(-B - disc) / (2 * A), (-B + disc) / (2 * A)].map((t) => [a[0] + d[0] * t, a[1] + d[1] * t]);
    g += smark("END", a[0], a[1]) + smark("END", b[0], b[1]) + smark("MID", (a[0] + b[0]) / 2, (a[1] + b[1]) / 2 - 1);
    g += smark("CEN", c[0], c[1]) + smark("QUA", c[0], c[1] + r) + smark("QUA", c[0] + r, c[1]);
    for (const p of ints) g += smark("INT", p[0], p[1]);
    label(a[0] + 4, a[1] + 34, "END", "a", 18, "middle");
    label((a[0] + b[0]) / 2 + 14, (a[1] + b[1]) / 2 + 26, "MID", "a", 18);
    label(c[0] + 16, c[1] + 6, "CEN", "a", 18);
    label(c[0], c[1] + r + 28, "QUA", "a", 18, "middle");
    label(ints[0][0] - 4, ints[0][1] - 18, "INT", "a", 18, "middle");
    return svg(g, "A line crossing a circle, with snap markers at the ends, the midpoint, the centre, two quadrants and the two intersections.", "50 70 520 320");
  }
  function snapFig2() {
    const c = [380, 210], r = 80, from = [100, 300];
    let g = path(rectD(40, 50, 520, 50));
    label(56, 84, "LINE  Specify next point:", "w", 20);
    label(330, 84, "CEN", "a", 22);
    g += circle(c[0], c[1], r) + circle(from[0], from[1], 5, "w");
    g += path(`M ${from[0]} ${from[1]} L ${c[0]} ${c[1]}`, "a") + smark("CEN", c[0], c[1]);
    g += path(`M ${c[0] + r * 0.7 + 6} ${c[1] + r * 0.7 + 6} l 16 18 l -4 -12 l 12 2 Z`, "w");
    label(c[0] + r * 0.7 + 30, c[1] + r * 0.7 + 30, "cursor anywhere on the circle", "w", 17);
    label(60, 360, "only CEN for this pick; Tab cycles when snaps overlap", "b", 18);
    return svg(g, "The prompt with CEN typed, and a line ending exactly at a circle's centre while the cursor is on the circle's edge.", "30 30 540 350");
  }
  function polarFig() {
    const o = [300, 220], R = 150;
    let g = "";
    for (let a = 0; a < 360; a += 15) g += `<path class="dsh c-b" style="--d:0s;opacity:${a % 90 ? 0.28 : 0.6};stroke-dasharray:3 7" d="M ${o[0]} ${o[1]} L ${f1(o[0] + R * Math.cos(rad(a)))} ${f1(o[1] - R * Math.sin(rad(a)))}"/>`;
    const tracked = (Math.round(s.pa / 15) * 15) % 360;
    const cur = [o[0] + 130 * Math.cos(rad(s.pa)), o[1] - 130 * Math.sin(rad(s.pa))];
    const tp = [o[0] + 130 * Math.cos(rad(tracked)) * Math.cos(rad(s.pa - tracked)), o[1] - 130 * Math.sin(rad(tracked)) * Math.cos(rad(s.pa - tracked))];
    g += path(`M ${o[0]} ${o[1]} L ${f1(tp[0])} ${f1(tp[1])}`, "a", "thick") + circle(o[0], o[1], 5, "w");
    g += path(`M ${f1(cur[0])} ${f1(cur[1])} l 14 16 l -4 -11 l 11 2 Z`, "w");
    label(tp[0] + (Math.cos(rad(tracked)) >= 0 ? 14 : -14), tp[1] - 12, `Polar ${tracked}°`, "a", 20, Math.cos(rad(tracked)) >= 0 ? "start" : "end");
    label(60, 405, "Polar tracks every 15° · Ortho locks to 0, 90, 180, 270", "b", 18);
    return svg(g, "Faint rays every 15 degrees around a point, with the line locked onto the ray nearest the cursor.", "40 50 520 370");
  }
  function snapTaskFig() {
    const x0 = 80, y0 = 300, W = 220, H = 110, c = [440, y0 - H / 2], r = 46;
    let g = path(rectD(x0, y0 - H, W, H)) + circle(c[0], c[1], r);
    const m = [x0 + W / 2, y0 - H];
    g += path(`M ${m[0]} ${m[1]} L ${c[0]} ${c[1]}`, "a") + smark("MID", m[0], m[1] - 1) + smark("CEN", c[0], c[1]);
    label(m[0], m[1] - 20, "MID", "a", 18, "middle");
    label(c[0] + 14, c[1] - 14, "CEN", "a", 18);
    return svg(g, "A rectangle and a circle joined by a line from the midpoint of the rectangle's top edge to the circle's centre.", "50 120 500 220");
  }
  const snapSetupVals = () => ({ W: xv(80, 3), H: xv(40, 1.5), cx: xv(130, 5), d: xv(20, 0.75) });

  const SNAPS = {
    id: "d-snaps",
    title: "Snaps, Ortho and Polar",
    q: "How do I land exactly on the end of a line?",
    blurb: "Object snaps, one-shot snaps, Tab, Ortho and Polar",
    src: "BlackCAD's snap kinds (END, MID, CEN, QUA, INT, NEA, NOD, INS), drawing aids F3, F7, F8, F9, F10 and 15° polar tracking.",
    steps: [
      {
        t: "Object snaps find exact points",
        p: "With OSNAP on (F3, and on by default), the cursor jumps to exact points near it and a marker shows which kind. A snapped point is exact however far you're zoomed out. In a good drawing, almost every point is snapped or typed.",
        ctl: [],
        fig: snapFig1,
        math: () => ln(`${V_("□")} endpoint · ${V_("△")} midpoint · ${V_("⊕")} centre`) + ln(`${V_("◇")} quadrant · ${V_("✕")} intersection · ${V_("⊗")} node`) + ln("Nearest is off unless you ask for it.", "dim"),
      },
      {
        t: "Ask for one snap, or press Tab",
        p: "At any point prompt, type END, MID, CEN, QUA, INT, NEA or NOD and press Enter, and the next pick uses only that snap. Where several snaps sit close together, Tab steps through them. NON switches snaps off for one pick.",
        ctl: [],
        fig: snapFig2,
        math: () => ln(`${V_("CEN")} then click the circle's edge: the line ends at its centre`) + ln("The command line notes the one-shot snap until you pick.", "dim"),
      },
      {
        t: "Ortho and Polar keep lines straight",
        p: "Polar (F10, on by default) tracks every 15°, so lines lock to 0°, 15°, 30° and so on as you near them. Ortho (F8) locks to straight across or straight up and down only. Turning one on turns the other off. Grid (F7) shows the grid and Snap (F9) steps the cursor along it.",
        ctl: ["pa"],
        fig: polarFig,
        math: () => {
          const tr = (Math.round(s.pa / 15) * 15) % 360, or = (Math.round(s.pa / 90) * 90) % 360;
          return ln(`cursor at ${Math.round(s.pa)}° → Polar locks to ${V_(`${tr}°`)}`) + ln(`with Ortho instead: ${V_(`${or}°`)}`) + ln("Typed points ignore both: @50&lt;37 goes to exactly 37°.", "dim");
        },
      },
      {
        t: "Your turn: midpoint to centre",
        p: "Join the middle of a plate's top edge to the centre of a hole beside it, using one-shot snaps. The board puts the plate and hole down for you.",
        ctl: [],
        fig: snapTaskFig,
        math: () => ln("Snap both ends; don't eyeball them."),
        task: {
          setup: {
            label: "Set up the plate and hole",
            run: () => {
              const v = snapSetupVals();
              return placeOnDrawing("plate and hole for the snap exercise", (d) => {
                d.rect(0, 0, v.W, v.H);
                d.circle(v.cx, v.H / 2, v.d);
              }, { raw: true });
            },
          },
          do: () => [`Type ${kbd("L")} and Enter.`, `Type ${kbd("MID")}, Enter, and click near the top edge of the plate.`, `Type ${kbd("CEN")}, Enter, and click on the circle.`, `Press Enter to finish the line.`],
          check: (t) => {
            const es = entities();
            const rs = rectsOf(es), cs = es.filter((g) => g.type === "CIRCLE");
            const lines = fresh(t, es).filter((g) => g.type === "LINE");
            if (!rs.length || !cs.length) return [{ ok: false, t: "Set up the plate and hole first, or draw a rectangle and a circle." }];
            const mids = rs.map((r) => [(r.x0 + r.x1) / 2, r.y1]);
            for (const g of lines) {
              const ends = [g.start, g.end];
              const m = mids.some((p) => ends.some((e) => samePt(e, p)));
              const c = cs.some((cc) => ends.some((e) => samePt(e, cc.center)));
              if (m && c) return [{ ok: true, t: "Starts exactly at the top edge's midpoint" }, { ok: true, t: "Ends exactly at the circle's centre" }];
            }
            const someMid = lines.some((g) => mids.some((p) => samePt(g.start, p) || samePt(g.end, p)));
            const someCen = lines.some((g) => cs.some((cc) => samePt(g.start, cc.center) || samePt(g.end, cc.center)));
            return [
              { ok: someMid, t: someMid ? "A line starts at the top edge's midpoint" : "No line starts exactly at the top edge's midpoint. Type MID before the pick." },
              { ok: someCen, t: someCen ? "A line ends at the circle's centre" : "No line ends exactly at the circle's centre. Type CEN before the pick." },
            ];
          },
          show: () => {
            const v = snapSetupVals();
            placeOnDrawing("snap exercise answer", (d) => {
              d.rect(0, 0, v.W, v.H);
              d.circle(v.cx, v.H / 2, v.d);
              d.line(v.W / 2, v.H, v.cx, v.H / 2);
            }, { raw: true });
          },
        },
      },
    ],
  };

  // ================================================================ LESSON: SELECT, GRIP AND CHANGE

  function selectFig() {
    let g = "";
    // window, left to right
    g += circle(120, 200, 40, "b", "thick") + path("M 150 140 L 250 260", "w");
    g += path(rectD(70, 140, 140, 120), "b");
    g += arrow(70, 290, 210, 290, "b");
    label(140, 120, "window", "b", 20, "middle");
    label(70, 324, "left → right: only what's inside", "b", 17);
    // crossing, right to left
    g += circle(390, 200, 40, "g", "thick") + path("M 420 140 L 520 260", "g", "thick");
    g += dash(rectD(340, 160, 130, 90), "g");
    g += arrow(470, 290, 340, 290, "g");
    label(405, 120, "crossing", "g", 20, "middle");
    label(330, 324, "right → left: anything it touches", "g", 17);
    return svg(g, "Two selection boxes. A solid window dragged left to right selects only the circle fully inside it. A dashed crossing box dragged right to left selects the circle and the line it touches.", "40 90 520 260");
  }
  function gripFig() {
    let g = path("M 80 250 L 260 130", "b") + dash("M 80 250 L 300 190", "a");
    const sq = (x, y) => path(`M ${x - 7} ${y - 7} h 14 v 14 h -14 Z`, "b");
    g += sq(80, 250) + sq(260, 130) + sq(170, 190);
    g += arrow(266, 140, 296, 182, "a");
    label(100, 300, "drag an end grip: stretch", "a", 18);
    g += circle(430, 200, 60, "b") + `<circle class="dsh c-a" style="--d:.4s" cx="430" cy="200" r="85"/>`;
    g += sq(430, 200) + sq(490, 200) + sq(430, 140) + sq(370, 200) + sq(430, 260);
    g += arrow(497, 200, 512, 200, "a");
    label(360, 316, "drag an edge grip: resize", "a", 18);
    return svg(g, "A selected line with grips at its ends and middle, one end dragged to a new place; a selected circle with grips, its edge grip dragged to make it bigger.", "50 100 510 240");
  }
  function propsFig() {
    let g = path(rectD(120, 60, 360, 260), "w");
    label(140, 96, "PROPERTIES", "b", 17);
    label(140, 128, "Circle", "w", 22);
    const rows = [["Layer", "VISIBLE"], ["Center X", "50.00"], ["Center Y", "30.00"], ["Radius", "10.00"], ["Diameter", null]];
    rows.forEach(([k, v], i) => {
      label(140, 170 + i * 30, k, "b", 18);
      if (v) label(460, 170 + i * 30, v, "w", 18, "end");
    });
    g += path(rectD(330, 272, 136, 30), "a");
    label(460, 294, '3/4"', "a", 20, "end");
    g += arrow(520, 287, 472, 287, "a");
    label(526, 293, "type, Enter", "a", 17);
    return svg(g, "The Properties panel for a circle, with a new diameter being typed into the Diameter row.", "100 40 520 300");
  }
  function mirrorFig() {
    const c = 300;
    const L = [[150, 300], [270, 300], [270, 270], [190, 270], [190, 140], [150, 140]];
    let g = path(poly(L)) + dash(poly(L.map(([x, y]) => [2 * c - x, y])), "a");
    g += lt(`M ${c} 110 V 330`, "y", "center", 2.2);
    g += smark("END", c, 110, "b", 7) + smark("END", c, 330, "b", 7);
    label(c + 12, 106, "mirror line: END, END", "b", 17);
    label(150, 356, "original stays", "w", 18);
    label(330, 356, "mirrored copy", "a", 18);
    return svg(g, "An L-shaped bracket on the left of a yellow centre line and its mirrored copy on the right.", "100 80 420 300");
  }
  function moreToolsFig() {
    const tools = [["OFFSET", "parallel copy"], ["TRIM", "cut back"], ["FILLET", "round a corner"], ["ARRAY", "rows of holes"], ["SCALE", "resize"]];
    let g = "";
    tools.forEach(([c, what], i) => {
      const y = 90 + i * 56;
      g += path(rectD(80, y - 30, 420, 44), "g");
      label(100, y, c, "g", 21);
      label(240, y, what, "w", 18);
      label(486, y, aliasOf(c), "g", 19, "end");
    });
    return svg(g, "A list of five drafting tools, each with the letters that start it.", "60 40 460 330");
  }
  const MIRROR_PTS = () => {
    const u = xv(1, 1 / 25);
    return [[0, 0], [30, 0], [30, 8], [10, 8], [10, 40], [0, 40]].map(([x, y]) => [x * u, y * u]);
  };
  const MIRROR_C = () => xv(45, 1.8);

  const EDIT = {
    id: "d-edit",
    title: "Select, grip and change",
    q: "How do I change what I've already drawn?",
    blurb: "Selection boxes, grips, Properties and the edit commands",
    src: "BlackCAD's selection (window and crossing), grips, Properties panel and MOVE, COPY, ROTATE, MIRROR, ERASE.",
    steps: [
      {
        t: "Click, window and crossing",
        p: "Click an object to select it. Drag a box from left to right to take only what's fully inside (a window), or from right to left to take anything the box touches (a crossing). Shift-click removes one from the selection. Esc clears it, and Ctrl+A takes everything that isn't locked.",
        ctl: [],
        fig: selectFig,
        math: () => ln("Selected objects show in the selection colour, with grips.") + ln("Delete erases the selection.", "dim"),
      },
      {
        t: "Grips: drag to change",
        p: "Selected objects show small squares called grips. Drag an end grip to stretch a line, a circle's edge grip to resize it, and the middle or centre grip to move it. Snaps work while you drag. Click a grip without dragging and it stays live, so you can type a point such as @20,0.",
        ctl: [],
        fig: gripFig,
        math: () => ln("Each grip drag is one Undo step."),
      },
      {
        t: "Type the exact value",
        p: "The Properties panel (Ctrl+1) lists the selected object's numbers. Click one, type a new value in any unit, and press Enter. A line's length, a circle's diameter and a text's height all change this way. On a phone, tap the value for a number pad.",
        ctl: [],
        fig: propsFig,
        math: () => ln(`type ${V_('3/4"')} in a metric drawing and it becomes ${V_("19.05 mm")}`) + ln("Layer is changed here too: click it and pick one.", "dim"),
      },
      {
        t: "Move, Copy, Rotate and Mirror",
        p: "These take the selection, or ask you to pick objects and press Enter. Then they ask for a base point and where it goes. Snap the base point to the part, and type the second point as @distance for exact moves. Mirror asks whether to keep the original.",
        ctl: [],
        fig: mirrorFig,
        math: () => ["MOVE", "COPY", "ROTATE", "MIRROR", "ERASE"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c}`)).join(""),
        task: {
          setup: {
            label: "Set up the bracket",
            run: () =>
              placeOnDrawing("bracket and centre line for the mirror exercise", (d) => {
                d.pline(MIRROR_PTS());
                d.layer("CENTER", 2, () => d.line(MIRROR_C(), xv(-5, -0.2), MIRROR_C(), xv(45, 1.8)));
              }, { raw: true }),
          },
          do: () => [`Select the bracket by clicking it.`, `Type ${kbd("MI")} and Enter.`, `For the mirror line, snap to both ends of the yellow centre line.`, `Answer ${kbd("N")} to keep the original.`],
          check: (t) => {
            const es = entities();
            const cl = es.filter((g) => g.type === "LINE" && lay(g) === "CENTER" && same(g.start[0], g.end[0]));
            const pls = es.filter((g) => g.type === "LWPOLYLINE" && g.points?.length === 6);
            if (!cl.length || !pls.length) return [{ ok: false, t: "Set up the bracket first." }];
            const r3 = (v) => (Math.round(v * 1000) / 1000 + 0).toFixed(3);
            const key = (pts) => pts.map((p) => `${r3(p[0])},${r3(p[1])}`).sort().join(";");
            for (const c of cl) {
              const x = c.start[0];
              for (const a of pls)
                for (const b of pls)
                  if (a !== b && key(b.points) === key(a.points.map(([px, py]) => [2 * x - px, py]))) return [{ ok: true, t: "Mirrored copy across the centre line" }, { ok: true, t: "Original kept" }];
            }
            const fr = fresh(t, es).filter((g) => g.type === "LWPOLYLINE");
            return [{ ok: false, t: fr.length ? "There's a new bracket, but it isn't an exact mirror across the centre line. Snap both ends of the line." : "No mirrored copy yet." }];
          },
          show: () =>
            placeOnDrawing("mirror exercise answer", (d) => {
              const c = MIRROR_C();
              d.pline(MIRROR_PTS());
              d.pline(MIRROR_PTS().map(([x, y]) => [2 * c - x, y]));
              d.layer("CENTER", 2, () => d.line(c, xv(-5, -0.2), c, xv(45, 1.8)));
            }, { raw: true }),
        },
      },
      {
        t: "Shaping tools come next",
        p: "Once the outline is down, you shape it. Offset draws a parallel line, Trim and Extend cut lines back to a corner, Fillet and Chamfer round or bevel it, Array repeats holes, and Scale and Stretch resize. The next unit teaches each one, with a small drawing to try it on.",
        ctl: [],
        fig: moreToolsFig,
        math: () => ["OFFSET", "TRIM", "EXTEND", "FILLET", "CHAMFER", "ARRAY", "SCALE", "STRETCH"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join(""),
        extra: () => `<div class="bb-row"><button class="chalkbtn" data-lesson="d-modify">Offset, trim and fillet</button><button class="chalkbtn" data-lesson="d-arrays">Arrays</button><button class="chalkbtn" data-lesson="d-blocks">Blocks</button></div>`,
      },
    ],
  };

  // ================================================================ LESSON: LAYERS MEAN SOMETHING

  function layersFig() {
    const ls = layerList();
    let g = "";
    ls.slice(0, 8).forEach((l, i) => {
      const y = 70 + i * 44, c = layerChalk(l.aci);
      label(60, y + 7, esc(l.name), c, 21);
      g += lt(`M 220 ${y} H 400`, c, ltKind(l.linetype), 1.4 + (l.weight || 0.25) * 6);
      label(420, y + 6, `${esc(l.linetype || "Continuous")} · ${(l.weight || 0.25).toFixed(2)}`, "w", 15);
      if (l.current) label(220, y - 9, "CURRENT", "a", 13);
    });
    label(60, 70 + Math.min(ls.length, 8) * 44 + 10, doc ? "from your drawing" : "a new drawing starts with these", "b", 17);
    return svg(g, "Each layer's name beside a sample of its line in its colour, linetype and weight.", "40 40 540 " + (60 + Math.min(layerList().length, 8) * 44 + 20));
  }
  function currentFig() {
    let g = path(rectD(60, 60, 220, 250));
    label(76, 90, "LAYERS", "b", 16);
    ["0", "VISIBLE", "CENTER", "HIDDEN", "DIMENSION", "NOTES"].forEach((nm_, i) => {
      const y = 126 + i * 30, cur = nm_ === "NOTES";
      label(80, y, nm_, cur ? "a" : "w", 18);
      if (cur) label(190, y, "CURRENT", "a", 13);
    });
    g += arrow(300, 276, 400, 276, "c");
    label(410, 282, "TEXT", "c", 20);
    g += arrow(300, 216, 400, 216, "g");
    label(410, 222, "DIM → DIMENSION", "g", 18);
    label(410, 240, "by itself", "g", 15);
    g += arrow(300, 120, 400, 120, "w");
    label(410, 126, "LINE, CIRCLE…", "w", 18);
    label(410, 144, "→ current layer", "w", 15);
    return svg(g, "The layers panel with NOTES current. New lines, circles and text go on the current layer; dimensions go on DIMENSION by themselves.", "40 40 540 290");
  }
  function plateLayersFig({ dims = true, note = true } = {}) {
    const x0 = 100, y0 = 300, W = 300, H = 180, cx = x0 + W / 2, cy = y0 - H / 2, r = 40;
    let g = lt(rectD(x0, y0 - H, W, H), "w", "cont", 4) + lt(circD(cx, cy, r), "w", "cont", 4);
    g += lt(`M ${cx - r - 16} ${cy} H ${cx + r + 16}`, "y", "center", 1.8) + lt(`M ${cx} ${cy - r - 16} V ${cy + r + 16}`, "y", "center", 1.8);
    if (dims) {
      g += dim(x0, y0 + 34, x0 + W, y0 + 34, "g") + dim(x0 + W + 34, y0, x0 + W + 34, y0 - H, "g");
      label(x0 + W / 2, y0 + 26, xs(xv(100, 4)), "g", 18, "middle");
      label(x0 + W + 42, y0 - H / 2 + 6, xs(xv(60, 2.5)), "g", 18);
      g += path(`M ${cx + r * 0.7} ${cy - r * 0.7} l 40 -50 h 30`, "g", "");
      label(cx + r * 0.7 + 74, cy - r * 0.7 - 44, `Ø${xs(xv(20, 0.75))}`, "g", 18);
    }
    if (note) label(x0, y0 + 76, "NOTE: BREAK SHARP EDGES", "c", 17);
    return svg(g, "A plate with a hole drawn on the right layers: thick white outline, yellow centre lines, green dimensions and a cyan note.", "60 60 500 360");
  }
  function plateLayersTaskVals() {
    return { W: xv(100, 4), H: xv(60, 2.5), d: xv(20, 0.75), e: xv(5, 0.25) };
  }

  const LAYERS = {
    id: "d-layers",
    title: "Layers mean something",
    q: "What do VISIBLE, CENTER, HIDDEN and DIMENSION tell the reader?",
    blurb: "BlackCAD's starting layers, the current layer, ByLayer",
    src: "BlackCAD's default layer set (0, VISIBLE, CENTER, HIDDEN, DIMENSION, NOTES) with its colours, linetypes and weights; line conventions per ASME Y14.2 and ISO 128-2.",
    steps: [
      {
        t: "BlackCAD's starting layers",
        p: "Every new drawing starts with the layers a reader expects. Each one is a promise about what its lines mean, and it sets their colour, linetype and weight. Someone opening your file can turn the dimensions off, or find every centre line, because you used the layers.",
        ctl: [],
        fig: layersFig,
        math: () => ["VISIBLE", "CENTER", "HIDDEN", "DIMENSION", "NOTES", "0"].map((k) => ln(`${V_(k)} ${LAYER_MEANS[k]}`)).join(""),
      },
      {
        t: "Draw on the right layer",
        p: "New objects go on the current layer, marked CURRENT in the Layers panel (Ctrl+2). Click a layer's name to make it current. Dimensions go on DIMENSION by themselves, but text goes on the current layer, so make NOTES current before you write notes. To move something to another layer, select it and click Layer in Properties.",
        ctl: [],
        fig: currentFig,
        math: () => ln(`current now: ${V_(esc(layerList().find((l) => l.current)?.name || "VISIBLE"))}`) + ln("The + in the Layers panel adds a layer of your own.", "dim"),
      },
      {
        t: "Colour and weight come from the layer",
        p: "Objects are drawn ByLayer: they take their colour, linetype and weight from their layer. Change a layer once and everything on it follows: click the line sample on its row for its linetype, its lineweight and whether it plots. The eye turns a layer off while you work; the lock keeps it visible but stops changes, which is handy while you dimension. LT loads a linetype, LW sets the weight of new objects, and LTS scales the dashes when centre lines look solid or coarse.",
        ctl: [],
        fig: () => plateLayersFig(),
        math: () => ln(`VISIBLE ${V_("0.50 mm")}, the rest ${V_("0.25 mm")}`) + ln("Thick for the part, thin for everything that explains it.", "dim") + ["LINETYPE", "LWEIGHT", "LTSCALE"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join(""),
        basis: "Two line weights, thick about twice thin, is the usual pair for part drawings (ASME Y14.2 and ISO 128-2 set the line types).",
      },
      {
        t: "Your turn: a plate on the right layers",
        p: "Draw a plate with a hole on VISIBLE, then mark the hole with centre lines on CENTER. The centre lines cross at the hole's centre and run a little past its edge.",
        ctl: [],
        fig: () => plateLayersFig({ dims: false, note: false }),
        math: () => {
          const v = plateLayersTaskVals();
          return ln(`plate ${V_(`${xs(v.W)} × ${xs(v.H)}`)} · hole ${V_(`Ø${xs(v.d)}`)} in the middle`);
        },
        task: {
          do: () => {
            const v = plateLayersTaskVals(), cx = v.W / 2, cy = v.H / 2, L = v.d + 2 * v.e;
            return [
              `Click ${kbd("VISIBLE")} in the Layers panel to make it current.`,
              `${kbd("R")}, then ${kbd("0,0")} and ${kbd(`@${xs(v.W)},${xs(v.H)}`)}.`,
              `${kbd("C")}, centre ${kbd(`${xs(cx)},${xs(cy)}`)}, ${kbd("D")}, ${kbd(xs(v.d))}.`,
              `Make ${kbd("CENTER")} current. ${kbd("L")} from ${kbd(`${xs(cx - L / 2)},${xs(cy)}`)} to ${kbd(`@${xs(L)},0`)}, Enter.`,
              `${kbd("L")} from ${kbd(`${xs(cx)},${xs(cy - L / 2)}`)} to ${kbd(`@0,${xs(L)}`)}, Enter.`,
            ];
          },
          check: () => {
            const v = plateLayersTaskVals();
            const es = entities();
            const res = [];
            const plate = rectsOf(es).find((r) => sizeIs(r, v.W, v.H));
            res.push(plate ? { ok: plate.layer === "VISIBLE", t: plate.layer === "VISIBLE" ? `${xf(v.W)} × ${xf(v.H)} plate on VISIBLE` : `The plate is on ${esc(plate.layer)}; select it and set Layer to VISIBLE in Properties.` } : { ok: false, t: `No ${xf(v.W)} × ${xf(v.H)} plate yet.` });
            if (!plate) return res;
            const c = [(plate.x0 + plate.x1) / 2, (plate.y0 + plate.y1) / 2];
            const hole = es.find((g) => g.type === "CIRCLE" && samePt(g.center, c));
            res.push(!hole ? { ok: false, t: "No circle at the plate's centre." } : !same(2 * hole.radius, v.d) ? { ok: false, t: `The hole is Ø${xf(2 * hole.radius)}; it should be Ø${xf(v.d)}.` } : { ok: lay(hole) === "VISIBLE", t: lay(hole) === "VISIBLE" ? `Ø${xf(v.d)} hole on VISIBLE` : `The hole is on ${esc(lay(hole))}; move it to VISIBLE.` });
            const through = (g, horiz) => {
              const [a, b] = [g.start, g.end];
              if (horiz) return same(a[1], c[1]) && same(b[1], c[1]) && Math.min(a[0], b[0]) < c[0] - v.d / 2 && Math.max(a[0], b[0]) > c[0] + v.d / 2;
              return same(a[0], c[0]) && same(b[0], c[0]) && Math.min(a[1], b[1]) < c[1] - v.d / 2 && Math.max(a[1], b[1]) > c[1] + v.d / 2;
            };
            for (const horiz of [true, false]) {
              const ls = es.filter((g) => g.type === "LINE" && through(g, horiz));
              const on = ls.find((g) => lay(g) === "CENTER");
              const w = horiz ? "Horizontal" : "Vertical";
              res.push(on ? { ok: true, t: `${w} centre line on CENTER, past the hole's edge` } : ls.length ? { ok: false, t: `${w} centre line is on ${esc(lay(ls[0]))}; move it to CENTER.` } : { ok: false, t: `No ${w.toLowerCase()} centre line through the hole that reaches past its edge.` });
            }
            return res;
          },
          show: () =>
            placeOnDrawing("plate on the right layers", (d) => {
              const v = plateLayersTaskVals(), cx = v.W / 2, cy = v.H / 2, L = v.d + 2 * v.e;
              d.layer("VISIBLE", 7, () => {
                d.rect(0, 0, v.W, v.H);
                d.circle(cx, cy, v.d);
              });
              d.layer("CENTER", 2, () => {
                d.line(cx - L / 2, cy, cx + L / 2, cy);
                d.line(cx, cy - L / 2, cx, cy + L / 2);
              });
            }, { raw: true }),
        },
      },
    ],
  };

  // ================================================================ LESSON: HIDDEN LINES AND CENTRE LINES

  /** Front view of a block with a vertical through-hole, and its top view above. */
  function twoViews({ hidden = true, centre = true, gapK = 0.35 } = {}) {
    const L = s.vL, W = s.vW, H = s.vH, d = Math.min(s.vd, L * 0.8, W * 0.8);
    const k = Math.min(400 / L, 260 / (H + W + gapK * 40));
    const x0 = 300 - (L * k) / 2, yF = 340, yT = yF - H * k - 34;
    const cx = 300, r = (d * k) / 2;
    let g = lt(rectD(x0, yF - H * k, L * k, H * k), "w", "cont", 3.4) + lt(rectD(x0, yT - W * k, L * k, W * k), "w", "cont", 3.4);
    const cyT = yT - (W * k) / 2;
    g += lt(circD(cx, cyT, r), "w", "cont", 3.4);
    if (hidden) g += lt(`M ${f1(cx - r)} ${f1(yF - H * k)} V ${yF} M ${f1(cx + r)} ${f1(yF - H * k)} V ${yF}`, "r", "hidden", 1.8);
    if (centre) {
      const e = 12;
      g += lt(`M ${cx} ${f1(yF - H * k - e)} V ${yF + e}`, "y", "center", 1.6);
      g += lt(`M ${cx} ${f1(cyT - r - e)} V ${f1(cyT + r + e)} M ${f1(cx - r - e)} ${f1(cyT)} H ${f1(cx + r + e)}`, "y", "center", 1.6);
    }
    label(x0 - 12, yF - (H * k) / 2 + 6, "front", "b", 17, "end");
    label(x0 - 12, cyT + 6, "top", "b", 17, "end");
    return { g, x0, yF, yT, k, cx, r };
  }
  function hiddenFig() {
    const v = twoViews({ centre: false });
    let g = v.g;
    label(v.cx, v.yF + 26, "hidden: the hole's edges", "r", 17, "middle");
    return svg(g, "A block's top view showing a round hole, and its front view below with the hole's two edges as dashed hidden lines.", "40 20 520 360");
  }
  function centreFig() {
    const r = clamp(s.vd * 3, 30, 110), cx = 220, cy = 200, e = 3 * 3;
    const ext = inch() ? 0.125 * 25.4 : 3;
    const ep = 18;
    let g = lt(circD(cx, cy, r), "w", "cont", 3.4);
    g += lt(`M ${cx - r - ep} ${cy} H ${cx + r + ep} M ${cx} ${cy - r - ep} V ${cy + r + ep}`, "y", "center", 1.8);
    g += dim(cx + r, cy + 22, cx + r + ep, cy + 22, "a");
    label(cx + r + ep + 6, cy + 30, `about ${nu(ext)}`, "a", 15);
    const sx = 470, sw = clamp(s.vd * 3, 30, 110);
    g += lt(rectD(sx, cy - 70, 90, 140), "w", "cont", 3.4);
    g += lt(`M ${sx - 14} ${cy} H ${sx + 104}`, "y", "center", 1.8);
    label(sx + 45, cy - 90, "side view: one line", "b", 16, "middle");
    label(cx, cy + r + 50, "round view: two lines", "b", 16, "middle");
    void e;
    void sw;
    return svg(g, "A circle with two centre lines crossing at its centre and running past its edge, and a side view of a round part with one centre line along its axis.", "60 60 540 320");
  }
  function precedenceFig() {
    let g = lt("M 80 120 H 500", "w", "cont", 3.4) + lt("M 80 130 H 500", "r", "hidden", 1.8);
    label(80, 100, "visible over hidden: draw only the visible line", "w", 18);
    g += mark(512, 124, false);
    g += lt("M 80 260 H 500", "r", "hidden", 1.8) + lt("M 80 270 H 500", "y", "center", 1.6);
    label(80, 240, "hidden over centre", "w", 18);
    g += mark(512, 264, false);
    label(80, 320, "visible › hidden › centre", "a", 22);
    return svg(g, "Pairs of overlapping lines, showing that a visible line wins over a hidden one and a hidden line wins over a centre line.", "60 70 480 270");
  }
  const hidVals = () => ({ L: xv(80, 3), H: xv(20, 0.75), W: xv(40, 1.5), G: xv(15, 0.5), d: xv(20, 0.75), e: xv(3, 0.125) });

  const LINES = {
    id: "d-lines",
    title: "Hidden lines and centre lines",
    q: "How do I show what you can't see?",
    blurb: "Hidden edges, centre lines, which line wins",
    src: "Line conventions per ASME Y14.2 and ISO 128-2; overshoot of centre lines is common practice.",
    steps: [
      {
        t: "Hidden lines show what's behind",
        p: "A hole drilled down through a block can't be seen from the front, but the reader needs to know it's there. Draw its edges on HIDDEN: thin and dashed. Each view shows exactly what you'd see looking from that side, with hidden edges dashed.",
        ctl: ["vL", "vH", "vd"],
        fig: hiddenFig,
        math: () => ln(`the hole's edges sit ${V_(nt(Math.min(s.vd, s.vL * 0.8, s.vW * 0.8) / 2))} each side of its centre`) + ln("Same size in both views, lined up.", "dim"),
      },
      {
        t: "Centre lines mark every axis",
        p: "Every hole and round feature gets centre lines on CENTER: two crossing at the centre in the view where it's round, one along its axis in the side view. Run them a few millimetres past the edge, and keep the short dash at the centre.",
        ctl: ["vd"],
        fig: centreFig,
        math: () => ln(`overshoot about ${V_(nu(inch() ? 3.175 : 3))} past the edge`) + ln("Centre lines can be extended and used as extension lines for dimensions.", "dim"),
        basis: "Common drafting practice; ASME Y14.2 and ISO 128-2 define the centre-line and hidden-line types.",
      },
      {
        t: "When lines land on each other",
        p: "Where two kinds of line would overlap, draw only the more important one: a visible edge wins over a hidden one, and a hidden edge wins over a centre line.",
        ctl: [],
        fig: precedenceFig,
        math: () => ln(`${V_("visible")} › ${V_("hidden")} › ${V_("centre")}`),
      },
      {
        t: "Your turn: the hole in the front view",
        p: "The board draws the front and top views of a block with a hole. The top view shows the hole round; add its hidden edges and centre line to the front view.",
        ctl: [],
        fig: () => svg(twoViews({ hidden: true, centre: true }).g, "Front and top views of a block with a hole, with hidden lines and a centre line in the front view.", "40 20 520 360"),
        math: () => ln("hidden lines on HIDDEN, centre line on CENTER"),
        task: {
          setup: {
            label: "Set up the two views",
            run: () =>
              placeOnDrawing("front and top views", (d) => {
                const v = hidVals();
                d.layer("VISIBLE", 7, () => {
                  d.rect(0, 0, v.L, v.H);
                  d.rect(0, v.H + v.G, v.L, v.H + v.G + v.W);
                  d.circle(v.L / 2, v.H + v.G + v.W / 2, v.d);
                });
              }, { raw: true }),
          },
          do: () => {
            const o = T0();
            if (!o) return [needSetup("Set up the two views")];
            const v = hidVals(), cx = v.L / 2, r = v.d / 2;
            return [
              `Make ${kbd("HIDDEN")} current.`,
              `${kbd("L")} from ${kbd(at(o, cx - r, 0))} to ${kbd(`@0,${xs(v.H)}`)}, Enter.`,
              `${kbd("L")} from ${kbd(at(o, cx + r, 0))} to ${kbd(`@0,${xs(v.H)}`)}, Enter.`,
              `Make ${kbd("CENTER")} current. ${kbd("L")} from ${kbd(at(o, cx, -v.e))} to ${kbd(`@0,${xs(v.H + 2 * v.e)}`)}, Enter.`,
            ];
          },
          check: (t) => {
            const o = t.o;
            if (!o) return [{ ok: false, t: "Set up the two views first." }];
            const v = hidVals(), cx = o[0] + v.L / 2, r = v.d / 2, yb = o[1], yt = o[1] + v.H;
            const es = entities().filter((g) => g.type === "LINE");
            const vert = (g, x) => same(g.start[0], x) && same(g.end[0], x);
            const spans = (g, a, b) => Math.min(g.start[1], g.end[1]) <= a + xtol() && Math.max(g.start[1], g.end[1]) >= b - xtol();
            const res = [];
            for (const [x, side] of [[cx - r, "Left"], [cx + r, "Right"]]) {
              const ls = es.filter((g) => vert(g, x) && spans(g, yb, yt));
              const on = ls.find((g) => lay(g) === "HIDDEN");
              res.push(on ? { ok: true, t: `${side} hidden edge, full height, on HIDDEN` } : ls.length ? { ok: false, t: `${side} edge is on ${esc(lay(ls[0]))}; move it to HIDDEN.` } : { ok: false, t: `No ${side.toLowerCase()} hidden edge at the hole's ${side.toLowerCase()} side through the front view.` });
            }
            const cl = es.filter((g) => vert(g, cx) && Math.min(g.start[1], g.end[1]) < yb - xtol() && Math.max(g.start[1], g.end[1]) > yt + xtol());
            const on = cl.find((g) => lay(g) === "CENTER");
            res.push(on ? { ok: true, t: "Centre line on CENTER, running past both edges" } : cl.length ? { ok: false, t: `The centre line is on ${esc(lay(cl[0]))}; move it to CENTER.` } : { ok: false, t: "No centre line through the front view that runs past its top and bottom edges." });
            return res;
          },
          show: () =>
            placeOnDrawing("hidden-line exercise answer", (d) => {
              const v = hidVals(), cx = v.L / 2, r = v.d / 2;
              d.layer("VISIBLE", 7, () => {
                d.rect(0, 0, v.L, v.H);
                d.rect(0, v.H + v.G, v.L, v.H + v.G + v.W);
                d.circle(cx, v.H + v.G + v.W / 2, v.d);
              });
              d.layer("HIDDEN", 1, () => {
                d.line(cx - r, 0, cx - r, v.H);
                d.line(cx + r, 0, cx + r, v.H);
              });
              d.layer("CENTER", 2, () => {
                d.line(cx, -v.e, cx, v.H + v.e);
                const cy = v.H + v.G + v.W / 2;
                d.line(cx, cy - r - v.e, cx, cy + r + v.e);
                d.line(cx - r - v.e, cy, cx + r + v.e, cy);
              });
            }, { raw: true }),
        },
      },
    ],
  };

  // ================================================================ LESSON: UNITS AND PRECISION

  function unitsStartFig() {
    let g = path(rectD(70, 80, 200, 110)) + path(rectD(330, 80, 200, 110));
    label(90, 126, "Metric drawing", "w", 22);
    label(90, 160, "millimetres", "b", 18);
    label(350, 126, "Imperial drawing", "w", 22);
    label(350, 160, "inches", "b", 18);
    const u = doc ? (doc.info.insunits === 1 ? "Inches" : "Millimeters") : null;
    g += path(rectD(70, 230, 460, 100), "b");
    label(90, 266, "DRAWING", "b", 15);
    label(90, 296, "Units", "w", 18);
    label(510, 296, u || "Millimeters", "a", 18, "end");
    label(90, 320, "Display", "w", 18);
    label(510, 320, esc(doc?.info.display || "Decimal, 0.00"), "a", 18, "end");
    return svg(g, "The two welcome-screen choices, Metric drawing and Imperial drawing, and the Drawing panel showing the units and display format.", "50 60 500 290");
  }
  function decimalsFig() {
    const v = 12.3456, cur = Math.round(s.dec);
    let g = "";
    for (let d = 0; d <= 4; d++) {
      const y = 90 + d * 52, on = d === cur;
      const half = 0.5 * Math.pow(10, -d);
      label(250, y, v.toFixed(d), on ? "a" : "w", on ? 34 : 26, "end");
      label(290, y, `could be ±${String(+half.toFixed(d + 1))}`, on ? "a" : "b", on ? 20 : 17);
      if (on) g += path(`M 120 ${y + 12} H 470`, "a");
    }
    return svg(g, "The number 12.3456 shown with zero to four decimal places, each with how far off the shown number could be.", "90 50 420 290");
  }

  const UNITS_ = {
    id: "d-units",
    title: "Units and precision",
    q: "Millimetres or inches, and how many decimals?",
    blurb: "Drawing units, typing other units, decimals",
    src: "BlackCAD's metric and imperial drawings (INSUNITS 4 and 1), display formats Decimal 0.00 and 0.000, and unit parsing in the core.",
    steps: [
      {
        t: "Pick the units when you start",
        p: "On the welcome screen, Metric drawing works in millimetres and Imperial drawing in inches. The Drawing panel shows which one you're in, and the status bar shows the cursor in those units. Start in the units the shop works in, because every plain number you type is read in them. If you started in the wrong ones, UN (UNITS) changes the unit and offers to scale the drawing so everything keeps its real size.",
        ctl: [],
        fig: unitsStartFig,
        math: () => (doc ? ln(`this drawing: ${V_(doc.info.insunits === 1 ? "inches" : "millimetres")}, ${V_(esc(doc.info.display))}`) + ln(`${V_(aliasOf("UNITS"))} UNITS changes the unit, the length type and the decimals`, "dim") : ln("No drawing open: start one from the welcome screen.", "dim")),
      },
      {
        t: "Your turn: the other unit",
        p: "Draw a line in the other unit, without converting it yourself: type the unit and let BlackCAD do the maths.",
        ctl: [],
        fig: unitsFig,
        math: () => ln(du() === "in" ? `in an inch drawing, ${V_("50mm")} becomes ${V_("1.969 in")}` : `in a metric drawing, ${V_('1-1/2"')} becomes ${V_("38.10 mm")}`),
        task: {
          do: () => [`Type ${kbd("L")} and Enter, and click a start point anywhere.`, `Type ${kbd(du() === "in" ? "@50mm&lt;0" : '@1-1/2"&lt;0')} and Enter, then Enter again to finish.`, "Click the line and read its Length in Properties."],
          check: (t) => {
            const want = du() === "in" ? 50 / 25.4 : 38.1;
            const ls = fresh(t, entities()).filter((g) => g.type === "LINE");
            const good = ls.find((g) => same(g.length, want));
            return good ? [{ ok: true, t: `A line exactly ${xf(want)} long` }] : [{ ok: false, t: ls.length ? `No line is exactly ${xf(want)}; the nearest is ${xf(ls.reduce((b, g) => (Math.abs(g.length - want) < Math.abs(b - want) ? g.length : b), ls[0].length))}.` : "No lines in the drawing yet." }];
          },
          show: () => placeOnDrawing("line in the other unit", (d) => d.line(0, 0, du() === "in" ? 50 / 25.4 : 38.1, 0), { raw: true }),
        },
      },
      {
        t: "Decimals say how precise",
        p: "The number of decimals on a size tells the reader how closely it matters: 12.5 reads looser than 12.500. A metric drawing starts at two places and an inch drawing at three. Show only as many as the part needs, and let the title block's tolerance say the rest. UN sets the decimals for the whole drawing, dimensions included.",
        ctl: ["dec"],
        fig: decimalsFig,
        math: () => ln(`12.3456 shown as ${V_((12.3456).toFixed(Math.round(s.dec)))}`) + ln("Inch title blocks often tie tolerance to decimals, such as .XX ±.01 and .XXX ±.005.", "dim"),
        extra: () => `<div class="bb-row"><button class="chalkbtn" data-lesson="tol">Tolerances, in Shop</button></div>`,
        task: {
          do: () => [`Type ${kbd("UN")} and Enter.`, `Press Enter to keep Decimal, then type ${kbd(du() === "in" ? "4" : "3")} for the decimal places.`, "Press Enter through the angle and unit questions."],
          check: () => {
            const want = du() === "in" ? 4 : 3, i = doc.info;
            return [{ ok: i.lunits === 2, t: i.lunits === 2 ? "Decimal lengths" : "The length type isn't Decimal." }, { ok: i.luprec === want, t: i.luprec === want ? `${want} decimal places` : `${i.luprec} decimal places now; the exercise asks for ${want}.` }];
          },
        },
      },
    ],
  };

  // ================================================================ LESSON: DRAW FULL SIZE

  const SHEETS = { A4: [297, 210], A3: [420, 297], Letter: [279.4, 215.9], Tabloid: [431.8, 279.4] };
  const SCALES = () => (du() === "in" ? [1, 2, 4, 8, 16, 32, 48, 96] : [1, 2, 5, 10, 20, 50, 100, 200]);
  const TB = { w: 170, h: 40, m: 10 }; // title block and margin on paper, mm
  /** The smallest standard scale that fits a w × h (mm) part on the sheet. */
  function fitScale(w, h, sheet = s.sheet) {
    const [SW, SH] = SHEETS[sheet];
    const uw = SW - 2 * TB.m - 10, uh = SH - 2 * TB.m - TB.h - 10;
    return SCALES().find((S) => w / S <= uw && h / S <= uh) || SCALES().at(-1);
  }
  const sheetButtons = () => `<div class="bb-row">${buttons("sheet", Object.keys(SHEETS).map((k) => [k, k === "Letter" ? "Letter" : k === "Tabloid" ? "Tabloid" : k]), s.sheet)}</div>`;
  function fullSizeFig() {
    const L = s.sL, k = 420 / Math.max(L, 1);
    let g = path(rectD(80, 160, L * k, 70)) + dim(80, 260, 80 + L * k, 260, "g");
    label(80 + (L * k) / 2, 252, nt(L), "g", 20, "middle");
    g += path(rectD(80, 80, 150, 34), "b");
    label(94, 103, "Model · 1:1", "b", 18);
    label(80, 320, "the part is its real length in the drawing", "w", 18);
    return svg(g, "A part drawn at its real length with its length dimensioned, and the Model 1:1 tag above it.", "60 60 480 280");
  }
  function sheetFig() {
    const [SW, SH] = SHEETS[s.sheet];
    const L = s.sL, Hp = L * 0.45, S = fitScale(L, Hp);
    const k = Math.min(440 / SW, 280 / SH);
    const x0 = 300 - (SW * k) / 2, y0 = 40;
    let g = path(rectD(x0, y0, SW * k, SH * k), "w") + path(rectD(x0 + TB.m * k, y0 + TB.m * k, (SW - 2 * TB.m) * k, (SH - 2 * TB.m) * k), "b");
    const tbx = x0 + (SW - TB.m - TB.w) * k, tby = y0 + (SH - TB.m - TB.h) * k;
    g += path(rectD(tbx, tby, TB.w * k, TB.h * k), "b");
    label(tbx + 8, tby + 18, `SCALE 1:${S}`, "a", 15);
    const pw = (L / S) * k, ph = (Hp / S) * k;
    g += fill(rectD(x0 + (SW * k - pw) / 2, y0 + TB.m * k + 10, pw, ph), "b") + path(rectD(x0 + (SW * k - pw) / 2, y0 + TB.m * k + 10, pw, ph), "a");
    label(300, y0 + SH * k + 30, `${s.sheet} sheet · part ${nt(L)} long → 1:${S}`, "a", 19, "middle");
    return svg(g, "A sheet with a border and title block, and the part drawn smaller to fit at the chosen scale.", "40 20 520 360");
  }
  const SCALE_ = {
    id: "d-scale",
    title: "Draw full size",
    q: "What scale do I draw at?",
    blurb: "Model at 1:1, scale on the sheet",
    src: "BlackCAD draws in model space at 1:1. Standard drawing scales: metric per ISO 5455 (1:2, 1:5, 1:10 …); inch scales by halves.",
    steps: [
      {
        t: "The model is always full size",
        p: "Draw every part at its real size: a 300 mm part is 300 long in BlackCAD. The tag at the top of the drawing reads Model · 1:1. Never shrink the geometry to fit a page; dimensions measure the geometry, so they'd come out wrong.",
        ctl: ["sL"],
        fig: fullSizeFig,
        math: () => ln(`drawn length ${V_(nt(s.sL))} = real length ${V_(nt(s.sL))}`),
      },
      {
        t: "The scale belongs to the sheet",
        p: "Scale only happens when the drawing goes onto a sheet. Type LO and choose New (or press Try it below) for a layout: real paper, in millimetres, with its own tab beside Model. MV puts a viewport on it, a window onto the model, and the viewport's scale (1:2, 1:5 …) says how much smaller the model looks there. The model itself never changes, and text on the sheet is paper size, so 3.5 mm text prints 3.5 mm.",
        ctl: ["sL"],
        fig: sheetFig,
        extra: sheetButtons,
        math: () => {
          const S = fitScale(s.sL, s.sL * 0.45), [SW, SH] = SHEETS[s.sheet];
          return ln(`scale ${V_(`1:${S}`)}`) + ln(`${esc(s.sheet)} sheet ${V_(`${SW} × ${SH} mm`)} of paper`) + ln(`the part stays ${V_(nu(s.sL))} long in the model; on the sheet it measures ${V_(nu(s.sL / S))}`) + ln(`${V_(aliasOf("LAYOUT"))} LAYOUT · ${V_(aliasOf("MVIEW"))} MVIEW · ${V_(aliasOf("PAGESETUP"))} PAGESETUP`, "dim");
        },
        place: { label: "Put my drawing on a sheet with a title block", run: () => placeLayout() },
      },
    ],
  };

  // ================================================================ LESSON: ORTHOGRAPHIC VIEWS

  /** A stepped block: length L (x), depth W (y), height H (z), step at 2/3 L, half height. */
  function pictorial(x0, y0, k) {
    const L = s.vL * k, W = s.vW * k, H = s.vH * k;
    const o = [W * 0.5 * Math.cos(Math.PI / 4), -W * 0.5 * Math.sin(Math.PI / 4)];
    const F = [[0, 0], [L, 0], [L, H / 2], [(2 * L) / 3, H / 2], [(2 * L) / 3, H], [0, H]].map(([x, z]) => [x0 + x, y0 - z]);
    const B = F.map(([x, y]) => [x + o[0], y + o[1]]);
    let g = fill(poly(F), "b") + path(poly(F));
    g += path(poly([B[5], B[4], B[3], B[2], B[1]], false));
    for (const i of [1, 2, 3, 4, 5]) g += path(`M ${f1(F[i][0])} ${f1(F[i][1])} L ${f1(B[i][0])} ${f1(B[i][1])}`);
    return { g, F, B };
  }
  function viewsFig1() {
    const k = Math.min(260 / (s.vL + s.vW * 0.35), 200 / (s.vH + s.vW * 0.35));
    const p = pictorial(150, 300, k);
    let g = p.g;
    g += arrow(p.F[0][0] + (s.vL * k) / 3, 380, p.F[0][0] + (s.vL * k) / 3, 320, "a");
    label(p.F[0][0] + (s.vL * k) / 3, 400, "FRONT", "a", 18, "middle");
    const top = [(p.F[5][0] + p.B[4][0]) / 2, Math.min(...p.B.map((q) => q[1])) - 70];
    g += arrow(top[0], top[1], top[0], top[1] + 50, "a");
    label(top[0], top[1] - 10, "TOP", "a", 18, "middle");
    const rx = Math.max(...p.B.map((q) => q[0])) + 70, ry = (p.F[1][1] + p.F[2][1]) / 2;
    g += arrow(rx, ry, rx - 50, ry, "a");
    label(rx + 8, ry + 6, "RIGHT", "a", 18);
    return svg(g, "A sketch of a stepped block with arrows showing the directions you look from for the front, top and right side views.", "60 10 520 410");
  }
  function viewLayout(third = s.proj !== "first") {
    const L = s.vL, W = s.vW, H = s.vH;
    const gp = 30;
    const k = Math.min((460 - gp) / (L + W), (320 - gp) / (H + W));
    const fx = third ? 70 : 70 + W * k + gp, fy = third ? 70 + W * k + gp : 70;
    const front = [[0, 0], [L, 0], [L, H / 2], [(2 * L) / 3, H / 2], [(2 * L) / 3, H], [0, H]].map(([x, z]) => [fx + x * k, fy + H * k - z * k]);
    let g = path(poly(front));
    const tx = fx, ty = third ? 70 : fy + H * k + gp;
    g += path(rectD(tx, ty, L * k, W * k)) + path(`M ${f1(tx + ((2 * L) / 3) * k)} ${f1(ty)} V ${f1(ty + W * k)}`);
    const rx = third ? fx + L * k + gp : 70, ry = fy;
    g += path(rectD(rx, ry, W * k, H * k)) + path(`M ${f1(rx)} ${f1(ry + (H * k) / 2)} H ${f1(rx + W * k)}`);
    label(fx + (L * k) / 2, fy + H * k + 22, "FRONT", "b", 16, "middle");
    label(tx + (L * k) / 2, third ? ty - 8 : ty + W * k + 20, "TOP", "b", 16, "middle");
    label(rx + (W * k) / 2, ry + H * k + 22, "RIGHT", "b", 16, "middle");
    return { g, k, fx, fy, tx, ty, rx, ry, L, W, H, gp };
  }
  function viewsFig2() {
    const v = viewLayout();
    label(560, 60, s.proj === "first" ? "first angle" : "third angle", "a", 22, "end");
    return svg(v.g, s.proj === "first" ? "First-angle layout: top view below the front, right side view to the left of it." : "Third-angle layout: top view above the front, right side view to the right of it.", "40 30 560 400");
  }
  function viewsFig3() {
    const v = viewLayout(true);
    let g = v.g;
    const right = v.fx + v.L * v.k, topB = v.ty + v.W * v.k;
    g += dash(`M ${f1(v.fx)} ${f1(topB)} V ${f1(v.fy)} M ${f1(right)} ${f1(topB)} V ${f1(v.fy)} M ${f1(v.fx + ((2 * v.L) / 3) * v.k)} ${f1(topB)} V ${f1(v.fy + (v.H / 2) * v.k)}`, "a");
    g += dash(`M ${f1(right)} ${f1(v.fy)} H ${f1(v.rx)} M ${f1(right)} ${f1(v.fy + v.H * v.k)} H ${f1(v.rx)}`, "a");
    // the 45° line carries depth from the top view across to the side view
    const Wk = v.W * v.k;
    g += path(`M ${f1(v.rx - 10)} ${f1(topB + 10)} L ${f1(v.rx + Wk + 10)} ${f1(v.ty - 10)}`, "b");
    g += dash(`M ${f1(right)} ${f1(v.ty)} H ${f1(v.rx + Wk)} V ${f1(v.ry)} M ${f1(right)} ${f1(topB)} H ${f1(v.rx)} V ${f1(v.ry)}`, "b");
    label(v.rx + Wk + 14, v.ty + 4, "45° line", "b", 15);
    return svg(g, "Front, top and right views lined up, with dashed projection lines carrying edges between them and a 45 degree line carrying depth from the top view to the side view.", "40 30 560 400");
  }
  function oneViewFig() {
    let g = path(rectD(110, 110, 300, 170)) + circle(180, 170, 24) + circle(340, 220, 24);
    g += lt("M 150 170 H 210 M 180 140 V 200 M 310 220 H 370 M 340 190 V 250", "y", "center", 1.6);
    label(110, 320, "PLATE T 3 · one view is enough", "c", 18);
    label(110, 90, "flat plate", "b", 18);
    g += lt("M 450 140 H 540 M 450 250 H 540", "w", "cont", 3) + lt("M 440 195 H 550", "y", "center", 1.6) + path("M 450 140 V 250 M 540 140 V 250");
    label(495, 280, "turned pin: Ø", "b", 16, "middle");
    return svg(g, "A flat plate shown in a single view with a thickness note, and a turned pin shown in one side view with its centre line.", "80 60 490 290");
  }
  const viewTaskVals = () => ({ L: xv(80, 3), H: xv(30, 1.25), W: xv(40, 1.5), G: xv(10, 0.5) });

  const VIEWS = {
    id: "d-views",
    title: "Orthographic views",
    q: "Which views does a part need?",
    blurb: "Front, top and side; third and first angle; lining views up",
    src: "Orthographic projection per ASME Y14.3 and ISO 128-3 (third-angle and first-angle arrangements).",
    steps: [
      {
        t: "Look straight at each side",
        p: "An orthographic view looks straight at one side of the part, so every edge facing you shows at its true length. The front view shows the part's most telling shape. The top view looks down on it, and the right side view looks at it from the right.",
        ctl: ["vL", "vW", "vH"],
        fig: viewsFig1,
        math: () => ln(`length ${V_(nt(s.vL))} · depth ${V_(nt(s.vW))} · height ${V_(nt(s.vH))}`),
      },
      {
        t: "Where the views go",
        p: "In third-angle projection, used in North America, the top view sits above the front and the right side view to its right. In first angle, used across most of Europe and Asia, they swap: the top view goes below and the right side view on the left. The title block says which.",
        ctl: ["vL", "vW", "vH"],
        fig: viewsFig2,
        extra: () => `<div class="bb-row">${buttons("proj", [["third", "Third angle"], ["first", "First angle"]], s.proj)}</div>`,
        math: () => (s.proj === "first" ? ln(`first angle: top ${V_("below")}, right side on the ${V_("left")}`) : ln(`third angle: top ${V_("above")}, right side on the ${V_("right")}`)),
      },
      {
        t: "Line the views up",
        p: "Views share sizes: the front and top have the same length, and the front and side the same height. Line them up so the reader can carry a feature from one view to the next. Construction lines do it: XL makes a line that runs forever both ways (press H or V for horizontal or vertical, then click a snapped corner), and RAY starts at a point, which suits the 45° line that carries depth from the top view to the side view. Keep them on a layer of their own, such as CONSTRUCTION, and switch it off when you are done.",
        ctl: ["vL", "vW", "vH"],
        fig: viewsFig3,
        math: () => ln(`front and top: same length ${V_(nt(s.vL))}`) + ln(`front and side: same height ${V_(nt(s.vH))}`) + ln(`top and side: same depth ${V_(nt(s.vW))}`) + ["XLINE", "RAY"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join(""),
        task: {
          setup: { label: "Set up the front view", run: () => placeOnDrawing("front view", (d) => d.layer("VISIBLE", 7, () => d.rect(0, 0, viewTaskVals().L, viewTaskVals().H)), { raw: true }) },
          do: () => (T0() ? [`Type ${kbd("XL")} and Enter, then ${kbd("V")} and Enter for vertical.`, "Snap to the front view's lower right corner (END) and click. Press Enter to finish."] : [needSetup("Set up the front view")]),
          check: (t) => {
            if (!t.o) return [{ ok: false, t: "Set up the front view first." }];
            const v = viewTaskVals();
            const xl = fresh(t, entities()).filter((g) => g.type === "XLINE");
            if (!xl.length) return [{ ok: false, t: "No construction line yet: XL, then V." }];
            const vert = xl.filter((g) => same(Math.abs(Math.sin((g.angle * Math.PI) / 180)), 1, 1e-6));
            const on = vert.find((g) => same(g.base[0], t.o[0] + v.L));
            return [{ ok: vert.length > 0, t: vert.length ? "A vertical construction line" : "The line isn't vertical: press V before you click." }, { ok: !!on, t: on ? "Through the front view's right edge" : "It should pass through the front view's right edge: snap to its corner." }];
          },
          show: () => placeOnDrawing("front view with a construction line", (d) => { const v = viewTaskVals(); d.layer("VISIBLE", 7, () => d.rect(0, 0, v.L, v.H)); d.exec(`xline-angle ${d.pt(v.L, 0)} 90`); }, { raw: true }),
        },
      },
      {
        t: "Only the views you need",
        p: "Draw the fewest views that show every feature. A flat plate needs one view and a thickness note. A turned pin needs one side view, with its diameters dimensioned Ø. Add a view only when it shows something the others can't.",
        ctl: [],
        fig: oneViewFig,
        math: () => ln("plate: one view + a note such as PLATE T 3") + ln("most machined parts: two or three views"),
      },
      {
        t: "Your turn: draw the top view",
        p: () => `The board draws the front view of a plain block. Draw its top view in line with it, ${s.proj === "first" ? "below (first angle)" : "above (third angle)"}: same length, and as deep as the block.`,
        ctl: [],
        fig: () => {
          const v = viewTaskVals(), k = 300 / v.L;
          const third = s.proj !== "first";
          const fy = third ? 300 : 150, ty = third ? 300 - v.H * k - 30 - v.W * k : 150 + v.H * k + 30;
          let g = path(rectD(150, fy - (third ? v.H * k : 0), v.L * k, v.H * k)) + dash(rectD(150, ty, v.L * k, v.W * k), "a");
          label(140, fy - (third ? (v.H * k) / 2 : -(v.H * k) / 2) + 6, "front", "b", 17, "end");
          label(140, ty + (v.W * k) / 2 + 6, "top: you", "a", 17, "end");
          return svg(g, "A block's front view, with a dashed rectangle where the top view goes.", "40 20 520 360");
        },
        math: () => {
          const v = viewTaskVals();
          return ln(`block ${V_(`${xs(v.L)} long × ${xs(v.W)} deep × ${xs(v.H)} high`)}`);
        },
        task: {
          setup: { label: "Set up the front view", run: () => placeOnDrawing("front view", (d) => d.layer("VISIBLE", 7, () => d.rect(0, 0, viewTaskVals().L, viewTaskVals().H)), { raw: true }) },
          do: () => {
            const o = T0();
            if (!o) return [needSetup("Set up the front view")];
            const v = viewTaskVals();
            const y = s.proj === "first" ? -v.G - v.W : v.H + v.G;
            return [`${kbd("R")}, first corner ${kbd(at(o, 0, y))}.`, `Other corner ${kbd(`@${xs(v.L)},${xs(v.W)}`)}.`, "Or snap it yourself: start above the front view's corner with Ortho on."];
          },
          check: (t) => {
            const o = t.o;
            if (!o) return [{ ok: false, t: "Set up the front view first." }];
            const v = viewTaskVals();
            const third = s.proj !== "first";
            const rs = rectsOf(fresh(t, entities()));
            const good = rs.find((r) => sizeIs(r, v.L, v.W) && same(r.x0, o[0]) && (third ? r.y0 > o[1] + v.H + xtol() : r.y1 < o[1] - xtol()));
            if (good) return [{ ok: true, t: `${xf(v.L)} × ${xf(v.W)} top view` }, { ok: true, t: `Lined up with the front view, ${third ? "above" : "below"} it` }];
            const sz = rs.find((r) => sizeIs(r, v.L, v.W));
            if (sz) return [{ ok: true, t: `${xf(v.L)} × ${xf(v.W)} top view` }, { ok: false, t: !same(sz.x0, o[0]) ? "It isn't lined up: its left edge should be straight above the front view's." : `It should sit ${third ? "above" : "below"} the front view, with a gap.` }];
            return [{ ok: false, t: `No ${xf(v.L)} × ${xf(v.W)} rectangle yet.` }];
          },
          show: () =>
            placeOnDrawing("front and top views", (d) => {
              const v = viewTaskVals(), y = s.proj === "first" ? -v.G - v.W : v.H + v.G;
              d.layer("VISIBLE", 7, () => {
                d.rect(0, 0, v.L, v.H);
                d.rect(0, y, v.L, y + v.W);
              });
            }, { raw: true }),
        },
      },
    ],
  };

  // ================================================================ LESSON: SECTIONS

  /** 45° hatch lines clipped to a rectangle, spacing sp. */
  function hatchRect(x, y, w, h, sp = 12, c = "b") {
    let d = "";
    for (let t = -h; t < w; t += sp) {
      const x1 = x + Math.max(t, 0), y1 = y + h - Math.max(-t, 0);
      const len_ = Math.min(w - Math.max(t, 0), h - Math.max(-t, 0));
      d += `M ${f1(x1)} ${f1(y1)} l ${f1(len_)} ${f1(-len_)} `;
    }
    return `<path class="dsh c-${c}" style="--d:.6s;stroke-dasharray:none;stroke-width:1.4" d="${d}"/>`;
  }
  function sectionParts() {
    const x0 = 120, L = 340, cx = x0 + L / 2;
    const D = 80, d = 40, cb = 18, T = 60;
    return { x0, L, cx, D, d, cb, T };
  }
  function sectionFig(withHatch = true, label2 = true) {
    const p = sectionParts();
    const ty = 60, tW = 120;
    let g = path(rectD(p.x0, ty, p.L, tW)) + circle(p.cx, ty + tW / 2, p.D / 2) + circle(p.cx, ty + tW / 2, p.d / 2);
    g += lt(`M ${p.x0 - 20} ${ty + tW / 2} H ${p.x0 + p.L + 20}`, "a", "phantom", 3);
    g += arrow(p.x0 - 20, ty + tW / 2, p.x0 - 20, ty + tW / 2 + 30, "a") + arrow(p.x0 + p.L + 20, ty + tW / 2, p.x0 + p.L + 20, ty + tW / 2 + 30, "a");
    label(p.x0 - 30, ty + tW / 2 - 8, "A", "a", 20, "end");
    label(p.x0 + p.L + 30, ty + tW / 2 - 8, "A", "a", 20);
    const sy = 240;
    const left = [[p.x0, sy], [p.cx - p.D / 2, sy], [p.cx - p.D / 2, sy + p.cb], [p.cx - p.d / 2, sy + p.cb], [p.cx - p.d / 2, sy + p.T], [p.x0, sy + p.T]];
    const right = left.map(([x, y]) => [2 * p.cx - x, y]);
    g += path(poly(left)) + path(poly(right));
    if (withHatch) {
      g += hatchRect(p.x0, sy, p.cx - p.D / 2 - p.x0, p.T) + hatchRect(p.cx - p.D / 2, sy + p.cb, (p.D - p.d) / 2, p.T - p.cb);
      g += hatchRect(p.cx + p.D / 2, sy, p.cx - p.D / 2 - p.x0, p.T) + hatchRect(p.cx + p.d / 2, sy + p.cb, (p.D - p.d) / 2, p.T - p.cb);
    }
    g += lt(`M ${p.cx} ${sy - 12} V ${sy + p.T + 12}`, "y", "center", 1.6);
    if (label2) label(p.cx, sy + p.T + 40, "SECTION A-A", "w", 20, "middle");
    return svg(g, "A plate's top view with a cutting-plane line A-A through a counterbored hole, and below it the section view with the cut material hatched and the hole left clear.", "50 30 500 360");
  }
  function hatchFig() {
    let g = path(rectD(100, 90, 160, 200)) + hatchRect(100, 90, 160, 200, 14);
    g += path(rectD(340, 90, 160, 200)) + hatchRect(340, 90, 160, 200, 14, "a");
    label(180, 320, "cut material: 45°", "b", 18, "middle");
    label(420, 320, "same angle, evenly spaced", "a", 18, "middle");
    return svg(g, "Two blocks of 45 degree hatching, evenly spaced.", "70 60 460 280");
  }
  const hatchVals = () => ({ W: xv(60, 2.5), H: xv(40, 1.5), d: xv(16, 0.625) });
  const SECTIONS = {
    id: "d-sections",
    title: "Sections",
    q: "How do I show the inside of a part?",
    blurb: "Cutting planes, hatching, naming",
    src: "Section views per ASME Y14.3 and ISO 128-50 (hatching at 45°).",
    steps: [
      {
        t: "Cut it open on paper",
        p: "When hidden lines get confusing, cut the part along a line and draw what you'd see. A cutting-plane line on another view marks where the cut is, and its arrows point the way you look.",
        ctl: [],
        fig: () => sectionFig(true, false),
        math: () => ln("cutting-plane line: thick, long-short-short dashes, arrows at each end") + ln("Draw it on a layer of its own, such as SECTION.", "dim"),
      },
      {
        t: "Hatch what the saw would touch",
        p: "In the section, the cut material is hatched with thin lines at 45°. Holes and gaps weren't cut, so they stay clear. Edges behind the cut are drawn as visible lines; hidden lines are usually left out. Type H, then click inside each cut region: the core finds the boundary, leaves the holes clear, and the hatch follows if you change the shape. The Scale option sets the spacing and Angle turns the lines, so two parts side by side can be hatched differently.",
        ctl: [],
        fig: hatchFig,
        math: () => ln(`${V_(aliasOf("HATCH"))} HATCH · pattern ${V_("ANSI31")} is the 45° steel hatch`) + ln(`spacing about ${V_(nu(3))} on paper`) + ln("Different parts in one section get different angles.", "dim"),
        task: {
          setup: { label: "Set up the cut plate", run: () => placeOnDrawing("plate with a hole, to hatch", (d) => d.layer("VISIBLE", 7, () => { const v = hatchVals(); d.rect(0, 0, v.W, v.H); d.circle(v.W / 2, v.H / 2, v.d); }), { raw: true }) },
          do: () => (T0() ? [`Type ${kbd("H")} and Enter.`, "Click on the plate between the hole and the edge.", "Press Enter to finish. The hole should stay clear."] : [needSetup("Set up the cut plate")]),
          check: (t) => {
            if (!t.o) return [{ ok: false, t: "Set up the cut plate first." }];
            const hs = fresh(t, entities()).filter((g) => g.type === "HATCH");
            if (!hs.length) return [{ ok: false, t: "No hatch yet: H, then click inside the plate." }];
            const around = hs.find((g) => g.loops >= 2);
            return [{ ok: true, t: "The plate is hatched" }, { ok: !!around, t: around ? "The hole is left clear" : "The hatch fills the hole too: click between the hole and the edge, not inside the hole." }];
          },
          show: () => placeOnDrawing("hatched plate", (d) => { const v = hatchVals(); d.layer("VISIBLE", 7, () => { d.rect(0, 0, v.W, v.H); d.circle(v.W / 2, v.H / 2, v.d); }); d.exec(`hatch ANSI31 ${d.n(1)} 45 0 ${d.pt(v.W * 0.12, v.H * 0.12)}`); }, { raw: true }),
        },
      },
      {
        t: "Name it",
        p: "Letter the cutting-plane line at both ends, A and A, and write SECTION A-A under the view. A second section on the same sheet uses B-B.",
        ctl: [],
        fig: () => sectionFig(true, true),
        math: () => ln(`label: ${V_("SECTION A-A")}, on NOTES`),
      },
    ],
  };

  // ================================================================ LESSON: DIMENSIONING IN BLACKCAD

  function dimTypesFig() {
    const x0 = 140, y0 = 300, W = 320, H = 180, ch = 50, r = 34;
    const o = [[x0, y0], [x0 + W, y0], [x0 + W, y0 - H + ch], [x0 + W - ch, y0 - H], [x0, y0 - H]];
    let g = path(poly(o)) + circle(x0 + 110, y0 - 90, r);
    g += dim(x0, y0 + 30, x0 + W, y0 + 30, "g");
    label(x0 + W / 2, y0 + 22, "DIMLINEAR", "g", 16, "middle");
    g += dim(x0 - 30, y0, x0 - 30, y0 - H, "g");
    label(x0 - 38, y0 - H / 2, "DIMLINEAR", "g", 16, "end");
    const nx = 22 * Math.SQRT1_2, ny = -22 * Math.SQRT1_2;
    g += dim(o[2][0] + nx, o[2][1] + ny, o[3][0] + nx, o[3][1] + ny, "g");
    label(o[3][0] + 34, o[3][1] - 12, "DIMALIGNED", "g", 16);
    g += path(`M ${x0 + 110 - r * 0.7} ${y0 - 90 + r * 0.7} L ${x0 + 110 + r * 0.7} ${y0 - 90 - r * 0.7} l 40 -30 h 30`, "g");
    label(x0 + 110 + r * 0.7 + 74, y0 - 90 - r * 0.7 - 24, "DIMDIAMETER", "g", 16);
    return svg(g, "A plate with a chamfered corner and a hole, dimensioned with horizontal and vertical linear dimensions, an aligned dimension on the chamfer and a diameter on the hole.", "-20 60 610 320");
  }
  function dimSnapFig() {
    let g = path(rectD(150, 120, 300, 130));
    g += smark("END", 150, 250) + smark("END", 450, 250);
    g += path("M 150 262 V 312 M 450 262 V 312", "g") + dim(150, 302, 450, 302, "g");
    label(300, 294, "300.00", "g", 18, "middle");
    g += arrow(300, 330, 300, 360, "a");
    label(300, 384, "pull down: horizontal", "a", 16, "middle");
    g += arrow(120, 185, 80, 185, "a");
    label(74, 160, "pull left:", "a", 16, "end");
    label(74, 210, "vertical", "a", 16, "end");
    return svg(g, "A dimension started on two snapped endpoints and pulled below the part so it reads horizontally; pulling it to the side would make it vertical.", "0 100 520 300");
  }

  function dimExtraFig() {
    let g = path("M 100 300 H 360 L 240 160 Z");
    g += path("M 170 300 A 70 70 0 0 0 150 253", "g");
    label(176, 272, "43°", "g", 18);
    g += path("M 300 250 L 390 190 H 480", "g");
    label(396, 180, "NOTE", "g", 17);
    g += path("M 100 330 V 360 M 200 330 V 380 M 330 330 V 400", "w");
    g += dim(100, 352, 200, 352, "g") + dim(100, 372, 330, 372, "g");
    label(150, 346, "DCO", "g", 14, "middle");
    label(215, 392, "DBA", "g", 14, "middle");
    return svg(g, "A triangle with an angle dimension and a leader note, and two dimensions measured from the same starting edge.", "60 130 460 290");
  }
  const baseVals = () => ({ L: xv(100, 4), H: xv(40, 1.5), d: xv(10, 0.375), y: xv(20, 0.75), xs: [xv(20, 0.75), xv(50, 2), xv(80, 3.25)] });
  const dimTaskVals = () => ({ L: xv(100, 4), H: xv(60, 2.5), d: xv(20, 0.75), hx: xv(30, 1.25), hy: xv(30, 1.25) });
  function dimPlate(d, v) {
    d.layer("VISIBLE", 7, () => {
      d.rect(0, 0, v.L, v.H);
      d.circle(v.hx, v.hy, v.d);
    });
    const e = v.d / 2 + xv(4, 0.15);
    d.layer("CENTER", 2, () => {
      d.line(v.hx - e, v.hy, v.hx + e, v.hy);
      d.line(v.hx, v.hy - e, v.hx, v.hy + e);
    });
  }
  const DIMCMD = {
    id: "d-dimcmd",
    title: "Dimensioning in BlackCAD",
    q: "Which dimension command do I use?",
    blurb: "DIM, DIMLINEAR, DIMALIGNED, DIMRADIUS, DIMDIAMETER",
    src: "BlackCAD's dimension commands; dimensions are placed on the DIMENSION layer by the core.",
    steps: [
      {
        t: "Five ways to dimension",
        p: "DIM does most jobs: pick a circle for its diameter, an arc for its radius, or two points for a straight size. The others make one kind only. Every dimension lands on the DIMENSION layer by itself.",
        ctl: [],
        fig: dimTypesFig,
        math: () => ["DIM", "DIMLINEAR", "DIMALIGNED", "DIMRADIUS", "DIMDIAMETER"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join(""),
      },
      {
        t: "Snap to the geometry",
        p: "Start every dimension on a snapped point: an endpoint, a centre or an intersection. Then it measures the geometry exactly. Place the dimension line by moving away from the part: above or below gives a horizontal size, left or right a vertical one. DIMLINEAR offers Aligned while you place it.",
        ctl: [],
        fig: dimSnapFig,
        math: () => ln("A dimension shows the measured size; you never type the number."),
      },
      {
        t: "Angles, stacks and leaders",
        p: "DAN (DIMANGULAR) dimensions the angle between two lines, an arc, or a vertex and two points, and writes the degree sign for you. DBA (DIMBASELINE) adds the next dimension from the same starting edge, stepping outward; DCO (DIMCONTINUE) chains the next one on from where the last stopped. LE (LEADER) points a note at a feature with an arrow, and MLD (MLEADER) draws the arrow and a landing line.",
        ctl: [],
        fig: dimExtraFig,
        math: () => ["DIMANGULAR", "DIMBASELINE", "DIMCONTINUE", "LEADER", "MLEADER"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join(""),
        task: {
          setup: { label: "Set up the plate", run: () => placeOnDrawing("plate with three holes", (d) => d.layer("VISIBLE", 7, () => { const v = baseVals(); d.rect(0, 0, v.L, v.H); v.xs.forEach((x) => d.circle(x, v.y, v.d)); }), { raw: true }) },
          do: () => (T0() ? [`${kbd("DL")}: snap the plate's lower left corner, then the first hole's centre (CEN). Place the dimension below the plate.`, `${kbd("DBA")}: click the second hole's centre, then the third. Each new dimension starts from the same left edge.`, "Press Enter to finish."] : [needSetup("Set up the plate")]),
          check: (t) => {
            if (!t.o) return [{ ok: false, t: "Set up the plate first." }];
            const v = baseVals();
            const ds = fresh(t, entities()).filter((g) => g.type === "DIMENSION_LINEAR");
            const got = v.xs.filter((x) => ds.some((g) => same(+g.measurement, x)));
            return [{ ok: got.length === v.xs.length, t: got.length === v.xs.length ? `The three holes are dimensioned from the left edge: ${v.xs.map(xs).join(", ")}` : `${got.length} of 3 distances found: ${v.xs.map(xs).join(", ")} from the left edge.` }];
          },
          show: () => placeOnDrawing("stacked dimensions", (d) => { const v = baseVals(); d.layer("VISIBLE", 7, () => { d.rect(0, 0, v.L, v.H); v.xs.forEach((x) => d.circle(x, v.y, v.d)); }); const g1 = xv(10, 0.4), g2 = xv(7, 0.28); v.xs.forEach((x, i) => d.dimH(0, v.y, x, v.y, -g1 - i * g2)); }, { raw: true }),
        },
      },
      {
        t: "Your turn: dimension a plate",
        p: "The board draws a plate with a hole. Give it its length and height, the hole's diameter, and the hole's position from the left and bottom edges.",
        ctl: [],
        fig: () => plateLayersFig({ note: false }),
        math: () => {
          const v = dimTaskVals();
          return ln(`plate ${V_(`${xs(v.L)} × ${xs(v.H)}`)} · hole ${V_(`Ø${xs(v.d)}`)} at ${V_(`${xs(v.hx)}, ${xs(v.hy)}`)} from the corner`);
        },
        task: {
          setup: { label: "Set up the plate", run: () => placeOnDrawing("plate to dimension", (d) => dimPlate(d, dimTaskVals()), { raw: true }) },
          do: () => [`${kbd("DL")}: snap the two bottom corners, place it below. Then the two right-hand corners, place it to the right.`, `${kbd("DL")}: the left edge's corner to the hole centre (CEN), placed below; and the bottom corner to the centre, placed to the left.`, `${kbd("D")}: click the circle, and place the diameter outside the plate.`],
          check: (t) => {
            const es = fresh(t, entities()).filter((g) => /^DIMENSION/.test(g.type));
            const lin = es.filter((g) => g.type === "DIMENSION_LINEAR" || g.type === "DIMENSION_ALIGNED").length;
            const dia = es.filter((g) => g.type === "DIMENSION_DIAMETER").length;
            const off = es.filter((g) => lay(g) !== "DIMENSION").length;
            return [
              { ok: lin >= 4, t: lin >= 4 ? `${lin} linear dimensions` : `${lin} of 4 linear dimensions: length, height and the hole's two positions.` },
              { ok: dia >= 1, t: dia ? "Diameter on the hole" : "No diameter dimension yet: D, then click the circle." },
              ...(es.length ? [{ ok: !off, t: off ? `${off} dimension${off === 1 ? " is" : "s are"} off the DIMENSION layer.` : "All on the DIMENSION layer" }] : []),
            ];
          },
          show: () => placeShownPlate(),
        },
      },
    ],
  };

  /** A plate dimensioned the way the rules lesson recommends: from one corner. */
  function placeShownPlate() {
    placeOnDrawing("dimensioned plate", (d) => {
      const v = dimTaskVals(), g1 = xv(10, 0.4), g2 = xv(7, 0.28);
      dimPlate(d, v);
      d.dimH(0, 0, v.hx, v.hy, -g1);
      d.dimH(0, 0, v.L, 0, -g1 - g2);
      d.dimV(0, 0, v.hx, v.hy, -g1);
      d.dimV(0, 0, 0, v.H, -g1 - g2);
      d.dimDia(v.hx, v.hy, v.d, 45);
    }, { raw: true });
  }

  // ================================================================ LESSON: DIMENSIONING RULES

  function chainFig() {
    const nS = Math.round(s.nseg), x0 = 70, W = 460, y0 = 270, step_ = W / nS;
    let g = path(`M ${x0} ${y0} H ${x0 + W}`, "w", "thick");
    for (let i = 0; i <= nS; i++) g += path(`M ${f1(x0 + i * step_)} ${y0 - 14} V ${y0 + 4}`);
    for (let i = 0; i < nS; i++) {
      const last = i === nS - 1;
      g += dim(x0 + i * step_, 200, x0 + (i + 1) * step_, 200, last ? "r" : "g");
      label(x0 + (i + 0.5) * step_, 192, last ? "×" : "✓", last ? "r" : "g", 18, "middle");
    }
    g += dim(x0, 140, x0 + W, 140, "g");
    label(x0 + W / 2, 132, "overall", "g", 18, "middle");
    label(x0, 330, `${nS} steps + overall = one too many: leave one step out`, "r", 18);
    return svg(g, "A row of step dimensions with an overall dimension above; the last step is crossed out because it repeats a size.", "40 100 520 250");
  }
  function spacingFig() {
    let g = path(rectD(120, 90, 300, 110));
    g += path("M 120 210 V 330 M 260 210 V 290 M 420 210 V 330", "w");
    g += dim(120, 250, 260, 250, "g") + dim(120, 310, 420, 310, "g");
    g += dim(460, 200, 460, 250, "a") + dim(500, 250, 500, 310, "a");
    label(470, 230, nt(10), "a", 16);
    label(510, 286, nt(7), "a", 16);
    label(120, 70, "short sizes nearest the part, long ones outside", "b", 17);
    return svg(g, "A part with two rows of dimensions below it: the shorter one nearer the part, about 10 away, and the longer one about 7 further out.", "60 50 500 300");
  }
  function whereFig() {
    let g = circle(160, 200, 50) + lt("M 100 200 H 220 M 160 140 V 260", "y", "center", 1.5);
    g += path("M 195 165 l 50 -50 h 40", "g");
    label(250, 106, "Ø where it's round", "g", 17);
    g += mark(320, 104, true);
    g += path(rectD(380, 150, 140, 100)) + lt("M 430 150 V 250 M 470 150 V 250", "r", "hidden", 1.6);
    g += dim(430, 120, 470, 120, "r");
    label(450, 110, "to hidden lines", "r", 16, "middle");
    g += mark(450, 285, false);
    return svg(g, "On the left, a hole's diameter dimensioned in the view where it's round, marked good; on the right, a dimension between hidden lines, marked wrong.", "70 80 480 230");
  }
  const DIMRULES = {
    id: "d-dimrules",
    title: "Dimensioning rules",
    q: "Where do the dimensions go?",
    blurb: "Once each, outside the part, where the feature shows",
    src: "General dimensioning rules per ASME Y14.5 and ISO 129-1 (each feature dimensioned once, no duplicate or closed chains); row spacing is common practice.",
    steps: [
      {
        t: "Each size once",
        p: "Give every size once, in one place. A row of steps that also has an overall size gives one size twice, and the shop can't tell which one wins. Leave one step out so it's clear which size is free to vary.",
        ctl: ["nseg"],
        fig: chainFig,
        math: () => ln(`dimension ${V_(Math.round(s.nseg) - 1)} of the ${Math.round(s.nseg)} steps, plus the overall`),
      },
      {
        t: "Outside the part, small inside large",
        p: "Put dimensions outside the outline where you can. Start the first row about 10 mm (3/8 in) from the part and space further rows about 7 mm (1/4 in) apart, with shorter sizes nearer the part so extension lines don't cross dimension lines.",
        ctl: [],
        fig: spacingFig,
        math: () => ln(`first row about ${V_(nu(inch() ? 9.525 : 10))} out · rows about ${V_(nu(inch() ? 6.35 : 7))} apart`) + ln("Leave a small gap between the part and each extension line.", "dim"),
        basis: "Spacing figures are common practice for 3.5 mm (1/8 in) text; check the standard your shop follows.",
      },
      {
        t: "Dimension where the feature shows",
        p: "Dimension a hole in the view where it's round, and a step in the view that shows its profile. Don't dimension to hidden lines: if a size only shows as hidden lines, add a view or a section.",
        ctl: [],
        fig: whereFig,
        math: () => ln("holes: Ø in the round view, located by their centres", "ok") + ln("never: to hidden lines", "bad"),
      },
      {
        t: "Work from one datum",
        p: "Choose one edge, corner or hole as the start and measure from it. Each feature is then only one tolerance away from the start. DBA (DIMBASELINE) stacks dimensions from that datum for you; DCO (DIMCONTINUE) chains them, which is what this lesson warns against. Shop's Dimensioning for fabrication lesson shows how chains add errors up.",
        ctl: [],
        fig: () => plateLayersFig({ note: false }),
        math: () => ln("datum: the lower-left corner, for example") + ln("hole positions measured from it, not from each other", "dim"),
        extra: () => `<div class="bb-row"><button class="chalkbtn" data-lesson="dims">Dimensioning for fabrication, in Shop</button></div>`,
        place: { label: "Draw a plate dimensioned this way", run: () => placeShownPlate() },
      },
    ],
  };

  // ================================================================ LESSON: THE SHEET

  const TB_FIELDS = () => [
    ["PART NAME", "BRACKET"],
    ["PART NO.", "P-0001"],
    ["MATERIAL", "MILD STEEL"],
    ["UNITS", du() === "in" ? "INCHES" : "MM"],
    ["SCALE", "1:1"],
    ["TOLERANCE", du() === "in" ? ".XX ±.01" : "ISO 2768-m"],
    ["DRAWN", (user() || "NAME").toUpperCase()],
    ["DATE", new Date().toISOString().slice(0, 10)],
    ["REV", "A"],
  ];
  function titleFig() {
    const x0 = 70, y0 = 90, w = 460, h = 210;
    let g = path(rectD(x0, y0, w, h));
    const rows = TB_FIELDS();
    const cols = 3, cw = w / cols, rh = h / 3;
    for (let i = 1; i < cols; i++) g += path(`M ${f1(x0 + i * cw)} ${y0} V ${y0 + h}`, "w");
    for (let j = 1; j < 3; j++) g += path(`M ${x0} ${f1(y0 + j * rh)} H ${x0 + w}`, "w");
    rows.forEach(([k, v], i) => {
      const cx = x0 + (i % cols) * cw + 10, cy = y0 + Math.floor(i / cols) * rh;
      label(cx, cy + 22, k, "b", 14);
      label(cx, cy + 52, esc(v), "w", 19);
    });
    label(x0, y0 - 16, "lower right corner of the sheet", "b", 17);
    return svg(g, "A title block divided into nine boxes: part name, part number, material, units, scale, tolerance, drawn by, date and revision.", "50 60 500 260");
  }
  function notesFig() {
    const notes = ["NOTES:", du() === "in" ? "1. DIMENSIONS IN INCHES." : "1. DIMENSIONS IN MM.", "2. MATERIAL: MILD STEEL, 3 THK.", "3. BREAK ALL SHARP EDGES.", "4. TOLERANCES PER TITLE BLOCK."];
    let g = "";
    notes.forEach((t, i) => label(90, 110 + i * 40, t, "c", 20));
    g += dim(70, 110 - 18, 70, 150 - 18, "a");
    label(62, 126, "1.6 h", "a", 15, "end");
    return svg(g, "A numbered list of general notes, each its own line of text, spaced 1.6 times the text height.", "20 70 520 230");
  }
  function revFig() {
    let g = path(rectD(70, 90, 460, 150));
    g += path("M 70 130 H 530 M 140 90 V 240 M 400 90 V 240", "w");
    label(84, 118, "REV", "b", 15);
    label(154, 118, "DESCRIPTION", "b", 15);
    label(414, 118, "DATE · BY", "b", 15);
    label(96, 168, "A", "w", 19);
    label(154, 168, "FIRST RELEASE", "w", 17);
    label(96, 212, "B", "a", 19);
    label(154, 212, "HOLE Ø10 WAS Ø8", "a", 17);
    label(414, 212, new Date().toISOString().slice(0, 10), "a", 15);
    return svg(g, "A revision table with revision A, first release, and revision B describing a changed hole size.", "50 70 500 190");
  }
  function filesFig() {
    const f = [[".cad", "BlackCAD's own"], [".dwg", "2018 format"], [".dxf", "exchange"], [".svg", "picture"]];
    let g = "";
    f.forEach(([e, w], i) => {
      const x = 80 + i * 120;
      g += path(`M ${x} 120 h 70 l 20 20 v 100 h -90 Z M ${x + 70} 120 v 20 h 20`, i ? "w" : "a");
      label(x + 45, 200, e, i ? "w" : "a", 22, "middle");
      label(x + 45, 270, w, "b", 15, "middle");
    });
    return svg(g, "Four file icons: .cad, BlackCAD's own format; .dwg in the 2018 format; .dxf for exchange; and .svg as a picture.", "60 100 500 190");
  }
  const SHEET = {
    id: "d-sheet",
    title: "Title block, notes and revisions",
    q: "What goes around the drawing?",
    blurb: "The title block, general notes, revisions, saving",
    src: "Title block content per ASME Y14.1 and ISO 7200; revisions per ASME Y14.35.",
    steps: [
      {
        t: "The title block",
        p: "The title block answers what every reader asks: what the part is, what it's made of, its units and scale, how accurate it must be, who drew it and which revision this is. It sits in the lower right corner of the sheet. In BlackCAD it is a block with a fill-in field for each answer (the Blocks lesson shows how), so you fill the fields once and ATE changes them later.",
        ctl: [],
        fig: titleFig,
        extra: sheetButtons,
        math: () => ln(`fields on ${V_("SHEET TITLE")}; change one later with ${V_("ATE")} or in Properties`) + ln("Try it makes a layout: border, a viewport at the best standard scale, and the title block.", "dim"),
        place: { label: "Put my drawing on a sheet with a title block", run: () => placeLayout() },
      },
      {
        t: "General notes",
        p: "Notes say what dimensions can't: units, material, finish, edge breaks and which tolerances apply. Number them and put them on NOTES. MT (MTEXT) writes the whole list as one object: drag a box, type each line and press Enter, and press Enter on an empty line to finish. LE (LEADER) points a note at the feature it is about.",
        ctl: [],
        fig: notesFig,
        math: () => ln(`text height ${V_(nu(inch() ? 3.175 : 3.5))} on paper; lines ${V_(nu((inch() ? 3.175 : 3.5) * 1.6))} apart`) + ln("Make NOTES current before you type them.", "dim") + ["MTEXT", "LEADER"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join(""),
      },
      {
        t: "Revisions",
        p: "When a released drawing changes, raise the revision letter: A, then B. A revision table says what changed, when and by whom, and the title block shows the current letter. A different part gets a new part number, never a revision.",
        ctl: [],
        fig: revFig,
        math: () => ln("first release: A") + ln("each change: next letter, and a line in the table", "dim"),
      },
      {
        t: "Save it so others can open it",
        p: "Save (Ctrl+S) keeps BlackCAD's own .cad format. Save as .dwg writes the 2018 format other drafting programs open, and DXF and SVG are in the file menu. PLOT (Ctrl+P) writes the sheet you are on as a PDF at its scale, and PRINT opens it for your printer; layers set not to plot, such as the viewport outline, stay off the page. Set Your name on saves, in the same menu, so every file records who saved it.",
        ctl: [],
        fig: filesFig,
        math: () => ln(`saves record: ${V_(esc(user() || "no name set"))}`) + ["PLOT", "PRINT"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c) === c ? "Ctrl+P" : aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join(""),
      },
    ],
  };

  /** The drawing on a sheet: the core makes the layout, the border, a viewport
   *  at the best standard scale and the title block (cad_core::sheet). */
  function placeLayout() {
    if (!doc) newDrawing(inch());
    if (!doc) return;
    const fields = TB_FIELDS().filter(([k]) => k !== "SCALE").map(([k, v]) => `${k}=${String(v).replace(/\|/g, "/")}`).join(" | ");
    try {
      core.exec(doc.id, `sheet ${s.sheet} | - | ${fields}`);
    } catch (e) {
      if (e instanceof CoreCrash) return crashed(e);
      return toast(e.message.replace(/^./, (c) => c.toUpperCase()), true);
    }
    const made = core.query(doc.id, "layouts").layouts.filter((l) => !l.model).at(-1);
    refresh(doc);
    if (made) switchSheetNow(made.name);
    checked = null;
    log(`Blackboard: ${made ? made.name : "sheet"} made. Undo takes it away.`, "ok");
    close();
    toast(`${made ? made.name : "Sheet"} made on ${s.sheet} paper. Undo takes it away.`);
  }

  // ================================================================ UNIT: SHAPE IT
  // Offset, trim, fillet, scale, arrays and blocks. Each tool does its work
  // in the core; the lesson only shows the picture and reads the result back.

  function offsetFig() {
    let g = path(rectD(110, 110, 250, 150)) + path(rectD(146, 146, 178, 78), "a");
    g += dim(110, 190, 146, 190, "g");
    label(86, 186, nt(xv(12, 0.5)), "g", 16, "end");
    g += arrow(300, 80, 250, 130, "a");
    label(306, 78, "click this side", "a", 17);
    label(110, 292, "O, the distance, the outline, then the side", "w", 17);
    return svg(g, "A rectangle with a smaller copy inside it, the same distance in from every edge, and a dimension showing that distance.", "70 50 400 270");
  }
  function trimFig() {
    let g = path("M 90 230 H 250") + path("M 250 130 V 230") + path("M 250 230 H 310", "r", "thin") + path("M 250 230 V 280", "r", "thin");
    g += path("M 430 230 H 590") + path("M 590 130 V 230");
    label(90, 112, "before: both ends overshoot", "w", 17);
    label(430, 112, "after TRIM", "g", 17);
    g += arrow(520, 292, 520, 250, "a");
    label(526, 312, "click the part to remove", "a", 16, "middle");
    return svg(g, "Two lines crossing with their overshoots in red, and the same two lines after trimming, meeting in a clean corner.", "60 80 560 250");
  }
  function filletFig() {
    let g = path("M 100 120 V 230 H 200") + path("M 280 120 V 200 A 30 30 0 0 0 310 230 H 390") + path("M 470 120 V 200 L 500 230 H 580");
    label(100, 270, "sharp", "w", 17);
    label(280, 270, "FILLET: radius 30", "a", 17);
    label(470, 270, "CHAMFER: 30 and 30", "c", 17);
    g += smark("CEN", 310, 200, "b", 6);
    return svg(g, "Three corners: a sharp one, one rounded by a fillet, and one cut off by a chamfer.", "70 80 540 220");
  }
  function scaleFig() {
    let g = path("M 120 270 v -70 h 70 v 70 Z") + dash("M 120 270 v -140 h 140 v 140 Z", "a");
    g += smark("END", 120, 270, "b", 8);
    label(120, 306, "base point: stays put", "b", 16);
    label(270, 160, "× 2", "a", 22);
    g += arrow(200, 215, 244, 171, "a");
    return svg(g, "A square scaled by a factor of two about its lower left corner: the corner stays where it is and the square doubles.", "80 100 360 220");
  }
  function arrayRectFig() {
    let g = "";
    for (let r = 0; r < 2; r++) for (let c = 0; c < 3; c++) g += circle(150 + c * 110, 140 + r * 90, 24, r === 0 && c === 0 ? "a" : "w");
    g += dim(150, 215 - 100, 260, 215 - 100, "g");
    label(205, 98, "column distance", "g", 16, "middle");
    g += dim(268, 140, 268, 230, "g");
    label(282, 190, "row distance", "g", 16);
    label(150, 270, "the first hole is the one you drew", "a", 16, "middle");
    return svg(g, "Six circles in two rows of three, the first one highlighted, with the column and row distances dimensioned from one hole to the next.", "90 70 440 230");
  }
  function arrayPolarFig() {
    const cx = 300, cy = 190, R = 100;
    let g = circle(cx, cy, R + 40) + lt(`M ${cx - 24} ${cy} H ${cx + 24} M ${cx} ${cy - 24} V ${cy + 24}`, "y", "center", 1.6);
    for (let k = 0; k < 6; k++) {
      const a = (k * Math.PI) / 3;
      g += circle(cx + R * Math.cos(a), cy - R * Math.sin(a), 16, k ? "w" : "a");
    }
    g += dash(circD(cx, cy, R), "b");
    label(cx + R + 26, cy + 6, "drawn first", "a", 16);
    label(cx, cy + 44, "centre point", "y", 16, "middle");
    label(cx, 360, "6 items over 360°", "w", 18, "middle");
    return svg(g, "A round plate with six holes evenly spaced on a bolt circle around a centre mark.", "130 30 340 350");
  }
  const offVals = () => ({ L: xv(80, 3), t: xv(12, 0.5) });
  const trimVals = () => ({ L: xv(60, 2.5), H: xv(40, 1.5), ov: xv(15, 0.6) });
  const filVals = () => ({ W: xv(80, 3), H: xv(50, 2), r: xv(8, 0.3125) });
  const scaVals = () => ({ A: xv(40, 1.5), k: 1.5 });

  const MODIFY = {
    id: "d-modify",
    title: "Offset, trim and fillet",
    q: "How do I build an outline from a few lines?",
    blurb: "OFFSET, TRIM, EXTEND, FILLET, CHAMFER, SCALE, STRETCH",
    src: "BlackCAD's modify commands, which run in the core (cad_core::modify, boundary and ops).",
    steps: [
      {
        t: "Offset: parallel lines",
        p: "Offset copies an object a set distance to one side. Type O, type the distance (or press Enter and click a point to go through), click the object, then click the side the copy goes on. It works on lines, circles, arcs and polylines, so a wall thickness or an inset is one command. Press Enter to finish.",
        ctl: [],
        fig: offsetFig,
        math: () => ln(`${V_(aliasOf("OFFSET"))} OFFSET · ${esc(COMMANDS.find((x) => x.name === "OFFSET").desc)}`) + ln("A circle offset inward gets smaller; outward, larger.", "dim"),
        task: {
          setup: { label: "Set up the line", run: () => placeOnDrawing("line to offset", (d) => d.line(0, 0, offVals().L, 0), { raw: true }) },
          do: () => {
            const v = offVals();
            if (!T0()) return [needSetup("Set up the line")];
            return [`Type ${kbd("O")} and Enter, then ${kbd(xs(v.t))} and Enter for the distance.`, "Click the line, then click anywhere above it.", "Press Enter to finish."];
          },
          check: (t) => {
            if (!t.o) return [{ ok: false, t: "Set up the line first." }];
            const v = offVals(), o = t.o;
            const ls = fresh(t, entities()).filter((g) => g.type === "LINE" && same(g.start[1], g.end[1]));
            const good = ls.find((g) => same(Math.abs(g.start[1] - o[1]), v.t) && same(Math.abs(g.end[0] - g.start[0]), v.L));
            if (good) return [{ ok: true, t: `A parallel copy ${xf(v.t)} from the line` }, { ok: true, t: "Same length as the original" }];
            const any = ls[0];
            return [{ ok: false, t: any ? `There is a new line, but it is ${xf(Math.abs(any.start[1] - o[1]))} away; it should be ${xf(v.t)}.` : "No new horizontal line yet." }];
          },
          show: () => placeOnDrawing("line and its offset", (d) => { d.line(0, 0, offVals().L, 0); d.line(0, offVals().t, offVals().L, offVals().t); }, { raw: true }),
        },
      },
      {
        t: "Trim and extend",
        p: "Trim cuts an object back to the next object that crosses it: type TR, then click the part you want gone. Every visible object counts as a cutting edge; the Edges option picks particular ones. Extend (EX) is the reverse: click the end of a line and it grows to the next object. Draw lines long, then trim them: that is quicker than snapping every end.",
        ctl: [],
        fig: trimFig,
        math: () => ["TRIM", "EXTEND"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join("") + ln("Esc or Enter finishes.", "dim"),
        task: {
          setup: { label: "Set up the overshoot", run: () => placeOnDrawing("two overlong lines", (d) => { const v = trimVals(); d.line(0, 0, v.L + v.ov, 0); d.line(v.L, -v.ov, v.L, v.H); }, { raw: true }) },
          do: () => (T0() ? [`Type ${kbd("TR")} and Enter.`, "Click the part of the horizontal line that sticks out past the vertical one.", "Click the part of the vertical line that hangs below the horizontal one, then press Enter."] : [needSetup("Set up the overshoot")]),
          check: (t) => {
            if (!t.o) return [{ ok: false, t: "Set up the overshoot first." }];
            const v = trimVals(), o = t.o;
            const ls = entities().filter((g) => g.type === "LINE");
            const lo = (g, i) => Math.min(g.start[i], g.end[i]), hi = (g, i) => Math.max(g.start[i], g.end[i]);
            const h = ls.find((g) => same(g.start[1], o[1]) && same(g.end[1], o[1]) && same(lo(g, 0), o[0]) && same(hi(g, 0), o[0] + v.L));
            const vt = ls.find((g) => same(g.start[0], o[0] + v.L) && same(g.end[0], o[0] + v.L) && same(lo(g, 1), o[1]) && same(hi(g, 1), o[1] + v.H));
            return [{ ok: !!h, t: h ? "The horizontal line stops at the corner" : "The horizontal line still runs past the corner." }, { ok: !!vt, t: vt ? "The vertical line starts at the corner" : "The vertical line still hangs below the corner." }];
          },
          show: () => placeOnDrawing("clean corner", (d) => { const v = trimVals(); d.line(0, 0, v.L, 0); d.line(v.L, 0, v.L, v.H); }, { raw: true }),
        },
      },
      {
        t: "Fillet and chamfer",
        p: "Fillet rounds the corner between two lines: type F, press R and type the radius, then click one line and the other. Chamfer (CHA) cuts the corner off instead; D sets its two distances. Press P to do every corner of a rectangle or polyline at once. A radius of 0 squares up two lines that don't quite meet.",
        ctl: [],
        fig: filletFig,
        math: () => ["FILLET", "CHAMFER"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join("") + ln("The radius you type stays for the next fillet.", "dim"),
        task: {
          setup: { label: "Set up the plate", run: () => placeOnDrawing("plate with square corners", (d) => d.layer("VISIBLE", 7, () => d.rect(0, 0, filVals().W, filVals().H)), { raw: true }) },
          do: () => {
            const v = filVals();
            return T0() ? [`Type ${kbd("F")} and Enter, then ${kbd("R")} and Enter, and type ${kbd(xs(v.r))} for the radius.`, `Press ${kbd("P")} and Enter, then click the plate.`] : [needSetup("Set up the plate")];
          },
          check: () => {
            const v = filVals();
            const p = entities().find((g) => g.type === "LWPOLYLINE" && g.vertices === 8 && g.closed);
            if (!p) return [{ ok: false, t: "The plate still has square corners (or isn't a single polyline)." }];
            const xs_ = p.points.map((q) => q[0]), ys = p.points.map((q) => q[1]);
            const x0 = Math.min(...xs_), y0 = Math.min(...ys);
            const r = Math.min(...p.points.filter((q) => same(q[0], x0)).map((q) => q[1] - y0).filter((d) => d > xtol()));
            return [{ ok: sizeIs({ w: Math.max(...xs_) - x0, h: Math.max(...ys) - y0 }, v.W, v.H), t: `${xf(v.W)} × ${xf(v.H)} plate` }, { ok: same(r, v.r), t: same(r, v.r) ? `All four corners rounded, radius ${xf(v.r)}` : `The corners are rounded with radius ${xf(r)}; they should be ${xf(v.r)}.` }];
          },
          show: () => placeOnDrawing("plate with rounded corners", (d) => { const v = filVals(); const h = d.rect(0, 0, v.W, v.H).handles[0]; d.exec(`fillet-pline ${d.n(v.r)} ${h}`); }, { raw: true }),
        },
      },
      {
        t: "Scale and stretch",
        p: "Scale (SC) resizes about a base point that stays put: select, click the base point, then type a factor (2 doubles, 0.5 halves). Reference is for when you know the length it is now and the length you want. Stretch (S) moves one end of an outline: cross that end with a window drawn right to left, pick a base point, and drag. What is inside the window moves; what crosses it stretches.",
        ctl: [],
        fig: scaleFig,
        math: () => ["SCALE", "STRETCH"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join("") + ln("Scale the geometry, never the dimensions: dimensions re-measure.", "dim"),
        task: {
          setup: { label: "Set up the square", run: () => placeOnDrawing("square to scale", (d) => d.rect(0, 0, scaVals().A, scaVals().A), { raw: true }) },
          do: () => {
            const v = scaVals();
            return T0() ? [`Type ${kbd("SC")} and Enter, click the square, then press Enter.`, "Snap the base point to the square's lower left corner (END).", `Type ${kbd(String(v.k))} and Enter.`] : [needSetup("Set up the square")];
          },
          check: (t) => {
            if (!t.o) return [{ ok: false, t: "Set up the square first." }];
            const v = scaVals(), o = t.o;
            const r = rectsOf(entities()).find((q) => sizeIs(q, v.A * v.k, v.A * v.k));
            if (!r) return [{ ok: false, t: `No ${xf(v.A * v.k)} square yet.` }];
            return [{ ok: true, t: `Scaled ${v.k} times to ${xf(v.A * v.k)}` }, { ok: same(r.x0, o[0]) && same(r.y0, o[1]), t: same(r.x0, o[0]) && same(r.y0, o[1]) ? "The base corner stayed put" : "The corner moved: the base point should be the lower left corner." }];
          },
          show: () => placeOnDrawing("scaled square", (d) => { const v = scaVals(); d.rect(0, 0, v.A * v.k, v.A * v.k); }, { raw: true }),
        },
      },
    ],
  };

  const arrVals = () => ({ d: xv(10, 0.375), px: xv(30, 1.25), py: xv(25, 1), hx: xv(15, 0.625), hy: xv(15, 0.625), rows: 2, cols: 3 });
  const polVals = () => ({ cx: xv(40, 1.5), cy: xv(40, 1.5), pcd: xv(60, 2.5), d: xv(6, 0.25), n: 6 });

  const ARRAYS = {
    id: "d-arrays",
    title: "Arrays: rows and circles of holes",
    q: "How do I repeat a hole without drawing it again?",
    blurb: "ARRAY: rectangular and polar",
    src: "BlackCAD's ARRAY command (cad_core::modify::array_rect and array_polar); the web rule is from Shop settings.",
    steps: [
      {
        t: "Rows and columns",
        p: "Draw one hole, then type AR, select it, press Enter and choose Rectangular. Type the rows and columns, then the two distances. A distance runs from a point on one copy to the same point on the next, so 30 between holes of Ø10 leaves 20 of metal. BlackCAD offers a spacing that clears what you selected; type your own.",
        ctl: [],
        fig: arrayRectFig,
        math: () => {
          const v = arrVals();
          return ln(`holes ${V_(`Ø${xs(v.d)}`)} at ${V_(xs(v.px))} leave ${V_(xs(+(v.px - v.d).toFixed(4)))} between them`) + ln("Rows are counted up the page, columns across it.", "dim");
        },
        task: {
          setup: { label: "Draw the first hole", run: () => placeOnDrawing("first hole", (d) => { const v = arrVals(); d.circle(v.hx, v.hy, v.d); }, { raw: true }) },
          do: () => {
            const v = arrVals();
            return T0() ? [`Type ${kbd("AR")} and Enter, click the hole, press Enter, and keep Rectangular.`, `${v.rows} rows, ${v.cols} columns.`, `Row distance ${kbd(xs(v.py))}, column distance ${kbd(xs(v.px))}.`] : [needSetup("Draw the first hole")];
          },
          check: (t) => {
            if (!t.o) return [{ ok: false, t: "Draw the first hole first." }];
            const v = arrVals(), o = t.o, cs = entities().filter((g) => g.type === "CIRCLE");
            let have = 0;
            for (let r = 0; r < v.rows; r++) for (let c = 0; c < v.cols; c++) if (cs.some((g) => samePt(g.center, [o[0] + v.hx + c * v.px, o[1] + v.hy + r * v.py]) && same(g.diameter, v.d))) have++;
            const n = v.rows * v.cols;
            return [{ ok: have === n, t: have === n ? `${v.rows} × ${v.cols} holes, ${xf(v.py)} by ${xf(v.px)} apart` : `${have} of ${n} holes are in the right place.` }];
          },
          show: () => placeOnDrawing("grid of holes", (d) => { const v = arrVals(); for (let r = 0; r < v.rows; r++) for (let c = 0; c < v.cols; c++) d.circle(v.hx + c * v.px, v.hy + r * v.py, v.d); }, { raw: true }),
        },
      },
      {
        t: "Holes around a circle",
        p: "For a bolt circle, type AR and choose Polar. Click the centre (snap it to the middle of the plate), type how many items, and the angle to fill: 360 for the whole way round. Rotate items on turns each copy to face the centre, which matters for slots and for holes with flats. Draw the first hole at the bolt-circle radius.",
        ctl: [],
        fig: arrayPolarFig,
        math: () => {
          const v = polVals();
          return ln(`bolt circle ${V_(`Ø${xs(v.pcd)}`)} · ${V_(v.n)} holes every ${V_(`${360 / v.n}°`)}`) + ln("Holes on a bolt circle are dimensioned by the circle's diameter, not each position.", "dim");
        },
        task: {
          setup: { label: "Set up the plate", run: () => placeOnDrawing("round plate with one hole", (d) => { const v = polVals(); d.circle(v.cx, v.cy, v.pcd + 2 * v.d * 2); d.circle(v.cx + v.pcd / 2, v.cy, v.d); }, { raw: true }) },
          do: () => {
            const v = polVals();
            return T0() ? [`Type ${kbd("AR")} and Enter, click the small hole, press Enter, and choose ${kbd("P")} for Polar.`, "For the centre point, snap to the centre of the big circle (CEN).", `${v.n} items, fill ${kbd("360")}, rotate items ${kbd("Y")}.`] : [needSetup("Set up the plate")];
          },
          check: (t) => {
            if (!t.o) return [{ ok: false, t: "Set up the plate first." }];
            const v = polVals(), o = t.o, cs = entities().filter((g) => g.type === "CIRCLE" && same(g.diameter, v.d));
            let have = 0;
            for (let k = 0; k < v.n; k++) {
              const a = (2 * Math.PI * k) / v.n;
              if (cs.some((g) => samePt(g.center, [o[0] + v.cx + (v.pcd / 2) * Math.cos(a), o[1] + v.cy + (v.pcd / 2) * Math.sin(a)]))) have++;
            }
            return [{ ok: have === v.n, t: have === v.n ? `${v.n} holes evenly spaced on a Ø${xf(v.pcd)} bolt circle` : `${have} of ${v.n} holes are on the bolt circle. Snap the centre to the big circle's centre.` }];
          },
          show: () => placeOnDrawing("bolt circle", (d) => { const v = polVals(); d.circle(v.cx, v.cy, v.pcd + 4 * v.d); for (let k = 0; k < v.n; k++) { const a = (2 * Math.PI * k) / v.n; d.circle(v.cx + (v.pcd / 2) * Math.cos(a), v.cy + (v.pcd / 2) * Math.sin(a), v.d); } }, { raw: true }),
        },
      },
      {
        t: "Check the spacing",
        p: "An array puts holes exactly where you asked, but it doesn't know the shop's rules. Holes too close to each other or to an edge crack or distort when cut. After any array, run SHOPCHECK, or press Check my drawing below, and fix what it flags.",
        ctl: [],
        fig: arrayRectFig,
        math: () => ln(`limits now: hole ≥ ${V_(num(rule.minDia()))}, web ≥ ${V_(num(rule.minWeb()))}, hole to bend ≥ ${V_(num(rule.holeBend()))}`),
        extra: () => `<div class="bb-row"><button class="chalkbtn" data-lesson="check">Check my drawing</button></div>`,
      },
    ],
  };

  function blocksFig() {
    const sym = (x, y, k, rot = 0, c = "w") => {
      const r = 18 * k, a = (rot * Math.PI) / 180, ex = r * 1.5 * Math.cos(a), ey = r * 1.5 * Math.sin(a), fx = -r * 1.5 * Math.sin(a), fy = r * 1.5 * Math.cos(a);
      return circle(x, y, r, c) + path(`M ${f1(x - ex)} ${f1(y + ey)} L ${f1(x + ex)} ${f1(y - ey)} M ${f1(x - fx)} ${f1(y + fy)} L ${f1(x + fx)} ${f1(y - fy)}`, c);
    };
    let g = sym(130, 170, 1.4, 0, "a") + sym(290, 170, 1) + sym(400, 170, 2, 0) + sym(520, 170, 1, 45);
    label(130, 240, "BLOCK HOLE", "a", 17, "middle");
    label(290, 240, "scale 1", "w", 16, "middle");
    label(400, 240, "scale 2", "w", 16, "middle");
    label(520, 240, "rotated 45°", "w", 16, "middle");
    g += arrow(180, 170, 250, 170, "a");
    return svg(g, "A hole symbol made into a block, then inserted at scale 1, at scale 2 and rotated 45 degrees.", "60 100 520 170");
  }
  function attFig() {
    let g = path(rectD(80, 120, 190, 70), "b") + label(94, 148, "PART_NAME", "b", 17) + label(94, 176, "tag, prompt, default", "b", 14);
    g += arrow(280, 155, 350, 155, "a");
    label(284, 142, "INSERT asks", "a", 14);
    g += path(rectD(360, 120, 190, 70), "w") + label(374, 156, "BRACKET", "w", 24);
    label(80, 100, "ATTDEF: a field inside a block", "w", 16);
    label(360, 100, "the value you typed", "w", 16);
    return svg(g, "An attribute definition showing its tag, and an arrow to the text that appears when the block is inserted.", "60 70 510 150");
  }
  const holeSym = (d, o = 0) => {
    const D = xv(12, 0.5), r = D / 2 + xv(3, 0.125);
    return [d.circle(o, o, D).handles[0], d.line(o - r, o, o + r, o).handles[0], d.line(o, o - r, o, o + r).handles[0]];
  };
  /** The HOLE block, placed once: made from a drawn symbol, or inserted if the drawing already has it. */
  const holeBlock = (d) => {
    if ((core.query(doc.id, "blocks")?.blocks || []).some((b) => b.name.toUpperCase() === "HOLE")) return d.exec(`insert ${d.pt(0, 0)} 1 0 | HOLE`);
    return d.exec(`block ${d.pt(0, 0)} convert ${holeSym(d).join(" ")} | HOLE`);
  };

  const BLOCKS = {
    id: "d-blocks",
    title: "Blocks and attributes",
    q: "How do I reuse part of a drawing?",
    blurb: "BLOCK, INSERT, ATTDEF, ATTEDIT, EXPLODE",
    src: "BlackCAD's block commands (cad_core::blocks): a block is stored once and placed as many times as needed.",
    steps: [
      {
        t: "Make a block",
        p: "A block packs several objects into one named thing you can place again and again. Type B, give it a name, click the base point (the point it will be placed by), then select the objects. The drawing stores the block once, so a symbol used fifty times is one set of geometry, not fifty. EXPLODE (X) breaks an insert back into its parts.",
        ctl: [],
        fig: blocksFig,
        math: () => ["BLOCK", "INSERT", "EXPLODE"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join("") + ln("Draw a block's parts on layer 0 if it should take the colour of the layer it's inserted on.", "dim"),
        task: {
          setup: { label: "Draw the symbol", run: () => placeOnDrawing("hole symbol", (d) => holeSym(d), { raw: true }) },
          do: () => (T0() ? [`Type ${kbd("B")} and Enter, and name it ${kbd("HOLE")}.`, "For the base point, snap to the circle's centre (CEN).", "Select the circle and both lines, then press Enter."] : [needSetup("Draw the symbol")]),
          check: () => {
            const b = (core.query(doc.id, "blocks")?.blocks || []).find((x) => x.name.toUpperCase() === "HOLE");
            const ins = entities().filter((g) => g.type === "INSERT" && String(g.block).toUpperCase() === "HOLE");
            return [{ ok: !!b, t: b ? "Block HOLE made" : "No block named HOLE yet." }, { ok: ins.length >= 1, t: ins.length ? "The symbol is now one object (an insert)" : "Nothing in the drawing uses the block yet." }];
          },
          show: () => placeOnDrawing("hole block", (d) => holeBlock(d), { raw: true }),
        },
      },
      {
        t: "Insert it",
        p: "I (INSERT) lists the blocks in the drawing. Type a name, click where it goes, then give a scale factor (2 doubles it) and a rotation. Every insert is one object you can move, copy and mirror, and its Properties show the block's name, scale and rotation.",
        ctl: [],
        fig: blocksFig,
        math: () => ln(`scale ${V_("1")} is full size · rotation ${V_("45")} turns it counter-clockwise`),
        task: {
          setup: { label: "Make the block", run: () => placeOnDrawing("hole block", (d) => holeBlock(d), { raw: true }) },
          do: () => (T0() ? [`Type ${kbd("I")} and Enter, name ${kbd("HOLE")}, click a point, then Enter twice for scale 1 and rotation 0.`, `${kbd("I")} again, ${kbd("HOLE")}, click another point, scale ${kbd("2")}, rotation ${kbd("45")}.`] : [needSetup("Make the block")]),
          check: () => {
            const ins = entities().filter((g) => g.type === "INSERT" && String(g.block).toUpperCase() === "HOLE");
            const big = ins.find((g) => same(+g.scale, 2, 0.001) && same(+g.rotation, 45, 0.01));
            return [{ ok: ins.length >= 3, t: ins.length >= 3 ? `${ins.length} inserts of HOLE` : `${ins.length} of 3 inserts (the one made with the block counts).` }, { ok: !!big, t: big ? "One at scale 2, rotated 45°" : "No insert at scale 2 and rotation 45 yet." }];
          },
        },
      },
      {
        t: "Attributes: fill-in fields",
        p: "An attribute is text that belongs to a block and asks for its value each time you insert it. ATT defines one: a tag (one word, such as PART_NAME), a prompt and a default. Put the attribute in the block with the other objects, and every INSERT asks for its text. ATE (ATTEDIT) changes the values later. The title block on a sheet works exactly this way.",
        ctl: [],
        fig: attFig,
        math: () => ["ATTDEF", "ATTEDIT"].filter(hasCmd).map((c) => ln(`${V_(aliasOf(c))} ${c} · ${esc(COMMANDS.find((x) => x.name === c).desc)}`)).join("") + ln("Tags are capitals with no spaces: PART_NAME, not Part name.", "dim"),
        task: {
          setup: { label: "Draw the circle", run: () => placeOnDrawing("circle for a numbered block", (d) => d.circle(0, 0, xv(12, 0.5)), { raw: true }) },
          do: () => (T0() ? [`${kbd("ATT")}: tag ${kbd("NUMBER")}, prompt ${kbd("Hole number")}, default ${kbd("1")}, click a point beside the circle, then Enter twice.`, `${kbd("B")}: name ${kbd("TAGGED")}, base point at the circle's centre, select the circle and the attribute text.`, `${kbd("I")}: ${kbd("TAGGED")}, click a point, Enter twice, then type a number.`] : [needSetup("Draw the circle")]),
          check: () => {
            const b = (core.query(doc.id, "blocks")?.blocks || []).find((x) => x.attributes.length);
            const ins = entities().filter((g) => g.type === "INSERT" && g.attributes?.length);
            return [{ ok: !!b, t: b ? `Block ${esc(b.name)} carries the field ${esc(b.attributes[0].tag)}` : "No block has an attribute yet." }, { ok: ins.length >= 1, t: ins.length ? "An insert holds a filled-in value" : "No insert with an attribute value yet." }];
          },
        },
      },
    ],
  };

  const DRAFTING_UNITS = [
    { name: "Start here: the tools", lede: "Commands, points, snaps and edits", lessons: [CMD, COORDS, SNAPS, EDIT] },
    { name: "Shape it", lede: "Offset, trim, fillet, arrays and blocks", lessons: [MODIFY, ARRAYS, BLOCKS] },
    { name: "Lines that talk", lede: "Layers, hidden lines and centre lines", lessons: [LAYERS, LINES] },
    { name: "Size and scale", lede: "Units, decimals and the sheet", lessons: [UNITS_, SCALE_] },
    { name: "Views", lede: "Seeing a part from every side", lessons: [VIEWS, SECTIONS] },
    { name: "Dimensions", lede: "Sizes the reader can trust", lessons: [DIMCMD, DIMRULES] },
    { name: "The sheet", lede: "What goes around the drawing", lessons: [SHEET] },
  ];
  // ================================================================ THE CURRICULUM
  // Subjects are the board's tabs. Shop is taught now; the others show their
  // syllabus so learners (and the people building lessons) can see where the
  // school is going.

  const CHECK = {
    id: "check",
    title: "Check my drawing",
    q: "Will the shop send this back?",
    blurb: "Holes against your shop's rules",
    src: "Uses the same rules as the lessons, from Shop settings.",
    steps: [
      {
        t: "Check my drawing",
        p: "The check measures every circle on the open drawing: too small for the process, too close to another hole, too close to an edge or cut-out, or too close to a bend. Lines on a layer named BEND count as bends.",
        ctl: ["T", "R"],
        fig: hfig5,
        math: () => checkMath(),
        extra: () => procButtons() + checkButtons(),
        basis: () => `Smallest hole ${trim(PR().hole)} × T, web ${trim(PR().web)} × T (${procName()}), hole to bend ${trim(shop.holeBend)} × T + R. Change them in Shop settings.`,
        place: { label: "Draw a test plate on my drawing", run: () => placeTestPlate() },
      },
    ],
  };

  const SUBJECTS = [
    {
      id: "drafting",
      name: "Drafting",
      title: "Drafting: drawings anyone can build from",
      lede: "Learn BlackCAD and the language of drawings together: commands and snaps, layers that mean something, views, dimensions and the sheet. Each lesson ends with something to draw, and the board checks it.",
      units: DRAFTING_UNITS,
    },
    {
      id: "shop",
      name: "Shop",
      title: "Shop: cut, bend, weld",
      lede: "The rules a fabricator checks before your part gets made. Every number moves, and every rule comes from your Shop settings.",
      units: [
        { name: "Forming", lede: "Bend it right the first time", lessons: [BEND, BD, DIE, GRAIN, HOLES, CORNERS] },
        { name: "Cutting", lede: "Laser and plasma, from the designer's side", lessons: [KERF, SMALL, LEAD, HEAT, EDGE] },
        { name: "Drawings for the shop", lede: "Dimensions and tolerances they can build to", lessons: [DIMS, TOL] },
        { name: "Weld and assembly", lede: "Edges that weld, parts that line up", lessons: [FITUP, TAB] },
      ],
    },
    {
      id: "electronics",
      name: "Electronics",
      title: "Electronics: hardware around the board",
      lede: "The mechanical side of electronics: boards that fit, panels that line up with connectors, and enclosures that stay cool.",
      syllabus: [
        { name: "Boards", lessons: [["Board outlines and mounting holes", "Hole sizes, keep-outs and where screws can go"], ["Clearances and keep-outs", "Room for parts, solder and screwdrivers"]] },
        { name: "Enclosures", lessons: [["Panel cutouts", "Connectors, switches and displays that line up"], ["Standoffs and stack heights", "Spacing boards and panels"], ["Vent slots and airflow", "Letting heat out without letting fingers in"]] },
        { name: "Heat", lessons: [["Heat sinks and thermal paths", "Getting heat from a part to the case"]] },
        { name: "Wiring", lessons: [["Glands, grommets and strain relief", "Cables through sheet metal"], ["Grounding studs and bonding", "Bare metal where it counts"]] },
      ],
    },
    {
      id: "machining",
      name: "Machining",
      hidden: true, // an outline for a later term; no tab yet
      title: "Machining: mill and lathe",
      lede: "Design parts a machinist can make in one setup: radii the cutter can reach, holes the drill can finish, fits that assemble.",
      syllabus: [
        { name: "Milling", lessons: [["Inside corners need a radius", "A round cutter can't cut a square corner"], ["Pockets and depth", "How deep a cutter can reach for its diameter"], ["Holes: drill points and depth", "The cone at the bottom and blind-hole depths"], ["Threads: tap drills and depth", "Tap drill sizes and how deep a thread really goes"]] },
        { name: "Turning", lessons: [["Shoulders, grooves and undercuts", "Where the tool has to stop, and what it leaves"], ["Chamfers and edge breaks", "Sharp edges, and how to call them out"]] },
        { name: "Fits and finish", lessons: [["Fits and limits", "Hole and shaft tolerances that slide, locate or press"], ["Surface finish", "Reading and writing roughness callouts"]] },
        { name: "Speeds and feeds", lessons: [["Cutting speed for the designer", "Why material choice changes the cost of a part"]] },
      ],
    },
  ];
  const SHOWN = SUBJECTS.filter((sj) => !sj.hidden);
  const LESSONS = [...SUBJECTS.filter((sj) => sj.units).flatMap((sj) => sj.units.flatMap((u) => u.lessons)), CHECK];
  const subjectOf = (id) => (id === "check" ? SUBJECTS.find((sj) => sj.id === "shop") : SUBJECTS.find((sj) => sj.units?.some((u) => u.lessons.some((l) => l.id === id)))) || SHOWN[0];
  let LESSON = CMD;
  let subject = "drafting";

  // ================================================================ CHECKING THE OPEN DRAWING

  let checked = null;
  /** Drawing units per millimetre. */
  function unitScale() {
    const u = doc?.info.insunits;
    return { 1: 1 / 25.4, 2: 1 / 304.8, 4: 1, 5: 0.1, 6: 0.001 }[u] ?? (doc?.imperial ? 1 / 25.4 : 1);
  }
  /** Check the holes on the sheet shown against the shop's limits: the
   *  core measures (cad_core::check), the board explains. */
  function runCheck() {
    if (!doc) return toast("Open or start a drawing first.", true);
    let r;
    try {
      r = core.query(doc.id, `check-holes ${N(rule.minDia())} ${N(rule.minWeb())} ${N(rule.holeBend())}`);
    } catch (e) {
      return handleError(e);
    }
    const set = (a) => new Set(a);
    checked = { holes: r.holes, bends: r.bends, edges: r.edges, small: set(r.small), close: set(r.close), edge: set(r.edge), near: set(r.near), all: set(r.flagged), T: s.T, R: s.R, proc: s.proc, name: doc.name };
    log(`Blackboard check: ${r.holes} hole${r.holes === 1 ? "" : "s"}, ${checked.all.size} flagged.`, "ok");
    renderExtra();
  }

  function showOffenders() {
    if (!checked?.all.size || !doc) return;
    selection.clear();
    for (const h of checked.all) if (doc.disp.byHandle.has(h)) selection.add(h);
    selectionChanged();
    needsDraw = true;
    log(`Blackboard: ${selection.size} hole${selection.size === 1 ? "" : "s"} selected.`, "ok");
    close();
    toast(`${selection.size} hole${selection.size === 1 ? "" : "s"} selected. Move or resize them, then check again.`);
  }

  function checkMath() {
    const limits = `Limits now: hole ≥ ${V_(num(rule.minDia()))}, web ≥ ${V_(num(rule.minWeb()))}, hole to bend ≥ ${V_(num(rule.holeBend()))}.`;
    if (!checked) return ln(doc ? "Press Check my drawing." : "Open or start a drawing, then check it.", "dim") + ln(limits, "dim");
    const c = checked;
    if (!c.holes) return ln("No holes found.", "bad") + ln("Draw circles with CIRCLE, then check again.", "dim");
    const stale = c.T !== s.T || c.R !== s.R || c.proc !== s.proc;
    const row = (set, text) => (set.size ? ln(`<span class="bad">${set.size}</span> ${text}`) : "");
    let h = ln(`${c.holes} hole${c.holes === 1 ? "" : "s"} in ${esc(c.name)} · ${PROCESSES[c.proc] ? shop.procs[c.proc].name : ""} · T ${num(c.T)} · R ${num(c.R)}`, "dim");
    h += row(c.small, `smaller than ${num(rule.minDia())}`) + row(c.close, `closer than ${num(rule.minWeb())} to another hole`) + row(c.edge, `closer than ${num(rule.minWeb())} to an edge or cut-out`) + row(c.near, `within ${num(rule.holeBend())} of a bend`);
    if (!c.all.size) h += ln("All clear.", "ok");
    if (!c.bends) h += ln("No lines on a layer named BEND, so bend distances were skipped.", "dim");
    if (stale) h += warnLn("The numbers changed since the check. Check again.");
    return h;
  }
  const checkButtons = () => `<div class="bb-row"><button class="chalkbtn" data-act="check">Check my drawing</button>${checked?.all.size ? `<button class="chalkbtn" data-act="show">Show them on the drawing</button>` : ""}</div>`;

  // ================================================================ TRY IT ON THE DRAWING
  // Lesson geometry is placed as real objects through the same command
  // language every tool uses, beside whatever is already drawn, as one undo
  // step. Outlines go on the current layer, bend lines on BEND, words on NOTES.

  /** opts.at: the origin in drawing units (default: beside what's drawn).
   *  opts.raw: the build works in drawing units rather than millimetres. */
  function placeOnDrawing(what, build, opts = {}) {
    if (!core) return null;
    if (!doc) newDrawing(inch());
    if (!doc) return null;
    const k0 = unitScale();
    const k = opts.raw ? 1 : k0;
    const b = doc.disp.bounds;
    const ox = opts.at ? opts.at[0] : b ? b[2] + 25 * k0 : 0, oy = opts.at ? opts.at[1] : b ? b[1] : 0;
    const X = (x) => N(ox + x * k), Y = (y) => N(oy + y * k), L = (v) => N(v * k);
    const textH = (inch() ? 3.175 : 3.5) * k0;
    const prev = doc.info.layers.find((l) => l.current);
    const layers = new Map(doc.info.layers.map((l) => [l.name.toUpperCase(), l.handle]));
    const exec = (line) => core.exec(doc.id, line);
    const onLayer = (name, aci, fn) => {
      let h = layers.get(name);
      if (!h) {
        h = exec(`layer-add ${name}`).handles[0];
        layers.set(name, h);
        if (aci) exec(`layer-color ${h} ${aci}`);
      }
      exec(`layer-current ${h}`);
      try {
        fn();
      } finally {
        if (prev) exec(`layer-current ${prev.handle}`);
      }
    };
    const api = {
      line: (x1, y1, x2, y2) => exec(`line ${X(x1)} ${Y(y1)} ${X(x2)} ${Y(y2)}`),
      rect: (x1, y1, x2, y2) => exec(`rect ${X(x1)} ${Y(y1)} ${X(x2)} ${Y(y2)}`),
      circle: (x, y, d) => exec(`circle ${X(x)} ${Y(y)} ${L(d / 2)}`),
      pline: (pts, closed = true) => exec(`pline ${closed ? 1 : 0} ${pts.map(([x, y, bl = 0]) => `${X(x)} ${Y(y)} ${N(bl)}`).join(" ")}`),
      text: (x, y, words, scale = 1) => onLayer("NOTES", 4, () => exec(`text ${X(x)} ${Y(y)} ${N(textH * scale)} 0 ${words}`)),
      textAt: (x, y, h, words) => onLayer("NOTES", 4, () => exec(`text ${X(x)} ${Y(y)} ${L(h)} 0 ${words}`)),
      layer: (name, aci, fn) => onLayer(name, aci, fn),
      dimV: (x1, y1, x2, y2, lx) => exec(`dim-linear ${X(x1)} ${Y(y1)} ${X(x2)} ${Y(y2)} ${X(lx)} ${Y((y1 + y2) / 2)} 90`),
      dimDia: (cx, cy, d, ang = 45) => exec(`dim-diameter ${X(cx)} ${Y(cy)} ${L(d / 2)} ${ang}`),
      bend: (x1, y1, x2, y2) => onLayer("BEND", 2, () => exec(`line ${X(x1)} ${Y(y1)} ${X(x2)} ${Y(y2)}`)),
      notes: (fn) => onLayer("NOTES", 4, fn),
      dimH: (x1, y1, x2, y2, ly) => exec(`dim-linear ${X(x1)} ${Y(y1)} ${X(x2)} ${Y(y2)} ${X((x1 + x2) / 2)} ${Y(ly)} 0`),
      exec,
      pt: (x, y) => `${X(x)} ${Y(y)}`,
      n: (v) => L(v),
      textH: textH / k0,
      u: U().toUpperCase(),
    };
    try {
      exec("begin");
      try {
        build(api);
      } finally {
        exec("end");
      }
    } catch (e) {
      if (e instanceof CoreCrash) return crashed(e);
      toast(e.message || String(e), true);
      refresh(doc);
      return null;
    }
    refresh(doc);
    zoomExtents();
    checked = null;
    log(`Blackboard: ${what} drawn. One Undo takes it away.`, "ok");
    close();
    toast(`${what.charAt(0).toUpperCase() + what.slice(1)} drawn. Undo takes it away.`);
    return [ox, oy];
  }

  const numTxt = (v) => `${num(v)} ${U().toUpperCase()}`;
  const partNote = () => `T ${num(s.T)}  R ${num(s.R)}  K ${s.K.toFixed(2)}  ${Math.round(s.ang)} DEG`;

  /** A flat blank for outside legs [A, B] or [A, B, C, ...], bend lines on BEND. */
  function placeFlat(legs) {
    const c = calc();
    const parts = legs.map((v, i) => v - (i === 0 || i === legs.length - 1 ? 1 : 2) * c.OSSB);
    if (parts.some((v) => v < 0)) return toast("A leg is shorter than the setback. Make it longer first.", true);
    const W = s.W;
    const flat = legs.reduce((a, v) => a + v, 0) - (legs.length - 1) * c.BD;
    placeOnDrawing(legs.length > 2 ? "channel flat" : "flat blank", (d) => {
      d.rect(0, 0, flat, W);
      let x = 0;
      parts.forEach((v, i) => {
        x += v;
        if (i < parts.length - 1) {
          d.bend(x, 0, x, W);
          d.bend(x + c.BA, 0, x + c.BA, W);
          x += c.BA;
        }
      });
      d.dimH(0, 0, flat, 0, -10);
      d.text(0, W + 6, `FLAT ${numTxt(flat)}  ${partNote()}`);
    });
  }

  function placeHolePlate() {
    const d0 = Math.max(s.d, 1), m = 15;
    const xb = m + d0 + s.h, Lp = xb + 30, Wp = Math.max(40, d0 * 3);
    placeOnDrawing("plate with a hole near a bend", (d) => {
      d.rect(0, 0, Lp, Wp);
      d.circle(m + d0 / 2, Wp / 2, d0);
      d.bend(xb, 0, xb, Wp);
      d.text(0, Wp + 6, `HOLE EDGE TO BEND ${numTxt(s.h)}  LIMIT ${numTxt(rule.holeBend())}`);
    });
  }

  function placeRelief() {
    const BL = 60, H = 90, y1 = 25, y2 = 65, F = 30, w = s.w, dp = s.dp;
    placeOnDrawing("relieved flange", (d) => {
      d.pline([[0, 0], [BL, 0], [BL, y1 - w], [BL - dp, y1 - w], [BL - dp, y1], [BL + F, y1], [BL + F, y2], [BL - dp, y2], [BL - dp, y2 + w], [BL, y2 + w], [BL, H], [0, H]]);
      d.bend(BL, y1, BL, y2);
      d.text(0, H + 6, `RELIEF ${numTxt(w)} WIDE x ${numTxt(dp)} DEEP`);
    });
  }

  function placeGrainNote() {
    placeOnDrawing("grain arrow and note", (d) => {
      d.notes(() => {
        d.line(0, 0, 60, 0);
        d.line(60, 0, 52, 3);
        d.line(60, 0, 52, -3);
      });
      d.text(0, 4, "GRAIN");
      d.text(0, -10, "NOTE: BEND ACROSS GRAIN");
    });
  }

  /** A plate with one hole of each kind the check looks for. */
  function placeTestPlate() {
    const dmin = rule.minDia(), web = rule.minWeb(), hb = rule.holeBend();
    const D = Math.max(dmin * 1.6, 4), ds = Math.max(dmin * 0.6, 0.2);
    const Wp = Math.max(60, 20 + 3 * web + 3 * D);
    const x1 = 10 + web + D / 2; // fine
    const x2 = x1 + D / 2 + 2 * web + ds / 2; // too small
    const x3 = x2 + ds / 2 + 2 * web + D / 2; // a pair too close together, and one too close to the edge
    const x4 = x3 + D + web * 0.5;
    const x6 = x4 + D + 2 * web; // too close to the bend
    const xb = x6 + D / 2 + hb * 0.6;
    const Lp = xb + 30;
    placeOnDrawing("test plate", (d) => {
      d.rect(0, 0, Lp, Wp);
      d.circle(x1, Wp / 2, D);
      d.circle(x2, Wp / 2, ds);
      d.circle(x3, Wp - 10 - web - D / 2, D);
      d.circle(x4, Wp - 10 - web - D / 2, D);
      d.circle(x3, web * 0.5 + D / 2, D);
      d.circle(x6, Wp / 2, D);
      d.bend(xb, 0, xb, Wp);
      d.text(0, Wp + 6, `TEST PLATE  ${procName().toUpperCase()}  T ${num(s.T)}`);
    });
  }

  function placeDimPlate() {
    const nH = Math.round(s.n), p = s.pt, dd = Math.max(rule.minDia(), Math.min(p * 0.35, 12));
    const Lp = (nH + 1) * p, Wp = Math.max(30, dd * 3);
    placeOnDrawing("baseline-dimensioned plate", (d) => {
      d.rect(0, 0, Lp, Wp);
      for (let i = 1; i <= nH; i++) d.circle(i * p, Wp / 2, dd);
      for (let i = 1; i <= nH; i++) d.dimH(0, 0, i * p, Wp / 2, -8 - (i - 1) * 7);
    });
  }

  function placeTabSlot() {
    const c = PR().slot, sw = rule.slotW(), tw = s.tabw, sl = tw + 2 * c;
    const Lb = Math.max(80, tw * 3), Wb = 50;
    placeOnDrawing("tab-and-slot pair", (d) => {
      d.rect(0, 0, Lb, Wb);
      const sx = (Lb - sl) / 2, sy = Wb / 2 - sw / 2;
      d.rect(sx, sy, sx + sl, sy + sw);
      const y0 = Wb + 25, H = 40, tx = (Lb - tw) / 2;
      d.pline([[0, y0], [tx, y0], [tx, y0 - s.T], [tx + tw, y0 - s.T], [tx + tw, y0], [Lb, y0], [Lb, y0 + H], [0, y0 + H]]);
      d.text(0, y0 + H + 6, `SLOT ${numTxt(sl)} x ${numTxt(sw)}  TAB ${numTxt(tw)} x ${numTxt(s.T)}`);
    });
  }

  // ================================================================ SHOP SETTINGS

  const SET_ROWS = {
    proc: [
      ["kerf", "Kerf width", "len", "Measure a test slot and use that."],
      ["hole", "Smallest hole", "xT", "From your cut chart for this process."],
      ["web", "Thinnest web", "xT", "Strip between two cuts, or a cut and the edge."],
      ["lead", "Lead-in length", "xT", "How far the cut runs in from the pierce."],
      ["leadMin", "Shortest lead-in", "len", "Floor for thin sheet."],
      ["pierce", "Pierce mark", "xk", "Crater size as a multiple of kerf."],
      ["tol", "Cut tolerance", "pm", "What a cut edge holds."],
      ["slot", "Tab-and-slot clearance", "len", "Each side of the tab."],
      ["bevel", "Edge bevel", "deg", "Lean on the scrap side."],
    ],
    forming: [
      ["holeBend", "Hole edge to bend", "xTR", "Times T, plus R."],
      ["slotBend", "Slot edge to bend", "xTR", "Times T, plus R."],
      ["relW", "Relief width", "xT", "Bend relief and corner relief."],
      ["relD", "Relief depth past the bend line", "RxT", "R plus this times T."],
      ["vRatio", "Die opening", "xT", "V-die opening as a multiple of T."],
      ["kShop", "Measured K-factor", "k", "0 picks K by R ÷ T."],
      ["bendTol", "Bend tolerance", "pm", "Per bend, on a flange length."],
      ["angTol", "Bend angle tolerance", "deg", "± on every bend angle."],
    ],
    mat: [
      ["air", "Air-bent radius", "xV", "Inside radius as a fraction of the die opening."],
      ["across", "Smallest R across the grain", "xT", "From the supplier's datasheet."],
      ["along", "Smallest R along the grain", "xT", "From the supplier's datasheet."],
      ["rho", "Density", "rho", "For weld-metal weight."],
    ],
    asm: [
      ["edge", "Loaded bolt: centre to edge", "xd", "Times the hole diameter."],
      ["buttMax", "Thickest square butt", "len", "Thicker than this gets a groove."],
      ["gapFrac", "Square-butt gap, at most", "xT", "Times thickness."],
    ],
  };
  const UNIT_TXT = { xT: "× T", xTR: "× T + R", RxT: "R + _ × T", xV: "× V", xk: "× kerf", xd: "× d", deg: "°", k: "", rho: "g/cm³" };
  const isLen = (kind) => kind === "len" || kind === "pm";
  function setTarget(scope) {
    return scope === "proc" ? shop.procs[settingsProc] : scope === "mat" ? shop.mats[s.mat] : shop;
  }
  let settingsProc = s.proc;

  function settingsHtml() {
    const row = (scope, [key, name, kind, hint]) => {
      const t = setTarget(scope);
      const v = isLen(kind) ? (inch() ? trim((t[key] / 25.4).toFixed(4)) : trim(t[key].toFixed(3))) : trim(t[key].toFixed(3));
      const u = isLen(kind) ? (kind === "pm" ? `± ${U()}` : U()) : UNIT_TXT[kind];
      return `<label class="set"><span class="nm">${name}<small>${hint}</small></span><input data-set="${scope}.${key}" data-kind="${kind}" inputmode="decimal" value="${v}" aria-label="${name}"><span class="u">${u}</span></label>`;
    };
    const sec = (title, scope, rows, head = "") => `<section class="sets"><h3>${title}</h3>${head}${rows.map((r) => row(scope, r)).join("")}</section>`;
    return (
      `<h2>Shop settings</h2><p class="lede">Your shop's own numbers. Every lesson and the drawing check read them from here. The defaults are common starting points; replace them with what your machines and materials actually do.</p>` +
      sec(`Cutting: ${shop.procs[settingsProc].name}`, "proc", SET_ROWS.proc, `<div class="bb-row">${buttons("setproc", Object.entries(shop.procs).map(([k, p]) => [k, p.name]), settingsProc)}</div>`) +
      sec("Forming", "shop", SET_ROWS.forming) +
      sec(`Material: ${MAT().name}`, "mat", SET_ROWS.mat, matButtons()) +
      sec("Weld and assembly", "shop", SET_ROWS.asm) +
      `<div class="bb-row"><button class="chalkbtn" data-act="print-card">Print the shop card</button><button class="chalkbtn" data-act="reset-shop">Put back the defaults</button></div>` +
      `<p class="note">Saved in this browser. Lesson sliders marked <i class="shoptag">shop</i> change these numbers too.</p>`
    );
  }

  function applySetting(input) {
    const [scope, key] = input.dataset.set.split(".");
    const kind = input.dataset.kind;
    const v = parseFloat(String(input.value).replace(",", "."));
    if (!Number.isFinite(v) || v < 0) return input.classList.add("bad");
    input.classList.remove("bad");
    setTarget(scope)[key] = isLen(kind) && inch() ? v * 25.4 : v;
    saveShop();
    checked = null;
    if (view === "settings") renderFig(true);
  }

  /** The shop card: every rule worked out for the part on the board. */
  function cardRows() {
    const c = calc();
    const p = PR(), m = MAT();
    return [
      ["Cutting", null],
      ["Smallest hole", len(rule.minDia()), `${trim(p.hole)} × T`],
      ["Thinnest web", len(rule.minWeb()), `${trim(p.web)} × T`],
      ["Kerf", len(p.kerf), "measured"],
      ["Pierce mark", len(rule.pierce()), `${trim(p.pierce)} × kerf`],
      ["Lead-in", len(rule.leadIn()), `${trim(p.lead)} × T, min ${num(p.leadMin)}`],
      ["Hole for a clean start", len(rule.pierce() + 2 * rule.leadIn()), "pierce + 2 × lead-in"],
      ["Inside corner radius", len(p.kerf / 2), "kerf ÷ 2"],
      ["Cut tolerance", `±${len(p.tol)}`, ""],
      ["Forming", null],
      ["Hole edge to bend", len(rule.holeBend()), `${trim(shop.holeBend)} × T + R`],
      ["Slot edge to bend", len(rule.slotBend()), `${trim(shop.slotBend)} × T + R`],
      ["Relief", `${num(rule.relW())} × ${len(rule.relD())}`, `${trim(shop.relW)}T wide, R + ${trim(shop.relD)}T deep`],
      ["Die opening", len(rule.V()), `${trim(shop.vRatio)} × T`],
      ["Air-bent radius", `≈ ${len(rule.dieR())}`, `${trim(m.air)} × V`],
      ["Shortest flange", len(rule.minFlange()), `at ${Math.round(s.ang)}°`],
      ["K-factor", suggestK().toFixed(2), shop.kShop > 0 ? "measured" : "by R ÷ T"],
      [`Bend deduction at ${Math.round(s.ang)}°`, len(c.BD), `K ${s.K.toFixed(2)}`],
      ["Smallest R, across / along grain", `${num(rule.minR(90))} / ${len(rule.minR(0))}`, `${trim(m.across)}T / ${trim(m.along)}T`],
      ["Bend tolerance", `±${len(shop.bendTol)}, ±${trim(shop.angTol)}°`, "per bend"],
      ["Assembly", null],
      ["Tab-and-slot slot width", len(rule.slotW()), `T + 2 × ${num(p.slot)}`],
      ["Loaded bolt, centre to edge", `${trim(shop.edge)} × d`, ""],
      ["Square butt up to", len(shop.buttMax), `gap ≤ ${trim(shop.gapFrac)} × T`],
    ];
  }
  const cardTitle = () => `${procName()} · ${MAT().name} · T ${len(s.T)} · R ${len(s.R)}`;
  function cardHtml() {
    return `<div class="card"><div class="card-h"><b>Shop card</b><span>${cardTitle()}</span></div><table>${cardRows()
      .map(([a, b, c]) => (b === null ? `<tr class="grp"><th colspan="3">${a}</th></tr>` : `<tr><td>${a}</td><td class="v">${b}</td><td class="r">${c}</td></tr>`))
      .join("")}</table></div>`;
  }

  // ================================================================ PRINTING
  // A print-only sheet in dark ink on white. The figures are the board's own.

  function printSheet(title, body, foot = "Starting points, not the rules of your shop. Replace them with your own measured numbers in Shop settings.") {
    let el = $("#print-sheet");
    if (!el) {
      el = document.createElement("div");
      el.id = "print-sheet";
      document.body.append(el);
    }
    el.innerHTML = `<header><span><b>BlackCAD</b> · Blackboard</span><span>${new Date().toLocaleDateString()}</span></header><h1>${title}</h1>${body}<footer>${foot}</footer>`;
    document.documentElement.classList.add("printing");
    const done = () => {
      document.documentElement.classList.remove("printing");
      removeEventListener("afterprint", done);
    };
    addEventListener("afterprint", done);
    setTimeout(() => {
      window.print();
      setTimeout(done, 500);
    }, 50);
  }
  function printCard() {
    printSheet("Shop card", `<p class="sub">${cardTitle()}</p>${cardHtml()}`);
  }
  function printTable() {
    const rows = brakeRows();
    const t = `<table class="grid"><tr><th>T</th><th>R</th><th>BA</th><th>OSSB</th><th>BD</th></tr>${rows.map((r) => `<tr><td>${num(r.T)}</td><td>${num(r.R)}</td><td>${num(r.BA)}</td><td>${num(r.OSSB)}</td><td><b>${num(r.BD)}</b></td></tr>`).join("")}</table>`;
    printSheet("Brake table", `<p class="sub">${U()} · R ÷ T ${(s.R / s.T).toFixed(2)} · K ${s.K.toFixed(2)} · ${Math.round(s.ang)}° bend</p>${t}`);
  }
  function printLesson() {
    const keep = step;
    printing = true;
    const parts = LESSON.steps.map((st, i) => {
      step = i;
      n = 0;
      words = [];
      const fig = st.fig();
      const basis = typeof st.basis === "function" ? st.basis() : st.basis || "";
      const task = st.task ? `<p><b>Your turn:</b></p><ol>${st.task.do().map((x) => `<li>${x}</li>`).join("")}</ol>` : "";
      return `<section class="pstep"><div class="pfig">${fig}</div><div><h2>${i + 1}. ${st.t}</h2><p>${textOf(st.p)}</p><div class="pmath">${st.math()}</div>${basis ? `<p class="pbasis">${basis}</p>` : ""}${task}</div></section>`;
    });
    step = keep;
    printing = false;
    const drafting = subjectOf(LESSON.id).id === "drafting";
    printSheet(LESSON.title, `<p class="sub">${LESSON.q} · ${drafting ? (du() === "in" ? "inches" : "millimetres") : cardTitle()}</p>${parts.join("")}`, drafting ? "From the BlackCAD Blackboard. Exercises assume their starting shapes at 0,0." : undefined);
  }

  // ================================================================ RENDERING

  const isLesson = () => !view.startsWith("tab:") && view !== "settings";
  const curSubject = () => SHOWN.find((sj) => sj.id === subject) || SHOWN[0];

  if (savedView === "settings") view = "settings";
  else if (typeof savedView === "string" && savedView.startsWith("tab:") && SHOWN.some((sj) => `tab:${sj.id}` === savedView)) {
    view = savedView;
    subject = savedView.slice(4);
  } else if (LESSONS.some((l) => l.id === savedView)) {
    view = savedView;
    LESSON = LESSONS.find((l) => l.id === savedView);
    step = Math.min(step, LESSON.steps.length - 1);
  }

  function sliderHtml(k) {
    const c = CTL[k];
    const useIn = inch() && !c.plain;
    const [lo, hi, st] = useIn ? c.in : c.mm;
    const v = useIn ? val(k) / 25.4 : val(k);
    return `<div class="ctl"><span class="sym">${c.sym}</span><span class="nm">${c.label}${c.shop ? ' <i class="shoptag" title="Saved in Shop settings">shop</i>' : ""}</span><button class="out" data-o="${k}" title="Type a value">${showK(k)}</button><input type="range" data-k="${k}" min="${lo}" max="${hi}" step="${st}" value="${v}" aria-label="${c.label}"></div>`;
  }
  function syncSliders() {
    textEl.querySelectorAll("input[data-k]").forEach((r) => {
      const k = r.dataset.k, c = CTL[k];
      const v = inch() && !c.plain ? val(k) / 25.4 : val(k);
      if (document.activeElement !== r) r.value = v;
      const o = textEl.querySelector(`[data-o="${k}"]`);
      if (o && o.tagName === "BUTTON") o.textContent = showK(k);
    });
  }

  function renderTabs() {
    const active = isLesson() ? subjectOf(LESSON.id).id : view === "settings" ? "" : subject;
    tabsEl.innerHTML =
      SHOWN.map((sj) => `<button class="bbtab${sj.id === active ? " on" : ""}" data-tab="${sj.id}" role="tab" aria-selected="${sj.id === active}">${sj.name}</button>`).join("") +
      `<button class="bbtab cfg${view === "settings" ? " on" : ""}" data-act="settings" title="Shop settings"><span class="long">Shop settings</span><span class="short">Settings</span></button>`;
  }

  function indexHtml() {
    const sj = curSubject();
    if (sj.units) {
      const all = sj.units.flatMap((u) => u.lessons);
      const nDone = all.filter((l) => done.has(l.id)).length;
      return (
        `<div class="bb-ix-head"><div><h2>${sj.title}</h2><p class="lede">${sj.lede}</p></div>` +
        `<div class="bb-tools">${toolsHtml(sj)}<span class="prog">${nDone} of ${all.length} done</span></div></div>` +
        sj.units
          .map(
            (u) =>
              `<section class="bb-unit"><h3>${u.name}<span>${u.lede}</span></h3><ul class="lessons">` +
              u.lessons.map((l) => `<li><button class="lesson${done.has(l.id) ? " done" : ""}" data-lesson="${l.id}"><b>${l.title}</b><span>${l.q}</span><em>${l.steps.length} steps${done.has(l.id) ? " · done" : ""}</em></button></li>`).join("") +
              `</ul></section>`
          )
          .join("") +
        ""
      );
    }
    return (
      `<div class="bb-ix-head"><div><h2>${sj.title}</h2><p class="lede">${sj.lede}</p></div><div class="bb-tools"><span class="prog term">Next term</span></div></div>` +
      sj.syllabus.map((u) => `<section class="bb-unit later"><h3>${u.name}</h3><ul class="lessons">${u.lessons.map(([t, q]) => `<li><div class="lesson later"><b>${t}</b><span>${q}</span><em>on the syllabus</em></div></li>`).join("")}</ul></section>`).join("")
    );
  }

  const textOf = (v) => (typeof v === "function" ? v() : v || "");
  function toolsHtml(sj) {
    if (sj.id === "shop") return `<button class="chalkbtn" data-act="settings">Shop settings</button><button class="chalkbtn" data-lesson="check">Check my drawing</button><button class="chalkbtn" data-act="print-card">Print the shop card</button>`;
    if (sj.id === "drafting") return `<button class="chalkbtn try" data-lesson="d-cmd">Start here</button>`;
    return "";
  }
  function extraHtml(st) {
    let h = textOf(st.extra);
    if (st.place) {
      const ready = st.place.ready ? st.place.ready() : true;
      h += `<div class="bb-row"><button class="chalkbtn try" data-act="place"${ready ? "" : " disabled"}>${st.place.label}</button></div>`;
    }
    return h;
  }

  function renderFig(live) {
    figEl.classList.toggle("live", !!live);
    n = 0;
    words = [];
    if (view === "settings") figEl.innerHTML = cardHtml();
    else if (isLesson()) figEl.innerHTML = LESSON.steps[step].fig();
    else figEl.innerHTML = "";
  }
  function renderMath() {
    if (!isLesson()) return;
    const st = LESSON.steps[step];
    const m = $("#bb-math");
    if (m) m.innerHTML = st.math();
    const b = $("#bb-basis");
    if (b) b.innerHTML = textOf(st.basis);
    const ex = $("#bb-ex");
    if (ex) ex.innerHTML = textOf(st.ex);
  }
  function renderExtra() {
    const e = $("#bb-extra");
    if (e && isLesson()) e.innerHTML = extraHtml(LESSON.steps[step]);
    renderMath();
  }
  function renderText() {
    if (view === "settings") {
      textEl.innerHTML = settingsHtml();
      return;
    }
    if (!isLesson()) {
      textEl.innerHTML = indexHtml();
      return;
    }
    const st = LESSON.steps[step];
    textEl.innerHTML =
      `<p class="kicker">${LESSON.title}</p><h2>${st.t}</h2><p class="lede">${textOf(st.p)}</p><div id="bb-ex"></div><div class="math" id="bb-math"></div>` +
      st.ctl.map(sliderHtml).join("") +
      `<p class="basis" id="bb-basis"></p><div id="bb-extra">${extraHtml(st)}</div>` +
      `<div id="bb-task">${taskHtml(st)}</div>` +
      (st.note ? `<p class="note">${st.note}</p>` : "");
    renderMath();
  }
  function renderTray() {
    const lesson = isLesson();
    $("#bb-back").hidden = $("#bb-next").hidden = $("#bb-erase").hidden = stepsEl.hidden = !lesson;
    $("#bb-lessons").hidden = !lesson && view !== "settings";
    $("#bb-print").hidden = !lesson || LESSON.id === "check";
    $("#bb-unit").innerHTML = `<span class="long">Units: </span>${s.unit}`;
    $("#bb-hc").classList.toggle("on", hc);
    $("#bb-hc").setAttribute("aria-pressed", hc);
    if (!lesson) {
      const sj = curSubject();
      $("#bb-label").textContent = view === "settings" ? "Shop settings" : sj.units ? `${sj.name} · ${sj.units.flatMap((u) => u.lessons).filter((l) => done.has(l.id)).length} of ${sj.units.flatMap((u) => u.lessons).length} lessons done` : `${sj.name} · next term`;
      return;
    }
    $("#bb-label").innerHTML = `<span class="long">${LESSON.title} · </span>step ${step + 1} of ${LESSON.steps.length}`;
    stepsEl.innerHTML = LESSON.steps.map((_, i) => `<button class="${i === step ? "on" : i < step ? "done" : ""}" data-step="${i}" aria-label="Step ${i + 1}"${i === step ? ' aria-current="step"' : ""}></button>`).join("");
    $("#bb-back").disabled = step === 0;
    $("#bb-next").textContent = step === LESSON.steps.length - 1 ? "Done" : "Next";
  }
  function render() {
    mainEl.classList.toggle("index", !isLesson() && view !== "settings");
    mainEl.classList.toggle("settings", view === "settings");
    root.classList.toggle("hc", hc);
    renderTabs();
    renderText();
    renderFig(false);
    renderTray();
    persist();
  }
  /** After a number changed: figure, maths and any number that depends on it. */
  function refreshLive() {
    renderFig(true);
    renderMath();
    syncSliders();
    const e = $("#bb-extra");
    if (e && isLesson()) {
      const st = LESSON.steps[step];
      const b = e.querySelector('[data-act="place"]');
      if (b && st.place?.ready) b.disabled = !st.place.ready();
      const k = e.querySelector('[data-act="suggest"]');
      if (k) k.textContent = `Use K = ${suggestK().toFixed(2)}`;
    }
    persist();
  }

  // ================================================================ ACTIONS

  function go(i) {
    step = clamp(i, 0, LESSON.steps.length - 1);
    render();
    textEl.scrollTop = 0;
  }
  function openLesson(id, i = 0) {
    LESSON = LESSONS.find((l) => l.id === id) || CMD;
    view = LESSON.id;
    if (subjectOf(LESSON.id)) subject = subjectOf(LESSON.id).id;
    go(i);
  }
  function openTab(id) {
    subject = id;
    view = `tab:${id}`;
    render();
    textEl.scrollTop = 0;
  }
  function openSettings() {
    settingsProc = s.proc;
    view = "settings";
    render();
    textEl.scrollTop = 0;
  }

  function setOpen(v, focus = true) {
    if (v === open) return;
    open = v;
    app.classList.toggle("board-open", v);
    root.setAttribute("aria-hidden", v ? "false" : "true");
    root.inert = !v;
    document.querySelectorAll("#bb-top-btn, #bb-phone-btn").forEach((b) => b.classList.toggle("on", v));
    if (v) {
      lastFocus = document.activeElement;
      render();
      if (focus) setTimeout(() => (isLesson() ? $("#bb-next") : textEl.querySelector("button.lesson") || tabsEl.querySelector(".bbtab.on"))?.focus({ preventScroll: true }), 60);
    } else {
      (lastFocus && lastFocus.focus ? lastFocus : canvas).focus?.({ preventScroll: true });
    }
  }
  function close() {
    setOpen(false);
  }

  /** Type a value in place of a slider's readout. */
  function editValue(btn) {
    const k = btn.dataset.o, c = CTL[k];
    const useIn = inch() && !c.plain;
    const input = document.createElement("input");
    input.className = "out-in";
    input.inputMode = "decimal";
    input.value = trim(useIn ? val(k) / 25.4 : val(k));
    input.setAttribute("aria-label", c.label);
    btn.replaceWith(input);
    input.focus();
    input.select();
    let finished = false;
    const finish = (commit) => {
      if (finished) return;
      finished = true;
      if (commit) {
        const v = parseFloat(String(input.value).replace(",", "."));
        if (Number.isFinite(v)) {
          const [lo, hi] = c.mm;
          setVal(k, clamp(useIn ? v * 25.4 : v, lo, hi));
        }
      }
      const b = document.createElement("button");
      b.className = "out";
      b.dataset.o = k;
      b.title = "Type a value";
      b.textContent = showK(k);
      input.replaceWith(b);
      refreshLive();
    };
    input.onkeydown = (e) => {
      e.stopPropagation();
      if (e.key === "Enter") finish(true);
      if (e.key === "Escape") finish(false);
    };
    input.onblur = () => finish(true);
  }

  // ================================================================ WIRING

  textEl.addEventListener("input", (e) => {
    const k = e.target.dataset?.k;
    if (k) {
      const raw = +e.target.value;
      setVal(k, inch() && !CTL[k].plain ? raw * 25.4 : raw);
      refreshLive();
      return;
    }
    if (e.target.dataset?.set) applySetting(e.target);
  });
  textEl.addEventListener("change", (e) => {
    if (e.target.dataset?.set) {
      applySetting(e.target);
      if (!e.target.classList.contains("bad")) renderText();
    }
  });
  textEl.addEventListener("keydown", (e) => {
    if (e.target.dataset?.set) e.stopPropagation();
  });

  root.addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b || !root.contains(b)) return;
    if (b.dataset.tab) return openTab(b.dataset.tab);
    if (b.dataset.lesson) return openLesson(b.dataset.lesson, 0);
    if (b.dataset.o) return editValue(b);
    const a = b.dataset.act;
    if (!a) return;
    if (a === "settings") return openSettings();
    if (a === "proc" || a === "setproc") {
      s.proc = b.dataset.v;
      settingsProc = s.proc;
      renderText();
      renderFig(true);
      persist();
      return;
    }
    if (a === "mat") {
      s.mat = b.dataset.v;
      renderText();
      renderFig(true);
      persist();
      return;
    }
    if (a === "proj" || a === "sheet") {
      s[a] = b.dataset.v;
      renderText();
      renderFig(true);
      persist();
      return;
    }
    if (a === "task-setup" || a === "task-check" || a === "task-show") {
      const st = LESSON.steps[step];
      if (!st.task) return;
      if (a === "task-setup") return runSetup(st);
      if (a === "task-show") return st.task.show();
      runTaskCheck(st);
      const t = $("#bb-task");
      if (t) t.innerHTML = taskHtml(st);
      return;
    }
    if (a === "wside") {
      s.wside = b.dataset.v;
      renderText();
      renderFig(true);
      persist();
      return;
    }
    if (a === "suggest") {
      s.K = suggestK();
      renderText();
      renderFig(true);
      persist();
      return;
    }
    if (a === "check") return runCheck();
    if (a === "show") return showOffenders();
    if (a === "place") return LESSON.steps[step].place?.run();
    if (a === "print-card") return printCard();
    if (a === "print-table") return printTable();
    if (a === "reset-shop") {
      shop = shopDefaults();
      saveShop();
      checked = null;
      render();
      toast("Shop settings put back to the starting values.");
    }
  });
  stepsEl.addEventListener("click", (e) => {
    const i = e.target.closest("button")?.dataset.step;
    if (i !== undefined) go(+i);
  });
  $("#bb-next").onclick = () => {
    if (step < LESSON.steps.length - 1) return go(step + 1);
    if (LESSON.id !== "check") done.add(LESSON.id);
    openTab(subjectOf(LESSON.id).id);
  };
  $("#bb-back").onclick = () => go(step - 1);
  $("#bb-lessons").onclick = () => openTab(isLesson() ? subjectOf(LESSON.id).id : subject);
  $("#bb-print").onclick = () => printLesson();
  $("#bb-erase").onclick = () => {
    const unit = s.unit, proc = s.proc, mat = s.mat;
    Object.assign(s, DEF, { unit, proc, mat });
    render();
    toast("Numbers put back to the starting values.");
  };
  $("#bb-unit").onclick = () => {
    s.unit = inch() ? "mm" : "in";
    renderText();
    renderFig(true);
    renderTray();
    persist();
  };
  $("#bb-hc").onclick = () => {
    hc = !hc;
    root.classList.toggle("hc", hc);
    renderTray();
    persist();
  };
  $("#bb-close").onclick = close;
  $("#board-scrim").onclick = close;
  addEventListener("keydown", (e) => {
    if (e.key === "F1") {
      e.preventDefault();
      return setOpen(!open);
    }
    if (!open || e.target instanceof HTMLInputElement) return;
    if (isLesson() && e.key === "ArrowRight") go(step + 1);
    else if (isLesson() && e.key === "ArrowLeft") go(step - 1);
  });

  // entry points: the top bar (desktop), the title bar (phone), the welcome card
  const boardIcon = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3.5" width="18" height="12" rx="1.5"/><path d="M6.5 11c1.8-2.6 3 1.4 4.8-.6s2.4.8 4.2-1.4"/><path d="M8 20l1.6-4.5M16 20l-1.6-4.5"/></svg>';
  for (const [id, before, cls] of [["bb-top-btn", "#undo-btn", "iconbtn"], ["bb-phone-btn", "#pundo", "iconbtn"]]) {
    const b = document.createElement("button");
    b.id = id;
    b.className = cls;
    b.title = "Blackboard: lessons (F1)";
    b.setAttribute("aria-label", "Blackboard: lessons");
    b.innerHTML = boardIcon;
    b.onclick = () => setOpen(!open);
    $(before).before(b);
  }
  document.querySelectorAll("[data-board]").forEach((b) => (b.onclick = () => setOpen(true)));

  return {
    open: (where) => {
      if (where === "settings") openSettings();
      else if (LESSONS.some((l) => l.id === where)) openLesson(where, 0);
      else if (SHOWN.some((sj) => sj.id === where)) openTab(where);
      setOpen(true);
    },
    close,
    toggle: () => setOpen(!open),
    isOpen: () => open,
    check: () => {
      openLesson("check", 0);
      setOpen(true);
      runCheck();
    },
    calc,
    state: s,
  };
})();


boot();

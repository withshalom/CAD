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
const APERTURE = { mouse: 10, touch: 22 }; // px, DESIGN.md §9
const GRIP = 8; // px, DESIGN.md §5
const GRID_SPACING = 10; // drawing units (standards::canvas::GRID_SPACING)
const SNAP_KINDS = { END: 1, MID: 2, CEN: 4, QUA: 8, INT: 16, NEA: 32, NOD: 64, INS: 128 };
const SNAP_DEFAULT = 1 | 2 | 4 | 8 | 16 | 64 | 128;
const POLAR_STEP = 15; // degrees
const TAU = Math.PI * 2;
const PHONE = matchMedia("(max-width: 760px)");

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
    $("#welcome-version").textContent = globalThis.CAD_CORE_WASM
      ? `${core.version} · single-file test build`
      : `${core.version} · DWG-native drafting`;
  } catch (e) {
    $("#welcome-version").textContent = `The core could not be loaded: ${e.message}`;
    document.querySelectorAll(".welcome-actions button").forEach((b) => (b.disabled = true));
    return;
  }
  if ("serviceWorker" in navigator && location.protocol.startsWith("http")) {
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
  ["dim", "Dimension (D)", "DIM"],
  ["measure", "Measure (DI)", "DIST"],
  "-",
  ["move", "Move (M)", "MOVE"],
  ["rotate", "Rotate (RO)", "ROTATE"],
  ["mirror", "Mirror (MI)", "MIRROR"],
  ["offset", "Offset (O): not built yet", "OFFSET", true],
  ["trim", "Trim (T): not built yet", "TRIM", true],
  ["fillet", "Fillet (F): not built yet", "FILLET", true],
  "-",
  ["copy", "Copy (CO)", "COPY"],
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
    const [ic, title, cmd, later] = t;
    const b = document.createElement("button");
    b.className = "tool";
    b.dataset.tool = cmd;
    b.title = title;
    b.setAttribute("aria-label", title);
    b.innerHTML = icon(ic);
    b.disabled = !!later;
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
    "-",
    ["Save", "Ctrl+S", () => save(), !has],
    ["Save as .cad…", "Ctrl+Shift+S", () => saveAs("cad"), !has],
    ["Save as DWG (AutoCAD 2018)…", "", () => saveAs("dwg"), !has],
    ["Export DXF…", "", () => saveAs("dxf"), !has],
    ["Export binary DXF…", "", () => saveAs("dxfb"), !has],
    ["Export SVG…", "", () => saveAs("svg"), !has],
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

function updateChrome() {
  const i = doc?.info;
  for (const id of ["#undo-btn", "#pundo"]) $(id).disabled = !i || !i.undo;
  for (const id of ["#redo-btn", "#predo"]) $(id).disabled = !i || !i.redo;
  const saved = $("#saved");
  saved.textContent = !i ? "" : i.dirty ? "Unsaved" : "Saved";
  saved.classList.toggle("dirty", !!i?.dirty);
  $("#crumb").innerHTML = i ? `<b>Model</b> · 1:1 · ${esc(i.units)}` : "";
  $("#ptitle").textContent = doc ? doc.name : "CAD";
  $("#psub").textContent = i ? `Model · ${unitMark() || i.units} · ${i.dirty ? "Unsaved" : "Saved"}` : "";
  const u = user();
  $("#who-av").textContent = u ? u.split(/\s+/).map((w) => w[0]).join("").slice(0, 2).toUpperCase() : "·";
  $("#who-text").textContent = u ? `${u} · this browser` : "Editing in this browser";
  const app = globalThis.CAD_CORE_WASM ? "CAD (test build)" : "CAD";
  document.title = doc ? `${doc.name}${i.dirty ? " •" : ""} — ${app}` : app;
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
  const b = doc.disp.bounds;
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
  ctx.fillStyle = COLORS.space;
  ctx.fillRect(0, 0, w, h);
  if (!doc) return;
  if (!doc.view) zoomExtents();
  if (!doc.view) return;
  if (aids.grid) drawGrid(w, h);

  const disp = doc.disp;
  const [vx0, vy1] = toWorld(0, 0);
  const [vx1, vy0] = toWorld(w, h);
  const visible = (it) => it.box[2] >= vx0 && it.box[0] <= vx1 && it.box[3] >= vy0 && it.box[1] <= vy1;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  disp.byStyle.forEach((list, s) => {
    const st = disp.styles[s];
    ctx.strokeStyle = st.screen;
    ctx.fillStyle = st.screen;
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
  if (!cursor.inside || cursor.touch) return;
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
    if (req?.type === "point" || req?.type === "distance" || req?.type === "angle") {
      const kinds = req.forceSnap ?? (aids.osnap ? SNAP_DEFAULT : 0);
      if (kinds) {
        const res = core.query(doc.id, `snap ${cursor.wx} ${cursor.wy} ${r} ${kinds}`);
        cursor.snaps = res?.snaps || [];
        if (cursor.snaps.length) cursor.snap = cursor.snaps[cursor.snapIndex % cursor.snaps.length];
      }
    }
    if ((!req || req.type === "select") && !pointerMode) {
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
      // On touch, a drag that starts on empty space pans.
      if (m.touch && !hoverAt(m.x0, m.y0)) pointerMode = { kind: "pan", x: e.clientX, y: e.clientY };
      else pointerMode = { ...m, kind: "box" };
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

/** Grips on the selected objects' defining points (DESIGN.md §5). */
function updateGrips() {
  grips = [];
  if (!doc || selection.size > 40) return;
  for (const h of selection) {
    const g = core.query(doc.id, `get ${h}`);
    if (!g) continue;
    const add = (x, y, kind, field) => grips.push({ x, y, handle: h, kind, field, g });
    if (g.type === "CIRCLE" || g.type === "ARC") {
      const [cx, cy] = g.center;
      add(cx, cy, "center");
      if (g.type === "CIRCLE") {
        for (let q = 0; q < 4; q++) add(cx + g.radius * Math.cos((q * Math.PI) / 2), cy + g.radius * Math.sin((q * Math.PI) / 2), "radius");
      } else {
        const a0 = (g.start * Math.PI) / 180, a1 = (g.end * Math.PI) / 180;
        let sw = a1 - a0;
        if (sw < 0) sw += TAU;
        add(cx + g.radius * Math.cos(a0), cy + g.radius * Math.sin(a0), "arc-start");
        add(cx + g.radius * Math.cos(a1), cy + g.radius * Math.sin(a1), "arc-end");
        add(cx + g.radius * Math.cos(a0 + sw / 2), cy + g.radius * Math.sin(a0 + sw / 2), "radius");
      }
    } else if (g.type === "LINE") {
      add(g.start[0], g.start[1], "end", "1");
      add(g.end[0], g.end[1], "end", "2");
      add((g.start[0] + g.end[0]) / 2, (g.start[1] + g.end[1]) / 2, "move");
    } else if (g.type === "LWPOLYLINE" && g.points) {
      g.points.forEach((p, i) => add(p[0], p[1], "vertex", String(i)));
    } else if (g.type === "TEXT" && g.insertion) {
      add(g.insertion[0], g.insertion[1], "move");
    }
  }
}

/** Dragging a grip edits the object: one command, one undo step. */
function startGrip(gr) {
  startCommand("GRIP", function* (c) {
    const base = [gr.x, gr.y];
    const g = gr.g;
    const req = c.point("Specify stretch point", { base });
    const cen = g.center;
    if (gr.kind === "radius") req.preview = (q) => [{ t: "circle", c: cen, r: Math.hypot(q[0] - cen[0], q[1] - cen[1]) }];
    else if (gr.kind === "center" || gr.kind === "move") {
      req.ghost = { handles: [gr.handle], xf: null };
      req.preview = (q) => {
        req.ghost.xf = xfMove(q[0] - base[0], q[1] - base[1]);
        return [{ t: "line", a: base, b: q }];
      };
    } else if (gr.kind === "end") {
      const other = gr.field === "1" ? g.end : g.start;
      req.preview = (q) => [{ t: "line", a: other, b: q }];
    } else req.preview = (q) => [{ t: "line", a: base, b: q }];
    const q = yield req;
    if (!q) return;
    const h = gr.handle;
    if (gr.kind === "center" || gr.kind === "move") c.edit(`move ${N(q[0] - base[0])} ${N(q[1] - base[1])} ${h}`);
    else if (gr.kind === "radius") c.edit(`set ${h} radius ${N(Math.hypot(q[0] - cen[0], q[1] - cen[1]))}`);
    else if (gr.kind === "arc-start" || gr.kind === "arc-end") {
      const ang = (Math.atan2(q[1] - cen[1], q[0] - cen[0]) * 180) / Math.PI;
      c.edit(`set ${h} ${gr.kind === "arc-start" ? "start" : "end"} ${N(ang)}`);
    } else if (gr.kind === "end") {
      c.exec("begin");
      c.edit(`set ${h} x${gr.field} ${N(q[0])}`);
      c.edit(`set ${h} y${gr.field} ${N(q[1])}`);
      c.exec("end");
    } else if (gr.kind === "vertex") c.edit(`set ${h} v${gr.field} ${N(q[0])} ${N(q[1])}`);
  });
}

// ---------------------------------------------------------------- keyboard

function onKey(e) {
  if (e.target instanceof HTMLInputElement && e.target !== cmdInput) return;
  const k = e.key;
  if (!$("#numpad").hidden) return;
  if (e.ctrlKey || e.metaKey) {
    const lower = k.toLowerCase();
    const map = { z: e.shiftKey ? "REDO" : "UNDO", y: "REDO", s: e.shiftKey ? "SAVEAS" : "SAVE", o: "OPEN", n: "NEW", a: "SELECTALL", 1: "PROPERTIES", 2: "LAYER" };
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
  if (c.later) return logError(`${name} is not built yet.`);
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
  } else if (req.type === "distance") step(Math.hypot(p[0] - req.base[0], p[1] - req.base[1]));
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

/** The circle through three points, and the arc from a over b to p. */
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
  if (!p) return;
  const arc = arc3(a, b, p);
  if (!arc) return c.log("The three points are in a line; no arc.");
  c.edit(`arc ${P(arc.c)} ${N(arc.r)} ${deg(arc.a0)} ${deg(arc.a1)}`);
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
  const defH = doc.info.insunits === 1 ? 0.125 : 3.5; // standards::text_height
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

/** Horizontal or vertical, by where the dimension line goes (as DIMLINEAR does). */
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
    const t = deg(Math.atan2(loc[1] - cen[1], loc[0] - cen[0]));
    c.edit(`dim-${radius ? "radius" : "diameter"} ${P(cen)} ${N(circ.radius)} ${t}`);
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
    if (aligned) {
      const dx = second[0] - first[0], dy = second[1] - first[1];
      const L = Math.hypot(dx, dy);
      const off = ((loc[1] - first[1]) * dx - (loc[0] - first[0]) * dy) / L;
      c.edit(`dim-aligned ${P(first)} ${P(second)} ${N(off)}`);
    } else {
      const u = linearDir(first, second, loc);
      c.edit(`dim-linear ${P(first)} ${P(second)} ${P(loc)} ${u[0] ? 0 : 90}`);
    }
    return;
  }
}

function* cmdDist(c) {
  const a = yield c.point("Specify first point");
  if (!a) return;
  const b = yield c.point("Specify second point", { base: a, preview: (q) => [{ t: "line", a, b: q }] });
  if (!b) return;
  const dx = b[0] - a[0], dy = b[1] - a[1];
  let ang = (Math.atan2(dy, dx) * 180) / Math.PI;
  if (ang < 0) ang += 360;
  const msg = `Distance = ${fmt(Math.hypot(dx, dy))}, Angle = ${ang.toFixed(2)}°, ΔX = ${fmt(dx)}, ΔY = ${fmt(dy)}`;
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
  { name: "OFFSET", aliases: ["O"], desc: "not built yet", later: true },
  { name: "TRIM", aliases: ["T", "TR"], desc: "not built yet", later: true },
  { name: "FILLET", aliases: ["F"], desc: "not built yet", later: true },
  { name: "ERASE", aliases: ["E"], desc: "remove objects", run: cmdErase, needsDoc: true },
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
    rows.push(["Center X", fmt(g.center[0]), u, "cx"], ["Center Y", fmt(g.center[1]), u, "cy"], ["Radius", fmt(g.radius), u, "radius"], ["Diameter", fmt(2 * g.radius), u, "diameter"], ["Circumference", fmt(TAU * g.radius), u, "circumference"], ["Area", fmt(Math.PI * g.radius * g.radius), u && `${u}²`, "area"]);
  } else if (g.type === "ARC") {
    let sw = g.end - g.start;
    if (sw < 0) sw += 360;
    rows.push(["Center X", fmt(g.center[0]), u, "cx"], ["Center Y", fmt(g.center[1]), u, "cy"], ["Radius", fmt(g.radius), u, "radius"], ["Start angle", g.start.toFixed(2), "°", "start"], ["End angle", g.end.toFixed(2), "°", "end"], ["Arc length", fmt((sw * Math.PI * g.radius) / 180), u]);
  } else if (g.type === "LINE") {
    let ang = (Math.atan2(g.end[1] - g.start[1], g.end[0] - g.start[0]) * 180) / Math.PI;
    if (ang < 0) ang += 360;
    rows.push(["Start X", fmt(g.start[0]), u, "x1"], ["Start Y", fmt(g.start[1]), u, "y1"], ["End X", fmt(g.end[0]), u, "x2"], ["End Y", fmt(g.end[1]), u, "y2"], ["Length", fmt(g.length), u, "length"], ["Angle", ang.toFixed(2), "°"]);
  } else if (g.type === "LWPOLYLINE") {
    rows.push(["Vertices", String(g.vertices), ""], ["Closed", g.closed ? "Yes" : "No", ""]);
  } else if (g.type === "TEXT") {
    rows.push(["Contents", g.value, "", "value"], ["Height", fmt(g.height), u, "height"], ["Position", pt(g.insertion), u]);
  }
  return rows;
}

function title(t) {
  const names = { LWPOLYLINE: "Polyline", MTEXT: "Multiline text", DIMENSION: "Dimension", INSERT: "Block reference" };
  return names[t] || t.charAt(0) + t.slice(1).toLowerCase();
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
    ${row(["Color", g.color])}${row(["Linetype", g.linetype])}${row(["Lineweight", g.lineweight])}
    ${propRows(g).length ? `<div class="grp">Geometry</div>${propRows(g).map(row).join("")}` : ""}`;
  el.querySelectorAll("[data-field]").forEach((r) => (r.onclick = () => editRow(r, g)));
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
  const raw = field === "value" ? g.value : v.firstChild.textContent;
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
  if (field === "value") return applySet(h, field, text);
  const angle = field === "start" || field === "end";
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
      <span class="n">${esc(l.name)}${l.current ? '<span class="curtag">CURRENT</span>' : ""}</span>
      <span class="lt" title="${esc(l.linetype || "Continuous")}">${linetypeSample(l)}</span>
      <span class="c">${l.count}</span>
      <button class="ib${l.locked ? "" : " dim"}" data-a="lock" title="${l.locked ? "Locked" : "Unlocked"}">${icon(l.locked ? "lock" : "unlock")}</button>
      <button class="ib${l.on ? "" : " dim"}" data-a="on" title="${l.on ? "On" : "Off"}">${icon(l.on ? "eye" : "eyeoff")}</button>`;
    row.onclick = (e) => {
      const a = e.target.closest("button")?.dataset.a;
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
  el.innerHTML = `<div class="kv"><span class="k">Units</span><span class="v">${esc(cap(i.units))}</span></div>
    <div class="kv"><span class="k">Display</span><span class="v">${esc(i.display)}</span></div>
    <div class="kv"><span class="k">Scale</span><span class="v">1:1</span></div>
    <div class="kv"><span class="k">Opened from</span><span class="v">${esc(i.source === "new" ? "New drawing" : i.source.toUpperCase())}</span></div>
    <details><summary class="grp" style="cursor:pointer">Summary</summary><pre class="summary">${esc(i.summary)}</pre></details>`;
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
  dwg: { description: "DWG drawing (AutoCAD 2018)", accept: { "application/acad": [".dwg"] } },
  dxf: { description: "DXF drawing", accept: { "application/dxf": [".dxf"] } },
  dxfb: { description: "Binary DXF drawing", accept: { "application/dxf": [".dxf"] } },
  svg: { description: "SVG image", accept: { "image/svg+xml": [".svg"] } },
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

// For automated tests of the page: where a world point is on screen.
window.cadTest = {
  toScreen: (x, y) => toScreen(x, y),
  canvasBox: () => canvas.getBoundingClientRect(),
  info: () => doc && core.info(doc.id),
  get: (h) => doc && core.query(doc.id, `get ${h}`),
  items: () => (doc ? doc.disp.live : 0),
  selection: () => [...selection],
  save: (format) => doc && core.save(doc.id, format).bytes.length,
  /** Whether the display, kept up to date by patches, is what the core
   *  would send whole. Reads the whole list, so the core's next patch
   *  starts from here. */
  displayMatches: () => {
    if (!doc) return true;
    const full = core.display(doc.id);
    const key = (d, it) => `${it.handle}|${styleKey(d.styles[it.style])}|${Array.from(it.ops.subarray(it.start, it.end)).join(",")}`;
    const a = full.items.map((it) => key(full, it)).sort();
    const b = doc.disp.items.filter((it) => !it.dead).map((it) => key(doc.disp, it)).sort();
    return a.length === b.length && a.every((x, i) => x === b[i]);
  },
  open: (name, bytes) => openFile(new File([bytes], name)),
  core: () => core,
  docs: () => docs.map((d) => d.name),
  history: () => document.querySelector("#history").textContent,
};

// Start once every constant above exists.
boot();

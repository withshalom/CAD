// The core as the page drives it: core.js over a built core.wasm, run in
// Node with no browser, so CI checks the WebAssembly build and the page's
// glue together on every push (the `web` job, after `cargo xtask web`).
//
//   node apps/web/tests/core.test.mjs dist/web/core.wasm corpus/mosaiciron-mi201
//
// Each check prints "ok" or "FAIL"; the exit status is 1 when any failed.
// Needs Node 18 or later and nothing else.

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const [wasmPath, corpus] = process.argv.slice(2);
if (!wasmPath) {
  console.error("usage: node core.test.mjs CORE.wasm [CORPUS_DIR]");
  process.exit(2);
}
// The single-file page's way in: core.js reads the module from this global
// instead of fetching it, which Node cannot do from a path.
globalThis.CAD_CORE_WASM = readFileSync(wasmPath).toString("base64");
const { Core, CoreCrash } = await import("../core.js");

let failures = 0;
async function check(name, fn) {
  const t = performance.now();
  try {
    await fn();
    console.log(`ok   ${name} (${Math.round(performance.now() - t)} ms)`);
  } catch (e) {
    failures++;
    console.log(`FAIL ${name}: ${e instanceof Error ? e.message : e}`);
  }
}
function expect(cond, msg) {
  if (!cond) throw new Error(msg);
}
/** Model space as the drawing's summary counts it: entities drawn,
 *  hidden (on layers that are off or frozen) and failed (not drawable). */
function modelSpace(core, id) {
  const m = core.info(id).summary.match(/Model space: (\d+) entit(?:y|ies) drawn, (\d+) hidden, (\d+) failed/);
  expect(m, "no model-space line in the summary");
  return { drawn: +m[1], hidden: +m[2], failed: +m[3] };
}
const entities = (core, id) => {
  const m = modelSpace(core, id);
  return m.drawn + m.hidden;
};
const textOf = (b) => new TextDecoder().decode(b.subarray(0, 64));

const core = await Core.load();

await check("the core names itself and is this tree's version", () => {
  const toml = readFileSync(new URL("../../../Cargo.toml", import.meta.url), "utf8");
  const version = toml.match(/\[workspace\.package\][^[]*?\nversion = "([^"]+)"/)?.[1];
  expect(version, "no version in Cargo.toml's [workspace.package]");
  expect(core.version === `BlackCAD ${version}`, `core says "${core.version}", Cargo.toml says ${version}`);
});

await check("new drawings, metric and imperial", () => {
  for (const [imperial, units] of [[false, "millimeters"], [true, "inches"]]) {
    const id = core.newDrawing(imperial);
    const info = core.info(id);
    expect(info.units === units, `units ${info.units}, not ${units}`);
    expect(entities(core, id) === 0, "a new drawing is not empty");
    core.close(id);
  }
});

await check("the sample plate opens and draws", () => {
  const id = core.sample();
  const info = core.info(id);
  expect(/Objects: \d+/.test(info.summary), "no object count in the summary");
  expect(entities(core, id) > 0, "no entities");
  const d = core.display(id);
  expect(d.items.length > 0 && d.ops.length > 0, "an empty display list");
  core.close(id);
});

await check("commands draw, edit, group, undo and redo", () => {
  const id = core.newDrawing(false);
  const run = (line) => {
    const r = core.exec(id, line);
    expect(r && r.ok, `${line}: ${JSON.stringify(r)}`);
    return r.handles || [];
  };
  const [ln] = run("line 0 0 100 0");
  const [ci] = run("circle 50 50 20");
  run("arc 0 0 30 0 90");
  run("arc-3p 0 0 10 10 20 0");
  run("pline 1 0 0 0 40 0 0 40 30 0 0 30 0");
  run("rect 120 0 160 30");
  run("text 0 60 3.5 0 BlackCAD smoke test");
  run("dim-linear 0 0 100 0 50 -15");
  run("dim-aligned 0 0 40 30 10");
  run(`dim-radius ${ci} 80 80`);
  run(`dim-diameter ${ci} 20 80`);
  const [cp] = run(`copy 0 200 ${ln}`);
  run(`move 10 0 ${cp}`);
  run(`rotate 0 0 90 ${cp}`);
  run(`mirror 0 0 0 100 1 ${ci}`);
  run(`erase ${cp}`);
  core.displayChanges(id);
  run("begin");
  run("line 0 -50 10 -50");
  run("line 10 -50 10 -40");
  run("end");
  const n = entities(core, id);
  expect(n >= 14, `${n} entities after the edits`);
  let steps = 0;
  core.exec(id, "undo");
  expect(entities(core, id) === n - 2, "begin…end did not undo as one step");
  core.exec(id, "redo");
  expect(entities(core, id) === n, "redo did not bring the group back");
  while (core.info(id).undo > 0 && steps++ < 100) core.exec(id, "undo");
  expect(entities(core, id) === 0, `${entities(core, id)} entities left after undoing everything`);
  while (core.info(id).redo > 0 && steps++ < 200) core.exec(id, "redo");
  expect(entities(core, id) === n, "redoing everything did not restore the drawing");
  core.close(id);
});

await check("the queries the page makes", () => {
  const id = core.sample();
  const d = core.display(id);
  const handles = [...d.byHandle.keys()];
  const h = handles[0];
  const [x0, y0, x1, y1] = d.bounds;
  const q = (line) => {
    const r = core.exec(id, line);
    expect(r && typeof r === "object", `${line}: no answer`);
    return r;
  };
  expect(q(`get ${h}`).type, "get: no type");
  expect("handle" in q(`pick ${(x0 + x1) / 2} ${(y0 + y1) / 2} ${(x1 - x0) / 2}`), "pick: no handle field");
  expect(Array.isArray(q(`box ${x0 - 1} ${y0 - 1} ${x1 + 1} ${y1 + 1} 0`).handles), "box: no handles");
  q(`snap ${x0} ${y0} 5 255`);
  expect(Array.isArray(q(`grips ${handles.slice(0, 5).join(" ")}`).grips), "grips: no list");
  q(`bounds ${handles.slice(0, 5).join(" ")}`);
  expect(q("fmt 12.5").text, "fmt: no text");
  expect(Math.abs(q("angle 30").value - Math.PI / 6) < 1e-12, "angle 30 is not π/6");
  expect(Math.abs(q('len 1/2"').value - 12.7) < 1e-12, 'len 1/2" is not 12.7 mm');
  expect(q(`measure ${x0} ${y0} ${x1} ${y1}`).text, "measure: no text");
  const layouts = q("layouts").layouts;
  expect(layouts.some((l) => l.model) && layouts.some((l) => !l.model), "layouts: no model and paper space");
  expect(q("patterns").patterns.length > 0, "patterns: none");
  expect(Array.isArray(q("blocks").blocks), "blocks: no list");
  expect(q("linetypes").loaded.length > 0, "linetypes: none loaded");
  expect(typeof q("check-holes 1 1 1").holes === "number", "check-holes: no count");
  const paper = layouts.find((l) => !l.model).name;
  core.exec(id, `space ${paper}`);
  expect(core.info(id).paper, `space ${paper} did not open the layout`);
  core.exec(id, "space Model");
  core.close(id);
});

await check("saves in every format and reads its own files back", () => {
  const id = core.sample();
  const n = entities(core, id);
  const magic = { dwg: (t) => t.startsWith("AC1032"), dxf: (t) => /^\s*0\s+SECTION/.test(t), dxfb: (t) => t.startsWith("AutoCAD Binary DXF"), pdf: (t) => t.startsWith("%PDF-"), svg: (t) => /<svg|<\?xml/.test(t) };
  for (const format of ["cad", "dwg", "dxf", "dxfb", "svg", "pdf"]) {
    const { bytes } = core.save(id, format);
    expect(bytes.length > 0, `${format}: no bytes`);
    if (magic[format]) expect(magic[format](textOf(bytes)), `${format}: starts ${JSON.stringify(textOf(bytes).slice(0, 20))}`);
    if (["cad", "dwg", "dxf", "dxfb"].includes(format)) {
      const back = core.open(bytes);
      const m = entities(core, back);
      expect(m === n, `${format}: ${m} entities read back, ${n} saved`);
      core.close(back);
    }
  }
  core.close(id);
});

if (corpus) {
  await check("the corpus opens (DWG R14–2018 but 2007, and every DXF)", () => {
    const counts = new Map();
    const refused = [];
    for (const name of readdirSync(corpus).filter((f) => /\.(dwg|dxf)$/i.test(f)).sort()) {
      try {
        const id = core.open(readFileSync(join(corpus, name)));
        const m = modelSpace(core, id);
        expect(!m.failed, `${name}: ${m.failed} entities failed to draw`);
        counts.set(name, m.drawn + m.hidden);
        core.close(id);
      } catch (e) {
        if (e instanceof CoreCrash) throw new Error(`${name} crashed the core: ${e.message}`);
        if (counts.has(name) || /failed to draw/.test(e.message)) throw e;
        refused.push(name);
      }
    }
    expect(counts.size > 0, "nothing opened");
    // The reader does not open AC1021 (2007) DWG yet; anything else refused is a regression.
    const unexpected = refused.filter((f) => !/r2007\.dwg$/i.test(f));
    expect(!unexpected.length, `refused: ${unexpected.join(", ")}`);
    // One drawing in every version: every file holds the same entities.
    const distinct = new Set(counts.values());
    expect(distinct.size === 1, `entity counts differ: ${[...counts].map(([f, c]) => `${f} ${c}`).join(", ")}`);
    console.log(`     ${counts.size} files, ${[...distinct][0]} entities each${refused.length ? `; refused as expected: ${refused.join(", ")}` : ""}`);
  });
}

await check("damaged and random input is refused without crashing the core", () => {
  let seed = 0x5eed;
  const rnd = () => ((seed = (Math.imul(seed, 1103515245) + 12345) >>> 0) / 2 ** 32);
  const inputs = [new Uint8Array(0), new TextEncoder().encode("0\nSECTION\n2\nENTITIES\n0\nLINE\n")];
  for (let i = 0; i < 20; i++) inputs.push(Uint8Array.from({ length: 1 + Math.floor(rnd() * 4096) }, () => Math.floor(rnd() * 256)));
  const sample = core.save(core.sample(), "dwg").bytes;
  for (let i = 0; i < 30; i++) {
    const b = sample.slice(0, 64 + Math.floor(rnd() * (sample.length - 64)));
    for (let k = 0; k < 8; k++) b[Math.floor(rnd() * b.length)] ^= 1 << Math.floor(rnd() * 8);
    inputs.push(b);
  }
  for (const b of inputs) {
    try {
      core.close(core.open(b));
    } catch (e) {
      if (e instanceof CoreCrash) throw new Error(`${b.length} bytes crashed the core: ${e.message}`);
    }
  }
  expect(!core.crashed, "the core is marked crashed");
  core.close(core.sample());
});

if (failures) {
  console.log(`${failures} failed`);
  process.exit(1);
}
console.log("all passed");

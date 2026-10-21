// The CAD core in the page: loads core.wasm (crates/cad-ffi built for
// wasm32-unknown-unknown) and wraps its entry points (crates/cad-ffi/src/web.rs).
// The module imports nothing; this file is all the glue there is.

const enc = new TextEncoder();
const dec = new TextDecoder();

/** Thrown when the core stops on a bug (a Rust panic). The module is replaced. */
export class CoreCrash extends Error {}

export class Core {
  /** The core refuses new drawings and edits above this (crates/cad-ffi/src/web.rs),
   *  well below the 2 GiB where browsers start to fail. */
  static SOFT_LIMIT_MB = 1024;

  static async load(url = "core.wasm") {
    let bytes;
    const inline = globalThis.CAD_CORE_WASM; // the single-file test page carries the module as base64
    if (typeof inline === "string") {
      const s = atob(inline);
      bytes = new Uint8Array(s.length);
      for (let i = 0; i < s.length; i++) bytes[i] = s.charCodeAt(i);
    } else {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`core.wasm: ${res.status} ${res.statusText}`);
      bytes = await res.arrayBuffer();
    }
    const module = await WebAssembly.compile(bytes);
    const core = new Core(module);
    await core.start();
    return core;
  }

  constructor(module) {
    this.module = module;
  }

  async start() {
    const inst = await WebAssembly.instantiate(this.module, {});
    this.x = inst.exports;
    if (this.x.cad_abi_version() !== 1) throw new Error("core.wasm is from a different build");
    this.x.cad_version();
    this.version = this.text();
  }

  /** The result buffer, copied out. Addresses are unsigned: past 2 GiB a
   *  pointer read as a signed number would be negative. */
  bytes() {
    return this.guard(() => {
      const x = this.x;
      return new Uint8Array(x.memory.buffer, x.cad_result_ptr() >>> 0, x.cad_result_len() >>> 0).slice();
    });
  }
  text() {
    return dec.decode(this.bytes());
  }

  /** Run `fn` with byte arrays copied into the module's memory. */
  withBytes(arrays, fn) {
    const x = this.x;
    const ptrs = this.guard(() =>
      arrays.map((a) => {
        if (!a.length) return 0;
        const p = x.cad_alloc(a.length) >>> 0;
        // Null: the core could not grow its memory. Writing there would
        // overwrite the core's own data.
        if (!p) throw new RangeError(`the core could not allocate ${a.length} bytes`);
        new Uint8Array(x.memory.buffer, p, a.length).set(a);
        return p;
      })
    );
    try {
      return this.guard(() => fn(ptrs));
    } finally {
      if (!this.crashed) arrays.forEach((a, i) => a.length && x.cad_free(ptrs[i], a.length));
    }
  }

  /** Calls into the core. A trap (a panic, or memory the core could not
   *  get), a RangeError (memory that could not grow, an address outside
   *  memory, a stack overflow) or a failed allocation all mean the module
   *  can no longer be trusted: it is marked crashed, and the app restarts
   *  it and reopens its drawings. */
  guard(fn) {
    if (this.crashed) throw new CoreCrash("the core stopped earlier and has not been restarted");
    try {
      return fn();
    } catch (e) {
      if (e instanceof CoreCrash) throw e;
      if (e instanceof WebAssembly.RuntimeError || e instanceof RangeError) {
        let msg = "";
        if (e instanceof WebAssembly.RuntimeError) {
          try {
            this.x.cad_panic_message();
            const x = this.x;
            msg = dec.decode(new Uint8Array(x.memory.buffer, x.cad_result_ptr() >>> 0, x.cad_result_len() >>> 0));
          } catch (_) {}
        }
        this.crashed = true;
        const mb = Math.round(this.memoryBytes() / 2 ** 20);
        const oom = !msg && (e instanceof RangeError || mb >= Core.SOFT_LIMIT_MB);
        const c = new CoreCrash(oom ? `out of memory (${mb} MB in use): ${e.message}` : msg || String(e));
        c.outOfMemory = oom;
        throw c;
      }
      throw e;
    }
  }

  /** Size of the core's memory, in bytes (it only grows). */
  memoryBytes() {
    try {
      return this.x.memory.buffer.byteLength;
    } catch (_) {
      return 0;
    }
  }

  /** Bytes the core holds right now (its allocations, not its high-water mark). */
  liveBytes() {
    return this.x.cad_heap_bytes ? this.x.cad_heap_bytes() >>> 0 : this.memoryBytes();
  }

  /** A fresh module after a crash. Every open drawing is gone. */
  async restart() {
    this.crashed = false;
    await this.start();
  }

  open(bytes) {
    const id = this.withBytes([bytes], ([p]) => this.x.cad_open(p, bytes.length));
    if (!id) throw new Error(this.text());
    return id;
  }
  newDrawing(imperial) {
    const id = this.guard(() => this.x.cad_new(imperial ? 1 : 0));
    if (!id) throw new Error(this.text());
    return id;
  }
  /** The sample plate from the workspace mockups. */
  sample() {
    const id = this.guard(() => this.x.cad_sample());
    if (!id) throw new Error(this.text());
    return id;
  }
  close(id) {
    this.guard(() => this.x.cad_close(id));
  }
  info(id) {
    const ok = this.guard(() => this.x.cad_info(id));
    if (!ok) throw new Error(this.text());
    return JSON.parse(this.text());
  }
  display(id) {
    const ok = this.guard(() => this.x.cad_display(id));
    if (!ok) throw new Error(this.text());
    return parseDisplay(this.bytes());
  }
  /** What changed since the display was last read: `patch` lists the
   *  entities whose items it replaces, or it is the whole list. */
  displayChanges(id) {
    const ok = this.guard(() => this.x.cad_display_changes(id));
    if (!ok) throw new Error(this.text());
    return parseDisplay(this.bytes());
  }
  /** Run a command line (crates/cad-ffi/src/session.rs); its JSON answer. */
  exec(id, line) {
    const b = enc.encode(line);
    const ok = this.withBytes([b], ([p]) => this.x.cad_exec(id, p, b.length));
    const t = this.text();
    if (!ok) throw new Error(t);
    return JSON.parse(t);
  }
  /** Try a command; null instead of an error. */
  query(id, line) {
    try {
      return this.exec(id, line);
    } catch (e) {
      if (e instanceof CoreCrash) throw e;
      return null;
    }
  }
  save(id, format, user = "") {
    const f = enc.encode(format);
    const u = enc.encode(user);
    const ok = this.withBytes([f, u], ([fp, up]) =>
      this.x.cad_save(id, fp, f.length, Date.now() / 1000, up, u.length)
    );
    if (!ok) throw new Error(this.text());
    const bytes = this.bytes();
    this.guard(() => this.x.cad_save_notes());
    const notes = this.text();
    return { bytes, notes: notes ? notes.split("\n") : [] };
  }
}

/** The display list (crates/cad-core/src/export/display.rs) for drawing. */
export function parseDisplay(buf) {
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  const magic = dec.decode(buf.subarray(0, 4));
  if (magic !== "DSPL" || dv.getUint32(4, true) !== 1) throw new Error("not a display list");
  const n = dv.getUint32(8, true);
  const head = JSON.parse(dec.decode(buf.subarray(12, 12 + n)));
  const start = Math.ceil((12 + n) / 8) * 8;
  const ops = new Float64Array(buf.buffer.slice(buf.byteOffset + start, buf.byteOffset + buf.byteLength));
  // Items: where each starts and ends in `ops`, its entity and style.
  const items = [];
  let i = 0;
  while (i < ops.length) {
    const code = ops[i];
    if (code === 1) {
      items.push({ handle: ops[i + 1].toString(16).toUpperCase(), style: ops[i + 2], start: i + 3, end: i + 3, ops });
      i += 3;
      continue;
    }
    i += opLength(ops, i);
    items[items.length - 1].end = i;
  }
  const byHandle = new Map();
  items.forEach((it, k) => {
    if (!byHandle.has(it.handle)) byHandle.set(it.handle, []);
    byHandle.get(it.handle).push(k);
  });
  return { ...head, ops, items, byHandle };
}

/** Values in the operation at `i`, its code included. */
export function opLength(ops, i) {
  switch (ops[i]) {
    case 2: return 5;
    case 3: return 6;
    case 4: return 3 + 2 * ops[i + 1];
    case 5: {
      let j = i + 2;
      for (let r = 0; r < ops[i + 1]; r++) j += 1 + 2 * ops[j];
      return j - i;
    }
    case 6: return 3;
    default: throw new Error(`display list: unknown operation ${ops[i]} at ${i}`);
  }
}

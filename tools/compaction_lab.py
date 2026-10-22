#!/usr/bin/env python3
"""Compaction experiments on the object pages of an uncompressed .blc file.

Each variant rewrites the same object records a different way, pages the
result like the writer does (about 64 KiB per page), and compresses every
segment on its own: with the format's LZ77 codec (through the
`lz77_segments` example), and with DEFLATE (zlib, level 9) and LZMA
(preset 6) from Python's standard library as references for what an
entropy-coding codec would add. Nothing here is part of the build; it only
measures. Results are recorded in docs/FORMAT-ROADMAP.md §5.

Usage (from the repository root):

    cargo build --release -p cad-core --example lz77_segments
    cargo run --release -p cad-core --example native_bench -- 200000 \\
        --rounded --save-uncompressed /tmp/bench.blc
    python3 tools/compaction_lab.py /tmp/bench.blc 200000 [--page-kib 64]

Any uncompressed .blc works as input (`cad save IN OUT.blc --no-compress`),
so the same experiments can run on real drawings. The variants simulate
layouts; they do not produce readable files. Defaults are the most common
value of each single-occurrence field per kind, as a schema would choose.
"""
import lzma
import os
import struct
import subprocess
import sys
import zlib
from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LZ77 = os.path.join(ROOT, "target", "release", "examples", "lz77_segments")
PAGE = 64 * 1024

# Value sizes by tag type; STR, BYTES and newer types are length-prefixed,
# HANDLE is a varint.
FIXED = {2: 8, 3: 2, 4: 4, 5: 8, 6: 1, 9: 16, 10: 24}


def varint(b, p):
    v = s = 0
    while True:
        x = b[p]
        p += 1
        v |= (x & 0x7F) << s
        if x < 0x80:
            return v, p
        s += 7


def put_varint(v):
    out = bytearray()
    while True:
        x = v & 0x7F
        v >>= 7
        if v:
            out.append(x | 0x80)
        else:
            out.append(x)
            return bytes(out)


def tags(rec):
    """(code, type, value bytes) per tag; for STR and BYTES the value
    excludes its length prefix, for HANDLE it is the varint."""
    p, out = 0, []
    while p < len(rec):
        code, t = struct.unpack_from("<HB", rec, p)
        p += 3
        if t in FIXED:
            v = rec[p:p + FIXED[t]]
            p += FIXED[t]
        elif t == 7:
            q = p
            _, p = varint(rec, p)
            v = rec[q:p]
        else:
            n, p = varint(rec, p)
            v = rec[p:p + n]
            p += n
        out.append((code, t, bytes(v)))
    return out


def object_records(path):
    """Records of every Objects.N section of an uncompressed .blc file."""
    b = open(path, "rb").read()
    (dir_off,) = struct.unpack_from("<Q", b, 24)
    p = dir_off
    n, p = varint(b, p)
    pages = []
    for _ in range(n):
        ln, p = varint(b, p)
        name = b[p:p + ln].decode()
        p += ln
        codec = b[p]
        p += 1
        off, p = varint(b, p)
        stored, p = varint(b, p)
        _, p = varint(b, p)
        p += 4
        _, p = varint(b, p)
        if name.startswith("Objects."):
            if codec != 0:
                sys.exit(f"{path}: section {name} is compressed; save with --no-compress")
            pages.append((int(name.split(".")[1]), off, stored))
    recs = []
    for _, off, stored in sorted(pages):
        data = b[off:off + stored]
        q = 0
        while q < len(data):
            ln, q = varint(data, q)
            recs.append(data[q:q + ln])
            q += ln
    return recs


def enc_tag(code, t, v):
    head = struct.pack("<HB", code, t)
    if t in FIXED or t == 7:
        return head + v
    return head + put_varint(len(v)) + v


def kind_of(ts):
    for c, _, v in ts:
        if c == 2001:
            return struct.unpack("<h", v)[0]
    return None


# ------------------------------------------------------------------ variants

def strip_names(ts):
    """No type name (tag 0) and no entity marker (tag 100)."""
    return [x for x in ts if x[0] not in (0, 100)]


def defaults_table(all_ts):
    count = defaultdict(Counter)
    for ts in all_ts:
        k = kind_of(ts)
        codes = Counter(c for c, _, _ in ts)
        for c, t, v in ts:
            if codes[c] == 1 and c not in (5, 2000, 2001):
                count[(k, c)][(t, v)] += 1
    return {key: cnt.most_common(1)[0][0] for key, cnt in count.items()}


def drop_defaults(ts, table):
    k = kind_of(ts)
    codes = Counter(c for c, _, _ in ts)
    return [x for x in ts if not (codes[x[0]] == 1 and table.get((k, x[0])) == (x[1], x[2]))]


def delta_keys(all_ts):
    """The key (tag 5) as the difference from the previous record's key."""
    out, prev = [], 0
    for ts in all_ts:
        new = []
        for c, t, v in ts:
            if c == 5:
                h, _ = varint(v, 0)
                v = put_varint(h - prev)
                prev = h
            new.append((c, t, v))
        out.append(new)
    return out


def tagged_pages(all_ts):
    pages, cur = [], bytearray()
    for ts in all_ts:
        body = b"".join(enc_tag(*x) for x in ts)
        rec = put_varint(len(body)) + body
        if cur and len(cur) + len(rec) > PAGE:
            pages.append([bytes(cur)])
            cur = bytearray()
        cur += rec
    if cur:
        pages.append([bytes(cur)])
    return pages


def zigzag(k):
    return k << 1 if k >= 0 else ((-k) << 1) - 1


def decimal_code(d):
    """x == k / 10**e exactly (same bits back) for e in 0..6: one byte e and
    a varint; otherwise 0xFF and the 8 bytes. Lossless either way."""
    (x,) = struct.unpack("<d", d)
    if x == x and abs(x) < 2**50:
        for e in range(7):
            k = round(x * 10**e)
            if abs(k) < 2**52 and struct.pack("<d", k / 10**e) == d:
                return bytes([e]) + put_varint(zigzag(k))
    return b"\xff" + d


def split_pages(all_ts, schema=False, shuffle=False, xor=False, decimal=False):
    """Each page split into streams: structure, doubles, integers and
    handles, strings. `schema`: no per-tag code or type bytes; each record
    has its kind and a 2-byte presence map instead (a fixed field order per
    kind, as DWG has)."""
    groups, cur, size = [], [], 0
    for ts in all_ts:
        s = sum(len(enc_tag(*x)) for x in ts)
        if cur and size + s > PAGE:
            groups.append(cur)
            cur, size = [], 0
        cur.append(ts)
        size += s
    if cur:
        groups.append(cur)
    out = []
    for g in groups:
        st, num, ints, strs = bytearray(), bytearray(), bytearray(), bytearray()
        last = {}
        for ts in g:
            k = kind_of(ts)
            st += struct.pack("<hH", -1 if k is None else k, 0) if schema else put_varint(len(ts))
            for c, t, v in ts:
                if not schema:
                    st += struct.pack("<HB", c, t)
                if t in (2, 9, 10):
                    for i in range(0, len(v), 8):
                        d = v[i:i + 8]
                        if xor:
                            key = (k, c, i)
                            prev = last.get(key, bytes(8))
                            last[key] = d
                            d = bytes(a ^ b for a, b in zip(d, prev))
                        num += decimal_code(d) if decimal else d
                elif t in (1, 8) or t > 10:
                    strs += put_varint(len(v)) + v
                else:
                    ints += v
        if shuffle and num:
            n8 = len(num) // 8
            num = bytearray(num[j * 8 + i] for i in range(8) for j in range(n8))
        out.append([bytes(st), bytes(num), bytes(ints), bytes(strs)])
    return out


# ----------------------------------------------------------------- measuring

def measure(label, pages, entities, tmp):
    segs = [s for pg in pages for s in pg if s]
    raw = sum(len(s) for s in segs)
    deflate = sum(len(zlib.compress(s, 9)) for s in segs)
    xz = sum(len(lzma.compress(s, preset=6)) for s in segs)
    with open(tmp, "wb") as f:
        for s in segs:
            f.write(struct.pack("<I", len(s)) + s)
    lz = int(subprocess.run([LZ77, tmp], capture_output=True, text=True, check=True).stdout.split()[2])
    e = entities
    print(f"{label:<46} {raw / e:7.1f} {lz / e:7.1f} {deflate / e:7.1f} {xz / e:7.1f}", flush=True)


def main():
    global PAGE
    args = sys.argv[1:]
    if "--page-kib" in args:
        i = args.index("--page-kib")
        PAGE = int(args[i + 1]) * 1024
        del args[i:i + 2]
    if len(args) != 2:
        sys.exit(__doc__)
    path, entities = args[0], int(args[1])
    if not os.path.exists(LZ77):
        sys.exit(f"build the LZ77 helper first: cargo build --release -p cad-core --example lz77_segments")
    recs = object_records(path)
    base = [tags(r) for r in recs]
    tmp = path + ".segments"
    print(f"{path}: {len(recs)} records, {entities} entities, pages of {PAGE // 1024} KiB")
    print(f"{'bytes per entity':<46} {'raw':>7} {'LZ77':>7} {'DEFLATE':>7} {'LZMA':>7}")
    v1 = [strip_names(ts) for ts in base]
    table = defaults_table(v1)
    v2 = [drop_defaults(ts, table) for ts in v1]
    v3 = delta_keys(v2)
    measure("V0 records as written today", tagged_pages(base), entities, tmp)
    measure("V1 minus type names and entity markers", tagged_pages(v1), entities, tmp)
    measure("V2 + default values omitted", tagged_pages(v2), entities, tmp)
    measure("V3 + keys as deltas", tagged_pages(v3), entities, tmp)
    measure("V4 + streams split by value type", split_pages(v3), entities, tmp)
    measure("V5 + fixed field order per kind (no tags)", split_pages(v3, schema=True), entities, tmp)
    measure("V6   V5 + doubles byte-shuffled", split_pages(v3, schema=True, shuffle=True), entities, tmp)
    measure("V7   V5 + doubles XOR previous, shuffled", split_pages(v3, schema=True, shuffle=True, xor=True), entities, tmp)
    measure("V8   V5 + decimal-aware doubles", split_pages(v3, schema=True, decimal=True), entities, tmp)
    os.remove(tmp)


if __name__ == "__main__":
    main()

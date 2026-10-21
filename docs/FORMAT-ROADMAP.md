# Native format (`.cad`): rationale, measurements and roadmap

Status as of 2026-10-01, format 2.0. The byte-level spec is `FORMAT.md`;
this document is the working plan for making the format as capable as
DWG, then more so. Nothing has been released, so the format can still
change freely.

**2.0 in one paragraph.** Records leave out default values and carry
their own schema; sections are split into streams and compressed with
DEFLATE; the directory is compressed and kept twice; a SHA-256 chain
covers the snapshot and every change set; recovery data rebuilds damaged
bytes; files carry drawing properties, and block records carry part data
(Proposed). Against 1.1 on the benchmark drawing: 18–34 % smaller with
recovery data included (23–38 % without), writes take 40–48 % less time
and reads 17–32 % less, incremental saves grow the file by 12 KB instead
of 20 KB, and one object is found in 0.4 ms once the file is open (§3). 2.0 reads every 1.x file; a fixture written by the 1.1 code is part
of the tests.

## 1. Why a DWG-style container with DXF-coded records

The format is often summed up as "binary DXF". That is misleading. A
binary DXF *file* is a flat stream of tags with no compression, no
checksums, no random access, no incremental save and no crash safety, and
would be a poor base. `.cad` takes only DXF's **vocabulary** (what group
code 10 or 62 means) and puts records keyed by it inside a **container of
our own, modelled on DWG R2004**: named, compressed, CRC-checked sections,
an object map, two header copies, change sets and preserved DWG bytes
(§2). Describe it that way: *a DWG-style container whose records use DXF
group codes*.

**What we kept from binary DXF:**
- The group-code vocabulary (10 = first point, 40 = radius, 62 = color,
  330 = owner…), exactly as the public DXF reference defines it.
- Records as flat streams of code/value tags.
- The `9 $NAME` + value style for header variables.
- The DXF CLASSES layout.

**What we changed:**
- Every tag carries an explicit type byte.
- Points are one tag, not three.
- Handles are binary.
- Strings are UTF-8 with a length prefix.
- Angles are radians.
- DWG flag words are stored as read.
- Codes 2000+ are our own.

**Why it is a good base.** The meaning of every standard field is public,
documented by Autodesk for interchange, so the clean-room position is
simple. The ODA DWG specification labels many DWG fields with their DXF group
code, so the same codes key DWG, DXF and `.cad`. That makes DWG → `.cad`
and `.cad` → DXF near-mechanical mappings. Anyone who knows DXF can read a
record dump (`cad inspect --tags`), and tag streams extend naturally.

**What it costs.** A tag per field. In 1.x every field was written:
- A line record is 165 bytes of tags. Its geometry (two points) is 54 of
  those bytes.
- The rest is structure and default values:
  - the type name as text,
  - an `"AcDbEntity"` marker,
  - zero thickness and the default extrusion,
  - a linetype scale of 1,
  - ByLayer color and linetype.
- Measured on 200,000 generated entities, our tags come out within ±2% of
  what the same content takes in real binary DXF.
- Compression removes most of this repetition (see §3), but it still costs
  encode and decode time.
- 2.0 leaves out the type name, the marker and every default (§4 item 1):
  a line record is now about 79 bytes, 54 of them its two points.

**Alternatives we did not take:**
- **DWG's own bit-level encoding.** The most compact form before
  compression, but it ties the native format to Autodesk's layouts and
  their evolution. Measured in §5.2: a fixed field order per type saves
  only 1–5% more once pages are compressed.
- **A SQLite database file.** Transactions and indexes for free, and SQLite
  is public domain. But it breaks the no-dependency rule and adds a C
  build step on every platform.
- **Generic binary encodings (CBOR, Protocol Buffers).** Our tag encoding is
  close to CBOR in spirit, but they carry no CAD semantics.
- **Zip + XML/JSON.** Readable, but large and slow for a million-entity
  drawing.

### 1.1 Decision record: the base reconsidered (2026-10-01)

Another review advised against "binary DXF as a base", on the grounds that
DWG has many capabilities that would have to be built from scratch, and
pointed to zip-based containers, STEP and existing open-source libraries
instead. The advice is right about a binary DXF *file*; it does not apply
to what `.cad` is. Both sides, steel-manned:

**Against the current design:**
1. DXF group codes are not a clean schema: the same code means different
   things in different entities and subclasses, so Autodesk's quirks are
   inherited.
2. No DXF reader opens `.cad`, so familiar codes bring no interchange by
   themselves.
3. A tag per field costs size and decode time (about 2× the needed size
   before compression, §3).
4. DXF codes say nothing useful about 3D solids, B-rep or constraints;
   DWG hides solids in proprietary ACIS data, while STEP has a standard
   B-rep.
5. Zip-based containers come with tools everywhere, easy packaging of
   images and fonts, and visible schema evolution; a custom binary
   container means owning every bug, the fuzzing and the recovery tools.
6. Autodesk's codes plus our own 2000+ codes make a dialect, and dialects
   drift.

**For the current design:**
1. The main job is a lossless DWG round trip. The ODA specification
   labels DWG fields with their DXF codes, so DWG → `.cad` → DXF/DWG maps
   field for field. Any other base needs the same mapping plus a layer of
   translation.
2. The container does not decide the big remaining work: modelling every
   DWG object type (layouts, plot settings, tables, dimension styles…).
   That work is the same in a zip, STEP or SQLite.
3. The DWG capabilities the review worried about are built and measured
   (§2, §3): compression, CRCs, random access in ~9 ms, crash-safe
   incremental saves in ~6 ms, unknown data kept, `.cdl` locks, a 19-check
   audit; several go beyond DWG.
4. STEP is the wrong domain: 3D product exchange, with no counterpart to
   layers, blocks, dimension styles, linetypes or paper-space layouts; its
   text form (Part 21) is large and slow; no incremental save; the
   specification is a paid ISO document.
5. Zip works against the key features: incremental saves orphan or
   rewrite its central directory, crash safety is weaker than ordered
   writes, random access needs an index we would build anyway, and
   XML/JSON at a million entities is large and slow. Our named sections
   are a zip in spirit, without those weaknesses.
6. Open-source DWG libraries are out under the clean-room rule (and some
   licenses forbid closed commercial use); the container and codes here
   need no dependency and no license.
7. Owning the format is what allows §7: history in the file, offline
   merge, fabrication data as first-class fields.

**Weighed:**

| Concern | Real? | Answer |
|---|---|---|
| Messy DXF codes | Yes | Explicit type byte per tag; each kind's fields are defined in FORMAT.md. The mess stays in one mapping. |
| No interchange gain | Partly | The gain is the DWG mapping, not DXF readers. |
| Size and speed | Yes | Measured; §4 item 1 halves records before the freeze. |
| 3D and B-rep | Yes, later | New sections and record kinds are allowed; a B-rep section when solids come. STEP as import/export, not as the base. |
| Packaging and tools | Partly | Embedded files as sections (§7); `cad inspect` and exports for tools. |
| Dialect drift | Yes | A schema section (§4 item 11) makes the format describe itself. |

**Decision:** keep the design. Call it a DWG-style container with
DXF-coded records, add the schema section, and treat STEP and a zipped
package as possible export formats, not as the native base.

## 2. What binary DXF lacks, and where `.cad` stands

| Capability | Binary DXF | DWG | `.cad` today |
|---|---|---|---|
| Keep data a reader does not know | Only within known code ranges (type is implied by code) | Custom classes are listed, then kept as proxies | Yes: explicit type byte; unknown fields, header variables and object kinds are kept and written back; unknown sections are skipped |
| Say what the data means | The DXF reference, outside the file | The ODA specification, outside the file | The file carries its schema: every kind's fields, types, defaults and meanings (2.0) |
| Compression | None | Yes (R2004+ pages; LZ77 without entropy coding) | DEFLATE per section after splitting records into streams (2.0); DWG's LZ77 still read |
| Integrity checks | None | CRCs and checksums throughout; Reed–Solomon error correction in R2007 only | CRC-32 per section and header copy; SHA-256 chain over snapshot and change sets; recovery data that rebuilds damaged runs; two directory copies (2.0) |
| Find one object without reading all | No: sequential | Yes: handle map | Yes: object map + 64 KiB pages; an open file keeps the map (0.4 ms per lookup) |
| Save changes without rewriting the file | No | Yes (ISAVEPERCENT incremental save) | Yes: change sets with author and time, crash-safe |
| Survive a crash mid-save | No | Backup (.bak) and autosave files | Yes: full saves write beside and rename; incremental saves are ordered so old or new state always loads |
| Edit lock / who has it open | No | `.dwl`/`.dwl2` sidecars (AutoCAD) | `.cdl` sidecar with heartbeat and stale-lock takeover |
| Handles | Hex strings | Bit-packed | Binary varints; stored as differences in split sections |
| Text encoding | Code page (older) / UTF-8, strings chunked | Code page / UTF-16 | UTF-8, any length |
| Preview | BMP thumbnail section | Thumbnail image | SVG (vector) |
| Objects the reader cannot decode | Lost or proxy | Proxy with graphics | Original DWG bytes kept |
| Drawing properties (summary info) | Header dates only | Yes | Yes: title, subject, author, keywords, comments, last saved by, revision, hyperlink base, dates, custom properties (2.0) |
| File dependencies, revision history | No | Partly | History through change sets; dependencies not yet |
| Password, digital signatures | No | Signatures; passwords (RC4) from AutoCAD 2004, removed in AutoCAD 2016 | Flags reserved; the hash chain is what a signature will sign (§5.4) |
| XDATA (extended data) | Yes (groups 1000–1071) | Yes (EED) | Yes: the same group codes; DWG extended data is decoded on import, and bytes that do not decode are kept |
| All object types, layouts, plot settings | Yes | Yes | Not yet (model limit, not format limit) |
| Change history, merge, collaboration | No | No | Foundation in place (change sets) |

## 3. Measurements

These numbers come from 200,000 generated model-space entities, run in a
cloud container (compression uses all its cores). Reproduce with:

```
cargo run --release -p cad-core --example native_bench -- 200000 [--lines] [--rounded] [--no-recovery]
```

**Format 2.0** (recovery data included; without it files are 5–7 %
smaller: 53.1, 34.0 and 19.7 bytes per entity in the first, second and
fourth columns):

|  | Mixed, unrounded | Mixed, rounded to 0.01 | Lines, unrounded | Lines, rounded |
|---|---|---|---|---|
| `.cad` compressed, bytes/entity | 56.5 | 36.2 | 38.3 | 21.0 |
| `.cad` uncompressed | 113.4 | 113.4 | 90.7 | 90.7 |
| Same tags as binary DXF | 103.2 | 103.2 | 84.7 | 84.7 |
| ASCII DXF R12 export¹ | 352.4 | 315.0 | 135.6 | 103.7 |
| Write snapshot | 0.48 s | 0.49 s | 0.32 s | 0.43 s |
| Read snapshot (decode all) | 0.32 s | 0.39 s | 0.25 s | 0.31 s |
| Read one object via the map (cold) | 10.5 ms | 11.0 ms | 8.6 ms | 7.7 ms |
| Read one object, file already open | 0.41 ms | 0.39 ms | 0.43 ms | 0.38 ms |
| Incremental save of 10 objects | 3.5 ms | 3.6 ms | 3.0 ms | 2.8 ms |
| File growth per incremental save | 12.2 KB | 11.9 KB | 8.6 KB | 8.5 KB |
| Read snapshot + 100 change sets | 0.38 s | 0.42 s | 0.31 s | 0.27 s |

**Format 1.1**, measured the same day on the same machine (where it
differs):

|  | Mixed, unrounded | Mixed, rounded to 0.01 | Lines, rounded |
|---|---|---|---|
| `.cad` compressed, bytes/entity | 68.6 | 50.4 | 31.9 |
| `.cad` uncompressed | 210.5 | 210.5 | 172.6 |
| Write snapshot | 0.80 s | 0.94 s | 0.76 s |
| Read snapshot (decode all) | 0.47 s | 0.47 s | 0.38 s |
| Read one object via the map (cold) | 6.1 ms | | |
| File growth per incremental save | 19.7 KB | | |

What changed it, measured on the mixed unrounded drawing: the record
diet took 68.6 to 63.5 bytes per entity with the old codec (and halved
the uncompressed records); DEFLATE and splitting brought it to 53.1, of
which splitting alone is 8 %. The `Best` DEFLATE level gains only 0.2 %
for twice the write time, so `Default` is the default; `Fast` is 3 %
larger and 25 % faster to write. A cold lookup is slower than in 1.1
because it inflates and keeps the whole object map; an open file
(`NativeFile`) then answers each lookup in under half a millisecond.

¹ The R12 export writes only geometry and layer names, with 10 decimal
places, and explodes blocks. It is a reference point, not a lossless
equivalent.

**Comparing with DWG.**
- **Estimate for one line:** from the spec's field encodings, a DWG line
  with unrounded coordinates takes roughly 50–55 bytes. That is our
  estimate, not a measurement.
- **Where each format wins:** DWG's bit packing wins before compression.
  But it shifts values off byte boundaries, which limits what its
  compressor can find, while byte-aligned `.cad` records compress further.
  With rounded coordinates, `.cad` lines take 31.9 bytes.
- **Unmeasured:** we expect `.cad` at or below DWG size on typical
  drawings. Measure it once the DWG corpus exists.

**One fix already made:** the reader rebuilt the object index after every
change set. Reading with 100 change sets went from 3.98 s to 0.54 s.

**Size of small files.** Every 2.0 file carries its schema (about 3.6 KB
compressed) and recovery data, so a new empty drawing is 5.7 KB instead
of 1.7 KB.

## 4. Fix before freezing the format

In priority order. Items 1–3, 9 and 11 change how data is stored in ways an
older reader could not follow, so they landed together as format 2.0; the
rest can also come later. **Status (2.0):** done: 1, 2, 3, 5, 6 (writing),
9, 11; partly: 4 (the directory is compressed instead of chained); open:
6 (reading), 7, 8, 10.

1. **Record overhead.**
   - Plan:
     - omit fields that hold their default value (DXF itself omits 39 and
       210 when default);
     - replace the `"AcDbEntity"` string with the space tag alone;
     - store type names only for custom classes (fixed types follow from
       the DWG type number);
     - store each record's key as the difference from the previous key
       (2.0 does this for every handle, in the split streams).
   - Measured (§5.2): records less than half their size uncompressed
     (205 → 90 bytes per entity), and 13–25% smaller compressed with
     today's codec. Reading and writing get faster too.
   - **Done (2.0).** The schema (item 11) holds the defaults.
2. **A codec with entropy coding** (codec 2), because DWG's §4.7 LZ77
   has none. DEFLATE (RFC 1951) is the simplest candidate; an ANS-based
   design in the style of Zstandard (RFC 8878) is faster and stronger but
   larger. Written from the RFC, with no dependency. Measured: 9–15%
   smaller than LZ77 on the same records. Codec 1 stays for DWG reading
   and as an option.
   - **Done (2.0):** DEFLATE from RFC 1951 (`codec`), three levels;
     checked against Python's zlib only as a source of test vectors. The
     PNG writer uses it too.
3. **Split each object page into streams** (structure, numbers, integers
   and handles, strings), as DWG splits each object's data, strings and
   handles from R2007 on. Measured: a further 5–8%.
   - **Done (2.0)** for every section of records, with handles as
     per-code differences: 8 % on the benchmark (FORMAT.md §4.2).
4. **Directory rewritten on every incremental save.** That costs 15–20 KB
   per save on a 200,000-entity file, against about 1 KB of actual
   changes.
   - Plan: a directory chain. Each incremental save writes only its new
     entries plus a pointer to (and CRC of) the previous directory; a full
     save writes a flat one again.
   - Crash-safety tests must be extended to cover it.
   - **2.0** compresses the directory instead (19.7 → 12.2 KB per save,
     with two copies). The chain is still the better fix for very large
     files.
5. **Random access re-reads the directory and the whole object map per
   lookup** (8–9 ms).
   - Plan: a `NativeFile` handle that opens once and keeps both in memory.
   - Effect: a lookup then costs one 64 KiB page. A phone can open a large
     drawing by reading only the map and the pages on screen.
   - **Done (2.0):** `NativeFile`; 0.4 ms per lookup once open.
6. **Single-threaded compression** runs at 30–40 MB/s.
   - Plan: pages are independent, so compress and decompress them on all
     cores with `std::thread`. Keep a single-thread path for WebAssembly.
   - **Done for writing (2.0)**, with `std::thread::scope`; one thread
     when only one core or one large section. Reading is still one thread.
7. **Back `.cdl` with an operating-system lock.** Rust's standard library
   has `File::try_lock` since 1.89, so this stays dependency-free.
   - Effect: a crashed holder's lock clears instantly on local disks.
   - Caveat: on Windows the OS lock blocks other readers, so the "who"
     text needs its own file or byte range.
   - Keep the heartbeat for network and cloud-sync folders.
8. **Respect AutoCAD's `.dwl`/`.dwl2`** when opening a DWG from a shared
   folder (open read-only and show who has it), and consider writing them.
9. **Reserve the hooks for §5.3 and §5.4**: directory flag bits for
   "encrypted" and "signed" sections, and the section names `Keys`,
   `Signatures` and `Recovery.N`; also `Schema` (item 11), `Brep.N` for
   3D solids and `Embedded.N` for packaged files (§7). A 1.0 reader then recognises an
   encrypted file and says so, instead of failing on it.
   - **Done (2.0):** entry flag ENCRYPTED and header flags SIGNED and
     ENCRYPTED reserved; critical and informational flag bits; unknown
     codecs make one section unreadable, not the file. Recovery became one
     `Recovery` section (§5.3).
10. **Label the spec "1.0 draft"** until the DWG corpus passes and items
    1–3, 9 and 11 are done, then freeze. After that, only additive changes
    until a 2.0.
11. **A `Schema` section** (§1.1): for each record kind, its group codes,
    value types, defaults and meanings, including our 2000+ codes and the
    layouts of plugin objects. It makes the format describe itself, so
    the dialect cannot drift silently and a reader can show the fields of
    a kind it does not know. It also carries the default values that item
    1 leaves out, so the two land together.
    - **Done (2.0).** A test checks the record code against it field by
      field, and FORMAT.md §8.2 is generated from it.

## 5. Taking DWG's form where it pays: compactness, error correction, encryption

### 5.1 How DWG is built, and what we take from it

The container already follows DWG R2004: named sections cut into pages,
each compressed and checked, a section map, a handle map and a second copy
of the file header. The rest of DWG's design, from the ODA specification
and Autodesk's documentation:

| DWG feature | What it does | For `.cad` |
|---|---|---|
| Objects bit-coded in a fixed field order per type, with 2-bit codes for common values such as 0.0, 1.0 and the default extrusion (ODA spec ch. 2 and 20) | Small before compression | Take the idea (omit defaults, short references), keep the tags: a fixed order saves only 1–5% after compression (§5.2) |
| Each object's data, strings and handles in separate streams (R2007+) | Similar values sit together and compress better | Take it, per page: 5–8% smaller |
| References stored relative to the object's own handle (§2.13 codes 6, 8, A, C) | Short references | Take it: keys and references as differences |
| LZ77 on 0x7400-byte pages, no entropy coding (§4.7) | Fast, but leaves redundancy | Add an entropy-coding codec: 9–15% smaller |
| Reed–Solomon (255,239) on system and data pages, in R2007 only; R2010 went back to the R2004 container, which only detects damage | Corrects scattered byte errors | Not per page: erasure-coded recovery pages instead (§5.3) |
| File header and page headers XOR-masked with fixed values (§4.1, §4.6) | Obfuscation, not security | No |
| Password encryption (the AcDb:Security section names a Windows cryptographic provider; Autodesk says RC4), AutoCAD 2004–2015, removed in AutoCAD 2016 | Weak, and gone from AutoCAD | No; modern authenticated encryption, later (§5.4) |
| Digital signatures (security flags "sign data" and "add timestamp", AcDb:Signature section) | Shows a file is unchanged and who issued it | Yes, per change set (§5.4) |

### 5.2 Compactness, measured

Each layout below was applied to the same 200,000 generated entities as
§3, paged at 64 KiB and compressed page by page. Only object pages are
counted, which is why today's figures sit a few bytes under §3's
whole-file ones. DEFLATE (zlib, level 9) and LZMA (preset 6) come from
Python's standard library and are there as yardsticks; nothing was added
to the build. Reproduce with `tools/compaction_lab.py` (its header gives
the commands). Bytes per entity, as LZ77 (today's codec) / DEFLATE / LZMA:

| Layout | Mixed, unrounded | Mixed, rounded to 0.01 | Lines, rounded |
|---|---|---|---|
| Today | 64.6 / 57.5 / 53.5 | 46.5 / 39.5 / 36.0 | 27.8 / 22.3 / 20.1 |
| Without type names and entity markers | 64.0 / 56.9 / 52.7 | 45.6 / 38.6 / 34.8 | 27.5 / 21.9 / 19.1 |
| + default values omitted | 59.5 / 54.4 / 51.0 | 40.0 / 35.3 / 32.3 | 23.9 / 20.5 / 18.1 |
| + keys as differences (the record diet, §4 item 1) | 56.0 / 51.2 / 48.5 | 36.5 / 32.2 / 29.8 | 20.9 / 17.6 / 16.1 |
| + pages split into streams by value type | 53.3 / 48.4 / 47.4 | 33.5 / 29.4 / 26.1 | 19.5 / 16.5 / 13.7 |
| + fixed field order per kind, no tags (DWG's form) | 51.7 / 47.4 / 46.4 | 32.0 / 28.4 / 25.1 | 19.1 / 16.3 / 13.4 |
| Uncompressed: today → record diet → fixed order | 205 → 90 → 70 | same | 167 → 72 → 58 |

What the numbers say:
- **The record diet is the first win:** 13–25% smaller compressed with
  today's codec, and less than half the size uncompressed, which also
  speeds up reading, writing and change sets.
- **An entropy-coding codec is the second:** on split pages DEFLATE is
  9–15% smaller than LZ77, and LZMA 11–30%. DWG's LZ77 is the weakest
  part of what we borrowed.
- **Splitting pages into streams** adds 5–8% with LZ77 (up to 15% with
  LZMA).
- **DWG's fixed field order adds only 1–5%** once pages are compressed,
  because the compressor already removes most of the tag overhead. Not
  worth giving up self-description and the forward compatibility of §6
  of the spec.
- **Tricks that did not help:** shuffling the bytes of doubles (alone,
  or after XOR with the previous value of the same field) made rounded
  data 1.5 to 2.2 times larger, because it breaks the exact repeats LZ
  finds, and gained nothing on unrounded data. Storing doubles
  as exact decimal integers (x = k / 10^e) cut uncompressed size by
  40–50% on rounded data, but after compression it gained 1–3% with LZ77
  and 7–13% with DEFLATE, and lost 2–4% with LZMA.
- **Page size:** 64 KiB is right. 16 KiB pages are 5% larger; 256 KiB and
  1 MiB pages gain 1–2% with LZ77 (5–6% with LZMA).
- **Limits of the test:** random geometry has no spatial locality, and
  unrounded random coordinates cannot be compressed at all. Real drawings
  are rounded and local (neighbouring entities share points), so
  coordinate deltas and decimal coding need measuring on the corpus.

Projected, record diet + streams + a DEFLATE-class codec: 25% smaller on
the unrounded worst case, 37% on mixed rounded (46.5 → 29.4 bytes per
entity) and 41% on rounded lines (27.8 → 16.5). An LZMA-class coder
reaches 27–51%, at a cost in speed.

### 5.3 Error correction

**What we have:** detection everywhere (CRC-32 per section and per header
copy), two header copies, records framed so damage stays local, saves
that survive a crash at any point, and older directories left behind by
incremental saves.

**Weak spots:**
- Compression spreads damage: one flipped bit can garble the rest of its
  page.
- Detection without correction: a page that fails its CRC is lost.
- The directory and object map exist once per generation.

**What DWG shows:** R2007 Reed–Solomon-encoded every page (6.7% overhead;
up to 8 bad bytes corrected per 255). R2010 went back to the R2004
container, which only detects damage. Disks and SSDs already correct bit
errors themselves. The damage files actually suffer is mostly whole
regions: a copy or sync cut short, lost sectors or blocks, software bugs.

**Built in 2.0:** recovery data and two directory copies (FORMAT.md
§7.7, §2, §4). The recovery data is XOR parity over fixed-size blocks of
the snapshot's sections rather than over pages: blocks of 64–4096 bytes,
each with a CRC to find it, one parity block per about 16 blocks. It
rebuilds any run of damage inside the sections up to about a sixteenth
of the file, wherever it starts, including across section boundaries,
for 6–11 % of the file, and checks its own layout so damage reaching its
parity only costs the stripes it hits. It sits between the two directory
copies, so a copy cut short still opens from the front header and
directory A. Readers use it on their own when the snapshot is damaged.
Reed–Solomon (several bad blocks per stripe), a separate copy of the
object map, and `cad recover` (which would also find older trailers in a
file cut short after an incremental save) remain.

**Plan:**
1. **Recovery pages (erasure coding).** For each group of N object pages
   write M parity pages, computed byte by byte across the group with
   Reed–Solomon over GF(256), as RAID-6 and PAR2 do. The CRCs already say
   which pages are bad, so any M bad pages in a group are rebuilt exactly.
   - One XOR parity page per 16 pages (6.25%) rebuilds any single lost
     page in the group and is a few lines of code.
   - Reed–Solomon with M ≥ 2 for an archival setting (N + M ≤ 255).
   - Stored as `Recovery.N` sections. An older writer drops them on a full
     save, which is correct: they are rebuilt from the pages.
2. **Two copies of the directory**, and of the object map, which is small
   (and can also be rebuilt from the pages).
3. **`cad recover`**, the counterpart of AutoCAD's RECOVER. It rebuilds a
   file from whatever still parses, falling back to older directories and
   rebuilding the object map from the pages, and it reports what was lost.

**Not planned:** Reed–Solomon on every page the way R2007 did. It costs on
every read and write and guards against the failure hardware already
handles.

### 5.4 Encryption and signatures

**DWG's record:**
- The "encrypted" header data and page headers are XOR-masked with fixed
  values. Anyone can undo that.
- Password protection (AcDb:Security, from AutoCAD 2004) used RC4, which
  is now considered weak. Autodesk removed it in AutoCAD 2016 and points
  users to external tools: encrypted archives, password-protected PDFs,
  AES-256 utilities and access control.
- Digital signatures remain.

**Is encryption worth having? Yes, as an option, done properly.**
- **What it buys:** drawings travel by email, USB stick, shared link and
  phone. Protection that goes with the file keeps customer parts and bids
  safe wherever the file lands.
- **What a native format adds over zipping the file:** the container stays
  readable, so tools can say "encrypted" rather than "corrupt". Incremental
  saves still work, each change set encrypted with the file key.
- **Costs:**
  - A lost password means a lost drawing, so offer a recovery key.
  - Collaboration needs key sharing.
  - The preview and metadata leak unless they are encrypted too.
  - Hand-written cryptography is where silent, catastrophic bugs happen.

**Design (hooks before 1.0, §4 item 9; implementation later):**
- **Section encryption:** a random 256-bit file key encrypts each section
  after compression, with authenticated encryption (ChaCha20-Poly1305,
  RFC 8439, or AES-256-GCM).
  - A fresh nonce for every write.
  - The section's name and generation as associated data, so sections
    cannot be swapped or replayed.
  - A MAC over the directory, so change sets cannot be dropped or rolled
    back unnoticed.
- **Key wrapping:** the file key is wrapped once for each way in, stored
  in a `Keys` section:
  - a password, through Argon2id (RFC 9106);
  - a printed recovery key;
  - each recipient's public key (X25519, RFC 7748), so a drawing can be
    shared with named people without sharing a password.
- **Left in the clear:** the file header, the directory and a flag per
  section, plus optionally the preview (DWG also controls its preview
  and properties separately).
- **CRCs** stay on the stored (encrypted) bytes, so damage can be detected
  without the key.
- **Where the crypto comes from:** use audited implementations, not our
  own. Permissively licensed (MIT/Apache-2.0) RustCrypto crates behind a
  feature flag, or each platform's own (CryptoKit, Windows CNG, Android
  Keystore). This is the one place where an exception to the
  no-dependency rule pays for itself. If the rule has to hold, implement
  from the RFCs, test against their published vectors, and have the code
  reviewed externally before release.

**Built in 2.0:** the SHA-256 chain (FORMAT.md §7.6): a `Digest` of the
snapshot, and each change set records the chain's value before it, so the
chain's last value identifies the file's whole history. `cad inspect`
checks it. Signing that value is the next step.

**Do signatures first:**
- Sign each change set (Ed25519, RFC 8032) and chain SHA-256 hashes across
  change sets.
- Anyone can then check that revision C is exactly what its author
  released, and that no history was rewritten.
- For a fabrication shop that is worth more day to day than secrecy: the
  part on the cutting table matches the approved drawing.
- Readers need only public keys, so there is no secret to manage.

### 5.5 Order of work

| When | Work |
|---|---|
| Done in 2.0 | Record diet; DEFLATE; streams per section; reserved hooks for encryption and signatures; schema; recovery data; two directory copies; SHA-256 chain (§4 items 1–3, 9, 11) |
| Next (additive) | `cad recover`; Reed–Solomon recovery for archives; a directory chain |
| Next | Ed25519 signatures over the chain's value |
| After 1.0, once the dependency question is settled | Encryption: authenticated encryption per section, Argon2id passwords, recovery key, X25519 recipients |
| With the DWG corpus | Re-run `tools/compaction_lab.py` on real drawings; decide on coordinate deltas and decimal coding |

## 6. Parity with DWG: what the model and format must carry

**Objects:**
- Every entity type in current DWG, not only the ~25 decoded now.
  Missing includes:
  - basic and annotation: VIEWPORT, LEADER/MULTILEADER, TABLE,
    TOLERANCE, RAY/XLINE, MLINE, WIPEOUT;
  - references: IMAGE, and PDF/DGN/DWF underlays;
  - 3D: REGION/3DSOLID/BODY, surfaces and meshes.
- Complete table records: linetype dash patterns, text style fonts, all
  ~70 dimension-style variables, UCS, views and viewports, application
  IDs.
- Dictionaries, XRECORDs and extension dictionaries, so third-party data
  survives. (XDATA is done.)
- Groups, draw order (sort-ents table), layer states and filters.

**Paper space and output:** layouts, viewports, plot settings and plot
style references.

**Annotation:** annotative scales, fields, multileader and table styles.

**References:** xrefs, images and underlays, plus a file-dependency list
(fonts, xrefs, images, plot styles).

**3D bodies:** keep ACIS SAT/SAB data as blobs first; a modelling kernel is
a separate project.

**Dynamic blocks and constraints:** preserve their evaluation graphs first,
evaluate them later.

**Metadata:** summary info, custom properties, revision history, and a
raster thumbnail for file browsers.

**DWG export:** a writer that uses the preserved DWG bytes and the model,
tested by round trip (DWG → `.cad` → DWG → AutoCAD/TrueView opens it
unchanged).

**Repair:** the audit exists; add a recover mode that rebuilds a file from
its readable parts (§5.3).

**Security:** signatures and encryption, planned in §5.4.

## 7. Beyond DWG: what owning the format allows

- **History built in.** Change sets already record author and time. Keeping
  them (compacting only on request) gives:
  - version history inside the file;
  - undo across sessions;
  - "show me what changed between Tuesday and today";
  - object-level drawing diff between revisions.
- **Collaboration.**
  - Change sets are the natural unit to send between devices.
  - Per-object revision counters let offline edits merge.
  - The `.cdl` heartbeat grows into live presence.
- **Fabrication data as first-class fields.** Store directly in the part,
  not as layer-name conventions or XDATA:
  - cut, etch or mark roles;
  - kerf, lead-ins/outs, pierce points and cut order;
  - material, grade and thickness;
  - quantity.
  Cache computed quoting data per part (cut length, pierce count, area,
  weight).

  **Status (2.0), and a decision needed.** DESIGN §1 (Decided) says the
  drawing is CAD, not CAM: machine operations such as cut or etch are not
  part of the drawing, and which layers a job uses is an export setting.
  The first two bullets are machine operations, so 2.0 does not store
  them. What 2.0 has, marked Proposed:
  - **part data** on block records (the model space record for a drawing
    of one part): material, grade, thickness and quantity, which describe
    the part rather than how it is cut (FORMAT.md §8.2; `cad part`);
  - **takeoff** (`cad_core::takeoff`, `cad takeoff`): length, closed
    contours, holes, open paths, net area and weight, measured on the
    layers the caller names (default `VISIBLE`). Computed when asked,
    not cached in the file, because which geometry counts is the job's
    choice.

  Storing roles, kerf, leads, pierce points and cut order in the file
  would need DESIGN §1 changed first; the format can take them as new
  fields or an operations section when that is decided.
- **Construction takeoff:** quantities by layer and block, with units, kept
  with the drawing.
- **Large drawings on small devices.**
  - Add a spatial-index section.
  - Group pages by area and layer.
  - Then a viewer loads only what is on screen.
- **Packaged drawings.** Optionally embed xrefs, images and fonts (where
  font licenses allow) in the file, so one file travels complete.
- **Self-describing custom objects.** A schema section describing plugin
  record layouts, so their data displays and survives without the plugin.
  DWG proxies keep only graphics.
- **Deterministic output.** The same drawing gives the same bytes, apart
  from the save time. That gives clean diffs in version control and
  de-duplication.
- **Tamper evidence.** An optional SHA-256 chain over change sets. SHA-256
  is small enough to implement and verify against published test vectors.

## 8. Where this leaves us against other DWG-based CAD

**Competitors:**
- **ARES Commander** comes from Graebert, a founding member of the Open
  Design Alliance. Graebert has presented it as "leveraging ODA DWG
  libraries". It runs on Windows, macOS and Linux, with ARES Touch for
  mobile and ARES Kudo in the browser.
- **DraftSight** is Dassault Systèmes' product. It was co-developed with
  Graebert on ARES technology; Graebert was still described as its
  technology partner in 2016.

**How they store drawings:** products like these save DWG as their native
format.
- Anything new they invent has to fit inside DWG's structures: custom
  objects (which AutoCAD shows as proxies), XDATA or XRECORDs.
- The container, save model and version support follow Autodesk's format.
  DWG reading and writing typically come from the ODA's shared libraries.

**Where we differ:**
- **Ownership:** we own the storage layer (history, collaboration,
  fabrication data, indexing) and the code (Rust, no dependencies, memory
  safe, one core for desktop, mobile and WebAssembly).
- **DWG's role:** DWG becomes one import/export target among others.
- **The price:** we must earn DWG fidelity ourselves. That is why the
  real-file corpus and the DWG writer stay at the top of the roadmap.

Sources:
- [Graebert — ARES CAD Platform leveraging ODA DWG libraries (ODA Summit 2021)](https://www.graebert.com/blog/events/ares-cad-platform-leveraging-oda-dwg-libraries-for-mechanical-bim-oda-summit-2021/)
- [Graebert — DraftSight co-developed with Dassault Systèmes](https://www.graebert.com/blog/white-paper/graebert-with-dassault-systemes-draftsight/)
- [Graebert — Dassault Systèmes testimonial (2016)](https://www.graebert.com/in/blog/oem-partners/testimonial-dassault-systemes/)
- [Open Design Alliance — Drawings SDK](https://www.opendesign.com/products/drawings)
- [Autodesk — ISAVEPERCENT and incremental saves](https://www.autodesk.com/support/technical/article/caas/sfdcarticles/sfdcarticles/ISAVEPERCENT-system-variable-affecting-file-size-performance-and-interoperability.html)
- [CAD Forum — What are .DWL files](https://www.cadforum.cz/en/qaID.asp?tip=3186)
- [Autodesk — Password protection feature in AutoCAD products](https://www.autodesk.com/support/technical/article/caas/sfdcarticles/sfdcarticles/Password-protection-feature-in-AutoCAD.html)
- [Autodesk — About Passwords and Encryption (AutoCAD 2016 help)](https://help.autodesk.com/cloudhelp/2016/ENU/AutoCAD-Core/files/GUID-4E32143C-AA81-4A4E-9191-B32C4AA259EE.htm)
- Open Design Alliance, *Open Design Specification for .dwg files* v5.4.1: §4.1 (R2004 file header: security flags, masked header data), §4.6 (masked page headers), §5.13 (R2007 Reed–Solomon encoding), chapter 6 (R2010 returns to the R2004 container), chapter 19 (AcDb:Security)

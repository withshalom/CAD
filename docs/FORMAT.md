# Native file format (`.blc`) — version 2.10

This is the byte-level specification of BlackCAD's native drawing format,
`.blc` (called `.cad` before 2.10), written and read by the `cad-native`
crate (MIT OR Apache-2.0 since 0.10.0; `cad_core::native` re-exports it).
It is original to this project: a DWG-style container whose records use
DXF group codes (not a binary DXF file; see `FORMAT-ROADMAP.md` §1). It borrows from published designs:

- **Records** follow *binary DXF* (AutoCAD DXF Reference, openly published
  by Autodesk): a record is a list of group codes and values, and a group
  code means what the DXF reference says it means (10 = first point,
  40 = radius, 62 = color index, 330 = owner, 1000–1071 = extended data,
  …).
- **The container** follows the *DWG R2004 file structure* (ODA Open Design
  Specification for .dwg files v5.4.1, chapter 4): named sections, each
  compressed and CRC-checked separately, found through a directory, with an
  object map from handle to location. Splitting records into streams by
  kind of value follows DWG R2007 and later, which keep an object's data,
  strings and handles apart (spec chapter 5).
- **Compression** is DEFLATE (RFC 1951) from 2.0 on; files of 1.x use
  DWG's own page compression (spec §4.7), which readers still accept.
- **The hash** is SHA-256 (FIPS 180-4).

Deliberate differences from DXF are listed in [§13](#13-differences-from-dxf).
Nothing here is compatible with DXF or DWG readers; the aim is a format
DXF and DWG map onto cleanly.

The format is proprietary for now. Whether to publish this document is a
separate decision. It is a draft until the DWG test corpus passes
(FORMAT-ROADMAP §4 item 10); until then it may still change.

**Revisions.**

- **1.0**: first version.
- **1.1**: object kinds 26 LTYPE (with its pattern), 27 STYLE and 28
  DIMSTYLE; header variables for display units, the current text style,
  linetype and dimension style (§7.2). A 1.0 reader keeps the new kinds
  and variables as unknown data and writes them back (§6), and 1.1 reads
  1.0 files unchanged with one exception: 1.0 wrote lineweight 0 for
  drawings from R13/R14 files, which have no lineweights, so in a 1.0
  file whose source is AC1012 or AC1014 a lineweight of 0 reads as by
  layer (29) on entities and as the default (31) on layers.
- **2.0** (a new major version: 1.x readers refuse it):
  - Records leave out every field equal to its default, the type name of
    DWG's fixed object types, and the `100 "AcDbEntity"` marker (§8).
  - The `Schema` section describes every record kind: fields, types,
    defaults and meanings (§7.5). The table in §8.2 is generated from the
    same source as the code.
  - Sections are compressed with DEFLATE (codec 2), and sections of
    records are first split into streams by kind of value, with handles
    stored as differences (§4.2). The directory is compressed too, and
    written twice, one copy for each header copy (§4).
  - Directory entries carry flags; two are reserved for encryption and
    signatures (§4.1, §3).
  - A SHA-256 hash chain covers the snapshot and every change set
    (§7.6, §10).
  - `Recovery` data rebuilds damaged bytes of the snapshot (§7.7).
  - Drawing properties (§7.2) and part data on block records (§8.2,
    *Proposed*).

  2.0 reads 1.x files, and a change set appended to a 1.x file is written
  the 1.x way, so the file stays readable by 1.x readers (§10). A full
  save writes 2.0.
- **2.1**: class records keep the number of objects of the class and the
  DWG and maintenance versions the class came with (§7.3), which a DWG
  writer needs; table records the core keeps only by name keep their DWG
  bytes too (§7.4).
- **2.2**: object kinds 29 VPORT, 30 DIMSTYLE_RECORD (every dimension
  variable), and the non-graphical objects 31 DICTIONARY, 32 XRECORD,
  33 ACDBPLACEHOLDER, 34 DICTIONARYVAR, 35 SCALE, 36 LAYOUT and
  37 MLINESTYLE, with private codes 2144–2182 (§8.2). Drawings read from
  DWG or DXF store these objects as records instead of DWG bytes, and a
  2.2 reader turns the DWG bytes of such objects in older files into
  records. A 2.1 reader keeps the new kinds as unknown data (§6).
- **2.3**: object kind 38 GROUPS keeps a style object as its DXF groups
  (code and typed value, in DXF order; private code 2183 names the DXF
  object type): visual styles, materials, table styles and their cell
  style maps, multileader, section and detail view styles, and dimension
  associations, which earlier versions kept only as DWG bytes (or not at
  all, from DXF). A 2.3 reader turns the DWG bytes of such objects in
  older files into records. The codec 1 stream ends `11 00 00` (§4).
- **2.4**: object kinds 39 XLINE / RAY (construction lines), 40 LEADER
  and 41 VIEWPORT (the windows on a layout), with private codes
  2184–2188; HATCH records end with the pattern's line families (77, 78
  and the items 53, 43, 45, 79, 49) and the seed points (98, 2188), which
  a 2.3 reader keeps as fields from a newer writer (§6). Hatches in older
  files have no line families; readers draw them from the pattern
  library by name.
- **2.5**: ATTRIB / ATTDEF records end with lock position (280, written
  only when set), which AutoCAD 2007 and later keep with each attribute.
  A 2.4 reader keeps it as a field from a newer writer (§6); records
  without it are as 2.4 wrote them.
- **2.6**: HATCH records end with the boundary objects of each path
  (2189, then 97 and the handles 2190), written only when a path has
  some: the objects an associative hatch was made from. A 2.5 reader
  keeps them as fields from a newer writer.
- **2.7**: object kind 42 TOLERANCE (geometric tolerance frames), with
  private codes 2191–2193 for what R13 and R14 DWG files store and later
  ones don't. Drawings read from DWG or DXF store tolerances as records
  instead of DWG bytes (or not at all, from DXF). A 2.6 reader keeps the
  new kind as unknown data (§6).
- **2.8**: object kinds 43 IMAGE and 44 WIPEOUT (raster images and
  wipeouts), with DXF's own group codes. The objects an image uses
  (IMAGEDEF, IMAGEDEF_REACTOR, RASTERVARIABLES) are kind 38 GROUPS
  records. A 2.7 reader keeps the new kinds as unknown data.
- **2.9**: object kind 45 MLINE (multilines), with DXF's own group
  codes: per vertex its point, direction and miter, then for each
  element of the style its segment parameters (74, 41) and area fill
  parameters (75, 42). A 2.8 reader keeps the new kind as unknown data.
- **2.10**: the format is BlackCAD's `.blc` (its lock `.bcl`): `.cad`
  and `.cdl` go to CADCraft's native format, which shares this container
  with its own records. Header bytes 40–41, reserved before, say whose
  records a file holds (0 BlackCAD's, 1 CADCraft's); earlier files have
  0 there and read as before. A 2.9 reader ignores the field.

The container (§2–§5, §7.7) and the tagged records (§6) are public in the
`cad-native` crate (`container`, `tags`, since 0.17.0), so CADCraft's
`.cad` keeps its own records in the same container, marked in the header.
Everything from §7.1 on that names records describes BlackCAD's.

## 1. Conventions

- All fixed-width integers and floats are **little-endian**. Floats are
  IEEE 754 binary64.
- **varint** = unsigned LEB128: 7 bits per byte, low group first, high bit
  set on every byte but the last. At most 10 bytes (64 bits).
- **zigzag**: a signed value v stored as an unsigned one, `(v << 1) ^
  (v >> 63)`, so small negative and positive numbers stay small.
- Strings are **UTF-8**, prefixed with their byte length as a varint. There
  is no code page: DWG text is converted to Unicode on import.
- **CRC-32** is the standard reflected CRC-32 (polynomial 0xEDB88320,
  initial value and final XOR 0xFFFFFFFF), the same function the DWG
  R2004 format uses (`dwg::crc::crc32` with seed 0).

## 2. File layout

```
offset 0      ┌──────────────────────────────┐
              │ file header (48 bytes)       │  copy 1, points at directory A
              ├──────────────────────────────┤
              │ section data …               │  Meta, Header, Classes, Schema,
              │                              │  Objects.0 … Objects.N,
              │                              │  ObjectMap, DwgPreserved,
              │                              │  Preview, Digest
              ├──────────────────────────────┤
              │ section directory A          │
              ├──────────────────────────────┤
              │ Recovery section             │  parity over the section data
              ├──────────────────────────────┤
              │ section directory B          │  the same bytes as A
              ├──────────────────────────────┤
              │ file header (48 bytes)       │  copy 2 ("trailer"), points at B
              ├──────────────────────────────┤
              │ Journal.G section            │  ┐ appended by each
              │ directory A, directory B     │  │ incremental save;
              │ trailer (new)                │  ┘ repeated
end of file   └──────────────────────────────┘
```

A full save writes everything from scratch to a temporary file beside the
target (`name.blc.saving`), flushes it, and renames it over the target, so
a failed save never damages the previous file. An incremental save appends
(§10).

The recovery data sits between the two directory copies, so damage at the
end of the file (a copy cut short, a lost last sector) leaves directory A
and the front header, and damage before the recovery data leaves
directory B and the trailer. Without recovery data the two copies are
side by side, as they are after an incremental save. Format 1.x files have
one directory, which both header copies point at.

## 3. File header

Both copies have the same layout.

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | Magic `89 43 41 44 0D 0A 1A 0A` (`\x89CAD\r\n\x1a\n`) |
| 8 | 2 | Format major version (2) |
| 10 | 2 | Format minor version (10) |
| 12 | 4 | Flags (below) |
| 16 | 8 | Generation: 1 for a new file, +1 per save of any kind |
| 24 | 8 | Directory offset from start of file |
| 32 | 4 | Directory length in bytes |
| 36 | 4 | CRC-32 of the directory bytes |
| 40 | 2 | Whose records the file holds: 0 BlackCAD's (§8; every file before 2.10), 1 CADCraft's |
| 42 | 2 | Reserved, zero |
| 44 | 4 | CRC-32 of bytes 0–43 of this header |

**Flags.** Bit 0: the file has at least one journal section. Bit 1
(reserved, 2.0): the file carries a signature over its hash chain. Bit 2
(reserved, 2.0): some sections are encrypted, as their directory entries
say. No writer sets bits 1 and 2 yet; readers ignore bits they do not
know.

**Whose records.** The container is shared: CADCraft's native `.cad`
files use it with CADCraft's own records, and say so at offset 40. A
reader that finds records it doesn't hold refuses the file with a message
naming whose it is, rather than misreading it. BlackCAD writes 0.

The magic's first byte has the high bit set (detects 7-bit transfers), the
CR LF and LF detect newline conversion, and 0x1A stops text-mode readers.

**Choosing a copy.** A reader decodes both copies (offset 0, and the last
48 bytes of the file) and discards any whose magic or CRC is wrong, so a
file is recognised even if its first bytes are damaged. It tries the valid
copy with the higher generation first. A copy is usable when its directory
is in bounds and passes its CRC, and, if the directory has a
`Journal.G` section for that copy's own generation G, that section
decodes and passes its CRC (otherwise the incremental save that wrote the
copy did not finish). If no copy passes, the reader accepts the first
copy with a sound directory and reports the damaged change set (§10).

In 2.0 the two copies of one state differ only in the directory they
point at, so a damaged directory copy is not fatal: the reader uses the
other copy and reports it.

**Versions.** Majors 1 and 2 are read; any other is refused. A file of a
newer minor version is read, and unknown data is skipped or kept (§6).

## 4. Section directory

In 2.0 the directory starts with a codec byte: 0, the entries follow as
they are; 2, a varint length then DEFLATE data that inflates to exactly
that many bytes of entries (writers compress directories over 256 bytes
when that saves more than 10 bytes; readers refuse more than 64 MiB). In
1.x the entries start at once.

```
varint  entry count
repeat:
  varint  name length, then name (UTF-8)
  u8      codec (0 stored, 1 DWG LZ77, 2 DEFLATE)
  varint  offset from start of file
  varint  stored length (bytes in the file)
  varint  length of the section's data, decoded
  u32     CRC-32 of the decoded data
  varint  flags (§4.1; 0 in 1.x)
```

If two entries have the same name, the later one wins. Readers ignore
sections they do not know.

**Codecs.**

- 0: bytes stored as is.
- 1: the DWG R2004 page compression (ODA spec §4.7), one stream per
  section, ending with opcode 0x11; it must produce exactly the decoded
  length. Readers stop at the 0x11 and ignore what follows; writers since
  cad-core 0.3.0 add two zero bytes after it (`11 00 00`), as AutoCAD
  ends its own streams. Written by 1.x, and by 2.0 only into change sets appended to
  1.x files.
- 2: raw DEFLATE (RFC 1951; no zlib or gzip wrapper). The stream must end
  exactly at the end of the stored bytes.
- Any other value: a codec of a newer revision. That section cannot be
  read; the rest of the file can.

Writers compress only sections over 64 bytes, and only when that saves at
least 8 bytes.

**Lengths are checked before anything is allocated.** A reader rejects an
entry whose decoded length cannot come from its stored length: not equal
to it for codec 0, more than 256 × stored + 64 for codec 1 (a §4.7 stream
expands at most 255-fold), more than 1032 × stored + 64 for codec 2
(DEFLATE's limit is a 258-byte match per two one-bit codes), four times
that for a split section (§4.2), more than 4 GiB, or larger than the
reader's address space.

**Damage.** A section that fails its CRC is still read, with a warning;
one that cannot be decoded or parsed is skipped with an error diagnostic.
Records are length-framed, so in an object page, `Classes` or
`DwgPreserved` the records before the damage still load. The file opens as
long as one header copy and a directory it points at are intact. Without
`Meta` the source version is taken as AC1032; without `Header` the header
variables are empty. Before reporting damage in the snapshot, a reader
tries the recovery data (§7.7).

### 4.1 Entry flags

Bits 0–31 change how the stored bytes decode: a reader that does not know
one of them cannot read that section, reports it, and reads the rest of
the file. Bits 32–63 are informational and may be ignored.

| Bit | Name | Meaning |
|---|---|---|
| 0 | SPLIT | The decoded bytes are the records split into streams (§4.2); join them to get the records. Only with codec 2; a reader refuses it with any other codec |
| 1 | ENCRYPTED | Reserved: the stored bytes are encrypted. This version reads no encrypted section and reports it as unreadable |

The CRC and the decoded length in the entry are those of the records,
after joining.

### 4.2 Split records

A section of length-prefixed records (`Classes`, `Schema`, `Objects.N`,
`DwgPreserved`, `Journal.G`) may be stored with its values sorted into
four streams, so that similar values sit together before compression:

```
varint  record count
varint  length of each of the 4 streams
stream 0  structure: per record a varint tag count, then per tag its u16
          group code and u8 value type
stream 1  floating point: F64 values and the coordinates of P2 and P3
stream 2  integers: I16, I32, I64 and BOOL values as stored in a tag, and
          handles as zigzag varints of the difference from the previous
          handle with the same group code in the section (0 before the
          first)
stream 3  everything else: STR, BYTES and newer types, with their length
          prefixes
```

Joining walks stream 0 and takes each value from its stream in order; the
streams must be used up exactly. Writers split only records that rebuild
byte for byte (a record kept as found after damage may not) and only when
compressing. Splitting a section of the benchmark drawing makes it about
8 % smaller and faster to read and write (FORMAT-ROADMAP §2).

## 5. Sections

| Name | Written | Contents |
|---|---|---|
| `Meta` | always | File properties (§7.1) |
| `Header` | always | Header variables and drawing properties (§7.2) |
| `Classes` | always (may be empty) | Custom class definitions (§7.3) |
| `Schema` | always (2.0) | The record schema (§7.5) |
| `Objects.0` … `Objects.N` | always (≥ 1) | Object records, in key order (§8). Pages of about 64 KiB of records; a record is never split across pages |
| `ObjectMap` | always | Key → page and offset (§9). Needed only for single-object reads |
| `DwgPreserved` | no | Original DWG bytes of objects not decoded (§7.4) |
| `Preview` | no | SVG of model space, UTF-8, at most 4 MiB |
| `Digest` | always (2.0) | SHA-256 of the snapshot (§7.6) |
| `Recovery` | by default (2.0) | Parity over the snapshot's sections, written between the two directory copies (§7.7) |
| `Journal.G` | no | Change set saved at generation G (§10) |

The snapshot is every section but `Digest`, `Recovery` and change sets.
Writers put `Digest` after the other sections and `Recovery` after the
first directory copy.

Reserved names, not written yet: `Keys` and `Signatures` (encryption and
signatures, FORMAT-ROADMAP §5.4), `Brep.N` (3D solids) and `Embedded.N`
(files packaged with the drawing). Readers skip them like any section
they do not know.

## 6. Tags and records

A **tag** is:

```
u16  group code
u8   value type
...  value
```

| Type | Name | Value encoding |
|---|---|---|
| 0x01 | STR | varint length + UTF-8 |
| 0x02 | F64 | 8 bytes |
| 0x03 | I16 | 2 bytes, signed |
| 0x04 | I32 | 4 bytes, signed |
| 0x05 | I64 | 8 bytes, signed |
| 0x06 | BOOL | 1 byte, 0 or 1 (non-zero reads as true) |
| 0x07 | HANDLE | varint |
| 0x08 | BYTES | varint length + bytes |
| 0x09 | P2 | 2 × F64 (x, y) |
| 0x0A | P3 | 3 × F64 (x, y, z) |
| 0x0B–0xFF | — | Reserved for later revisions. The value is always a varint length and that many bytes, so a reader can step over a tag whose type it does not know |
| 0x00 | — | Never used. A reader treats it as damage |

Unsigned model fields are stored bit-for-bit in the signed type of the same
width (a u16 flag word in an I16, as DXF does). u8 fields use I16.

A **record** is a varint byte length followed by that many bytes of tags.
`Classes`, `Schema`, `Objects.N`, `DwgPreserved` and `Journal.G` are
sequences of records. `Meta`, `Header` and `Digest` are a single tag
stream without a length prefix. `ObjectMap` and `Recovery` have their own
encodings (§9, §7.7) and `Preview` is plain SVG.

Codes below 2000 have their DXF meaning. **Codes 2000 and up are this
format's own** (DXF defines no codes above 1071). Codes 1000–1071 appear
only in extended data (§8.3).

**Defaults (2.0).** A field the schema gives a default (§8.2) is left out
when its value equals the default, and a reader that finds it absent uses
the default. Floating-point values are compared bit for bit, so −0.0 and
NaN are written out. Defaults are part of the format: changing one needs
a new major version, because files leave the field out.

**Forward compatibility.** Records are read in order. A field that may be
absent is taken if the next tag has its code. A reader looks at the code
only, and every value type can be stepped over (types 0x0B and up are
length-prefixed), so no tag a later revision adds can stop a reader. A
newer minor version may:

- **append fields** to a record: after the fields defined here, and in
  object records before the extended data, using group codes that do not
  already appear in that record (fields left out at their defaults could
  otherwise be mistaken for the new ones);
- add **value types** (0x0B and up), **object kinds**, **header
  variables**, **sections** and **entry flags** (§4.1).

**A reader keeps what it does not know, and a writer writes it back
unchanged.** This covers fields a newer writer appended to an object
record, the fields of an object kind the reader does not know, and header
variables it does not know, whatever their value types. So opening a file
in an older version and saving it does not strip newer data. A tag that
cannot be parsed at all is damage: the reader keeps the rest of that
record as it is, reports it, and writes it back unchanged.

Limits, which later revisions design around:

- A full save drops sections the writer does not know, and journal
  operations it does not know are skipped (§10). A revision therefore keeps
  drawing data in object records and header variables, and uses new
  sections only for data that can be rebuilt (indexes, caches, previews).
- `Meta`, `Schema`, `Digest` and `Recovery` are rewritten by every full
  save, and fields appended to `Classes` or `DwgPreserved` records are
  skipped, not kept.
- An older writer may change the fields it knows while carrying newer
  ones along. A revision may only add fields that stay correct when the
  older fields change, or that let the newer reader detect that they are
  stale (for example by recording what they were derived from).

Anything else needs a new major version.

## 7. Non-object sections

### 7.1 Meta

A tag stream in the DXF header style: a `9` tag with a variable name, then
the value tag. Unknown names are skipped with their value.

| Name | Value |
|---|---|
| `$CAD_FORMAT` | 1 STR `"2.0"` |
| `$CAD_WRITER` | 1 STR, e.g. `"cad-core 0.3.0"` |
| `$CAD_SOURCE_VERSION` | 1 STR, DWG version tag of the source (`"AC1032"`) |
| `$CAD_MAINTVER` | 70 I16, DWG maintenance version |
| `$CAD_CODEPAGE` | 70 I16, the source's DWG code page number |
| `$CAD_SAVED` | 160 I64, Unix seconds |
| `$CAD_OBJECTS` | 160 I64, objects in the snapshot (check value) |
| `$CAD_PAGES` | 160 I64, number of `Objects.*` sections |

### 7.2 Header

Same style. Only variables with a value are written.

| Name | Value |
|---|---|
| `$CAD_VERIFIED` | 290 BOOL: header was cross-checked against the objects on import |
| `$CAD_NOTE` | 1 STR, why not verified (*opt*) |
| `$INSUNITS`, `$MEASUREMENT` | 70 I16 |
| `$EXTMIN`, `$EXTMAX`, `$INSBASE` | 10 P3 |
| `$LIMMIN`, `$LIMMAX` | 10 P2 |
| `$LTSCALE`, `$TEXTSIZE`, `$CELTSCALE` (1.1) | 40 F64 |
| `$LUNITS`, `$LUPREC`, `$AUNITS`, `$AUPREC`, `$ANGDIR` (1.1) | 70 I16: linear and angular display format and precision, as DXF defines them |
| `$ANGBASE` (1.1) | 50 F64, radians |
| `$HANDSEED` | 5 HANDLE |
| `$CLAYER`, `$TEXTSTYLE`, `$CELTYPE`, `$DIMSTYLE` (the last three 1.1) | 330 HANDLE (DXF uses the record's name) |
| `$CAD_BLOCK_CONTROL`, `$CAD_LAYER_CONTROL`, `$CAD_STYLE_CONTROL`, `$CAD_LTYPE_CONTROL`, `$CAD_DIMSTYLE_CONTROL` (1.1), `$CAD_MODEL_SPACE`, `$CAD_PAPER_SPACE`, `$CAD_NOD` | 330 HANDLE |
| `$CAD_TITLE`, `$CAD_SUBJECT`, `$CAD_AUTHOR`, `$CAD_KEYWORDS`, `$CAD_COMMENTS`, `$CAD_LAST_SAVED_BY`, `$CAD_REVISION`, `$CAD_HYPERLINK_BASE` (2.0) | 1 STR: drawing properties, the fields of a CAD program's drawing-properties dialog (DWG keeps them in its summary information); written when not empty |
| `$CAD_CREATED`, `$CAD_MODIFIED` (2.0) | 160 I64, Unix seconds; written when not 0 |
| `$CAD_PROPERTY` (2.0, repeated) | 1 STR name, 2 STR value: custom drawing properties, in order, names unique |
| `$CAD_SOURCE_VAR` (repeated) | 2200 STR name, 2201 STR value: every variable read from the source file, as text, in order |

A variable a reader does not know is kept, from its `9` tag up to the next
`9` tag, and written back after the known ones. So is a known variable
whose value does not decode (damage): the reader reports it and leaves
that field unset. Extra value tags after a known variable are not kept,
so a revision adds a new variable rather than extending an existing one.

### 7.3 Classes

One record per class, using the DXF CLASSES section codes:

`0 STR "CLASS"`, `1 STR` DXF name, `2 STR` C++ class name, `3 STR`
application name, `90 I32` proxy flags, `91 I32` number of objects of
the class (2.1; 0 or left out when unknown), `280 BOOL` was a zombie,
`281 BOOL` is an entity class, `2300 I16` class number (500+), `2301 I16`
item class id (0x1F2 entity, 0x1F3 object), `2302 I32` DWG version and
`2303 I32` maintenance version the class says it came with (2.1; left out
when 0).

### 7.4 DwgPreserved

One record per object whose type the core does not decode yet, and (2.1)
per table record it keeps only by name (VIEW, UCS, APPID and the like),
so that a drawing opened from DWG and saved as `.blc` keeps the object
whole for a DWG writer:

`5 HANDLE` object, `1 STR` DWG version tag the bytes belong to,
`2400 I64` handle-stream size in bits (R2010+, else 0), `310 BYTES` the
object's data exactly as stored after its size prefix (CRC not included).

### 7.5 Schema

The file describes its own records: one record per kind, the common parts
first (kind −1, every object; kind −2, every entity), then kinds 0, 1, 2,
… as in §8.2:

| Code | Type | Field |
|---|---|---|
| 2001 | I16 | Kind |
| 2 | STR | Kind name |

then per field, in record order:

| Code | Type | Field |
|---|---|---|
| 2800 | I16 | Group code (as stored, an unsigned 16-bit value) |
| 2801 | I16 | Value type (§6) |
| 2802 | I16 | How the field appears: 1 always; 2 left out when equal to its default; 4 left out when null; 8 depends on other fields; plus 16 a list count; 32 repeated per list item |
| 2803 | STR | Meaning |
| 2804 | the field's type | The default, for fields with flag 2 |

A reader uses the schema to name the fields of records it does not know
(`cad inspect --tags` does) and to see what a newer writer added. It is
generated from the same table as the code, and tests check the record
code against it field by field.

### 7.6 Digest

A tag stream: `9 "$CAD_HASH"`, `1 STR "SHA-256"`; `9 "$CAD_SNAPSHOT"`,
`310 BYTES` the 32-byte hash of the snapshot: every section in the
directory except `Digest`, `Recovery` and `Journal.*`, wherever it is
placed, in directory order, each fed to SHA-256 as

```
varint  name length, name
varint  decoded length, then the decoded data (the records, after joining)
```

Each change set extends the chain: chain(snapshot) = the snapshot hash;
chain(G) = SHA-256(chain(G − 1) ‖ the decoded data of `Journal.G`). Each
change set states the value before it (§10), so a section added or edited
with its CRC fixed up, or a change set replaced, reordered or dropped from
the middle, shows. Dropping the newest change sets leaves a shorter chain
that is still sound; only a signature over the chain's last value can show
that, and that value identifies the file's whole state, so it is what a
signature (header flag bit 1, reserved) will sign. Readers do not check
the chain when opening a file; `cad inspect` does. A writer refuses to
append to a 2.0 file whose `Digest` is missing or damaged; a full save
writes a new one.

### 7.7 Recovery

Parity that rebuilds damaged bytes of the snapshot. It covers the
sections listed before it in the directory, as stored: the bytes from the
end of the file header to the end of the last of those sections (where
directory A begins).

```
varint  scheme (1: XOR stripes)
varint  start of the covered bytes (file offset; 48)
varint  length L of the covered bytes
varint  block size B: a power of two from 64 to 65536
varint  stripe count K
u32     CRC-32 of each block, ceil(L / B) of them (the last block is
        shorter)
u32     CRC-32 of everything above
K × B   parity: block r is the XOR of covered blocks r, r + K, r + 2K, …
        (each padded with zeros to B)
```

Writers choose B as the smallest power of two not below ⌊L / 256⌋, kept
between 64 and 4096, and K = ceil(blocks / 16), so the parity is about a
sixteenth of the snapshot. Any damage that leaves at most one bad block in
each stripe is rebuilt; in particular any run of damage inside the
covered bytes up to (K − 1) × B + 1 bytes long, wherever it starts.

A reader that finds the snapshot damaged finds the bad blocks by their
CRCs, rebuilds those it can, checks each rebuilt block against its CRC,
and reads the rebuilt bytes instead, reporting it. It uses the section
even when the section fails its own directory CRC, as long as its layout
and block CRCs pass their CRC: a damaged parity block only stops the
blocks of its own stripe from being rebuilt. The file on disk is not
changed; a full save writes a sound copy. Change sets appended later are
not covered (an unreadable one falls back to the state before it), nor is
the directory (it has two copies) or the file header (two copies).
`WriteOptions::recovery` turns it off.

## 8. Object records

### 8.1 Common part

Every object record starts with the fields of kind −1 in §8.2: its type
name (2.0 leaves it out when it is the name DWG gives the type number in
2000, for DWG's fixed types), its key (the handle the drawing stores it
under), its DWG type number, then the optional fields. The reactor and
extension-dictionary groups are written as DXF writes them:

| Code | Type | Field |
|---|---|---|
| 102, 330…, 102 | STR, HANDLE…, STR | `"{ACAD_REACTORS"`, one 330 per reactor, `"}"` (when there are reactors) |
| 102, 360, 102 | STR, HANDLE, STR | `"{ACAD_XDICTIONARY"`, extension dictionary, `"}"` (when it has one) |

**Entities** continue with the fields of kind −2. The space tag (2005)
marks the entity part; 1.x wrote `100 "AcDbEntity"` before it, which
readers still accept. The color is one of: nothing (by layer, 2.0); 62
I16 256 by layer (1.x), 0 by block, 1–255 an index; 420 I32 a true color
0x00RRGGBB; 2009 BOOL true, "no color".

**Lineweights** (group 370 here and in LAYER and DIMSTYLE records) are
DWG's codes, not DXF's hundredths of a millimetre: 0–23 index the
standard weights 0.00, 0.05, 0.09, 0.13, 0.15, 0.18, 0.20, 0.25, 0.30,
0.35, 0.40, 0.50, 0.53, 0.60, 0.70, 0.80, 0.90, 1.00, 1.06, 1.20, 1.40,
1.58, 2.00 and 2.11 mm; 29 is by layer, 30 by block, 31 the drawing's
default. Other values are kept as read and drawn as the default.

Then every record has `2001 I16` kind (table below), followed by that
kind's fields. After the kind's fields come any fields a newer revision
added (§6), then the extended data (§8.3), which ends the record.

### 8.2 Kinds

The table is generated from the schema (`cad schema` prints it; a test
fails when this copy differs). Fields are in record order. *[default V]*:
left out when equal to V. *[absent when null]*: a handle or optional
value, left out when null or unset. *[conditional]*: present or not as
the other fields say. A count is followed by its items (*each:*), item by
item; a count with a default of 0 is left out with its empty list. Angles
are **radians**. *Proposed* fields are in the format but their meaning may
still change before the format is published.

| Kind | Name | Fields, in order |
|---|---|---|
| -1 | (every object) | 0 STR type name (left out for DWG's fixed types, whose name follows from group 2000) *[conditional]*; 5 HANDLE key: the handle the object is stored under; 2000 I16 DWG object type number; 2133 HANDLE own handle, when it differs from the key *[absent when null]*; 330 HANDLE owner *[absent when null]*; 102 STR "{ACAD_REACTORS", then 330 per reactor, then "}" *[conditional]*; 102 STR "{ACAD_XDICTIONARY", 360 extension dictionary, "}" *[conditional]*; 2002 I64 offset in the source DWG *[absent when null]*; 2003 I64 size in the source DWG *[absent when null]*; 2004 STR import note *[absent when null]* |
| -2 | (every entity, after the object part) | 2005 I16 space: 0 owned by a block, 1 paper, 2 model; marks the entity part; 2006 HANDLE layer *[absent when null]*; 2007 I16 linetype: 0 by layer, 1 by block, 2 continuous, 3 by handle *[default 0]*; 2008 HANDLE linetype record (when 2007 is 3) *[conditional]*; 62 I16 color index: 0 by block, 1-255; absent with 420 and 2009 means by layer *[conditional]*; 420 I32 true color 0x00RRGGBB *[conditional]*; 2009 BOOL no color *[conditional]*; 440 I32 transparency word as DWG stores it *[absent when null]*; 48 F64 linetype scale *[default 1]*; 370 I16 lineweight code *[default 29]*; 60 I16 1 when invisible *[absent when null]*; 390 HANDLE plot style *[absent when null]*; 347 HANDLE material *[absent when null]* |
| 0 | NOT_DECODED | — |
| 1 | FAILED | 2125 STR why the DWG object did not decode |
| 2 | LINE | 10 P3 start; 11 P3 end; 39 F64 thickness *[default 0]*; 210 P3 extrusion *[default (0, 0, 1)]* |
| 3 | CIRCLE | 10 P3 center (OCS); 40 F64 radius; 39 F64 thickness *[default 0]*; 210 P3 extrusion *[default (0, 0, 1)]* |
| 4 | ARC | 10 P3 center (OCS); 40 F64 radius; 39 F64 thickness *[default 0]*; 210 P3 extrusion *[default (0, 0, 1)]*; 50 F64 start angle (radians) *[default 0]*; 51 F64 end angle (radians) |
| 5 | POINT | 10 P3 position; 39 F64 thickness *[default 0]*; 210 P3 extrusion *[default (0, 0, 1)]*; 50 F64 x-axis angle (radians) *[default 0]* |
| 6 | ELLIPSE | 10 P3 center (WCS); 11 P3 major axis end, from the center; 210 P3 extrusion *[default (0, 0, 1)]*; 40 F64 minor to major axis ratio; 41 F64 start parameter *[default 0]*; 42 F64 end parameter *[default 2π]* |
| 7 | LWPOLYLINE | 2132 I16 flag word as DWG stores it (0x200 closed) *[default 0]*; 43 F64 constant width *[default 0]*; 38 F64 elevation *[default 0]*; 39 F64 thickness *[default 0]*; 210 P3 extrusion *[default (0, 0, 1)]*; 90 I32 vertex count; each: 10 P2 vertex; 2113 I32 bulge count *[default 0]*; each: 42 F64 bulge; 2114 I32 width pair count *[default 0]*; each: 40 F64 start width; each: 41 F64 end width; 2115 I32 vertex id count *[default 0]*; each: 91 I32 vertex id |
| 8 | POLYLINE | 2100 I16 subkind: 0 2D, 1 3D, 2 polyface mesh, 3 polygon mesh; 70 I16 flags *[default 0]*; 75 I16 curve type *[default 0]*; 40 F64 default start width *[default 0]*; 41 F64 default end width *[default 0]*; 39 F64 thickness *[default 0]*; 38 F64 elevation *[default 0]*; 210 P3 extrusion *[default (0, 0, 1)]*; 2111 I32 vertex count *[default 0]*; each: 2112 HANDLE vertex; 2106 HANDLE first vertex *[absent when null]*; 2107 HANDLE last vertex *[absent when null]*; 2108 HANDLE SEQEND *[absent when null]* |
| 9 | VERTEX | 2100 I16 subkind: 0 2D, 1 3D, 2 mesh, 3 polyface vertex, 4 polyface face; 70 I16 flags *[default 0]*; 10 P3 point; 40 F64 start width *[default 0]*; 41 F64 end width *[default 0]*; 42 F64 bulge *[default 0]*; 50 F64 tangent direction *[default 0]*; 91 I32 vertex id *[default 0]*; 71 I16 face vertex 1 *[default 0]*; 72 I16 face vertex 2 *[default 0]*; 73 I16 face vertex 3 *[default 0]*; 74 I16 face vertex 4 *[default 0]* |
| 10 | SEQEND | — |
| 11 | TEXT | 10 P2 insertion point; 11 P2 alignment point; 38 F64 elevation *[default 0]*; 210 P3 extrusion *[default (0, 0, 1)]*; 39 F64 thickness *[default 0]*; 51 F64 oblique angle (radians) *[default 0]*; 50 F64 rotation (radians) *[default 0]*; 40 F64 height; 41 F64 width factor *[default 1]*; 1 STR text; 71 I16 generation flags: 2 backwards, 4 upside down *[default 0]*; 72 I16 horizontal alignment *[default 0]*; 73 I16 vertical alignment *[default 0]*; 2101 HANDLE text style *[absent when null]* |
| 12 | ATTRIB / ATTDEF | 10 P2 insertion point; 11 P2 alignment point; 38 F64 elevation *[default 0]*; 210 P3 extrusion *[default (0, 0, 1)]*; 39 F64 thickness *[default 0]*; 51 F64 oblique angle (radians) *[default 0]*; 50 F64 rotation (radians) *[default 0]*; 40 F64 height; 41 F64 width factor *[default 1]*; 1 STR text; 71 I16 generation flags: 2 backwards, 4 upside down *[default 0]*; 72 I16 horizontal alignment *[default 0]*; 74 I16 vertical alignment *[default 0]*; 2101 HANDLE text style *[absent when null]*; 2 STR tag; 3 STR prompt *[default empty]*; 70 I16 flags *[default 0]*; 73 I16 field length *[default 0]*; 2102 BOOL is an ATTDEF *[default false]*; 280 BOOL lock position (2.5) *[default false]* |
| 13 | MTEXT | 10 P3 insertion point; 210 P3 extrusion *[default (0, 0, 1)]*; 11 P3 x-axis direction *[default (1, 0, 0)]*; 41 F64 reference width *[default 0]*; 46 F64 reference height *[default 0]*; 40 F64 text height; 71 I16 attachment: 1 top left … 9 bottom right *[default 1]*; 72 I16 drawing direction *[default 1]*; 1 STR contents with formatting codes; 44 F64 line-spacing factor *[default 1]*; 2101 HANDLE text style *[absent when null]* |
| 14 | INSERT / MINSERT | 10 P3 insertion point; 41 F64 x scale *[default 1]*; 42 F64 y scale *[default 1]*; 43 F64 z scale *[default 1]*; 50 F64 rotation (radians) *[default 0]*; 210 P3 extrusion *[default (0, 0, 1)]*; 66 BOOL has attributes *[default false]*; 2103 HANDLE block record *[absent when null]*; 2111 I32 attribute count *[default 0]*; each: 2112 HANDLE attribute; 2106 HANDLE first attribute *[absent when null]*; 2107 HANDLE last attribute *[absent when null]*; 2108 HANDLE SEQEND *[absent when null]*; 70 I16 MINSERT columns (70, 71, 44, 45 appear together) *[conditional]*; 71 I16 MINSERT rows *[conditional]*; 44 F64 MINSERT column spacing *[conditional]*; 45 F64 MINSERT row spacing *[conditional]* |
| 15 | SOLID / TRACE | 39 F64 thickness *[default 0]*; 38 F64 elevation *[default 0]*; 10 P2 corner 1; 11 P2 corner 2; 12 P2 corner 3; 13 P2 corner 4; 210 P3 extrusion *[default (0, 0, 1)]*; 2117 BOOL is a TRACE *[default false]* |
| 16 | 3DFACE | 10 P3 corner 1; 11 P3 corner 2; 12 P3 corner 3; 13 P3 corner 4; 70 I16 invisible-edge flags *[default 0]* |
| 17 | SPLINE | 70 I16 flags: 1 closed, 2 periodic, 4 rational *[default 0]*; 71 I32 degree; 2127 I32 knot count *[default 0]*; each: 40 F64 knot; 2128 I32 control point count *[default 0]*; each: 10 P3 control point; 2116 I32 weight count *[default 0]*; each: 41 F64 weight; 2129 I32 fit point count *[default 0]*; each: 11 P3 fit point; 44 F64 fit tolerance *[default 0]*; 12 P3 start tangent *[default (0, 0, 0)]*; 13 P3 end tangent *[default (0, 0, 0)]* |
| 18 | DIMENSION | 2100 I16 subkind: 0 linear, 1 aligned, 2 ordinate, 3 angular 3-point, 4 angular 2-line, 5 radius, 6 diameter; 210 P3 extrusion *[default (0, 0, 1)]*; 11 P2 text midpoint; 38 F64 elevation *[default 0]*; 2132 I16 flags as DWG stores them *[default 0]*; 1 STR user text (<> is the measurement) *[default empty]*; 53 F64 text rotation *[default 0]*; 51 F64 horizontal direction *[default 0]*; 2118 P3 insert scale *[default (1, 1, 1)]*; 2126 F64 insert rotation *[default 0]*; 71 I16 text attachment *[default 0]*; 42 F64 measurement; 2119 P2 clone insertion point *[default (0, 0)]*; 10 P3 definition point; 13 P3 definition point 13 *[default (0, 0, 0)]*; 14 P3 definition point 14 *[default (0, 0, 0)]*; 15 P3 definition point 15 *[default (0, 0, 0)]*; 16 P2 definition point 16 *[default (0, 0)]*; 52 F64 extension-line rotation *[default 0]*; 50 F64 dimension rotation *[default 0]*; 40 F64 leader length *[default 0]*; 2105 HANDLE dimension style *[absent when null]*; 2104 HANDLE graphics block (*D) *[absent when null]* |
| 19 | HATCH | 38 F64 elevation *[default 0]*; 210 P3 extrusion *[default (0, 0, 1)]*; 2 STR pattern name; 70 BOOL solid fill *[default false]*; 71 BOOL associative *[default false]*; 450 BOOL gradient *[default false]*; 75 I16 style *[default 0]*; 76 I16 pattern type *[default 1]*; 52 F64 pattern angle *[default 0]*; 41 F64 pattern scale *[default 1]*; 2130 I32 boundary path count; each: 2100 I16 path kind: 0 edges, 1 polyline; each: 92 I32 path flags *[default 0]*; each: 2131 I32 edge count (edge paths); each: 72 I16 edge type: 1 line, 2 arc, 3 ellipse, 4 spline; each: 10 P2 edge start, center or control point; polyline vertex; each: 11 P2 edge end or major axis; each: 40 F64 radius, axis ratio or knot; each: 50 F64 start angle; each: 51 F64 end angle; each: 73 BOOL counterclockwise (arcs, ellipses); closed (polyline paths); each: 94 I32 spline degree; each: 2127 I32 spline knot count; each: 2128 I32 spline control point count; each: 2116 I32 spline weight count; each: 93 I32 polyline vertex count; each: 42 F64 bulge or spline weight *[default 0]*; 77 BOOL double hatch (2.4) *[default false]*; 78 I32 pattern line count (2.4) *[default 0]*; each: 53 F64 pattern line angle, radians; each: 43 P2 pattern line base point; each: 45 P2 pattern line offset to the next line; each: 79 I32 pattern line dash count; each: 49 F64 dash length: positive dash, negative gap, 0 dot; 98 I32 seed point count (2.4) *[default 0]*; each: 2188 P2 seed point; 2189 I32 boundary object lists, one per path (2.6) *[default 0]*; each: 97 I32 boundary object count; each: 2190 HANDLE boundary object |
| 20 | BLOCK | 2 STR block name |
| 21 | ENDBLK | — |
| 22 | BLOCK_HEADER | 2 STR name; 70 I16 flags: 1 anonymous, 2 has attributes, 4 xref, 8 overlay *[default 0]*; 10 P3 base point *[default (0, 0, 0)]*; 1 STR xref path *[default empty]*; 4 STR description *[default empty]*; 2120 I16 insert units *[default 0]*; 2110 HANDLE BLOCK entity *[absent when null]*; 2111 I32 entity count *[default 0]*; each: 2112 HANDLE entity, in drawing order; 2106 HANDLE first entity *[absent when null]*; 2107 HANDLE last entity *[absent when null]*; 2109 HANDLE ENDBLK *[absent when null]*; 340 HANDLE layout *[absent when null]*; 2140 STR part material (Proposed) *[default empty]*; 2141 STR part grade (Proposed) *[default empty]*; 2142 F64 part thickness in drawing units (Proposed) *[default 0]*; 2143 I32 part quantity (Proposed) *[default 0]* |
| 23 | LAYER | 2 STR name; 2132 I16 flag word as DWG stores it *[default 0]*; 2121 I16 state: 1 frozen, 2 off, 4 locked, 8 plots; 62 I16 color index (or 420, 2009, as for entities) *[conditional]*; 420 I32 true color *[conditional]*; 2009 BOOL no color *[conditional]*; 2122 HANDLE linetype *[absent when null]*; 370 I16 lineweight code *[default 31]* |
| 24 | TABLE_CONTROL | 2111 I32 entry count *[default 0]*; each: 2112 HANDLE entry; 2123 HANDLE model space block record *[absent when null]*; 2124 HANDLE paper space block record *[absent when null]* |
| 25 | NAMED_RECORD | 2 STR name |
| 26 | LTYPE | 2 STR name; 70 I16 flags *[default 0]*; 3 STR description *[default empty]*; 40 F64 total pattern length *[default 0]*; 73 I32 element count *[default 0]*; each: 49 F64 length: positive dash, negative gap, 0 dot; each: 74 I16 complex flags: 1 absolute rotation, 2 text, 4 shape *[default 0]*; each: 75 I16 shape number *[default 0]*; each: 340 HANDLE text style or shape file *[absent when null]*; each: 46 F64 scale *[default 1]*; each: 50 F64 rotation (radians) *[default 0]*; each: 44 P2 offset *[default (0, 0)]*; each: 9 STR text *[default empty]* |
| 27 | STYLE | 2 STR name; 70 I16 flags: 1 shape file, 4 vertical *[default 0]*; 40 F64 fixed height (0 none) *[default 0]*; 41 F64 width factor *[default 1]*; 50 F64 oblique angle (radians) *[default 0]*; 71 I16 generation flags *[default 0]*; 42 F64 last height used *[default 0]*; 3 STR font file *[default empty]*; 4 STR big-font file *[default empty]* |
| 28 | DIMSTYLE | 2 STR name; 70 I16 flags *[default 0]*; 3 STR DIMPOST *[default empty]*; 40 F64 DIMSCALE *[default 1]*; 41 F64 DIMASZ *[default 0]*; 42 F64 DIMEXO *[default 0]*; 43 F64 DIMDLI *[default 0]*; 44 F64 DIMEXE *[default 0]*; 45 F64 DIMRND *[default 0]*; 46 F64 DIMDLE *[default 0]*; 140 F64 DIMTXT *[default 0]*; 141 F64 DIMCEN *[default 0]*; 142 F64 DIMTSZ *[default 0]*; 143 F64 DIMALTF *[default 25.4]*; 144 F64 DIMLFAC *[default 1]*; 147 F64 DIMGAP *[default 0]*; 73 BOOL DIMTIH *[default true]*; 74 BOOL DIMTOH *[default true]*; 77 I16 DIMTAD *[default 0]*; 78 I16 DIMZIN *[default 0]*; 170 BOOL DIMALT *[default false]*; 171 I16 DIMALTD *[default 2]*; 2134 I32 DIMCLRD as a color method word *[default 0xC1000000]*; 2135 I32 DIMCLRE as a color method word *[default 0xC1000000]*; 2136 I32 DIMCLRT as a color method word *[default 0xC1000000]*; 271 I16 DIMDEC *[default 2]*; 276 I16 DIMFRAC *[default 0]*; 277 I16 DIMLUNIT *[default 2]*; 278 I32 DIMDSEP (a Unicode scalar value) *[default 46]*; 280 I16 DIMJUST *[default 0]*; 340 HANDLE DIMTXSTY *[absent when null]*; 371 I16 DIMLWD *[default 30]*; 372 I16 DIMLWE *[default 30]* |
| 29 | VPORT | 2 STR name; 40 F64 view height; 41 F64 aspect ratio (width over height); 12 P2 view centre; 17 P3 target; 16 P3 view direction; 51 F64 twist angle, radians; 42 F64 lens length; 43 F64 front clip; 44 F64 back clip; 71 I16 view mode (DXF 71); 281 I16 render mode; 292 BOOL default lighting; 282 I16 lighting type; 141 F64 brightness; 142 F64 contrast; 2144 I32 ambient colour as a color method word *[default 0xC30000FA]*; 10 P2 lower-left corner; 11 P2 upper-right corner; 72 I16 circle zoom percent; 73 BOOL fast zoom; 74 I16 UCS icon setting; 76 BOOL grid on; 15 P2 grid spacing; 75 BOOL snap on; 77 BOOL snap style (isometric); 78 I16 isometric snap plane; 50 F64 snap rotation, radians; 13 P2 snap base; 14 P2 snap spacing; 2145 BOOL the DWG record's unnamed bit; 65 BOOL UCS per viewport; 110 P3 UCS origin; 111 P3 UCS X axis; 112 P3 UCS Y axis; 146 F64 UCS elevation; 79 I16 UCS orthographic type; 60 I16 grid flags; 61 I16 major grid lines; 332 HANDLE background *[absent when null]*; 348 HANDLE visual style *[absent when null]*; 361 HANDLE sun *[absent when null]*; 345 HANDLE named UCS *[absent when null]*; 346 HANDLE base UCS *[absent when null]* |
| 30 | DIMSTYLE_RECORD | 2 STR name; 3 STR DIMPOST; 4 STR DIMAPOST; 40 F64 DIMSCALE; 41 F64 DIMASZ; 42 F64 DIMEXO; 43 F64 DIMDLI; 44 F64 DIMEXE; 45 F64 DIMRND; 46 F64 DIMDLE; 47 F64 DIMTP; 48 F64 DIMTM; 49 F64 DIMFXL; 50 F64 DIMJOGANG, radians; 69 I16 DIMTFILL; 2144 I32 DIMTFILLCLR as a color method word *[default 0xC1000000]*; 71 BOOL DIMTOL; 72 BOOL DIMLIM; 73 BOOL DIMTIH; 74 BOOL DIMTOH; 75 BOOL DIMSE1; 76 BOOL DIMSE2; 77 I16 DIMTAD; 78 I16 DIMZIN; 79 I16 DIMAZIN; 90 I16 DIMARCSYM; 140 F64 DIMTXT; 141 F64 DIMCEN; 142 F64 DIMTSZ; 143 F64 DIMALTF; 144 F64 DIMLFAC; 145 F64 DIMTVP; 146 F64 DIMTFAC; 147 F64 DIMGAP; 148 F64 DIMALTRND; 170 BOOL DIMALT; 171 I16 DIMALTD; 172 BOOL DIMTOFL; 173 BOOL DIMSAH; 174 BOOL DIMTIX; 175 BOOL DIMSOXD; 2134 I32 DIMCLRD as a color method word *[default 0xC1000000]*; 2135 I32 DIMCLRE as a color method word *[default 0xC1000000]*; 2136 I32 DIMCLRT as a color method word *[default 0xC1000000]*; 179 I16 DIMADEC; 271 I16 DIMDEC; 272 I16 DIMTDEC; 273 I16 DIMALTU; 274 I16 DIMALTTD; 275 I16 DIMAUNIT; 276 I16 DIMFRAC; 277 I16 DIMLUNIT; 278 I16 DIMDSEP; 279 I16 DIMTMOVE; 280 I16 DIMJUST; 281 BOOL DIMSD1; 282 BOOL DIMSD2; 283 I16 DIMTOLJ; 284 I16 DIMTZIN; 285 I16 DIMALTZ; 286 I16 DIMALTTZ; 288 BOOL DIMUPT; 289 I16 DIMATFIT; 290 BOOL DIMFXLON; 294 BOOL DIMTXTDIRECTION; 2146 F64 DIMALTMZF; 2147 STR DIMALTMZS; 2148 F64 DIMMZF; 2149 STR DIMMZS; 371 I16 DIMLWD (DXF lineweight); 372 I16 DIMLWE (DXF lineweight); 2150 BOOL the DWG record's trailing bit; 340 HANDLE DIMTXSTY *[absent when null]*; 341 HANDLE DIMLDRBLK *[absent when null]*; 342 HANDLE DIMBLK *[absent when null]*; 343 HANDLE DIMBLK1 *[absent when null]*; 344 HANDLE DIMBLK2 *[absent when null]*; 345 HANDLE DIMLTYPE *[absent when null]*; 346 HANDLE DIMLTEX1 *[absent when null]*; 347 HANDLE DIMLTEX2 *[absent when null]* |
| 31 | DICTIONARY | 281 I16 duplicate record cloning; 280 I16 hard owner *[default 0]*; 2111 I32 entry count *[default 0]*; each: 3 STR entry key; each: 350 HANDLE entry object; 2162 BOOL has a default entry (ACDBDICTIONARYWDFLT) *[default false]*; 340 HANDLE default entry, after 2162 *[conditional]* |
| 32 | XRECORD | 280 I16 duplicate record cloning; 2111 I32 item count *[default 0]*; each: 2151 I16 the item's DXF group code; each: 2152 STR string value *[conditional]*; each: 2153 P3 point value *[conditional]*; each: 2154 F64 real value *[conditional]*; each: 2155 I16 8-bit integer value *[conditional]*; each: 2156 I16 16-bit integer value *[conditional]*; each: 2157 I32 32-bit integer value *[conditional]*; each: 2158 I64 64-bit integer value *[conditional]*; each: 2159 BOOL boolean value *[conditional]*; each: 2160 BYTES binary value *[conditional]*; each: 2161 HANDLE handle value *[conditional]* |
| 33 | ACDBPLACEHOLDER | — |
| 34 | DICTIONARYVAR | 280 I16 schema number *[default 0]*; 1 STR value |
| 35 | SCALE | 70 I16 flag *[default 0]*; 300 STR name; 140 F64 paper units; 141 F64 drawing units; 290 BOOL unit scale *[default false]* |
| 36 | LAYOUT | 1 STR page setup name; 2 STR printer or plotter; 4 STR paper size; 70 I16 plot layout flags; 40 F64 left margin, mm; 41 F64 bottom margin, mm; 42 F64 right margin, mm; 43 F64 top margin, mm; 44 F64 paper width, mm; 45 F64 paper height, mm; 46 P2 plot origin; 72 I16 paper units; 73 I16 plot rotation; 74 I16 plot type; 48 P2 plot window lower left; 140 P2 plot window upper right; 142 F64 custom scale: real-world units; 143 F64 custom scale: drawing units; 7 STR plot style sheet; 75 I16 standard scale type; 147 F64 scale factor; 148 P2 paper image origin; 76 I16 shade plot mode; 77 I16 shade plot resolution level; 78 I16 shade plot custom DPI; 2163 HANDLE plot view *[absent when null]*; 2164 HANDLE plot visual style *[absent when null]*; 2165 STR layout name; 2166 I16 layout flags; 2167 I32 tab order; 2168 P2 limits min; 2169 P2 limits max; 2170 P3 insertion base; 2171 P3 extents min; 2172 P3 extents max; 2173 F64 elevation; 2174 P3 UCS origin; 2175 P3 UCS X axis; 2176 P3 UCS Y axis; 2177 I16 UCS orthographic type; 2178 HANDLE paper-space block record *[absent when null]*; 2179 HANDLE last active viewport *[absent when null]*; 2180 HANDLE base UCS *[absent when null]*; 2181 HANDLE named UCS *[absent when null]*; 2111 I32 viewport count *[default 0]*; each: 2112 HANDLE viewport |
| 37 | MLINESTYLE | 2 STR name; 3 STR description; 70 I16 flags (DXF); 2144 I32 fill colour as a color method word *[default 0xC1000000]*; 51 F64 start angle, radians; 52 F64 end angle, radians; 2111 I32 element count *[default 0]*; each: 49 F64 element offset; each: 2144 I32 element colour as a color method word *[default 0xC1000000]*; each: 2112 HANDLE element linetype; each: 2182 I16 element linetype index (before R2018) |
| 38 | GROUPS | 2183 STR DXF object type; 2111 I32 item count *[default 0]*; each: 2151 I16 the item's DXF group code; each: 2152 STR string value *[conditional]*; each: 2153 P3 point value *[conditional]*; each: 2154 F64 real value *[conditional]*; each: 2155 I16 8-bit integer value *[conditional]*; each: 2156 I16 16-bit integer value *[conditional]*; each: 2157 I32 32-bit integer value *[conditional]*; each: 2158 I64 64-bit integer value *[conditional]*; each: 2159 BOOL boolean value *[conditional]*; each: 2160 BYTES binary value *[conditional]*; each: 2161 HANDLE handle value *[conditional]* |
| 39 | XLINE / RAY | 10 P3 base point (WCS); 11 P3 direction, a unit vector; 2184 BOOL is a RAY: infinite from the base point on *[default false]* |
| 40 | LEADER | 71 BOOL arrowhead *[default true]*; 72 BOOL spline path *[default false]*; 73 I16 annotation type: 0 MTEXT, 1 tolerance, 2 block reference, 3 none *[default 3]*; 74 BOOL hook line along the horizontal direction *[default true]*; 75 BOOL hook line *[default false]*; 40 F64 annotation height *[default 0]*; 41 F64 annotation width *[default 0]*; 76 I32 vertex count *[default 0]*; each: 10 P3 vertex (WCS), the arrow at the first; 2185 P3 leader plane origin (DWG) *[default (0, 0, 0)]*; 210 P3 extrusion *[default (0, 0, 1)]*; 211 P3 horizontal direction *[default (1, 0, 0)]*; 212 P3 offset of the last vertex from a block annotation *[default (0, 0, 0)]*; 213 P3 offset of the last vertex from the annotation *[default (0, 0, 0)]*; 2186 I16 DWG bits of unknown meaning: 1 the first, 2 and 4 the last two *[default 0]*; 2187 I16 DWG short of unknown meaning *[default 0]*; 340 HANDLE annotation *[absent when null]*; 2105 HANDLE dimension style *[absent when null]* |
| 41 | VIEWPORT | 10 P3 centre on the paper; 40 F64 width on the paper; 41 F64 height on the paper; 69 I16 viewport id: 1 the layout's own view; 17 P3 view target *[default (0, 0, 0)]*; 16 P3 view direction *[default (0, 0, 1)]*; 51 F64 twist angle, radians *[default 0]*; 45 F64 view height in model units; 42 F64 lens length *[default 50]*; 43 F64 front clip *[default 0]*; 44 F64 back clip *[default 0]*; 50 F64 snap angle, radians *[default 0]*; 12 P2 view centre (model); 13 P2 snap base *[default (0, 0)]*; 14 P2 snap spacing; 15 P2 grid spacing; 72 I16 circle zoom percent *[default 1000]*; 61 I16 major grid lines *[default 5]*; 90 I32 status flags (DXF 90; 0x20000 off); 1 STR plot style sheet *[default empty]*; 281 I16 render mode *[default 0]*; 74 BOOL UCS icon at origin *[default true]*; 71 BOOL UCS per viewport *[default true]*; 110 P3 UCS origin *[default (0, 0, 0)]*; 111 P3 UCS X axis *[default (1, 0, 0)]*; 112 P3 UCS Y axis *[default (0, 1, 0)]*; 146 F64 UCS elevation *[default 0]*; 79 I16 UCS orthographic type *[default 0]*; 170 I16 shade plot mode *[default 0]*; 292 BOOL default lighting *[default true]*; 282 I16 lighting type *[default 1]*; 141 F64 brightness *[default 0]*; 142 F64 contrast *[default 0]*; 2144 I32 ambient colour as a color method word *[default 0xC30000FA]*; 2111 I32 frozen layer count *[default 0]*; each: 331 HANDLE layer frozen in this viewport; 340 HANDLE clip boundary *[absent when null]*; 345 HANDLE named UCS *[absent when null]*; 346 HANDLE base UCS *[absent when null]*; 332 HANDLE background *[absent when null]*; 348 HANDLE visual style *[absent when null]*; 333 HANDLE shade plot *[absent when null]*; 361 HANDLE sun *[absent when null]* |
| 42 | TOLERANCE | 1 STR frame contents, in the DXF reference's GD&T codes *[default empty]*; 10 P3 insertion point (WCS): the middle of the first line's left edge *[default (0, 0, 0)]*; 11 P3 X direction (WCS) *[default (1, 0, 0)]*; 210 P3 extrusion *[default (0, 0, 1)]*; 2105 HANDLE dimension style *[absent when null]*; 2191 I16 R13/R14 DWG only: undocumented short (2191 to 2193 appear together) *[conditional]*; 2192 F64 R13/R14 DWG only: text height *[conditional]*; 2193 F64 R13/R14 DWG only: gap *[conditional]* |
| 43 | IMAGE | 90 I32 class version *[default 0]*; 10 P3 insertion point (WCS): the image's lower-left corner *[default (0, 0, 0)]*; 11 P3 one pixel's width vector (U) *[default (1, 0, 0)]*; 12 P3 one pixel's height vector (V) *[default (0, 1, 0)]*; 13 P2 size in pixels *[default (1, 1)]*; 70 I16 display: 1 show, 2 show unaligned, 4 use the clip boundary, 8 transparency *[default 7]*; 280 BOOL clipping on *[default false]*; 281 I16 brightness, 0-100 *[default 50]*; 282 I16 contrast, 0-100 *[default 50]*; 283 I16 fade, 0-100 *[default 0]*; 290 BOOL the inside of the clip boundary is clipped away *[default false]*; 71 I16 clip boundary: 1 rectangle (two corners), 2 polygon *[default 1]*; 91 I32 clip boundary vertex count *[default 0]*; each: 14 P2 clip boundary vertex, in pixels; 340 HANDLE image definition (IMAGEDEF) *[absent when null]*; 360 HANDLE image definition reactor, owned *[absent when null]* |
| 44 | WIPEOUT | 90 I32 class version *[default 0]*; 10 P3 insertion point (WCS): the image's lower-left corner *[default (0, 0, 0)]*; 11 P3 one pixel's width vector (U) *[default (1, 0, 0)]*; 12 P3 one pixel's height vector (V) *[default (0, 1, 0)]*; 13 P2 size in pixels *[default (1, 1)]*; 70 I16 display: 1 show, 2 show unaligned, 4 use the clip boundary, 8 transparency *[default 7]*; 280 BOOL clipping on *[default false]*; 281 I16 brightness, 0-100 *[default 50]*; 282 I16 contrast, 0-100 *[default 50]*; 283 I16 fade, 0-100 *[default 0]*; 290 BOOL the inside of the clip boundary is clipped away *[default false]*; 71 I16 clip boundary: 1 rectangle (two corners), 2 polygon *[default 1]*; 91 I32 clip boundary vertex count *[default 0]*; each: 14 P2 clip boundary vertex, in pixels; 340 HANDLE image definition (none for a wipeout) *[absent when null]*; 360 HANDLE image definition reactor (none for a wipeout) *[absent when null]* |
| 45 | MLINE | 340 HANDLE multiline style (MLINESTYLE) *[absent when null]*; 40 F64 scale *[default 1]*; 70 I16 justification: 0 top, 1 zero, 2 bottom *[default 0]*; 10 P3 start point (WCS) *[default (0, 0, 0)]*; 210 P3 extrusion *[default (0, 0, 1)]*; 71 I16 flags: 1 has vertices, 2 closed, 4 no start caps, 8 no end caps *[default 1]*; 73 I16 elements in the style *[default 0]*; 72 I32 vertex count *[default 0]*; each: 11 P3 vertex (WCS); each: 12 P3 direction of the segment from the vertex; each: 13 P3 miter direction; each: 74 I32 per element (73 times): segment parameter count; each: 41 F64 segment parameter; each: 75 I32 per element: area fill parameter count; each: 42 F64 area fill parameter |

Notes on the table:

- **Kind 0** keeps the object's DWG bytes in `DwgPreserved`; **kind 1**
  too, with the reason it failed. A native record whose kind fields do not
  decode is not rewritten as kind 1; see below.
- **TEXT and ATTRIB** share their first fields (10 … 72); their vertical
  alignment is 73 in TEXT and 74 in ATTRIB, as in DXF.
- **MTEXT** contents are one string, not split into 250-character chunks.
- **INSERT**: the MINSERT fields 70, 71, 44 and 45 appear together, only
  for a MINSERT.
- **HATCH**: after the path count, each path is 2100 path kind, then for
  an edge path (0) 92 flags, 2131 edge count and per edge 72 type and its
  fields (1 line: 10, 11; 2 arc: 10 center, 40 radius, 50, 51, 73 ccw;
  3 ellipse: 10 center, 11 major axis, 40 ratio, 50, 51, 73 ccw; 4 spline:
  94 degree, then counted lists 2127/40 knots, 2128/10 control points,
  2116/42 weights), or for a polyline path (1) 92 flags, 73 closed, 93
  vertex count and per vertex 10 and 42 bulge.
- **LTYPE**: each element is 49 length (positive dash, negative gap, 0
  dot) and its optional fields.
- **DIMSTYLE**: 2134–2136 are color method words: high byte 0xC0 by layer,
  0xC1 by block, 0xC2 true color 0xRRGGBB, 0xC3 index in the low byte,
  0xC8 none.
- **BLOCK_HEADER part data** (*Proposed*): what a block is as a
  manufactured part (material, grade, thickness, quantity); the model
  space record holds it for a drawing of one part. It describes the part,
  not how a machine cuts it (DESIGN §1). `cad part` sets it and
  `cad takeoff` uses it to compute the length, contours, net area and
  weight a quote needs.

A reader that meets a kind number it does not know keeps the object as a
**newer kind**. It reads the common part as usual, keeps everything from
the kind's fields up to the extended data as it is, and writes it back
unchanged. If a known kind's fields do not decode, or the kind tag is
missing (damage), the object is marked damaged in memory. Everything from
where its 2001 tag is, or should be, is kept the same way, and a save
writes it back exactly as it was, so the next reader reports the same
problem.

### 8.3 Extended data (XDATA)

Values applications attach to an object, in blocks that each start with
the application's name. They use DXF's extended-data group codes, with the
meanings the DXF reference gives them. DWG stores the same data (ODA spec
chapter 28, "EED"). Extended data ends the object record.

A block is a `1001` tag naming the application, then any number of items:

| Code | Type | Meaning |
|---|---|---|
| 1001 | STR | Registered application name, as in its APPID table record. Starts a block |
| 1001 | HANDLE | The application's APPID record, used only when its name is unknown (the DWG source referred to a record that is missing or unreadable) |
| 1000 | STR | String |
| 1002 | STR | `"{"` or `"}"`: opens or closes a nested list |
| 1003 | HANDLE | Layer, by handle (DXF writes the layer's name) |
| 1004 | BYTES | Binary data |
| 1005 | HANDLE | Handle of another object |
| 1010 | P3 | Three reals. Editing never changes them |
| 1011 | P3 | World position: moved, scaled, rotated, mirrored and stretched with the object |
| 1012 | P3 | World displacement: scaled, rotated and mirrored with the object, not moved or stretched |
| 1013 | P3 | World direction: rotated and mirrored with the object, not moved, scaled or stretched |
| 1040 | F64 | Real |
| 1041 | F64 | Distance: scaled with the object |
| 1042 | F64 | Scale factor: scaled with the object |
| 1070 | I16 | 16-bit integer |
| 1071 | I32 | 32-bit integer |
| 2600, 2601 | STR, BYTES | DWG extended data that did not decode on import: the DWG version tag of the source file, then the bytes from the item that failed to the end of the application's data, as stored. The importer puts it last in its block |

- **Order** of blocks and items is kept. AutoCAD keeps one block per
  application; this format keeps whatever the source had.
- **No limits are enforced.** AutoCAD allows 16 KB of extended data per
  object (AutoLISP reference, `xdroom`), strings of 255 bytes, application
  names of 31 bytes and binary chunks of 127 bytes (DXF reference). Writers
  of DXF and DWG apply them.
- **Braces** should balance; AutoCAD rejects data where they do not. A
  reader accepts any order, and the audit reports unbalanced blocks.
- **References.** 1003 and 1005 are handles like any other reference: they
  follow renames, and are translated when objects are copied into another
  drawing.
- **Damage.** If the extended data does not decode (a code outside this
  table, a value of the wrong type, a 1002 that is not a brace), the reader
  keeps the whole of it as it is, with the object's unknown fields, reports
  it, and writes it back unchanged.
- **Closed set.** Extended data uses exactly the codes and types above.
  Adding an item kind needs a new major version; everything else a later
  revision adds goes in fields before the extended data.

**From DWG.** Each block is a byte length (BS), the APPID record's handle,
and the items, each a one-byte code (the DXF group minus 1000) and its
value. The importer names each application from its APPID record.

- **Strings** become UTF-8. R2007+ strings are UTF-16 and are converted
  exactly. R13–R2004 strings are 8-bit with a code page: in ANSI_1252
  (code page 30, or 1252) they are converted one character per byte,
  exactly, with `\U+XXXX` sequences left as they are; in any other code
  page the rest of the block is kept undecoded (2600/2601). Strings are
  kept whole, including any NUL characters (the ODA spec describes no
  terminator).
- **Handles of items 3 and 5** are eight bytes whose order the ODA spec
  leaves open: it says to read them "as usual for handles" (most
  significant byte first, §2.13), but also that all data is little-endian
  unless stated otherwise (§2). The importer reads them most significant
  first, checks which order makes the drawing's 1003 and 1005 values point
  at existing layers and objects, and keeps that order for the whole
  drawing, reporting which one it used.

## 9. ObjectMap

```
varint  entry count
repeat, in ascending key order:
  varint  key − previous key (first: key − 0)
  varint  page number (the N in Objects.N)
  varint  byte offset of the record's length prefix within that page
```

This is the DWG object map idea (handle → location, delta-coded) with a
page number added, so a viewer can decompress one page to show one object
(§12).

## 10. Journal and incremental save

An incremental save appends a change set instead of rewriting the file.

**Section `Journal.G`**, where G is the generation it created. Its data is
a sequence of records:

1. A properties record in the header style: `$CAD_AUTHOR` (1 STR),
   `$CAD_TIME` (160 I64, Unix seconds), `$CAD_GENERATION` (160 I64), and
   in a 2.0 file `$CAD_PREVIOUS` (310 BYTES, the hash chain's value before
   this change set, §7.6).
2. Operation records, each starting with `2500 I16` operation:
   - 1 **put**: the rest of the record is an object record (§8), keyed by
     the object's own handle; it adds the object or replaces the one with
     that key.
   - 2 **delete**: `5 HANDLE`.
   - 3 **header**: the rest is a header tag stream (§7.2) replacing all
     header variables.
   - 4 **preserved bytes**: the rest is a `DwgPreserved` record (§7.4) for
     the object put just before it.
   Unknown operations are skipped, so a full save drops them (§6). Object
   records inside put operations keep their unknown fields and extended
   data like any other.

**Reading.** Load the snapshot, then apply every `Journal.*` section in
ascending G. If one is unreadable, stop there and report it; later ones
depend on it. A writer must refuse to append after an unreadable change
set (the new one would be hidden too); a full save clears the damage.

**Writing, in this order:**

1. Append the journal section at the end of the file. Flush to disk.
2. Append the new directory (the old entries plus the journal) twice, then
   a new trailer with generation + 1 pointing at the second copy. Flush.
3. Overwrite the front header with the same values, pointing at the first
   copy. Flush.

A crash before step 2 completes leaves the old headers and directories
intact, so the old state loads. A crash between 2 and 3 leaves a valid
trailer with a higher generation, so the new state loads. A torn write in
step 3 fails the front header's CRC, and the trailer is used. If the disk
reorders writes so the trailer lands without the journal, the journal
check in §3 rejects that trailer and the old state loads.

**Into a 1.x file**, a change set is written as 1.x writes it: records in
the 1.x form (the type name always, `100 "AcDbEntity"` before the entity
part, every field written, color 62 = 256 for by layer), codec 1 or 0, no
entry flags, no `$CAD_PREVIOUS`, one directory, and the file keeps its
version, so 1.x readers can still read it. Fields new in 2.0 are written
only when set, and a 1.x reader keeps them as fields from a newer writer.
A writer refuses to append to a 1.0 file, whose lineweight 0 means
something else (see Revisions); a full save first makes it 2.0.

The generation must not wrap: a writer refuses to append at
generation 2^64 − 1.

**Compaction.** A full save writes a fresh snapshot with no journal
sections and generation + 1 (`cad compact`).

Journal sections are also the unit a live-collaboration layer can send
between copies of the app.

## 11. Lock file (`.bcl`)

The equivalent of AutoCAD's `.dwl`. While a drawing is open for editing,
a UTF-8 text file sits beside it: `part.blc` → `part.bcl`, and any other
file gets the extension appended (`part.dwg` → `part.dwg.bcl`). CADCraft's
lock is the same file named `.cdl`, beside its `.cad` drawings (`cdl 1`
on the first line); the `lock` module writes either.

```
bcl 1
user=<name>
host=<machine>
pid=<process id>
app=<writer, e.g. cad-core 0.3.0>
session=<16 hex digits, random per open>
opened=<Unix seconds>
heartbeat=<Unix seconds>
```

The first line must be `bcl 1` (or CADCraft's `cdl 1`, read the same
way). Unknown keys are ignored.

- **Acquire**: create the file exclusively (fails if it exists). If it
  exists and its heartbeat is less than 120 s old, the drawing is held:
  open read-only and show who holds it. If older, the lock was abandoned:
  rename it aside, confirm the renamed file is the one judged stale and is
  still stale, and create a new one. If it is not, put it back without
  replacing anything created meanwhile (hard link, which fails if the name
  exists).
- **Heartbeat**: the holder rewrites the file (write to `.tmp`, rename)
  every 30 s with a new `heartbeat`, after checking its `session` is still
  the one in the file. If not, the lock was taken over; stop saving.
- **Release**: delete the file if its session is still ours.
- A file that cannot be parsed, or has no `heartbeat` line (cut short
  while being written), is dated by its modification time, so a lock
  being written at that moment is not taken over.

Like `.dwl`, the lock is advisory. Limits of a lock made of a plain file:
refresh and release check, then act, so a takeover in that instant can be
overwritten (the next refresh of either holder detects it); staleness
compares wall clocks across machines; cloud-sync folders may delay or
reorder file operations.

## 12. Reading part of a file

`native::NativeFile` opens a file once (header copies and directory) and
answers questions without decoding the whole drawing: one object (the
object map is read on first use and kept; then only that object's page
and the change sets are decoded), the header variables and drawing
properties, the preview, the schema, the hash chain and a summary of the
sections. Objects, header variables, properties, the preview and the
schema fall back to the recovery data, as a full read does, when the bytes
they need are damaged. When those bytes cannot be rebuilt, a lookup
returns what the section holds, as a full read does, but without a
diagnostic to say so; `info()` reports which sections fail their CRC.

## 13. Differences from DXF

| DXF | This format | Why |
|---|---|---|
| Value type implied by group code range | Explicit type byte | Readers can skip unknown tags; new types are possible |
| Points as three tags (10, 20, 30) | One P3 or P2 tag | Smaller, one read |
| Handles as hex strings | Binary varint | Smaller, no parsing |
| Every group written, defaults included | Fields equal to their default left out (2.0) | Half the size of the records before compression |
| Angles in degrees | Radians | No conversion, so values round-trip exactly |
| Layer, linetype, style, block by name | By handle (codes 2006, 2008, 2101, 2103) | Matches the DWG object graph; renames touch one record |
| Group 70 flag words of LWPOLYLINE, LAYER, DIMENSION | Kept as DWG stores them, under 2132 | Their DWG bits differ from DXF's; storing them as read loses nothing |
| Strings split in 250-character chunks | One string of any length | No limit to work around |
| Code page per file | UTF-8 always | One encoding everywhere |
| XDATA layer (1003) by name, handle (1005) as hex text | Both as binary handles | 1003 follows layer renames, as in DWG |
| XDATA limits: 16 KB per object, 255-byte strings, 127-byte binary chunks | None | Applied when writing DXF or DWG |
| Unknown group codes dropped by most readers | Kept and written back (§6) | Older software does not strip newer data |
| Group code meanings known only from the reference | The file carries its schema (§7.5) | Readers can name fields they do not know |

## 14. Not in this version

- Fields of decoded objects that the model does not keep yet are not
  stored. Objects the core does not decode keep their full DWG bytes;
  decoded ones keep their modelled fields and extended data.
- A full save drops sections the writer does not know (§6).
- No encryption or signatures yet: the header flags and the entry flag
  are reserved for them (§3, §4.1), and the hash chain (§7.6) is what a
  signature will sign.
- No per-object timestamps or authorship; change sets carry author and time.
- Journals are not paged; a very large change set is one section.
- Each incremental save rewrites the whole section directory (compressed,
  and twice).
- Recovery data covers the snapshot, not change sets appended after it.

## 15. Validity rules

A file is well formed when all of these hold. `cad inspect` (the
`native::audit` module) checks every one and reports each as PASS or FAIL.

1. Both header copies have the magic and a correct CRC, and describe the
   same state; in 2.0 each points at its own copy of the directory. After
   an interrupted save they may differ; the reader still opens the file,
   per §3.
2. The chosen copy's directory is in bounds and matches its CRC. In 2.0
   the other copy does too, and lists the same sections.
3. Every section lies after the file header and before the last copy of
   its directory, and no two sections or directory copies overlap.
4. Every section decodes to exactly its decoded length and matches its
   CRC-32. No section name appears twice.
5. `Meta`, `Header`, `Classes`, `ObjectMap` and `Objects.0` exist, and in
   2.0 `Schema` and `Digest`; object pages are numbered 0, 1, 2, …
   without gaps.
6. No section uses a codec or a critical flag this version does not know,
   or is encrypted; a 1.x file uses neither DEFLATE nor entry flags.
7. `Recovery`, if present, covers exactly the bytes from the end of the
   file header to the end of the sections listed before it, and every
   block matches its CRC.
8. `Meta` names a writer and a known source version; every tag of
   `Header` parses, every `Classes` record parses, and so does every
   `Schema` record (the audit also says whether the schema matches this
   version's).
9. Every object record parses, including its extended data (§8.3); keys
   ascend strictly across pages (no duplicates), and their number equals
   `Meta`'s `$CAD_OBJECTS`. Objects of newer kinds and fields from newer
   writers are allowed and counted.
10. The object map has one entry per record, in ascending key order, each
    pointing at the start of that record, with no trailing bytes.
11. Every `DwgPreserved` record parses and belongs to an object in the
    file.
12. `Journal.G` sections parse, G ascends, the newest G equals the
    header's generation, and the journal flag is set exactly when change
    sets exist.
13. In 2.0, the snapshot (every section but `Digest`, `Recovery` and
    change sets) hashes to the value in `Digest`, and every change set's
    `$CAD_PREVIOUS` is the chain's value before it (§7.6).
14. `Preview`, if present, is an SVG document.

`cad inspect` also shows the byte map of the whole file (every range
accounted for, including bytes superseded by incremental saves). It lists
the extended data by application, with any blocks whose braces do not
balance, and counts references to handles that are not in the file (both
informational). With `--tags N` it prints the tags of the first N records
of each section, naming each field from the file's schema.

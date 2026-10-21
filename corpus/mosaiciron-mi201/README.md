# MI-201: bent clip plate (MosaicIron)

The owner's own part (MosaicIron is the owner's brand), supplied for
testing on 2026-10-02. One drawing, saved by AutoCAD 2027 in every DWG
version it offers, with a DXF of each as the reference.

| File | Format | Notes |
|---|---|---|
| `mi201-r14.dwg` | AC1014 (R14 / LT97 / LT98) | Checked against `mi201-r12.dxf` |
| `mi201-r2000.dwg`, `.dxf` | AC1015 | |
| `mi201-r2004.dwg`, `.dxf` | AC1018 | |
| `mi201-r2007.dwg`, `.dxf` | AC1021 | The reader cannot open AC1021 yet |
| `mi201-r2010.dwg`, `.dxf` | AC1024 | |
| `mi201-r2013.dwg`, `.dxf` | AC1027 | |
| `mi201-r2018.dwg`, `.dxf` | AC1032 | |
| `mi201-r12.dxf` | AC1009 (R12 / LT2) | DXF only |
| `mi201-r2018-binary.dxf` | AC1032, binary DXF | AutoCAD 2027's binary save of an ASCII DXF of this drawing that this project wrote; the reference for the binary DXF encoder |
| `mi201-r2018-from-ascii-dxf.dwg`, `mi201-r2018-from-binary-dxf.dwg` | AC1032 | AutoCAD 2027's DWG saves of the ASCII and binary DXFs this project wrote from the `.cad` (2026-10-08); checked against `mi201-r2018.dxf`, and references for the DWG container and compressor |

What the drawing holds (34 model-space entities):

- The flat plate, 6 × 7 in, 3/8 A36: R1/2 bottom corners, 1/2 × 45°
  chamfers at the top (drawn on layer `DIMS`), a 1/2 in half-round notch in
  each side, four 9/16 holes for 1/2 A325 bolts at gage 3.
- A side view of the 90° bend (R 3/8 inside), on layer `0` like the plate.
- Two linear dimensions (6.0000, 7.0000) with their `*D` blocks, and eight
  lines of text, one with a degree sign.
- No drawing units (`$INSUNITS` 0), so measurements are in drawing units
  (inches by intent).

Notes for the tests:

- The R12, 2000 and 2004 DXFs reached the project with the degree sign
  replaced by U+FFFD (bytes EF BF BD); the DWGs and the 2007+ DXFs have it.
  `tests/corpus.rs` treats U+FFFD in a reference as matching any character.
- Takeoff of model space measures both the flat plate and the side view
  (two outlines); a quote chooses one by layer or block.
- `mi201-r2018-binary.dxf` checks the binary DXF encoder: it reads to its
  last byte (6,756 groups) and writes back byte for byte, and the DXF
  writer's header matches its variables, codes and order. It also holds
  objects AutoCAD added on loading (visual styles, materials and others)
  that the writer does not produce.
- The three AC1032 DWGs are rebuilt byte for byte by the DWG container
  writer from their own compressed pages, and every compressed stream of
  the 2004 to 2018 files keeps to the forms the compressor writes
  (`dwg/write/tests.rs`).

# cad-dwg

BlackCAD's DWG reader and writer.

- **Reads** DWG from R13 to AutoCAD 2018 and later (AC1012–AC1032) into a
  [`cad-model`](https://crates.io/crates/cad-model) drawing, except AutoCAD 2007–2009 (AC1021),
  whose container is not implemented yet. Each object is decoded on its
  own: a damaged object is recorded and the rest of the drawing loads.
- **Writes** a drawing as an AutoCAD 2018 (AC1032) file. Objects the model
  holds whole are encoded from it; objects kept as their original DWG
  records of the same version are written as they were.

```rust
let drawing = cad_dwg::read_bytes(&std::fs::read("part.dwg")?)?;
let out = cad_dwg::write::to_bytes(&drawing)?; // out.bytes, and out.notes on what was left out
```

`write::to_bytes_with` takes `WriteOptions`: the name and version the file
records as the program that wrote it, a preview image, text widths in the
program's font, and a hatch pattern library.

Checked against a corpus of one part saved by AutoCAD in every version
from R14 to 2018, with AutoCAD's own DXF of each (the repository's
`corpus/`).

Rust standard library only, no `unsafe` code; depends on `cad-model`.

## License

Licensed under either of the
[MIT License](https://github.com/withshalom/CAD/blob/main/crates/cad-dwg/LICENSE-MIT) or the
[Apache License, Version 2.0](https://github.com/withshalom/CAD/blob/main/crates/cad-dwg/LICENSE-APACHE),
at your option. The rest of
the BlackCAD repository is proprietary; see its top-level `LICENSE`.

The format knowledge comes from published specifications only (the Open
Design Alliance's public DWG specification and Autodesk's DXF reference);
the clean-room record is in the repository's `docs/PROVENANCE.md`.

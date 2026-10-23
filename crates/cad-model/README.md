# cad-model

BlackCAD's drawing database: every object keyed by its DWG-style handle,
with its owner, reactors and extension dictionary; the tables, entities,
blocks and layouts; header variables; editing with undoable changes; and
the geometry (vectors, transforms, OCS, curves), text code pages and DWG
version codes they use.

The DWG and DXF codecs, [`cad-dwg`](https://crates.io/crates/cad-dwg) and [`cad-dxf`](https://crates.io/crates/cad-dxf),
read files into it and write files from it.

Rust standard library only, no `unsafe` code, no dependencies.

## License

Licensed under either of the
[MIT License](https://github.com/withshalom/CAD/blob/main/crates/cad-model/LICENSE-MIT) or the
[Apache License, Version 2.0](https://github.com/withshalom/CAD/blob/main/crates/cad-model/LICENSE-APACHE),
at your option. The rest of
the BlackCAD repository is proprietary; see its top-level `LICENSE`.

The code was written from published specifications only; the clean-room
record is in the repository's `docs/PROVENANCE.md`.

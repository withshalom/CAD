# cad-dxf

BlackCAD's DXF reader and writer.

- **Reads** ASCII and binary DXF into a [`cad-model`](../cad-model)
  drawing, from AutoCAD's own files down to files with nothing but an
  ENTITIES section.
- **Writes** the whole drawing database as an AutoCAD 2018 (AC1032) DXF,
  ASCII or binary, with every table, block, entity and the objects the
  model can express. Objects a drawing keeps as DWG records are written
  when [`cad-dwg`](../cad-dwg) decodes them into DXF records.

```rust
let drawing = cad_dxf::read_bytes(&std::fs::read("part.dxf")?)?;
let out = cad_dxf::to_bytes(&drawing, cad_dxf::DxfFormat::Ascii); // out.bytes, out.notes
```

Rust standard library only, no `unsafe` code; depends on `cad-model` and
`cad-dwg`.

## License

Licensed under either of the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option. The rest of
the BlackCAD repository is proprietary; see its top-level `LICENSE`.

The format knowledge comes from Autodesk's published DXF reference; the
clean-room record is in the repository's `docs/PROVENANCE.md`.

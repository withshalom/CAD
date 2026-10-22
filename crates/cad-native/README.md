# cad-native

BlackCAD's native drawing format, `.blc`: the drawing as tagged records
that use the public DXF group codes, inside a container modelled on DWG's:
named sections, each compressed (DEFLATE) and checked (CRC-32), a section
directory, an object map for reading one object without loading the rest,
and two copies of the file header. On top of what DWG and DXF offer it
adds:

- **Incremental saves** to an append-only change journal that survives a
  crash, merged into a full snapshot on the next whole save.
- **A SHA-256 hash chain** over the snapshot and every change set, so a
  reader can tell whether a file was altered.
- **Recovery data** (about 1/16 of the file) that rebuilds damaged bytes.
- **A schema** in each file describing every record, so older readers can
  skip what they don't know.
- **Preserved DWG bytes** for objects the model doesn't decode yet, so a
  DWG opened and saved as `.blc` loses nothing.
- **`.bdl` lock files** for drawings shared on a network folder.

The drawing is [`cad-model`](../cad-model)'s database, the same one
[`cad-dwg`](../cad-dwg) and [`cad-dxf`](../cad-dxf) read and write.

```rust,no_run
let d = cad_dwg::read_bytes(&std::fs::read("part.dwg")?)?;
cad_native::write_file(&d, "part.blc", &Default::default())?;
let again = cad_native::read_file("part.blc")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The format leaves two things to the program that writes it, through
`WriteOptions`: its name and version (`writer`), and a preview of the
drawing (`preview_svg`: an SVG it drew already, or a function that draws
one). The DEFLATE, zlib and SHA-256 code is public as
`cad_native::codec`.

The container and its tagged records are public too, for a program that
keeps its own records in it, as CADCraft's `.cad` does: `container`
(`Builder::for_records` marks the file as holding someone else's records;
`Container::parse`, `records_of`) and `tags` (`TagWriter`, `TagReader`,
`put_record`, `records`). Compression, recovery data and the two header
copies come with them; the journal, schema and object map are BlackCAD's.

```rust
use cad_native::codec::Level;
use cad_native::container::{Builder, Container, Encoding};
use cad_native::tags::{put_record, records, TagReader, TagWriter};

let mut entities = Vec::new();
let mut w = TagWriter::new();
w.str(0, "line");
put_record(&mut entities, w);
let mut b = Builder::new(Encoding::current(true, Level::Default), true)
    .for_records(cad_native::RECORDS_CADCRAFT);
b.records("Entities", entities);
let bytes = b.finish(1, 0);

let c = Container::parse(&bytes)?;
let (raw, _crc_ok) = c.read(c.entry("Entities").ok_or("no Entities")?)?;
assert_eq!(TagReader::new(records(&raw)?[0]).str(0)?, "line");
# Ok::<(), Box<dyn std::error::Error>>(())
```

The byte-level specification is [`docs/FORMAT.md`](../../docs/FORMAT.md).

Rust standard library only, no `unsafe` code; depends on `cad-model` and
`cad-dwg`.

## License

Licensed under either of the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option. The rest of
the BlackCAD repository is proprietary; see its top-level `LICENSE`.

The code was written from published specifications only; the clean-room
record is in the repository's `docs/PROVENANCE.md`.

# Changelog

## 0.6.0 (2026-10-15)
Engine (cad-core / cad-ffi):
- All drafting tools taught by the lessons now live in the core: modify, arrays, blocks, units and scale, views (XLINE/RAY), sections and hatch, dimension commands, dimension rules, layers (plot flag), layouts and the sheet builder.
- Saved files carry the 0.6.0 stamp; DWG/DXF output now includes the synthesized dimension blocks.
- 249 + 5 + 26 + 1 + 1 tests pass; wasm has 0 imports, 19 exports.
Page:
- Drafting tutorials finished: 16 lessons in 7 units, with checked exercises. The page only prompts, previews and sends command lines to the core.

## 0.5.1
Earlier build (version number was not bumped for later engine work).

//! Compress byte segments with the format's LZ77 codec, each on its own,
//! and print the totals. Used by `tools/compaction_lab.py` to measure
//! layout experiments with the codec `.blc` uses today.
//!
//!     cargo run --release -p cad-core --example lz77_segments -- FILE...
//!
//! Each FILE holds segments framed as a little-endian u32 length followed
//! by that many bytes. Output per file: name, raw bytes, compressed bytes.
//! A segment that does not shrink counts at its raw size, as the writer
//! stores such sections uncompressed.

fn main() {
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).expect("read");
        let (mut p, mut raw, mut packed) = (0usize, 0usize, 0usize);
        while p + 4 <= data.len() {
            let n = u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as usize;
            let seg = &data[p + 4..(p + 4 + n).min(data.len())];
            raw += seg.len();
            packed += match cad_core::dwg::lz77::compress(seg) {
                Some(c) if c.len() < seg.len() => c.len(),
                _ => seg.len(),
            };
            p += 4 + n;
        }
        println!("{path}\t{raw}\t{packed}");
    }
}

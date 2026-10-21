//! Study aids for files other programs wrote: run with
//! `cargo test -p cad-core probe -- --ignored --nocapture`.

use super::r2004;
use std::path::PathBuf;

fn corpus(name: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/mosaiciron-mi201")
        .join(name);
    std::fs::read(p).ok()
}

pub(crate) fn hex_dump(bytes: &[u8]) -> String {
    let mut o = String::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        o.push_str(&format!("{:06x}  ", i * 16));
        for b in chunk {
            o.push_str(&format!("{b:02x} "));
        }
        o.push_str("  ");
        for &b in chunk {
            o.push(if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' });
        }
        o.push('\n');
    }
    o
}

#[test]
#[ignore]
fn probe_container_layout() {
    let Some(file) = corpus("mi201-r2018.dwg") else { return };
    println!("{}", r2004::describe(&file));
    let c = r2004::read(&file, super::DwgVersion::R2018).unwrap();
    for (name, bytes) in &c.others {
        println!("== {name} ({} bytes)\n{}", bytes.len(), hex_dump(&bytes[..bytes.len().min(2048)]));
    }
    println!("== handles ({} bytes)\n{}", c.handles.len(), hex_dump(&c.handles));
}

/// Every object of the 2018 file: type, handle, size, and the handle
/// references in its handle stream as (code, value).
#[test]
#[ignore]
fn probe_object_handles() {
    use super::bits::BitReader;
    let Some(file) = corpus("mi201-r2018.dwg") else { return };
    let c = r2004::read(&file, super::DwgVersion::R2018).unwrap();
    let d = super::read_bytes(&file).unwrap();
    println!("objects section starts {:02x?}", &c.objects[..8]);
    for o in d.objects.values() {
        let data = &c.objects;
        let mut r = BitReader::new(data);
        r.set_position(o.offset * 8).unwrap();
        let size = r.ms().unwrap() as usize;
        let hsize = r.mc_unsigned().unwrap() as usize;
        let start = r.position() / 8;
        let obj = &data[start..start + size];
        let crc = &data[start + size..start + size + 2];
        let bitsize = size * 8 - hsize;
        let mut h = BitReader::with_bounds(obj, bitsize, size * 8);
        let mut refs = Vec::new();
        while h.remaining() >= 8 {
            match h.handle() {
                Ok(x) => refs.push(format!("{:X}.{:X}", x.code, x.value)),
                Err(_) => break,
            }
        }
        // string stream flag
        let mut f = BitReader::new(obj);
        f.set_position(bitsize - 1).unwrap();
        let has_strings = f.b().unwrap();
        println!(
            "{:6X} {:22} off {:5} size {:4} hbits {:4} strs {} crc {:02x?} refs [{}]",
            o.handle.0,
            o.type_name,
            o.offset,
            size,
            hsize,
            has_strings as u8,
            crc,
            refs.join(" ")
        );
    }
}

/// Header and classes sections: ends, sizes and CRC checks.
#[test]
#[ignore]
fn probe_header_classes() {
    use super::crc::crc16;
    let Some(file) = corpus("mi201-r2018.dwg") else { return };
    let c = r2004::read(&file, super::DwgVersion::R2018).unwrap();
    for (name, s) in [("header", c.header.unwrap()), ("classes", c.classes.unwrap())] {
        let n = s.len();
        println!("== {name}: {n} bytes\nstart:\n{}end:\n{}", hex_dump(&s[..64]), hex_dump(&s[n - 48..]));
        let size = u32::from_le_bytes(s[16..20].try_into().unwrap()) as usize;
        let hi = u32::from_le_bytes(s[20..24].try_into().unwrap());
        println!("size {size} hi {hi}; body+crc+sentinel = {}", 24 + size + 2 + 16);
        let crc_at = 24 + size;
        let stored = u16::from_le_bytes([s[crc_at], s[crc_at + 1]]);
        for (label, from) in [("from size", 16usize), ("from body", 24)] {
            println!("  crc {label}: {:04x} (stored {stored:04x})", crc16(0xC0C1, &s[from..crc_at]));
        }
        let bitsize = u32::from_le_bytes(s[24..28].try_into().unwrap());
        println!("  first RL of body (bitsize?) {bitsize} = {} bytes + {} bits; body bits {}", bitsize / 8, bitsize % 8, size * 8);
    }
}

/// The string stream of a few objects: BS length and the UTF-16 units.
#[test]
#[ignore]
fn probe_string_streams() {
    use super::bits::BitReader;
    use super::streams::string_stream;
    let Some(file) = corpus("mi201-r2018.dwg") else { return };
    let c = r2004::read(&file, super::DwgVersion::R2018).unwrap();
    for off in [11054usize, 11090, 10031, 11286, 11645] {
        let data = &c.objects;
        let mut r = BitReader::new(data);
        r.set_position(off * 8).unwrap();
        let size = r.ms().unwrap() as usize;
        let hsize = r.mc_unsigned().unwrap() as usize;
        let start = r.position() / 8;
        let obj = &data[start..start + size];
        let endbit = size * 8 - hsize;
        let (mut s, st) = string_stream(obj, endbit).unwrap().unwrap();
        let mut out = Vec::new();
        while s.remaining() >= 2 {
            let len = s.bs().unwrap() as usize;
            let mut units = Vec::new();
            for _ in 0..len {
                units.push(s.rs().unwrap());
            }
            out.push(format!("len {len} {:?}", String::from_utf16_lossy(&units)));
            if s.remaining() < 2 { break; }
        }
        println!("obj at {off}: strings start bit {st}, endbit {endbit}: {}", out.join(" | "));
    }
}

#[test]
#[ignore]
fn probe_nongraphic() {
    let Some(file) = corpus("mi201-r2018.dwg") else { return };
    let d = super::read_bytes(&file).unwrap();
    for o in d.objects.values() {
        if let Some(r) = super::nongraphic::decode(o, &d.classes, d.codepage) {
            println!("{:X} {} owner {:X} reactors {:?} xdic {:X}: {:?}", o.handle.0, o.type_name, o.owner.0, o.reactors, o.xdictionary.0, r);
        }
    }
}

/// Page checksums, padding and the bytes after the system pages, as
/// AutoCAD wrote them.
#[test]
#[ignore]
fn probe_page_details() {
    use super::crc::page_checksum;
    let Some(file) = corpus("mi201-r2018.dwg") else { return };
    let u32_at = |o: usize| u32::from_le_bytes(file[o..o + 4].try_into().unwrap());
    let magic = r2004::magic_sequence(0x100);
    println!("0xEC..0x100 == magic[0xEC..0x100]: {}", file[0xEC..0x100] == magic[0xEC..0x100]);
    // Data pages (address, from the layout probe).
    for a in [0x100usize, 0x1a0, 0x13e0, 0x14a0, 0x4760] {
        let m = 0x4164_536Bu32 ^ a as u32;
        let mut hdr: Vec<u8> = (0..8).flat_map(|i| (u32_at(a + i * 4) ^ m).to_le_bytes()).collect();
        let size = u32::from_le_bytes(hdr[8..12].try_into().unwrap()) as usize;
        let stored_h = u32::from_le_bytes(hdr[24..28].try_into().unwrap());
        let stored_d = u32::from_le_bytes(hdr[28..32].try_into().unwrap());
        let data = &file[a + 32..a + 32 + size];
        let dsum = page_checksum(0, data);
        hdr[24..28].fill(0);
        let hsum = page_checksum(dsum, &hdr);
        let mut hdr0 = hdr.clone();
        hdr0[28..32].fill(0);
        let hsum0 = page_checksum(dsum, &hdr0);
        let end = a + 32 + size;
        let page = u32::from_le_bytes(hdr[12..16].try_into().unwrap()) as usize;
        let pad = &file[end..a + page];
        let pos = magic.windows(pad.len().max(1)).position(|w| w == pad);
        println!("page {a:#x}: data sum {dsum:#x} stored {stored_d:#x}; header sum {hsum:#x} (with data sum field) {hsum0:#x} (zeroed) stored {stored_h:#x}; pad {} bytes {:02x?} found in magic at {:?}", pad.len(), pad, pos);
    }
    for a in [0x4b20usize, 0x4d40] {
        let comp = u32_at(a + 8) as usize;
        let end = a + 20 + comp;
        println!("system page {a:#x}: after data {:02x?}", &file[end..(end + 40).min(file.len())]);
    }
}

#[test]
#[ignore]
fn probe_other_containers() {
    for name in ["mi201-r2004.dwg", "mi201-r2010.dwg", "mi201-r2013.dwg"] {
        let Some(file) = corpus(name) else { continue };
        println!("===== {name}\n{}", r2004::describe(&file));
    }
}

/// What fills each data page past its section's content.
#[test]
#[ignore]
fn probe_page_padding() {
    use super::lz77;
    let Some(file) = corpus("mi201-r2018.dwg") else { return };
    let u32_at = |o: usize| u32::from_le_bytes(file[o..o + 4].try_into().unwrap());
    // (address, section size, compressed)
    for (a, size, comp) in [
        (0x100usize, 0x4e, false), (0x1a0, 0x545, false), (0x9c0, 0x2ce, false), (0xce0, 0x522, false),
        (0x1280, 0xcc, true), (0x13e0, 0x10, true), (0x14a0, 0x474d, true), (0x3ca0, 0x59, true),
        (0x3d80, 6, true), (0x3e40, 0x232, true), (0x4120, 0x546, true), (0x4660, 0x81, true), (0x4760, 0x36c, true),
    ] {
        let m = 0x4164_536Bu32 ^ a as u32;
        let ds = (u32_at(a + 8) ^ m) as usize;
        let body = &file[a + 32..a + 32 + ds];
        let data = if comp { lz77::decompress(body, 0x10_0000).unwrap() } else { body.to_vec() };
        let tail = &data[size.min(data.len())..];
        let nz = tail.iter().filter(|&&b| b != 0).count();
        println!("page {a:#x}: stored {ds:#x}, decompressed {:#x}, content {size:#x}, tail {} bytes, nonzero {nz}; tail starts {:02x?}", data.len(), tail.len(), &tail[..tail.len().min(24)]);
        if comp && a == 0x3d80 { println!("   compressed bytes {:02x?}", body); }
    }
}

/// `DWGFILE=path cargo test probe_describe_file -- --ignored --nocapture`
#[test]
#[ignore]
fn probe_describe_file() {
    let Ok(p) = std::env::var("DWGFILE") else { return };
    let file = std::fs::read(p).unwrap();
    println!("{}", r2004::describe(&file));
    let c = r2004::read(&file, super::DwgVersion::R2018).unwrap();
    for (name, bytes) in &c.others {
        println!("== {name} ({} bytes)\n{}", bytes.len(), hex_dump(&bytes[..bytes.len().min(256)]));
    }
}

/// `DWG=path cargo test probe_any -- --ignored --nocapture`
#[test]
#[ignore]
fn probe_any() {
    let Ok(p) = std::env::var("DWG") else { return };
    let file = std::fs::read(p).unwrap();
    println!("{}", r2004::describe(&file));
    let c = r2004::read(&file, super::DwgVersion::R2018).unwrap();
    for (name, bytes) in &c.others {
        println!("== {name} ({} bytes)\n{}", bytes.len(), hex_dump(&bytes[..bytes.len().min(512)]));
    }
    let h = c.header.as_ref().unwrap();
    println!("== header first 32 {:02x?}\n   last 40 {:02x?}", &h[..32], &h[h.len() - 40..]);
    println!("== objects first 16 {:02x?} len {}", &c.objects[..16], c.objects.len());
}

#[test]
#[ignore]
fn probe_header_tails() {
    use super::bits::BitReader;
    for (name, ver) in [("mi201-r2004.dwg", super::DwgVersion::R2004), ("mi201-r2010.dwg", super::DwgVersion::R2010), ("mi201-r2013.dwg", super::DwgVersion::R2013), ("mi201-r2018.dwg", super::DwgVersion::R2018)] {
        let Some(file) = corpus(name) else { continue };
        let c = r2004::read(&file, ver).unwrap();
        let s = c.header.as_ref().unwrap();
        let size = u32::from_le_bytes(s[16..20].try_into().unwrap()) as usize;
        let off = if ver >= super::DwgVersion::R2010 { 24 } else { 20 };
        let body = &s[off..off + size];
        if ver < super::DwgVersion::R2007 {
            println!("{name}: size {size}, last 16 {:02x?}", &body[size - 16..]);
            continue;
        }
        let endbit = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
        let mut r = BitReader::with_bounds(body, endbit, size * 8);
        let mut refs = Vec::new();
        while r.remaining() >= 8 {
            match r.handle() {
                Ok(h) if h.code <= 5 => refs.push(format!("{:X}.{:X}", h.code, h.value)),
                _ => break,
            }
        }
        println!("{name}: size {size} endbit {endbit}, {} refs, last refs {:?}", refs.len(), &refs[refs.len().saturating_sub(4)..]);
        let mut t = r.clone();
        let mut tail = String::new();
        while t.remaining() > 0 { tail.push(if t.b().unwrap() { '1' } else { '0' }); }
        println!("   tail {} bits {tail}", tail.len());
    }
}

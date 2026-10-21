//! R2004-family container (AC1018, AC1024, AC1027, AC1032): an encrypted
//! file header, a page map, a section map and compressed data pages.
//!
//! Spec §4.1–§4.7 (verified against the spec text). R2010, R2013 and R2018
//! use the same container (spec chapters 6–8).

use super::container::Container;
use super::crc::{crc32, page_checksum};
use super::lz77;
use super::version::DwgVersion;
use crate::error::{Error, ErrorKind, Result};
use crate::model::{Diagnostic, SectionInfo, Severity};
use std::collections::HashMap;

const FILE_ID: &[u8; 12] = b"AcFssFcAJMB\0";
const PAGE_MAP_TYPE: u32 = 0x4163_0E3B;
const SECTION_MAP_TYPE: u32 = 0x4163_003B;
const DATA_PAGE_TYPE: u32 = 0x4163_043B;
const PAGE_MASK: u32 = 0x4164_536B;
/// Refuse to allocate absurd section sizes from corrupt maps.
const MAX_SECTION: u64 = 2 * 1024 * 1024 * 1024;

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}
fn u64_at(d: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(o..o + 8)?.try_into().ok()?))
}

/// The "magic number sequence" (§4.1): an LCG with seed 1.
pub(crate) fn magic_sequence(len: usize) -> Vec<u8> {
    let mut seed: u32 = 1;
    (0..len)
        .map(|_| {
            seed = seed.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
            (seed >> 16) as u8
        })
        .collect()
}

struct Page {
    address: u64,
    size: u32,
}

#[derive(Debug)]
struct PageRef {
    number: i32,
    data_size: u32,
    start: u64,
}

#[derive(Debug)]
struct SectionDesc {
    name: String,
    size: u64,
    max_decompressed: u32,
    compressed: u32,
    encrypted: u32,
    pages: Vec<PageRef>,
}

/// Read a system page (page map or section map) at `addr`.
fn system_page(file: &[u8], addr: u64, notes: &mut Vec<Diagnostic>) -> Result<(u32, Vec<u8>)> {
    let a = usize::try_from(addr).map_err(|_| Error::format("page address overflow"))?;
    let hdr = file.get(a..a + 20).ok_or_else(|| {
        Error::truncated(format!("system page header at {a:#x} past end of file"))
    })?;
    let kind = u32_at(hdr, 0).unwrap_or(0);
    let decompressed = u32_at(hdr, 4).unwrap_or(0) as usize;
    let compressed = u32_at(hdr, 8).unwrap_or(0) as usize;
    let compression = u32_at(hdr, 12).unwrap_or(0);
    let stored_checksum = u32_at(hdr, 16).unwrap_or(0);
    let body = file
        .get(a + 20..a + 20 + compressed)
        .ok_or_else(|| Error::truncated(format!("system page data at {a:#x} past end of file")))?;
    // §4.3: header checksum (seed 0, checksum field zeroed), then the data
    // with that result as the seed. Reported, not enforced.
    let mut h = hdr.to_vec();
    h[16..20].fill(0);
    let computed = page_checksum(page_checksum(0, &h), body);
    if computed != stored_checksum {
        notes.push(Diagnostic {
            severity: Severity::Info,
            message: format!(
                "system page {kind:#010x} at {a:#x}: checksum {stored_checksum:#010x} != computed {computed:#010x}"
            ),
        });
    }
    let data = if compression == 2 {
        lz77::decompress(body, decompressed)
            .map_err(|e| e.context(format!("system page at {a:#x}")))?
    } else {
        body.to_vec()
    };
    Ok((kind, data))
}

pub(crate) fn read(file: &[u8], ver: DwgVersion) -> Result<Container> {
    if file.len() < 0x100 {
        return Err(Error::truncated(
            "file is shorter than the R2004 file header",
        ));
    }
    let maintenance = file[0x0B];
    let codepage = u16::from_le_bytes([file[0x13], file[0x14]]);
    let security_flags = u32_at(file, 0x18).unwrap_or(0);
    let mut notes = Vec::new();
    if security_flags != 0 {
        notes.push(Diagnostic {
            severity: Severity::Warning,
            message: format!(
                "security flags {security_flags:#x}: the drawing may be password protected"
            ),
        });
    }

    // Decrypt the 0x6C-byte header block at 0x80.
    let mask = magic_sequence(0x6C);
    let mut hdr: Vec<u8> = file[0x80..0x80 + 0x6C]
        .iter()
        .zip(&mask)
        .map(|(a, b)| a ^ b)
        .collect();
    if &hdr[..12] != FILE_ID {
        return Err(Error::format(format!(
            "{}: encrypted file header did not decode to the expected file ID",
            ver.magic()
        )));
    }
    let stored_crc = u32_at(&hdr, 0x68).unwrap_or(0);
    hdr[0x68..0x6C].fill(0);
    let computed_crc = crc32(0, &hdr);
    if computed_crc != stored_crc {
        notes.push(Diagnostic {
            severity: Severity::Warning,
            message: format!(
                "file header CRC32 {stored_crc:#010x} != computed {computed_crc:#010x}"
            ),
        });
    }
    let page_map_address = u64_at(&hdr, 0x54).unwrap_or(0) + 0x100;
    let section_map_id = u32_at(&hdr, 0x5C).unwrap_or(0) as i32;

    // Page map: page number -> file address.
    let (kind, pm) =
        system_page(file, page_map_address, &mut notes).map_err(|e| e.context("page map"))?;
    if kind != PAGE_MAP_TYPE {
        return Err(Error::format(format!(
            "page map has type {kind:#010x}, expected {PAGE_MAP_TYPE:#010x}"
        )));
    }
    let mut pages: HashMap<i32, Page> = HashMap::new();
    let mut addr: u64 = 0x100;
    let mut p = 0usize;
    while p + 8 <= pm.len() {
        let number = u32_at(&pm, p).unwrap_or(0) as i32;
        let size = u32_at(&pm, p + 4).unwrap_or(0);
        p += 8;
        if number < 0 {
            p += 16; // gap: parent, left, right, 0
        } else {
            pages.insert(
                number,
                Page {
                    address: addr,
                    size,
                },
            );
        }
        addr += size as u64;
    }

    // Section map.
    let sm_addr = pages
        .get(&section_map_id)
        .map(|pg| pg.address)
        .ok_or_else(|| {
            Error::format(format!("section map page {section_map_id} not in page map"))
        })?;
    let (kind, sm) =
        system_page(file, sm_addr, &mut notes).map_err(|e| e.context("section map"))?;
    if kind != SECTION_MAP_TYPE {
        return Err(Error::format(format!(
            "section map has type {kind:#010x}, expected {SECTION_MAP_TYPE:#010x}"
        )));
    }
    let n_desc = u32_at(&sm, 0).ok_or_else(|| Error::truncated("section map header"))? as usize;
    let mut descs = Vec::new();
    let mut q = 20usize;
    for _ in 0..n_desc.min(4096) {
        let size = u64_at(&sm, q).ok_or_else(|| Error::truncated("section description"))?;
        let page_count = u32_at(&sm, q + 8).unwrap_or(0) as usize;
        let max_decompressed = u32_at(&sm, q + 12).unwrap_or(0x7400);
        let compressed = u32_at(&sm, q + 20).unwrap_or(2);
        let encrypted = u32_at(&sm, q + 28).unwrap_or(0);
        let name_bytes = sm
            .get(q + 32..q + 96)
            .ok_or_else(|| Error::truncated("section name"))?;
        let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(64);
        let name = String::from_utf8_lossy(&name_bytes[..name_len]).into_owned();
        q += 96;
        let mut page_refs = Vec::with_capacity(page_count.min(65_536));
        for _ in 0..page_count {
            let number =
                u32_at(&sm, q).ok_or_else(|| Error::truncated("section page record"))? as i32;
            let data_size = u32_at(&sm, q + 4).unwrap_or(0);
            let start = u64_at(&sm, q + 8).unwrap_or(0);
            q += 16;
            page_refs.push(PageRef {
                number,
                data_size,
                start,
            });
        }
        descs.push(SectionDesc {
            name,
            size,
            max_decompressed,
            compressed,
            encrypted,
            pages: page_refs,
        });
    }

    let sections: Vec<SectionInfo> = descs
        .iter()
        .map(|d| SectionInfo {
            name: if d.name.is_empty() {
                "(unnamed)".into()
            } else {
                d.name.clone()
            },
            size: d.size,
            pages: d.pages.len() as u32,
            compressed: d.compressed == 2,
            encrypted: d.encrypted == 1,
        })
        .collect();

    let find = |name: &str| descs.iter().find(|d| d.name == name);
    let load = |name: &str, notes: &mut Vec<Diagnostic>| -> Result<Option<Vec<u8>>> {
        match find(name) {
            None => Ok(None),
            Some(d) => assemble(file, d, &pages, notes)
                .map(Some)
                .map_err(|e| e.context(name)),
        }
    };
    let header = load("AcDb:Header", &mut notes).unwrap_or_else(|e| {
        notes.push(Diagnostic {
            severity: Severity::Warning,
            message: e.to_string(),
        });
        None
    });
    let classes = load("AcDb:Classes", &mut notes).unwrap_or_else(|e| {
        notes.push(Diagnostic {
            severity: Severity::Warning,
            message: e.to_string(),
        });
        None
    });
    let handles = load("AcDb:Handles", &mut notes)?
        .ok_or_else(|| Error::format("no AcDb:Handles section"))?;
    let objects = load("AcDb:AcDbObjects", &mut notes)?
        .ok_or_else(|| Error::format("no AcDb:AcDbObjects section"))?;
    let mut others = Vec::new();
    for d in &descs {
        if d.name.is_empty()
            || matches!(
                d.name.as_str(),
                "AcDb:Header" | "AcDb:Classes" | "AcDb:Handles" | "AcDb:AcDbObjects"
            )
        {
            continue;
        }
        let mut quiet = Vec::new();
        if let Ok(bytes) = assemble(file, d, &pages, &mut quiet) {
            others.push((d.name.clone(), bytes));
        }
    }

    Ok(Container {
        maintenance,
        codepage,
        header,
        classes,
        handles,
        objects,
        sections,
        others,
        notes,
    })
}

/// A readable account of an R2004-family file's container: the file
/// header fields, the page map and the section map with each page's
/// masked header. For studying files written by other programs.
#[cfg(test)]
pub(crate) fn describe(file: &[u8]) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    let _ = writeln!(o, "file header bytes 0..0x80: {:02x?}", &file[..0x80]);
    let mask = magic_sequence(0x6C);
    let hdr: Vec<u8> = file[0x80..0xEC].iter().zip(&mask).map(|(a, b)| a ^ b).collect();
    for off in (0x0C..0x6C).step_by(4) {
        let _ = writeln!(o, "  enc[{off:#04x}] = {:#010x}", u32_at(&hdr, off).unwrap());
    }
    let _ = writeln!(o, "bytes 0xEC..0x100: {:02x?}", &file[0xEC..0x100]);
    let tail = &file[file.len() - 0x6C..];
    let tail_dec: Vec<u8> = tail.iter().zip(&mask).map(|(a, b)| a ^ b).collect();
    let _ = writeln!(o, "trailer header equal to first: {}", tail_dec == hdr);
    let pm_addr = u64_at(&hdr, 0x54).unwrap() + 0x100;
    let mut notes = Vec::new();
    let (_, pm) = system_page(file, pm_addr, &mut notes).unwrap();
    let _ = writeln!(o, "page map at {pm_addr:#x}, {} bytes, header {:02x?}", pm.len(), &file[pm_addr as usize..pm_addr as usize + 20]);
    let mut addr = 0x100u64;
    let mut p = 0;
    while p + 8 <= pm.len() {
        let n = u32_at(&pm, p).unwrap() as i32;
        let s = u32_at(&pm, p + 4).unwrap();
        let _ = writeln!(o, "  page {n:4} size {s:#7x} at {addr:#8x}");
        p += 8;
        if n < 0 {
            p += 16;
        }
        addr += s as u64;
    }
    let _ = writeln!(o, "file length {:#x}, pages end at {addr:#x}", file.len());
    // Section map.
    let sm_id = u32_at(&hdr, 0x5C).unwrap() as i32;
    let mut addr = 0x100u64;
    let mut p = 0;
    let mut page_addr = HashMap::new();
    while p + 8 <= pm.len() {
        let n = u32_at(&pm, p).unwrap() as i32;
        let s = u32_at(&pm, p + 4).unwrap();
        page_addr.insert(n, addr);
        p += if n < 0 { 24 } else { 8 };
        addr += s as u64;
    }
    let sm_addr = page_addr[&sm_id];
    let (_, sm) = system_page(file, sm_addr, &mut notes).unwrap();
    let _ = writeln!(
        o,
        "section map page {sm_id} at {sm_addr:#x}, {} bytes, header {:02x?}; first words {:?}",
        sm.len(),
        &file[sm_addr as usize..sm_addr as usize + 20],
        (0..5).map(|i| u32_at(&sm, i * 4).unwrap()).collect::<Vec<_>>()
    );
    let n_desc = u32_at(&sm, 0).unwrap() as usize;
    let mut q = 20;
    for _ in 0..n_desc {
        let size = u64_at(&sm, q).unwrap();
        let count = u32_at(&sm, q + 8).unwrap();
        let words: Vec<u32> = (0..6).map(|i| u32_at(&sm, q + 8 + i * 4).unwrap()).collect();
        let name_bytes = &sm[q + 32..q + 96];
        let len = name_bytes.iter().position(|&b| b == 0).unwrap_or(64);
        let _ = writeln!(
            o,
            "  section {:?} size {size:#x} words(count,max,unk,compr,id,enc) {words:?} name-tail nonzero {}",
            String::from_utf8_lossy(&name_bytes[..len]),
            name_bytes[len..].iter().any(|&b| b != 0)
        );
        q += 96;
        for _ in 0..count {
            let num = u32_at(&sm, q).unwrap() as i32;
            let ds = u32_at(&sm, q + 4).unwrap();
            let st = u64_at(&sm, q + 8).unwrap();
            let a = page_addr[&num];
            let m = PAGE_MASK ^ (a as u32);
            let w: Vec<u32> = (0..8)
                .map(|i| u32_at(file, a as usize + i * 4).unwrap() ^ m)
                .collect();
            let _ = writeln!(
                o,
                "     page {num} data {ds:#x} start {st:#x} at {a:#x}; header {:x?}",
                w
            );
            q += 16;
        }
    }
    o.push_str(&notes.iter().map(|n| n.message.clone()).collect::<Vec<_>>().join("\n"));
    o
}

/// Reassemble one section from its pages.
fn assemble(
    file: &[u8],
    d: &SectionDesc,
    pages: &HashMap<i32, Page>,
    notes: &mut Vec<Diagnostic>,
) -> Result<Vec<u8>> {
    if d.encrypted == 1 {
        return Err(Error::new(
            ErrorKind::Unsupported,
            "section is encrypted (password-protected drawing)",
        ));
    }
    if d.size > MAX_SECTION {
        return Err(Error::format(format!(
            "section size {} is implausible",
            d.size
        )));
    }
    let mut buf = vec![0u8; d.size as usize];
    for pr in &d.pages {
        let page = pages
            .get(&pr.number)
            .ok_or_else(|| Error::format(format!("page {} not in page map", pr.number)))?;
        let a = page.address as usize;
        let raw_hdr = file.get(a..a + 32).ok_or_else(|| {
            Error::truncated(format!("data page header at {a:#x} past end of file"))
        })?;
        // §4.6: each 32-bit word of the header is XORed with 0x4164536B ^ address.
        let m = PAGE_MASK ^ (page.address as u32);
        let word = |i: usize| u32::from_le_bytes([raw_hdr[i * 4], raw_hdr[i * 4 + 1], raw_hdr[i * 4 + 2], raw_hdr[i * 4 + 3]]) ^ m;
        let page_type = word(0);
        if page_type != DATA_PAGE_TYPE {
            notes.push(Diagnostic {
                severity: Severity::Info,
                message: format!(
                    "{}: page {} header type {page_type:#010x} unexpected",
                    d.name, pr.number
                ),
            });
        }
        let header_size = word(2) as usize;
        let comp_size = if pr.data_size > 0 {
            pr.data_size as usize
        } else {
            header_size
        };
        let body = file.get(a + 32..a + 32 + comp_size).ok_or_else(|| {
            Error::truncated(format!(
                "data page {} ({comp_size} bytes at {a:#x}) past end of file",
                pr.number
            ))
        })?;
        let _ = page.size;
        let data = if d.compressed == 2 {
            let limit = (d.max_decompressed.max(word(3)) as usize).clamp(1, 0x10_0000);
            lz77::decompress(body, limit).map_err(|e| e.context(format!("page {}", pr.number)))?
        } else {
            body.to_vec()
        };
        let start = pr.start as usize;
        if start < buf.len() {
            let n = data.len().min(buf.len() - start);
            buf[start..start + n].copy_from_slice(&data[..n]);
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_sequence_starts_as_documented_lcg() {
        // seed = 1 * 0x343FD + 0x269EC3 = 0x29E2C0 -> first byte 0x29.
        let m = magic_sequence(4);
        assert_eq!(m[0], 0x29);
        // Recompute independently.
        let mut s: u64 = 1;
        let mut v = Vec::new();
        for _ in 0..4 {
            s = (s * 0x343FD + 0x269EC3) & 0xFFFF_FFFF;
            v.push((s >> 16) as u8);
        }
        assert_eq!(m, v);
    }
}

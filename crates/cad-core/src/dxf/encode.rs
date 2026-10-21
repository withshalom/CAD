//! Group codes and values, and their two encodings: ASCII DXF and binary
//! DXF (Autodesk DXF reference, "Group Code Value Types" and "Binary DXF
//! Files").
//!
//! In ASCII each group is two lines, the code right-aligned in three
//! columns and the value, as AutoCAD writes them (integers padded the way
//! AutoCAD pads them). In binary the file starts with a 22-byte sentinel,
//! each code is a 2-byte little-endian integer, and each value is stored
//! by the type the "Group Code Value Types" table gives its code (the
//! reference says binary DXF types values "by the same rules used in
//! decoding ASCII DXF files"): strings and handles UTF-8 and 0-terminated,
//! doubles 8 bytes, 16-, 32- and 64-bit integers 2, 4 and 8 bytes. 280–289
//! are 16-bit in that table, so 2 bytes. The reference leaves two sizes
//! open, and AutoCAD's own binary DXF settles them: booleans (290–299) are
//! one byte, and binary chunks (310–319) are stored like 1004, a length
//! byte and then the bytes.
//!
//! The rules were checked against a binary DXF saved by AutoCAD 2027
//! (`corpus/mosaiciron-mi201/mi201-r2018-binary.dxf`): [`from_binary`]
//! reads its 6,756 groups ending exactly at its last byte, and
//! [`to_binary`] writes them back byte for byte (`tests/corpus.rs`).

/// A group value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Real(f64),
    /// Any integer; its code decides how it is stored.
    Int(i64),
    Bool(bool),
    Bytes(Vec<u8>),
    /// A handle, written as hexadecimal text.
    Handle(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub code: u16,
    pub value: Value,
}

/// How a group code's value is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Str,
    Real,
    I16,
    I32,
    I64,
    Bool,
    Bytes,
    Handle,
}

/// The value type of a group code (DXF reference, "Group Code Value
/// Types"). Codes outside every range are treated as strings.
pub fn kind(code: u16) -> Kind {
    match code {
        5 | 105 | 320..=369 | 390..=399 | 480..=481 | 1005 => Kind::Handle,
        0..=9 | 100..=102 | 300..=309 | 410..=419 | 430..=439 | 470..=479 | 999 | 1000..=1003 => {
            Kind::Str
        }
        10..=59 | 110..=149 | 210..=239 | 460..=469 | 1010..=1059 => Kind::Real,
        60..=79 | 170..=179 | 270..=289 | 370..=389 | 400..=409 | 1060..=1070 => Kind::I16,
        90..=99 | 420..=429 | 440..=459 | 1071 => Kind::I32,
        160..=169 => Kind::I64,
        290..=299 => Kind::Bool,
        310..=319 | 1004 => Kind::Bytes,
        _ => Kind::Str,
    }
}

/// A real as AutoCAD reads it back exactly: the shortest text that round
/// trips, always with a decimal point, in E notation outside 1e-6–1e16.
pub fn real_text(v: f64) -> String {
    if !v.is_finite() {
        return "0.0".into();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let a = v.abs();
    if !(1e-6..1e16).contains(&a) {
        let s = format!("{v:E}");
        let (m, e) = s.split_once('E').unwrap_or((&s, "0"));
        let m = if m.contains('.') { m.to_string() } else { format!("{m}.0") };
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{m}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let s = format!("{v}");
    if s.contains('.') {
        s
    } else {
        format!("{s}.0")
    }
}

fn int_of(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        Value::Bool(b) => *b as i64,
        Value::Real(r) => *r as i64,
        Value::Handle(h) => *h as i64,
        Value::Str(s) => s.trim().parse().unwrap_or(0),
        Value::Bytes(_) => 0,
    }
}

/// Text in a DXF line: line breaks and other control characters would end
/// the line early, so they become spaces.
fn line_safe(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// ASCII DXF, CR LF line ends, UTF-8 text (AC1021 and later).
pub fn to_ascii(groups: &[Group]) -> Vec<u8> {
    use std::fmt::Write;
    let mut o = String::with_capacity(groups.len() * 16);
    for g in groups {
        let _ = write!(o, "{:>3}\r\n", g.code);
        let k = kind(g.code);
        match (&g.value, k) {
            (Value::Str(s), _) => o.push_str(&line_safe(s)),
            (Value::Handle(h), _) => {
                let _ = write!(o, "{h:X}");
            }
            (Value::Bytes(b), _) => {
                for x in b {
                    let _ = write!(o, "{x:02X}");
                }
            }
            (Value::Real(r), Kind::Real) => o.push_str(&real_text(*r)),
            (v, Kind::I32) => {
                let _ = write!(o, "{:>9}", int_of(v));
            }
            (v, Kind::I64) => {
                let _ = write!(o, "{:>18}", int_of(v));
            }
            (v, Kind::I16 | Kind::Bool) => {
                let _ = write!(o, "{:>6}", int_of(v));
            }
            (Value::Real(r), _) => o.push_str(&real_text(*r)),
            (v, _) => {
                let _ = write!(o, "{}", int_of(v));
            }
        }
        o.push_str("\r\n");
    }
    o.into_bytes()
}

/// The binary DXF sentinel.
pub const BINARY_SENTINEL: &[u8; 22] = b"AutoCAD Binary DXF\r\n\x1a\0";

/// Binary DXF.
pub fn to_binary(groups: &[Group]) -> Vec<u8> {
    let mut o = Vec::with_capacity(groups.len() * 12);
    o.extend_from_slice(BINARY_SENTINEL);
    for g in groups {
        o.extend_from_slice(&g.code.to_le_bytes());
        match kind(g.code) {
            Kind::Str => {
                let s = match &g.value {
                    Value::Str(s) => s.clone(),
                    Value::Real(r) => real_text(*r),
                    other => int_of(other).to_string(),
                };
                o.extend(s.bytes().filter(|&b| b != 0));
                o.push(0);
            }
            Kind::Handle => {
                let s = match &g.value {
                    Value::Handle(h) => format!("{h:X}"),
                    Value::Str(s) => s.clone(),
                    other => format!("{:X}", int_of(other)),
                };
                o.extend_from_slice(s.as_bytes());
                o.push(0);
            }
            Kind::Real => {
                let r = match &g.value {
                    Value::Real(r) => *r,
                    other => int_of(other) as f64,
                };
                o.extend_from_slice(&r.to_le_bytes());
            }
            Kind::Bool => o.push(int_of(&g.value) as u8),
            Kind::I16 => o.extend_from_slice(&(int_of(&g.value) as i16).to_le_bytes()),
            Kind::I32 => o.extend_from_slice(&(int_of(&g.value) as i32).to_le_bytes()),
            Kind::I64 => o.extend_from_slice(&int_of(&g.value).to_le_bytes()),
            Kind::Bytes => {
                let b = match &g.value {
                    Value::Bytes(b) => b.as_slice(),
                    _ => &[],
                };
                let b = &b[..b.len().min(255)];
                o.push(b.len() as u8);
                o.extend_from_slice(b);
            }
        }
    }
    o
}

/// Reads a binary DXF back into groups, sizing each value by its code as
/// [`to_binary`] does. Handles become [`Value::Handle`] when they are the
/// plain upper-case hexadecimal [`to_binary`] writes, and strings otherwise,
/// so writing the groups again gives the same bytes. Text is UTF-8
/// (AC1021 and later); bytes that are not are replaced.
pub fn from_binary(bytes: &[u8]) -> crate::Result<Vec<Group>> {
    use crate::Error;
    let body = bytes
        .strip_prefix(&BINARY_SENTINEL[..])
        .ok_or_else(|| Error::format("not a binary DXF: no sentinel"))?;
    let mut p = 0usize;
    let take = |p: &mut usize, n: usize| -> crate::Result<&[u8]> {
        let s = body
            .get(*p..*p + n)
            .ok_or_else(|| Error::truncated(format!("binary DXF ends inside a group at byte {}", *p + 22)))?;
        *p += n;
        Ok(s)
    };
    let text = |p: &mut usize| -> crate::Result<String> {
        let rest = body.get(*p..).unwrap_or(&[]);
        let n = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::truncated(format!("unterminated string at byte {}", *p + 22)))?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        *p += n + 1;
        Ok(s)
    };
    let mut out = Vec::new();
    while p < body.len() {
        let c = take(&mut p, 2)?;
        let code = u16::from_le_bytes([c[0], c[1]]);
        let value = match kind(code) {
            Kind::Str => Value::Str(text(&mut p)?),
            Kind::Handle => {
                let s = text(&mut p)?;
                match u64::from_str_radix(&s, 16) {
                    Ok(h) if format!("{h:X}") == s => Value::Handle(h),
                    _ => Value::Str(s),
                }
            }
            Kind::Real => Value::Real(f64::from_le_bytes(take(&mut p, 8)?.try_into().unwrap())),
            Kind::I16 => Value::Int(i16::from_le_bytes(take(&mut p, 2)?.try_into().unwrap()) as i64),
            Kind::I32 => Value::Int(i32::from_le_bytes(take(&mut p, 4)?.try_into().unwrap()) as i64),
            Kind::I64 => Value::Int(i64::from_le_bytes(take(&mut p, 8)?.try_into().unwrap())),
            Kind::Bool => match take(&mut p, 1)?[0] {
                b @ (0 | 1) => Value::Bool(b == 1),
                b => Value::Int(b as i64),
            },
            Kind::Bytes => {
                let n = take(&mut p, 1)?[0] as usize;
                Value::Bytes(take(&mut p, n)?.to_vec())
            }
        };
        out.push(Group { code, value });
    }
    Ok(out)
}

/// Reads an ASCII DXF into groups, typing each value by its code as
/// [`to_ascii`] writes it. Text is UTF-8 (AC1021 and later); a file that is
/// not is read as Windows-1252.
pub fn from_ascii(bytes: &[u8]) -> crate::Result<Vec<Group>> {
    use crate::Error;
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t.to_string(),
        Err(_) => crate::text::decode_ansi(bytes, 30),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut out = Vec::with_capacity(lines.len() / 2);
    let mut i = 0;
    while i + 1 < lines.len() {
        let c = lines[i].trim();
        let raw = lines[i + 1];
        let code: u16 = c
            .parse()
            .map_err(|_| Error::format(format!("DXF line {}: group code {c:?}", i + 1)))?;
        let t = raw.trim();
        let bad = || Error::format(format!("DXF line {}: group {code} value {t:?}", i + 2));
        let value = match kind(code) {
            Kind::Str => Value::Str(raw.to_string()),
            Kind::Handle => match u64::from_str_radix(t, 16) {
                Ok(h) => Value::Handle(h),
                Err(_) => Value::Str(raw.to_string()),
            },
            Kind::Real => Value::Real(t.parse().map_err(|_| bad())?),
            Kind::I16 | Kind::I32 | Kind::I64 | Kind::Bool => match t.parse::<i64>() {
                Ok(v) => Value::Int(v),
                Err(_) => Value::Int(t.parse::<f64>().map_err(|_| bad())? as i64),
            },
            Kind::Bytes => {
                if !t.len().is_multiple_of(2) {
                    return Err(bad());
                }
                let b: Option<Vec<u8>> = (0..t.len())
                    .step_by(2)
                    .map(|j| u8::from_str_radix(&t[j..j + 2], 16).ok())
                    .collect();
                Value::Bytes(b.ok_or_else(bad)?)
            }
        };
        out.push(Group { code, value });
        i += 2;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_groups_read_back_as_written() {
        let groups = vec![
            g(0, Value::Str("SECTION".into())),
            g(2, Value::Str("BEND UP 90° R 3/8".into())),
            g(5, Value::Handle(0x3D0B5)),
            g(70, Value::Int(-2)),
            g(90, Value::Int(427)),
            g(160, Value::Int(1 << 40)),
            g(280, Value::Int(127)),
            g(290, Value::Bool(true)),
            g(310, Value::Bytes(vec![0x89, b'P', b'N', b'G'])),
            g(40, Value::Real(0.24999999999974698)),
            g(1071, Value::Int(-7)),
            g(0, Value::Str("EOF".into())),
        ];
        let b = to_binary(&groups);
        assert_eq!(from_binary(&b).unwrap(), groups);
        assert!(from_binary(&b[..b.len() - 1]).is_err());
        assert!(from_binary(b"AutoCAD DXF").is_err());
    }

    fn g(code: u16, value: Value) -> Group {
        Group { code, value }
    }

    #[test]
    fn reals_read_back_exactly_and_look_like_autocads() {
        for v in [0.0, 1.0, 0.2, 120.0, 44.797841552215395, -122.394, 1e20, -1e20, 1e-9, 123456789.125] {
            let t = real_text(v);
            assert!(t.contains('.'), "{t}");
            assert_eq!(t.parse::<f64>().unwrap(), v, "{t}");
        }
        assert_eq!(real_text(1e20), "1.0E+20");
        assert_eq!(real_text(-1e20), "-1.0E+20");
        assert_eq!(real_text(12.0), "12.0");
        assert_eq!(real_text(0.0), "0.0");
    }

    #[test]
    fn ascii_groups_are_laid_out_as_autocad_writes_them() {
        let groups = [
            g(0, Value::Str("SECTION".into())),
            g(70, Value::Int(32)),
            g(90, Value::Int(0)),
            g(160, Value::Int(0)),
            g(5, Value::Handle(0x3D0B5)),
            g(40, Value::Real(0.18)),
            g(1001, Value::Str("ACAD".into())),
        ];
        let text = String::from_utf8(to_ascii(&groups)).unwrap();
        assert_eq!(
            text,
            "  0\r\nSECTION\r\n 70\r\n    32\r\n 90\r\n        0\r\n160\r\n                 0\r\n  5\r\n3D0B5\r\n 40\r\n0.18\r\n1001\r\nACAD\r\n"
        );
    }

    #[test]
    fn binary_values_take_their_codes_sizes() {
        let groups = [
            g(0, Value::Str("EOF".into())),
            g(70, Value::Int(-2)),
            g(90, Value::Int(7)),
            g(280, Value::Int(1)),
            g(290, Value::Bool(true)),
            g(40, Value::Real(1.5)),
            g(330, Value::Handle(0x1F)),
            g(310, Value::Bytes(vec![0xAB, 0xCD])),
        ];
        let b = to_binary(&groups);
        let mut want = BINARY_SENTINEL.to_vec();
        want.extend_from_slice(&[0, 0, b'E', b'O', b'F', 0]);
        want.extend_from_slice(&[70, 0, 0xFE, 0xFF]);
        want.extend_from_slice(&[90, 0, 7, 0, 0, 0]);
        want.extend_from_slice(&[0x18, 1, 1, 0]);
        want.extend_from_slice(&[0x22, 1, 1]);
        want.extend_from_slice(&[40, 0]);
        want.extend_from_slice(&1.5f64.to_le_bytes());
        want.extend_from_slice(&[0x4A, 1, b'1', b'F', 0]);
        want.extend_from_slice(&[0x36, 1, 2, 0xAB, 0xCD]);
        assert_eq!(b, want);
    }
}

//! Typed access to a drawing's header variables, for the writers.
//!
//! The reader keeps every variable it read as display text, in file order
//! (`HeaderVars::raw`), and the variables the core uses as typed fields.
//! A writer asks here by DWG variable name: the typed field wins when the
//! drawing has it (an edit may have changed it), then the stored text,
//! then the caller's default. Drawings started in this program have no
//! stored text, so every lookup has a default.

use super::version::DwgVersion;
use crate::geom::{Vec2, Vec3};
use crate::model::{Color, Drawing, Handle};
use std::collections::HashMap;

pub(crate) struct Hv<'a> {
    d: &'a Drawing,
    raw: HashMap<&'a str, &'a str>,
}

/// Undo the `{:?}` quoting the reader stores strings with.
fn unquote(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next()? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '0' => out.push('\0'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            'u' => {
                if it.next()? != '{' {
                    return None;
                }
                let mut hex = String::new();
                for c in it.by_ref() {
                    if c == '}' {
                        break;
                    }
                    hex.push(c);
                }
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            _ => return None,
        }
    }
    Some(out)
}

fn floats(s: &str) -> Option<Vec<f64>> {
    let inner = s.trim().strip_prefix('(')?.strip_suffix(')')?;
    inner.split(',').map(|p| p.trim().parse::<f64>().ok()).collect()
}

impl<'a> Hv<'a> {
    pub fn new(d: &'a Drawing) -> Self {
        let mut raw: HashMap<&'a str, &'a str> = d
            .header
            .raw
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        // Readers before 0.2.1 skipped MENUNAME in R2007+ files, so every
        // string variable after the four unknown ones carries the value of
        // the one before it, and the last string was not read.
        if d.version >= DwgVersion::R2007 && !raw.contains_key("MENUNAME") && raw.contains_key("DIMPOST") {
            let names: Vec<&'static str> = super::header::vars()
                .filter(|(n, k, applies)| {
                    *k == super::header::VarKind::Tv
                        && applies(d.version, 0)
                        && !n.starts_with("UNKNOWN_TV")
                        && *n != "MENUNAME"
                })
                .map(|(n, _, _)| n)
                .collect();
            let values: Vec<&'a str> = names.iter().map(|n| raw.get(n).copied().unwrap_or("\"\"")).collect();
            raw.insert("MENUNAME", values[0]);
            for (i, n) in names.iter().enumerate() {
                raw.insert(n, values.get(i + 1).copied().unwrap_or("\"\""));
            }
        }
        Hv { d, raw }
    }

    fn raw(&self, name: &str) -> Option<&str> {
        self.raw.get(name).copied()
    }

    pub fn real(&self, name: &str, default: f64) -> f64 {
        let h = &self.d.header;
        let typed = match name {
            "LTSCALE" => h.ltscale,
            "TEXTSIZE" => h.textsize,
            "CELTSCALE" => h.celtscale,
            "ANGBASE" => h.angbase,
            _ => None,
        };
        typed
            .or_else(|| self.raw(name)?.parse().ok())
            .unwrap_or(default)
    }

    pub fn int(&self, name: &str, default: i64) -> i64 {
        let h = &self.d.header;
        let typed = match name {
            "INSUNITS" => h.insunits,
            "MEASUREMENT" => h.measurement,
            "LUNITS" => h.lunits,
            "LUPREC" => h.luprec,
            "AUNITS" => h.aunits,
            "AUPREC" => h.auprec,
            _ => None,
        };
        if let Some(v) = typed {
            return v as i64;
        }
        match self.raw(name) {
            Some("true") => 1,
            Some("false") => 0,
            Some(t) => t.parse().unwrap_or(default),
            None => default,
        }
    }

    pub fn bool(&self, name: &str, default: bool) -> bool {
        if name == "ANGDIR" {
            if let Some(v) = self.d.header.angdir {
                return v != 0;
            }
        }
        match self.raw(name) {
            Some("true") => true,
            Some("false") => false,
            Some(t) => t.parse::<i64>().map(|v| v != 0).unwrap_or(default),
            None => default,
        }
    }

    pub fn p2(&self, name: &str, default: Vec2) -> Vec2 {
        let h = &self.d.header;
        let typed = match name {
            "LIMMIN" => h.limmin,
            "LIMMAX" => h.limmax,
            _ => None,
        };
        typed
            .or_else(|| {
                let v = floats(self.raw(name)?)?;
                (v.len() == 2).then(|| Vec2::new(v[0], v[1]))
            })
            .unwrap_or(default)
    }

    pub fn p3(&self, name: &str, default: Vec3) -> Vec3 {
        let h = &self.d.header;
        let typed = match name {
            "EXTMIN" => h.extmin,
            "EXTMAX" => h.extmax,
            "INSBASE" => h.insbase,
            _ => None,
        };
        typed
            .or_else(|| {
                let v = floats(self.raw(name)?)?;
                (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
            })
            .unwrap_or(default)
    }

    pub fn text(&self, name: &str, default: &str) -> String {
        if name == "HYPERLINKBASE" && !self.d.header.properties.hyperlink_base.is_empty() {
            return self.d.header.properties.hyperlink_base.clone();
        }
        self.raw(name)
            .and_then(unquote)
            .unwrap_or_else(|| default.to_string())
    }

    /// A handle variable; NULL when unknown.
    pub fn handle(&self, name: &str) -> Handle {
        let h = &self.d.header;
        let typed = match name {
            "CLAYER" => h.clayer,
            "TEXTSTYLE" => h.textstyle,
            "CELTYPE" => h.celtype,
            "DIMSTYLE" => h.dimstyle,
            "BLOCK_CONTROL" => h.block_control,
            "LAYER_CONTROL" => h.layer_control,
            "STYLE_CONTROL" => h.style_control,
            "LTYPE_CONTROL" => h.linetype_control,
            "DIMSTYLE_CONTROL" => h.dimstyle_control,
            "BLOCK_RECORD_MODEL_SPACE" => self.d.model_space(),
            "BLOCK_RECORD_PAPER_SPACE" => self.d.paper_space(),
            "DICTIONARY_NAMED_OBJECTS" => h.named_objects_dict,
            _ => Handle::NULL,
        };
        if !typed.is_null() {
            return typed;
        }
        self.raw(name)
            .and_then(|t| t.strip_prefix('#'))
            .and_then(|t| u64::from_str_radix(t, 16).ok())
            .map(Handle)
            .unwrap_or(Handle::NULL)
    }

    /// A CmColor variable. Files the reader saved before it kept these
    /// whole stored only the index field, which reads 0 for every colour
    /// from R2004 on; such a value is unknown and gives `default`.
    pub fn color(&self, name: &str, default: Color) -> Color {
        match self.raw(name) {
            Some("BYLAYER") => Color::ByLayer,
            Some("BYBLOCK") => Color::ByBlock,
            Some("NONE") => Color::None,
            Some(t) if t.starts_with("RGB(") => {
                let v: Vec<u8> = t[4..t.len() - 1]
                    .split(',')
                    .filter_map(|p| p.trim().parse().ok())
                    .collect();
                if v.len() == 3 {
                    Color::Rgb(v[0], v[1], v[2])
                } else {
                    default
                }
            }
            Some(t) => match t.parse::<i16>() {
                Ok(0) if self.d.version >= DwgVersion::R2004 => default,
                Ok(i) => Color::from_aci(i),
                Err(_) => default,
            },
            None => default,
        }
    }

    /// A date: Julian day plus fraction.
    pub fn time(&self, name: &str, default: f64) -> f64 {
        self.raw(name)
            .and_then(|t| t.parse().ok())
            .unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_text_reads_back() {
        assert_eq!(unquote("\"m\"").as_deref(), Some("m"));
        assert_eq!(unquote("\"\"").as_deref(), Some(""));
        assert_eq!(unquote(r#""a\"b\\c\u{b0}""#).as_deref(), Some("a\"b\\c°"));
        assert_eq!(floats("(1, -2.5, 1e20)"), Some(vec![1.0, -2.5, 1e20]));
        let mut d = crate::standards::new_drawing(crate::standards::Template::Imperial);
        d.version = DwgVersion::R2018;
        d.header.raw = vec![
            ("CECOLOR".into(), "0".into()),
            ("DIMCLRD".into(), "BYBLOCK".into()),
            ("INTERFERECOLOR".into(), "RGB(255,0,0)".into()),
            ("PUCSNAME".into(), "#3D0B5".into()),
            ("MENUNAME".into(), "\"acad\"".into()),
            ("DIMPOST".into(), "\".\"".into()),
            ("EXTMIN".into(), "(1, 2, 3)".into()),
        ];
        let hv = Hv::new(&d);
        assert_eq!(hv.color("CECOLOR", Color::ByLayer), Color::ByLayer);
        assert_eq!(hv.color("DIMCLRD", Color::Index(7)), Color::ByBlock);
        assert_eq!(hv.color("INTERFERECOLOR", Color::ByLayer), Color::Rgb(255, 0, 0));
        assert_eq!(hv.handle("PUCSNAME"), Handle(0x3D0B5));
        assert_eq!(hv.text("DIMPOST", ""), ".");
        assert_eq!(hv.int("MISSING", 42), 42);
    }

    /// A header saved by a reader that skipped MENUNAME reads back with
    /// every string where it belongs.
    #[test]
    fn strings_read_one_place_early_are_put_back() {
        let mut d = crate::standards::new_drawing(crate::standards::Template::Imperial);
        d.version = DwgVersion::R2018;
        let q = |s: &str| format!("{s:?}");
        d.header.raw = vec![
            ("DIMPOST".into(), q(".")),
            ("DIMAPOST".into(), q("")),
            ("DIMALTMZS".into(), q("")),
            ("DIMMZS".into(), q("")),
            ("HYPERLINKBASE".into(), q("")),
            ("STYLESHEET".into(), q("")),
            ("FINGERPRINTGUID".into(), q("")),
            ("VERSIONGUID".into(), q("{FP}")),
            ("PROJECTNAME".into(), q("{VG}")),
        ];
        let hv = Hv::new(&d);
        assert_eq!(hv.text("MENUNAME", "?"), ".");
        assert_eq!(hv.text("DIMPOST", "?"), "");
        assert_eq!(hv.text("FINGERPRINTGUID", "?"), "{FP}");
        assert_eq!(hv.text("VERSIONGUID", "?"), "{VG}");
        assert_eq!(hv.text("PROJECTNAME", "?"), "");
    }
}

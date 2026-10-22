//! `cad` — command-line test build for the CAD core.
//!
//! No third-party crates: arguments are parsed by hand.

#![forbid(unsafe_code)]

use cad_core::export::{dxf, svg};
use cad_core::model::{units_name, Color, Drawing, Handle, ObjectKind, Severity};
use cad_core::native::{self, lock};
use cad_core::units::{self, LengthFormat, LinearFormat, Unit};
use cad_core::{raster, render, text};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
cad — CAD core test build

USAGE:
    cad info    <file>                      Version, sections, object counts, diagnostics
    cad objects <file> [--type NAME] [--failed]
                                            List objects (handle, type, layer, summary)
    cad dump    <file> <handle>             Print one decoded object (handle in hex)
    cad header  <file>                      Print header variables as read
    cad svg     <file> <out.svg> [--light [--color]]
                                            Render model space to SVG
    cad png     <file> <out.png> [--width N] [--height N] [--light [--color]] [--no-grid]
                                            Draw model space as the canvas shows it,
                                            or (--light) as it prints: black on
                                            white, or in colour with --color
    cad dxf     <file> <out.dxf> [--binary] Write the drawing as an AutoCAD 2018 DXF
                                            (ASCII, or binary with --binary)
    cad dxf     <file> <out.dxf> --r12      Export model space to R12 DXF (blocks exploded)
    cad dwg     <file> <out.dwg>            Write the drawing as an AutoCAD 2018 .dwg file
    cad layers  <file>                      Layers: colour, linetype, lineweight, state

  Drawings and units:
    cad new     <out.blk> [--imperial]      Start a drawing from the Metric (mm) or
                                            Imperial (in) template
    cad sample  <out.blk>                   Save the sample plate drawing
    cad length  <text> [--unit U] [--format F] [--precision N]
                                            Read a typed length (10mm, 3/8\", 1'-2 1/2\")
                                            in a drawing of unit U (mm, in, ft, m …;
                                            default mm) and show it in format F
                                            (decimal, fractional, architectural,
                                            engineering, scientific)

  Native format (.blk):
    cad save    <file> <out.blk> [--no-preview] [--no-compress] [--no-recovery]
                [--level fast|default|best] Save any drawing as native .blk
    cad props   <file.blk> [--set NAME=VALUE]...
                                            Show the drawing properties, or change
                                            them (title, subject, author, keywords,
                                            comments, last-saved-by, revision,
                                            hyperlink-base, or any custom name; an
                                            empty value removes a custom one),
                                            saved as a change set
    cad part    <file.blk> [--block NAME] [--set NAME=VALUE]...
                                            Show or set what a block (default: model
                                            space) is made of: material, grade,
                                            thickness (drawing units), quantity
    cad takeoff <file> [--block NAME] [--layers A,B]
                                            Length, contours, holes, net area and
                                            weight of the part (default layer
                                            VISIBLE)
    cad verify  <file>                      Save to .blk in memory, read back, compare
    cad compact <file.blk>                  Merge change sets into a fresh snapshot
    cad inspect <file.blk> [--tags N]       Byte map and structure checks (audit);
                                            --tags N also prints the tags of the
                                            first N records of each section
    cad preview <file.blk> <out.svg>        Extract the stored preview
    cad schema  [file.blk]                  The record schema as a Markdown table:
                                            this build's, or the one a file carries
    cad presence <file>                     Who has the drawing open (.bdl lock)

    cad version                             Print version
    cad help                                This text

<file> is a native .blk or a DWG file; the format is detected from its
contents. DWG: R13–R2000 (AC1012–AC1015) and R2004–R2018+ (AC1018, AC1024,
AC1027, AC1032). AC1021 (2007–2009) is not read yet.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match run(cmd, &args[1..]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// One command's arguments: its values in order, and the options given.
struct Args<'a> {
    values: Vec<&'a str>,
    options: Vec<(&'a str, Option<&'a str>)>,
}

impl<'a> Args<'a> {
    /// Split `rest` by what the command accepts: `switches` stand alone,
    /// `valued` options take the next argument, and at most `max` values.
    /// Anything else starting with `--` is refused, so a misplaced or
    /// mistyped option is never taken for a file name.
    fn parse(
        rest: &'a [String],
        switches: &[&str],
        valued: &[&str],
        max: usize,
    ) -> Result<Args<'a>, String> {
        let mut a = Args {
            values: Vec::new(),
            options: Vec::new(),
        };
        let mut it = rest.iter().map(String::as_str);
        while let Some(arg) = it.next() {
            if !arg.starts_with("--") {
                if a.values.len() == max {
                    return Err(format!("unexpected argument {arg:?}"));
                }
                a.values.push(arg);
            } else if valued.contains(&arg) {
                let v = it.next().ok_or_else(|| format!("{arg} needs a value"))?;
                a.options.push((arg, Some(v)));
            } else if switches.contains(&arg) {
                a.options.push((arg, None));
            } else {
                return Err(format!("unknown option {arg:?}"));
            }
        }
        Ok(a)
    }

    fn value(&self, i: usize, what: &str) -> Result<&'a str, String> {
        self.values
            .get(i)
            .copied()
            .ok_or_else(|| format!("missing {what}"))
    }

    fn has(&self, name: &str) -> bool {
        self.options.iter().any(|(n, _)| *n == name)
    }

    /// Every value given for a repeatable option, in order.
    fn all(&self, name: &str) -> Vec<&'a str> {
        self.options
            .iter()
            .filter(|(n, _)| *n == name)
            .filter_map(|(_, v)| *v)
            .collect()
    }

    /// The value of an option given with one (the last, if repeated).
    fn option(&self, name: &str) -> Option<&'a str> {
        self.options
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .and_then(|(_, v)| *v)
    }
}

/// Mark the drawing as saved now by the person running the command, as a
/// CAD program does when it writes a file.
fn stamp(d: &mut cad_core::Drawing) {
    let user = ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_secs() as i64)
        .unwrap_or(0);
    d.stamp_save(&user, now);
}

fn open(path: &str) -> Result<Drawing, String> {
    cad_core::open(path).map_err(|e| e.to_string())
}

fn run(cmd: &str, rest: &[String]) -> Result<(), String> {
    let args =
        |switches: &[&str], valued: &[&str], max: usize| Args::parse(rest, switches, valued, max);
    match cmd {
        "info" => {
            let a = args(&[], &[], 1)?;
            let path = a.value(0, "input file")?;
            info(&open(path)?, path);
            Ok(())
        }
        "objects" => {
            let a = args(&["--failed"], &["--type"], 1)?;
            let d = open(a.value(0, "input file")?)?;
            objects(&d, a.option("--type"), a.has("--failed"));
            Ok(())
        }
        "dump" => {
            let a = args(&[], &[], 2)?;
            let d = open(a.value(0, "input file")?)?;
            dump(&d, a.values.get(1).copied())
        }
        "header" => {
            let a = args(&[], &[], 1)?;
            header(&open(a.value(0, "input file")?)?);
            Ok(())
        }
        "svg" => {
            let a = args(&["--light", "--color"], &[], 2)?;
            let d = open(a.value(0, "input file")?)?;
            let out = a.value(1, "output path")?;
            let scene = render::model_space(&d);
            let doc = svg::to_svg(
                &scene,
                &svg::SvgOptions {
                    dark_background: !a.has("--light"),
                    paper_color: a.has("--color"),
                    ..Default::default()
                },
            );
            std::fs::write(out, doc).map_err(|e| format!("{out}: {e}"))?;
            report_scene(&scene);
            println!("wrote {out}");
            Ok(())
        }
        "png" => {
            let a = args(
                &["--light", "--color", "--no-grid"],
                &["--width", "--height"],
                2,
            )?;
            png(&open(a.value(0, "input file")?)?, &a)
        }
        "layers" => {
            let a = args(&[], &[], 1)?;
            layers(&open(a.value(0, "input file")?)?);
            Ok(())
        }
        "new" => {
            let a = args(&["--imperial"], &[], 1)?;
            let out = a.value(0, "output path")?;
            let template = if a.has("--imperial") {
                cad_core::Template::Imperial
            } else {
                cad_core::Template::Metric
            };
            let d = cad_core::new_drawing(template);
            native::write_file(&d, out, &Default::default()).map_err(|e| e.to_string())?;
            println!(
                "wrote {out}: new drawing in {}, layers {}",
                Unit::of(&d),
                d.layers()
                    .iter()
                    .map(|(_, l)| l.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            Ok(())
        }
        "sample" => {
            let a = args(&[], &[], 1)?;
            let out = a.value(0, "output path")?;
            let d = cad_core::samples::plate();
            native::write_file(&d, out, &Default::default()).map_err(|e| e.to_string())?;
            println!("wrote {out}");
            Ok(())
        }
        "length" => length(&args(&[], &["--unit", "--format", "--precision"], 1)?),
        "dxf" => {
            let a = args(&["--binary", "--r12"], &[], 2)?;
            let d = open(a.value(0, "input file")?)?;
            let out = a.value(1, "output path")?;
            if a.has("--r12") {
                if a.has("--binary") {
                    return Err("--r12 writes ASCII only".into());
                }
                let scene = render::model_space(&d);
                std::fs::write(out, dxf::to_dxf(&scene, Some(&d)))
                    .map_err(|e| format!("{out}: {e}"))?;
                report_scene(&scene);
                println!("wrote {out} (R12, blocks exploded)");
                return Ok(());
            }
            let format = if a.has("--binary") {
                cad_core::dxf::DxfFormat::Binary
            } else {
                cad_core::dxf::DxfFormat::Ascii
            };
            let r = cad_core::dxf::to_bytes(&d, format);
            std::fs::write(out, &r.bytes).map_err(|e| format!("{out}: {e}"))?;
            for n in &r.notes {
                println!("note: {n}");
            }
            println!(
                "wrote {out} ({} DXF, AutoCAD 2018 format, {} bytes)",
                if a.has("--binary") { "binary" } else { "ASCII" },
                r.bytes.len()
            );
            Ok(())
        }
        "dwg" => {
            let a = args(&[], &[], 2)?;
            let mut d = open(a.value(0, "input file")?)?;
            stamp(&mut d);
            let out = a.value(1, "output path")?;
            let r = cad_core::dwg::write::to_bytes(&d).map_err(|e| e.to_string())?;
            std::fs::write(out, &r.bytes).map_err(|e| format!("{out}: {e}"))?;
            for n in &r.notes {
                println!("note: {n}");
            }
            println!("wrote {out} (AutoCAD 2018 format, {} bytes)", r.bytes.len());
            Ok(())
        }
        "save" => {
            let a = args(
                &["--no-preview", "--no-compress", "--no-recovery"],
                &["--level"],
                2,
            )?;
            let d = open(a.value(0, "input file")?)?;
            let out = a.value(1, "output path")?;
            let level = match a.option("--level") {
                None | Some("default") => native::Level::Default,
                Some("fast") => native::Level::Fast,
                Some("best") => native::Level::Best,
                Some(x) => return Err(format!("--level is fast, default or best, not {x:?}")),
            };
            let opts = native::WriteOptions {
                preview: !a.has("--no-preview"),
                compress: !a.has("--no-compress"),
                recovery: !a.has("--no-recovery"),
                level,
                ..Default::default()
            };
            let generation = native::write_file(&d, out, &opts).map_err(|e| e.to_string())?;
            let size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
            println!("wrote {out} ({size} bytes, generation {generation})");
            Ok(())
        }
        "part" => {
            let a = args(&[], &["--set", "--block"], 1)?;
            part(
                a.value(0, "input file")?,
                a.option("--block"),
                &a.all("--set"),
            )
        }
        "takeoff" => {
            let a = args(&[], &["--block", "--layers"], 1)?;
            takeoff(
                &open(a.value(0, "input file")?)?,
                a.option("--block"),
                a.option("--layers"),
            )
        }
        "props" => {
            let a = args(&[], &["--set"], 1)?;
            props(a.value(0, "input file")?, &a.all("--set"))
        }
        "verify" => {
            let a = args(&[], &[], 1)?;
            let path = a.value(0, "input file")?;
            verify(&open(path)?, path)
        }
        "compact" => {
            let a = args(&[], &[], 1)?;
            let path = a.value(0, "input file")?;
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            if !native::is_native(&bytes) {
                return Err("compact works on native .blk files".into());
            }
            let generation = native::write_file(&open(path)?, path, &Default::default())
                .map_err(|e| e.to_string())?;
            println!("compacted {path} (generation {generation})");
            Ok(())
        }
        "inspect" => {
            let a = args(&[], &["--tags"], 1)?;
            let tags = match a.option("--tags") {
                Some(n) => n
                    .parse()
                    .map_err(|_| format!("--tags needs a number, not {n:?}"))?,
                None => 0,
            };
            inspect(a.value(0, "input file")?, tags)
        }
        "schema" => {
            let a = args(&[], &[], 1)?;
            match a.values.first() {
                None => print!("{}", native::schema::markdown_table()),
                Some(path) => {
                    let f = native::NativeFile::open(path).map_err(|e| e.to_string())?;
                    let kinds = f.schema().map_err(|e| e.to_string())?;
                    if kinds.is_empty() {
                        return Err(format!(
                            "{path} carries no schema (format {}.{})",
                            f.format().0,
                            f.format().1
                        ));
                    }
                    println!("| Kind | Name | Fields, in order |\n|---|---|---|");
                    for k in kinds {
                        let fields: Vec<String> = k
                            .fields
                            .iter()
                            .map(|x| format!("{} {}", x.code, x.name))
                            .collect();
                        println!("| {} | {} | {} |", k.kind, k.name, fields.join("; "));
                    }
                }
            }
            Ok(())
        }
        "preview" => {
            let a = args(&[], &[], 2)?;
            let path = a.value(0, "input file")?;
            let out = a.value(1, "output path")?;
            let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            match native::preview_svg(&bytes).map_err(|e| e.to_string())? {
                Some(svg) => {
                    std::fs::write(out, svg).map_err(|e| format!("{out}: {e}"))?;
                    println!("wrote {out}");
                    Ok(())
                }
                None => Err(format!("{path} has no preview")),
            }
        }
        "presence" => {
            let a = args(&[], &[], 1)?;
            let path = a.value(0, "input file")?;
            match lock::presence(path).map_err(|e| e.to_string())? {
                None => println!(
                    "not open anywhere (no {})",
                    lock::lock_path(Path::new(path)).display()
                ),
                Some(i) => {
                    let now = lock::now_unix();
                    println!(
                        "open by {}@{} (pid {}, {}), since {}s ago, last heartbeat {}s ago{}",
                        i.user,
                        i.host,
                        i.pid,
                        i.app,
                        now.saturating_sub(i.opened),
                        now.saturating_sub(i.heartbeat),
                        if i.is_stale(now) {
                            " — STALE, can be taken over"
                        } else {
                            ""
                        }
                    );
                }
            }
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!(
                "{} {} (cad CLI {}; .blk format {}.{})",
                cad_core::PRODUCT_NAME,
                cad_core::VERSION,
                env!("CARGO_PKG_VERSION"),
                cad_core::native::FORMAT_MAJOR,
                cad_core::native::FORMAT_MINOR
            );
            Ok(())
        }
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

fn info(d: &Drawing, path: &str) {
    let size = std::fs::metadata(Path::new(path))
        .map(|m| m.len())
        .unwrap_or(0);
    println!("File:        {path} ({size} bytes)");
    let native_info = std::fs::read(path)
        .ok()
        .filter(|b| native::is_native(b))
        .and_then(|b| native::inspect(&b).ok());
    if let Some(n) = &native_info {
        println!(
            "Format:      native .blk {}.{}, generation {}, {} change sets, written by {}",
            n.format.0, n.format.1, n.generation, n.change_sets, n.writer
        );
        println!("Source:      {}", d.version);
    } else {
        println!("Version:     {}", d.version);
    }
    println!("Maintenance: {}", d.maintenance_version);
    println!(
        "Code page:   {} ({})",
        d.codepage,
        text::codepage_name(d.codepage)
    );
    println!();
    println!("Sections:");
    if let Some(n) = &native_info {
        for s in &n.sections {
            println!(
                "  {:<24} {:>10} bytes  stored {:>10}  {}{}{}",
                s.name,
                s.raw,
                s.stored,
                s.codec,
                if s.split { ", split" } else { "" },
                if s.crc_ok { "" } else { "  CRC MISMATCH" }
            );
        }
        println!();
        if n.schema_kinds > 0 {
            println!(
                "Schema:      {} record kinds described in the file",
                n.schema_kinds
            );
        }
        if let Some(head) = &n.chain_head {
            println!("Hash chain:  SHA-256, head {head}");
        }
        if let Some(r) = &n.recovery {
            println!(
                "Recovery:    {} bytes covered in blocks of {}, {} stripes{}",
                r.covered,
                r.block,
                r.stripes,
                if r.damaged == 0 {
                    ", every block intact".to_string()
                } else {
                    format!(
                        "; {} blocks DAMAGED, {} beyond repair",
                        r.damaged, r.unrepairable
                    )
                }
            );
        }
    }
    for s in d.sections.iter().filter(|_| native_info.is_none()) {
        println!(
            "  {:<24} {:>10} bytes  pages {:<3}{}{}",
            s.name,
            s.size,
            s.pages,
            if s.compressed { " compressed" } else { "" },
            if s.encrypted { " ENCRYPTED" } else { "" }
        );
    }
    println!();
    let total = d.objects.len();
    let failed = d
        .objects
        .values()
        .filter(|o| matches!(o.kind, ObjectKind::Failed(_) | ObjectKind::Damaged(_)))
        .count();
    let unsupported = d
        .objects
        .values()
        .filter(|o| matches!(o.kind, ObjectKind::Unsupported | ObjectKind::NewerKind(_)))
        .count();
    println!("Classes:     {}", d.classes.len());
    println!(
        "Objects:     {total} ({} decoded, {unsupported} not decoded yet, {failed} failed)",
        total - failed - unsupported
    );
    let mut apps: std::collections::BTreeMap<String, usize> = Default::default();
    let mut carriers = 0usize;
    for o in d.objects.values().filter(|o| !o.xdata.is_empty()) {
        carriers += 1;
        for x in &o.xdata {
            let name = x.app_name().unwrap_or("(missing APPID)").to_string();
            *apps.entry(name).or_default() += 1;
        }
    }
    if carriers > 0 {
        let list: Vec<String> = apps.iter().map(|(n, k)| format!("{n} ×{k}")).collect();
        println!("XDATA:       {carriers} objects; {}", list.join(", "));
    }
    let h = &d.header;
    if !h.properties.is_empty() {
        print_properties(&h.properties, "  ");
    }
    println!(
        "Header:      {}",
        if h.verified {
            "verified against object map".to_string()
        } else {
            format!("NOT verified — {}", h.note)
        }
    );
    match h.insunits {
        Some(u) if h.verified => println!("Units:       {u} ({})", units_name(u)),
        _ => println!("Units:       unknown"),
    }
    if let (Some(a), Some(b)) = (h.extmin, h.extmax) {
        if h.verified {
            println!(
                "Extents:     ({:.4}, {:.4}) – ({:.4}, {:.4})",
                a.x, a.y, b.x, b.y
            );
        }
    }
    let ms = d.model_space();
    println!(
        "Model space: block record {ms}, {} entities",
        d.model_space_entities().len()
    );
    println!();
    println!(
        "{:<28} {:>9} {:>9} {:>9}",
        "Type", "decoded", "pending", "failed"
    );
    for (name, (ok, pending, bad)) in d.type_counts() {
        println!("{name:<28} {ok:>9} {pending:>9} {bad:>9}");
    }
    if !d.diagnostics.is_empty() {
        println!();
        println!("Diagnostics:");
        for diag in &d.diagnostics {
            let tag = match diag.severity {
                Severity::Info => "info",
                Severity::Warning => "warn",
                Severity::Error => "ERROR",
            };
            println!("  [{tag}] {}", diag.message);
        }
    }
}

/// Civil date and time (UTC) of Unix seconds, `YYYY-MM-DD hh:mm:ss`.
fn utc(t: i64) -> String {
    // Days to civil date, after Howard Hinnant's public-domain algorithm.
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

fn print_properties(p: &cad_core::model::Properties, indent: &str) {
    println!("Properties:");
    for (k, v) in p.fields() {
        if !v.is_empty() {
            println!("{indent}{k:<16} {v}");
        }
    }
    for (k, t) in [("created", p.created), ("modified", p.modified)] {
        if t != 0 {
            println!("{indent}{k:<16} {}", utc(t));
        }
    }
    for (k, v) in &p.custom {
        println!("{indent}{k:<16} {v}  (custom)");
    }
}

fn block_handle(d: &Drawing, block: Option<&str>) -> Result<Handle, String> {
    match block {
        None => Ok(d.model_space()),
        Some(name) => d
            .block_by_name(name)
            .map(|(h, _)| h)
            .ok_or_else(|| format!("no block named {name:?}")),
    }
}

fn print_part(p: &cad_core::model::Part, unit: Unit) {
    let show = |k: &str, v: String| println!("  {k:<10} {v}");
    if p.is_empty() {
        println!("  (no part data)");
        return;
    }
    if !p.material.is_empty() {
        show("material", p.material.clone());
    }
    if !p.grade.is_empty() {
        show("grade", p.grade.clone());
    }
    if p.thickness > 0.0 {
        show("thickness", format!("{} {}", p.thickness, unit.symbol()));
    }
    if p.quantity > 0 {
        show("quantity", p.quantity.to_string());
    }
}

/// Show or set the part data of a block record, saved as a change set.
fn part(path: &str, block: Option<&str>, sets: &[&str]) -> Result<(), String> {
    let d = open(path)?;
    let h = block_handle(&d, block)?;
    let mut o = d
        .get(h)
        .cloned()
        .ok_or_else(|| "the drawing has no model space record".to_string())?;
    let ObjectKind::BlockHeader(b) = &mut o.kind else {
        return Err(format!("{h} is not a block record"));
    };
    if sets.is_empty() {
        println!("Part {}:", b.name);
        print_part(&b.part, Unit::of(&d));
        return Ok(());
    }
    if !native::is_native(&std::fs::read(path).map_err(|e| format!("{path}: {e}"))?) {
        return Err("part data is saved into native .blk files; `cad save` one first".into());
    }
    for s in sets {
        let (k, v) = s
            .split_once('=')
            .ok_or_else(|| format!("--set takes NAME=VALUE, not {s:?}"))?;
        let v = v.trim();
        match k.trim() {
            "material" => b.part.material = v.into(),
            "grade" => b.part.grade = v.into(),
            "thickness" => {
                b.part.thickness = if v.is_empty() {
                    0.0
                } else {
                    units::parse_length(v, Unit::of(&d)).map_err(|e| format!("thickness: {e}"))?
                }
            }
            "quantity" => {
                b.part.quantity = if v.is_empty() {
                    0
                } else {
                    v.parse()
                        .map_err(|_| format!("quantity is a whole number, not {v:?}"))?
                }
            }
            other => {
                return Err(format!(
                    "unknown part field {other:?} (material, grade, thickness, quantity)"
                ))
            }
        }
    }
    let name = b.name.clone();
    let g = native::append_changes(
        path,
        &native::ChangeSet {
            author: "cad part".into(),
            time: 0,
            changes: vec![native::Change::Put(Box::new(o))],
        },
    )
    .map_err(|e| e.to_string())?;
    println!("saved the part data of {name} to {path} as change set {g}");
    Ok(())
}

/// Measure a part for a quote.
fn takeoff(d: &Drawing, block: Option<&str>, layers: Option<&str>) -> Result<(), String> {
    let opts = cad_core::takeoff::Options {
        block: match block {
            None => Handle::NULL,
            Some(_) => block_handle(d, block)?,
        },
        layers: layers
            .map(|l| l.split(',').map(|x| x.trim().to_string()).collect())
            .unwrap_or_default(),
    };
    let t = cad_core::takeoff::takeoff(d, &opts);
    // A drawing with no units still measures, in its own units.
    let u = match t.unit.symbol() {
        "" => "units",
        s => s,
    };
    println!("Part:        {}", t.block_name);
    print_part(&t.part, t.unit);
    println!(
        "Layers:      {}",
        if t.layers.is_empty() {
            "all".to_string()
        } else {
            t.layers.join(", ")
        }
    );
    println!("Measured:    {} curves", t.measured);
    if !t.skipped.is_empty() {
        let s: Vec<String> = t.skipped.iter().map(|(k, n)| format!("{k} ×{n}")).collect();
        println!("Not measured: {}", s.join(", "));
    }
    println!("Length:      {:.3} {u}", t.length);
    if t.duplicates > 0 {
        println!(
            "Set aside:   {} repeated or overlapping curve(s), {:.3} {u}, not counted \
             (drawn twice they would be cut twice; OVERKILL removes them)",
            t.duplicates, t.duplicate_length
        );
    }
    println!(
        "Contours:    {} closed ({} holes), {} open paths",
        t.contours, t.holes, t.open_paths
    );
    let outlines = t.contours - t.holes;
    if outlines > 1 {
        println!(
            "             {outlines} separate outlines: more than one part, or a second view \
             of the same part, is being measured (choose --layers or --block)"
        );
    }
    println!("Net area:    {:.3} {u}²", t.area);
    if !t.bbox.is_empty() {
        println!(
            "Extents:     {:.3} × {:.3} {u}",
            t.bbox.width(),
            t.bbox.height()
        );
    }
    match (t.weight_kg, t.density) {
        (Some(kg), Some(rho)) => println!(
            "Weight:      {kg:.3} kg each (nominal {rho} kg/m³){}",
            if t.part.quantity > 1 {
                format!(
                    ", {:.3} kg for {}",
                    kg * t.part.quantity as f64,
                    t.part.quantity
                )
            } else {
                String::new()
            }
        ),
        _ => println!(
            "Weight:      needs a known material, a thickness and drawing units (`cad part`)"
        ),
    }
    Ok(())
}

/// Show or change the drawing properties of a native file.
fn props(path: &str, sets: &[&str]) -> Result<(), String> {
    let f = native::NativeFile::open(path).map_err(|e| e.to_string())?;
    let mut h = f.header().map_err(|e| e.to_string())?;
    if sets.is_empty() {
        if h.properties.is_empty() {
            println!("{path}: no drawing properties set");
        } else {
            print_properties(&h.properties, "  ");
        }
        return Ok(());
    }
    drop(f);
    for s in sets {
        let (k, v) = s
            .split_once('=')
            .ok_or_else(|| format!("--set takes NAME=VALUE, not {s:?}"))?;
        let k = k.trim();
        if k.is_empty() {
            return Err(format!("--set {s:?}: the name is empty"));
        }
        h.properties.set(k, v);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    h.properties.modified = now;
    if h.properties.created == 0 {
        h.properties.created = now;
    }
    let g = native::append_changes(
        path,
        &native::ChangeSet {
            author: "cad props".into(),
            time: 0,
            changes: vec![native::Change::Header(Box::new(h))],
        },
    )
    .map_err(|e| e.to_string())?;
    println!("saved the properties to {path} as change set {g}");
    Ok(())
}

fn summary(d: &Drawing, o: &cad_core::model::Object) -> String {
    use ObjectKind as K;
    match &o.kind {
        K::Line(l) => format!(
            "({:.4}, {:.4}) → ({:.4}, {:.4})",
            l.start.x, l.start.y, l.end.x, l.end.y
        ),
        K::Circle(c) => format!(
            "center ({:.4}, {:.4}) r {:.4}",
            c.center.x, c.center.y, c.radius
        ),
        K::Arc(a) => format!(
            "center ({:.4}, {:.4}) r {:.4} {:.2}°–{:.2}°",
            a.center.x,
            a.center.y,
            a.radius,
            a.start_angle.to_degrees(),
            a.end_angle.to_degrees()
        ),
        K::LwPolyline(p) => format!(
            "{} vertices{}",
            p.points.len(),
            if p.is_closed() { ", closed" } else { "" }
        ),
        K::Polyline(p) => format!("{:?}, {} vertices", p.kind, d.polyline_vertices(o).len()),
        K::Text(t) => format!("{:?} h {:.4}", t.value, t.height),
        K::MText(m) => format!("{:?}", text::strip_mtext(&m.contents)),
        K::Attribute(a) => format!("{}={:?}", a.tag, a.text.value),
        K::Insert(i) => format!(
            "block {} at ({:.4}, {:.4})",
            d.block_header(i.block_header)
                .map(|b| b.name.as_str())
                .unwrap_or("?"),
            i.insertion.x,
            i.insertion.y
        ),
        K::BlockHeader(b) => format!("{:?}", b.name),
        K::Layer(l) => format!(
            "{:?}{}{}",
            l.name,
            if l.frozen { " frozen" } else { "" },
            if l.off { " off" } else { "" }
        ),
        K::NamedRecord(r) => format!("{:?}", r.name),
        K::Linetype(l) => format!("{:?} {:?}, pattern {:?}", l.name, l.description, l.dashes()),
        K::TextStyle(s) => format!("{:?} font {:?}", s.name, s.font_file),
        K::DimStyle(s) => format!(
            "{:?} text {} arrows {} decimals {}",
            s.name, s.text_height, s.arrow_size, s.decimals
        ),
        K::Control(c) => format!("{} entries", c.entries.len()),
        K::Dimension(dm) => format!("{:?} measurement {:.4}", dm.kind, dm.measurement),
        K::Hatch(h) => format!(
            "{:?} {} paths{}",
            h.pattern_name,
            h.paths.len(),
            if h.solid_fill { " solid" } else { "" }
        ),
        K::Failed(msg) => format!("FAILED: {msg}"),
        K::Unsupported => "not decoded yet".into(),
        K::NewerKind(k) => format!("kind {k} from a newer version, kept as is"),
        K::Damaged(msg) => format!("DAMAGED, kept as is: {msg}"),
        _ => String::new(),
    }
}

fn objects(d: &Drawing, type_filter: Option<&str>, only_failed: bool) {
    let type_filter = type_filter.map(str::to_uppercase);
    for o in d.objects.values() {
        if let Some(t) = &type_filter {
            if !o.type_name.to_uppercase().contains(t.as_str()) {
                continue;
            }
        }
        if only_failed && !matches!(o.kind, ObjectKind::Failed(_) | ObjectKind::Damaged(_)) {
            continue;
        }
        let layer = o
            .entity
            .as_ref()
            .and_then(|e| d.layer(e.layer))
            .map(|l| l.name.clone())
            .unwrap_or_default();
        println!(
            "{:>8}  {:<22} {:<16} {}",
            o.handle.to_string(),
            o.type_name,
            layer,
            summary(d, o)
        );
    }
}

fn dump(d: &Drawing, handle: Option<&str>) -> Result<(), String> {
    let h = handle.ok_or("missing handle (hex)")?;
    let v = u64::from_str_radix(h.trim_start_matches("0x").trim_start_matches('#'), 16)
        .map_err(|_| format!("bad handle {h:?}"))?;
    let o = d
        .get(Handle(v))
        .ok_or_else(|| format!("no object with handle {v:X}"))?;
    println!("{o:#?}");
    Ok(())
}

fn header(d: &Drawing) {
    println!(
        "verified: {}{}",
        d.header.verified,
        if d.header.note.is_empty() {
            String::new()
        } else {
            format!(" ({})", d.header.note)
        }
    );
    for (k, v) in &d.header.raw {
        println!("{k:<32} {v}");
    }
}

/// Byte map, tag statistics and structure checks of a native file.
fn inspect(path: &str, tags: usize) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    if !native::looks_native(&bytes) {
        return Err(format!("{path} is not a native .blk file"));
    }
    let rep = native::audit::audit(&bytes);
    println!("File        {path} ({} bytes)", rep.file_len);
    let copy = |c: &Option<native::audit::HeaderCopy>| match c {
        Some(h) => format!(
            "format {}.{}, generation {}, flags {:#x}, directory at {} ({} bytes)",
            h.major, h.minor, h.generation, h.flags, h.dir_offset, h.dir_len
        ),
        None => "DAMAGED".into(),
    };
    println!("Header      {}", copy(&rep.front));
    println!("Trailer     {}", copy(&rep.trailer));
    if let Some(u) = rep.used {
        println!("Reading     through the {u} copy");
    }
    println!();
    println!("Byte map");
    for g in &rep.regions {
        println!(
            "  {:>10}  {:#010x}  {:>9} bytes  {}",
            g.start, g.start, g.len, g.label
        );
    }
    let sup = rep.superseded_bytes();
    if sup > 0 {
        println!("  ({sup} superseded bytes; `cad compact` reclaims them)");
    }
    let t = &rep.tags;
    if t.tags > 0 {
        println!();
        println!(
            "Object records: {} records, {} tags, {} bytes as .blk tags; the same tags as binary DXF: {} bytes ({:+.1}%)",
            t.records,
            t.tags,
            t.cad_bytes,
            t.binary_dxf_bytes,
            (t.cad_bytes as f64 / t.binary_dxf_bytes.max(1) as f64 - 1.0) * 100.0
        );
        let by: Vec<String> = t.by_type.iter().map(|(k, v)| format!("{k} {v}")).collect();
        println!("  tags by type: {}", by.join(", "));
    }
    println!();
    println!("Checks");
    for c in &rep.checks {
        println!(
            "  {}  {:<22} {}",
            if c.ok { "PASS" } else { "FAIL" },
            c.name,
            c.detail
        );
    }
    if tags > 0 {
        let info = native::inspect(&bytes).map_err(|e| e.to_string())?;
        for s in &info.sections {
            if s.name == "ObjectMap" || s.name == "Preview" {
                continue;
            }
            match native::audit::dump_section(&bytes, &s.name, tags) {
                Ok(recs) => {
                    println!();
                    println!("Section {} (first {} records)", s.name, recs.len());
                    for (i, lines) in recs.iter().enumerate() {
                        println!("  record {i}");
                        for l in lines {
                            println!("    {l}");
                        }
                    }
                }
                Err(e) => println!("\nSection {}: {e}", s.name),
            }
        }
    }
    let failed = rep.failed().count();
    println!();
    if failed == 0 {
        println!("Result: all {} checks passed", rep.checks.len());
        Ok(())
    } else {
        Err(format!("{failed} of {} checks failed", rep.checks.len()))
    }
}

/// Save to the native format in memory, read it back and compare.
fn verify(d: &Drawing, path: &str) -> Result<(), String> {
    let t = std::time::Instant::now();
    let bytes = native::to_bytes(d, &native::WriteOptions::default());
    let write_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = std::time::Instant::now();
    let back = native::read_bytes(&bytes).map_err(|e| format!("read back: {e}"))?;
    let read_ms = t.elapsed().as_secs_f64() * 1000.0;
    let src = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut problems = Vec::new();
    if back.version != d.version
        || back.codepage != d.codepage
        || back.maintenance_version != d.maintenance_version
    {
        problems.push("file properties differ".to_string());
    }
    if back.header != d.header {
        problems.push("header variables differ".into());
    }
    if back.classes != d.classes {
        problems.push("classes differ".into());
    }
    for (h, o) in &d.objects {
        match back.objects.get(h) {
            Some(b) if b == o => {}
            Some(_) => problems.push(format!("object {h} differs")),
            None => problems.push(format!("object {h} missing")),
        }
    }
    if back.objects.len() != d.objects.len() {
        problems.push(format!(
            "{} objects read back, {} written",
            back.objects.len(),
            d.objects.len()
        ));
    }
    let preserved = d.objects.values().filter(|o| o.raw.is_some()).count();
    println!(
        "{path}: {} objects ({preserved} kept as original DWG bytes), source {src} bytes → .blk {} bytes; write {write_ms:.1} ms, read {read_ms:.1} ms",
        d.objects.len(),
        bytes.len()
    );
    if problems.is_empty() {
        println!("OK: lossless round trip");
        Ok(())
    } else {
        for p in problems.iter().take(20) {
            println!("  {p}");
        }
        Err(format!("{} differences", problems.len()))
    }
}

fn png(d: &Drawing, a: &Args) -> Result<(), String> {
    let out = a.value(1, "output path")?;
    let number = |name: &str, default: u32| -> Result<u32, String> {
        match a.option(name) {
            None => Ok(default),
            Some(v) => v
                .parse()
                .map_err(|_| format!("{name} needs a number, not {v:?}")),
        }
    };
    let opts = raster::RasterOptions {
        width: number("--width", 1600)?,
        height: number("--height", 1000)?,
        dark: !a.has("--light"),
        paper_color: a.has("--color"),
        grid: !a.has("--no-grid"),
        ..Default::default()
    };
    let scene = render::model_space(d);
    let t = std::time::Instant::now();
    let img = raster::render(&scene, &opts);
    let draw_ms = t.elapsed().as_secs_f64() * 1000.0;
    let bytes = img.to_png();
    std::fs::write(out, &bytes).map_err(|e| format!("{out}: {e}"))?;
    report_scene(&scene);
    println!(
        "wrote {out} ({}×{}, {} bytes, drawn in {draw_ms:.1} ms)",
        img.width,
        img.height,
        bytes.len()
    );
    Ok(())
}

fn color_name(c: Color) -> String {
    match c {
        Color::Index(i) => format!("{i}"),
        Color::Rgb(r, g, b) => format!("#{r:02X}{g:02X}{b:02X}"),
        Color::ByLayer => "ByLayer".into(),
        Color::ByBlock => "ByBlock".into(),
        Color::None => "none".into(),
    }
}

fn layers(d: &Drawing) {
    let current = d.current_layer();
    println!(
        "  {:<20} {:>7}  {:<12} {:>9}  {:<22} {:>7}",
        "Layer", "Colour", "Linetype", "Weight", "State", "Entities"
    );
    for (h, l) in d.layers() {
        let mut state = vec![if l.off { "off" } else { "on" }];
        if l.frozen {
            state.push("frozen");
        }
        if l.locked {
            state.push("locked");
        }
        if !l.plot {
            state.push("no plot");
        }
        let linetype = d.record_name(l.linetype).unwrap_or("Continuous");
        println!(
            "{} {:<20} {:>7}  {:<12} {:>9}  {:<22} {:>7}",
            if Some(h) == current { '*' } else { ' ' },
            l.name,
            color_name(l.color),
            linetype,
            l.weight().to_string(),
            state.join(", "),
            d.layer_entity_count(h)
        );
    }
    println!("(* current layer)");
}

fn length(a: &Args) -> Result<(), String> {
    let input = a.value(0, "length to read")?;
    let unit = match a.option("--unit").unwrap_or("mm") {
        "mm" => Unit::Millimeter,
        "cm" => Unit::Centimeter,
        "m" => Unit::Meter,
        "in" | "inch" => Unit::Inch,
        "ft" | "foot" | "feet" => Unit::Foot,
        "yd" => Unit::Yard,
        "none" | "unitless" => Unit::Unitless,
        other => return Err(format!("unknown unit {other:?}")),
    };
    let format = match a.option("--format").unwrap_or("decimal") {
        "decimal" => LinearFormat::Decimal,
        "fractional" => LinearFormat::Fractional,
        "architectural" => LinearFormat::Architectural,
        "engineering" => LinearFormat::Engineering,
        "scientific" => LinearFormat::Scientific,
        other => return Err(format!("unknown format {other:?}")),
    };
    let precision = match a.option("--precision") {
        Some(p) => p.parse().map_err(|_| format!("bad precision {p:?}"))?,
        None => 4,
    };
    let v = units::parse_length(input, unit).map_err(|e| e.to_string())?;
    let shown = units::format_length(
        v,
        unit,
        &LengthFormat {
            format,
            precision,
            unit_mark: true,
            ..Default::default()
        },
    );
    let exact = units::format_length(
        v,
        unit,
        &LengthFormat {
            precision: 8,
            trim_zeros: true,
            unit_mark: true,
            ..Default::default()
        },
    );
    println!("{input} = {exact} → {shown}");
    Ok(())
}

fn report_scene(scene: &render::Scene) {
    let s = &scene.stats;
    println!(
        "{} entities drawn, {} shapes, {} hidden, {} failed{}",
        s.entities_drawn,
        scene.items.len(),
        s.hidden,
        s.failed,
        if s.depth_limited > 0 {
            format!(", {} beyond nesting limit", s.depth_limited)
        } else {
            String::new()
        }
    );
    if !s.skipped.is_empty() {
        let list: Vec<String> = s.skipped.iter().map(|(k, v)| format!("{k} ×{v}")).collect();
        println!("not drawn yet: {}", list.join(", "));
    }
    if !scene.bounds.is_empty() {
        println!(
            "extents: ({:.4}, {:.4}) – ({:.4}, {:.4})",
            scene.bounds.min.x, scene.bounds.min.y, scene.bounds.max.x, scene.bounds.max.y
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_times_print_as_utc_dates() {
        for (t, want) in [
            (0, "1970-01-01 00:00:00 UTC"),
            (1_700_000_000, "2023-11-14 22:13:20 UTC"),
            (951_782_400, "2000-02-29 00:00:00 UTC"),
            (-1, "1969-12-31 23:59:59 UTC"),
            (4_102_444_800, "2100-01-01 00:00:00 UTC"),
        ] {
            assert_eq!(utc(t), want);
        }
    }
}

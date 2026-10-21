//! `cargo xtask web` and `cargo xtask serve`: the browser workbench.
//!
//! `web` builds `crates/cad-ffi` for `wasm32-unknown-unknown` and puts
//! the app together in `dist/web/`: the files of `apps/web/` and the core
//! as `core.wasm`, with the service worker's cache name stamped with a
//! hash of the build. `serve` runs a small web server over that folder for
//! testing, on this computer only unless told otherwise. Both use nothing
//! but the standard library, like the rest of the workspace.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub const USAGE: &str = "    cargo xtask web [--debug]
        Build the core for WebAssembly (wasm32-unknown-unknown) and put the
        browser workbench together in dist/web/. Also writes
        dist/cad-test.html: the same workbench as one file (scripts, styles
        and core inlined) for testing the GUI by opening it in a browser; it
        has no service worker, so it does not install or work offline.

    cargo xtask serve [--port N] [--host ADDR] [--no-build] [--debug]
        Build as `web` does, then serve dist/web/ at http://127.0.0.1:8080/
        until stopped (Ctrl+C). --host 0.0.0.0 serves other devices on the
        network too; browsers install the app and run its service worker
        only from localhost or HTTPS, so for a phone forward the port
        instead: adb reverse tcp:8080 tcp:8080.";

const TARGET: &str = "wasm32-unknown-unknown";

fn opt<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => args
            .get(i + 1)
            .map(|s| Some(s.as_str()))
            .ok_or_else(|| format!("{name} needs a value")),
    }
}

fn check_args(args: &[String], flags: &[&str], valued: &[&str]) -> Result<(), String> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if valued.contains(&a) {
            i += 2;
        } else if flags.contains(&a) {
            i += 1;
        } else {
            return Err(format!("unknown option {a:?}\n\n{USAGE}"));
        }
    }
    Ok(())
}

pub fn web(root: &Path, args: &[String]) -> Result<(), String> {
    check_args(args, &["--debug"], &[])?;
    build(root, !args.iter().any(|a| a == "--debug")).map(|_| ())
}

/// Build and assemble; the folder made.
fn build(root: &Path, release: bool) -> Result<PathBuf, String> {
    if !target_installed() {
        return Err(format!(
            "the Rust standard library for {TARGET} is not installed; install it with\n    rustup target add {TARGET}"
        ));
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root).args([
        "rustc",
        "--package",
        "cad-ffi",
        "--lib",
        "--crate-type",
        "cdylib",
        "--target",
        TARGET,
    ]);
    if release {
        cmd.arg("--release");
    }
    println!("Building the core for {TARGET}");
    let status = cmd.status().map_err(|e| format!("could not run cargo: {e}"))?;
    if !status.success() {
        return Err("the WebAssembly build failed".into());
    }
    let wasm = crate::target_dir(root)
        .join(TARGET)
        .join(if release { "release" } else { "debug" })
        .join("cad_ffi.wasm");
    let wasm_bytes = fs::read(&wasm).map_err(|e| format!("{}: {e}", wasm.display()))?;
    check_wasm(&wasm_bytes)?;

    let src = root.join("apps").join("web");
    let out = root.join("dist").join("web");
    if out.exists() {
        fs::remove_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    let mut files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    collect(&src, &src, &mut files)?;
    files.push((PathBuf::from("core.wasm"), wasm_bytes));
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = Fnv::new();
    for (p, b) in &files {
        h.write(p.to_string_lossy().as_bytes());
        h.write(b);
    }
    let stamp = format!("{:016x}", h.0);
    for (rel, bytes) in &mut files {
        if rel == Path::new("sw.js") {
            let text = String::from_utf8_lossy(bytes).replace("const BUILD = \"dev\";", &format!("const BUILD = \"{stamp}\";"));
            *bytes = text.into_bytes();
        }
        let dest = out.join(&*rel);
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(&dest, &*bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    let size = files.iter().find(|f| f.0 == Path::new("core.wasm")).map(|f| f.1.len()).unwrap_or(0);
    let single = single_file(&files, &stamp)?;
    let single_path = root.join("dist").join("cad-test.html");
    fs::write(&single_path, &single).map_err(|e| format!("{}: {e}", single_path.display()))?;
    println!("Built {} (one file, {} KB)", single_path.display(), single.len() / 1024);
    println!(
        "Built {} ({} files, core.wasm {} KB, build {stamp})",
        out.display(),
        files.len(),
        size / 1024
    );
    Ok(out)
}

fn target_installed() -> bool {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    match Command::new(rustc)
        .args(["--print", "target-libdir", "--target", TARGET])
        .output()
    {
        Ok(o) if o.status.success() => {
            let dir = String::from_utf8_lossy(&o.stdout);
            let dir = Path::new(dir.trim());
            dir.is_dir()
                && fs::read_dir(dir)
                    .map(|mut d| d.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("libstd"))))
                    .unwrap_or(false)
        }
        // Cannot tell: let Cargo report it.
        _ => true,
    }
}

/// The module must import nothing: the page gives it no functions.
pub fn check_wasm(b: &[u8]) -> Result<(), String> {
    if b.len() < 8 || &b[..4] != b"\0asm" {
        return Err("cad_ffi.wasm is not a WebAssembly module".into());
    }
    let mut p = 8;
    let leb = |p: &mut usize| -> Result<u64, String> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let byte = *b.get(*p).ok_or("cad_ffi.wasm ends inside a section header")?;
            *p += 1;
            v |= u64::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
            if shift > 63 {
                return Err("cad_ffi.wasm has a malformed section size".into());
            }
        }
    };
    while p < b.len() {
        let id = b[p];
        p += 1;
        let size = leb(&mut p)? as usize;
        if id == 2 {
            let mut q = p;
            let n = leb(&mut q)?;
            if n > 0 {
                return Err(format!(
                    "cad_ffi.wasm imports {n} functions or values; the page provides none, so \
                     something in the core reaches for the platform (a clock, files, threads) \
                     where the browser has none"
                ));
            }
        }
        p += size;
    }
    Ok(())
}

fn collect(base: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for e in entries {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "core.wasm" {
            continue;
        }
        if p.is_dir() {
            collect(base, &p, out)?;
        } else {
            let bytes = fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            out.push((p.strip_prefix(base).unwrap().to_path_buf(), bytes));
        }
    }
    Ok(())
}

/// The workbench as one HTML file: the stylesheet and scripts inlined, the
/// modules joined into one, and core.wasm carried as base64. Fails if the
/// page has changed shape, so the file is never quietly incomplete.
fn single_file(files: &[(PathBuf, Vec<u8>)], stamp: &str) -> Result<String, String> {
    let get = |name: &str| -> Result<&[u8], String> {
        files
            .iter()
            .find(|f| f.0 == Path::new(name))
            .map(|f| f.1.as_slice())
            .ok_or_else(|| format!("single file: {name} is missing"))
    };
    let text = |name: &str| -> Result<String, String> {
        String::from_utf8(get(name)?.to_vec()).map_err(|_| format!("single file: {name} is not UTF-8"))
    };
    let mut html = text("index.html")?;
    let css = text("style.css")?;
    if css.to_ascii_lowercase().contains("</style") {
        return Err("single file: style.css contains </style".into());
    }
    let script = bundle(&text, "app.js")?;
    let icon = format!("data:image/svg+xml;base64,{}", base64(get("icons/icon.svg")?));
    let swaps: [(&str, String); 5] = [
        ("<link rel=\"manifest\" href=\"manifest.webmanifest\">\n", String::new()),
        ("<link rel=\"apple-touch-icon\" href=\"icons/icon-192.png\">\n", String::new()),
        ("href=\"icons/icon.svg\"", format!("href=\"{icon}\"")),
        ("<link rel=\"stylesheet\" href=\"style.css\">", format!("<style>\n{css}</style>")),
        (
            "<script type=\"module\" src=\"app.js\"></script>",
            format!(
                "<!-- CAD single-file test build {stamp}: no service worker, no install. -->\n\
                 <script>globalThis.CAD_CORE_WASM = \"{}\";</script>\n<script type=\"module\">\n{script}</script>",
                base64(get("core.wasm")?)
            ),
        ),
    ];
    for (from, to) in swaps {
        if !html.contains(from) {
            return Err(format!("single file: index.html no longer has {from:?}"));
        }
        html = html.replacen(from, &to, 1);
    }
    html = html.replacen("<title>CAD</title>", "<title>CAD (test build)</title>", 1);
    Ok(html)
}

/// Join `entry` and the modules it imports into one module script. Each
/// imported module is wrapped in a function so its private names stay
/// private; its `export`s become the object it returns. One level deep:
/// an imported module that imports is refused.
fn bundle(text: &dyn Fn(&str) -> Result<String, String>, entry: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut body = String::new();
    for line in text(entry)?.lines() {
        if let Some((names, from)) = parse_import(line) {
            let src = text(&from)?;
            let var = format!("__{}", from.trim_end_matches(".js").replace(|c: char| !c.is_ascii_alphanumeric(), "_"));
            let (code, exports) = strip_exports(&src, &from)?;
            out.push_str(&format!(
                "const {var} = (() => {{\n{code}return {{ {} }};\n}})();\nconst {{ {} }} = {var};\n",
                exports.join(", "),
                names.join(", ")
            ));
            for n in &names {
                if !exports.contains(n) {
                    return Err(format!("single file: {from} does not export {n}"));
                }
            }
        } else if line.trim_start().starts_with("import ") {
            return Err(format!("single file: {entry}: cannot inline {line:?}"));
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    out.push_str(&body);
    if out.to_ascii_lowercase().contains("</script") {
        return Err("single file: a script contains </script".into());
    }
    Ok(out)
}

/// `import { a, b } from "./x.js";` gives (["a", "b"], "x.js").
fn parse_import(line: &str) -> Option<(Vec<String>, String)> {
    let rest = line.trim().strip_prefix("import {")?;
    let (names, rest) = rest.split_once('}')?;
    let from = rest.trim().strip_prefix("from")?.trim().trim_end_matches(';');
    let from = from.trim_matches('"').trim_matches('\'').strip_prefix("./")?;
    let names = names.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
    Some((names, from.to_string()))
}

fn strip_exports(src: &str, name: &str) -> Result<(String, Vec<String>), String> {
    let mut code = String::new();
    let mut exports = Vec::new();
    for line in src.lines() {
        if line.trim_start().starts_with("import ") {
            return Err(format!("single file: {name} imports; only one level is joined"));
        }
        if let Some(rest) = line.strip_prefix("export ") {
            let words: Vec<&str> = rest.split(|c: char| c.is_whitespace() || c == '(' || c == '{' || c == '=').filter(|w| !w.is_empty()).collect();
            let n = match words.as_slice() {
                ["async", "function", n, ..] | ["function", n, ..] | ["class", n, ..] | ["const" | "let", n, ..] => *n,
                _ => return Err(format!("single file: {name}: cannot read {line:?}")),
            };
            exports.push(n.to_string());
            code.push_str(rest);
        } else {
            code.push_str(line);
        }
        code.push('\n');
    }
    Ok((code, exports))
}

fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// FNV-1a, for the build stamp.
struct Fnv(u64);
impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn write(&mut self, b: &[u8]) {
        for x in b {
            self.0 ^= u64::from(*x);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

pub fn serve(root: &Path, args: &[String]) -> Result<(), String> {
    check_args(args, &["--no-build", "--debug"], &["--port", "--host"])?;
    let port: u16 = opt(args, "--port")?
        .map(|p| p.parse().map_err(|_| format!("{p:?} is not a port number")))
        .transpose()?
        .unwrap_or(8080);
    let host = opt(args, "--host")?.unwrap_or("127.0.0.1");
    let dir = if args.iter().any(|a| a == "--no-build") {
        root.join("dist").join("web")
    } else {
        build(root, !args.iter().any(|a| a == "--debug"))?
    };
    if !dir.join("index.html").is_file() {
        return Err(format!("{} has no index.html; run cargo xtask web first", dir.display()));
    }
    let listener = TcpListener::bind((host, port)).map_err(|e| format!("cannot listen on {host}:{port}: {e}"))?;
    let shown = if host == "0.0.0.0" { "localhost" } else { host };
    println!("Serving {} at http://{shown}:{port}/  (Ctrl+C stops)", dir.display());
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let dir = dir.clone();
        std::thread::spawn(move || {
            let _ = handle(stream, &dir);
        });
    }
    Ok(())
}

pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "webmanifest" => "application/manifest+json",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The file a request path names, if it stays inside `dir`.
pub fn resolve(dir: &Path, url_path: &str) -> Option<PathBuf> {
    let path = url_path.split(['?', '#']).next().unwrap_or("/");
    let path = percent_decode(path)?;
    let mut out = dir.to_path_buf();
    for c in Path::new(path.trim_start_matches('/')).components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.is_dir() {
        out.push("index.html");
    }
    Some(out)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn handle(mut stream: TcpStream, dir: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    // Skip the headers.
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let reply = |s: &mut TcpStream, code: &str, ty: &str, body: &[u8], head_only: bool| -> std::io::Result<()> {
        write!(
            s,
            "HTTP/1.1 {code}\r\nContent-Type: {ty}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        if !head_only {
            s.write_all(body)?;
        }
        s.flush()
    };
    if method != "GET" && method != "HEAD" {
        return reply(&mut stream, "405 Method Not Allowed", "text/plain", b"GET only\n", false);
    }
    match resolve(dir, target).and_then(|p| fs::read(&p).ok().map(|b| (p, b))) {
        Some((p, body)) => {
            println!("{method} {target}");
            reply(&mut stream, "200 OK", content_type(&p), &body, method == "HEAD")
        }
        None => {
            println!("{method} {target} (not found)");
            reply(&mut stream, "404 Not Found", "text/plain", b"not found\n", method == "HEAD")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_folder() {
        let d = Path::new("/srv/web");
        assert_eq!(resolve(d, "/app.js?v=2"), Some(PathBuf::from("/srv/web/app.js")));
        assert_eq!(resolve(d, "/icons/icon%20a.png"), Some(PathBuf::from("/srv/web/icons/icon a.png")));
        assert_eq!(resolve(d, "/../secret"), None);
        assert_eq!(resolve(d, "/a/%2e%2e/%2e%2e/b"), None);
        assert_eq!(content_type(Path::new("core.wasm")), "application/wasm");
    }

    #[test]
    fn one_file_joins_the_modules() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
        assert_eq!(
            parse_import("import { Core, opLength } from \"./core.js\";"),
            Some((vec!["Core".to_string(), "opLength".to_string()], "core.js".to_string()))
        );
        let files = |n: &str| -> Result<String, String> {
            match n {
                "a.js" => Ok("import { f } from \"./b.js\";\nf();\n".into()),
                "b.js" => Ok("const k = 1;\nexport function f() { return k; }\n".into()),
                "c.js" => Ok("import { g } from \"./b.js\";\n".into()),
                _ => Err(n.into()),
            }
        };
        let js = bundle(&files, "a.js").unwrap();
        assert!(js.contains("const __b = (() => {\nconst k = 1;\nfunction f()"));
        assert!(js.contains("return { f };") && js.contains("const { f } = __b;") && js.ends_with("f();\n"));
        assert!(bundle(&files, "c.js").unwrap_err().contains("does not export g"));
    }

    #[test]
    fn modules_that_import_are_refused() {
        // Magic, version, then an import section with one entry.
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        assert!(check_wasm(&m).is_ok());
        m.extend_from_slice(&[2, 1, 1]);
        assert!(check_wasm(&m).unwrap_err().contains("imports 1"));
        assert!(check_wasm(b"nope").is_err());
    }
}

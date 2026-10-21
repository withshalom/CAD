//! Build helpers for this workspace, run as `cargo xtask <command>`.
//!
//! `android` builds `crates/cad-ffi` for Android, with the NDK's own clang
//! as the linker, checks each library it produces, and puts it in
//! `apps/android/app/src/main/jniLibs/<abi>/`, where Gradle packages it.
//! `check-elf` prints the machine and page alignment of shared libraries.
//! `web` and `serve` build the browser workbench and serve it (`web.rs`).
//!
//! Standard library only, like the rest of the workspace, and the same on
//! Windows, macOS and Linux.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod web;

const USAGE: &str = "\
cargo xtask — build helpers

USAGE:
    cargo xtask android [--debug] [--abi LIST|all] [--api LEVEL] [--ndk DIR]
        Build crates/cad-ffi for Android and copy libcad_ffi.so into
        apps/android/app/src/main/jniLibs/<abi>/ (replacing what was there).
          --abi    comma-separated: arm64-v8a, armeabi-v7a, x86_64, x86
                   (default arm64-v8a,x86_64; all = arm64-v8a,armeabi-v7a,x86_64)
          --api    oldest Android API level to support (default 26; keep it
                   equal to minSdk in apps/android/app/build.gradle.kts)
          --ndk    Android NDK folder (default: ANDROID_NDK_HOME,
                   ANDROID_NDK_ROOT, or the newest NDK in the Android SDK)
          --debug  unoptimised build (the default is a release build)

    cargo xtask check-elf FILE...
        Print the machine and the memory-page alignment of ELF libraries.

";

/// One Android ABI and how to build for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Abi {
    /// Android's name for it, also the folder name under `jniLibs`.
    name: &'static str,
    /// Rust target.
    triple: &'static str,
    /// Target for the NDK's clang, to which the API level is appended.
    clang: &'static str,
    /// ELF machine number (`e_machine`).
    machine: u16,
    bits64: bool,
}

const ABIS: [Abi; 4] = [
    Abi {
        name: "arm64-v8a",
        triple: "aarch64-linux-android",
        clang: "aarch64-linux-android",
        machine: 183,
        bits64: true,
    },
    Abi {
        name: "armeabi-v7a",
        triple: "armv7-linux-androideabi",
        clang: "armv7a-linux-androideabi",
        machine: 40,
        bits64: false,
    },
    Abi {
        name: "x86_64",
        triple: "x86_64-linux-android",
        clang: "x86_64-linux-android",
        machine: 62,
        bits64: true,
    },
    Abi {
        name: "x86",
        triple: "i686-linux-android",
        clang: "i686-linux-android",
        machine: 3,
        bits64: false,
    },
];
/// Phones and tablets, plus the emulator and Chromebooks.
const DEFAULT_ABIS: [&str; 2] = ["arm64-v8a", "x86_64"];
/// The above plus older 32-bit ARM devices.
const ALL_ABIS: [&str; 3] = ["arm64-v8a", "armeabi-v7a", "x86_64"];
const DEFAULT_API: u32 = 26;
/// Devices with 16 KB memory pages (Android 15 and later) can only load
/// 64-bit libraries whose segments are aligned to at least this.
const PAGE_16K: u64 = 16384;
const LIB: &str = "libcad_ffi.so";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("android") => android(&workspace_root(), &args[1..]),
        Some("check-elf") => check_elf(&args[1..]),
        Some("web") => web::web(&workspace_root(), &args[1..]),
        Some("serve") => web::serve(&workspace_root(), &args[1..]),
        Some("help" | "--help" | "-h") => {
            println!("{USAGE}{}", web::USAGE);
            Ok(())
        }
        None => Err(format!("{USAGE}{}", web::USAGE)),
        Some(other) => Err(format!("unknown command {other:?}\n\n{USAGE}{}", web::USAGE)),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace root")
        .to_path_buf()
}

// ------------------------------------------------------------------ android

#[derive(Debug, PartialEq)]
struct AndroidOptions {
    release: bool,
    abis: Vec<Abi>,
    api: u32,
    ndk: Option<PathBuf>,
}

fn parse_android(args: &[String]) -> Result<AndroidOptions, String> {
    let mut o = AndroidOptions {
        release: true,
        abis: Vec::new(),
        api: DEFAULT_API,
        ndk: None,
    };
    let mut abis = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = || {
            inline
                .clone()
                .or_else(|| it.next().cloned())
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag {
            "--debug" => o.release = false,
            "--release" => o.release = true,
            "--abi" => abis = Some(value()?),
            "--api" => {
                let v = value()?;
                o.api = v
                    .parse()
                    .map_err(|_| format!("--api: {v:?} is not an API level"))?;
            }
            "--ndk" => o.ndk = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown option {other:?}\n\n{USAGE}")),
        }
    }
    o.abis = parse_abis(abis.as_deref())?;
    Ok(o)
}

fn parse_abis(list: Option<&str>) -> Result<Vec<Abi>, String> {
    let names: Vec<&str> = match list {
        None => DEFAULT_ABIS.to_vec(),
        Some("all") => ALL_ABIS.to_vec(),
        Some(l) => l
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect(),
    };
    let mut out: Vec<Abi> = Vec::new();
    for n in names {
        let abi = ABIS
            .iter()
            .find(|a| a.name == n || a.triple == n)
            .ok_or_else(|| {
                let known: Vec<&str> = ABIS.iter().map(|a| a.name).collect();
                format!("unknown ABI {n:?}; known: {}", known.join(", "))
            })?;
        if !out.contains(abi) {
            out.push(*abi);
        }
    }
    if out.is_empty() {
        return Err("--abi: no ABI given".into());
    }
    Ok(out)
}

fn android(root: &Path, args: &[String]) -> Result<(), String> {
    let o = parse_android(args)?;
    let ndk = find_ndk(o.ndk.as_deref(), root)?;
    let meta = NdkMeta::read(&ndk);
    if let Some(min) = meta.min_api {
        if o.api < min {
            return Err(format!(
                "the NDK in {} supports API {min} and up; --api {} is lower",
                ndk.display(),
                o.api
            ));
        }
    }
    if let Some(max) = meta.max_api {
        if o.api > max {
            return Err(format!(
                "the NDK in {} supports API levels up to {max}; --api {} is higher",
                ndk.display(),
                o.api
            ));
        }
    }
    let missing = missing_targets(&o.abis);
    if !missing.is_empty() {
        return Err(format!(
            "Rust's standard library for {} is not installed. Run:\n    rustup target add {}",
            missing.join(", "),
            missing.join(" ")
        ));
    }
    println!(
        "NDK {} in {}; Android API {} and up",
        meta.revision.as_deref().unwrap_or("(version unknown)"),
        ndk.display(),
        o.api
    );

    let clang = clang_path(&ndk);
    let mut built = Vec::new();
    for abi in &o.abis {
        let lib = build(root, abi, &clang, o.api, o.release)?;
        let info = check_library(abi, &lib)?;
        built.push((*abi, lib, info));
    }

    // Replace whatever an earlier run left, so the app gets exactly this
    // build and no stale ABI.
    let jni = jni_libs_dir(root);
    if jni.exists() {
        fs::remove_dir_all(&jni).map_err(|e| format!("{}: {e}", jni.display()))?;
    }
    for (abi, lib, info) in &built {
        let dir = jni.join(abi.name);
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let to = dir.join(LIB);
        fs::copy(lib, &to).map_err(|e| format!("{} -> {}: {e}", lib.display(), to.display()))?;
        let size = fs::metadata(&to).map(|m| m.len()).unwrap_or(0);
        println!(
            "  {:<12} {:>6.2} MB  {}",
            abi.name,
            size as f64 / 1e6,
            describe(info)
        );
    }
    println!("Copied to {}", jni.display());
    println!(
        "Next: open apps/android in Android Studio and run the app, \
         or build it with: gradle -p apps/android assembleDebug"
    );
    Ok(())
}

fn jni_libs_dir(root: &Path) -> PathBuf {
    ["apps", "android", "app", "src", "main", "jniLibs"]
        .iter()
        .fold(root.to_path_buf(), |p, s| p.join(s))
}

/// Linker arguments for one ABI, after `-C link-arg=`.
fn link_args(abi: &Abi, api: u32) -> Vec<String> {
    vec![
        // Tells clang the target and the API level, so it links against
        // that level's system libraries (what the NDK's per-level wrapper
        // scripts do).
        format!("--target={}{api}", abi.clang),
        // Android 15 and later can run with 16 KB memory pages. NDK r28 and
        // later align for that by default; older NDKs need telling.
        "-Wl,-z,max-page-size=16384".into(),
        "-Wl,-z,common-page-size=16384".into(),
        // Android's loader expects the library to name itself.
        format!("-Wl,-soname,{LIB}"),
    ]
}

fn build(root: &Path, abi: &Abi, clang: &Path, api: u32, release: bool) -> Result<PathBuf, String> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    // Only the shared library: Cargo skips link-time optimisation for a
    // build that also makes an rlib.
    cmd.current_dir(root).args([
        "rustc",
        "--package",
        "cad-ffi",
        "--lib",
        "--crate-type",
        "cdylib",
        "--target",
        abi.triple,
    ]);
    if release {
        cmd.arg("--release");
    }
    // Everything after `--` goes to rustc for cad-ffi alone, the crate that
    // is linked; nothing is set in the environment or in Cargo config.
    let mut linker = OsString::from("linker=");
    linker.push(clang.as_os_str());
    cmd.arg("--").arg("-C").arg(linker);
    for a in link_args(abi, api) {
        cmd.arg("-C").arg(format!("link-arg={a}"));
    }
    println!("Building {} ({})", abi.name, abi.triple);
    let status = cmd
        .status()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("the build for {} failed", abi.name));
    }
    let lib = target_dir(root)
        .join(abi.triple)
        .join(if release { "release" } else { "debug" })
        .join(LIB);
    if !lib.is_file() {
        return Err(format!(
            "the build finished but {} is missing (if Cargo's target folder is \
             configured elsewhere, set CARGO_TARGET_DIR)",
            lib.display()
        ));
    }
    Ok(lib)
}

pub(crate) fn target_dir(root: &Path) -> PathBuf {
    for var in ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"] {
        if let Some(p) = env_path(var) {
            return if p.is_absolute() { p } else { root.join(p) };
        }
    }
    root.join("target")
}

/// Targets whose Rust standard library is not installed. A target that
/// cannot be checked is left for Cargo to report.
fn missing_targets(abis: &[Abi]) -> Vec<&'static str> {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    abis.iter()
        .filter(|a| {
            match Command::new(&rustc)
                .args(["--print", "target-libdir", "--target", a.triple])
                .output()
            {
                Ok(out) if out.status.success() => {
                    let dir = String::from_utf8_lossy(&out.stdout);
                    !Path::new(dir.trim()).is_dir()
                }
                _ => false,
            }
        })
        .map(|a| a.triple)
        .collect()
}

// ---------------------------------------------------------------------- NDK

fn env_path(var: &str) -> Option<PathBuf> {
    env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Folder name of the NDK's prebuilt tools for this computer.
fn host_tag() -> &'static str {
    if cfg!(windows) {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") {
        // The macOS tools are universal binaries under this name.
        "darwin-x86_64"
    } else {
        "linux-x86_64"
    }
}

fn clang_path(ndk: &Path) -> PathBuf {
    let exe = if cfg!(windows) { "clang.exe" } else { "clang" };
    ["toolchains", "llvm", "prebuilt", host_tag(), "bin", exe]
        .iter()
        .fold(ndk.to_path_buf(), |p, s| p.join(s))
}

fn check_ndk(ndk: &Path) -> Result<(), String> {
    let clang = clang_path(ndk);
    if clang.is_file() {
        Ok(())
    } else {
        Err(format!(
            "not an Android NDK for this computer (no {})",
            clang.display()
        ))
    }
}

fn find_ndk(explicit: Option<&Path>, root: &Path) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        check_ndk(p).map_err(|e| format!("--ndk {}: {e}", p.display()))?;
        return Ok(p.to_path_buf());
    }
    for var in ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "ANDROID_NDK"] {
        if let Some(p) = env_path(var) {
            check_ndk(&p).map_err(|e| format!("{var}={}: {e}", p.display()))?;
            return Ok(p);
        }
    }
    let sdks = sdk_dirs(root);
    for sdk in &sdks {
        if let Some(ndk) = newest_ndk(&sdk.join("ndk")) {
            return Ok(ndk);
        }
        let bundle = sdk.join("ndk-bundle");
        if check_ndk(&bundle).is_ok() {
            return Ok(bundle);
        }
    }
    let looked: Vec<String> = sdks.iter().map(|p| p.display().to_string()).collect();
    Err(format!(
        "no Android NDK found. Looked at ANDROID_NDK_HOME, ANDROID_NDK_ROOT and \
         ANDROID_NDK, and in the Android SDK at: {}.\n\
         Install one with Android Studio's SDK Manager (SDK Tools tab, \
         \"NDK (Side by side)\"), or pass --ndk DIR.",
        if looked.is_empty() {
            "(none found)".to_string()
        } else {
            looked.join(", ")
        }
    ))
}

/// Where an Android SDK may be, most specific first.
fn sdk_dirs(root: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        v.extend(env_path(var));
    }
    // Android Studio records the SDK it uses here when it opens the app.
    let local = root.join("apps").join("android").join("local.properties");
    if let Ok(text) = fs::read_to_string(local) {
        v.extend(property(&text, "sdk.dir").map(PathBuf::from));
    }
    // Android Studio's default install locations.
    if cfg!(windows) {
        v.extend(env_path("LOCALAPPDATA").map(|p| p.join("Android").join("Sdk")));
    } else if let Some(home) = env_path("HOME") {
        v.push(if cfg!(target_os = "macos") {
            home.join("Library").join("Android").join("sdk")
        } else {
            home.join("Android").join("Sdk")
        });
    }
    let mut seen = Vec::new();
    v.retain(|p| {
        let new = !seen.contains(p);
        seen.push(p.clone());
        new
    });
    v
}

/// The newest NDK in an SDK's `ndk` folder, whose subfolders are named by
/// version (`28.2.13676358`).
fn newest_ndk(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(version) = version_key(&name) else {
            continue;
        };
        let path = entry.path();
        if check_ndk(&path).is_err() {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| version > *b) {
            best = Some((version, path));
        }
    }
    best.map(|(_, p)| p)
}

fn version_key(s: &str) -> Option<Vec<u64>> {
    s.split('.').map(|p| p.parse().ok()).collect()
}

/// What an NDK says about itself.
#[derive(Debug, Default, PartialEq)]
struct NdkMeta {
    /// `Pkg.Revision` from `source.properties`, such as `29.0.14206865`.
    revision: Option<String>,
    /// Oldest and newest API levels it has system libraries for, from
    /// `meta/platforms.json`.
    min_api: Option<u32>,
    max_api: Option<u32>,
}

impl NdkMeta {
    fn read(ndk: &Path) -> NdkMeta {
        let props = fs::read_to_string(ndk.join("source.properties")).unwrap_or_default();
        let platforms =
            fs::read_to_string(ndk.join("meta").join("platforms.json")).unwrap_or_default();
        NdkMeta {
            revision: property(&props, "Pkg.Revision"),
            min_api: json_number(&platforms, "min"),
            max_api: json_number(&platforms, "max"),
        }
    }
}

/// A value from a Java-style `.properties` file (`key=value` or
/// `key: value`), with its escapes undone.
fn property(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(value) = rest.strip_prefix('=').or_else(|| rest.strip_prefix(':')) else {
            continue;
        };
        return Some(unescape(value.trim()));
    }
    None
}

/// Undo `.properties` escapes: Android Studio writes Windows paths as
/// `C\:\\Users\\...`.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// The whole-number value of `"key":` in a small JSON file.
fn json_number(text: &str, key: &str) -> Option<u32> {
    let quoted = format!("\"{key}\"");
    let at = text.find(&quoted)? + quoted.len();
    let rest = text[at..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

// ---------------------------------------------------------------------- ELF

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ElfInfo {
    bits64: bool,
    machine: u16,
    /// Smallest alignment among the loadable (`PT_LOAD`) segments.
    load_align: u64,
}

/// Read what matters here from an ELF file's headers.
fn elf_info(bytes: &[u8]) -> Result<ElfInfo, String> {
    if bytes.get(..4) != Some(b"\x7fELF".as_slice()) {
        return Err("not an ELF file".into());
    }
    let bits64 = match bytes.get(4) {
        Some(1) => false,
        Some(2) => true,
        other => return Err(format!("unknown ELF class {other:?}")),
    };
    if bytes.get(5) != Some(&1) {
        return Err("not a little-endian ELF file".into());
    }
    let field = |at: usize, len: usize| -> Result<u64, String> {
        let b = at
            .checked_add(len)
            .and_then(|end| bytes.get(at..end))
            .ok_or("truncated ELF file")?;
        Ok(b.iter().rev().fold(0u64, |v, &x| (v << 8) | u64::from(x)))
    };
    let machine = field(0x12, 2)? as u16;
    let (phoff, phentsize, phnum, need) = if bits64 {
        (field(0x20, 8)?, field(0x36, 2)?, field(0x38, 2)?, 56)
    } else {
        (field(0x1C, 4)?, field(0x2A, 2)?, field(0x2C, 2)?, 32)
    };
    if phentsize < need {
        return Err(format!(
            "program headers of {phentsize} bytes, expected {need}"
        ));
    }
    let mut load_align: Option<u64> = None;
    for i in 0..phnum {
        let at = i
            .checked_mul(phentsize)
            .and_then(|o| o.checked_add(phoff))
            .and_then(|o| usize::try_from(o).ok())
            .ok_or("program header out of range")?;
        const PT_LOAD: u64 = 1;
        if field(at, 4)? != PT_LOAD {
            continue;
        }
        let align = if bits64 {
            field(at + 48, 8)?
        } else {
            field(at + 28, 4)?
        };
        load_align = Some(load_align.map_or(align, |m| m.min(align)));
    }
    Ok(ElfInfo {
        bits64,
        machine,
        load_align: load_align.ok_or("no loadable segments")?,
    })
}

fn machine_name(machine: u16) -> &'static str {
    match machine {
        3 => "x86",
        40 => "ARM",
        62 => "x86-64",
        183 => "AArch64",
        243 => "RISC-V",
        _ => "another machine",
    }
}

fn describe(info: &ElfInfo) -> String {
    let pages = if info.load_align >= PAGE_16K {
        "loads with 16 KB pages"
    } else {
        "4 KB pages only"
    };
    format!(
        "{} {}-bit, segments aligned to {} bytes ({pages})",
        machine_name(info.machine),
        if info.bits64 { 64 } else { 32 },
        info.load_align
    )
}

/// Check a library built for `abi`: right machine, and 16 KB alignment
/// where Android requires it.
fn check_library(abi: &Abi, lib: &Path) -> Result<ElfInfo, String> {
    let bytes = fs::read(lib).map_err(|e| format!("{}: {e}", lib.display()))?;
    let info = elf_info(&bytes).map_err(|e| format!("{}: {e}", lib.display()))?;
    check_elf_for(abi, &info).map_err(|e| format!("{}: {e}", lib.display()))?;
    Ok(info)
}

fn check_elf_for(abi: &Abi, info: &ElfInfo) -> Result<(), String> {
    if info.machine != abi.machine || info.bits64 != abi.bits64 {
        return Err(format!(
            "built for {} {}-bit, not for {}",
            machine_name(info.machine),
            if info.bits64 { 64 } else { 32 },
            abi.name
        ));
    }
    if abi.bits64 && info.load_align < PAGE_16K {
        return Err(format!(
            "segments are aligned to {} bytes; devices with 16 KB memory pages \
             need {PAGE_16K}",
            info.load_align
        ));
    }
    Ok(())
}

fn check_elf(files: &[String]) -> Result<(), String> {
    if files.is_empty() {
        return Err(format!("check-elf needs at least one file\n\n{USAGE}"));
    }
    let mut failed = false;
    for f in files {
        match fs::read(f)
            .map_err(|e| e.to_string())
            .and_then(|b| elf_info(&b))
        {
            Ok(info) => println!("{f}: {}", describe(&info)),
            Err(e) => {
                eprintln!("{f}: {e}");
                failed = true;
            }
        }
    }
    if failed {
        Err("some files could not be read as ELF".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn abi(name: &str) -> Abi {
        *ABIS.iter().find(|a| a.name == name).expect("known ABI")
    }

    #[test]
    fn android_options() {
        let o = parse_android(&[]).unwrap();
        assert!(o.release);
        assert_eq!(o.api, DEFAULT_API);
        assert_eq!(o.abis, vec![abi("arm64-v8a"), abi("x86_64")]);
        assert_eq!(o.ndk, None);

        let o = parse_android(&args(&[
            "--debug", "--abi", "all", "--api=30", "--ndk", "/opt/ndk",
        ]))
        .unwrap();
        assert!(!o.release);
        assert_eq!(o.api, 30);
        assert_eq!(o.abis.len(), 3);
        assert_eq!(o.ndk, Some(PathBuf::from("/opt/ndk")));

        let o = parse_android(&args(&["--abi=x86, aarch64-linux-android,x86"])).unwrap();
        assert_eq!(o.abis, vec![abi("x86"), abi("arm64-v8a")]);

        for bad in [
            &["--abi", "mips"][..],
            &["--abi", ","],
            &["--api", "new"],
            &["--api"],
            &["--fast"],
        ] {
            assert!(parse_android(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn link_arguments_name_the_api_level_and_16_kb_pages() {
        let a = link_args(&abi("armeabi-v7a"), 26);
        assert_eq!(a[0], "--target=armv7a-linux-androideabi26");
        assert!(a.contains(&"-Wl,-z,max-page-size=16384".to_string()));
        assert!(a.contains(&"-Wl,-soname,libcad_ffi.so".to_string()));
    }

    #[test]
    fn properties_files() {
        let text = "# written by Android Studio\n\
                    sdk.directory=/wrong\n\
                    sdk.dir=C\\:\\\\Users\\\\me\\\\AppData\\\\Local\\\\Android\\\\Sdk\n";
        assert_eq!(
            property(text, "sdk.dir").as_deref(),
            Some(r"C:\Users\me\AppData\Local\Android\Sdk")
        );
        assert_eq!(
            property(
                "Pkg.Desc = Android NDK\nPkg.Revision = 29.0.14206865\n",
                "Pkg.Revision"
            )
            .as_deref(),
            Some("29.0.14206865")
        );
        assert_eq!(property("# sdk.dir=/x\n", "sdk.dir"), None);
    }

    #[test]
    fn platforms_json() {
        let text = "{\n  \"min\": 21,\n  \"max\": 36,\n  \"aliases\": {\"20\": 19}\n}\n";
        assert_eq!(json_number(text, "min"), Some(21));
        assert_eq!(json_number(text, "max"), Some(36));
        assert_eq!(json_number(text, "other"), None);
    }

    #[test]
    fn versions_order_numerically() {
        assert!(version_key("29.0.14206865") > version_key("28.2.13676358"));
        assert!(version_key("27.10.1") > version_key("27.9.99"));
        assert_eq!(version_key("r27"), None);
    }

    /// A scratch folder, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let p = env::temp_dir().join(format!("xtask-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Scratch(p)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fake_ndk(dir: &Path) {
        let clang = clang_path(dir);
        fs::create_dir_all(clang.parent().unwrap()).unwrap();
        fs::write(clang, b"").unwrap();
    }

    #[test]
    fn the_newest_complete_ndk_is_chosen() {
        let s = Scratch::new("ndk");
        let ndk = s.0.join("ndk");
        fake_ndk(&ndk.join("27.3.13750724"));
        fake_ndk(&ndk.join("29.0.14206865"));
        fake_ndk(&ndk.join("not-a-version"));
        fs::create_dir_all(ndk.join("30.0.1")).unwrap(); // incomplete
        assert_eq!(newest_ndk(&ndk), Some(ndk.join("29.0.14206865")));
        assert!(check_ndk(&ndk.join("30.0.1")).is_err());
        assert_eq!(
            find_ndk(Some(&ndk.join("27.3.13750724")), &s.0),
            Ok(ndk.join("27.3.13750724"))
        );
        assert!(find_ndk(Some(&ndk.join("30.0.1")), &s.0).is_err());
        assert_eq!(newest_ndk(&s.0.join("missing")), None);
    }

    #[test]
    fn ndk_metadata() {
        let s = Scratch::new("meta");
        fs::create_dir_all(s.0.join("meta")).unwrap();
        fs::write(
            s.0.join("source.properties"),
            "Pkg.Revision = 28.2.13676358\n",
        )
        .unwrap();
        fs::write(
            s.0.join("meta/platforms.json"),
            "{\"min\": 21, \"max\": 35}",
        )
        .unwrap();
        assert_eq!(
            NdkMeta::read(&s.0),
            NdkMeta {
                revision: Some("28.2.13676358".into()),
                min_api: Some(21),
                max_api: Some(35),
            }
        );
        assert_eq!(NdkMeta::read(&s.0.join("missing")), NdkMeta::default());
    }

    /// A minimal ELF header followed by program headers, each `(type,
    /// align)`.
    fn elf(bits64: bool, machine: u16, segments: &[(u32, u64)]) -> Vec<u8> {
        let (ehsize, phentsize) = if bits64 { (64usize, 56usize) } else { (52, 32) };
        let mut b = vec![0u8; ehsize + phentsize * segments.len()];
        b[..4].copy_from_slice(b"\x7fELF");
        b[4] = if bits64 { 2 } else { 1 };
        b[5] = 1;
        b[0x12..0x14].copy_from_slice(&machine.to_le_bytes());
        if bits64 {
            b[0x20..0x28].copy_from_slice(&(ehsize as u64).to_le_bytes());
            b[0x36..0x38].copy_from_slice(&(phentsize as u16).to_le_bytes());
            b[0x38..0x3A].copy_from_slice(&(segments.len() as u16).to_le_bytes());
        } else {
            b[0x1C..0x20].copy_from_slice(&(ehsize as u32).to_le_bytes());
            b[0x2A..0x2C].copy_from_slice(&(phentsize as u16).to_le_bytes());
            b[0x2C..0x2E].copy_from_slice(&(segments.len() as u16).to_le_bytes());
        }
        for (i, &(ty, align)) in segments.iter().enumerate() {
            let at = ehsize + i * phentsize;
            b[at..at + 4].copy_from_slice(&ty.to_le_bytes());
            if bits64 {
                b[at + 48..at + 56].copy_from_slice(&align.to_le_bytes());
            } else {
                b[at + 28..at + 32].copy_from_slice(&(align as u32).to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn elf_headers() {
        // PT_PHDR (6) is ignored; the smallest PT_LOAD alignment counts.
        let info = elf_info(&elf(true, 183, &[(6, 8), (1, 16384), (1, 65536)])).unwrap();
        assert_eq!(
            info,
            ElfInfo {
                bits64: true,
                machine: 183,
                load_align: 16384
            }
        );
        assert!(check_elf_for(&abi("arm64-v8a"), &info).is_ok());
        assert!(
            check_elf_for(&abi("x86_64"), &info).is_err(),
            "wrong machine"
        );

        let small = elf_info(&elf(true, 183, &[(1, 4096), (1, 16384)])).unwrap();
        assert_eq!(small.load_align, 4096);
        let e = check_elf_for(&abi("arm64-v8a"), &small).unwrap_err();
        assert!(e.contains("16 KB"), "{e}");

        let arm = elf_info(&elf(false, 40, &[(1, 4096)])).unwrap();
        assert_eq!(
            arm,
            ElfInfo {
                bits64: false,
                machine: 40,
                load_align: 4096
            }
        );
        assert!(
            check_elf_for(&abi("armeabi-v7a"), &arm).is_ok(),
            "32-bit: 4 KB is fine"
        );

        assert!(elf_info(b"MZ\x90\x00").is_err());
        assert!(
            elf_info(&elf(true, 183, &[(1, 16384)])[..60]).is_err(),
            "truncated"
        );
        assert!(elf_info(&elf(true, 183, &[(6, 8)])).is_err(), "no PT_LOAD");
        let mut big_endian = elf(true, 183, &[(1, 16384)]);
        big_endian[5] = 2;
        assert!(elf_info(&big_endian).is_err());
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn a_real_elf_file_is_read() {
        let exe = fs::read(env::current_exe().unwrap()).unwrap();
        let info = elf_info(&exe).unwrap();
        assert_eq!(info.machine, 62);
        assert!(info.bits64);
        assert!(info.load_align.is_power_of_two());
    }
}

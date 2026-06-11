//! `--target clangd`: emit a clang compilation database (compile_commands.json)
//! from the same parsed project model the ninja backend uses.
//!
//! The entries are clang-cl flavored: cl.exe flags pass through where clang-cl
//! understands them, codegen-only flags are dropped, and a fixed compatibility
//! tail pins the MSVC 8.0 view (i686 triple, _MSC_VER=1400, C++98). Everything
//! environment-specific is DERIVED, not configured: system include dirs and
//! the stlport native-path define come from the `INCLUDE` env var (the same
//! one cl.exe resolves through), and the case-insensitive VFS overlay is
//! generated next to the output. `--extra-arg` remains as an escape hatch.
//!
//! Paths are emitted in native `/...` form: under `--wine` the build-graph
//! arithmetic produces `Z:\...` strings, which we lower back here (clangd runs
//! natively on the host, not under Wine).

use std::fmt::Write as _;
use std::path::Path;

use nom::{
    Parser,
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::{char, multispace0, multispace1},
    combinator::map,
    multi::{many0, many1},
    sequence::{delimited, preceded},
};

use crate::ninja::NinjaFile;
use crate::utils::to_host_str;

/// Flags that only affect codegen/PDB/PCH/driver output - meaningless for a
/// syntax-only clang view. Matched by prefix against the token (so the
/// attached-argument spellings `/Fp"..."`, `/Fo"..."` die with their flag).
const DROP_PREFIXES: &[&str] = &[
    "/Yc",
    "/Yu",
    "/Fp",
    "/Fo",
    "/Fd",
    "/FD",
    "/GL",
    "/GT",
    "/Gy",
    "/Zi",
    "/ZI",
    "/Zc:",
    "/errorReport",
    "/nologo",
    "/showIncludes",
    "/c",
];

/// Fixed clang-cl compatibility tail (see vostok docs: clangd spike notes).
/// `-imsvc` MUST stay a clang-cl-mode argument: forwarded through `/clang:` the
/// gcc-mode driver silently drops it.
const COMPAT_TAIL: &[&str] = &[
    "-fms-compatibility-version=14.00",
    "/clang:--target=i686-pc-windows-msvc",
    "/clang:-std=c++98",
    // navigation-only policy: clang's opinion of MSVC8 code is not build truth
    "-Wno-non-pod-varargs",
];

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// One classified cl argument.
enum ClArg {
    /// `/I<dir>`, `/I "<dir>"`, `/I"<dir>"`
    Include(String),
    /// `/D<def>`, `/D "<def>"`
    Define(String),
    /// anything else, quotes stripped
    Other(String),
}

/// One lexical token: a run of quoted and bare segments glued together
/// (`/Fp"a b.pch"` is ONE token `/Fpa b.pch`), matching cl's rsp quoting.
fn token(input: &str) -> nom::IResult<&str, String> {
    map(
        many1(alt((
            delimited(char('"'), take_while(|c| c != '"'), char('"')),
            take_while1(|c: char| !c.is_whitespace() && c != '"'),
        ))),
        |parts: Vec<&str>| parts.concat(),
    )
    .parse(input)
}

/// `/I` / `/D` value: attached (rest of this token) or detached (next token).
fn arg_value(input: &str) -> nom::IResult<&str, String> {
    alt((token, preceded(multispace1, token))).parse(input)
}

fn cl_arg(input: &str) -> nom::IResult<&str, ClArg> {
    alt((
        map(preceded(tag("/I"), arg_value), ClArg::Include),
        map(preceded(tag("/D"), arg_value), ClArg::Define),
        map(token, ClArg::Other),
    ))
    .parse(input)
}

/// Translate one cl rsp flag string into clang-cl arguments (without sources).
fn translate_flags(rsp_flags: &str, wine: bool) -> Vec<String> {
    let (_rest, parsed) = many0(preceded(multispace0, cl_arg))
        .parse(rsp_flags)
        .expect("many0 over tokens cannot fail");

    let mut args = vec![];
    for arg in parsed {
        match arg {
            ClArg::Include(dir) => args.push(format!("/I{}", to_host_str(&dir, wine))),
            ClArg::Define(def) => args.push(format!("/D{def}")),
            ClArg::Other(t) => {
                if DROP_PREFIXES.iter().any(|p| t.starts_with(p)) {
                    // codegen-only flag; attached-arg spellings die with it
                } else if t.ends_with(".cpp") || t.ends_with(".c") || t.ends_with(".cc") {
                    // sources are appended per-entry by the caller
                } else {
                    args.push(t);
                }
            }
        }
    }
    args
}

/// System include dirs, from the same `INCLUDE` environment variable cl.exe
/// itself resolves system headers through (vcvars on Windows, the Wine prefix
/// registry env here) - no caller configuration needed.
fn include_env_dirs(wine: bool) -> Vec<String> {
    std::env::var("INCLUDE")
        .unwrap_or_default()
        .split(';')
        .filter(|d| !d.trim().is_empty())
        .map(|d| to_host_str(d, wine))
        .collect()
}

/// File kinds that can never be `#include`d - keep the overlay lean.
const OVERLAY_SKIP_EXT: &[&str] = &[
    "cpp", "c", "cc", "obj", "lib", "pdb", "vcproj", "sln", "rc", "ico", "bmp", "png", "jpg",
    "txt", "md", "py", "cmake", "bat", "exe", "dll",
];

/// Write a case-insensitive VFS overlay over the system include dirs and the
/// source tree. Wine's filesystem view is case-insensitive, so the sources
/// freely mix include/file case (`<fastdelegate/fastdelegate.h>` vs
/// `FastDelegate.h`); on a case-sensitive host clang needs this overlay to
/// resolve them. Returns the number of files mapped.
fn write_vfs_overlay(
    overlay_path: &Path,
    include_dirs: &[String],
    source_root: &Path,
) -> anyhow::Result<usize> {
    let mut out = String::from("{\"version\": 0, \"case-sensitive\": \"false\", \"roots\": [");
    let mut nfiles = 0usize;
    let mut ndirs = 0usize;

    let mut stack: Vec<std::path::PathBuf> = include_dirs
        .iter()
        .map(std::path::PathBuf::from)
        .chain([source_root.to_path_buf()])
        .collect();
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut contents = String::new();
        let mut first = true;
        for entry in rd.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if path.is_dir() {
                if name != ".git" {
                    stack.push(path);
                }
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if OVERLAY_SKIP_EXT.contains(&ext.to_ascii_lowercase().as_str()) {
                continue;
            }
            if !first {
                contents.push_str(", ");
            }
            first = false;
            write!(
                contents,
                "{{\"name\": \"{}\", \"type\": \"file\", \"external-contents\": \"{}\"}}",
                json_escape(name),
                json_escape(path.to_str().unwrap_or_default()),
            )?;
            nfiles += 1;
        }
        if !contents.is_empty() {
            if ndirs > 0 {
                out.push_str(", ");
            }
            write!(
                out,
                "{{\"name\": \"{}\", \"type\": \"directory\", \"contents\": [{contents}]}}",
                json_escape(dir.to_str().unwrap_or_default()),
            )?;
            ndirs += 1;
        }
    }
    out.push_str("]}");
    std::fs::write(overlay_path, out)
        .map_err(|e| anyhow::anyhow!("writing '{}': {e}", overlay_path.display()))?;
    Ok(nfiles)
}

/// Render compile_commands.json for every cl group of every project, plus the
/// VFS overlay next to it (under `--wine`; a case-insensitive host filesystem
/// is assumed otherwise). Everything environment-specific is derived: system
/// include dirs and the stlport native-header pin come from `INCLUDE`.
/// Returns the number of TU entries written.
pub fn write_compile_commands(
    ninja_files: &[(uuid::Uuid, String, NinjaFile)],
    out_path: &Path,
    source_root: &Path,
    wine: bool,
    extra_args: &[String],
) -> anyhow::Result<usize> {
    let include_dirs = include_env_dirs(wine);
    if include_dirs.is_empty() {
        eprintln!("warning: INCLUDE is empty - system headers (windows.h, CRT) will not resolve");
    }

    let mut derived: Vec<String> = include_dirs.iter().map(|d| format!("-imsvc{d}")).collect();
    // stlport resolves native headers via `<$(path)/header)>`; its MSVC default
    // `../include` only works relative to the VC include dir - pin it absolute.
    // By vcvars convention the VC include dir is the first INCLUDE entry.
    if let Some(vc_include) = include_dirs.first() {
        derived.push(format!("/D_STLP_NATIVE_INCLUDE_PATH={vc_include}"));
    }
    if wine {
        let overlay_path = out_path.with_file_name("clangd-vfs.yaml");
        let n = write_vfs_overlay(&overlay_path, &include_dirs, source_root)?;
        eprintln!(
            "[compdb] vfs overlay: {n} files -> '{}'",
            overlay_path.display()
        );
        derived.push(format!(
            "/clang:-ivfsoverlay{}",
            overlay_path.to_str().unwrap_or_default()
        ));
    }

    let mut entries = 0usize;
    let mut out = String::from("[\n");

    for (_guid, _name, nf) in ninja_files {
        let dir = to_host_str(&nf.proj_dir, wine);
        for group in &nf.cl {
            let flags = translate_flags(&group.flags.rsp_flags, wine);
            for src in &group.flags.files {
                let src = to_host_str(src, wine);
                let src = src.trim_start_matches("./");
                let file = if src.starts_with('/') {
                    src.to_string()
                } else {
                    format!("{dir}/{src}")
                };

                if entries > 0 {
                    out.push_str(",\n");
                }
                write!(out, " {{\"directory\": \"{}\",", json_escape(&dir))?;
                write!(out, " \"file\": \"{}\",", json_escape(&file))?;
                out.push_str(" \"arguments\": [\"clang-cl\"");
                for a in flags
                    .iter()
                    .map(String::as_str)
                    .chain(COMPAT_TAIL.iter().copied())
                    .chain(derived.iter().map(String::as_str))
                    .chain(extra_args.iter().map(String::as_str))
                    .chain(["/c", file.as_str()])
                {
                    write!(out, ", \"{}\"", json_escape(a))?;
                }
                out.push_str("]}");
                entries += 1;
            }
        }
    }
    out.push_str("\n]\n");

    std::fs::write(out_path, out)
        .map_err(|e| anyhow::anyhow!("writing '{}': {e}", out_path.display()))?;
    Ok(entries)
}

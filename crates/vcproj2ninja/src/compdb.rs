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

use crate::ninja::NinjaFile;

/// Flags that only affect codegen/PDB/PCH output - meaningless or harmful for
/// a syntax-only clang view. Matched by prefix against the token (so the
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
    "/W1",
    "/W2",
    "/W3",
    "/W4",
    "/WX",
    "/wd",
    "/errorReport",
    "/nologo",
    "/analyze",
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

/// Lower a possibly Wine-rooted (`Z:\...`) string to native form.
fn wine_to_unix(p: &str) -> String {
    let p = p.replace('\\', "/");
    match p.strip_prefix("Z:").or_else(|| p.strip_prefix("z:")) {
        Some(rest) => rest.to_string(),
        None => p,
    }
}

/// Tokenize an rsp flag string, honoring double quotes (`/I "a b"` is two
/// tokens, the second being `a b` unquoted).
fn tokenize(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in s.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Translate one cl rsp flag string into clang-cl arguments (without sources).
fn translate_flags(rsp_flags: &str) -> Vec<String> {
    let toks = tokenize(rsp_flags);
    let mut args = vec![];
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        if let Some(rest) = t.strip_prefix("/I") {
            let dir = if rest.is_empty() {
                i += 1;
                toks.get(i).cloned().unwrap_or_default()
            } else {
                rest.to_string()
            };
            args.push(format!("/I{}", wine_to_unix(&dir)));
        } else if let Some(rest) = t.strip_prefix("/D") {
            let def = if rest.is_empty() {
                i += 1;
                toks.get(i).cloned().unwrap_or_default()
            } else {
                rest.to_string()
            };
            args.push(format!("/D{def}"));
        } else if DROP_PREFIXES.iter().any(|p| t.starts_with(p)) {
            // attached-arg spellings die here; the rsps use no detached ones
            // for the dropped set.
        } else if t.ends_with(".cpp") || t.ends_with(".c") || t.ends_with(".cc") {
            // sources are appended per-entry by the caller
        } else {
            args.push(t.clone());
        }
        i += 1;
    }
    args
}

/// System include dirs, from the same `INCLUDE` environment variable cl.exe
/// itself resolves system headers through (vcvars on Windows, the Wine prefix
/// registry env here) - no caller configuration needed.
fn include_env_dirs() -> Vec<String> {
    std::env::var("INCLUDE")
        .unwrap_or_default()
        .split(';')
        .filter(|d| !d.trim().is_empty())
        .map(wine_to_unix)
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
    let include_dirs = include_env_dirs();
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
        let dir = wine_to_unix(&nf.proj_dir);
        for group in &nf.cl {
            let flags = translate_flags(&group.flags.rsp_flags);
            for src in &group.flags.files {
                let src = wine_to_unix(src);
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

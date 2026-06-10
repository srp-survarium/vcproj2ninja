//! `--target clangd`: emit a clang compilation database (compile_commands.json)
//! from the same parsed project model the ninja backend uses.
//!
//! The entries are clang-cl flavored: cl.exe flags pass through where clang-cl
//! understands them, codegen-only flags are dropped, and a fixed compatibility
//! tail pins the MSVC 8.0 view (i686 triple, _MSC_VER=1400, C++98). Everything
//! environment-specific (VC8 CRT / WinSDK include dirs, the case-insensitive
//! VFS overlay, the stlport native-path define) is supplied by the caller via
//! `--imsvc` / `--extra-arg` - this tool stays installation-agnostic.
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

/// Render compile_commands.json for every cl group of every project.
/// Returns the number of TU entries written.
pub fn write_compile_commands(
    ninja_files: &[(uuid::Uuid, String, NinjaFile)],
    out_path: &Path,
    imsvc: &[String],
    extra_args: &[String],
) -> anyhow::Result<usize> {
    let imsvc_args: Vec<String> = imsvc.iter().map(|d| format!("-imsvc{d}")).collect();
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
                    .chain(imsvc_args.iter().map(String::as_str))
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

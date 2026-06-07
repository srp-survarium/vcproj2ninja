use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

use vs2008_parser_lib::vcproj::{ClGroup, Flags};

pub enum FinalStep {
    /// Static lib: lib then link /LIB, both sharing the same rsp.
    Lib(Flags),
    /// Exe/dll: link only.
    Link(Flags),
}

pub struct NinjaFile {
    pub cl: Vec<ClGroup>,
    pub final_step: FinalStep,
    /// Absolute path to the vcproj directory; commands cd here before running.
    pub proj_dir: String,
    /// Output file paths of sln-level dependencies.
    pub depends_on: Vec<String>,
}

/// All text and rsp files produced for one project.
pub struct NinjaOutput {
    pub ninja_text: String,
    /// (path, content) pairs — caller writes these to disk.
    pub rsp_files: Vec<(PathBuf, String)>,
    /// Directories that must exist before the build runs (PDB parent dirs, etc.).
    pub required_dirs: Vec<PathBuf>,
}

/// A single ninja build statement. Paths are stored normalized but unescaped;
/// `render` applies ninja syntax escaping.
pub struct NinjaBuildStatement {
    pub outputs: Vec<String>,
    pub implicit_outputs: Vec<String>,
    pub rule: &'static str,
    pub inputs: Vec<String>,
    /// Implicit inputs (`|`): trigger rebuilds but are not passed as command arguments.
    /// Use for prerequisites baked into flags (dep libs, pch files).
    pub implicit_inputs: Vec<String>,
    pub order_only_deps: Vec<String>,
    /// The `flags = ...` binding. `None` for rules that carry no flags (e.g. phony).
    pub flags: Option<String>,
    /// Ninja pool name. Steps in the same pool with `depth = 1` never run in parallel.
    pub pool: Option<String>,
}

impl NinjaBuildStatement {
    fn render(&self, out: &mut impl FmtWrite) -> std::fmt::Result {
        let Self {
            outputs,
            implicit_outputs,
            rule,
            inputs,
            implicit_inputs,
            order_only_deps,
            flags,
            pool,
        } = self;

        if outputs.is_empty() {
            return Ok(());
        }

        writeln!(out, "build $")?;
        for o in outputs {
            writeln!(out, "    {} $", ninja_escape(o))?;
        }
        // Implicit outputs: `|` appears once before the first item.
        for (i, o) in implicit_outputs.iter().enumerate() {
            if i == 0 {
                writeln!(out, "    | {} $", ninja_escape(o))?;
            } else {
                writeln!(out, "      {} $", ninja_escape(o))?;
            }
        }

        let has_inputs = !inputs.is_empty();
        let has_implicit = !implicit_inputs.is_empty();
        let has_oo = !order_only_deps.is_empty();

        if !has_inputs && !has_implicit && !has_oo {
            writeln!(out, "    : {rule}")?;
        } else {
            writeln!(out, "    : {rule} $")?;
            let last_explicit = inputs.len().saturating_sub(1);
            for (i, inp) in inputs.iter().enumerate() {
                if i < last_explicit || has_implicit || has_oo {
                    writeln!(out, "    {} $", ninja_escape(inp))?;
                } else {
                    writeln!(out, "    {}", ninja_escape(inp))?;
                }
            }
            // Implicit inputs: `|` appears once before the first item.
            let last_implicit = implicit_inputs.len().saturating_sub(1);
            for (i, imp) in implicit_inputs.iter().enumerate() {
                let prefix = if i == 0 { "    | " } else { "      " };
                if i < last_implicit || has_oo {
                    writeln!(out, "{}{} $", prefix, ninja_escape(imp))?;
                } else {
                    writeln!(out, "{}{}", prefix, ninja_escape(imp))?;
                }
            }
            if has_oo {
                write!(out, "    ||")?;
                for dep in order_only_deps {
                    write!(out, " {}", ninja_escape(dep))?;
                }
                writeln!(out)?;
            }
        }

        if let Some(flags) = flags {
            writeln!(out, "  flags = {flags}")?;
        }
        if let Some(pool) = pool {
            writeln!(out, "  pool = {pool}")?;
        }
        writeln!(out)
    }
}

impl NinjaFile {
    pub fn write(&self, stem: &str, rsp_dir: &Path) -> NinjaOutput {
        let Self {
            cl,
            final_step,
            proj_dir,
            depends_on,
        } = self;
        let mut statements: Vec<NinjaBuildStatement> = vec![];
        let mut rsp_files: Vec<(PathBuf, String)> = vec![];
        let mut required_dirs: Vec<PathBuf> = vec![];

        for (i, group) in cl.iter().enumerate() {
            let ClGroup {
                flags,
                pch_output,
                pch_input,
                fd_path,
                header_deps,
                ..
            } = group;

            let rsp_path = rsp_dir.join(format!("{stem}_cl_{i}.rsp"));

            if !flags.files.is_empty() {
                let outputs: Vec<String> = flags
                    .files
                    .iter()
                    .map(|src| normalize_path(&compute_obj(&flags.output_file, src)))
                    .collect();

                let implicit_outputs: Vec<String> = pch_output
                    .as_deref()
                    .map(|p| normalize_path(p.to_str().expect("pch path is UTF-8")))
                    .into_iter()
                    .collect();

                let inputs: Vec<String> = flags
                    .files
                    .iter()
                    .map(|src| normalize_rpath(&proj_dir, src))
                    .collect();

                let mut implicit_inputs: Vec<String> = pch_input
                    .as_deref()
                    .map(|p| normalize_path(p.to_str().expect("pch dep is UTF-8")))
                    .into_iter()
                    .collect();
                // Header dependencies discovered by the preprocessor: edits to
                // any of them must force this cl group to recompile. They are
                // already in ninja-space form (see main.rs), so pass them through.
                implicit_inputs.extend(header_deps.iter().cloned());

                let flag_str = flags
                    .flags
                    .replace("$(RspFile)", rsp_path.to_str().unwrap());

                let pool = fd_path.as_deref().map(fd_pool_name);

                // Collect parent directory of the /Fd PDB so the caller can pre-create it.
                if let Some(fd) = fd_path.as_deref() {
                    if let Some(parent) = Path::new(fd).parent() {
                        required_dirs.push(parent.to_path_buf());
                    }
                }

                statements.push(NinjaBuildStatement {
                    outputs,
                    implicit_outputs,
                    rule: "cl",
                    inputs,
                    implicit_inputs,
                    order_only_deps: vec![],
                    flags: Some(flag_str),
                    pool,
                });
            }

            rsp_files.push((rsp_path, flags.rsp_file_content()));
        }

        let output_file = match &final_step {
            FinalStep::Lib(flags) => {
                let rsp_path = rsp_dir.join(format!("{stem}_lib.rsp"));
                statements.push(build_final_statement(
                    "lib",
                    flags,
                    &rsp_path,
                    &proj_dir,
                    &depends_on,
                ));
                rsp_files.push((rsp_path, flags.rsp_file_content()));
                &flags.output_file
            }
            FinalStep::Link(flags) => {
                let rsp_path = rsp_dir.join(format!("{stem}_link.rsp"));
                statements.push(build_final_statement(
                    "link",
                    flags,
                    &rsp_path,
                    &proj_dir,
                    &depends_on,
                ));
                rsp_files.push((rsp_path, flags.rsp_file_content()));
                &flags.output_file
            }
        };

        // Also ensure the directory for the final output file (lib/exe) exists.
        if let Some(parent) = Path::new(output_file).parent() {
            required_dirs.push(parent.to_path_buf());
        }

        statements.push(NinjaBuildStatement {
            outputs: vec![stem.to_string()],
            implicit_outputs: vec![],
            rule: "phony",
            inputs: vec![output_file.clone()],
            implicit_inputs: vec![],
            order_only_deps: vec![],
            flags: None,
            pool: None,
        });

        let mut out = String::new();
        writeln!(out, "proj_dir = {}", proj_dir).unwrap();
        writeln!(out).unwrap();

        // Declare one pool per unique /Fd path so parallel cl steps don't race on the PDB.
        let mut seen_pools: std::collections::HashSet<String> = std::collections::HashSet::new();
        for ClGroup { fd_path, .. } in cl {
            if let Some(fd_path) = fd_path {
                let name = fd_pool_name(fd_path);
                if seen_pools.insert(name.clone()) {
                    writeln!(out, "pool {name}").unwrap();
                    writeln!(out, "  depth = 1").unwrap();
                    writeln!(out).unwrap();
                }
            }
        }

        write_rules(&mut out).unwrap();
        for stmt in &statements {
            stmt.render(&mut out).unwrap();
        }

        NinjaOutput {
            ninja_text: out,
            rsp_files,
            required_dirs,
        }
    }
}

/// Normalize an absolute path string (resolve `..`, no ninja escaping).
fn normalize_path(path: &str) -> String {
    normalize_rpath("", path)
}

/// Join `base_path` with a relative path and normalize the result.
fn normalize_rpath(base_path: &str, rpath: &str) -> String {
    Path::new(base_path)
        .join(rpath)
        .normalize_lexically()
        .expect("base_path must be absolute")
        .to_str()
        .expect("path is valid UTF-8")
        .to_string()
}

/// Escape a path for use in a ninja build statement.
fn ninja_escape(path: &str) -> String {
    path.replace('$', "$$")
        .replace(' ', "$ ")
        .replace(':', "$:")
}

fn write_rules(out: &mut impl FmtWrite) -> std::fmt::Result {
    // @TODO: cd /d is a proper solution. On my system cd is overriden by zoxide.
    // So we need to fix zoxide cd override first.
    const _WRITE_RULES: &str = r#"
rule cl
  command = cd /d "$proj_dir" && cl $flags

rule lib
  command = cd /d "$proj_dir" && lib $flags

rule link
  command = cd /d "$proj_dir" && link $flags

"#;

    const WRITE_RULES: &str = r#"
rule cl
  command = cmd /c cd "$proj_dir" && cl $flags

rule lib
  command = cmd /c cd "$proj_dir" && lib $flags

rule link
  command = cmd /c cd "$proj_dir" && link $flags

"#;

    writeln!(out, "{}", WRITE_RULES)
}

fn compute_obj(output_file: &str, src: &str) -> String {
    if Path::new(output_file)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("obj"))
    {
        output_file.to_string()
    } else {
        let sep = if output_file.ends_with(['\\', '/']) {
            ""
        } else {
            "\\"
        };
        let stem = Path::new(src)
            .file_stem()
            .expect("source has stem")
            .to_str()
            .expect("valid UTF-8");
        format!("{output_file}{sep}{stem}.obj")
    }
}

fn fd_pool_name(fd_path: &str) -> String {
    fd_path
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

fn build_final_statement(
    rule: &'static str,
    flags: &Flags,
    rsp_path: &Path,
    proj_dir: &str,
    depends_on: &[String],
) -> NinjaBuildStatement {
    let mut outputs = vec![normalize_path(&flags.output_file)];
    if let Some(import_lib) = &flags.import_library {
        outputs.push(normalize_path(import_lib));
    }

    let flag_str = flags
        .flags
        .replace("$(RspFile)", rsp_path.to_str().unwrap());

    NinjaBuildStatement {
        outputs,
        implicit_outputs: vec![],
        rule,
        inputs: flags
            .files
            .iter()
            .map(|f| normalize_rpath(proj_dir, f))
            .collect(),
        implicit_inputs: depends_on.iter().map(|d| normalize_path(d)).collect(),
        order_only_deps: vec![],
        flags: Some(flag_str),
        pool: None,
    }
}

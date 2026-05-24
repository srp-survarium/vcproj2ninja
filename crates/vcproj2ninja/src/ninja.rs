use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

use vs2008_parser_lib::vcproj::{Flags, FlagsTree};

pub enum FinalStep {
    /// Static lib: lib then link /LIB, both sharing the same rsp.
    Lib(Flags),
    /// Exe/dll: link only.
    Link(Flags),
}

pub struct NinjaFile {
    pub cl: Vec<FlagsTree>,
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
}

impl NinjaBuildStatement {
    fn render(&self, out: &mut impl FmtWrite) -> std::fmt::Result {
        if self.outputs.is_empty() {
            return Ok(());
        }

        writeln!(out, "build $")?;
        for o in &self.outputs {
            writeln!(out, "    {} $", ninja_escape(o))?;
        }
        // Implicit outputs: `|` appears once before the first item.
        for (i, o) in self.implicit_outputs.iter().enumerate() {
            if i == 0 {
                writeln!(out, "    | {} $", ninja_escape(o))?;
            } else {
                writeln!(out, "      {} $", ninja_escape(o))?;
            }
        }

        let has_inputs = !self.inputs.is_empty();
        let has_implicit = !self.implicit_inputs.is_empty();
        let has_oo = !self.order_only_deps.is_empty();

        if !has_inputs && !has_implicit && !has_oo {
            writeln!(out, "    : {}", self.rule)?;
        } else {
            writeln!(out, "    : {} $", self.rule)?;
            let last_explicit = self.inputs.len().saturating_sub(1);
            for (i, inp) in self.inputs.iter().enumerate() {
                if i < last_explicit || has_implicit || has_oo {
                    writeln!(out, "    {} $", ninja_escape(inp))?;
                } else {
                    writeln!(out, "    {}", ninja_escape(inp))?;
                }
            }
            // Implicit inputs: `|` appears once before the first item.
            let last_implicit = self.implicit_inputs.len().saturating_sub(1);
            for (i, imp) in self.implicit_inputs.iter().enumerate() {
                let prefix = if i == 0 { "    | " } else { "      " };
                if i < last_implicit || has_oo {
                    writeln!(out, "{}{} $", prefix, ninja_escape(imp))?;
                } else {
                    writeln!(out, "{}{}", prefix, ninja_escape(imp))?;
                }
            }
            if has_oo {
                write!(out, "    ||")?;
                for dep in &self.order_only_deps {
                    write!(out, " {}", ninja_escape(dep))?;
                }
                writeln!(out)?;
            }
        }

        if let Some(flags) = &self.flags {
            writeln!(out, "  flags = {flags}")?;
        }
        writeln!(out)
    }
}

impl NinjaFile {
    pub fn write(&self, stem: &str, rsp_dir: &Path) -> NinjaOutput {
        let mut statements: Vec<NinjaBuildStatement> = vec![];
        let mut rsp_files: Vec<(PathBuf, String)> = vec![];

        let mut counter = 0usize;
        for tree in &self.cl {
            collect_cl_tree(
                tree,
                rsp_dir,
                stem,
                &mut counter,
                &self.proj_dir,
                &mut statements,
                &mut rsp_files,
                None,
            );
        }

        let output_file = match &self.final_step {
            FinalStep::Lib(flags) => {
                let rsp_path = rsp_dir.join(format!("{stem}_lib.rsp"));
                statements.push(build_final_statement(
                    "lib",
                    flags,
                    &rsp_path,
                    &self.proj_dir,
                    &self.depends_on,
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
                    &self.proj_dir,
                    &self.depends_on,
                ));
                rsp_files.push((rsp_path, flags.rsp_file_content()));
                &flags.output_file
            }
        };

        statements.push(NinjaBuildStatement {
            outputs: vec![stem.to_string()],
            implicit_outputs: vec![],
            rule: "phony",
            inputs: vec![output_file.clone()],
            implicit_inputs: vec![],
            order_only_deps: vec![],
            flags: None,
        });

        let mut out = String::new();
        writeln!(out, "proj_dir = {}", self.proj_dir).unwrap();
        writeln!(out).unwrap();
        write_rules(&mut out).unwrap();
        for stmt in &statements {
            stmt.render(&mut out).unwrap();
        }

        NinjaOutput {
            ninja_text: out,
            rsp_files,
        }
    }
}

/// Normalize an absolute path string (resolve `..`, no ninja escaping).
fn normalize_path(path: &str) -> String {
    normalize_rpath("", path)
}

/// Join `base_dir` with a relative path and normalize the result.
fn normalize_rpath(base_dir: &str, rel_path: &str) -> String {
    Path::new(base_dir)
        .join(rel_path)
        .normalize_lexically()
        .expect("base_dir must be absolute")
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

#[allow(clippy::too_many_arguments)]
fn collect_cl_tree(
    tree: &FlagsTree,
    rsp_dir: &Path,
    stem: &str,
    counter: &mut usize,
    proj_dir: &str,
    statements: &mut Vec<NinjaBuildStatement>,
    rsp_files: &mut Vec<(PathBuf, String)>,
    depends_on_pch: Option<&Path>,
) {
    let i = *counter;
    *counter += 1;
    let rsp_path = rsp_dir.join(format!("{stem}_cl_{i}.rsp"));

    let flags = &tree.flags;
    let pch_implicit_out = tree.dependants.first().map(|(_, p)| p.as_path());

    if !flags.files.is_empty() {
        let outputs: Vec<String> = flags
            .files
            .iter()
            .map(|src| normalize_path(&compute_obj(&flags.output_file, src)))
            .collect();

        let implicit_outputs: Vec<String> = pch_implicit_out
            .map(|p| normalize_path(p.to_str().expect("pch path is UTF-8")))
            .into_iter()
            .collect();

        let inputs: Vec<String> = flags
            .files
            .iter()
            .map(|src| normalize_rpath(proj_dir, src))
            .collect();

        let implicit_inputs: Vec<String> = depends_on_pch
            .map(|p| normalize_path(p.to_str().expect("pch dep is UTF-8")))
            .into_iter()
            .collect();

        let flag_str = flags
            .flags
            .replace("$(RspFile)", rsp_path.to_str().unwrap());

        statements.push(NinjaBuildStatement {
            outputs,
            implicit_outputs,
            rule: "cl",
            inputs,
            implicit_inputs,
            order_only_deps: vec![],
            flags: Some(flag_str),
        });
    }

    rsp_files.push((rsp_path, flags.rsp_file_content()));

    for (dep_tree, pch_path) in &tree.dependants {
        collect_cl_tree(
            dep_tree,
            rsp_dir,
            stem,
            counter,
            proj_dir,
            statements,
            rsp_files,
            Some(pch_path),
        );
    }
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
    }
}

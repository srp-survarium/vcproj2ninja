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
}

/// All text and rsp files produced for one project.
pub struct NinjaOutput {
    pub ninja_text: String,
    /// (path, content) pairs — caller writes these to disk.
    pub rsp_files: Vec<(PathBuf, String)>,
}

impl NinjaFile {
    pub fn write(&self, stem: &str, rsp_dir: &Path) -> NinjaOutput {
        let mut out = String::new();
        let mut rsp_files: Vec<(PathBuf, String)> = vec![];

        writeln!(out, "proj_dir = {}", self.proj_dir).unwrap();
        writeln!(out).unwrap();
        write_rules(&mut out).unwrap();

        let mut counter = 0usize;
        for tree in &self.cl {
            write_cl_tree(&mut out, tree, rsp_dir, stem, &mut counter, &self.proj_dir, &mut rsp_files, None).unwrap();
        }

        let output_file = match &self.final_step {
            FinalStep::Lib(flags) => {
                let rsp_path = rsp_dir.join(format!("{stem}_lib.rsp"));
                write_final(&mut out, "lib", flags, &rsp_path, &self.proj_dir).unwrap();
                rsp_files.push((rsp_path, flags.rsp_file_content()));
                &flags.output_file
            }

            FinalStep::Link(flags) => {
                let rsp_path = rsp_dir.join(format!("{stem}_link.rsp"));
                write_final(&mut out, "link", flags, &rsp_path, &self.proj_dir).unwrap();
                rsp_files.push((rsp_path, flags.rsp_file_content()));
                &flags.output_file
            }
        };

        writeln!(out, "build {stem}: phony {}", ninja_path("", output_file)).unwrap();

        NinjaOutput {
            ninja_text: out,
            rsp_files,
        }
    }
}

/// Join `base` and `rel`, normalize away `.`/`..`, and escape for a ninja build statement.
/// Pass `""` as `base` when `rel` is already an absolute path.
fn ninja_path(dir_path: &str, file_rpath: &str) -> String {
    let file_path = Path::new(dir_path)
        .join(file_rpath)
        .normalize_lexically()
        .expect("dir_path to be absolute");

    file_path
        .to_str()
        .expect("path is valid UTF-8")
        .replace('$', "$$")
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
        let sep = if output_file.ends_with(['\\', '/']) { "" } else { "\\" };
        let stem = Path::new(src)
            .file_stem()
            .expect("source has stem")
            .to_str()
            .expect("valid UTF-8");
        format!("{output_file}{sep}{stem}.obj")
    }
}

#[allow(clippy::too_many_arguments)]
fn write_cl_tree(
    out: &mut impl FmtWrite,
    tree: &FlagsTree,
    rsp_dir: &Path,
    stem: &str,
    counter: &mut usize,
    proj_dir: &str,
    rsp_files: &mut Vec<(PathBuf, String)>,
    order_only_dep: Option<&std::path::Path>,
) -> std::fmt::Result {
    let i = *counter;
    *counter += 1;
    let rsp_path = rsp_dir.join(format!("{stem}_cl_{i}.rsp"));

    let implicit_out = tree.dependants.first().map(|(_, p)| p.as_path());

    write_cl_node(out, &tree.flags, &rsp_path, proj_dir, implicit_out, order_only_dep)?;
    rsp_files.push((rsp_path, tree.flags.rsp_file_content()));

    for (dep_tree, pch_path) in &tree.dependants {
        write_cl_tree(out, dep_tree, rsp_dir, stem, counter, proj_dir, rsp_files, Some(pch_path))?;
    }

    Ok(())
}

fn write_cl_node(
    out: &mut impl FmtWrite,
    flags: &Flags,
    rsp_path: &Path,
    proj_dir: &str,
    implicit_out: Option<&std::path::Path>,
    order_only_dep: Option<&std::path::Path>,
) -> std::fmt::Result {
    let Flags { output_file, flags, rsp_flags: _, files } = flags;

    if files.is_empty() {
        return Ok(());
    }

    writeln!(out, "build $")?;
    for src in files {
        writeln!(out, "    {} $", ninja_path("", &compute_obj(output_file, src)))?;
    }
    if let Some(p) = implicit_out {
        writeln!(out, "    | {} $", ninja_path("", p.to_str().expect("pch path is UTF-8")))?;
    }

    write!(out, "    : cl")?;
    for src in files {
        write!(out, " {}", ninja_path(proj_dir, src))?;
    }
    if let Some(dep) = order_only_dep {
        write!(out, " || {}", ninja_path("", dep.to_str().expect("pch dep is UTF-8")))?;
    }
    writeln!(out)?;

    let flags = flags.replace("$(RspFile)", rsp_path.to_str().unwrap());
    writeln!(out, "  flags = {flags}")?;
    writeln!(out)
}

fn write_final(
    out: &mut impl FmtWrite,
    rule: &str,
    flags: &Flags,
    rsp_path: &Path,
    proj_dir: &str,
) -> std::fmt::Result {
    let Flags {
        output_file,
        flags,
        rsp_flags: _,
        files,
    } = flags;

    writeln!(out, "build $")?;
    writeln!(out, "    {} $", ninja_path("", output_file))?;
    if files.is_empty() {
        writeln!(out, "    : {rule}")?;
    } else {
        writeln!(out, "    : {rule} $")?;
        let last = files.len() - 1;
        for (i, file) in files.iter().enumerate() {
            if i < last {
                writeln!(out, "    {} $", ninja_path(proj_dir, file))?;
            } else {
                writeln!(out, "    {}", ninja_path(proj_dir, file))?;
            }
        }
    }

    let flags = flags.replace("$(RspFile)", rsp_path.to_str().unwrap());
    writeln!(out, "  flags = {flags}")?;
    writeln!(out)
}

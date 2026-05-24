use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

use vs2008_parser_lib::vcproj::Flags;

pub enum FinalStep {
    /// Static lib: lib then link /LIB, both sharing the same rsp.
    Lib(Flags),
    /// Exe/dll: link only.
    Link(Flags),
}

pub struct NinjaFile {
    pub cl: Vec<Flags>,
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

        for (i, cl) in self.cl.iter().enumerate() {
            let rsp_path = rsp_dir.join(format!("{stem}_cl_{i}.rsp"));
            write_cl(&mut out, cl, &rsp_path, &self.proj_dir).unwrap();
            rsp_files.push((rsp_path, cl.rsp_file_content()));
        }

        match &self.final_step {
            FinalStep::Lib(flags) => {
                let rsp_path = rsp_dir.join(format!("{stem}_lib.rsp"));
                write_final(&mut out, "lib_link", flags, &rsp_path, &self.proj_dir).unwrap();
                rsp_files.push((rsp_path, flags.rsp_file_content()));
            }

            FinalStep::Link(flags) => {
                let rsp_path = rsp_dir.join(format!("{stem}_link.rsp"));
                write_final(&mut out, "link", flags, &rsp_path, &self.proj_dir).unwrap();
                rsp_files.push((rsp_path, flags.rsp_file_content()));
            }
        }

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

rule lib_link
  command = cd /d "$proj_dir" && lib $flags && link $flags

rule link
  command = cd /d "$proj_dir" && link $flags

"#;

    const WRITE_RULES: &str = r#"
rule cl
  command = cmd /c cd "$proj_dir" && cl $flags

rule lib_link
  command = cmd /c cd "$proj_dir" && lib $flags && link $flags

rule link
  command = cmd /c cd "$proj_dir" && link $flags

"#;

    writeln!(out, "{}", WRITE_RULES)
}

fn write_cl(
    out: &mut impl FmtWrite,
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
    for src in files {
        let obj_file = {
            if Path::new(output_file)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("obj"))
            {
                output_file.clone()
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
        };
        writeln!(out, "    {} $", ninja_path("", &obj_file))?;
    }

    write!(out, "    : cl")?;
    for src in files {
        write!(out, " {}", ninja_path(proj_dir, src))?;
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

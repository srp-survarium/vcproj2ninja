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
            rsp_files.push((rsp_path, rsp_file_content(cl)));
        }

        match &self.final_step {
            FinalStep::Lib(flags) => {
                let rsp_path = rsp_dir.join(format!("{stem}_lib.rsp"));
                write_final(&mut out, "lib_link", flags, &rsp_path, &self.proj_dir).unwrap();
                rsp_files.push((rsp_path, rsp_file_content(flags)));
            }
            FinalStep::Link(flags) => {
                let rsp_path = rsp_dir.join(format!("{stem}_link.rsp"));
                write_final(&mut out, "link", flags, &rsp_path, &self.proj_dir).unwrap();
                rsp_files.push((rsp_path, rsp_file_content(flags)));
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
fn ninja_path(base: &str, rel: &str) -> String {
    let normalized = Path::new(base)
        .join(rel)
        .normalize_lexically()
        .expect("path normalization");
    normalized
        .to_str()
        .expect("path is valid UTF-8")
        .replace('$', "$$")
        .replace(' ', "$ ")
        .replace(':', "$:")
}

/// Build the rsp file content: rsp_flags on first line(s), then one filename per line.
fn rsp_file_content(flags: &Flags) -> String {
    let mut content = flags.rsp_flags.clone();
    for file in &flags.files {
        content.push('\n');
        content.push('"');
        content.push_str(file);
        content.push('"');
    }
    content
}

fn write_rules(out: &mut impl FmtWrite) -> std::fmt::Result {
    writeln!(out, "rule cl")?;
    writeln!(out, "  command = cd /d \"$proj_dir\" && cl @$rsp")?;
    writeln!(out)?;
    writeln!(out, "rule lib_link")?;
    writeln!(
        out,
        "  command = cd /d \"$proj_dir\" && lib @$rsp && link /LIB @$rsp"
    )?;
    writeln!(out)?;
    writeln!(out, "rule link")?;
    writeln!(out, "  command = cd /d \"$proj_dir\" && link @$rsp")?;
    writeln!(out)?;
    Ok(())
}

fn write_cl(
    out: &mut impl FmtWrite,
    flags: &Flags,
    rsp_path: &Path,
    proj_dir: &str,
) -> std::fmt::Result {
    if flags.files.is_empty() {
        return Ok(());
    }

    writeln!(out, "build $")?;
    for src in &flags.files {
        let obj = {
            let out_file = &flags.output_file;
            if Path::new(out_file).extension().is_some_and(|e| e.eq_ignore_ascii_case("obj")) {
                out_file.clone()
            } else {
                let sep = if out_file.ends_with(['\\', '/']) { "" } else { "\\" };
                let stem = Path::new(src).file_stem().expect("source has stem").to_str().expect("valid UTF-8");
                format!("{out_file}{sep}{stem}.obj")
            }
        };
        writeln!(out, "    {} $", ninja_path("", &obj))?;
    }
    write!(out, "    : cl")?;
    for src in &flags.files {
        write!(out, " {}", ninja_path(proj_dir, src))?;
    }
    writeln!(out)?;
    writeln!(out, "  rsp = {}", rsp_path.display())?;
    writeln!(out)
}

fn write_final(
    out: &mut impl FmtWrite,
    rule: &str,
    flags: &Flags,
    rsp_path: &Path,
    proj_dir: &str,
) -> std::fmt::Result {
    writeln!(out, "build $")?;
    writeln!(out, "    {} $", ninja_path("", &flags.output_file))?;
    if flags.files.is_empty() {
        writeln!(out, "    : {rule}")?;
    } else {
        writeln!(out, "    : {rule} $")?;
        let last = flags.files.len() - 1;
        for (i, f) in flags.files.iter().enumerate() {
            if i < last {
                writeln!(out, "    {} $", ninja_path(proj_dir, f))?;
            } else {
                writeln!(out, "    {}", ninja_path(proj_dir, f))?;
            }
        }
    }
    writeln!(out, "  rsp = {}", rsp_path.display())?;
    writeln!(out)
}

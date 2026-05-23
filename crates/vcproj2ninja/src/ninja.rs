use std::fmt::Write;
use std::path::Path;

use vs2008_parser_lib::vcproj::Flags;

pub enum FinalStep {
    Lib(Flags),
    LinkForLib(Flags),
    Link(Flags),
}

pub struct NinjaFile {
    pub cl: Vec<Flags>,
    pub final_step: FinalStep,
}

impl NinjaFile {
    pub fn write(&self, out: &mut impl Write) -> std::fmt::Result {
        write_rules(out)?;

        for cl in &self.cl {
            write_cl(out, cl)?;
        }

        match &self.final_step {
            FinalStep::Lib(flags) => write_final(out, "lib", flags)?,
            FinalStep::LinkForLib(flags) => write_final(out, "lib", flags)?,
            FinalStep::Link(flags) => write_final(out, "link", flags)?,
        }

        Ok(())
    }
}

fn write_rules(out: &mut impl Write) -> std::fmt::Result {
    writeln!(out, "rule cl")?;
    writeln!(out, "  command = cl @$rspfile")?;
    writeln!(out, "  rspfile = $out.rsp")?;
    writeln!(out, "  rspfile_content = $flags $in")?;
    writeln!(out)?;
    writeln!(out, "rule lib")?;
    writeln!(out, "  command = lib @$rspfile")?;
    writeln!(out, "  rspfile = $out.rsp")?;
    writeln!(out, "  rspfile_content = $flags $in")?;
    writeln!(out)?;
    writeln!(out, "rule link")?;
    writeln!(out, "  command = link @$rspfile")?;
    writeln!(out, "  rspfile = $out.rsp")?;
    writeln!(out, "  rspfile_content = $flags $in")?;
    writeln!(out)?;
    Ok(())
}

fn write_cl(out: &mut impl Write, flags: &Flags) -> std::fmt::Result {
    // Compute one .obj output per source file using the IntDir as base.
    let obj_dir = &flags.output_file;
    let obj_files: Vec<String> = flags
        .files
        .iter()
        .map(|src| {
            let stem = Path::new(src)
                .file_stem()
                .expect("source file must have a stem")
                .to_str()
                .expect("stem is valid UTF-8");
            format!("{obj_dir}{stem}.obj")
        })
        .collect();

    // build <outputs>: cl <inputs>
    write!(out, "build")?;
    for obj in &obj_files {
        write!(out, " {obj}")?;
    }
    write!(out, ": cl")?;
    for src in &flags.files {
        write!(out, " {src}")?;
    }
    writeln!(out)?;
    writeln!(out, "  flags = {}", flags.flags.trim())?;
    writeln!(out)?;

    Ok(())
}

fn write_final(out: &mut impl Write, rule: &str, flags: &Flags) -> std::fmt::Result {
    write!(out, "build {}: {rule}", flags.output_file)?;
    for f in &flags.files {
        write!(out, " {f}")?;
    }
    writeln!(out)?;
    if !flags.flags.trim().is_empty() {
        writeln!(out, "  flags = {}", flags.flags.trim())?;
    }
    writeln!(out)?;
    Ok(())
}

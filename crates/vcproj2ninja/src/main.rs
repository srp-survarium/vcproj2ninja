#![allow(dead_code)]
#![allow(unused_imports)]
#![feature(os_string_truncate)]

mod ninja;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Context;
use clap::Parser;

use ninja::{FinalStep, NinjaFile};
use vs2008_parser_lib::vcproj::{ConfigurationType, LinkerTool, MsBuildEnvironment};
use vs2008_parser_lib::{sln, vcproj};

#[derive(clap::Parser)]
pub struct Cli {
    #[arg(long, value_hint = clap::ValueHint::FilePath)]
    pub sln_path: std::path::PathBuf,

    /// Project to build.
    #[arg(long)]
    pub project_name: String,

    /// Configuration to build project with
    #[arg(long)]
    pub configuration_platform: String,

    /// Directory to write generated .ninja files into (cleared on each run).
    #[arg(long, value_hint = clap::ValueHint::DirPath)]
    pub output_dir: std::path::PathBuf,
}

fn main() -> anyhow::Result<()> {
    let Cli {
        sln_path,
        project_name,
        configuration_platform,
        output_dir,
    } = Cli::parse();

    let sln = std::fs::read_to_string(&sln_path)?;
    let sln = match sln::Sln::parse(&sln) {
        Ok((_leftovers, sln)) => sln,
        Err(error) => anyhow::bail!("{error}"),
    };

    let deps = sln
        .find_project_dependencies(&project_name)
        .context("Project is not found")?;

    let sln_root = sln_path
        .parent()
        .context("Sln path must have a parent")?
        .to_path_buf();
    let mut project_path = sln_root.clone();

    let mut sln_root = sln_root.to_string_lossy().to_string();
    sln_root.push('\\');

    let base_len = project_path.as_os_str().as_encoded_bytes().len();

    // Phase 1: collect all ninja files before touching the output directory.
    let mut collected: Vec<(String, NinjaFile)> = vec![];

    for dep in deps {
        project_path.as_mut_os_string().truncate(base_len);

        for component in dep.path.split(['\\', '/']) {
            project_path.push(component);
        }

        let vcproj_text = std::fs::read_to_string(&project_path)?;
        let vcproj = vcproj::VCProject::parse_xml(&vcproj_text)
            .with_context(|| format!("Failed parsing '{}' at '{}'", dep.name, dep.path))?;

        let cfg_platform = sln
            .global
            .cfg_platforms
            .platforms
            .iter()
            .find(|cfg| cfg.uuid == vcproj.guid && cfg.target_cfg.0 == configuration_platform)
            .with_context(|| {
                format!(
                    "Failed find related config '{}' of '{configuration_platform}'",
                    dep.name
                )
            })?;

        if !cfg_platform.is_enabled {
            continue;
        }

        let build_cfg = vcproj
            .configurations
            .iter()
            .find(|cfg| cfg.name == cfg_platform.actual_cfg.0)
            .with_context(|| {
                format!(
                    "Failed find related config '{}' of '{}'",
                    dep.name, cfg_platform.actual_cfg.0
                )
            })?;

        let env = MsBuildEnvironment::get(&vcproj.name, build_cfg, &sln_root);

        let cl = build_cfg.compiler_tool.as_ref().with_context(|| {
            format!(
                "Only xbox configurations do not have a compiler enabled: {}",
                vcproj.name
            )
        })?;

        let cl_flags = cl.to_flags(build_cfg, &vcproj, env);

        let final_step = match build_cfg.configuration_type {
            ConfigurationType::_4 => {
                let flags = match &build_cfg.lib_tool {
                    Some(lib_tool) => lib_tool.to_flags(&dep.path, build_cfg, &vcproj, env),
                    None => LinkerTool::to_flags_for_lib(&dep.path, build_cfg, &vcproj, env),
                };
                FinalStep::Lib(flags)
            }
            ConfigurationType::_1 | ConfigurationType::_2 => {
                let linker_tool = build_cfg.linker_tool.as_ref().with_context(|| {
                    format!(
                        "Failed to find linker tool for exe/dll configuration: {}",
                        vcproj.name
                    )
                })?;
                FinalStep::Link(linker_tool.to_flags(&dep.path, build_cfg, &vcproj, env))
            }
            cfg_type => anyhow::bail!(
                "Unsupported configuration type {:?} for '{}'",
                cfg_type,
                vcproj.name
            ),
        };

        collected.push((dep.name.clone(), NinjaFile { cl: cl_flags, final_step }));
    }

    // Phase 2: clear and recreate the output directory.
    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir)?;
    }
    std::fs::create_dir_all(&output_dir)?;

    // Phase 3: assign unique filenames (conflict → _2, _3, …) and write.
    let mut used: HashSet<String> = HashSet::new();
    let mut subninja_names: Vec<String> = vec![];

    for (base_name, ninja_file) in collected {
        let stem = unique_stem(&mut used, &base_name);
        let file_name = format!("{stem}.ninja");

        let mut content = String::new();
        ninja_file.write(&mut content).expect("writing to String never fails");

        let out_path = output_dir.join(&file_name);
        std::fs::write(&out_path, &content)
            .with_context(|| format!("Failed to write '{}'", out_path.display()))?;

        subninja_names.push(file_name);
    }

    // Top-level build.ninja that includes all per-project files.
    let mut top = String::new();
    for name in &subninja_names {
        top.push_str(&format!("subninja {name}\n"));
    }
    let top_path = output_dir.join("build.ninja");
    std::fs::write(&top_path, &top)
        .with_context(|| format!("Failed to write '{}'", top_path.display()))?;

    println!("Wrote {} project file(s) to '{}'", subninja_names.len(), output_dir.display());

    Ok(())
}

fn unique_stem(used: &mut HashSet<String>, base: &str) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let mut n = 2usize;
    loop {
        let candidate = format!("{base}_{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

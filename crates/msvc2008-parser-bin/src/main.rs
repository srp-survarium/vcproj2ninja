#![allow(dead_code)]
#![allow(unused_imports)]
#![feature(os_string_truncate)]

use std::path::Path;

use anyhow::Context;
use clap::Parser;

use msvc2008_parser_lib::{
    sln,
    vcproj::{self, MsBuildEnvironment},
};

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
}

fn main() -> anyhow::Result<()> {
    let Cli {
        sln_path,
        project_name,
        configuration_platform,
    } = Cli::parse();
    let sln = std::fs::read_to_string(&sln_path)?;
    let sln = match sln::Sln::parse(&sln) {
        Ok((_leftovers, sln)) => sln,
        Err(error) => anyhow::bail!("{error}"),
    };

    let deps = sln
        .find_project_dependencies(&project_name)
        .context("Project is not found")?;

    // println!("Found {} dependencies for '{}'", deps.len(), project_name);
    // for dep in &deps {
    //     println!("> {}", dep.name);
    // }
    // println!();

    let sln_root = sln_path
        .parent()
        .context("Sln path must have a parent")?
        .to_path_buf();
    let mut project_path = sln_root.clone();

    let mut sln_root = sln_root.to_string_lossy().to_string(); // TODO
    sln_root.push('\\');

    let base_len = project_path.as_os_str().as_encoded_bytes().len();

    for dep in deps {
        project_path.as_mut_os_string().truncate(base_len);

        for component in dep.path.split(['\\', '/']) {
            project_path.push(component);
        }

        let vcproj = std::fs::read_to_string(&project_path)?;
        let vcproj = vcproj::VCProject::parse_xml(&vcproj)
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

        if !cfg_platform.is_enabled {
            continue;
        }

        let cl = build_cfg
            .compiler_tool
            .as_ref()
            .context("Only xbox configurations do not have a compiler enabled")?;
        let flags_n_files = cl.to_flags(build_cfg, &vcproj, env);

        for (flag, files) in flags_n_files {
            println!("[{}]: {}", vcproj.name, flag);
            // println!("[{}] [{}]: {}", vcproj.name, build_cfg.name, flag);
            // for file in files {
            //     println!("  {file}");
            // }
        }
    }

    Ok(())
}

fn skip_nones(object: impl std::fmt::Debug) -> String {
    format!("{object:#?}")
        .lines()
        .filter(|line| !line.contains("None"))
        .collect::<Vec<_>>()
        .join("\n")
}

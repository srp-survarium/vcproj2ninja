#![feature(os_string_truncate, normalize_lexically)]

mod ninja;

use std::collections::{HashMap, HashSet};

use anyhow::Context;
use clap::Parser;
use uuid::Uuid;

use ninja::{FinalStep, NinjaFile};
use vs2008_parser_lib::vcproj::{ConfigurationType, MsBuildEnvironment};
use vs2008_parser_lib::{sln, vcproj};

#[derive(clap::Parser)]
pub struct Cli {
    #[arg(long, value_hint = clap::ValueHint::FilePath)]
    pub sln_path: std::path::PathBuf,

    /// Project to build.
    #[arg(long)]
    pub project_name: String,

    /// Configuration to build project with.
    #[arg(long)]
    pub configuration_platform: String,

    /// Directory to write generated .ninja files into (cleared on each run).
    #[arg(long, value_hint = clap::ValueHint::DirPath)]
    pub output_dir: std::path::PathBuf,

    /// Print flags for each tool invocation in greppable format.
    #[arg(long)]
    pub verbose: bool,
}

fn main() -> anyhow::Result<()> {
    let Cli {
        sln_path,
        project_name,
        configuration_platform,
        output_dir,
        verbose,
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
    let mut ninja_files: Vec<(Uuid, String, NinjaFile)> = vec![];
    let mut guid_to_output: HashMap<Uuid, String> = HashMap::new();

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

        let proj_dir = project_path
            .parent()
            .expect("vcproj path must have a parent")
            .to_string_lossy()
            .into_owned();

        let final_step = match build_cfg.configuration_type {
            ConfigurationType::_4 => {
                let lib_tool = build_cfg.lib_tool.as_ref().with_context(|| {
                    format!(
                        "Failed to find lib tool for static lib configuration: {}",
                        vcproj.name
                    )
                })?;
                FinalStep::Lib(lib_tool.to_flags(&dep.path, build_cfg, &vcproj, env))
            }
            ConfigurationType::_1 | ConfigurationType::_2 => {
                let linker_tool = build_cfg.linker_tool.as_ref().with_context(|| {
                    format!(
                        "Failed to find linker tool for exe/dll configuration: {}",
                        vcproj.name
                    )
                })?;

                let link_flags = linker_tool.to_flags(&dep.path, build_cfg, &vcproj, env);

                FinalStep::Link(link_flags)
            }
            cfg_type => anyhow::bail!(
                "Unsupported configuration type {:?} for '{}'",
                cfg_type,
                vcproj.name
            ),
        };

        let output_file = match &final_step {
            FinalStep::Lib(f) => f.output_file.clone(),
            FinalStep::Link(f) => f.output_file.clone(),
        };
        guid_to_output.insert(vcproj.guid, output_file);

        ninja_files.push((
            vcproj.guid,
            dep.name.clone(),
            NinjaFile {
                cl: cl_flags,
                final_step,
                proj_dir,
                depends_on: vec![],
            },
        ));
    }

    // Populate order-only deps from sln project dependencies.
    let sln_projects: HashMap<Uuid, &sln::Project> =
        sln.projects.iter().map(|p| (p.uuid, p)).collect();
    for (guid, _name, ninja_file) in &mut ninja_files {
        let Some(sln_proj) = sln_projects.get(guid) else {
            continue;
        };
        let Some(section_deps) = &sln_proj.section_dependencies else {
            continue;
        };
        for dep in &section_deps.deps {
            if let Some(output) = guid_to_output.get(&dep.uuid) {
                ninja_file.depends_on.push(output.clone());
            }
        }
    }

    if verbose {
        for (_guid, name, ninja_file) in &ninja_files {
            for tree in &ninja_file.cl {
                print_tree_flags("cl", name, tree);
            }
            match &ninja_file.final_step {
                FinalStep::Lib(flags) => eprintln!("[lib][{name}]: {}", flags.rsp_flags),
                FinalStep::Link(flags) => eprintln!("[linker][{name}]: {}", flags.rsp_flags),
            }
        }
    }

    // Phase 2: clear and recreate the output directory.
    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir)?;
    }
    std::fs::create_dir_all(&output_dir)?;

    let rsp_dir = output_dir.join("rsp");
    std::fs::create_dir_all(&rsp_dir)?;

    // Phase 3: assign unique filenames and write.
    let mut used: HashSet<String> = HashSet::new();
    let mut subninja_names: Vec<String> = vec![];

    for (_guid, base_name, ninja_file) in ninja_files {
        let stem = unique_stem(&mut used, &base_name);
        let output = ninja_file.write(&stem, &rsp_dir);

        let ninja_path = output_dir.join(format!("{stem}.ninja"));
        std::fs::write(&ninja_path, &output.ninja_text)
            .with_context(|| format!("Failed to write '{}'", ninja_path.display()))?;

        for (rsp_path, rsp_content) in output.rsp_files {
            std::fs::write(&rsp_path, &rsp_content)
                .with_context(|| format!("Failed to write '{}'", rsp_path.display()))?;
        }

        subninja_names.push(format!("{stem}.ninja"));
    }

    // Top-level build.ninja that includes all per-project files.
    let mut top = String::new();
    for name in &subninja_names {
        top.push_str(&format!("subninja {name}\n"));
    }
    let top_path = output_dir.join("build.ninja");
    std::fs::write(&top_path, &top)
        .with_context(|| format!("Failed to write '{}'", top_path.display()))?;

    println!(
        "Wrote {} project file(s) to '{}'",
        subninja_names.len(),
        output_dir.display()
    );

    Ok(())
}

fn print_tree_flags(tool: &str, name: &str, tree: &vs2008_parser_lib::vcproj::FlagsTree) {
    eprintln!("[{tool}][{name}]: {}", tree.flags.rsp_flags);
    for (child, _) in &tree.dependants {
        print_tree_flags(tool, name, child);
    }
}

fn unique_stem(used: &mut HashSet<String>, base: &str) -> String {
    let base: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if used.insert(base.clone()) {
        return base;
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

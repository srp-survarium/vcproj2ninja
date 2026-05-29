#![feature(os_string_truncate, normalize_lexically)]

mod ninja;

use std::collections::{HashMap, HashSet};

use anyhow::Context;
use clap::Parser;
use uuid::Uuid;

use ninja::{FinalStep, NinjaFile};
use vs2008_parser_lib::vcproj::{ConfigurationType, Flags, MsBuildEnvironment};
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

    /// Generate a build graph for running ninja.exe/cl.exe under Wine on Linux.
    ///
    /// This binary is a Windows .exe run under Wine, fed native Linux paths.
    /// Windows path arithmetic (normalize/pathdiff) only behaves correctly on
    /// *drive-rooted* paths; on drive-less `/home/...` it misfires. Under Wine
    /// the Linux root is mounted at drive `Z:`, so with `--wine` we lift the
    /// arithmetic roots (`sln_path`'s dir and each project dir) to `Z:\...` so
    /// every emitted build-graph path comes out `Z:\...` (what ninja/cl resolve).
    /// The actual filesystem reads/writes still use the original `/home/...`
    /// paths, since Rust's std cannot open `Z:\` paths under Wine.
    #[arg(long)]
    pub wine: bool,
}

fn main() -> anyhow::Result<()> {
    let Cli {
        sln_path,
        project_name,
        configuration_platform,
        output_dir,
        verbose,
        wine,
    } = Cli::parse();

    let sln = std::fs::read_to_string(&sln_path)
        .with_context(|| format!("Reading sln '{}'", sln_path.display()))?;
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

    // $(SolutionDir) seeds all path arithmetic and every emitted build-graph
    // path. In --wine mode lift it to the drive-rooted `Z:\...` form so the
    // (Windows-target) arithmetic is correct; `project_path` above keeps the
    // native `/home/...` form for the actual `.vcproj` reads.
    let mut sln_root = if wine {
        unix_to_wine(&sln_root.to_string_lossy())
    } else {
        sln_root.to_string_lossy().into_owned()
    };
    sln_root.push('\\');

    let base_len = project_path.as_os_str().as_encoded_bytes().len();

    // Phase 1: collect all ninja files before touching the output directory.
    let mut ninja_files: Vec<(Uuid, String, NinjaFile)> = vec![];
    let mut guid_to_link_output: HashMap<Uuid, String> = HashMap::new();

    for dep in deps {
        project_path.as_mut_os_string().truncate(base_len);

        for component in dep.path.split(['\\', '/']) {
            project_path.push(component);
        }

        let vcproj_text = std::fs::read_to_string(&project_path)
            .with_context(|| format!("Reading '{}' at '{}'", dep.name, project_path.display()))?;
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
        // Match the drive-rooted env base in --wine mode: proj_dir is the `cd`
        // target and the base for resolving relative obj/source paths, so it
        // must agree with sln_root (Z:\...).
        let proj_dir = if wine { unix_to_wine(&proj_dir) } else { proj_dir };

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

        let link_output_file = match &final_step {
            FinalStep::Link(Flags {
                import_library: Some(import_library),
                ..
            }) => import_library.clone(),
            FinalStep::Lib(flags) | FinalStep::Link(flags) => flags.output_file.clone(),
        };
        guid_to_link_output.insert(vcproj.guid, link_output_file);

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

    // Populate order-only deps and linker inputs from sln project dependencies.
    //
    // The authoritative source for which libs a project needs is the sln
    // ProjectSection(ProjectDependencies), collected transitively.
    //
    // A more precise alternative would be to scan each project's source files and
    // their transitively included headers (resolved via the /I include paths) for
    // `#pragma comment(lib, "name.lib")` directives, then map the bare name to a
    // full path. This avoids false positives from deps that don't actually
    // contribute symbols, but requires a simplified C preprocessor (following
    // #include chains without macro expansion or conditional evaluation). The COFF
    // .drectve section approach is equivalent but doesn't work for LTCG anonymous
    // objects, and `dumpbin /DIRECTIVES` also returns empty for them.
    let sln_projects: HashMap<Uuid, &sln::Project> =
        sln.projects.iter().map(|p| (p.uuid, p)).collect();
    for (guid, _name, ninja_file) in &mut ninja_files {
        let mut visited = std::collections::HashSet::new();
        visited.insert(*guid);
        collect_transitive_deps(
            *guid,
            &sln_projects,
            &guid_to_link_output,
            &mut visited,
            &mut ninja_file.depends_on,
        );

        // For link steps, bake the dep lib paths directly into rsp_flags so
        // link.exe sees them as explicit inputs.
        if let FinalStep::Link(ref mut flags) = ninja_file.final_step {
            for dep_path in &ninja_file.depends_on {
                let path = std::path::Path::new(dep_path);
                let Some(ext) = path.extension() else {
                    continue;
                };

                let lib_path = match ext.as_encoded_bytes() {
                    b"lib" => dep_path.clone(),
                    _ => unimplemented!(
                        "Linker dependencies can only be libraries, yet: {}",
                        ext.to_string_lossy()
                    ),
                };
                flags.rsp_flags.push(' ');
                flags.rsp_flags.push_str(&lib_path);
            }
        }
    }

    if verbose {
        for (_guid, name, ninja_file) in &ninja_files {
            for group in &ninja_file.cl {
                print_cl_flags(name, group);
            }
            match &ninja_file.final_step {
                FinalStep::Lib(flags) => eprintln!("[lib][{name}]: {}", flags.rsp_flags),
                FinalStep::Link(flags) => eprintln!("[linker][{name}]: {}", flags.rsp_flags),
            }
        }
    }

    // Phase 2: clear and recreate the output directory.
    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir)
            .with_context(|| format!("Clearing output dir '{}'", output_dir.display()))?;
    }
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("Creating output dir '{}'", output_dir.display()))?;

    let rsp_dir = output_dir.join("rsp");
    std::fs::create_dir_all(&rsp_dir)
        .with_context(|| format!("Creating rsp dir '{}'", rsp_dir.display()))?;

    // The rsp dir is handed to ninja so it can reference rsp files in the cl/link
    // command lines, which run under Wine. In --wine mode pass the drive-rooted
    // `Z:\...` form; the rsp files themselves are still written to their /home
    // location below.
    let rsp_dir_for_ninja: std::path::PathBuf = if wine {
        unix_to_wine(&rsp_dir.to_string_lossy()).into()
    } else {
        rsp_dir.clone()
    };

    // Phase 3: assign unique filenames and write.
    let mut used: HashSet<String> = HashSet::new();
    let mut subninja_names: Vec<String> = vec![];
    let mut all_required_dirs: HashSet<std::path::PathBuf> = HashSet::new();

    for (_guid, base_name, ninja_file) in ninja_files {
        let stem = unique_stem(&mut used, &base_name);
        let output = ninja_file.write(&stem, &rsp_dir_for_ninja);

        let ninja_path = output_dir.join(format!("{stem}.ninja"));
        std::fs::write(&ninja_path, &output.ninja_text)
            .with_context(|| format!("Failed to write '{}'", ninja_path.display()))?;

        for (rsp_path, rsp_content) in output.rsp_files {
            // rsp_path came back in the `Z:\...` form we passed in; write the file
            // to its native /home location.
            let rsp_path = native_path(&rsp_path, wine);
            std::fs::write(&rsp_path, &rsp_content)
                .with_context(|| format!("Failed to write '{}'", rsp_path.display()))?;
        }

        all_required_dirs.extend(output.required_dirs);
        subninja_names.push(format!("{stem}.ninja"));
    }

    // required_dirs come from the emitted build graph, so in --wine mode they
    // are `Z:\...` strings. This binary can't create those under Wine, so map
    // back to the native `/home/...` path for the syscall.
    for dir in &all_required_dirs {
        let dir = native_path(dir, wine);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Creating required directory '{}'", dir.display()))?;
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

/// Lift a native Linux path to the drive-rooted form Wine exposes: the Linux
/// root `/` is mounted at drive `Z:`, so `/home/x` -> `Z:\home\x`. Non-absolute
/// inputs are returned unchanged.
fn unix_to_wine(p: &str) -> String {
    if p.starts_with('/') {
        format!("Z:{}", p.replace('/', "\\"))
    } else {
        p.to_string()
    }
}

/// Inverse of [`unix_to_wine`]: `Z:\home\x` -> `/home/x`. Used for the few real
/// filesystem syscalls, since Rust's std cannot open `Z:\` paths under Wine.
fn wine_to_unix(p: &str) -> String {
    p.strip_prefix("Z:")
        .or_else(|| p.strip_prefix("z:"))
        .unwrap_or(p)
        .replace('\\', "/")
}

/// Map a build-graph path back to the path this binary must touch on disk.
/// In --wine mode the graph is in `Z:\...` form (what ninja/cl resolve), but the
/// real file lives at the native `/home/...` path; without --wine it's already
/// native. Used at every filesystem write/create-dir site.
fn native_path(p: &std::path::Path, wine: bool) -> std::path::PathBuf {
    if wine {
        wine_to_unix(&p.to_string_lossy()).into()
    } else {
        p.to_path_buf()
    }
}

fn print_cl_flags(name: &str, group: &vs2008_parser_lib::vcproj::ClGroup) {
    eprintln!("[cl][{name}]: {}", group.flags.rsp_flags);
}

fn collect_transitive_deps(
    guid: Uuid,
    sln_projects: &HashMap<Uuid, &sln::Project>,
    guid_to_link_output: &HashMap<Uuid, String>,
    visited: &mut std::collections::HashSet<Uuid>,
    result: &mut Vec<String>,
) {
    let Some(sln_proj) = sln_projects.get(&guid) else {
        return;
    };
    let Some(section_deps) = &sln_proj.section_dependencies else {
        return;
    };

    for dep in &section_deps.deps {
        if !visited.insert(dep.uuid) {
            continue;
        }
        if let Some(output) = guid_to_link_output.get(&dep.uuid) {
            result.push(output.clone());
        }
        collect_transitive_deps(dep.uuid, sln_projects, guid_to_link_output, visited, result);
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

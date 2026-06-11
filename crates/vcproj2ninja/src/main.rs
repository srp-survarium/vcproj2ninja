#![feature(os_string_truncate, normalize_lexically)]

mod compdb;
mod ninja;
mod preprocess;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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

    /// Run the generated/consuming tools under Wine on Linux (affects HOW
    /// paths are computed, not WHAT is generated - combine with any --target).
    ///
    /// This binary is a Windows .exe run under Wine, fed native Linux paths.
    /// Windows path arithmetic (normalize/pathdiff) only behaves correctly on
    /// *drive-rooted* paths; on drive-less `/home/...` it misfires. Under Wine
    /// the Linux root is mounted at drive `Z:`, so with `--wine` we lift the
    /// arithmetic roots (`sln_path`'s dir and each project dir) to `Z:\...`.
    /// For `--target ninja` the emitted graph keeps the `Z:\...` form (what
    /// ninja/cl resolve under Wine); for `--target clangd` the arithmetic
    /// result is lowered back to `/...` at emission (clangd runs natively).
    /// The actual filesystem reads/writes always use the original `/home/...`
    /// paths, since Rust's std cannot open `Z:\` paths under Wine.
    #[arg(long)]
    pub wine: bool,

    /// WHAT to generate: `ninja` (default) - the build graph + rsp files;
    /// `clangd` - compile_commands.json (clang-cl flavor) into the output
    /// dir instead (the output dir is NOT cleared in this mode).
    #[arg(long, value_enum, default_value_t = Target::Ninja)]
    pub target: Target,

    /// (--target clangd) System include dir, emitted as `-imsvc<dir>`
    /// (VC8 CRT, WinSDK). Repeatable.
    #[arg(long)]
    pub imsvc: Vec<String>,

    /// (--target clangd) Extra argument appended verbatim to every entry
    /// (VFS overlay, _STLP_NATIVE_INCLUDE_PATH define, ...). Repeatable.
    #[arg(long)]
    pub extra_arg: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum Target {
    /// Ninja build graph + rsp files.
    Ninja,
    /// compile_commands.json for clangd.
    Clangd,
}

fn main() -> anyhow::Result<()> {
    let Cli {
        sln_path,
        project_name,
        configuration_platform,
        output_dir,
        verbose,
        wine,
        target,
        imsvc,
        extra_arg,
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
    let sln_root_str = sln_root.to_str().expect("sln dir is valid UTF-8");
    let mut sln_root = if wine {
        unix_to_wine(sln_root_str)
    } else {
        sln_root_str.to_string()
    };
    sln_root.push('\\');

    let base_len = project_path.as_os_str().as_encoded_bytes().len();

    // Phase 1: collect all ninja files before touching the output directory.
    let mut ninja_files: Vec<(Uuid, String, NinjaFile)> = vec![];
    let mut guid_to_link_output: HashMap<Uuid, String> = HashMap::new();

    // Preprocessor directives we don't follow, warned about once per keyword
    // across the whole run so we can see what — if anything — we're missing.
    let mut warned_directives: HashSet<String> = HashSet::new();

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

        let mut cl_flags = cl.to_flags(build_cfg, &vcproj, env);

        let proj_dir_native = project_path
            .parent()
            .expect("vcproj path must have a parent")
            .to_path_buf();

        // Preprocess each cl group's sources to discover the headers they
        // transitively include, so header edits force a recompile. The scan
        // also collects #pragma comment(lib, ...) directives (surfaced only for
        // now; not yet wired into the linker).
        let mut file_cache = preprocess::FileCache::default();
        let mut pragma_libs: Vec<String> = vec![];
        for group in &mut cl_flags {
            let include_dirs: Vec<PathBuf> = group
                .include_dirs
                .iter()
                .map(|dir| to_native(dir, &proj_dir_native))
                .collect();
            let sources: Vec<PathBuf> = group
                .flags
                .files
                .iter()
                .map(|file| to_native(file, &proj_dir_native))
                .collect();

            let result = preprocess::scan_translation_units(
                &sources,
                &include_dirs,
                &group.defines,
                &mut file_cache,
            );

            for unknown in &result.unknown_directives {
                if warned_directives.insert(unknown.keyword.clone()) {
                    eprintln!(
                        "warning: unhandled preprocessor directive '#{}' not followed for \
                         header deps (first seen in {}: `{}`)",
                        unknown.keyword,
                        unknown.file.display(),
                        unknown.line,
                    );
                }
            }

            let mut deps: Vec<String> = result
                .headers
                .iter()
                .map(|header| native_to_ninja(header, wine))
                .collect();
            deps.sort();
            deps.dedup();

            if verbose {
                eprintln!(
                    "[headers][{}]: {} header dep(s) across {} source file(s)",
                    dep.name,
                    deps.len(),
                    group.flags.files.len()
                );
            }

            group.header_deps = deps;
            pragma_libs.extend(result.pragma_libs);
        }
        pragma_libs.sort();
        pragma_libs.dedup();
        if verbose && !pragma_libs.is_empty() {
            eprintln!("[pragma-libs][{}]: {}", dep.name, pragma_libs.join(" "));
        }

        let proj_dir = proj_dir_native
            .to_str()
            .expect("project dir is valid UTF-8")
            .to_string();
        // Match the drive-rooted env base in --wine mode: proj_dir is the `cd`
        // target and the base for resolving relative obj/source paths, so it
        // must agree with sln_root (Z:\...).
        let proj_dir = if wine {
            unix_to_wine(&proj_dir)
        } else {
            proj_dir
        };

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
    // ProjectSection(ProjectDependencies), collected transitively — but only
    // through static lib projects (ConfigurationType 4).  VS2008 stops recursion
    // at DLL/EXE boundaries: a DLL is self-contained, so its transitive static-lib
    // deps are internal to that DLL and must not be re-linked into the consumer.
    // Recursing through DLLs over-includes libs (e.g. squish/nvcore/nvimage/nvtt
    // via editor → texture_compressor → nvtt → squish) and causes CRT conflicts
    // when those libs use a different RuntimeLibrary than the final exe.
    //
    // A more precise alternative would be to scan each project's source files and
    // their transitively included headers (resolved via the /I include paths) for
    // `#pragma comment(lib, "name.lib")` directives, then map the bare name to a
    // full path. This avoids false positives from deps that don't actually
    // contribute symbols, but requires a simplified C preprocessor (following
    // #include chains without macro expansion or conditional evaluation). The COFF
    // .drectve section approach is equivalent but doesn't work for LTCG anonymous
    // objects, and `dumpbin /DIRECTIVES` also returns empty for them.
    let guid_to_is_static_lib: HashMap<Uuid, bool> = ninja_files
        .iter()
        .map(|(guid, _, nf)| (*guid, matches!(nf.final_step, FinalStep::Lib(_))))
        .collect();
    let sln_projects: HashMap<Uuid, &sln::Project> =
        sln.projects.iter().map(|p| (p.uuid, p)).collect();
    for (guid, _name, ninja_file) in &mut ninja_files {
        let mut visited = std::collections::HashSet::new();
        visited.insert(*guid);
        collect_transitive_deps(
            *guid,
            &sln_projects,
            &guid_to_link_output,
            &guid_to_is_static_lib,
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
                        ext.to_str().expect("extension is valid UTF-8")
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

    // --target clangd: emit the compilation database and stop. The ninja
    // backend's output dir handling (clear + rsp tree) is not wanted here.
    if target == Target::Clangd {
        std::fs::create_dir_all(&output_dir)
            .with_context(|| format!("Creating output dir '{}'", output_dir.display()))?;
        let out_path = output_dir.join("compile_commands.json");
        let n = compdb::write_compile_commands(&ninja_files, &out_path, &imsvc, &extra_arg)?;
        println!("Wrote {n} compile command(s) to '{}'", out_path.display());
        return Ok(());
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
        unix_to_wine(rsp_dir.to_str().expect("rsp dir is valid UTF-8")).into()
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

/// Convert an include dir / source path as it appears in the emitted flags
/// (possibly `Z:\...` under --wine, possibly relative with backslashes) into a
/// native absolute filesystem path the preprocessor can actually open.
fn to_native(raw: &str, proj_dir: &Path) -> PathBuf {
    let replaced = raw.trim().replace('\\', "/");
    let stripped = replaced
        .strip_prefix("Z:")
        .or_else(|| replaced.strip_prefix("z:"))
        .unwrap_or(&replaced);
    let path = Path::new(stripped);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        proj_dir.join(path)
    };
    joined.normalize_lexically().unwrap_or(joined)
}

/// Map a native filesystem path back into the build-graph path space: `Z:\...`
/// under --wine, native otherwise — mirroring how obj/source paths are emitted.
///
/// These header paths come from the scanner's `Path` operations. When this
/// binary runs as a Windows PE under Wine (the `--wine` case), those operations
/// normalize separators to `\` and drop the leading `/`, so we unify to forward
/// slashes first; `unix_to_wine` then lifts a rooted `/home/...` to the
/// drive-rooted `Z:\home\...` form. Without the unification the path would be
/// emitted drive-less (`\home\...`), inconsistent with every other graph path.
fn native_to_ninja(path: &Path, wine: bool) -> String {
    let path = path
        .to_str()
        .expect("header path is valid UTF-8")
        .replace('\\', "/");
    if wine { unix_to_wine(&path) } else { path }
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
        wine_to_unix(p.to_str().expect("path is valid UTF-8")).into()
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
    guid_to_is_static_lib: &HashMap<Uuid, bool>,
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
        // Only recurse into static libs. DLLs and EXEs are self-contained: their
        // transitive static-lib deps are internal and must not be re-linked into
        // the consumer (matches VS2008 linker behavior). Removing this gate pulls
        // in editor/nvtt-internal libs (nvcore/nvimage/squish, the image libs)
        // that are built /MD and reference msvcrt imports (e.g. __imp__vsnprintf_s)
        // -> unresolved against the /MT exe (which excludes msvcrt). VS never links
        // these into the exe.
        let is_static_lib = guid_to_is_static_lib
            .get(&dep.uuid)
            .copied()
            .unwrap_or(false);
        if is_static_lib {
            collect_transitive_deps(
                dep.uuid,
                sln_projects,
                guid_to_link_output,
                guid_to_is_static_lib,
                visited,
                result,
            );
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ninja::{FinalStep, NinjaFile};
    use vs2008_parser_lib::vcproj::{Flags, MsBuildEnvironment, VCProject};

    /// End-to-end check of the header-dependency feature over real on-disk
    /// files: parse a .vcproj, lower it to flags, run the preprocessor scan the
    /// way `main` does, and confirm the transitively-included headers land in
    /// the generated ninja text as implicit inputs — while an `#if 0` include is
    /// pruned.
    #[test]
    fn header_dependencies_reach_generated_ninja() {
        let root = std::env::temp_dir().join(format!("vc2ninja_e2e_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let proj_dir = root.join("proj");
        let inc_dir = root.join("inc");
        let int_dir = root.join("int");
        std::fs::create_dir_all(&proj_dir).unwrap();
        std::fs::create_dir_all(&inc_dir).unwrap();
        std::fs::create_dir_all(&int_dir).unwrap();

        // main.cpp -> shared.h -> deep.h ; dead.h is behind #if 0.
        std::fs::write(
            proj_dir.join("main.cpp"),
            "#include \"shared.h\"\n#if 0\n#include \"dead.h\"\n#endif\n",
        )
        .unwrap();
        std::fs::write(inc_dir.join("shared.h"), "#include \"deep.h\"\n").unwrap();
        std::fs::write(inc_dir.join("deep.h"), "// leaf\n").unwrap();
        std::fs::write(inc_dir.join("dead.h"), "// must be pruned\n").unwrap();

        let vcproj_xml = format!(
            r#"<?xml version="1.0" encoding="Windows-1252"?>
<VisualStudioProject ProjectType="Visual C++" Version="9,00" Name="myproject"
    ProjectGUID="{{00000000-0000-0000-0000-000000000001}}" RootNamespace="myproject"
    TargetFrameworkVersion="196613">
    <Platforms><Platform Name="Win32"/></Platforms>
    <Configurations>
        <Configuration Name="Release|Win32" OutputDirectory="{out}"
            IntermediateDirectory="{int}" ConfigurationType="4">
            <Tool Name="VCCLCompilerTool" AdditionalIncludeDirectories="{inc}"/>
            <Tool Name="VCLibrarianTool" OutputFile="{out}/myproject.lib"/>
        </Configuration>
    </Configurations>
    <Files>
        <File RelativePath=".\main.cpp"/>
    </Files>
</VisualStudioProject>"#,
            out = int_dir.display(),
            int = int_dir.display(),
            inc = inc_dir.display(),
        );

        let vcproj = VCProject::parse_xml(&vcproj_xml).unwrap();
        let cfg = &vcproj.configurations[0];
        let env = MsBuildEnvironment::get(&vcproj.name, cfg, "/sln/");
        let cl = cfg.compiler_tool.as_ref().unwrap();
        let mut cl_flags = cl.to_flags(cfg, &vcproj, env);

        // Mirror main's per-group scan (non-wine path forms).
        let mut cache = preprocess::FileCache::default();
        for group in &mut cl_flags {
            let include_dirs: Vec<PathBuf> = group
                .include_dirs
                .iter()
                .map(|dir| to_native(dir, &proj_dir))
                .collect();
            let sources: Vec<PathBuf> = group
                .flags
                .files
                .iter()
                .map(|file| to_native(file, &proj_dir))
                .collect();
            let result = preprocess::scan_translation_units(
                &sources,
                &include_dirs,
                &group.defines,
                &mut cache,
            );
            group.header_deps = result
                .headers
                .iter()
                .map(|header| native_to_ninja(header, false))
                .collect();
            group.header_deps.sort();
            group.header_deps.dedup();
        }

        let ninja_file = NinjaFile {
            cl: cl_flags,
            final_step: FinalStep::Lib(Flags {
                output_file: int_dir
                    .join("myproject.lib")
                    .to_str()
                    .expect("path is valid UTF-8")
                    .to_string(),
                import_library: None,
                flags: "@$(RspFile)".to_string(),
                rsp_flags: String::new(),
                files: vec![],
            }),
            proj_dir: proj_dir.to_str().expect("path is valid UTF-8").to_string(),
            depends_on: vec![],
        };

        let output = ninja_file.write("myproject", &root.join("rsp"));
        let text = output.ninja_text;

        let as_str = |p: PathBuf| p.to_str().expect("path is valid UTF-8").to_string();
        let shared = as_str(inc_dir.join("shared.h"));
        let deep = as_str(inc_dir.join("deep.h"));
        let dead = as_str(inc_dir.join("dead.h"));

        assert!(
            text.contains(&shared),
            "shared.h must be an implicit input:\n{text}"
        );
        assert!(
            text.contains(&deep),
            "deep.h (transitive) must be an implicit input:\n{text}"
        );
        assert!(
            !text.contains(&dead),
            "dead.h behind #if 0 must be pruned:\n{text}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Discovery sweep: walk a real source tree (path from `VC2NINJA_SWEEP_DIR`)
    /// and report every `#`-directive the preprocessor doesn't follow, ranked by
    /// how many files contain it. Run with:
    ///   VC2NINJA_SWEEP_DIR=/path cargo test sweep_unknown_directives -- --ignored --nocapture
    #[test]
    #[ignore]
    fn sweep_unknown_directives() {
        let dir =
            std::env::var("VC2NINJA_SWEEP_DIR").expect("set VC2NINJA_SWEEP_DIR to a source tree");
        let root = PathBuf::from(dir);

        let mut files = Vec::new();
        collect_sources(&root, &mut files);
        eprintln!(
            "scanning {} source/header files under {}",
            files.len(),
            root.display()
        );

        // keyword -> (files containing it, one sample "file: line")
        let mut tally: HashMap<String, (usize, String)> = HashMap::new();
        let mut cache = preprocess::FileCache::default();

        for file in &files {
            // Each file as its own root, no include dirs: we only want to
            // classify the directives literally present in the tree.
            let result = preprocess::scan_translation_units(
                std::slice::from_ref(file),
                &[],
                &[],
                &mut cache,
            );
            for unknown in result.unknown_directives {
                let entry = tally.entry(unknown.keyword).or_insert_with(|| {
                    (0, format!("{}: {}", unknown.file.display(), unknown.line))
                });
                entry.0 += 1;
            }
        }

        let mut ranked: Vec<_> = tally.into_iter().collect();
        ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));

        eprintln!("\n=== unhandled #-directives (by file count) ===");
        if ranked.is_empty() {
            eprintln!("(none)");
        }
        for (keyword, (count, sample)) in &ranked {
            eprintln!("  #{keyword:<14} {count:>5} file(s)   e.g. {sample}");
        }
    }

    fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip VCS / tooling noise.
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if matches!(name, ".git" | ".claude" | "target") {
                    continue;
                }
                collect_sources(&path, out);
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext = ext.to_ascii_lowercase();
                if matches!(
                    ext.as_str(),
                    "h" | "hpp" | "hxx" | "inl" | "cpp" | "cxx" | "cc" | "c"
                ) {
                    out.push(path);
                }
            }
        }
    }
}

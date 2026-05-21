use msvc2008_parser_proc::{ParseXml, flag_enum};

use super::macros::*;
use super::utils::pathdiff;
use super::{Configuration, File, Files, Filter, MsBuildEnvironment, VCProject};

use std::ffi::OsStr;
use std::fmt::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, ParseXml)]
#[parse_xml(tag = "VCLibrarianTool", ignore = "Name")]
pub struct LibTool {
    pub additional_options: Option<String>,
    // Requires interpolation: $(SolutionDir)../binaries/$(PlatformName)/libraries/vostok_$(ProjectName)-static-gold.lib"
    pub output_file: Option<String>,
    pub additional_library_directories: Option<Vec<String>>,
    pub ignore_default_library_names: Option<Vec<String>>,
    pub suppress_startup_banner: Option<SuppressStartupBanner>,
}

flag_enum! {
    enum SuppressStartupBanner {
        false => "",
        true => "/NOLOGO",
    }
}

impl LibTool {
    pub fn to_flags(
        &self,
        vcproj_rpath: &str,
        cfg: &Configuration,
        vcproject: &VCProject,
        env: MsBuildEnvironment,
    ) -> String {
        let Self {
            additional_options,
            output_file,
            additional_library_directories,
            ignore_default_library_names,
            suppress_startup_banner: _,
        } = self;

        let output_file = output_file
            .as_deref()
            .unwrap_or("$(OutDir)\\$(ProjectName).lib");

        let mut result = String::new();

        let out_file = env.expand(output_file);
        let out_file = out_file.trim().trim_matches('"');
        write!(result, " /OUT:\"{out_file}\"").unwrap();

        for lib_path in additional_library_directories
            .iter()
            .flatten()
            .filter(|s| !s.is_empty())
        {
            let lib_path = env.expand(lib_path);
            let lib_path = lib_path.trim().trim_matches('"');

            write!(result, " /LIBPATH:\"{lib_path}\"").unwrap();
        }

        for lib_path in ignore_default_library_names
            .iter()
            .flatten()
            .filter(|s| !s.is_empty())
        {
            let lib_path = env.expand(lib_path);
            let lib_path = lib_path.trim().trim_matches('"');

            write!(result, " /NODEFAULTLIB:\"{lib_path}\"").unwrap();
        }

        if matches!(cfg.whole_program_optimization, Some(true)) {
            result.push(' ');
            result.push_str("/LTCG");
        }

        if let Some(additional_options) = additional_options
            && !additional_options.is_empty()
        {
            result.push(' ');
            result.push_str(additional_options);
        }

        let files = Self::file_flags(&vcproject.files, &cfg.name, vcproj_rpath, env);

        result.push_str(&files.join("\n"));

        result
    }

    pub fn file_flags(
        files: &Files,
        configuration_platform: &str,
        vcproj_rpath: &str,
        env: MsBuildEnvironment,
    ) -> Vec<String> {
        let vcproj_dir = {
            let mut vcproj_dir = Path::new(env.solution_dir).to_path_buf();
            for vcproj_part in Path::new(vcproj_rpath).components() {
                vcproj_dir.push(vcproj_part);
            }

            let mut vcproj_dir = vcproj_dir.normalize_lexically().unwrap();
            vcproj_dir.pop();
            vcproj_dir
        };
        let int_dir = PathBuf::from(env.expand(env.int_dir));

        let source_files = Self::parse_files(files, configuration_platform)
            .into_iter()
            .map(|source_file| Path::new(source_file).file_name().unwrap());
        Self::check_no_conflicts(source_files.clone());

        let mut int_rpath = pathdiff(&vcproj_dir, &int_dir);
        let base_len = int_rpath.as_os_str().as_encoded_bytes().len();

        let mut result = vec![];
        for source_file in source_files {
            int_rpath.as_mut_os_string().truncate(base_len);

            int_rpath.push(source_file);
            int_rpath.set_extension("obj");
            result.push(format!("\"{}\"", int_rpath.to_str().unwrap()));
        }
        result
    }

    pub fn parse_files<'a>(files: &'a Files, configuration_platform: &str) -> Vec<&'a str> {
        let mut result = vec![];

        for filter in &files.filters {
            Self::parse_filter(&mut result, filter, configuration_platform);
        }

        for file in &files.files {
            Self::parse_file(&mut result, file, configuration_platform);
        }

        result
    }

    fn check_no_conflicts<'a>(_files: impl Iterator<Item = &'a OsStr>) {
        // use std::collections::HashSet;

        // @TODO:
        // In our code `render_engine_pc_dx11` has conflicts.
        // Specifically, `effect_editor_selection` is repeated twice.
        // Doesn't seem to cause any problems with the original build system though.
        //
        // @TODO:
        // The order is also different there. Why? I don't know.
        // Need to check that file for cl flags.
        // Need to write parser-comparer to find all mismatches.

        // let mut conflicts = HashSet::new();
        // for file in files {
        //     if !conflicts.insert(file) {
        //         panic!(
        //             "Failed parsing linker flags. The file '{}' has a conflict with the same name",
        //             file.to_string_lossy()
        //         )
        //     }
        // }
    }

    fn parse_filter<'a>(
        result: &mut Vec<&'a str>,
        filter: &'a Filter,
        configuration_platform: &str,
    ) {
        for filter in &filter.filters {
            Self::parse_filter(result, filter, configuration_platform);
        }

        for file in &filter.files {
            Self::parse_file(result, file, configuration_platform);
        }
    }

    fn parse_file<'a>(result: &mut Vec<&'a str>, file: &'a File, configuration_platform: &str) {
        for file in &file.files {
            Self::parse_file(result, file, configuration_platform);
        }

        let file_extension = Path::new(&file.relative_path)
            .extension()
            .map(OsStr::as_encoded_bytes);

        match file_extension {
            Some(b"c" | b"cpp") => (),
            _ => return,
        };

        let config = file
            .file_configurations
            .iter()
            .filter(|config| config.name == configuration_platform)
            .find(|config| config.tool.is_some());

        match config {
            Some(config) if config.excluded_from_build == Some(true) => return,
            _ => (),
        }

        result.push(&file.relative_path);
    }
}

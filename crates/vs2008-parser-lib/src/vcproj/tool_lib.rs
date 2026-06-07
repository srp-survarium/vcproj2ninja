use vs2008_parser_proc::{ParseXml, flag_enum};

use super::flags::Flags;
use super::macros::*;
use super::utils::pathdiff;
use super::{Configuration, File, Files, Filter, MsBuildEnvironment, VCProject};

use std::collections::HashSet;
use std::ffi::OsStr;
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
    ) -> Flags {
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

        let mut rsp_flags: Vec<String> = vec![];

        let output_file = env.expand(output_file);
        let output_file = output_file.trim().trim_matches('"');
        rsp_flags.push(format!("/OUT:\"{output_file}\""));

        for lib_path in additional_library_directories
            .iter()
            .flatten()
            .filter(|s| !s.is_empty())
        {
            let lib_path = env.expand(lib_path);
            let lib_path = lib_path.trim().trim_matches('"');
            rsp_flags.push(format!("/LIBPATH:\"{lib_path}\""));
        }

        for lib_path in ignore_default_library_names
            .iter()
            .flatten()
            .filter(|s| !s.is_empty())
        {
            let lib_path = env.expand(lib_path);
            let lib_path = lib_path.trim().trim_matches('"');
            rsp_flags.push(format!("/NODEFAULTLIB:\"{lib_path}\""));
        }

        if matches!(cfg.whole_program_optimization, Some(true)) {
            rsp_flags.push("/LTCG".to_string());
        }

        if let Some(additional_options) = additional_options
            && !additional_options.is_empty()
        {
            rsp_flags.push(additional_options.clone());
        }

        let files = Self::file_flags(&vcproject.files, &cfg.name, vcproj_rpath, env);

        Flags {
            output_file: output_file.to_string(),
            import_library: None,
            flags: "@$(RspFile) /NOLOGO".to_string(),
            rsp_flags: rsp_flags.join(" "),
            files,
        }
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

        let source_files = Self::parse_files(files, configuration_platform);
        Self::check_no_conflicts(
            source_files
                .iter()
                .map(|(p, _)| Path::new(p).file_name().unwrap()),
        );

        let mut int_rpath = pathdiff(&vcproj_dir, &int_dir);
        let base_len = int_rpath.as_os_str().as_encoded_bytes().len();

        let mut result = vec![];
        for (source_path, obj_override) in source_files {
            let mut env = env;
            env.input_name = Path::new(source_path)
                .file_stem()
                .map(|x| x.to_str().expect("Path was constructed from String"))
                .expect("source_path cannot be an empty path");

            let obj_override = obj_override.map(|obj_override| {
                let obj_override = env.expand(obj_override);
                let obj_override = obj_override.trim().trim_matches('"').to_string();
                assert_eq!(
                    Path::new(&obj_override).extension(),
                    Some(OsStr::new("obj"))
                );

                obj_override
            });

            let source_path = match &obj_override {
                None => source_path,
                Some(obj_override) => obj_override.as_str(),
            };
            let file_name = Path::new(source_path).file_stem().unwrap();
            int_rpath.as_mut_os_string().truncate(base_len);
            int_rpath.push(file_name);
            int_rpath.set_extension("obj");
            result.push(int_rpath.to_str().unwrap().to_string());
        }
        result
    }

    pub fn parse_files<'a>(
        files: &'a Files,
        configuration_platform: &str,
    ) -> Vec<(&'a str, Option<&'a str>)> {
        let mut result = vec![];

        for filter in &files.filters {
            Self::parse_filter(&mut result, filter, configuration_platform);
        }

        for file in &files.files {
            Self::parse_file(&mut result, file, configuration_platform);
        }

        // It is possible for the same file to repeat multiple times in `Files` tag.
        // This can be seen in `render_engine_pc_dx11.vcproj` for `effect_editor_shader_complexity.cpp`.
        let mut conflicts = HashSet::new();
        result.retain(|(file, _)| conflicts.insert(*file));

        result
    }

    fn check_no_conflicts<'a>(files: impl Iterator<Item = &'a OsStr>) {
        // @TODO:
        // In our code `render_engine_pc_dx11` has conflicts.
        // Specifically, `effect_editor_selection` is repeated twice.
        // Doesn't seem to cause any problems with the original build system though.
        //
        // @TODO:
        // The order is also different there. Why? I don't know.
        // Need to check that file for cl flags.
        // Need to write parser-comparer to find all mismatches.

        use std::collections::HashSet;
        let mut conflicts = HashSet::new();
        for file in files {
            if !conflicts.insert(file) {
                panic!(
                    "Failed parsing linker flags. The file '{}' has a conflict with the same name",
                    file.to_str().expect("file path is valid UTF-8")
                )
            }
        }
    }

    fn parse_filter<'a>(
        result: &mut Vec<(&'a str, Option<&'a str>)>,
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

    fn parse_file<'a>(
        result: &mut Vec<(&'a str, Option<&'a str>)>,
        file: &'a File,
        configuration_platform: &str,
    ) {
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

        if matches!(config, Some(config) if config.excluded_from_build == Some(true)) {
            return;
        }

        let obj_override = config
            .and_then(|c| c.tool.as_ref())
            .and_then(|t| t.object_file.as_deref());

        result.push((&file.relative_path, obj_override));
    }
}

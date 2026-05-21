use super::Configuration;
use super::ConfigurationType;

use std::path::Path;

/// MSBuild/Visual Studio macros expanded at build time in .vcxproj files.
#[derive(Clone, Copy, Debug)]
pub struct MsBuildEnvironment<'a> {
    /// Directory of the .sln file, with trailing backslash.
    /// Example: solution at `C:\projects\App\App.sln` -> `C:\projects\App\`.
    pub solution_dir: &'a str,

    /// Intermediate output directory (object files, etc.), with trailing backslash.
    /// Example: `E:\Projects\vostok\binaries\Win32\intermediates\Release (static)\script`
    pub int_dir: &'a str,

    // TODO
    pub out_dir: &'a str,

    /// Base name of the source file being compiled (no extension, no path).
    /// Example: compiling `src\parser.cpp` -> `parser`.
    pub input_name: &'a str,

    /// Project name (taken from .vcproj configuration).
    /// Example: `survarium - PC - DirectX 11`
    pub project_name: &'a str,

    /// Taken from linker `OutputFile` option. If not present defaults to `project_name`.
    pub target_name: &'a str,

    /// Configuration name.
    /// Example: `Release` or `Master Gold`.
    pub configuration_name: &'a str,

    /// Target platform of the current configuration.
    /// Example: `Win32`, `x64`, `ARM64`.
    pub platform_name: &'a str,
}

// SolutionDir=E:\Projects\vostok\sources
// PlatformName=Win32
// ConfigurationName=Master Gold
// ProjectName=network

impl<'a> MsBuildEnvironment<'a> {
    pub fn get(project_name: &'a str, configuration: &'a Configuration, sln_root: &'a str) -> Self {
        let (configuration_name, platform_name) = configuration
            .name
            .split_once('|')
            .expect("Configuration should be parsed by sln parser");

        let mut target_name = project_name;

        let mut output_file = None;
        match configuration.configuration_type {
            ConfigurationType::_1 | ConfigurationType::_2 => {
                output_file = configuration
                    .linker_tool
                    .as_ref()
                    .and_then(|linker| linker.output_file.as_deref());
            }
            ConfigurationType::_4 => {
                output_file = configuration
                    .lib_tool
                    .as_ref()
                    .and_then(|lib| lib.output_file.as_deref());
            }
            _ => (),
        }
        if let Some(output_file) = output_file {
            target_name = Path::new(output_file)
                .file_stem()
                .expect("Output file must have stem")
                .to_str()
                .expect("Path was constructed from String")
        }

        let int_dir = configuration
            .intermediate_directory
            .as_deref()
            .expect("For my case is always present");

        let out_dir = configuration
            .output_directory
            .as_deref()
            .unwrap_or("$(SolutionDir)$(ConfigurationName)");

        Self {
            solution_dir: sln_root,
            int_dir,
            out_dir,
            input_name: "", // input_name is set per file basis
            project_name,
            target_name,
            configuration_name,
            platform_name,
        }
    }

    pub fn expand(&self, input: &str) -> String {
        let Self {
            // Location of .sln file
            solution_dir,
            // IntermediateDirectory from Configuration
            int_dir,
            out_dir,
            input_name,
            project_name,
            //
            target_name,
            configuration_name,
            platform_name,
        } = self;

        input
            // IntermediateDirectory="$(SolutionDir)../binaries/$(PlatformName)/intermediates/$(ConfigurationName)/$(ProjectName)"
            .replace("$(IntDir)", int_dir)
            .replace("$(OutDir)", out_dir)
            // OutputFile="$(SolutionDir)../binaries/$(PlatformName)/survarium-dx11-win32-dynamic.exe"
            .replace("$(TargetName)", target_name)
            .replace("$(InputName)", input_name)
            //
            .replace("$(ProjectName)", project_name)
            .replace("$(ConfigurationName)", configuration_name)
            .replace("$(PlatformName)", platform_name)
            .replace("$(SolutionDir)", solution_dir)
    }
}

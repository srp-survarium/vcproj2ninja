use vs2008_parser_proc::{ParseXml, flag_enum};

use super::flags::{ClGroup, Flags, append_flags};
use super::macros::*;
use super::{CharacterSet, ConfigurationType};
use super::{Configuration, File, Files, Filter, MsBuildEnvironment, VCProject};

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(Debug, ParseXml, Eq, PartialEq, Hash, Clone, Default)]
#[parse_xml(
    merge,
    tag = "VCCLCompilerTool",
    ignore = "Name",
    ignore = "ExecutionBucket"              // u8     -- VS related flag for parallelism. `ogg` and `vorbis` set it to '7'.
    ignore = "UseUnicodeResponseFiles",     // bool   -- VS related flag
    ignore = "XMLDocumentationFileName",    // String -- Something related to documentation.
    ignore = "AssemblerListingLocation",    // String -- Set with /Fa flag when `AseemblerOutput` is set, which is not the case here.
)]
pub struct CompilerTool {
    #[append]
    pub additional_options: Option<String>,

    pub optimization: Option<Optimization>,
    pub inline_function_expansion: Option<InlineFunctionExpansion>,
    pub enable_intrinsic_functions: Option<EnableIntrinsicFunctions>,
    pub omit_frame_pointers: Option<OmitFramePointers>,
    pub enable_fiber_safe_optimizations: Option<EnableFiberSafeOptimizations>,
    pub favor_size_or_speed: Option<FavorSizeOrSpeed>,
    pub whole_program_optimization: Option<WholeProgramOptimization>,
    pub string_pooling: Option<StringPooling>,

    pub exception_handling: Option<ExceptionHandling>,

    pub runtime_library: Option<RuntimeLibrary>,
    pub buffer_security_check: Option<BufferSecurityCheck>,
    pub enable_enhanced_instruction_set: Option<EnableEnhancedInstructionSet>,
    pub floating_point_model: Option<FloatingPointModel>,
    pub warning_level: Option<WarningLevel>,
    pub debug_information_format: Option<DebugInformationFormat>,
    pub use_precompiled_header: Option<UsePrecompiledHeader>,
    pub compile_as: Option<CompileAs>,
    pub runtime_type_info: Option<RuntimeTypeInfo>,

    pub minimal_rebuild: Option<MinimalRebuild>,
    pub basic_runtime_checks: Option<BasicRuntimeChecks>,
    pub enable_function_level_linking: Option<EnableFunctionLevelLinking>,
    pub smaller_type_check: Option<SmallerTypeCheck>,
    pub browse_information: Option<BrowseInformation>,
    pub calling_convention: Option<CallingConvention>,
    pub floating_point_exceptions: Option<FloatingPointExceptions>,
    pub force_conformance_in_for_loop_scope: Option<ForceConformanceInForLoopScope>,

    #[unset(GeneratePreprocessedFile::_0)]
    pub generate_preprocessed_file: Option<GeneratePreprocessedFile>,

    pub show_includes: Option<ShowIncludes>,
    pub struct_member_alignment: Option<StructMemberAlignment>,
    pub suppress_startup_banner: Option<SuppressStartupBanner>,
    pub detect_64_bit_portability_problems: Option<Detect64BitPortabilityProblems>,

    pub precompiled_header_through: Option<String>,

    pub object_file: Option<String>,
    pub precompiled_header_file: Option<String>,
    pub program_data_base_file_name: Option<String>,

    pub disable_specific_warnings: Option<Vec<String>>,

    // Requires interpolation: $(SolutionDir)/stlport;
    pub additional_include_directories: Option<Vec<String>>,

    // PreprocessorDefinitions="WIN32;NDEBUG;VOSTOK_STATIC_LIBRARIES;MASTER_GOLD;"
    pub preprocessor_definitions: Option<Vec<String>>,
}

flag_enum! {
    enum Optimization {
        0 => "/Od",
        1 => "/O1",
        2 => "/O2",
        3 => "/Ox",
    }
}
flag_enum! {
    enum InlineFunctionExpansion {
        0 => "",
        1 => "/Ob1",
        2 => "/Ob2",
    }
}
flag_enum! {
    enum EnableIntrinsicFunctions {
        false => "",
        true => "/Oi",
    }
}
flag_enum! {
    enum OmitFramePointers {
        false => "",
        true => "/Oy",
    }
}
flag_enum! {
    enum EnableFiberSafeOptimizations {
        false => "",
        true => "/GT",
    }
}
flag_enum! {
    enum FavorSizeOrSpeed {
        0 => "",
        1 => "/Ot",
        2 => "/Os",
    }
}
flag_enum! {
    enum WholeProgramOptimization {
        false => "",
        true => "/GL",
    }
}
flag_enum! {
    enum StringPooling {
        false => "",
        true => "/GF",
    }
}
flag_enum! {
    enum ExceptionHandling {
        0 => "",
        1 => "/EHsc",
        2 => "/EHa",
    }
}
flag_enum! {
    enum RuntimeLibrary {
        0 => "/MT",
        1 => "/MTd",
        2 => "/MD",
        3 => "/MDd",
    }
}
flag_enum! {
    enum BufferSecurityCheck {
        false => "/GS-",
        true => "",
    }
}
flag_enum! {
    enum EnableEnhancedInstructionSet {
        0 => "",
        1 => "/arch:SSE",
        2 => "/arch:SSE2",
    }
}
flag_enum! {
    enum FloatingPointModel {
        0 => "/fp:precise",
        1 => "/fp:strict",
        2 => "/fp:fast",
    }
}
flag_enum! {
    enum WarningLevel {
        0 => "/W0",
        1 => "/W1",
        2 => "/W2",
        3 => "/W3",
        4 => "/W4",
    }
}
flag_enum! {
    enum DebugInformationFormat {
        0 => "",
        1 => "/Z7",
        3 => "/Zi",
        4 => "/ZI",
    }
}
flag_enum! {
    enum UsePrecompiledHeader {
        0 => "",
        1 => "/Yc",
        2 => "/Yu",
        3 => "/YX", // deprecated: Automatic
    }
}
flag_enum! {
    enum CompileAs {
        0 => "",
        1 => "/TC",
        2 => "/TP",
    }
}
flag_enum! {
    enum RuntimeTypeInfo {
        false => "/GR-",
        true => "", // VS2008 doesn't emit it, since it is the default
    }
}
flag_enum! {
    enum MinimalRebuild {
        false => "",
        true => "/Gm",
    }
}
flag_enum! {
    enum EnableFunctionLevelLinking {
        false => "",
        true => "/Gy",
    }
}
flag_enum! {
    enum SmallerTypeCheck {
        false => "",
        true => "/RTCc",
    }
}
flag_enum! {
    enum FloatingPointExceptions {
        false => "",
        true => "/fp:except",
    }
}
flag_enum! {
    enum ForceConformanceInForLoopScope {
        false => "/Zc:forScope-",
        true => "", // VS2008 doesn't emit it, since it is the default
    }
}
flag_enum! {
    enum ShowIncludes {
        false => "",
        true => "/showIncludes",
    }
}
flag_enum! {
    enum SuppressStartupBanner {
        false => "",
        true => "/nologo",
    }
}
flag_enum! {
    enum BasicRuntimeChecks {
        0 => "",
        1 => "/RTCs",
        2 => "/RTCu",
        3 => "/RTC1",
    }
}
flag_enum! {
    enum BrowseInformation {
        0 => "",
        1 => "/FR",
        2 => "/Fr",
    }
}
flag_enum! {
    enum CallingConvention {
        0 => "/Gd",
        1 => "/Gr",
        2 => "/Gz",
    }
}
flag_enum! {
    enum GeneratePreprocessedFile {
        0 => "",
        1 => "/P",
        2 => "/EP /P",
    }
}
flag_enum! {
    enum StructMemberAlignment {
        0 => "",
        1 => "/Zp1",
        2 => "/Zp2",
        3 => "/Zp4",
        4 => "/Zp8",
        5 => "/Zp16",
    }
}
flag_enum! {
    enum Detect64BitPortabilityProblems {
        false => "",
        true => "/Wp64",
    }
}
flag_enum! {
    enum GenerateProgramDatabase {
        false => "",
        true => "/FD",
    }
}
flag_enum! {
    enum CompileOnly {
        false => "",
        true => "/c",
    }
}

/// Result of lowering a single merged [`CompilerTool`] to flags, including the
/// structured data the generator's preprocessor needs (include dirs, defines).
pub struct ClFlags {
    pub flags: Flags,
    pub pch_output: Option<PathBuf>,
    pub fd_path: Option<String>,
    pub include_dirs: Vec<String>,
    pub defines: Vec<String>,
}

impl CompilerTool {
    pub fn to_flags(
        &self,
        cfg: &Configuration,
        vcproject: &VCProject,
        env: MsBuildEnvironment,
    ) -> Vec<ClGroup> {
        let mut result: Vec<ClGroup> = vec![];
        let mut pch_path: Option<PathBuf> = None;

        let mut tool_n_files = Self::parse_files(&vcproject.files, &cfg.name)
            .into_iter()
            .fold(
                HashMap::<CompilerTool, Vec<&str>>::new(),
                |mut map, (k, v)| {
                    map.entry(self.clone().merge(k)).or_default().extend(v);
                    map
                },
            )
            .into_iter()
            .collect::<Vec<_>>();

        // PCH creation (/Yc) must be compiled before everything else
        tool_n_files.sort_by_key(|(tool, _)| {
            if matches!(tool.use_precompiled_header, Some(UsePrecompiledHeader::_1)) {
                0usize
            } else {
                1usize
            }
        });

        for (tool, files) in tool_n_files {
            let mut env = env;
            if files.len() == 1 {
                let input_name = Path::new(&files[0])
                    .file_stem()
                    .map(|x| x.to_str().expect("Path was constructed from String"))
                    .unwrap_or(files[0]);

                env.input_name = input_name;
            } else {
                env.input_name = "<poison>";
            }

            let ClFlags {
                mut flags,
                pch_output,
                fd_path,
                include_dirs,
                defines,
            } = tool.to_flags_impl(cfg, env);
            flags.files = files.iter().map(|file| file.to_string()).collect();

            let pch_input = if pch_output.is_none()
                && matches!(
                    tool.use_precompiled_header,
                    Some(UsePrecompiledHeader::_2 | UsePrecompiledHeader::_3)
                ) {
                pch_path.clone()
            } else {
                None
            };

            if let Some(p) = &pch_output {
                pch_path = Some(p.clone());
            }

            result.push(ClGroup {
                flags,
                pch_output,
                pch_input,
                fd_path,
                include_dirs,
                defines,
                header_deps: vec![],
            });
        }

        result
    }

    pub fn to_flags_impl(&self, cfg: &Configuration, env: MsBuildEnvironment) -> ClFlags {
        let Self {
            additional_options,
            optimization,
            inline_function_expansion,
            enable_intrinsic_functions,
            omit_frame_pointers,
            enable_fiber_safe_optimizations,
            favor_size_or_speed,
            whole_program_optimization,
            string_pooling,
            exception_handling,
            runtime_library,
            buffer_security_check,
            enable_enhanced_instruction_set,
            floating_point_model,
            warning_level,
            debug_information_format,
            use_precompiled_header,
            compile_as,
            runtime_type_info,
            minimal_rebuild,
            basic_runtime_checks,
            enable_function_level_linking,
            smaller_type_check,
            browse_information,
            calling_convention,
            floating_point_exceptions,
            force_conformance_in_for_loop_scope,
            generate_preprocessed_file,
            show_includes,
            struct_member_alignment,
            suppress_startup_banner: _,
            detect_64_bit_portability_problems,
            //
            precompiled_header_file,
            object_file,
            program_data_base_file_name,
            //
            precompiled_header_through,
            disable_specific_warnings,
            additional_include_directories,
            preprocessor_definitions,
        } = self;

        let mut additional_include_directories =
            additional_include_directories.clone().unwrap_or_default();
        let mut preprocessor_definitions = preprocessor_definitions.clone().unwrap_or_default();

        if let Some(inherited_property_sheets) = &cfg.inherited_property_sheets {
            let mut vc_version = 0;

            for inherited_property_sheet in inherited_property_sheets {
                match inherited_property_sheet.as_str() {
                    "$(VCInstallDir)VCProjectDefaults\\UpgradeFromVC60.vsprops"
                    | "UpgradeFromVC60.vsprops" => {
                        assert_eq!(vc_version, 0);
                        vc_version = 60;
                    }
                    "$(VCInstallDir)VCProjectDefaults\\UpgradeFromVC70.vsprops"
                    | "UpgradeFromVC70.vsprops" => {
                        assert_eq!(vc_version, 0);
                        vc_version = 70;
                    }
                    "$(VCInstallDir)VCProjectDefaults\\UpgradeFromVC71.vsprops"
                    | "UpgradeFromVC71.vsprops" => {
                        assert_eq!(vc_version, 0);
                        vc_version = 71;
                    }
                    "..\\libogg.vsprops" => {
                        for additional_include_directory in [
                            r#"..\..\..\..\libogg-1.1.3\include"#, // r#"..\..\..\..\libogg-$(LIBOGG_VERSION)\include"#,
                            r#"..\..\..\..\ogg\include"#,
                            r#"..\..\..\..\..\..\..\core\ogg\libogg\include"#,
                        ] {
                            additional_include_directories
                                .push(additional_include_directory.to_string());
                        }
                    }
                    _ => unreachable!(
                        "Requires to properly handle .vsprops files, relative paths and user macro. Currently hardcoded for my cases"
                    ),
                }
            }
            match vc_version {
                0 => (),
                60 => preprocessor_definitions.push("_VC80_UPGRADE=0x0600".to_string()),
                70 => preprocessor_definitions.push("_VC80_UPGRADE=0x0700".to_string()),
                71 => preprocessor_definitions.push("_VC80_UPGRADE=0x0710".to_string()),
                _ => unreachable!(),
            }
        }
        //
        //
        //

        let exception_handling = Some(exception_handling.unwrap_or(ExceptionHandling::_1));
        let precompiled_header_through =
            precompiled_header_through.as_deref().unwrap_or("stdafx.h");

        let whole_program_optimization =
            match (cfg.whole_program_optimization, whole_program_optimization) {
                (Some(true), None) => Some(WholeProgramOptimization::_1),
                _ => *whole_program_optimization,
            };

        // TODO: This needs to be solved differently. As this can still result in multiple invocations of the compiler.
        let generate_program_database = match debug_information_format {
            Some(DebugInformationFormat::_0) | None => None,
            _ => Some(GenerateProgramDatabase::_1),
        };

        let compile_only = Some(CompileOnly::_1);

        let use_precompiled_header_flag = match use_precompiled_header {
            Some(use_precompiled_header)
                if !matches!(*use_precompiled_header, UsePrecompiledHeader::_0) =>
            {
                let mut flag = use_precompiled_header.as_str().to_string();
                flag.push('"');
                flag.push_str(precompiled_header_through);
                flag.push('"');
                Some(flag)
            }
            _ => None,
        };

        let object_file = object_file.as_deref().unwrap_or("$(IntDir)");

        let precompiled_header_file = match (precompiled_header_file, use_precompiled_header) {
            (None, Some(use_precompiled_header))
                if !matches!(*use_precompiled_header, UsePrecompiledHeader::_0) =>
            {
                Some("$(IntDir)\\$(TargetName).pch")
            }
            _ => precompiled_header_file.as_deref(),
        };

        let program_data_base_file_name = program_data_base_file_name
            .as_deref()
            .unwrap_or("$(IntDir)\\vc90.pdb");

        //
        //
        //

        let mut rsp_flags: Vec<String> = vec![];

        append_flags!(
            rsp_flags,
            [
                optimization,
                inline_function_expansion,
                enable_intrinsic_functions,
                favor_size_or_speed,
                omit_frame_pointers,
                enable_fiber_safe_optimizations,
                whole_program_optimization,
            ]
        );

        // Resolved include search dirs, surfaced for the generator's preprocessor.
        let mut include_dirs: Vec<String> = vec![];
        for include_directory in additional_include_directories
            .iter()
            .filter(|s| !s.is_empty())
        {
            let include_directory = env.expand(include_directory.trim());
            let include_directory = include_directory.trim().trim_matches('"');

            include_dirs.push(include_directory.to_string());

            // TODO: This needs proper handling
            if include_directory.ends_with('\\') {
                rsp_flags.push(format!("/I \"{include_directory}\\\""));
            } else {
                rsp_flags.push(format!("/I \"{include_directory}\""));
            }
        }

        let preprocessor_definitions = {
            match cfg.configuration_type {
                ConfigurationType::_2 => preprocessor_definitions.push("_WINDLL".to_string()),
                _ => (),
            }

            if let Some(character_set) = cfg.character_set {
                match character_set {
                    CharacterSet::_1 => {
                        preprocessor_definitions.push("_UNICODE".to_string());
                        preprocessor_definitions.push("UNICODE".to_string());
                    }
                    CharacterSet::_2 => preprocessor_definitions.push("_MBCS".to_string()),
                    _ => (),
                }
            }
            preprocessor_definitions
        };

        // Surface the raw defines for the generator's preprocessor (`#if`/`#ifdef`).
        let defines: Vec<String> = preprocessor_definitions
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect();
        for preprocessor_definition in preprocessor_definitions.iter().filter(|s| !s.is_empty()) {
            rsp_flags.push(format!("/D \"{preprocessor_definition}\""));
        }

        append_flags!(
            rsp_flags,
            [
                string_pooling,
                generate_program_database,
                exception_handling,
                runtime_library,
                buffer_security_check,
                enable_enhanced_instruction_set,
                enable_function_level_linking,
                floating_point_model,
                runtime_type_info,
                use_precompiled_header_flag,
            ]
        );

        // /Fp"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Master Gold/fs\vostok_fs-static-gold.pch"
        let pch_output = if let Some(precompiled_header_file) = precompiled_header_file
            && !precompiled_header_file.is_empty()
        {
            let expanded = env.expand(precompiled_header_file);
            rsp_flags.push(format!("/Fp\"{expanded}\""));
            if matches!(use_precompiled_header, Some(UsePrecompiledHeader::_1)) {
                Some(PathBuf::from(expanded))
            } else {
                None
            }
        } else {
            None
        };

        // /Fo"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Master Gold/fs\\"
        let output_file = if !object_file.is_empty() {
            let is_specific_obj = object_file.to_ascii_lowercase().ends_with(".obj");
            let object_file = env.expand(object_file);

            let mut fo_path = object_file.clone();
            if !is_specific_obj && !fo_path.ends_with('/') {
                fo_path = format!("{}\\\\", fo_path.trim_end_matches('\\'));
            }
            rsp_flags.push(format!("/Fo\"{fo_path}\""));
            object_file
        } else {
            String::new() // TODO
        };

        // /Fd"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Master Gold/fs\vc90.pdb"
        let fd_path = if !program_data_base_file_name.is_empty() {
            let fd_path = env.expand(program_data_base_file_name);
            rsp_flags.push(format!("/Fd\"{fd_path}\""));
            Some(fd_path)
        } else {
            None
        };

        append_flags!(
            rsp_flags,
            [
                warning_level,
                compile_only,
                debug_information_format,
                minimal_rebuild,
                basic_runtime_checks,
                smaller_type_check,
                browse_information,
                calling_convention,
                compile_as,
                floating_point_exceptions,
                force_conformance_in_for_loop_scope,
                generate_preprocessed_file,
                show_includes,
                struct_member_alignment,
                detect_64_bit_portability_problems,
            ]
        );

        for disable_specific_warning in disable_specific_warnings
            .iter()
            .flatten()
            .filter(|s| !s.is_empty())
        {
            rsp_flags.push(format!("/wd{disable_specific_warning}"));
        }

        if let Some(additional_options) = additional_options
            && !additional_options.is_empty()
        {
            rsp_flags.push(additional_options.clone());
        }
        ClFlags {
            flags: Flags {
                output_file,
                import_library: None,
                flags: "@$(RspFile) /nologo /errorReport:prompt".to_string(),
                rsp_flags: rsp_flags.join(" "),
                files: vec![],
            },
            pch_output,
            fd_path,
            include_dirs,
            defines,
        }
    }

    fn parse_files<'a>(
        files: &'a Files,
        configuration_platform: &str,
    ) -> HashMap<CompilerTool, Vec<&'a str>> {
        let mut result = HashMap::new();

        for filter in &files.filters {
            Self::parse_filter(&mut result, filter, configuration_platform);
        }

        for file in &files.files {
            Self::parse_file(&mut result, file, configuration_platform);
        }
        for files in result.values_mut() {
            // It is possible for the same file to repeat multiple times in `Files` tag.
            // This can be seen in `render_engine_pc_dx11.vcproj` for `effect_editor_shader_complexity.cpp`.
            let mut conflicts = HashSet::new();
            files.retain(|file| conflicts.insert(*file));
        }

        result
    }

    fn parse_filter<'a>(
        result: &mut HashMap<CompilerTool, Vec<&'a str>>,
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
        result: &mut HashMap<CompilerTool, Vec<&'a str>>,
        file: &'a File,
        configuration_platform: &str,
    ) {
        for file in &file.files {
            Self::parse_file(result, file, configuration_platform);
        }

        let file_extension = Path::new(&file.relative_path)
            .extension()
            .map(OsStr::as_encoded_bytes);

        // TODO: This is actually incorrect. Because sometimes msbuild doesn't set anything:
        //
        // [LibJPEG]: /O2 /Ob2 /Oi /Ot /Oy /GT /GL /I "..\zlib" /D "WIN32" /D "NDEBUG" /D "_LIB" /D "_CRT_SECURE_NO_DEPRECATE" /D "_VC80_UPGRADE=0x0710" /D "_MBCS" /GF /FD /MT /GS- /arch:SSE /fp:fast /Fp".\Release/LibJPEG.pch" /Fo"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Release/LibJPEG\" /Fd"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Release/LibJPEG\vc90.pdb" /W3 /c /Zi  /MP
        // [LibPNG]: /O2 /Ob2 /Oi /Ot /Oy /GT /GL /I "E:\Projects\vostok\sources\" /D "WIN32" /D "NDEBUG" /D "_LIB" /D "_CRT_SECURE_NO_DEPRECATE" /D "_VC80_UPGRADE=0x0710" /D "_MBCS" /GF /FD /MT /GS- /arch:SSE /fp:fast /Fp".\Release/LibPNG.pch" /Fo"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Release/LibPNG\" /Fd"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Release/LibPNG\vc90.pdb" /W3 /c /Zi  /MP
        // [LibTIFF]: /O2 /Ob2 /Oi /Ot /Oy /GT /GL /I "..\libtiff\libtiff" /I "E:\Projects\vostok\sources\" /D "WIN32" /D "NDEBUG" /D "_LIB" /D "_CRT_SECURE_NO_DEPRECATE" /D "_VC80_UPGRADE=0x0710" /D "_MBCS" /GF /FD /MT /GS- /arch:SSE /fp:fast /Fp".\Release/LibTIFF.pch" /Fo"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Release/LibTIFF\" /Fd"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Release/LibTIFF\vc90.pdb" /W3 /c /Zi  /MP
        //
        // We are relying on it to always be set though
        let compile_as = match file_extension {
            Some(b"c") => CompileAs::_1,
            Some(b"cpp" | b"cxx" | b"cc") => CompileAs::_2,
            Some(
                b"h" | b"hpp" | b"ico" | b"rc" | b"bmp" | b"avi" | b"ampl" | b"txt" | b"inl"
                | b"def",
            ) => return,
            // "Actually can be any kind of file, but I want to be explicit for vostok project.
            _ => {
                eprintln!("Couldn't parse extension: {}", file.relative_path);
                return;
            }
        };

        let config = file
            .file_configurations
            .iter()
            .filter(|config| config.name == configuration_platform)
            .find(|config| config.tool.is_some());

        let mut cl_tool = match config {
            Some(config) if config.excluded_from_build == Some(true) => return,
            Some(config) => config.tool.clone(),
            None => None,
        }
        .unwrap_or_default();

        cl_tool.compile_as = Some(compile_as);

        result
            .entry(cl_tool)
            .or_default()
            .push(file.relative_path.as_str());
    }
}

#[cfg(test)]
mod tests {
    use crate::vcproj::{MsBuildEnvironment, VCProject};

    const PCH_VCPROJ: &str = r#"<?xml version="1.0" encoding="Windows-1252"?>
<VisualStudioProject
    ProjectType="Visual C++"
    Version="9,00"
    Name="myproject"
    ProjectGUID="{00000000-0000-0000-0000-000000000001}"
    RootNamespace="myproject"
    TargetFrameworkVersion="196613"
    >
    <Platforms><Platform Name="Win32"/></Platforms>
    <Configurations>
        <Configuration
            Name="Release|Win32"
            OutputDirectory="C:\out"
            IntermediateDirectory="C:\int\myproject"
            ConfigurationType="4"
            >
            <Tool Name="VCCLCompilerTool"/>
            <Tool Name="VCLibrarianTool" OutputFile="C:\out\myproject.lib"/>
        </Configuration>
    </Configurations>
    <Files>
        <File RelativePath=".\pch.cpp">
            <FileConfiguration Name="Release|Win32">
                <Tool Name="VCCLCompilerTool" UsePrecompiledHeader="1"/>
            </FileConfiguration>
        </File>
        <File RelativePath=".\use_pch.cpp">
            <FileConfiguration Name="Release|Win32">
                <Tool Name="VCCLCompilerTool" UsePrecompiledHeader="2"/>
            </FileConfiguration>
        </File>
        <File RelativePath=".\other.cpp"/>
    </Files>
</VisualStudioProject>"#;

    #[test]
    fn pch_yc_yu_and_independent_file_produce_three_groups() {
        let vcproj = VCProject::parse_xml(PCH_VCPROJ).unwrap();
        let cfg = &vcproj.configurations[0];
        let env = MsBuildEnvironment::get(&vcproj.name, cfg, r"C:\solution\");
        let cl = cfg.compiler_tool.as_ref().unwrap();

        let groups = cl.to_flags(cfg, &vcproj, env);

        // Yc group + Yu group + independent other.cpp = 3 groups.
        assert_eq!(groups.len(), 3, "expected Yc, Yu, and independent groups");

        // Sort puts Yc (/Yc = key 0) first.
        let yc_group = &groups[0];
        assert!(yc_group.flags.files.iter().any(|f| f.contains("pch.cpp")));
        assert!(
            yc_group.pch_output.is_some(),
            "Yc group should have pch_output"
        );
        assert!(yc_group.pch_input.is_none());

        // Yu group consumes the PCH.
        let yu_group = &groups[1];
        assert!(
            yu_group
                .flags
                .files
                .iter()
                .any(|f| f.contains("use_pch.cpp"))
        );
        assert!(
            yu_group.pch_input.is_some(),
            "Yu group should have pch_input"
        );
        assert!(yu_group.pch_output.is_none());

        // The non-PCH file is independent.
        let other_group = &groups[2];
        assert!(
            other_group
                .flags
                .files
                .iter()
                .any(|f| f.contains("other.cpp"))
        );
        assert!(other_group.pch_input.is_none());
        assert!(other_group.pch_output.is_none());
    }
}

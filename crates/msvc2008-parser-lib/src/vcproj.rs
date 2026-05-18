use std::fmt::Write;
use std::{collections::HashMap, ffi::OsStr, path::Path};

use anyhow::Context;
use msvc2008_parser_proc::{ParseXml, flag_enum};

/// MSBuild/Visual Studio macros expanded at build time in .vcxproj files.
#[derive(Clone, Copy, Debug)]
pub struct MsBuildEnvironment<'a> {
    /// Directory of the .sln file, with trailing backslash.
    /// Example: solution at `C:\projects\App\App.sln` -> `C:\projects\App\`.
    pub solution_dir: &'a str,

    /// Intermediate output directory (object files, etc.), with trailing backslash.
    /// Example: `obj\Release\`.
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

#[derive(Debug, ParseXml)]
pub struct VCProject {
    pub name: String,
    pub project_type: String,
    pub version: String,
    #[rename("ProjectGUID")]
    pub guid: uuid::Uuid,
    pub root_namespace: String,

    // TODO: Figure out whether we should care for this flag or not.
    // Set in `freeimage\LibOpenJPEG\LibOpenJPEG.vcproj`
    pub keyword: Option<String>,
    pub target_framework_version: String,

    #[skip]
    pub platforms: Vec<Platform>,
    #[skip]
    pub configurations: Vec<Configuration>,
    #[skip]
    pub files: Files,
}

#[derive(Debug, ParseXml)]
pub struct Configuration {
    /// Example: `Release|Win32`.
    pub name: String,

    // Requires interpolation:
    // OutputDirectory="$(SolutionDir)../binaries/$(PlatformName)/intermediates/$(ConfigurationName)/$(ProjectName)"
    pub output_directory: Option<String>,

    // Requires interpolation:
    // IntermediateDirectory="$(SolutionDir)../binaries/$(PlatformName)/intermediates/$(ConfigurationName)/$(ProjectName)"
    pub intermediate_directory: Option<String>,

    pub configuration_type: ConfigurationType,
    pub character_set: Option<CharacterSet>,
    pub whole_program_optimization: Option<bool>,
    pub managed_extensions: Option<ManagedExtensions>,
    #[rename("ATLMinimizesCRunTimeLibraryUsage")]
    pub atl_minimizes_c_runtime_library_usage: Option<bool>,
    pub delete_extensions_on_clean: Option<String>,
    pub inherited_property_sheets: Option<Vec<String>>,
    #[rename("UseOfATL")]
    pub use_of_atl: Option<UseOfATL>,
    #[rename("UseOfMFC")]
    pub use_of_mfc: Option<UseOfMFC>,

    #[skip]
    pub compiler_tool: Option<CompilerTool>,
    #[skip]
    pub lib_tool: Option<LibTool>,
    #[skip]
    pub linker_tool: Option<LinkerTool>,
}

#[derive(Debug, ParseXml)]
pub struct Platform {
    pub name: String,
}

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

#[derive(Debug, ParseXml, Default)]
#[parse_xml(tag = "VCLinkerTool", ignore = "Name")]
pub struct LinkerTool {
    pub additional_options: Option<String>,
    pub additional_dependencies: Option<Vec<String>>,
    pub output_file: Option<String>,
    pub link_incremental: Option<LinkIncremental>,
    pub additional_library_directories: Option<Vec<String>>,
    pub ignore_default_library_names: Option<Vec<String>>,
    pub module_definition_file: Option<String>,
    pub generate_debug_information: Option<bool>,
    pub program_database_file: Option<String>,
    pub generate_map_file: Option<bool>,
    pub map_file_name: Option<String>,
    pub map_exports: Option<bool>,
    pub sub_system: Option<SubSystem>,
    pub large_address_aware: Option<LargeAddressAware>,
    pub optimize_references: Option<OptimizeReferences>,
    #[rename("EnableCOMDATFolding")]
    pub enable_comdat_folding: Option<EnableCOMDATFolding>,
    pub randomized_base_address: Option<RandomizedBaseAddress>,
    pub data_execution_prevention: Option<DataExecutionPrevention>,
    pub import_library: Option<String>,
    pub target_machine: Option<TargetMachine>,
    pub assembly_debug: Option<AssemblyDebug>,
    pub assembly_link_resource: Option<String>,
    pub base_address: Option<String>,
    #[rename("CLRThreadAttribute")]
    pub clr_thread_attribute: Option<CLRThreadAttribute>,
    #[rename("DelayLoadDLLs")]
    pub delay_load_dlls: Option<Vec<String>>,
    pub embed_managed_resource_file: Option<String>,
    pub entry_point_symbol: Option<String>,
    pub fixed_base_address: Option<FixedBaseAddress>,
    pub generate_manifest: Option<bool>,
    pub ignore_import_library: Option<bool>,
    pub optimize_for_windows98: Option<OptimizeForWindows98>,
    #[rename("SupportUnloadOfDelayLoadedDLL")]
    pub support_unload_of_delay_loaded_dll: Option<bool>,
    pub version: Option<String>,
}

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

#[derive(Debug, Default)]
pub struct Files {
    pub filters: Vec<Filter>,
    pub files: Vec<File>,
}

#[derive(Debug, ParseXml)]
pub struct Filter {
    pub name: String,
    pub filter: Option<Vec<String>>,
    pub unique_identifier: Option<String>,

    #[skip]
    pub filters: Vec<Filter>,
    #[skip]
    pub files: Vec<File>,
}

#[derive(Debug, ParseXml)]
pub struct File {
    pub relative_path: String,
    pub file_type: Option<u8>, // NOTE: Seems to not to affect flags
    pub sub_type: Option<String>,

    #[skip]
    pub file_configurations: Vec<FileConfiguration>,
    #[skip]
    pub files: Vec<File>,
}

#[derive(Debug, ParseXml)]
pub struct FileConfiguration {
    pub name: String,
    pub excluded_from_build: Option<bool>,

    #[skip]
    pub tool: Option<CompilerTool>,
}

//
// Configuration flags
//

flag_enum! {
    // If new types are added `MsBuildEnvironment::get` should be updated accordingly.
    enum ConfigurationType {
        1 => "exe",
        2 => "dll",
        4 => "lib",
        10 => "utility",
    }
}
flag_enum! {
    enum CharacterSet {
        0 => "",
        1 => "_UNICODE",
        2 => "_MBCS",
    }
}
flag_enum! {
    enum ManagedExtensions {
        0 => "",
        1 => "/clr",
    }
}
flag_enum! {
    enum UseOfATL {
        0 => "",
        1 => "/ATL:static",
        2 => "/ATL:dynamic",
    }
}
flag_enum! {
    enum UseOfMFC {
        -1 => "", // Means not applicable. Set specifically on Xbox.
        0 => "",
        1 => "/MFC:static",
        2 => "/MFC:dynamic",
    }
}

//
// Compiler flags
//

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

//

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

//
// Linker flags
//

flag_enum! {
    enum LinkIncremental {
        0 => "",
        1 => "/INCREMENTAL:NO",
        2 => "/INCREMENTAL",
    }
}
flag_enum! {
    enum SubSystem {
        0 => "",
        1 => "/SUBSYSTEM:CONSOLE",
        2 => "/SUBSYSTEM:WINDOWS",
        3 => "/SUBSYSTEM:NATIVE",
        4 => "/SUBSYSTEM:EFI_APPLICATION",
        5 => "/SUBSYSTEM:EFI_BOOT_SERVICE_DRIVER",
        6 => "/SUBSYSTEM:EFI_ROM",
        7 => "/SUBSYSTEM:EFI_RUNTIME_DRIVER",
        8 => "/SUBSYSTEM:WINDOWSCE",
    }
}
flag_enum! {
    enum LargeAddressAware {
        0 => "",
        1 => "/LARGEADDRESSAWARE:NO",
        2 => "/LARGEADDRESSAWARE",
    }
}
flag_enum! {
    enum OptimizeReferences {
        0 => "",
        1 => "/OPT:NOREF",
        2 => "/OPT:REF",
    }
}
flag_enum! {
    enum EnableCOMDATFolding {
        0 => "",
        1 => "/OPT:NOICF",
        2 => "/OPT:ICF",
    }
}
flag_enum! {
    enum RandomizedBaseAddress {
        0 => "",
        1 => "/DYNAMICBASE:NO",
        2 => "/DYNAMICBASE",
    }
}
flag_enum! {
    enum DataExecutionPrevention {
        0 => "",
        1 => "/NXCOMPAT:NO",
        2 => "/NXCOMPAT",
    }
}
flag_enum! {
    enum TargetMachine {
        0 => "",
        1 => "/MACHINE:X86",
        3 => "/MACHINE:ARM",
        4 => "/MACHINE:EBC",
        5 => "/MACHINE:IA64",
        7 => "/MACHINE:MIPS",
        8 => "/MACHINE:MIPS16",
        9 => "/MACHINE:MIPSFPU",
        10 => "/MACHINE:MIPSFPU16",
        14 => "/MACHINE:SH4",
        16 => "/MACHINE:THUMB",
        17 => "/MACHINE:X64",
    }
}
flag_enum! {
    enum AssemblyDebug {
        0 => "",
        1 => "/ASSEMBLYDEBUG",
        2 => "/ASSEMBLYDEBUG:DISABLE",
    }
}
flag_enum! {
    enum CLRThreadAttribute {
        0 => "/CLRTHREADATTRIBUTE:NONE",
        1 => "/CLRTHREADATTRIBUTE:MTA",
        2 => "/CLRTHREADATTRIBUTE:STA",
    }
}
flag_enum! {
    enum FixedBaseAddress {
        0 => "",
        1 => "/FIXED:NO",
        2 => "/FIXED",
    }
}
flag_enum! {
    enum OptimizeForWindows98 {
        0 => "",
        1 => "",  // MSVS only, no linker flag
        2 => "",  // MSVS only, no linker flag
    }
}

//
// Flag generation logic
//

macro_rules! flags {
    ($($opt:expr),* $(,)?) => {{
        let mut v = vec![$($opt.as_ref().map(|v| v.as_str()).unwrap_or(""),)*];
        v.retain(|s| !s.is_empty());
        v.join(" ")
    }};
}

impl CompilerTool {
    pub fn to_flags(
        &self,
        cfg: &Configuration,
        vcproject: &VCProject,
        env: MsBuildEnvironment,
    ) -> Vec<(String, Vec<String>)> {
        let mut result = vec![];

        let mut tool_n_files = Self::parse_files(&vcproject.files, &cfg.name)
            .into_iter()
            .map(|(k, v)| (self.clone().merge(k), v))
            .collect::<HashMap<_, _>>()
            .into_iter()
            .collect::<Vec<_>>();

        // "Yc" > "Yu"
        tool_n_files.sort_by_key(|tool| std::cmp::Reverse(tool.0.use_precompiled_header));

        for (tool, files) in tool_n_files {
            let mut env = env;
            if files.len() == 1 {
                let input_name = Path::new(&files[0])
                    .file_stem()
                    .map(|x| x.to_str().expect("Path was constructed from String"))
                    .unwrap_or(files[0].as_str());

                env.input_name = input_name;
            } else {
                env.input_name = "<poison>";
            }

            let flags = tool.to_flags_impl(cfg, env);
            result.push((flags, files));
        }

        result
    }

    pub fn to_flags_impl(&self, cfg: &Configuration, env: MsBuildEnvironment) -> String {
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
            suppress_startup_banner,
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

        let mut result = flags![
            optimization,
            inline_function_expansion,
            enable_intrinsic_functions,
            favor_size_or_speed,
            omit_frame_pointers,
            enable_fiber_safe_optimizations,
            whole_program_optimization,
        ];

        for include_directory in additional_include_directories
            .iter()
            .filter(|s| !s.is_empty())
        {
            result.push(' ');
            result.push_str("/I ");
            if !include_directory.starts_with('"') {
                result.push('"');
            }
            result.push_str(&env.expand(include_directory.trim()));
            if !include_directory.ends_with('"') {
                result.push('"');
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

        for preprocessor_definition in preprocessor_definitions.iter().filter(|s| !s.is_empty()) {
            result.push(' ');
            result.push_str("/D ");
            result.push('"');
            result.push_str(&preprocessor_definition);
            result.push('"');
        }

        result.push(' ');

        result.push_str(&flags![
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
        ]);

        // /Fp"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Master Gold/fs\vostok_fs-static-gold.pch"
        if let Some(precompiled_header_file) = precompiled_header_file
            && !precompiled_header_file.is_empty()
        {
            result.push(' ');
            result.push_str("/Fp");
            result.push('"');
            result.push_str(&env.expand(precompiled_header_file));
            result.push('"');
        }

        // /Fo"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Master Gold/fs\\"
        if !object_file.is_empty() {
            let object_file = env.expand(object_file);

            result.push(' ');
            result.push_str("/Fo");
            result.push('"');
            result.push_str(&object_file);

            // TODO: Incorrect because of two reasons.
            // msbuild doesn't actually match on extension. It does it somehow differently:
            // Fo"E:\Projects\vostok\sources\/../binaries/Win32/intermediates/Release (static)/lua.5.1.4\"
            //
            // Here extension would be .4, which is wrong :)
            //
            // Also / as an end counts, not just \.
            if Path::new(&object_file).extension().is_none() && !object_file.ends_with('\\') {
                result.push('\\');
            }
            result.push('"');
        }

        // /Fd"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Master Gold/fs\vc90.pdb"
        if !program_data_base_file_name.is_empty() {
            result.push(' ');
            result.push_str("/Fd");
            result.push('"');
            result.push_str(&env.expand(program_data_base_file_name));
            result.push('"');
        }
        result.push(' ');

        result.push_str(&flags![
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
        ]);

        for disable_specific_warning in disable_specific_warnings
            .iter()
            .flatten()
            .filter(|s| !s.is_empty())
        {
            result.push(' ');
            result.push_str("/wd");
            result.push_str(disable_specific_warning);
        }

        if let Some(additional_options) = additional_options
            && !additional_options.is_empty()
        {
            result.push(' ');
            result.push(' ');
            result.push_str(additional_options);
        }

        for flag in suppress_startup_banner.iter() {
            result.push(' ');
            result.push_str(flag.as_str());
        }

        result.trim_start().to_string()
    }

    fn parse_files(
        files: &Files,
        configuration_platform: &str,
    ) -> HashMap<CompilerTool, Vec<String>> {
        let mut result = HashMap::new();

        for file in &files.files {
            Self::parse_file(&mut result, file, configuration_platform);
        }

        for filter in &files.filters {
            Self::parse_filter(&mut result, filter, configuration_platform);
        }

        result
    }

    fn parse_filter(
        result: &mut HashMap<CompilerTool, Vec<String>>,
        filter: &Filter,
        configuration_platform: &str,
    ) {
        for file in &filter.files {
            Self::parse_file(result, file, configuration_platform);
        }

        for filter in &filter.filters {
            Self::parse_filter(result, filter, configuration_platform);
        }
    }

    fn parse_file(
        result: &mut HashMap<CompilerTool, Vec<String>>,
        file: &File,
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
            Some(b"cpp") => CompileAs::_2,
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
            .push(file.relative_path.clone());
    }
}

impl LibTool {
    pub fn to_flags(
        &self,
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

        result
    }
}

//
// Custom parser logic
//

impl VCProject {
    pub fn parse_xml(input: &str) -> anyhow::Result<Self> {
        let xml = roxmltree::Document::parse(input)?;
        let root = xml.root_element();
        if root.tag_name().name() != "VisualStudioProject" {
            anyhow::bail!("Expected 'VisualStudioProject' as a root object")
        }

        let mut this = Self::parse_xml_inner(root)?;

        for child in root.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "Platforms" => {
                    for node in child
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == "Platform")
                    {
                        this.platforms.push(Platform::parse_xml(node)?);
                    }
                }
                "Configurations" => {
                    for node in child
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == "Configuration")
                    {
                        this.configurations.push(Configuration::parse_xml(node)?);
                    }
                }
                "Files" => {
                    this.files = Files::parse_xml(child)?;
                }
                "ToolFiles" => {
                    if child.children().any(|n| n.is_element()) {
                        anyhow::bail!("Expected '{}' to be empty", child.tag_name().name());
                    }
                }
                "References" => {
                    // TODO: BugTrapN.vcproj
                    // <References>
                    // 	<AssemblyReference
                    // 		RelativePath="System.dll"
                    // 		AssemblyName="System, Version=2.0.0.0, PublicKeyToken=b77a5c561934e089, processorArchitecture=MSIL"
                    // 		MinFrameworkVersion="131072"
                    // 	/>
                    // 	<AssemblyReference
                    // 		RelativePath="System.Windows.Forms.dll"
                    // 		AssemblyName="System.Windows.Forms, Version=2.0.0.0, PublicKeyToken=b77a5c561934e089, processorArchitecture=MSIL"
                    // 		MinFrameworkVersion="131072"
                    // 	/>
                    // </References>
                }
                "Globals" => {
                    // TODO: 'ode\sources\ode.vcproj'
                    // <Globals>
                    // 	<Global
                    // 		Name="DevPartner_IsInstrumented"
                    // 		Value="0"
                    // 	/>
                    // </Globals>
                }
                "" => (),
                tag_name => anyhow::bail!("Unexpected tag name: '{tag_name}'"),
            }
        }

        Ok(this)
    }
}

impl Configuration {
    pub fn parse_xml(node: roxmltree::Node) -> anyhow::Result<Self> {
        let mut this = Self::parse_xml_inner(node)?;

        for child in node
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "Tool")
        {
            match child.attribute("Name") {
                Some("VCCLCompilerTool") => {
                    let compiler_tool = CompilerTool::parse_xml(child)?;
                    this.compiler_tool = Some(compiler_tool);
                }
                Some("VCLibrarianTool") => {
                    let lib_tool = LibTool::parse_xml(child)?;
                    this.lib_tool = Some(lib_tool);
                }
                Some("VCLinkerTool") => {
                    let linker_tool = LinkerTool::parse_xml(child)?;
                    this.linker_tool = Some(linker_tool);
                }
                _ => (),
            }
        }

        Ok(this)
    }
}

impl Files {
    pub fn parse_xml(node: roxmltree::Node) -> anyhow::Result<Self> {
        let mut filters = vec![];
        let mut files = vec![];

        for child in node.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "Filter" => filters.push(Filter::parse_xml(child)?),
                "File" => files.push(File::parse_xml(child)?),
                tag => anyhow::bail!("Unexpected tag in Files: '{tag}'"),
            }
        }

        Ok(Self { filters, files })
    }
}

impl Filter {
    pub fn parse_xml(node: roxmltree::Node) -> anyhow::Result<Self> {
        let mut this = Self::parse_xml_inner(node)?;

        for child in node.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "Filter" => this.filters.push(Filter::parse_xml(child)?),
                "File" => this.files.push(File::parse_xml(child)?),
                tag => anyhow::bail!("Unexpected tag in Filter: '{tag}'"),
            }
        }

        Ok(this)
    }
}

impl File {
    pub fn parse_xml(node: roxmltree::Node) -> anyhow::Result<Self> {
        let mut this = Self::parse_xml_inner(node)?;

        for child in node.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "FileConfiguration" => {
                    let file_configuration = FileConfiguration::parse_xml(child)?;
                    this.file_configurations.push(file_configuration)
                }
                "File" => this.files.push(File::parse_xml(child)?),
                "Tool" => {}
                tag => anyhow::bail!("Unexpected tag in File: '{tag}'"),
            }
        }

        Ok(this)
    }
}

impl FileConfiguration {
    pub fn parse_xml(node: roxmltree::Node) -> anyhow::Result<Self> {
        let mut this = Self::parse_xml_inner(node)?;

        for tool in node
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "Tool")
        {
            match tool.attribute("Name") {
                Some("VCCLCompilerTool") => {
                    assert!(this.tool.is_none(), "{:#?}", this.tool);
                    this.tool = Some(CompilerTool::parse_xml(tool)?);
                }
                Some(_) => (),
                None => anyhow::bail!("Tool without a name"),
            }
        }

        Ok(this)
    }
}

//
// macro_rules! used by proc-macro
//

#[rustfmt::skip]
macro_rules! optparse {
    ($f:ident: $($t:tt)+) => {
        let $f = match $f {
            None => None,
            Some(s) => {
                parse!(s: $($t)+);
                Some(s)
            }
        };
    };
}
pub(crate) use optparse;

#[rustfmt::skip]
macro_rules! parse {
    ($f:ident: bool)        => { let $f = parse_bool($f)?; };
    ($f:ident: String)      => { let $f = $f.to_string(); };
    ($f:ident: Vec<String>) => { let $f = parse_list($f); };
    ($f:ident: $t:ty)       => { let $f = $f.parse::<$t>()?; };
}
pub(crate) use parse;

macro_rules! parse_attrs {
    ($node:expr, $ctx:literal, {
        $(          $attr_name    :literal => $field:ident,    )*
        $(optional: $attr_name_opt:literal => $field_opt:ident,)*
        $(ignore:   $attr_name_igr:literal,                    )*
    }) => {
        $(let mut $field: Option<&str> = None;)*
        $(let mut $field_opt: Option<&str> = None;)*

        for attr in $node.attributes() {
            match attr.name() {
                $($attr_name_igr)|*|"" => {}
                $($attr_name     => _ = $field.replace(attr.value()),)*
                $($attr_name_opt => _ = $field_opt.replace(attr.value()),)*
                attr_name => {
                    anyhow::bail!("Unexpected {} attribute: '{attr_name}' with value: '{}'", $ctx, attr.value())
                }
            }
        }

        $(let $field = $field.context(concat!($ctx, " missing '", $attr_name, "'"))?;)*
    };
}
pub(crate) use parse_attrs;

//
// Helpers used by macro_rules!
//

fn parse_bool(s: &str) -> anyhow::Result<bool> {
    match s {
        "1" | "TRUE" | "true" => Ok(true),
        "0" | "FALSE" | "false" => Ok(false),
        _ => anyhow::bail!("Unexpected boolean value: '{s}'"),
    }
}

fn parse_list(s: &str) -> Vec<String> {
    // Note that we do not remove empty strings from here!
    //
    // This is important, since even if the resulting arguments will be the same,
    // msbuild will still separate them into two compiler invocations if there are empty strings.
    //
    // For example:
    // ```
    // <File
    // 	RelativePath="OPC_BaseModel.cpp"
    // 	>
    // 	<FileConfiguration
    // 		Name="Release|Win32"
    // 		>
    // 		<Tool
    // 			Name="VCCLCompilerTool"
    // 			PreprocessorDefinitions=""
    // 		/>
    // 	</FileConfiguration>
    // 	...
    // ```
    // This file will be compiled separately by the compiler.
    s.split([';', ',']).map(str::to_string).collect()
}

//
//
//

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

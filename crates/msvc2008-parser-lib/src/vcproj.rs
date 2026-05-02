use std::{collections::HashMap, ffi::OsStr, path::Path};

use anyhow::Context;
use msvc2008_parser_proc::{ParseXml, flag_enum};

#[derive(Debug, ParseXml)]
pub struct VCProject {
    pub name: String,
    pub project_type: String,
    pub version: String,
    #[rename("ProjectGUID")]
    pub guid: uuid::Uuid,
    pub root_namespace: String,
    pub keyword: Option<String>, // TODO: freeimage\LibOpenJPEG\LibOpenJPEG.vcproj
    pub target_framework_version: String,

    #[skip]
    pub platforms: Vec<Platform>,
    #[skip]
    pub configurations: Vec<Configuration>,
    #[skip]
    pub files: Files,
}

// TODO: DisableSpecificWarnings can be on files as well:
//
// [zlibN][Release|Win32]: /Ob2 /Oi /Ot /Oy /GT /GL /FD /MD /GS- /arch:SSE /fp:fast /W3 /c /Zi /TP /MP       | my
// [zlibN][Release|Win32]: /Ob2 /Oi /Ot /Oy /GT /GL /FD /MD /GS- /arch:SSE /fp:fast /W3 /c /Zi /TC /wd4996  /MP
// [zlib][Release|Win32]: /O2 /Ob2 /Oi /Ot /Oy /GT /GL /FD /MT /GS- /arch:SSE /fp:fast /W3 /c /Zi /TP /MP    | my
// [zlib][Release|Win32]: /O2 /Ob2 /Oi /Ot /Oy /GT /GL /FD /MT /GS- /arch:SSE /fp:fast /W3 /c /Zi /TC /wd4996  /MP
// [LibTIFF][Release|Win32]: /O2 /Ob2 /Oi /Ot /Oy /GT /GL /GF /FD /MT /GS- /arch:SSE /fp:fast /W3 /c /Zi /MP | my
// [LibTIFF][Release|Win32]: /O2 /Ob2 /Oi /Ot /Oy /GT /GL /GF /FD /MT /GS- /arch:SSE /fp:fast /W3 /c /Zi /wd4996  /MP

#[derive(Debug, ParseXml)]
pub struct Configuration {
    pub name: String,
    // Requires interpolation:
    // OutputDirectory="$(SolutionDir)../binaries/$(PlatformName)/intermediates/$(ConfigurationName)/$(ProjectName)"
    pub output_directory: Option<String>, // TODO: Can be missing in game in random config
    // Requires interpolation:
    // IntermediateDirectory="$(SolutionDir)../binaries/$(PlatformName)/intermediates/$(ConfigurationName)/$(ProjectName)"
    pub intermediate_directory: Option<String>, // TODO: Same as above

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
    pub compiler_tool: Option<CompilerTool>, // TODO: Can be missing for Xbox
    #[skip]
    pub lib_tool: Option<LibTool>, // Should be either lib or linker
    #[skip]
    pub linker_tool: Option<LinkerTool>,
}

#[derive(Debug, ParseXml)]
pub struct Platform {
    pub name: String,
}

#[derive(Debug, ParseXml, Eq, PartialEq, Hash, Clone, Default)]
#[parse_xml(
    tag = "VCCLCompilerTool",
    ignore = "Name",
    ignore = "ExecutionBucket" // u8 -- VS related flag for parallelism. `ogg` and `vorbis` set it to '7'.
    ignore = "UseUnicodeResponseFiles", // bool -- VS related flag
)]
pub struct CompilerTool {
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
    pub generate_preprocessed_file: Option<GeneratePreprocessedFile>,
    pub show_includes: Option<ShowIncludes>,
    pub struct_member_alignment: Option<StructMemberAlignment>,
    pub suppress_startup_banner: Option<SuppressStartupBanner>,
    pub detect_64_bit_portability_problems: Option<Detect64BitPortabilityProblems>,

    pub object_file: Option<String>,
    pub program_data_base_file_name: Option<String>,
    pub assembler_listing_location: Option<String>,
    pub precompiled_header_through: Option<String>,
    pub precompiled_header_file: Option<String>,
    // NOTE: can be ';' or ',' separated depending on project
    pub disable_specific_warnings: Option<Vec<String>>,
    // Requires interpolation: $(SolutionDir)/stlport;
    pub additional_include_directories: Option<Vec<String>>,
    // PreprocessorDefinitions="WIN32;NDEBUG;VOSTOK_STATIC_LIBRARIES;MASTER_GOLD;"
    pub preprocessor_definitions: Option<Vec<String>>,

    #[rename("XMLDocumentationFileName")]
    pub xml_documentation_file_name: Option<String>,
}

#[derive(Debug, ParseXml)]
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
    pub output_file: Option<String>, // TODO: nvidia\nvt\project\squish.vcproj
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
        -1 => "", // Means not applicable. Set specifically on Xbox (TODO: Requires verification)
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

type ResultParse = (
    HashMap<CompilerTool, Vec<String>>,
    HashMap<CompilerTool, Vec<String>>,
);
impl CompilerTool {
    pub fn to_flags(
        &self,
        cfg: &Configuration,
        vcproject: &VCProject,
        configuration_platform: &str,
    ) -> (ResultParse, Vec<String>) {
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
            object_file: _,
            program_data_base_file_name: _,
            assembler_listing_location: _,
            precompiled_header_through,
            precompiled_header_file,
            disable_specific_warnings,
            additional_include_directories: _,
            preprocessor_definitions,
            xml_documentation_file_name: _,
        } = self;

        if let Some(phf) = precompiled_header_file {
            println!("phf: '{phf}'")
        }

        //
        //

        let whole_program_optimization =
            match (cfg.whole_program_optimization, whole_program_optimization) {
                (Some(true), None) => Some(WholeProgramOptimization::_1),
                _ => *whole_program_optimization,
            };

        let generate_program_database = match debug_information_format {
            Some(DebugInformationFormat::_0) | None => None,
            _ => Some(GenerateProgramDatabase::_1),
        };

        let compile_only = Some(CompileOnly::_1);

        // TODO: `compile_as` is a bit more complex.
        // If there are C++ and C files, it will emit two calls (verify!) to the compiler
        // This is important for dependencies
        let compile_as = match compile_as {
            None => Some(CompileAs::_2),
            _ => *compile_as,
        };

        // TODO: I'd rather we verified that while parsing.
        let use_precompiled_header = match (use_precompiled_header, precompiled_header_through) {
            (Some(use_precompiled_header), Some(precompiled_header_through))
                if !matches!(*use_precompiled_header, UsePrecompiledHeader::_0) =>
            {
                assert_ne!(use_precompiled_header, &UsePrecompiledHeader::_1);

                let mut flag = use_precompiled_header.as_str().to_string();
                flag.push('"');
                flag.push_str(precompiled_header_through);
                flag.push('"');
                Some(flag)
            }
            _ => None,
        };
        println!("use_precompiled_header: '{use_precompiled_header:?}'");

        // TODO: default option
        let exception_handling = Some(
            exception_handling
                .as_ref()
                .unwrap_or(&ExceptionHandling::_1),
        );

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

        let mut preprocessor_definitions = preprocessor_definitions
            .iter()
            .flatten()
            .map(String::as_str)
            .collect::<Vec<_>>();

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
                        // TODO
                    }
                    _ => unreachable!("TODO"),
                }
            }
            match vc_version {
                0 => (),
                60 => preprocessor_definitions.push("_VC80_UPGRADE=0x0600"),
                70 => preprocessor_definitions.push("_VC80_UPGRADE=0x0700"),
                71 => preprocessor_definitions.push("_VC80_UPGRADE=0x0710"),
                _ => unreachable!(),
            }
        }

        match cfg.configuration_type {
            ConfigurationType::_2 => preprocessor_definitions.push("_WINDLL"),
            _ => (),
        }

        if let Some(character_set) = cfg.character_set {
            match character_set {
                CharacterSet::_1 => {
                    preprocessor_definitions.push("_UNICODE");
                    preprocessor_definitions.push("UNICODE");
                }
                CharacterSet::_2 => preprocessor_definitions.push("_MBCS"),
                _ => (),
            }
        }

        if !preprocessor_definitions.is_empty() {
            result.push(' ');
        }

        for preprocessor_definition in preprocessor_definitions {
            result.push_str("/D ");
            result.push('"');
            result.push_str(&preprocessor_definition);
            result.push('"');
            result.push(' ');
        }

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
            use_precompiled_header,
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

        for disable_specific_warning in disable_specific_warnings.iter().flatten() {
            result.push(' ');
            result.push_str("/wd");
            result.push_str(&disable_specific_warning);
        }

        // @TODO: Requires correctly handling overrides.
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

        let file_types = Self::_parse_files(&vcproject.files, configuration_platform);

        (file_types, vec![result])
    }

    // @TODO: We only care right now about the compiler
    fn _parse_files(files: &Files, configuration_platform: &str) -> ResultParse {
        let mut result = (HashMap::new(), HashMap::new());

        for file in &files.files {
            _ = Self::_parse_file(&mut result, file, configuration_platform);
        }

        for filter in &files.filters {
            _ = Self::_parse_filter(&mut result, filter, configuration_platform);
        }

        result
    }

    fn _parse_filter(result: &mut ResultParse, filter: &Filter, configuration_platform: &str) {
        for file in &filter.files {
            _ = Self::_parse_file(result, file, configuration_platform);
        }

        for filter in &filter.filters {
            _ = Self::_parse_filter(result, filter, configuration_platform);
        }
    }

    fn _parse_file(result: &mut ResultParse, file: &File, configuration_platform: &str) {
        for file in &file.files {
            _ = Self::_parse_file(result, file, configuration_platform);
        }

        if file.file_configurations.is_empty() {
            let map = match Path::new(&file.relative_path)
                .extension()
                .map(OsStr::as_encoded_bytes)
            {
                Some(b"c") => &mut result.0,
                Some(b"cpp") => &mut result.1,
                Some(b"h") => return,
                _ => {
                    eprintln!("Couldn't parse extension: {}", file.relative_path);
                    return;
                }
            };

            map.entry(CompilerTool::default())
                .or_default()
                .push(file.relative_path.clone());
            return;
        }
        for config in &file.file_configurations {
            if config.name == configuration_platform {
                let Some(cl_tool) = &config.tool else {
                    continue;
                };

                let map = match Path::new(&file.relative_path)
                    .extension()
                    .map(OsStr::as_encoded_bytes)
                {
                    Some(b"c") => &mut result.0,
                    Some(b"cpp") => &mut result.1,
                    Some(b"h") => continue,
                    _ => unreachable!("Couldn't parse extension: {}", file.relative_path),
                };

                map.entry(cl_tool.clone())
                    .or_default()
                    .push(file.relative_path.clone());
            }
        }
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
                // TODO
                Some("VCCLCompilerTool") => {
                    this.compiler_tool = Some(CompilerTool::parse_xml(child)?)
                }
                Some("VCLibrarianTool") => this.lib_tool = Some(LibTool::parse_xml(child)?),
                Some("VCLinkerTool") => this.linker_tool = Some(LinkerTool::parse_xml(child)?),
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
    ($f:ident: bool)   => { let $f = $f.map(parse_bool).transpose()?; };
    ($f:ident: String) => { let $f = $f.map(str::to_string); };
    ($f:ident: Vec<_>) => { let $f = $f.map(parse_list); };
    ($f:ident: $t:ty)  => { let $f = $f.map(|s| s.parse::<$t>()).transpose()?; };
}
pub(crate) use optparse;

#[rustfmt::skip]
macro_rules! parse {
    ($f:ident: bool)   => { let $f = parse_bool($f)?; };
    ($f:ident: String) => { let $f = $f.to_string(); };
    ($f:ident: Vec<_>) => { let $f = parse_list($f); };
    ($f:ident: $t:ty)  => { let $f = $f.parse::<$t>()?; };
}
pub(crate) use parse;

macro_rules! parse_attrs {
    ($node:expr, $ctx:literal, {
        $($attr_name:literal => $field:ident,)*
        $(optional: $attr_name_opt:literal => $field_opt:ident,)*
        $(ignore: $ignore:literal,)*
    }) => {
        $(let mut $field: Option<&str> = None;)*
        $(let mut $field_opt: Option<&str> = None;)*

        for attr in $node.attributes() {
            match attr.name() {
                $($ignore)|*|"" => {}
                $($attr_name => _ = $field.replace(attr.value()),)*
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
    s.split([';', ','])
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

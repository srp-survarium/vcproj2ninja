use vs2008_parser_proc::{flag_enum, ParseXml};

use super::flags::{append_flags, Flags};
use super::macros::*;
use super::utils;
use super::ConfigurationType;
use super::{Configuration, LibTool, MsBuildEnvironment, VCProject};

use std::path::Path;

#[derive(Debug, ParseXml, Default)]
#[parse_xml(tag = "VCLinkerTool", ignore = "Name")]
pub struct LinkerTool {
    pub additional_options: Option<String>,
    pub additional_dependencies: Option<String>,
    pub output_file: Option<String>,
    pub link_incremental: Option<LinkIncremental>,
    pub additional_library_directories: Option<Vec<String>>,
    pub ignore_default_library_names: Option<Vec<String>>,
    pub module_definition_file: Option<String>,
    pub generate_debug_information: Option<GenerateDebugInformation>,
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
    pub support_unload_of_delay_loaded_dll: Option<SupportUnloadOfDelayLoadedDLL>,
    pub version: Option<String>,
}

//
// Linker flags
//

flag_enum! {
    enum GenerateDebugInformation {
        false => "",
        true => "/DEBUG",
    }
}
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
        1 => "/FIXED:No",
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

flag_enum! {
    enum SupportUnloadOfDelayLoadedDLL {
        false => "",
        true => "/DELAY:UNLOAD",
    }
}

macro_rules! unimplemented_flag {
    ($flag:expr) => {
        if $flag.is_some() {
            unimplemented!()
        }
    };
    (empty: $flag:expr) => {
        if let Some(flag) = $flag
            && !flag.is_empty()
        {
            unimplemented!()
        }
    };
    (false: $flag:expr) => {
        if let Some(flag) = $flag
            && *flag
        {
            unimplemented!()
        }
    };
}

impl LinkerTool {
    pub fn to_flags(
        &self,
        vcproj_rpath: &str,
        cfg: &Configuration,
        vcproject: &VCProject,
        env: MsBuildEnvironment,
    ) -> Flags {
        let Self {
            additional_options,
            additional_dependencies,
            output_file,
            link_incremental,
            additional_library_directories,
            ignore_default_library_names,
            module_definition_file,
            generate_debug_information,
            program_database_file,
            generate_map_file,
            map_file_name,
            map_exports,
            sub_system,
            large_address_aware,
            optimize_references,
            enable_comdat_folding,
            randomized_base_address,
            data_execution_prevention,
            import_library,
            target_machine,
            assembly_debug,
            assembly_link_resource,
            base_address,
            clr_thread_attribute,
            delay_load_dlls,
            embed_managed_resource_file,
            entry_point_symbol,
            fixed_base_address,
            generate_manifest,
            ignore_import_library,
            optimize_for_windows98,
            support_unload_of_delay_loaded_dll,
            version: _,
        } = self;

        unimplemented_flag!(clr_thread_attribute);
        unimplemented_flag!(assembly_debug);
        unimplemented_flag!(assembly_link_resource);
        unimplemented_flag!(embed_managed_resource_file);
        unimplemented_flag!(ignore_import_library);
        unimplemented_flag!(optimize_for_windows98);

        unimplemented_flag!(empty: map_file_name);
        unimplemented_flag!(empty: entry_point_symbol);

        unimplemented_flag!(false: map_exports);
        unimplemented_flag!(false: generate_map_file);

        let mut rsp_flags = vec![];

        let output_file = output_file
            .as_deref()
            .unwrap_or_else(|| match cfg.configuration_type {
                ConfigurationType::_1 => "$(OutDir)\\$(ProjectName).exe",
                ConfigurationType::_2 => "$(OutDir)\\$(ProjectName).dll",
                _ => unimplemented!(),
            });
        let output_file = env.expand(output_file);
        let output_file = utils::clean(&output_file);
        rsp_flags.push(format!("/OUT:\"{output_file}\""));

        append_flags!(rsp_flags, [link_incremental]);

        for lib_path in additional_library_directories.iter().flatten() {
            let lib_path = env.expand(lib_path);
            let lib_path = utils::clean(&lib_path);

            if !lib_path.is_empty() {
                rsp_flags.push(format!("/LIBPATH:\"{lib_path}\""));
            }
        }

        if matches!(cfg.configuration_type, ConfigurationType::_2) {
            rsp_flags.push("/DLL".to_string());
        }

        let generate_manifest = generate_manifest.unwrap_or(true);
        if generate_manifest {
            rsp_flags.push("/MANIFEST".to_string());

            let target_file_name = Path::new(output_file)
                .file_name()
                .expect("OutputFile always points to something")
                .to_str()
                .expect("OutputFile is built from String");

            let manifest_file = env.expand(&format!(
                "$(IntDir)\\{target_file_name}.intermediate.manifest"
            ));
            // /MANIFESTFILE:"E:\Projects\vostok\sources\../binaries/Win32/intermediates/Release/nvtt\vostok_nvtt.dll.intermediate.manifest"
            let manifest_file = utils::clean(&manifest_file);
            rsp_flags.push(format!("/MANIFESTFILE:\"{manifest_file}\""));

            // @TODO: VS2008 SP1 default UAC fragment. Extend with struct fields when added.
            // /MANIFESTUAC:"level='asInvoker' uiAccess='false'"
            rsp_flags.push("/MANIFESTUAC:\"level='asInvoker' uiAccess='false'\"".to_string());
        }

        for lib_name in ignore_default_library_names.iter().flatten() {
            let lib_name = utils::clean(lib_name);

            if !lib_name.is_empty() {
                rsp_flags.push(format!("/NODEFAULTLIB:\"{lib_name}\""));
            }
        }

        if let Some(module_definition_file) = module_definition_file {
            let module_definition_file = utils::clean(module_definition_file);

            rsp_flags.push(format!("/DEF:\"{module_definition_file}\""));
        }

        // @TODO: Does `generate_debug_information` affect PDB?
        append_flags!(rsp_flags, [generate_debug_information]);
        {
            let program_database_file = program_database_file.clone().unwrap_or_else(|| {
                Path::new(output_file)
                    .with_extension("pdb")
                    .normalize_lexically()
                    .expect("Requires normalization to match original flags")
                    .into_os_string()
                    .into_string()
                    .expect("Original 'output_file' is String")
            });
            let program_database_file = env.expand(&program_database_file);
            let program_database_file = utils::clean(&program_database_file);

            rsp_flags.push(format!("/PDB:\"{program_database_file}\""));
        }

        append_flags!(
            rsp_flags,
            [
                sub_system,
                large_address_aware,
                optimize_references,
                enable_comdat_folding,
            ]
        );

        if matches!(cfg.whole_program_optimization, Some(true)) {
            rsp_flags.push("/LTCG".to_string());
        }

        if let Some(base_address) = base_address {
            let base_address = utils::clean(base_address);

            rsp_flags.push(format!("/BASE:\"{base_address}\""));
        }

        append_flags!(
            rsp_flags,
            [
                randomized_base_address,
                data_execution_prevention,
                fixed_base_address,
                support_unload_of_delay_loaded_dll,
            ]
        );

        {
            let Some(import_library) = import_library else {
                unimplemented!("TODO: Figure out what the default should be")
            };
            let import_library = env.expand(import_library);
            let import_library = utils::clean(&import_library);
            rsp_flags.push(format!("/IMPLIB:\"{import_library}\""));
        }

        append_flags!(rsp_flags, [target_machine]);

        for dll in delay_load_dlls.iter().flatten() {
            let dll = utils::clean(dll);

            if !dll.is_empty() {
                rsp_flags.push(format!("/delayload:{dll}"));
            }
        }

        append_flags!(rsp_flags, [additional_options, additional_dependencies]);

        let files = LibTool::file_flags(&vcproject.files, &cfg.name, vcproj_rpath, env);

        Flags {
            output_file: output_file.to_string(),
            flags: "@$(RspFile) /NOLOGO /ERRORREPORT:PROMPT".to_string(),
            rsp_flags: rsp_flags.join(" "),
            files,
        }
    }

    pub fn to_flags_for_lib(
        vcproj_rpath: &str,
        cfg: &Configuration,
        vcproject: &VCProject,
        env: MsBuildEnvironment,
    ) -> Flags {
        let output_file = match cfg.configuration_type {
            ConfigurationType::_1 => "$(OutDir)\\$(ProjectName).exe",
            ConfigurationType::_2 => "$(OutDir)\\$(ProjectName).dll",
            _ => unimplemented!(),
        };
        let output_file = env.expand(output_file);
        let output_file = utils::clean(&output_file);

        let files = LibTool::file_flags(&vcproject.files, &cfg.name, vcproj_rpath, env);

        let mut rsp_flags = vec![];
        rsp_flags.push(format!("/OUT:\"{output_file}\""));

        Flags {
            output_file: output_file.to_string(),
            flags: "/LIB @$(RspFile) /NOLOGO".to_string(),
            rsp_flags: rsp_flags.join(" "),
            files,
        }
    }
}

mod env;
mod flags;
mod macros;
mod tool_cl;
mod tool_lib;
mod tool_linker;
mod utils;

use vs2008_parser_proc::{flag_enum, ParseXml};

use macros::{optparse, parse, parse_attrs};

pub use env::MsBuildEnvironment;
pub use tool_cl::CompilerTool;
pub use tool_lib::LibTool;
pub use tool_linker::LinkerTool;

use anyhow::Context;

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
    // OutputDirectory       = "$(SolutionDir)../binaries/$(PlatformName)/intermediates/$(ConfigurationName)/$(ProjectName)"
    pub output_directory: Option<String>,

    // Requires interpolation:
    // IntermediateDirectory = "$(SolutionDir)../binaries/$(PlatformName)/intermediates/$(ConfigurationName)/$(ProjectName)"
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

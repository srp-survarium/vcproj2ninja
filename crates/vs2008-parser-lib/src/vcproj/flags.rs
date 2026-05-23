pub struct Flags {
    /// CL:       expanded IntDir (trailing backslash), e.g. `E:\...\Release\mylib\`
    /// LIB/LINK: expanded output file path, e.g. `E:\...\libraries\mylib.lib`
    pub output_file: String,

    /// Full command-line flags, including /Fo for CL and /OUT: for LIB/LINK.
    pub flags: String,

    /// Input files.
    /// CL:       source .cpp/.c paths.
    /// LIB/LINK: .obj paths from LibTool::file_flags.
    pub files: Vec<String>,
}

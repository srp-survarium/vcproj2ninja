pub struct Flags {
    /// CL:       expanded IntDir (trailing backslash), e.g. `E:\...\Release\mylib\`
    /// LIB/LINK: expanded output file path, e.g. `E:\...\libraries\mylib.lib`
    pub output_file: String,

    /// Command-line flags.
    /// CL:       without /Fo (emitted by the ninja rspfile rule via $obj_dir).
    /// LIB/LINK: without /OUT: (emitted by the ninja rspfile rule via $out).
    pub flags: String,

    /// Input files.
    /// CL:       source .cpp/.c paths.
    /// LIB/LINK: .obj paths from LibTool::file_flags.
    pub files: Vec<String>,
}

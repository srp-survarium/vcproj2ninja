macro_rules! append_flags {
    ($vec:ident, [$($opt:expr),* $(,)?]) => {{
        $(
            if let Some(flag) = $opt.as_ref() {
                let flag_str = flag.as_str();
                if !flag_str.is_empty() {
                    $vec.push(String::from(flag_str));
                }
            }
        )*
    }};
}
pub(crate) use append_flags;

pub struct Flags {
    /// CL:       expanded IntDir (trailing backslash), e.g. `E:\...\Release\mylib\`
    /// LIB/LINK: expanded output file path, e.g. `E:\...\libraries\mylib.lib`
    pub output_file: String,

    /// Flags passed to the compiler.
    /// `$(RspFile)` requires interpolation.
    pub flags: String,

    /// Flags stored in a temporary .rsp file.
    pub rsp_flags: String,

    /// Input files.
    /// CL:       source .cpp/.c paths.
    /// LIB/LINK: .obj paths from LibTool::file_flags.
    pub files: Vec<String>,
}

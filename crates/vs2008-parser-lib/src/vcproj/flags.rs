use std::path::PathBuf;

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
impl Flags {
    /// Build the rsp file content: rsp_flags on first line(s), then one filename per line.
    pub fn rsp_file_content(&self) -> String {
        let mut content = self.rsp_flags.clone();
        for file in &self.files {
            content.push('\n');
            content.push('"');
            content.push_str(file);
            content.push('"');
        }

        content
    }
}

pub struct FlagsTree {
    pub flags: Flags,
    /// Children that depend on an artifact this node produces (e.g. a .pch file).
    /// The `PathBuf` is the artifact path: listed as implicit output (`|`) on this
    /// node and as order-only dep (`||`) on each child.
    pub dependants: Vec<(FlagsTree, PathBuf)>,
}

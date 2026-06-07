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

    /// For DLL link steps: the import library path (`/IMPLIB:`).
    /// Downstream projects link against this, not the `.dll` itself.
    pub import_library: Option<String>,

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
        let Self {
            rsp_flags,
            files,
            output_file: _,
            import_library: _,
            flags: _,
        } = self;
        let mut content = rsp_flags.clone();
        for file in files {
            content.push('\n');
            content.push('"');
            content.push_str(file);
            content.push('"');
        }

        content
    }
}

pub struct ClGroup {
    pub flags: Flags,
    /// PCH file this step creates (`/Yc`); listed as implicit output.
    pub pch_output: Option<PathBuf>,
    /// PCH file this step consumes (`/Yu`); listed as implicit input.
    pub pch_input: Option<PathBuf>,
    /// Expanded `/Fd` path shared by all cl steps in the project.
    /// Used as the ninja pool key to prevent parallel PDB writes.
    /// `None` when the project does not write a PDB (no `/Fd` flag).
    pub fd_path: Option<String>,

    /// Expanded `/I` include search directories (same path form as the emitted
    /// flags). Consumed by the generator's preprocessor to resolve `#include`s.
    pub include_dirs: Vec<String>,
    /// Preprocessor `/D` definitions (raw `NAME` or `NAME=VALUE`). Used to
    /// evaluate `#if`/`#ifdef` while scanning for header dependencies.
    pub defines: Vec<String>,
    /// Transitively-included header files, in ninja-space paths, attached as
    /// implicit inputs so header edits force a recompile. Empty until the
    /// generator fills it in (it owns the filesystem/path-form knowledge).
    pub header_deps: Vec<String>,
}

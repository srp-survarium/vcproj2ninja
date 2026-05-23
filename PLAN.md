# Plan: Generate build.ninja from vcproj

## Goal

Produce one `<project>.ninja` file per vcproj dependency, plus a top-level
`build.ninja` that `subninja`-includes them all. Long command lines are written
to `.rsp` response files (as MSBuild does) to stay within Windows's ~32767-char
CLI limit.

IntDir/OutDir will be exposed as ninja variables (overridable with `ninja -D`)
in a follow-up once basic builds work.

**Each step below is one commit.**
Split a step into two commits if the diff grows large.

---

## Step 1 — `Flags` struct (library)

**File:** `crates/vs2008-parser-lib/src/vcproj/flags.rs` (new)
**Re-export from:** `mod.rs`

```rust
pub struct Flags {
    /// CL:       expanded IntDir (trailing backslash), e.g. `E:\...\Release\mylib\`
    /// LIB/LINK: expanded output file path, e.g. `E:\...\libraries\mylib.lib`
    pub output_file: String,

    /// Command-line flags.
    /// CL:       without /Fo (handled by the rsp rule via $obj_dir).
    /// LIB/LINK: without /OUT: (handled by the rsp rule via $out).
    pub flags: String,

    /// Input files.
    /// CL:   source .cpp/.c paths.
    /// LIB/LINK: .obj paths from LibTool::file_flags.
    pub files: Vec<String>,
}
```

---

## Step 2 — Refactor `to_flags` return types

### `CompilerTool::to_flags` → `Vec<Flags>`

- Keep existing grouping (HashMap<CompilerTool, Vec<String>>).
- Per group: `output_file` = `env.expand(env.int_dir)`, `files` = source paths.
- Strip `/Fo"..."` from `to_flags_impl` (output goes into the ninja rspfile).
- Ninja writer derives `.obj` paths as `output_file + stem(source) + ".obj"`.

### `LibTool::to_flags` → `Flags`

- `output_file` = expanded .lib path.
- `flags` = current flags **without** `/OUT:"..."`.
- `files` = `Self::file_flags(...)` result (move from appended string to Vec).

### `LinkerTool::to_flags` → `Flags` (exe/dll)

- `output_file` = expanded .exe/.dll path.
- `flags` = current flags **without** `/OUT:"..."`.
- `files` = `LibTool::file_flags(...)` result (move from `flags.extend` to Vec).

### `LinkerTool::to_flags_for_lib` → `Flags` (lib via link.exe)

Some ConfigurationType::_4 projects use `VCLinkerTool` (link.exe `/LIB`) instead
of `VCLibrarianTool`. This function handles that case:
- `output_file` = expanded output path.
- `flags` = flags **without** `/OUT:"..."`.
- `files` = `LibTool::file_flags(...)` result.

---

## Step 3 — `NinjaFile` struct + writer (vcproj2ninja crate)

**File:** `crates/vcproj2ninja/src/ninja.rs` (new)
**Declare:** `mod ninja;` in `main.rs`

```rust
use vs2008_parser_lib::vcproj::Flags;

pub enum FinalStep {
    Lib(Flags),        // VCLibrarianTool  (lib.exe)
    LinkForLib(Flags), // VCLinkerTool /LIB (link.exe used as lib)
    Link(Flags),       // VCLinkerTool      (link.exe for exe/dll)
}

pub struct NinjaFile {
    pub cl: Vec<Flags>,
    pub final_step: FinalStep,
}

impl NinjaFile {
    pub fn write(&self, out: &mut impl std::fmt::Write) -> std::fmt::Result;
}
```

### Rules (written once per file, using rsp to handle long lines)

```ninja
rule cl
  command = cl @$rspfile
  rspfile = $out.rsp
  rspfile_content = $flags /Fo"$obj_dir" $in

rule lib
  command = lib @$rspfile
  rspfile = $out.rsp
  rspfile_content = /OUT:"$out" $flags $in

rule link
  command = link @$rspfile
  rspfile = $out.rsp
  rspfile_content = /OUT:"$out" $flags $in
```

### Build statements

```ninja
# One per CL flags-group; outputs derived from output_file + stem(source) + ".obj"
build intdir\foo.obj intdir\bar.obj: cl src\foo.cpp src\bar.cpp
  flags = /O2 /W3 ...
  obj_dir = intdir\

# Final step — lib, link-as-lib, or link
build E:\...\mylib.lib: lib intdir\foo.obj intdir\bar.obj
  flags = /NOLOGO /LTCG
```

---

## Step 4 — Wire up in `main.rs` + one file per vcproj

Add `--output-dir` CLI arg. For each dep:

1. Build `NinjaFile { cl, final_step }` from the refactored `to_flags` calls.
2. Write to `<output_dir>/<project_name>.ninja`.

After all deps, write `<output_dir>/build.ninja`:

```ninja
subninja foo.ninja
subninja bar.ninja
...
```

---

## Future: IntDir / OutDir overrides

Expose as top-level ninja variables so the user can override without regenerating:

```ninja
int_dir = E:\...\intermediates\Release\myproject\
out_dir = E:\...\output\Release\

build $int_dir\foo.obj: cl foo.cpp
  obj_dir = $int_dir
```

Override at build time: `ninja -Dint_dir=E:\alt\int`

---

## Verification

```
cargo run -p vcproj2ninja -- \
  --sln-path E:\Projects\vostok\sources\vostok.sln \
  --project-name network \
  --configuration-platform "Release|Win32" \
  --output-dir E:\build\ninja

ninja -f E:\build\ninja\build.ninja -t targets
```

Check:
- One `.ninja` file per vcproj dep.
- Each `.cpp` is an input to a `cl` build statement; `.obj` outputs match `lib`/`link` inputs.
- `ninja -t targets` parses without errors.

# msvc2008-parser — Agent Context

## What this project does

Converts Visual Studio 2008 solution (`.sln`) and project (`.vcproj`) files into
Ninja build files so the Vostok game engine (at `E:\Projects\vostok`) can be
built with Ninja instead of the MSVC IDE.

The generated files land in `E:\Projects\vostok\binaries\ninja\`.

## Crate layout

```
crates/
  vs2008-parser-lib/   # Core parsing + flag generation (library)
    src/
      sln.rs           # .sln parser (nom-based)
      vcproj/
        mod.rs         # VCProject / Configuration structs
        tool_cl.rs     # CompilerTool → ClGroup flags (cl.exe flags)
        tool_lib.rs    # LibTool flags
        tool_linker.rs # LinkerTool flags
        env.rs         # MSBuild macro expansion ($(IntDir), $(SolutionDir), …)
        flags.rs       # Flags / ClGroup structs
  vs2008-parser-proc/  # Derive macros: ParseXml, flag_enum
  vcproj2ninja/        # Binary: reads .sln, walks deps, writes .ninja + .rsp files
    src/
      main.rs          # CLI entry point, project traversal, build.ninja
      ninja.rs         # NinjaFile → ninja text + per-project .rsp files
```

## How to run

```
cargo run --release --bin vcproj2ninja -- \
  --sln-path "E:\Projects\vostok\sources\survarium - PC - DirectX 11.sln" \
  --project-name "survarium - PC - DirectX 11" \
  --configuration-platform "Release (static)|Win32" \
  --output-dir "E:\Projects\vostok\binaries\ninja"
```

Then build with:
```
ninja -C E:\Projects\vostok\binaries\ninja -j8
```

## Key design decisions

### ClGroup grouping (`tool_cl.rs::to_flags`)
Files within a project are grouped by their merged `CompilerTool` settings
(all files with identical compiler flags share one `cl @rsp` invocation).
The main grouping axes:
- PCH creation (`/Yc`) — must be its own group, scheduled first
- PCH consumption (`/Yu`) — depends on the PCH implicit output
- Independent files (no PCH, or different flags) — separate groups

### PDB pool serialisation (`ninja.rs`)
MSVC 2008 writes a single PDB per project (via `/Fd`).  Multiple concurrent
`cl.exe` invocations from Ninja would all try to write the same file.

**Fix**: each subninja file declares a Ninja pool keyed on the `/Fd` path
(non-alphanumeric chars → `_`, depth 1).  Every `cl` build step is assigned to
that pool, so at most one compiler runs per shared PDB at a time.

Pool name derivation: `fd_pool_name(fd_path)` in `ninja.rs`.

### /MP is stripped (`tool_cl.rs::to_flags_impl`)
The `AdditionalOptions` in vcproj files often contain `/MP` (multi-process
compilation).  This flag is **removed** from generated RSP files because:
- Ninja already parallelises at the build-rule level.
- `/MP` makes `cl.exe` spawn child processes that each open the same `/Fd` PDB.
- MSVC 2008 has no `mspdbsrv.exe` PDB server; the children race on the file,
  corrupting or locking it → next rule fails with `C1033: cannot open program
  database`.
- With the depth-1 pool serialising rules, `/MP` buys nothing and breaks things.

### RSP files
Compiler flags too long for a command line go in `<stem>_cl_<n>.rsp`.
The ninja `cl` rule references them via `@<path>`.  `$(RspFile)` in the flags
string is replaced by the actual path at write time.

### Dependency resolution
`main.rs` walks `ProjectSection(ProjectDependencies)` transitively to build
the dep graph.  The final lib/link step lists dep outputs as implicit inputs so
Ninja rebuilds downstream when an upstream lib changes.

## Known issues / TODOs

- `//TODO` in `tool_cl.rs`: `/Fd` flag comment notes that multiple invocations
  still possible in theory (left for reference).
- `$(InputName)` expansion is only correct when a group contains exactly one
  file; multi-file groups use `"<poison>"` as a sentinel.
- `vsprops` files (property sheets) are only partially handled — current
  hard-coded list covers the Vostok project; a general parser would need to
  follow `InheritedPropertySheets` paths and expand user macros.
- Linker dep injection reads `#pragma comment(lib, …)` via sln
  `ProjectDependencies` (section-based); a more precise approach would scan
  source headers for those directives.

## Generated file structure

```
E:\Projects\vostok\binaries\ninja\
  build.ninja          # top-level: subninja <project>.ninja for every dep
  <project>.ninja      # per-project: pool decl, cl/lib/link rules, build stmts
  rsp\
    <project>_cl_<n>.rsp   # compiler flags + file list
    <project>_lib.rsp      # librarian flags
    <project>_link.rsp     # linker flags
```

## Debugging tips

- `--verbose` prints all flag strings to stderr before writing files.
- To inspect a PDB conflict: check which RSP files share the same `/Fd` path.
  They should all be assigned the same pool name in the corresponding `.ninja`.
- To reproduce a build error: `ninja -C E:\Projects\vostok\binaries\ninja -j1`
  (single-threaded) isolates whether it's a concurrency issue.

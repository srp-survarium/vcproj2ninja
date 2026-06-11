//! Path-form conversions between the three coordinate systems this tool
//! straddles: native Linux (`/home/...`), the Wine view of it (`Z:\home\...`),
//! and native Windows (drive-rooted, passed through untouched).

use std::path::{Path, PathBuf};

/// Lift a native Linux path to the drive-rooted form Wine exposes: the Linux
/// root `/` is mounted at drive `Z:`, so `/home/x` -> `Z:\home\x`. Non-absolute
/// inputs are returned unchanged.
fn unix_to_wine(p: &str) -> String {
    if p.starts_with('/') {
        format!("Z:{}", p.replace('/', "\\"))
    } else {
        p.to_string()
    }
}

/// Inverse of [`unix_to_wine`]: `Z:\home\x` -> `/home/x`. Used for real
/// filesystem syscalls (Rust's std cannot open `Z:\` paths under Wine) and for
/// emitting host-native artifacts like compile_commands.json.
fn wine_to_unix(p: &str) -> String {
    p.strip_prefix("Z:")
        .or_else(|| p.strip_prefix("z:"))
        .unwrap_or(p)
        .replace('\\', "/")
}

/// Host form of a path string: lowered from the Wine view under `--wine`,
/// untouched on Windows (drive-rooted paths must stay drive-rooted).
pub fn to_host_str(p: &str, wine: bool) -> String {
    if wine { wine_to_unix(p) } else { p.to_string() }
}

/// [`to_host_str`] for `Path` values.
pub fn to_host(p: &Path, wine: bool) -> PathBuf {
    if wine {
        wine_to_unix(p.to_str().expect("path is valid UTF-8")).into()
    } else {
        p.to_path_buf()
    }
}

/// Map a native filesystem path back into the build-graph path space: `Z:\...`
/// under --wine, native otherwise — mirroring how obj/source paths are emitted.
///
/// These header paths come from the scanner's `Path` operations. When this
/// binary runs as a Windows PE under Wine (the `--wine` case), those operations
/// normalize separators to `\` and drop the leading `/`, so we unify to forward
/// slashes first; `unix_to_wine` then lifts a rooted `/home/...` to the
/// drive-rooted `Z:\home\...` form. Without the unification the path would be
/// emitted drive-less (`\home\...`), inconsistent with every other graph path.
pub fn to_graph(path: &Path, wine: bool) -> String {
    let path = path
        .to_str()
        .expect("header path is valid UTF-8")
        .replace('\\', "/");
    if wine { unix_to_wine(&path) } else { path }
}

/// Convert an include dir / source path as it appears in the emitted flags
/// (possibly `Z:\...` under --wine, possibly relative with backslashes) into a
/// native absolute filesystem path the preprocessor can actually open.
pub fn resolve_host(raw: &str, proj_dir: &Path) -> PathBuf {
    let replaced = raw.trim().replace('\\', "/");
    let stripped = replaced
        .strip_prefix("Z:")
        .or_else(|| replaced.strip_prefix("z:"))
        .unwrap_or(&replaced);
    let path = Path::new(stripped);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        proj_dir.join(path)
    };
    joined.normalize_lexically().unwrap_or(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wine_round_trip_is_identity() {
        // wine_to_unix . unix_to_wine == id for the paths this tool feeds it:
        // absolute Linux paths and backslash-free relative ones.
        for p in [
            "/home/sheep/Projects/surv/vostok",
            "/home/sheep/Projects/surv/vostok_1/sources/vostok v2.0.sln",
            "/nix/store/abc123-vostok-toolchain/msvc/VC/include",
            "/",
            "relative/path.cpp",
            "pch.cpp",
        ] {
            assert_eq!(wine_to_unix(&unix_to_wine(p)), p, "round trip of {p:?}");
        }
    }

    #[test]
    fn wine_to_unix_lowers_both_drive_cases() {
        assert_eq!(wine_to_unix("Z:\\home\\x"), "/home/x");
        assert_eq!(wine_to_unix("z:\\home\\x"), "/home/x");
    }

    #[test]
    fn to_host_str_is_untouched_on_windows() {
        assert_eq!(
            to_host_str("C:\\Projects\\vostok", false),
            "C:\\Projects\\vostok"
        );
        assert_eq!(to_host_str("Z:\\home\\x", true), "/home/x");
    }
}

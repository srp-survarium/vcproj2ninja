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

/// Convert an include dir / source path as it appears in the emitted flags
/// (possibly `Z:\...` under --wine, possibly relative with backslashes) into a
/// native absolute filesystem path the preprocessor can actually open.
pub fn to_host_normalized(raw: &str, proj_dir: &Path) -> PathBuf {
    let host = wine_to_unix(raw.trim());
    let path = Path::new(&host);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        proj_dir.join(path)
    };
    joined.normalize_lexically().unwrap_or(joined)
}

/// Path form written into the emitted ninja graph: `Z:\...` under --wine,
/// native otherwise.
///
/// Separators are unified to `/` first: under Wine, `Path` ops yield `\` and a
/// drive-less root, which `unix_to_wine` could not lift.
pub fn to_ninja_path(path: &Path, wine: bool) -> String {
    let path = path
        .to_str()
        .expect("header path is valid UTF-8")
        .replace('\\', "/");
    if wine { unix_to_wine(&path) } else { path }
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

// Path helper functions.
// We cannot use `Path` usually, because we want to run this script on Linux for wine.

use std::path::{Path, PathBuf};

pub fn pathdiff(vcproj_dir: &Path, int_dir: &Path) -> PathBuf {
    let mut vcproj_parts = vcproj_dir.components().peekable();
    let mut int_parts = int_dir.components().peekable();
    loop {
        let Some(vcproj_part) = vcproj_parts.peek() else {
            break;
        };
        let Some(int_part) = int_parts.peek() else {
            break;
        };
        if int_part != vcproj_part {
            break;
        }
        vcproj_parts.next();
        int_parts.next();
    }

    let vcproj_parts_unmatched_count = vcproj_parts.clone().count();
    let mut int_rpath = match vcproj_parts_unmatched_count {
        0 => PathBuf::from(".\\"),
        _ => fill_dotdot(vcproj_parts_unmatched_count),
    };
    for int_part in int_parts {
        int_rpath.push(int_part);
    }

    int_rpath
}
#[test]
fn pathdiff_works() {
    fn diff(a: &str, b: &str) -> PathBuf {
        pathdiff(Path::new(a), Path::new(b))
    }
    assert_eq!(diff("/a/b/x/y", "/a/b/c/d"), PathBuf::from("../../c/d"));
    assert_eq!(diff("/a/b/c", "/a/b/c"), PathBuf::from(".\\"));
    assert_eq!(diff("/a/b", "/a/b/c/d"), PathBuf::from(".\\c\\d"));
    assert_eq!(diff("/a/b/c/d", "/a/b"), PathBuf::from("../.."));
}

pub fn fill_dotdot(c: usize) -> PathBuf {
    std::iter::repeat_n("..", c).collect::<PathBuf>()
}

pub fn canonize_path(s: &str) -> String {
    let mut parts = Vec::new();
    for part in s.split(['\\', '/']) {
        match part {
            ".." if parts.iter().all(|&p| p == "..") => parts.push(part),
            "" if parts.is_empty() => parts.push(part),
            ".." => _ = parts.pop(),
            "." | "" => {}
            part => parts.push(part),
        }
    }
    parts.join("\\")
}

#[test]
#[rustfmt::skip]
fn canonize_path_works() {
    assert_eq!(canonize_path("path/../to/file.txt"),          "to\\file.txt");
    assert_eq!(canonize_path("../path/../to/file.txt"),       "..\\to\\file.txt");
    assert_eq!(canonize_path("../../path/../to/file.txt"),    "..\\..\\to\\file.txt");
    assert_eq!(canonize_path("path/../../to/file.txt"),       "..\\to\\file.txt");
    assert_eq!(canonize_path("path/../../to/../../file.txt"), "..\\..\\file.txt");

}

pub fn concat_path(lhs: &str, rhs: &str) -> String {
    let mut parts = Vec::new();
    for part in lhs.split(['\\', '/']).chain(rhs.split(['\\', '/'])) {
        parts.push(part)
    }
    parts.join("\\")
}

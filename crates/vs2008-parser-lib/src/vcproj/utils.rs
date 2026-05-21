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
        _ => std::iter::repeat_n("..", vcproj_parts_unmatched_count).collect::<PathBuf>(),
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

pub fn clean(s: &str) -> &str {
    s.trim().trim_matches('"')
}

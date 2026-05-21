// Path helper functions.
// We cannot use `Path` usually, because we want to run this script on Linux for wine.

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

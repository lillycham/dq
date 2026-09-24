use std::fs;
use std::path::Path;

use dq::Query;
use tempfile::TempDir;

/// Tree used by most tests:
///
/// ```text
/// README.md
/// data.json      {"version": "1.2"}
/// src/main.rs
/// src/img/a.jpeg
/// .git/HEAD
/// ```
fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for d in ["src/img", ".git"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    for (f, s) in [
        ("README.md", "# hi\n"),
        ("data.json", r#"{"version": "1.2"}"#),
        ("src/main.rs", "fn main() {}\n"),
        ("src/img/a.jpeg", "jpeg"),
        (".git/HEAD", "ref"),
    ] {
        fs::write(root.join(f), s).unwrap();
    }
    dir
}

/// Run `code` on `root` and return every output as compact JSON.
fn run(root: &Path, code: &str) -> Vec<String> {
    let query = Query::compile(code).unwrap();
    let input = dq::root(root).unwrap();
    query.run(input).map(|v| v.unwrap().to_string()).collect()
}

/// Paths output by `code`, relative to `root`.
fn paths(root: &Path, code: &str) -> Vec<String> {
    let prefix = format!("{}/", root.display());
    run(root, &format!("{code} | .path"))
        .into_iter()
        .map(|p| p.trim_matches('"').trim_start_matches(&prefix).to_owned())
        .collect()
}

#[test]
fn tree_walks_depth_first_in_name_order() {
    let dir = fixture();
    let got = paths(dir.path(), "tree");
    let want = [
        ".git",
        ".git/HEAD",
        "README.md",
        "data.json",
        "src",
        "src/img",
        "src/img/a.jpeg",
        "src/main.rs",
    ];
    assert_eq!(got, want);
}

#[test]
fn tree_with_condition_prunes() {
    let dir = fixture();
    let got = paths(
        dir.path(),
        "tree(.hidden | not) | select(.type == \"file\")",
    );
    assert_eq!(
        got,
        ["README.md", "data.json", "src/img/a.jpeg", "src/main.rs"]
    );
}

#[test]
fn ls_lists_children_only() {
    let dir = fixture();
    assert_eq!(
        paths(dir.path(), "at(\"src\") | ls"),
        ["src/img", "src/main.rs"]
    );
    assert_eq!(
        paths(dir.path(), "at(\"README.md\") | ls"),
        Vec::<String>::new()
    );
}

#[test]
fn metadata_fields() {
    let dir = fixture();
    let got = run(
        dir.path(),
        "at(\"src/main.rs\") | [.name, .stem, .ext, .type, .size, .hidden]",
    );
    assert_eq!(got, [r#"["main.rs","main","rs","file",13,false]"#]);
    let got = run(dir.path(), "at(\".git\") | [.name, .ext, .type, .hidden]");
    assert_eq!(got, [r#"[".git",null,"dir",true]"#]);
}

#[test]
fn content_feeds_back_into_jq() {
    let dir = fixture();
    let got = run(
        dir.path(),
        "at(\"data.json\") | content | fromjson | .version",
    );
    assert_eq!(got, [r#""1.2""#]);
}

#[test]
fn aggregates_work_like_jq() {
    let dir = fixture();
    assert_eq!(run(dir.path(), "[files | .size] | add"), ["43"]);
    let visible = "[tree(.hidden | not) | select(.type == \"file\") | .size] | add";
    assert_eq!(run(dir.path(), visible), ["40"]);
}

#[test]
fn missing_entry_is_a_runtime_error() {
    let dir = fixture();
    let query = Query::compile("at(\"nope\")").unwrap();
    let mut out = query.run(dq::root(dir.path()).unwrap());
    assert!(out.next().unwrap().is_err());
}

#[test]
fn compile_errors_have_positions() {
    let err = Query::compile(". | nope").err().unwrap();
    assert_eq!(err, "1:5: undefined filter `nope/0`");
    let err = Query::compile("at(1").err().unwrap();
    assert!(err.starts_with("1:5: expected"), "{err}");
}

use std::fs;
use std::path::Path;

use dq::{Op, Plan, Query};
use jaq_json::Val;
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

fn plan(root: &Path, code: &str) -> Plan {
    let query = Query::compile(code).unwrap();
    let mut plan = Plan::default();
    for v in query.run(dq::root(root).unwrap()) {
        plan.push(Op::from_val(&v.unwrap()).expect("an operation").unwrap());
    }
    plan
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

#[test]
fn values_that_are_not_operations_are_ignored_by_the_plan() {
    assert!(Op::from_val(&Val::from("rm".to_owned())).is_none());
}

#[test]
fn mv_target_is_relative_to_the_parent() {
    let dir = fixture();
    let plan = plan(
        dir.path(),
        "files | select(.ext == \"jpeg\") | mv(.stem + \".jpg\")",
    );
    let root = dir.path();
    assert_eq!(
        plan.ops(),
        [Op::Mv {
            from: root.join("src/img/a.jpeg"),
            to: root.join("src/img/a.jpg")
        }]
    );
    plan.check().unwrap();
    plan.apply().unwrap();
    assert!(root.join("src/img/a.jpg").exists());
    assert!(!root.join("src/img/a.jpeg").exists());
}

#[test]
fn apply_creates_writes_copies_and_removes() {
    let dir = fixture();
    let root = dir.path();
    let code = r#"
        (child("out") | mkdir),
        (child("out/x.txt") | write("x")),
        (at("README.md") | cp("out/README.md")),
        (at(".git") | rm)
    "#;
    let plan = plan(root, code);
    plan.check().unwrap();
    plan.apply().unwrap();
    assert_eq!(fs::read_to_string(root.join("out/x.txt")).unwrap(), "x");
    assert_eq!(
        fs::read_to_string(root.join("out/README.md")).unwrap(),
        "# hi\n"
    );
    assert!(!root.join(".git").exists());
}

#[test]
fn duplicate_operations_are_merged() {
    let dir = fixture();
    let plan = plan(dir.path(), "at(\"README.md\") | rm, rm");
    assert_eq!(plan.ops().len(), 1);
}

#[test]
fn check_rejects_conflicts_without_changing_anything() {
    let dir = fixture();
    let root = dir.path();
    let cases = [
        // Removing a directory and something inside it.
        "(at(\"src\") | rm), (at(\"src/main.rs\") | rm)",
        // Two moves to the same place.
        "(at(\"README.md\") | mv(\"x\")), (at(\"data.json\") | mv(\"x\"))",
        // Moving onto something that exists.
        "at(\"README.md\") | mv(\"data.json\")",
        // Writing into a directory that does not exist.
        "child(\"nope/x\") | write(\"x\")",
    ];
    for code in cases {
        let plan = plan(root, code);
        assert!(plan.check().is_err(), "{code} should not pass the check");
    }
    assert_eq!(paths(root, "tree").len(), 8);
}

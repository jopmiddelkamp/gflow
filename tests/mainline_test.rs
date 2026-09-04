mod common;

use common::MockGit;
use gflow::mainline::{resolve_main_branch, MAIN_BRANCH_KEY};

fn git_with_branches(local: &[&str], remote: &[&str]) -> MockGit {
    let mut git = MockGit::new();
    for b in local { git.existing_local_branches.insert(b.to_string()); }
    for b in remote { git.existing_remote_branches.insert(b.to_string()); }
    git
}

#[test]
fn a_configured_value_is_used_without_probing_any_branch() {
    for configured in ["main", "master"] {
        let mut git = git_with_branches(&[], &[]);
        git.config.insert(MAIN_BRANCH_KEY.to_string(), configured.to_string());

        assert_eq!(resolve_main_branch(&git).unwrap(), configured);
        assert_eq!(git.calls(), vec![format!("get_config:{MAIN_BRANCH_KEY}")],
            "a configured mainline is the answer — no detection, no write-back");
    }
}

#[test]
fn an_unsupported_configured_value_is_a_hard_error_naming_the_key() {
    let mut git = git_with_branches(&["trunk"], &[]);
    git.config.insert(MAIN_BRANCH_KEY.to_string(), "trunk".to_string());

    let err = resolve_main_branch(&git).unwrap_err();

    assert!(err.contains(MAIN_BRANCH_KEY), "must name the key; got: {err}");
    assert!(err.contains("main") && err.contains("master"), "must name the legal values; got: {err}");
    assert!(err.contains("trunk"), "must name the offending value; got: {err}");
}

#[test]
fn an_unset_key_detects_main_and_saves_it_locally() {
    let git = git_with_branches(&["main", "develop"], &[]);

    assert_eq!(resolve_main_branch(&git).unwrap(), "main");
    assert!(git.calls().contains(&format!("set_config:local:{MAIN_BRANCH_KEY}:main")),
        "the detected value must be persisted; calls: {:?}", git.calls());
}

#[test]
fn an_unset_key_detects_master_when_no_main_exists() {
    let git = git_with_branches(&["master"], &[]);

    assert_eq!(resolve_main_branch(&git).unwrap(), "master");
    assert!(git.calls().contains(&format!("set_config:local:{MAIN_BRANCH_KEY}:master")),
        "calls: {:?}", git.calls());
}

#[test]
fn detection_looks_at_remote_branches_too() {
    // A fresh clone that has not checked main out yet still has origin/main.
    let git = git_with_branches(&["master"], &["main"]);

    assert_eq!(resolve_main_branch(&git).unwrap(), "main",
        "main wins over master wherever it exists");
}

#[test]
fn a_repo_with_neither_branch_defaults_to_main() {
    let git = git_with_branches(&[], &[]);

    assert_eq!(resolve_main_branch(&git).unwrap(), "main");
    assert!(git.calls().contains(&format!("set_config:local:{MAIN_BRANCH_KEY}:main")),
        "calls: {:?}", git.calls());
}

#[test]
fn an_empty_configured_value_falls_back_to_detection() {
    // decisions.md: reads are trimmed and empty-after-trim falls back to default.
    let mut git = git_with_branches(&["master"], &[]);
    git.config.insert(MAIN_BRANCH_KEY.to_string(), "  ".to_string());

    assert_eq!(resolve_main_branch(&git).unwrap(), "master");
}

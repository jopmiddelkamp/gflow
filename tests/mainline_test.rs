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
fn detection_prefers_main_then_master_then_defaults_to_main() {
    let cases: [(&str, &[&str], &[&str], &str); 4] = [
        ("an_unset_key_detects_main", &["main", "develop"], &[], "main"),
        ("master_when_no_main_exists", &["master"], &[], "master"),
        // A fresh clone that has not checked main out yet still has origin/main.
        ("a_remote_main_wins_over_a_local_master", &["master"], &["main"], "main"),
        ("neither_branch_defaults_to_main", &[], &[], "main"),
    ];
    for (case, local, remote, expected) in cases {
        let git = git_with_branches(local, remote);

        assert_eq!(resolve_main_branch(&git).unwrap_or_else(|error| panic!("{case}: {error}")), expected,
            "{case}: main wins over master wherever it exists");
        assert!(git.calls().contains(&format!("set_config:local:{MAIN_BRANCH_KEY}:{expected}")),
            "{case}: the detected value must be persisted; calls: {:?}", git.calls());
    }
}

#[test]
fn an_empty_configured_value_falls_back_to_detection() {
    // decisions.md: reads are trimmed and empty-after-trim falls back to default.
    let mut git = git_with_branches(&["master"], &[]);
    git.config.insert(MAIN_BRANCH_KEY.to_string(), "  ".to_string());

    assert_eq!(resolve_main_branch(&git).unwrap(), "master");
}

#[test]
fn mainline_lookup_failures_never_guess_or_persist_a_fallback() {
    for failure in [
        "get_config:gflow.branch.main",
        "local_branch_exists:main",
        "remote_branch_exists:main",
        "set_config:local:gflow.branch.main:main",
    ] {
        let mut git = MockGit::new();
        git.fail_call = Some((failure.into(), 1));

        assert_eq!(
            resolve_main_branch(&git).unwrap_err(),
            format!("injected git failure: {failure}")
        );
        assert_eq!(git.calls().last().unwrap(), failure);
    }
}

mod common;

use common::{MockCommandRunner, MockGit, MockHosting};
use gflow::flows::finish_release::finish_release;
use gflow::flows::start::start_hotfix_fix;
use gflow::git::{Git, GitCli};
use gflow::repo_config::{BumpStrategy, Mode, RepoConfig};

#[test]
fn a_release_without_a_staging_tag_stops_before_mutation() {
    for (strategy, error) in [
        (
            BumpStrategy::Rc,
            "No RC tag found on this release branch. Run 'gflow bump' first.",
        ),
        (
            BumpStrategy::Patch,
            "No version tag found on this release branch. Run 'gflow bump' first.",
        ),
    ] {
        let git = MockGit::new();
        let hosting = MockHosting::new();
        let cfg = RepoConfig {
            bump_strategy: strategy,
            ..RepoConfig::default()
        };
        assert_eq!(
            finish_release(&git, &hosting, &cfg, 1, 2, "main", None, false).unwrap_err(),
            error
        );
        assert_eq!(git.calls(), ["tags_on_branch:release/1.2.0"]);
        assert!(hosting.calls().is_empty());
    }
}

#[test]
fn refreshing_a_finish_branch_conflict_preserves_the_source_and_stops_publication() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.2.0-rc.1".into()];
    git.existing_local_branches
        .insert("finish/release-1.2.0-into-main".into());
    git.fail_nth_merge = Some(1);
    let hosting = MockHosting::new();
    let cfg = RepoConfig {
        mode: Mode::Protected,
        ..RepoConfig::default()
    };

    let error = finish_release(&git, &hosting, &cfg, 1, 2, "main", None, false).unwrap_err();

    assert!(
        error.contains("git merge --abort && git switch release/1.2.0"),
        "{error}"
    );
    assert!(error.contains("gflow finish"), "{error}");
    assert_eq!(
        git.calls().last().unwrap(),
        "merge:release/1.2.0:chore: refresh finish/release-1.2.0-into-main with release/1.2.0"
    );
    assert!(!hosting
        .calls()
        .iter()
        .any(|call| call.starts_with("create_or_get_pr:")));
    assert!(!git
        .calls()
        .iter()
        .any(|call| call.starts_with("delete_branch_local:")
            || call.starts_with("delete_branch_remote:")
            || call.starts_with("push:finish/")));
}

#[test]
fn a_landing_pr_uses_its_explicit_template() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.2.0-rc.1".into()];
    let hosting = MockHosting::new();
    let cfg = RepoConfig {
        mode: Mode::Protected,
        ..RepoConfig::default()
    };

    finish_release(
        &git,
        &hosting,
        &cfg,
        1,
        2,
        "main",
        Some(std::path::Path::new("landing.md")),
        false,
    )
    .unwrap();

    assert!(hosting.calls().iter().any(|call| call ==
        "create_or_get_pr:finish/release-1.2.0-into-main:main:chore: merge release 1.2.0 into main:template=landing.md"),
        "{:?}", hosting.calls());
}

#[test]
fn the_first_patch_hotfix_starts_at_zero_zero_one() {
    let mut git = MockGit::new();
    git.current_branch = "main".into();
    let cfg = RepoConfig {
        bump_strategy: BumpStrategy::Patch,
        ..RepoConfig::default()
    };

    start_hotfix_fix(
        &git,
        &MockHosting::new(),
        &cfg,
        "repair",
        false,
        None,
        "main",
        None,
    )
    .unwrap();

    assert!(git
        .calls()
        .contains(&"create_branch:hotfix/0.0.1:main".into()));
    assert!(git
        .calls()
        .contains(&"create_branch:hotfix-fix/0.0.1/repair:hotfix/0.0.1".into()));
}

#[cfg(unix)]
#[test]
fn invalid_worktree_path_encoding_stops_before_running_git() {
    use std::os::unix::ffi::OsStrExt;
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/worktrees/\xff"));
    let runner = MockCommandRunner::scripted(&[]);
    let git = GitCli::new(&runner);
    for result in [
        git.is_working_tree_clean_at(path).map(|_| ()),
        git.ff_merge_at(path, "develop"),
        git.merge_at(path, "develop", "merge into develop"),
    ] {
        assert!(result.unwrap_err().contains("Path is not valid UTF-8"));
    }
    assert!(runner.calls().is_empty());
}

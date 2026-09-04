mod common;

use common::{MockGit, MockHosting, MockVersionScript};
use gflow::flows::finish_release::{bump_version, sync_with_develop, finish_release};
use gflow::repo_config::{BumpStrategy, Mode, RepoConfig};

fn patch_cfg() -> RepoConfig {
    RepoConfig { bump_strategy: BumpStrategy::Patch, ..RepoConfig::default() }
}

fn patch_protected_cfg() -> RepoConfig {
    RepoConfig { mode: Mode::Protected, bump_strategy: BumpStrategy::Patch, ..RepoConfig::default() }
}

/// Configure a MockGit for a "fresh start" release finish: nothing yet merged,
/// no tags exist past the latest RC, source branch still exists.
fn fresh_release_mock(major: u32, minor: u32, rc_tags: &[&str]) -> MockGit {
    let mut git = MockGit::new();
    let source = format!("release/{major}.{minor}.0");
    git.existing_local_branches.insert(source.clone());
    git.existing_remote_branches.insert(source);
    git.tags_on_branch = rc_tags.iter().map(|t| t.to_string()).collect();
    git
}

#[test]
fn bump_version_increments_rc() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];

    bump_version(&git, &MockHosting::new(), None, &RepoConfig::default(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "create_tag:v1.1.0-rc.2:chore: bump version to v1.1.0-rc.2",
        "push_tag:v1.1.0-rc.2",
    ]);
}

#[test]
fn bump_version_multiple_rcs() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string(), "v1.1.0-rc.2".to_string()];

    bump_version(&git, &MockHosting::new(), None, &RepoConfig::default(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "create_tag:v1.1.0-rc.3:chore: bump version to v1.1.0-rc.3",
        "push_tag:v1.1.0-rc.3",
    ]);
}

#[test]
fn bump_version_ignores_mismatched_patch_tags() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string(), "v1.1.1".to_string()];

    bump_version(&git, &MockHosting::new(), None, &RepoConfig::default(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "create_tag:v1.1.0-rc.2:chore: bump version to v1.1.0-rc.2",
        "push_tag:v1.1.0-rc.2",
    ]);
}

#[test]
fn bump_version_ignores_non_rc_pre_release_tags() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string(), "v1.1.0-beta.5".to_string()];

    bump_version(&git, &MockHosting::new(), None, &RepoConfig::default(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "create_tag:v1.1.0-rc.2:chore: bump version to v1.1.0-rc.2",
        "push_tag:v1.1.0-rc.2",
    ]);
}

#[test]
fn bump_version_cuts_rc1_when_branch_has_no_rc_tag() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec![];

    bump_version(&git, &MockHosting::new(), None, &RepoConfig::default(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: bump version to v1.1.0-rc.1",
        "push_tag:v1.1.0-rc.1",
    ]);
}

#[test]
fn bump_free_with_script_commits_and_pushes_before_tagging() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    git.working_tree_clean_seq.get_mut().extend([true, false]);
    let script = MockVersionScript::new();

    bump_version(&git, &MockHosting::new(), Some(&script), &RepoConfig::default(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "is_working_tree_clean",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.2:chore: bump version to v1.1.0-rc.2",
        "push_tag:v1.1.0-rc.2",
    ]);
    assert_eq!(script.calls(), vec!["run:1.1.0"]);
}

#[test]
fn bump_free_with_script_noop_still_cuts_the_tag() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    git.working_tree_clean_seq.get_mut().extend([true, true]);
    let script = MockVersionScript::new();

    bump_version(&git, &MockHosting::new(), Some(&script), &RepoConfig::default(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "is_working_tree_clean",
        "is_working_tree_clean",
        "create_tag:v1.1.0-rc.2:chore: bump version to v1.1.0-rc.2",
        "push_tag:v1.1.0-rc.2",
    ]);
}

#[test]
fn bump_patch_increments_patch_and_tags_clean() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0".to_string()];

    bump_version(&git, &MockHosting::new(), None, &patch_cfg(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "create_tag:v1.1.1:chore: bump version to v1.1.1",
        "push_tag:v1.1.1",
    ]);
}

#[test]
fn bump_patch_ignores_pre_release_and_other_minor_tags() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec![
        "v1.1.0".to_string(),
        "v1.1.1".to_string(),
        "v1.1.2-rc.1".to_string(),
        "v1.0.9".to_string(),
    ];

    bump_version(&git, &MockHosting::new(), None, &patch_cfg(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "create_tag:v1.1.2:chore: bump version to v1.1.2",
        "push_tag:v1.1.2",
    ]);
}

#[test]
fn bump_patch_with_no_tag_tags_the_release_version_itself() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec![];

    bump_version(&git, &MockHosting::new(), None, &patch_cfg(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "create_tag:v1.1.0:chore: bump version to v1.1.0",
        "push_tag:v1.1.0",
    ]);
}

#[test]
fn bump_patch_with_script_runs_it_with_the_new_version() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0".to_string()];
    git.working_tree_clean_seq.get_mut().extend([true, false]);
    let script = MockVersionScript::new();

    bump_version(&git, &MockHosting::new(), Some(&script), &patch_cfg(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "is_working_tree_clean",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.1",
        "push:release/1.1.0",
        "create_tag:v1.1.1:chore: bump version to v1.1.1",
        "push_tag:v1.1.1",
    ]);
    assert_eq!(script.calls(), vec!["run:1.1.1"]);
}

#[test]
fn bump_patch_protected_fresh_defers_the_tag_and_uses_the_new_version() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0".to_string()];
    git.working_tree_clean_seq.get_mut().extend([true, false]);
    let hosting = MockHosting::new();
    let script = MockVersionScript::new();

    bump_version(&git, &hosting, Some(&script), &patch_protected_cfg(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "remote_branch_exists:release-chore/1.1.0/set-version",
        "is_working_tree_clean",
        "local_branch_exists:release-chore/1.1.0/set-version",
        "create_branch:release-chore/1.1.0/set-version:release/1.1.0",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.1",
        "push:release-chore/1.1.0/set-version",
        "checkout:release/1.1.0",
    ]);
    assert_eq!(hosting.calls(), vec![
        "merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0",
        "create_or_get_pr:release-chore/1.1.0/set-version:release/1.1.0:chore: set version 1.1.1",
        "copy_text:chore: set version 1.1.1\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    assert_eq!(script.calls(), vec!["run:1.1.1"]);
    assert!(!git.calls().iter().any(|c| c.starts_with("create_tag")));
}

#[test]
fn bump_patch_protected_cuts_the_deferred_tag_at_the_merge_commit() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0".to_string()];
    git.tag_commits.insert("v1.1.0".to_string(), "old-tip-sha".to_string());
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("release-chore/1.1.0/set-version".to_string(), "release/1.1.0".to_string()),
        gflow::hosting::LandedPr {
            url: "https://github.com/org/repo/pull/9".to_string(),
            head_sha: "chore-head-sha".to_string(),
            merge_commit_sha: "merge-commit-sha".to_string(),
        },
    );
    git.parent_counts.insert("merge-commit-sha".to_string(), 2);
    let script = MockVersionScript::new();

    bump_version(&git, &hosting, Some(&script), &patch_protected_cfg(), 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "tag_commit_sha:v1.1.0",
        "create_tag_at:v1.1.1:chore: bump version to v1.1.1:merge-commit-sha",
        "push_tag:v1.1.1",
        "local_branch_exists:release-chore/1.1.0/set-version",
        "remote_branch_exists:release-chore/1.1.0/set-version",
    ]);
    assert_eq!(hosting.calls(), vec!["merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0"]);
    // Convergence rule carries over from rc mode: never re-run the script on
    // the merged-PR path.
    assert!(script.calls().is_empty());
}

#[test]
fn bump_protected_fresh_with_changes_defers_the_tag_to_the_landing_pr() {
    let mut git = MockGit::new();
    git.working_tree_clean_seq.get_mut().extend([true, false]);
    let hosting = MockHosting::new();
    let script = MockVersionScript::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    bump_version(&git, &hosting, Some(&script), &cfg, 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "remote_branch_exists:release-chore/1.1.0/set-version",
        "is_working_tree_clean",
        "local_branch_exists:release-chore/1.1.0/set-version",
        "create_branch:release-chore/1.1.0/set-version:release/1.1.0",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release-chore/1.1.0/set-version",
        "checkout:release/1.1.0",
    ]);
    assert_eq!(hosting.calls(), vec![
        "merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0",
        "create_or_get_pr:release-chore/1.1.0/set-version:release/1.1.0:chore: set version 1.1.0",
        "copy_text:chore: set version 1.1.0\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    // The deferral: no tag is cut on this run, of any kind.
    assert!(!git.calls().iter().any(|c| c.starts_with("create_tag")));
}

#[test]
fn bump_protected_fresh_noop_cuts_the_tag_immediately() {
    let mut git = MockGit::new();
    git.working_tree_clean_seq.get_mut().extend([true, true]);
    let hosting = MockHosting::new();
    let script = MockVersionScript::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    bump_version(&git, &hosting, Some(&script), &cfg, 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "remote_branch_exists:release-chore/1.1.0/set-version",
        "is_working_tree_clean",
        "local_branch_exists:release-chore/1.1.0/set-version",
        "create_branch:release-chore/1.1.0/set-version:release/1.1.0",
        "is_working_tree_clean",
        "checkout:release/1.1.0",
        "delete_branch_local:release-chore/1.1.0/set-version",
        "tags_on_branch:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: bump version to v1.1.0-rc.1",
        "push_tag:v1.1.0-rc.1",
    ]);
    assert_eq!(hosting.calls(), vec!["merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0"]);
}

#[test]
fn bump_protected_cuts_the_deferred_tag_at_the_merge_commit_once_the_pr_lands() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    git.tag_commits.insert("v1.1.0-rc.1".to_string(), "old-tip-sha".to_string());
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("release-chore/1.1.0/set-version".to_string(), "release/1.1.0".to_string()),
        gflow::hosting::LandedPr {
            url: "https://github.com/org/repo/pull/9".to_string(),
            head_sha: "chore-head-sha".to_string(),
            merge_commit_sha: "merge-commit-sha".to_string(),
        },
    );
    git.parent_counts.insert("merge-commit-sha".to_string(), 2);
    let script = MockVersionScript::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    bump_version(&git, &hosting, Some(&script), &cfg, 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "tag_commit_sha:v1.1.0-rc.1",
        "create_tag_at:v1.1.0-rc.2:chore: bump version to v1.1.0-rc.2:merge-commit-sha",
        "push_tag:v1.1.0-rc.2",
        "local_branch_exists:release-chore/1.1.0/set-version",
        "remote_branch_exists:release-chore/1.1.0/set-version",
    ]);
    assert_eq!(hosting.calls(), vec!["merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0"]);
    // Convergence rule: a history-derived script would diff again after every
    // merge, so the merged-PR path never re-runs it.
    assert!(script.calls().is_empty());
}

#[test]
fn bump_protected_already_consumed_falls_through_to_the_fresh_path() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    git.tag_commits.insert("v1.1.0-rc.1".to_string(), "merge-commit-sha".to_string());
    git.working_tree_clean_seq.get_mut().extend([true, false]);
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("release-chore/1.1.0/set-version".to_string(), "release/1.1.0".to_string()),
        gflow::hosting::LandedPr {
            url: "https://github.com/org/repo/pull/9".to_string(),
            head_sha: "chore-head-sha".to_string(),
            merge_commit_sha: "merge-commit-sha".to_string(),
        },
    );
    git.parent_counts.insert("merge-commit-sha".to_string(), 2);
    let script = MockVersionScript::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    bump_version(&git, &hosting, Some(&script), &cfg, 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "tag_commit_sha:v1.1.0-rc.1",
        "local_branch_exists:release-chore/1.1.0/set-version",
        "remote_branch_exists:release-chore/1.1.0/set-version",
        "remote_branch_exists:release-chore/1.1.0/set-version",
        "is_working_tree_clean",
        "local_branch_exists:release-chore/1.1.0/set-version",
        "create_branch:release-chore/1.1.0/set-version:release/1.1.0",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release-chore/1.1.0/set-version",
        "checkout:release/1.1.0",
    ]);
    assert_eq!(hosting.calls(), vec![
        "merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0",
        "create_or_get_pr:release-chore/1.1.0/set-version:release/1.1.0:chore: set version 1.1.0",
        "copy_text:chore: set version 1.1.0\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    // Consumed on a previous run — this run never re-cuts a tag.
    assert!(!git.calls().iter().any(|c| c.starts_with("create_tag")));
}

#[test]
fn bump_protected_with_no_script_tags_the_tip_directly() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    let hosting = MockHosting::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    bump_version(&git, &hosting, None, &cfg, 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "create_tag:v1.1.0-rc.2:chore: bump version to v1.1.0-rc.2",
        "push_tag:v1.1.0-rc.2",
    ]);
    assert_eq!(hosting.calls(), vec!["merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0"]);
}

#[test]
fn bump_protected_reuses_a_leftover_remote_chore_branch_without_recreating_it() {
    let mut git = MockGit::new();
    git.existing_remote_branches.insert("release-chore/1.1.0/set-version".to_string());
    let hosting = MockHosting::new();
    let script = MockVersionScript::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    bump_version(&git, &hosting, Some(&script), &cfg, 1, 1).unwrap();

    assert_eq!(git.calls(), vec!["remote_branch_exists:release-chore/1.1.0/set-version"]);
    assert_eq!(hosting.calls(), vec![
        "merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0",
        "create_or_get_pr:release-chore/1.1.0/set-version:release/1.1.0:chore: set version 1.1.0",
        "copy_text:chore: set version 1.1.0\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    assert!(script.calls().is_empty());
    assert!(!git.calls().iter().any(|c| c.starts_with("create_branch")));
}

#[test]
fn bump_protected_deletes_leftover_local_chore_branch_before_recreating() {
    // A prior run crashed after `create_branch` but before the chore branch was
    // ever pushed: it exists locally only. Re-running must not die on git's raw
    // "branch already exists" — the leftover is machine-owned, so gflow clears
    // it itself before recreating.
    let mut git = MockGit::new();
    git.existing_local_branches.insert("release-chore/1.1.0/set-version".to_string());
    git.working_tree_clean_seq.get_mut().extend([true, false]);
    let hosting = MockHosting::new();
    let script = MockVersionScript::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    bump_version(&git, &hosting, Some(&script), &cfg, 1, 1).unwrap();

    assert_eq!(git.calls(), vec![
        "remote_branch_exists:release-chore/1.1.0/set-version",
        "is_working_tree_clean",
        "local_branch_exists:release-chore/1.1.0/set-version",
        "delete_branch_local:release-chore/1.1.0/set-version",
        "create_branch:release-chore/1.1.0/set-version:release/1.1.0",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release-chore/1.1.0/set-version",
        "checkout:release/1.1.0",
    ]);
    assert_eq!(hosting.calls(), vec![
        "merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0",
        "create_or_get_pr:release-chore/1.1.0/set-version:release/1.1.0:chore: set version 1.1.0",
        "copy_text:chore: set version 1.1.0\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

#[test]
fn bump_protected_script_failure_returns_to_the_release_branch() {
    // A failed version script must not strand the operator on the chore
    // branch: gflow best-effort restores the release branch before the error
    // propagates, and never pushes or opens a PR for a run that never committed.
    let mut git = MockGit::new();
    git.working_tree_clean_seq.get_mut().extend([true]);
    let hosting = MockHosting::new();
    let mut script = MockVersionScript::new();
    script.fail = Some("set-version.sh: command not found".to_string());
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    let err = bump_version(&git, &hosting, Some(&script), &cfg, 1, 1).unwrap_err();

    assert_eq!(err, "set-version.sh: command not found");
    let calls = git.calls();
    assert_eq!(calls, vec![
        "remote_branch_exists:release-chore/1.1.0/set-version",
        "is_working_tree_clean",
        "local_branch_exists:release-chore/1.1.0/set-version",
        "create_branch:release-chore/1.1.0/set-version:release/1.1.0",
        "checkout:release/1.1.0",
    ]);
    assert!(!calls.iter().any(|c| c.starts_with("push:")), "no push on script failure; calls: {calls:?}");
    assert_eq!(hosting.calls(), vec!["merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0"],
        "no PR call on script failure");
}

#[test]
fn sync_with_develop_merges_and_returns_to_current() {
    let mut git = MockGit::new();
    git.current_branch = "release/1.1.0".to_string();
    let hosting = MockHosting::new();

    sync_with_develop(&git, &hosting, &RepoConfig::default(), 1, 1, None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "checkout:develop",
        "ff_merge:origin/develop",
        "merge:release/1.1.0:chore: sync release 1.1.0 with develop",
        "push:develop",
        "checkout:release/1.1.0",
    ]);
    assert!(hosting.calls().is_empty(), "free mode must make zero hosting calls; calls: {:?}", hosting.calls());
}

#[test]
fn sync_protected_opens_develop_pr_and_stops() {
    let mut git = MockGit::new();
    git.current_branch = "release/1.1.0".to_string();
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());

    let hosting = MockHosting::new();
    sync_with_develop(&git, &hosting, &protected_cfg(false), 1, 1, None, false).unwrap();

    assert_eq!(hosting.calls(), vec![
        "open_pr_to:release/1.1.0:develop",
        "merged_pr_to:finish/release-1.1.0-into-develop:develop",
        "merged_pr_to:release/1.1.0:develop",
        "create_or_get_pr:finish/release-1.1.0-into-develop:develop:chore: sync release 1.1.0 with develop:empty-body",
        "copy_text:chore: sync release 1.1.0 with develop\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c == "checkout:develop"), "must never land into develop locally; calls: {calls:?}");
    assert!(calls.iter().filter(|c| c.starts_with("merge:")).all(|c| c.ends_with("into finish/release-1.1.0-into-develop")),
        "merges may only append to the finish branch; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c == "push:develop"), "must not push develop directly; calls: {calls:?}");
}

#[test]
fn sync_protected_already_landed_is_a_noop() {
    let mut git = MockGit::new();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/develop".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "develop".to_string()), landed("relsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);

    sync_with_develop(&git, &hosting, &protected_cfg(false), 1, 1, None, false).unwrap();

    assert_eq!(git.calls(), vec!["is_ancestor:mc1:origin/develop", "commit_parent_count:mc1", "branch_sha:release/1.1.0"],
        "already-landed sync must make no mutating git calls beyond the landed-check reads");
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:release/1.1.0:develop",
        "merged_pr_to:finish/release-1.1.0-into-develop:develop",
        "merged_pr_to:release/1.1.0:develop",
    ]);
}

#[test]
fn sync_protected_stale_merged_pr_reopens_new_pr() {
    let mut git = MockGit::new();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());

    git.ancestors.insert(("mc1".to_string(), "origin/develop".to_string()));
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "develop".to_string()), landed("stale", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);

    sync_with_develop(&git, &hosting, &protected_cfg(false), 1, 1, None, false).unwrap();

    assert_eq!(hosting.calls(), vec![
        "open_pr_to:release/1.1.0:develop",
        "merged_pr_to:finish/release-1.1.0-into-develop:develop",
        "merged_pr_to:release/1.1.0:develop",
        "create_or_get_pr:finish/release-1.1.0-into-develop:develop:chore: sync release 1.1.0 with develop:empty-body",
        "copy_text:chore: sync release 1.1.0 with develop\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

#[test]
fn finish_release_creates_clean_tag_from_rc() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1", "v1.1.0-rc.2"]);
    git.rev_list_count_result = 0;

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "is_ancestor:release/1.1.0:main",
        "rev_list_count:v1.1.0-rc.2:release/1.1.0",
        "worktree_of:main",
        "checkout:main",
        "ff_merge:origin/main",
        "merge:release/1.1.0:chore: merge release 1.1.0 into main",
        "tag_exists:v1.1.0",
        "create_tag:v1.1.0:chore: release 1.1.0",
        "is_pushed:main",
        "push:main",
        "remote_tag_exists:v1.1.0",
        "push_tag:v1.1.0",
        "is_ancestor:release/1.1.0:develop",
        "worktree_of:develop",
        "checkout:develop",
        "ff_merge:origin/develop",
        "merge:release/1.1.0:chore: merge release 1.1.0 into develop",
        "is_pushed:develop",
        "push:develop",
        "current_branch",
        "local_branch_exists:release/1.1.0",
        "delete_branch_local:release/1.1.0",
        "remote_branch_exists:release/1.1.0",
        "delete_branch_remote:release/1.1.0",
    ]);
    assert!(hosting.calls().is_empty(), "free mode must make zero hosting calls; calls: {:?}", hosting.calls());
}

#[test]
fn finish_release_targets_master_when_that_is_the_mainline() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1"]);
    git.rev_list_count_result = 0;

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "master", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "is_ancestor:release/1.1.0:master",
        "rev_list_count:v1.1.0-rc.1:release/1.1.0",
        "worktree_of:master",
        "checkout:master",
        "ff_merge:origin/master",
        "merge:release/1.1.0:chore: merge release 1.1.0 into master",
        "tag_exists:v1.1.0",
        "create_tag:v1.1.0:chore: release 1.1.0",
        "is_pushed:master",
        "push:master",
        "remote_tag_exists:v1.1.0",
        "push_tag:v1.1.0",
        "is_ancestor:release/1.1.0:develop",
        "worktree_of:develop",
        "checkout:develop",
        "ff_merge:origin/develop",
        "merge:release/1.1.0:chore: merge release 1.1.0 into develop",
        "is_pushed:develop",
        "push:develop",
        "current_branch",
        "local_branch_exists:release/1.1.0",
        "delete_branch_local:release/1.1.0",
        "remote_branch_exists:release/1.1.0",
        "delete_branch_remote:release/1.1.0",
    ]);
}

#[test]
fn finish_release_patch_mode_merges_without_tagging() {
    // The last bump tag (v1.1.1) is already the final version — finish only merges.
    let mut git = fresh_release_mock(1, 1, &["v1.1.0", "v1.1.1"]);
    git.rev_list_count_result = 0;

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &patch_cfg(), 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "is_ancestor:release/1.1.0:main",
        "rev_list_count:v1.1.1:release/1.1.0",
        "worktree_of:main",
        "checkout:main",
        "ff_merge:origin/main",
        "merge:release/1.1.0:chore: merge release 1.1.0 into main",
        "is_pushed:main",
        "push:main",
        "remote_tag_exists:v1.1.1",
        "push_tag:v1.1.1",
        "is_ancestor:release/1.1.0:develop",
        "worktree_of:develop",
        "checkout:develop",
        "ff_merge:origin/develop",
        "merge:release/1.1.0:chore: merge release 1.1.0 into develop",
        "is_pushed:develop",
        "push:develop",
        "current_branch",
        "local_branch_exists:release/1.1.0",
        "delete_branch_local:release/1.1.0",
        "remote_branch_exists:release/1.1.0",
        "delete_branch_remote:release/1.1.0",
    ]);
    assert!(hosting.calls().is_empty());
}

#[test]
fn finish_release_patch_mode_gate_fires_when_head_past_latest_tag() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0", "v1.1.1"]);
    git.rev_list_count_result = 2;

    let hosting = MockHosting::new();
    let err = finish_release(&git, &hosting, &patch_cfg(), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("v1.1.1"), "error should name the latest patch tag; got: {err}");
    assert!(err.contains("gflow bump"), "error should tell user to bump; got: {err}");
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("checkout:main")),
        "guard must abort before touching main; calls: {calls:?}");
}

#[test]
fn protected_patch_finish_opens_main_pr_without_any_tag_machinery() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0".to_string()];
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &patch_protected_cfg(), 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "tags_on_branch:release/1.1.0",
        "rev_list_count:v1.1.0:release/1.1.0",
        "remote_branch_exists:release/1.1.0",
        "is_pushed:release/1.1.0",
        "remote_branch_exists:finish/release-1.1.0-into-main",
        "local_branch_exists:finish/release-1.1.0-into-main",
        "create_branch_no_checkout:finish/release-1.1.0-into-main:release/1.1.0",
        "is_ancestor:release/1.1.0:finish/release-1.1.0-into-main",
        "is_ancestor:origin/main:finish/release-1.1.0-into-main",
        "current_branch",
        "checkout:finish/release-1.1.0-into-main",
        "merge:origin/main:chore: merge main into finish/release-1.1.0-into-main",
        "checkout:develop",
        "push:finish/release-1.1.0-into-main",
    ]);
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:release/1.1.0:main",
        "merged_pr_to:finish/release-1.1.0-into-main:main",
        "merged_pr_to:release/1.1.0:main",
        "create_or_get_pr:finish/release-1.1.0-into-main:main:chore: merge release 1.1.0 into main:empty-body",
        "copy_text:chore: merge release 1.1.0 into main\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("tag_exists")),
        "patch mode has no tag step to sequence on; calls: {calls:?}");
}

#[test]
fn protected_patch_finish_completes_after_both_legs_without_tagging() {
    let mut git = MockGit::new();
    git.current_branch = "release/1.1.0".to_string();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_local_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.tags_on_branch = vec!["v1.1.0".to_string(), "v1.1.1".to_string()];
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "main".to_string()), landed("relsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "develop".to_string()), landed("relsha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_release(&git, &hosting, &patch_protected_cfg(), 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "is_ancestor:mc1:origin/main",
        "commit_parent_count:mc1",
        "tags_on_branch:release/1.1.0",
        "remote_tag_exists:v1.1.1",
        "push_tag:v1.1.1",
        "branch_sha:release/1.1.0",
        "is_ancestor:mc2:origin/develop",
        "commit_parent_count:mc2",
        "branch_sha:release/1.1.0",
        "branch_sha:release/1.1.0",
        "list_branches_matching:finish/release-1.1.0-into-*",
        "current_branch",
        "is_linked_worktree",
        "worktree_of:main",
        "checkout:main",
        "local_branch_exists:release/1.1.0",
        "delete_branch_local:release/1.1.0",
        "remote_branch_exists:release/1.1.0",
        "delete_branch_remote:release/1.1.0",
    ]);
    let calls = git.calls();
    // The final tag was cut at bump — finish never creates one, but it does
    // re-push it (a bump's push_tag can have failed on network/auth).
    assert!(!calls.iter().any(|c| c.starts_with("tag_exists:") || c.starts_with("create_tag")),
        "no tag is cut or checked at patch-mode finish; calls: {calls:?}");
}

#[test]
fn the_rc_gate_error_names_the_configured_mainline() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1"]);
    git.rev_list_count_result = 3;

    let hosting = MockHosting::new();
    let err = finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "master", None, false).unwrap_err();

    assert!(err.contains("merged to master"), "got: {err}");
}

#[test]
fn finish_release_single_rc() {
    let mut git = fresh_release_mock(2, 0, &["v2.0.0-rc.1"]);
    git.rev_list_count_result = 0;

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &RepoConfig::default(), 2, 0, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(calls.iter().any(|c| c == "rev_list_count:v2.0.0-rc.1:release/2.0.0"),
        "the only RC is the one the gate measures against; calls: {calls:?}");
    assert!(calls.iter().any(|c| c == "create_tag:v2.0.0:chore: release 2.0.0"),
        "the clean tag is the RC stripped of its pre-release; calls: {calls:?}");
}

#[test]
fn finish_release_fails_when_head_past_latest_rc() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1", "v1.1.0-rc.2"]);
    git.rev_list_count_result = 2; // 2 commits on release/1.1.0 past v1.1.0-rc.2

    let hosting = MockHosting::new();
    let result = finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "main", None, false);

    assert!(result.is_err(), "expected guard to reject finish when HEAD is past latest RC");
    let err = result.unwrap_err();
    assert!(err.contains("v1.1.0-rc.2"), "error should name the latest RC tag; got: {err}");
    assert!(err.contains("gflow bump"), "error should tell user to bump; got: {err}");
    assert!(err.contains("2 commit"), "error should state how many commits past the RC; got: {err}");

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("checkout:main")),
        "guard must abort before touching main; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("create_tag:")),
        "guard must abort before tagging; calls: {calls:?}");
}

#[test]
fn finish_release_error_message_uses_singular_for_one_commit() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1"]);
    git.rev_list_count_result = 1;

    let hosting = MockHosting::new();
    let err = finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "main", None, false).unwrap_err();
    assert!(err.contains("1 commit past"), "expected singular 'commit'; got: {err}");
    assert!(!err.contains("1 commits"), "should not use plural for 1; got: {err}");
}

// --- Idempotent resume tests ---

#[test]
fn finish_release_resume_after_main_already_merged_and_tagged() {
    // Scenario: previous finish merged into main, tagged, pushed, but failed
    // on the develop merge (conflict). User resolves, re-runs.
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1"]);
    git.rev_list_count_result = 0;
    git.ancestors.insert(("release/1.1.0".to_string(), "main".to_string()));
    git.existing_tags.insert("v1.1.0".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.pushed_branches.insert("main".to_string());

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    // No re-merge into main, no re-tag, no re-push
    assert!(!calls.iter().any(|c| c == "checkout:main"), "must not re-checkout main; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("create_tag:")), "must not re-create tag");
    assert!(!calls.iter().any(|c| c == "push:main"), "must not re-push main");
    assert!(!calls.iter().any(|c| c == "push_tag:v1.1.0"), "must not re-push tag");
    // Develop merge still runs
    assert!(calls.iter().any(|c| c == "merge:release/1.1.0:chore: merge release 1.1.0 into develop"), "must merge into develop; calls: {calls:?}");
    // Cleanup runs
    assert!(calls.iter().any(|c| c == "delete_branch_local:release/1.1.0"));
}

#[test]
fn finish_release_resume_skips_rc_gate_when_already_merged_to_main() {
    // Scenario: main is already merged. The rev_list_count gate should be
    // skipped because we're past the merge step.
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1"]);
    // Set count high so that the gate would fail IF it ran — proving it doesn't.
    git.rev_list_count_result = 99;
    git.ancestors.insert(("release/1.1.0".to_string(), "main".to_string()));

    let hosting = MockHosting::new();
    let result = finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "main", None, false);
    assert!(result.is_ok(), "expected resume to succeed past the gate; got: {result:?}");
}

#[test]
fn finish_release_fully_idempotent_no_op_on_second_run() {
    // Everything already done.
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    git.ancestors.insert(("release/1.1.0".to_string(), "main".to_string()));
    git.ancestors.insert(("release/1.1.0".to_string(), "develop".to_string()));
    git.existing_tags.insert("v1.1.0".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    // No entries in existing_local_branches/existing_remote_branches → already deleted

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("merge:")), "no merges; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("create_tag:")));
    assert!(!calls.iter().any(|c| c.starts_with("push:")));
    assert!(!calls.iter().any(|c| c.starts_with("delete_branch_")));
}

#[test]
fn finish_release_keeps_branch_when_configured() {
    // Same fully-completed world as finish_release_fully_idempotent_no_op_on_second_run,
    // except the source branch still exists and HEAD is still on it — proving
    // keep-release-branches=true skips delete_source_branch entirely, not just
    // its individual deletion calls.
    let mut git = MockGit::new();
    git.current_branch = "release/1.1.0".to_string();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    git.ancestors.insert(("release/1.1.0".to_string(), "main".to_string()));
    git.ancestors.insert(("release/1.1.0".to_string(), "develop".to_string()));
    git.existing_tags.insert("v1.1.0".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    git.existing_local_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());

    let hosting = MockHosting::new();
    let cfg = RepoConfig { mode: Mode::Free, keep_release_branches: true, ..RepoConfig::default() };
    finish_release(&git, &hosting, &cfg, 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("delete_branch_")), "keep must skip deletion; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c == "checkout:main"), "keep must skip delete_source_branch's checkout; calls: {calls:?}");
}

// --- Conflict guidance: every merge step must tell the user to switch back ---

#[test]
fn finish_release_main_merge_conflict_names_source_branch_to_switch_back() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1"]);
    git.fail_nth_merge = Some(1); // main merge

    let hosting = MockHosting::new();
    let err = finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "main", None, false).unwrap_err();
    assert!(err.contains("git add . && git commit --no-edit"),
        "the commit step must come before git switch, which fails mid-merge; got: {err}");
    assert!(err.contains("git switch release/1.1.0"),
        "main conflict should tell user to switch back to the release branch; got: {err}");
    assert!(err.contains("gflow finish"), "should mention re-running gflow finish; got: {err}");
}

#[test]
fn finish_release_develop_merge_conflict_names_source_branch_to_switch_back() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1"]);
    git.fail_nth_merge = Some(2); // develop merge

    let hosting = MockHosting::new();
    let err = finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "main", None, false).unwrap_err();
    assert!(err.contains("git switch release/1.1.0"),
        "develop conflict should tell user to switch back to the release branch; got: {err}");
}

// --- Protected mode: sequential landing PRs (run 1 → RC gate → main PR) ---

fn protected_cfg(keep: bool) -> RepoConfig {
    RepoConfig { mode: Mode::Protected, keep_release_branches: keep, ..RepoConfig::default() }
}

fn landed(head_sha: &str, merge_commit_sha: &str) -> gflow::hosting::LandedPr {
    gflow::hosting::LandedPr {
        url: "https://github.com/org/repo/pull/1".to_string(),
        head_sha: head_sha.to_string(),
        merge_commit_sha: merge_commit_sha.to_string(),
    }
}

#[test]
fn protected_finish_rc_gate_blocks_before_pr() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.2".to_string()];
    git.rev_list_count_result = 2;

    let hosting = MockHosting::new();
    let err = finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("v1.1.0-rc.2"), "got: {err}");
    assert!(err.contains("gflow bump"), "got: {err}");
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:release/1.1.0:main",
        "merged_pr_to:finish/release-1.1.0-into-main:main",
        "merged_pr_to:release/1.1.0:main",
    ]);
}

#[test]
fn protected_finish_tags_merge_commit_then_opens_develop_pr() {
    let mut git = MockGit::new();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    let mut hosting = MockHosting::new();
    // Head deliberately unequal to the branch tip: the main leg is proven
    // landed by its merge commit reaching the mainline, never by head equality.
    hosting.merged_prs_to.insert(
        ("release/1.1.0".to_string(), "main".to_string()),
        landed("old-head", "mc1"),
    );
    git.parent_counts.insert("mc1".to_string(), 2);

    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "is_ancestor:mc1:origin/main",
        "commit_parent_count:mc1",
        "tag_exists:v1.1.0",
        "tag_exists:v1.1.0",
        "create_tag_at:v1.1.0:chore: release 1.1.0:mc1",
        "remote_tag_exists:v1.1.0",
        "push_tag:v1.1.0",
        "branch_sha:release/1.1.0",
        "rev_list_count:old-head:release/1.1.0",
        "is_ancestor:release/1.1.0:origin/develop",
        "remote_branch_exists:release/1.1.0",
        "is_pushed:release/1.1.0",
        "remote_branch_exists:finish/release-1.1.0-into-develop",
        "local_branch_exists:finish/release-1.1.0-into-develop",
        "create_branch_no_checkout:finish/release-1.1.0-into-develop:release/1.1.0",
        "is_ancestor:release/1.1.0:finish/release-1.1.0-into-develop",
        "is_ancestor:origin/develop:finish/release-1.1.0-into-develop",
        "current_branch",
        "checkout:finish/release-1.1.0-into-develop",
        "merge:origin/develop:chore: merge develop into finish/release-1.1.0-into-develop",
        "checkout:develop",
        "push:finish/release-1.1.0-into-develop",
    ]);
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:release/1.1.0:main",
        "merged_pr_to:finish/release-1.1.0-into-main:main",
        "merged_pr_to:release/1.1.0:main",
        "open_pr_to:release/1.1.0:develop",
        "merged_pr_to:finish/release-1.1.0-into-develop:develop",
        "merged_pr_to:release/1.1.0:develop",
        "create_or_get_pr:finish/release-1.1.0-into-develop:develop:chore: merge release 1.1.0 into develop:empty-body",
        "copy_text:chore: merge release 1.1.0 into develop\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    let calls = git.calls();
    // `tags_on_branch` is the gate's own first call and nothing else makes it,
    // so it discriminates where `rev_list_count` no longer can — the past-landing
    // report counts commits with the same primitive.
    assert!(!calls.iter().any(|c| c.starts_with("tags_on_branch")), "gate must be skipped once landed; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("rev_list_count:v")), "the gate's count is from an RC tag; calls: {calls:?}");
}

#[test]
fn a_squashed_main_landing_hard_stops_the_finish() {
    // Landing PRs must be completed with a merge commit. A 1-parent commit on
    // main means the PR was squashed — the finish stops before any tagging.
    let mut git = MockGit::new();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.parent_counts.insert("mc1".to_string(), 1);
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("release/1.1.0".to_string(), "main".to_string()),
        landed("old-head", "mc1"),
    );

    let err = finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("SQUASH"), "must name the actual type; got: {err}");
    assert!(err.contains("MERGE COMMIT"), "must name the expected type; got: {err}");
    assert!(err.contains("--accept-merge-type"), "must name the override; got: {err}");
    assert!(err.contains("revert the landing PR"), "must name the recovery; got: {err}");
    assert!(!git.calls().iter().any(|c| c.starts_with("create_tag_at")),
        "no tag may be cut on a wrongly completed landing; got: {:?}", git.calls());
}

#[test]
fn accept_merge_type_permits_a_squashed_landing() {
    let mut git = MockGit::new();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.parent_counts.insert("mc1".to_string(), 1);
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("release/1.1.0".to_string(), "main".to_string()),
        landed("old-head", "mc1"),
    );

    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, true).unwrap();

    assert!(git.calls().contains(&"create_tag_at:v1.1.0:chore: release 1.1.0:mc1".to_string()),
        "accepting the mistake lets the finish continue; got: {:?}", git.calls());
}

#[test]
fn protected_finish_completes_after_develop_merge() {
    let mut git = MockGit::new();
    git.current_branch = "release/1.1.0".to_string();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.existing_local_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "main".to_string()), landed("relsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "develop".to_string()), landed("relsha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "is_ancestor:mc1:origin/main",
        "commit_parent_count:mc1",
        "tag_exists:v1.1.0",
        "tag_commit_sha:v1.1.0",
        "is_ancestor:mc1:origin/main",
        "remote_tag_exists:v1.1.0",
        "branch_sha:release/1.1.0",
        "is_ancestor:mc2:origin/develop",
        "commit_parent_count:mc2",
        "branch_sha:release/1.1.0",
        "branch_sha:release/1.1.0",
        "list_branches_matching:finish/release-1.1.0-into-*",
        "current_branch",
        "is_linked_worktree",
        "worktree_of:main",
        "checkout:main",
        "local_branch_exists:release/1.1.0",
        "delete_branch_local:release/1.1.0",
        "remote_branch_exists:release/1.1.0",
        "delete_branch_remote:release/1.1.0",
    ]);
}

#[test]
fn protected_release_in_its_own_worktree_removes_the_worktree_last() {
    // The process stands in the release's linked worktree: removing it destroys
    // the process cwd, so every other git call — finish-branch cleanup
    // included — must have happened before.
    let mut git = MockGit::new();
    git.current_branch = "release/1.1.0".to_string();
    git.linked_worktree = true;
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.existing_local_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    git.branches_matching_by.insert("finish/release-1.1.0-into-*".to_string(), vec![
        "finish/release-1.1.0-into-develop".to_string(),
        "finish/release-1.1.0-into-main".to_string(),
    ]);
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "main".to_string()), landed("relsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "develop".to_string()), landed("relsha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"list_branches_matching:finish/release-1.1.0-into-*".to_string()),
        "finish branches are still cleaned; calls: {calls:?}");
    assert_eq!(calls.last().map(String::as_str), Some("remove_current_worktree"),
        "no git call may follow worktree removal — the process cwd is gone; calls: {calls:?}");
}

#[test]
fn protected_finish_tag_identity_mismatch_is_fatal() {
    let mut git = MockGit::new();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "other".to_string());
    // The main leg really did land (its merge commit reached origin/main) —
    // it is the *tag* that is wrong, sitting on some other commit.
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "main".to_string()), landed("relsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);

    let err = finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("points at other"), "got: {err}");
    assert!(err.contains("mc1"), "got: {err}");
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("push_tag")), "must not push a mismatched tag; calls: {calls:?}");
    assert!(!hosting.calls().iter().any(|c| c.contains(":develop")), "must not probe develop; calls: {:?}", hosting.calls());
}

#[test]
fn protected_finish_unlanded_main_pr_re_enters_the_rc_gate() {
    // A merged PR whose merge commit never reached the mainline is not a
    // landing: no `ancestors` entry puts `mc1` in `origin/main`. The gate must
    // still guard the branch rather than treating the PR as proof.
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    git.rev_list_count_result = 1;
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "main".to_string()), landed("relsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);

    let err = finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("1 commit past"), "an unlanded main PR must re-enter the RC gate; got: {err}");
    assert!(err.contains("gflow bump"), "got: {err}");
}

#[test]
fn protected_finish_keeps_branch_when_configured() {
    let mut git = MockGit::new();
    git.current_branch = "release/1.1.0".to_string();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "main".to_string()), landed("relsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "develop".to_string()), landed("relsha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_release(&git, &hosting, &protected_cfg(true), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c == "checkout:main"), "keep must skip delete_source_branch's checkout; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("delete_branch_")), "keep must skip deletion; calls: {calls:?}");
}

#[test]
fn protected_finish_reports_commits_that_will_miss_the_release() {
    // The main leg landed, so the tag is cut at its merge commit and the leg is
    // never re-opened — but commits pushed after that merge are in neither the
    // tag nor the mainline. gflow cannot put them there (the tag is published),
    // so it counts them and says so instead of shipping in silence.
    let mut git = MockGit::new();
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    git.branch_shas.insert("release/1.1.0".to_string(), "moved-tip".to_string());
    git.rev_list_counts.insert(("old-head".to_string(), "release/1.1.0".to_string()), 1);
    git.existing_local_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "main".to_string()), landed("old-head", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "develop".to_string()), landed("moved-tip", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(
        calls.contains(&"rev_list_count:old-head:release/1.1.0".to_string()),
        "commits past the main landing must be counted so they can be reported; calls: {calls:?}"
    );
    // Reporting only — the release still completes and the branch is cleaned up.
    assert!(calls.contains(&"delete_branch_remote:release/1.1.0".to_string()), "calls: {calls:?}");
}

#[test]
fn protected_finish_stays_silent_when_the_branch_never_moved() {
    // The common case: nothing was pushed after the main landing, so there is
    // nothing to report and no count is taken.
    let mut git = MockGit::new();
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    git.branch_shas.insert("release/1.1.0".to_string(), "same-tip".to_string());
    git.existing_local_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "main".to_string()), landed("same-tip", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "develop".to_string()), landed("same-tip", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("rev_list_count")), "nothing moved: no count, no report; calls: {calls:?}");
}

#[test]
fn protected_finish_cleans_up_when_the_branch_moved_after_the_tag_landed() {
    // The main leg's PR record is stale (no fresh merged_pr_to hit), but the
    // clean tag's own commit is already contained in origin/main — proof the
    // leg landed at some point in the past, regardless of what the branch's
    // tip looks like now. Cleanup must proceed on that basis, without the RC
    // gate ever re-running.
    let mut git = MockGit::new();
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "old-tip-sha".to_string());
    git.ancestors.insert(("old-tip-sha".to_string(), "origin/main".to_string()));
    git.branch_shas.insert("release/1.1.0".to_string(), "new-tip-sha".to_string());
    git.existing_local_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "develop".to_string()), landed("new-tip-sha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "tag_exists:v1.1.0",
        "tag_commit_sha:v1.1.0",
        "is_ancestor:old-tip-sha:origin/main",
        "remote_tag_exists:v1.1.0",
        "push_tag:v1.1.0",
        "is_ancestor:mc2:origin/develop",
        "commit_parent_count:mc2",
        "branch_sha:release/1.1.0",
        "branch_sha:release/1.1.0",
        "list_branches_matching:finish/release-1.1.0-into-*",
        "current_branch",
        "local_branch_exists:release/1.1.0",
        "delete_branch_local:release/1.1.0",
        "remote_branch_exists:release/1.1.0",
        "delete_branch_remote:release/1.1.0",
    ]);
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:release/1.1.0:main",
        "merged_pr_to:finish/release-1.1.0-into-main:main",
        "merged_pr_to:release/1.1.0:main",
        "open_pr_to:release/1.1.0:develop",
        "merged_pr_to:finish/release-1.1.0-into-develop:develop",
        "merged_pr_to:release/1.1.0:develop",
    ]);
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("rev_list_count")), "RC gate must not run once the tag has landed; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("tags_on_branch")), "RC gate must not run once the tag has landed; calls: {calls:?}");
}

#[test]
fn protected_finish_keeps_the_branch_when_its_tip_landed_nowhere() {
    // Same landed world as the test above, except the branch's current tip is
    // in neither the tag's commit nor the develop landing — commits that never
    // went anywhere. The strict develop-leg check re-opens the leg with a
    // refreshed finish branch instead of completing, so nothing is deleted and
    // the commits reach develop through a fresh PR.
    let mut git = MockGit::new();
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "old-tip-sha".to_string());
    git.ancestors.insert(("old-tip-sha".to_string(), "origin/main".to_string()));
    git.branch_shas.insert("release/1.1.0".to_string(), "unrelated-tip-sha".to_string());
    git.existing_local_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("release/1.1.0".to_string(), "develop".to_string()), landed("new-tip-sha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    let result = finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false);
    assert!(result.is_ok(), "re-opening the leg must not fail the run; got: {result:?}");

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("delete_branch_local")), "unlanded commits must not be deleted; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("delete_branch_remote")), "unlanded commits must not be deleted; calls: {calls:?}");
    let hosting_calls = hosting.calls();
    assert_eq!(hosting_calls[hosting_calls.len() - 3..], [
        "create_or_get_pr:finish/release-1.1.0-into-develop:develop:chore: merge release 1.1.0 into develop:empty-body".to_string(),
        "copy_text:chore: merge release 1.1.0 into develop\nhttps://github.com/org/repo/pull/1".to_string(),
        "open_url:https://github.com/org/repo/pull/1".to_string(),
    ], "the develop leg must re-open with a finish-branch PR and put it in the browser");
}

// --- Reconcile edges (exercised through run 1's main-PR step) ---

#[test]
fn protected_finish_pushes_when_remote_branch_missing_before_opening_pr() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "tag_exists:v1.1.0",
        "tags_on_branch:release/1.1.0",
        "rev_list_count:v1.1.0-rc.1:release/1.1.0",
        "remote_branch_exists:release/1.1.0",
        "push:release/1.1.0",
        "remote_branch_exists:finish/release-1.1.0-into-main",
        "local_branch_exists:finish/release-1.1.0-into-main",
        "create_branch_no_checkout:finish/release-1.1.0-into-main:release/1.1.0",
        "is_ancestor:release/1.1.0:finish/release-1.1.0-into-main",
        "is_ancestor:origin/main:finish/release-1.1.0-into-main",
        "current_branch",
        "checkout:finish/release-1.1.0-into-main",
        "merge:origin/main:chore: merge main into finish/release-1.1.0-into-main",
        "checkout:develop",
        "push:finish/release-1.1.0-into-main",
    ]);
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:release/1.1.0:main",
        "merged_pr_to:finish/release-1.1.0-into-main:main",
        "merged_pr_to:release/1.1.0:main",
        "create_or_get_pr:finish/release-1.1.0-into-main:main:chore: merge release 1.1.0 into main:empty-body",
        "copy_text:chore: merge release 1.1.0 into main\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

#[test]
fn protected_finish_pushes_when_local_ahead_of_origin() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("origin/release/1.1.0".to_string(), "release/1.1.0".to_string()));

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "tag_exists:v1.1.0",
        "tags_on_branch:release/1.1.0",
        "rev_list_count:v1.1.0-rc.1:release/1.1.0",
        "remote_branch_exists:release/1.1.0",
        "is_pushed:release/1.1.0",
        "is_ancestor:origin/release/1.1.0:release/1.1.0",
        "push:release/1.1.0",
        "remote_branch_exists:finish/release-1.1.0-into-main",
        "local_branch_exists:finish/release-1.1.0-into-main",
        "create_branch_no_checkout:finish/release-1.1.0-into-main:release/1.1.0",
        "is_ancestor:release/1.1.0:finish/release-1.1.0-into-main",
        "is_ancestor:origin/main:finish/release-1.1.0-into-main",
        "current_branch",
        "checkout:finish/release-1.1.0-into-main",
        "merge:origin/main:chore: merge main into finish/release-1.1.0-into-main",
        "checkout:develop",
        "push:finish/release-1.1.0-into-main",
    ]);
}

#[test]
fn protected_finish_errors_when_local_behind_origin() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("release/1.1.0".to_string(), "origin/release/1.1.0".to_string()));

    let hosting = MockHosting::new();
    let err = finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("is behind origin/release/1.1.0"), "got: {err}");
    assert!(err.contains("git pull --ff-only"), "got: {err}");
}

#[test]
fn protected_finish_errors_when_local_and_origin_diverged() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.1".to_string()];
    git.existing_remote_branches.insert("release/1.1.0".to_string());

    let hosting = MockHosting::new();
    let err = finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("have diverged"), "got: {err}");
    assert!(err.contains("git pull --rebase"), "got: {err}");
}

#[test]
fn finish_release_merges_into_main_in_place_when_main_lives_in_another_worktree() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1", "v1.1.0-rc.2"]);
    git.rev_list_count_result = 0;
    git.current_branch = "release/1.1.0".to_string();
    git.worktrees.insert("main".to_string(), std::path::PathBuf::from("/repos/beans-api-main"));

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &RepoConfig::default(), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    let before_tag: Vec<&String> = calls.iter().take_while(|c| *c != "tag_exists:v1.1.0").collect();
    assert!(!before_tag.contains(&&"checkout:main".to_string()), "the main leg must not check out a branch held by another worktree; calls: {calls:?}");
    let main_leg: Vec<&String> = calls.iter().skip_while(|c| *c != "worktree_of:main").take(4).collect();
    assert_eq!(main_leg, vec![
        "worktree_of:main",
        "is_working_tree_clean_at:/repos/beans-api-main",
        "ff_merge_at:/repos/beans-api-main:origin/main",
        "merge_at:/repos/beans-api-main:release/1.1.0:chore: merge release 1.1.0 into main",
    ]);
}

#[test]
fn finish_release_refuses_a_dirty_main_worktree_before_touching_it() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1", "v1.1.0-rc.2"]);
    git.rev_list_count_result = 0;
    git.current_branch = "release/1.1.0".to_string();
    git.worktrees.insert("main".to_string(), std::path::PathBuf::from("/repos/beans-api-main"));
    git.working_tree_clean = false;

    let err = finish_release(&git, &MockHosting::new(), &RepoConfig::default(), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("/repos/beans-api-main"), "got: {err}");
    assert!(err.contains("release/1.1.0"), "must carry the resume hint naming the source branch; got: {err}");
    assert!(!git.calls().iter().any(|c| c.starts_with("merge_at") || c.starts_with("ff_merge_at")), "calls: {:?}", git.calls());
}

#[test]
fn finish_release_cleanup_detaches_when_main_is_held_by_another_worktree() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1"]);
    git.rev_list_count_result = 0;
    git.current_branch = "release/1.1.0".to_string();
    git.ancestors.insert(("release/1.1.0".to_string(), "main".to_string()));
    git.ancestors.insert(("release/1.1.0".to_string(), "develop".to_string()));
    git.existing_tags.insert("v1.1.0".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    git.worktrees.insert("main".to_string(), std::path::PathBuf::from("/repos/beans-api-main"));

    finish_release(&git, &MockHosting::new(), &RepoConfig::default(), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.contains(&"checkout:main".to_string()), "calls: {calls:?}");
    let cleanup: Vec<&String> = calls.iter().skip_while(|c| *c != "current_branch").take(4).collect();
    assert_eq!(cleanup, vec!["current_branch", "is_linked_worktree", "worktree_of:main", "detach_head"]);
}

#[test]
fn finish_release_in_its_own_worktree_removes_the_worktree_last() {
    let mut git = fresh_release_mock(1, 1, &["v1.1.0-rc.1"]);
    git.rev_list_count_result = 0;
    git.current_branch = "release/1.1.0".to_string();
    git.linked_worktree = true;

    finish_release(&git, &MockHosting::new(), &RepoConfig::default(), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert_eq!(calls.last().map(String::as_str), Some("remove_current_worktree"), "calls: {calls:?}");
    let detach = calls.iter().position(|c| c == "detach_head").expect("detach before delete");
    let del = calls.iter().position(|c| c == "delete_branch_local:release/1.1.0").unwrap();
    assert!(detach < del, "calls: {calls:?}");
}

// --- Protected landings via finish/* branches ---

#[test]
fn protected_release_opens_main_pr_from_the_finish_branch() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.2".to_string()];
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "tag_exists:v1.1.0",
        "tags_on_branch:release/1.1.0",
        "rev_list_count:v1.1.0-rc.2:release/1.1.0",
        "remote_branch_exists:release/1.1.0",
        "is_pushed:release/1.1.0",
        "remote_branch_exists:finish/release-1.1.0-into-main",
        "local_branch_exists:finish/release-1.1.0-into-main",
        "create_branch_no_checkout:finish/release-1.1.0-into-main:release/1.1.0",
        "is_ancestor:release/1.1.0:finish/release-1.1.0-into-main",
        "is_ancestor:origin/main:finish/release-1.1.0-into-main",
        "current_branch",
        "checkout:finish/release-1.1.0-into-main",
        "merge:origin/main:chore: merge main into finish/release-1.1.0-into-main",
        "checkout:develop",
        "push:finish/release-1.1.0-into-main",
    ]);
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:release/1.1.0:main",
        "merged_pr_to:finish/release-1.1.0-into-main:main",
        "merged_pr_to:release/1.1.0:main",
        "create_or_get_pr:finish/release-1.1.0-into-main:main:chore: merge release 1.1.0 into main:empty-body",
        "copy_text:chore: merge release 1.1.0 into main\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c == "checkout:main"), "must never land into the target locally; calls: {calls:?}");
    assert!(calls.iter().filter(|c| c.starts_with("merge:")).all(|c| c.ends_with("into finish/release-1.1.0-into-main")),
        "merges may only append to the finish branch; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("create_tag")), "must not tag before main lands; calls: {calls:?}");
}

#[test]
fn protected_release_merges_the_target_into_the_fresh_finish_branch() {
    // A landing PR is born mergeable: the fresh finish branch gets the target
    // merged in before it is pushed and before the PR opens.
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.2".to_string()];
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    let merge = calls.iter()
        .position(|c| c == "merge:origin/main:chore: merge main into finish/release-1.1.0-into-main")
        .unwrap_or_else(|| panic!("target merge missing; calls: {calls:?}"));
    let push = calls.iter().position(|c| c == "push:finish/release-1.1.0-into-main").unwrap();
    assert!(merge < push, "target merged before the finish branch is published; calls: {calls:?}");
    assert!(hosting.calls().iter().any(|c| c.starts_with("create_or_get_pr:finish/release-1.1.0-into-main")),
        "hosting calls: {:?}", hosting.calls());
}

#[test]
fn protected_release_conflicted_target_merge_stops_before_opening_the_pr() {
    // The conflict surfaces locally, mid-run: the tree is left mid-merge on the
    // finish branch with recovery steps, nothing is pushed, no PR opens.
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.2".to_string()];
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.fail_nth_merge = Some(1);

    let hosting = MockHosting::new();
    let err = finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("finish/release-1.1.0-into-main") && err.contains("git merge --abort"), "got: {err}");
    assert!(err.contains("git switch release/1.1.0"), "recovery must name the way back to the source; got: {err}");
    assert!(!hosting.calls().iter().any(|c| c.starts_with("create_or_get_pr")),
        "no PR while the merge is unresolved; hosting calls: {:?}", hosting.calls());
    assert!(!git.calls().contains(&"push:finish/release-1.1.0-into-main".to_string()),
        "an unresolved merge must not be pushed; calls: {:?}", git.calls());
}

#[test]
fn protected_release_rerun_merges_a_moved_target_into_the_open_finish_branch() {
    // The PR is open and the source is unchanged, but the target moved since —
    // a re-run merges the target back into the finish branch (healing a PR the
    // platform now flags as conflicted) without re-refreshing the source.
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.2".to_string()];
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("finish/release-1.1.0-into-main".to_string());
    git.ancestors.insert(("release/1.1.0".to_string(), "origin/finish/release-1.1.0-into-main".to_string()));
    git.ancestors.insert(("release/1.1.0".to_string(), "finish/release-1.1.0-into-main".to_string()));
    git.ancestors.insert(("origin/finish/release-1.1.0-into-main".to_string(), "finish/release-1.1.0-into-main".to_string()));

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"merge:origin/main:chore: merge main into finish/release-1.1.0-into-main".to_string()),
        "a moved target is merged back in on re-run; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("merge:release/1.1.0:")),
        "an unchanged source needs no refresh; calls: {calls:?}");
    assert!(calls.contains(&"push:finish/release-1.1.0-into-main".to_string()), "calls: {calls:?}");
}

#[test]
fn protected_release_untouched_finish_branch_is_left_alone() {
    // origin/finish already holds both the source tip and the target tip:
    // nothing to refresh, nothing to merge, nothing to push.
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.2".to_string()];
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("finish/release-1.1.0-into-main".to_string());
    git.ancestors.insert(("release/1.1.0".to_string(), "origin/finish/release-1.1.0-into-main".to_string()));
    git.ancestors.insert(("origin/main".to_string(), "origin/finish/release-1.1.0-into-main".to_string()));

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("merge:") || c.starts_with("checkout:finish/")
            || c == "push:finish/release-1.1.0-into-main"),
        "an up-to-date finish branch is untouched; calls: {calls:?}");
}

#[test]
fn protected_release_refuses_an_open_legacy_pr() {
    let mut git = MockGit::new();
    git.tags_on_branch = vec!["v1.1.0-rc.2".to_string()];
    let mut hosting = MockHosting::new();
    hosting.open_prs_to.insert(("release/1.1.0".to_string(), "main".to_string()), "https://github.com/o/r/pull/9".to_string());

    let err = finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("pull/9") && err.contains("close/abandon"), "got: {err}");
    assert!(git.calls().is_empty(), "must refuse before any git call; calls: {:?}", git.calls());
    assert_eq!(hosting.calls(), vec!["open_pr_to:release/1.1.0:main"]);
}

#[test]
fn protected_release_tags_the_finish_prs_merge_commit() {
    let mut git = MockGit::new();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("mcF".to_string(), "origin/main".to_string()));
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("finish/release-1.1.0-into-main".to_string(), "main".to_string()),
        landed("finhead", "mcF"),
    );
    git.parent_counts.insert("mcF".to_string(), 2);

    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"create_tag_at:v1.1.0:chore: release 1.1.0:mcF".to_string()),
        "the tag is cut at the finish PR's merge commit; calls: {calls:?}");
    let hosting_calls = hosting.calls();
    assert_eq!(hosting_calls[hosting_calls.len() - 3..], [
        "create_or_get_pr:finish/release-1.1.0-into-develop:develop:chore: merge release 1.1.0 into develop:empty-body".to_string(),
        "copy_text:chore: merge release 1.1.0 into develop\nhttps://github.com/org/repo/pull/1".to_string(),
        "open_url:https://github.com/org/repo/pull/1".to_string(),
    ]);
}

#[test]
fn develop_leg_reopens_when_release_gained_commits_after_a_landed_sync() {
    let mut git = MockGit::new();
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.branch_shas.insert("release/1.1.0".to_string(), "newsha".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("finish/release-1.1.0-into-develop".to_string());
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("finish/release-1.1.0-into-develop".to_string(), "develop".to_string()),
        landed("oldsha", "mc2"),
    );
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"merge:release/1.1.0:chore: refresh finish/release-1.1.0-into-develop with release/1.1.0".to_string()),
        "the finish branch must be refreshed with the new commits; calls: {calls:?}");
    assert!(!calls.contains(&"delete_branch_local:release/1.1.0".to_string()),
        "the finish must NOT complete while develop misses commits; calls: {calls:?}");
    let hosting_calls = hosting.calls();
    assert_eq!(hosting_calls[hosting_calls.len() - 3..], [
        "create_or_get_pr:finish/release-1.1.0-into-develop:develop:chore: merge release 1.1.0 into develop:empty-body".to_string(),
        "copy_text:chore: merge release 1.1.0 into develop\nhttps://github.com/org/repo/pull/1".to_string(),
        "open_url:https://github.com/org/repo/pull/1".to_string(),
    ]);
}

#[test]
fn ensure_reuses_a_remote_finish_that_gained_resolution_commits() {
    let mut git = MockGit::new();
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("finish/release-1.1.0-into-develop".to_string());
    git.ancestors.insert(("release/1.1.0".to_string(), "origin/finish/release-1.1.0-into-develop".to_string()));
    // A resolution commit is a merge of the target: origin/finish carries develop too.
    git.ancestors.insert(("origin/develop".to_string(), "origin/finish/release-1.1.0-into-develop".to_string()));

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("push:finish/") || c.starts_with("create_branch_no_checkout:finish/") || c.starts_with("checkout:finish/")),
        "a remote finish that already contains the tip is reused untouched; calls: {calls:?}");
    let hosting_calls = hosting.calls();
    assert_eq!(hosting_calls[hosting_calls.len() - 3..], [
        "create_or_get_pr:finish/release-1.1.0-into-develop:develop:chore: merge release 1.1.0 into develop:empty-body".to_string(),
        "copy_text:chore: merge release 1.1.0 into develop\nhttps://github.com/org/repo/pull/1".to_string(),
        "open_url:https://github.com/org/repo/pull/1".to_string(),
    ]);
}

#[test]
fn leg_skips_without_a_pr_when_target_already_contains_the_finish() {
    let mut git = MockGit::new();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_local_branches.insert("release/1.1.0".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("release/1.1.0".to_string(), "origin/develop".to_string()));
    git.branches_matching_by.insert("finish/release-1.1.0-into-*".to_string(),
        vec!["finish/release-1.1.0-into-main".to_string()]);
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("finish/release-1.1.0-into-main".to_string(), "main".to_string()),
        landed("relsha", "mc1"),
    );
    git.parent_counts.insert("mc1".to_string(), 2);

    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    assert!(!hosting.calls().iter().any(|c| c.starts_with("create_or_get_pr")),
        "develop already contains the release: no PR; calls: {:?}", hosting.calls());
    let calls = git.calls();
    assert!(calls.contains(&"delete_branch_local:release/1.1.0".to_string()), "calls: {calls:?}");
    assert!(calls.contains(&"list_branches_matching:finish/release-1.1.0-into-*".to_string()), "calls: {calls:?}");
    let guard_read = calls.iter().position(|c| c == "branch_sha:release/1.1.0").unwrap();
    let finish_delete = calls.iter().position(|c| c == "local_branch_exists:finish/release-1.1.0-into-main").unwrap();
    assert!(guard_read < finish_delete, "guard must run before finish branches are deleted; calls: {calls:?}");
}

#[test]
fn protected_sync_lands_via_the_develop_finish_branch() {
    let mut git = MockGit::new();
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());

    let hosting = MockHosting::new();
    sync_with_develop(&git, &hosting, &protected_cfg(false), 1, 1, None, false).unwrap();

    assert_eq!(hosting.calls(), vec![
        "open_pr_to:release/1.1.0:develop",
        "merged_pr_to:finish/release-1.1.0-into-develop:develop",
        "merged_pr_to:release/1.1.0:develop",
        "create_or_get_pr:finish/release-1.1.0-into-develop:develop:chore: sync release 1.1.0 with develop:empty-body",
        "copy_text:chore: sync release 1.1.0 with develop\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

#[test]
fn ensure_refreshes_a_local_leftover_finish_branch_without_a_remote() {
    // A crashed earlier run left a local finish branch that was never pushed;
    // the release has moved since. The leftover is refreshed by merging the
    // release in (no origin to ff-sync from), then pushed.
    let mut git = MockGit::new();
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.existing_local_branches.insert("finish/release-1.1.0-into-develop".to_string());

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    let tail: Vec<&String> = calls.iter().skip_while(|c| *c != "checkout:finish/release-1.1.0-into-develop").collect();
    assert_eq!(tail, vec![
        "checkout:finish/release-1.1.0-into-develop",
        "merge:release/1.1.0:chore: refresh finish/release-1.1.0-into-develop with release/1.1.0",
        "checkout:develop",
        "is_ancestor:origin/develop:finish/release-1.1.0-into-develop",
        "current_branch",
        "checkout:finish/release-1.1.0-into-develop",
        "merge:origin/develop:chore: merge develop into finish/release-1.1.0-into-develop",
        "checkout:develop",
        "push:finish/release-1.1.0-into-develop",
    ], "no origin to ff-sync from; calls: {calls:?}");
    assert!(!calls.contains(&"ff_merge:origin/finish/release-1.1.0-into-develop".to_string()), "calls: {calls:?}");
}

#[test]
fn ensure_pushes_a_local_finish_that_contains_the_tip_but_never_reached_origin() {
    // Crashed between refresh and push: the local finish already contains the
    // tip and origin is strictly behind — nothing to refresh, just push.
    let mut git = MockGit::new();
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.0".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.existing_local_branches.insert("finish/release-1.1.0-into-develop".to_string());
    git.existing_remote_branches.insert("finish/release-1.1.0-into-develop".to_string());
    git.ancestors.insert(("release/1.1.0".to_string(), "finish/release-1.1.0-into-develop".to_string()));
    git.ancestors.insert(("origin/finish/release-1.1.0-into-develop".to_string(), "finish/release-1.1.0-into-develop".to_string()));
    // The crash happened after both merges: the target is already in too.
    git.ancestors.insert(("origin/develop".to_string(), "finish/release-1.1.0-into-develop".to_string()));

    let hosting = MockHosting::new();
    finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("checkout:finish/")), "nothing to refresh; calls: {calls:?}");
    assert!(calls.contains(&"push:finish/release-1.1.0-into-develop".to_string()), "the push must be retried; calls: {calls:?}");
}

#[test]
fn bump_protected_merged_pr_with_no_prior_tag_cuts_the_first_rc_at_the_merge_commit() {
    // A branch whose very first RC went out through a chore PR has no tag yet
    // to compare against — nothing is "consumed", so the deferred tag is cut
    // at that PR's merge commit.
    let mut git = MockGit::new();
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("release-chore/1.1.0/set-version".to_string(), "release/1.1.0".to_string()),
        landed("chore-head", "merge-commit-sha"),
    );
    let script = MockVersionScript::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    bump_version(&git, &hosting, Some(&script), &cfg, 1, 1).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"create_tag_at:v1.1.0-rc.1:chore: bump version to v1.1.0-rc.1:merge-commit-sha".to_string()),
        "the first RC must be cut at the PR merge commit; calls: {calls:?}");
    assert!(calls.contains(&"push_tag:v1.1.0-rc.1".to_string()), "calls: {calls:?}");
    assert!(script.calls().is_empty(), "consuming a landed PR never re-runs the script");
}

#[test]
fn protected_release_refuses_a_clean_tag_pointing_at_the_wrong_commit() {
    // The clean tag exists but points at neither the mainline nor the main
    // PR's merge commit — a stale or hand-created tag. Silently skipping it
    // would ship the wrong commit as the release; gflow stops and names it.
    let mut git = MockGit::new();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.pushed_branches.insert("release/1.1.0".to_string());
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "somewhere-else".to_string());
    git.ancestors.insert(("mcF".to_string(), "origin/main".to_string()));
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("finish/release-1.1.0-into-main".to_string(), "main".to_string()),
        landed("finhead", "mcF"),
    );
    git.parent_counts.insert("mcF".to_string(), 2);

    let err = finish_release(&git, &hosting, &protected_cfg(false), 1, 1, "main", None, false).unwrap_err();

    assert!(err.contains("points at somewhere-else"), "got: {err}");
    assert!(err.contains("Move or delete the tag"), "the error must name the fix; got: {err}");
}

// --- Guard arms unreachable through the strict flows, pinned directly ---

#[test]
fn tag_at_if_missing_skips_a_tag_already_at_the_right_commit() {
    // Doc contract: the equal-commit arm is unreachable through current
    // callers and stays as a guard for future ones — pinned here directly.
    use gflow::flows::tag_at_if_missing;
    let mut git = MockGit::new();
    git.existing_tags.insert("v1.1.0".to_string());
    git.tag_commits.insert("v1.1.0".to_string(), "mcF".to_string());

    tag_at_if_missing(&git, "v1.1.0", "chore: release 1.1.0", "mcF").unwrap();

    assert!(!git.calls().iter().any(|c| c.starts_with("create_tag")),
        "an existing correct tag must never be re-cut; calls: {:?}", git.calls());
}

#[test]
fn tip_landed_nowhere_provable_is_false() {
    // Doc contract: nothing provable → false, so cleanup keeps the branch
    // rather than delete commits that may never have landed.
    use gflow::flows::tip_landed_somewhere;
    let mut git = MockGit::new();
    git.branch_shas.insert("release/1.1.0".to_string(), "tip".to_string());

    let landed = tip_landed_somewhere(&git, "release/1.1.0", &[], &["finish/release-1.1.0-into-main".to_string()]).unwrap();

    assert!(!landed);
}

#[test]
fn release_cleanup_with_unlanded_tip_keeps_the_branch() {
    // Last-resort guard behind the strict legs: completing with an unlanded
    // tip must warn and keep the branch, never delete commits.
    use gflow::flows::finish_release::finish_release_cleanup;
    use gflow::version::SemVer;
    let git = MockGit::new();

    finish_release_cleanup(&git, &protected_cfg(false), "release/1.1.0", "main", &SemVer::new(1, 1, 0), false).unwrap();

    assert!(!git.calls().iter().any(|c| c.starts_with("delete_branch")),
        "an unlanded tip must never be deleted; calls: {:?}", git.calls());
}

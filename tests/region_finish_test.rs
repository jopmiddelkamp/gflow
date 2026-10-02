mod common;

use common::{MockGit, MockHosting, MockPrompter, MockVersionScript};
use gflow::flows::finish_hotfix::finish_hotfix;
use gflow::flows::finish_release::{bump_version, finish_release, sync_with_develop};
use gflow::flows::finish_work::{finish_release_fix, finish_work_branch};
use gflow::git::branch::BranchType;
use gflow::hosting::{LandedPr, MergedPr};
use gflow::repo_config::{BumpStrategy, Mode, RepoConfig};

fn assert_git_failure(result: Result<(), String>, git: &MockGit, call: &str) {
    let error = result.expect_err(call);
    assert!(
        error.contains(&format!("injected git failure: {call}")),
        "{error}"
    );
    assert_eq!(
        git.calls().last().map(String::as_str),
        Some(call),
        "calls continue after {error}"
    );
}

fn assert_hosting_failure(result: Result<(), String>, hosting: &MockHosting, call: &str) {
    let error = result.expect_err(call);
    assert!(
        error.contains(&format!("injected hosting failure: {call}")),
        "{error}"
    );
    assert_eq!(
        hosting.calls().last().map(String::as_str),
        Some(call),
        "calls continue after {error}"
    );
}

fn release_git() -> MockGit {
    let mut git = MockGit::new();
    git.current_branch = "release/1.1.0".into();
    git.tags_on_branch = vec!["v1.1.0-rc.1".into()];
    git.existing_local_branches.insert("release/1.1.0".into());
    git.existing_remote_branches.insert("release/1.1.0".into());
    git.pushed_branches.insert("release/1.1.0".into());
    git
}

fn protected_cfg(strategy: BumpStrategy) -> RepoConfig {
    RepoConfig {
        mode: Mode::Protected,
        bump_strategy: strategy,
        ..RepoConfig::default()
    }
}

fn landed() -> LandedPr {
    LandedPr {
        url: "https://example.com/pr/1".into(),
        head_sha: "headsha".into(),
        merge_commit_sha: "merged".into(),
    }
}

#[test]
fn free_bump_stops_when_git_cannot_validate_commit_or_publish() {
    for (strategy, call, occurrence) in [
        (BumpStrategy::Rc, "tags_on_branch:release/1.1.0", 1),
        (BumpStrategy::Patch, "tags_on_branch:release/1.1.0", 1),
        (BumpStrategy::Rc, "is_working_tree_clean", 1),
        (BumpStrategy::Rc, "is_working_tree_clean", 2),
        (BumpStrategy::Rc, "stage_all", 1),
        (BumpStrategy::Rc, "push:release/1.1.0", 1),
    ] {
        let mut git = release_git();
        git.working_tree_clean_seq.get_mut().extend([true, false]);
        git.fail_call = Some((call.into(), occurrence));
        let cfg = RepoConfig {
            bump_strategy: strategy,
            ..RepoConfig::default()
        };
        let result = bump_version(
            &git,
            &MockHosting::new(),
            Some(&MockVersionScript::new()),
            &cfg,
            1,
            1,
        );
        assert_git_failure(result, &git, call);
        assert!(!git.calls().iter().any(|c| c.starts_with("create_tag:")));
    }
    for call in [
        "create_tag:v1.1.0-rc.2:chore: bump version to v1.1.0-rc.2",
        "push_tag:v1.1.0-rc.2",
    ] {
        let mut git = release_git();
        git.fail_call = Some((call.into(), 1));
        assert_git_failure(
            bump_version(
                &git,
                &MockHosting::new(),
                None,
                &RepoConfig::default(),
                1,
                1,
            ),
            &git,
            call,
        );
    }
}

#[test]
fn protected_bump_stops_after_failed_merged_version_step() {
    for call in [
        "tags_on_branch:release/1.1.0",
        "tag_commit_sha:v1.1.0-rc.1",
        "create_tag_at:v1.1.0-rc.2:chore: bump version to v1.1.0-rc.2:merged",
        "push_tag:v1.1.0-rc.2",
        "local_branch_exists:release-chore/1.1.0/set-version",
    ] {
        let mut git = release_git();
        git.tag_commits
            .insert("v1.1.0-rc.1".into(), "previous".into());
        git.fail_call = Some((call.into(), 1));
        let mut hosting = MockHosting::new();
        hosting.merged_prs_to.insert(
            (
                "release-chore/1.1.0/set-version".into(),
                "release/1.1.0".into(),
            ),
            landed(),
        );
        let result = bump_version(&git, &hosting, None, &protected_cfg(BumpStrategy::Rc), 1, 1);
        assert_git_failure(result, &git, call);
        assert!(!hosting
            .calls()
            .iter()
            .any(|c| c.starts_with("create_or_get_pr:")));
    }
}

#[test]
fn protected_bump_stops_when_consumed_version_cleanup_fails() {
    let call = "local_branch_exists:release-chore/1.1.0/set-version";
    let mut git = release_git();
    git.tag_commits
        .insert("v1.1.0-rc.1".into(), "merged".into());
    git.fail_call = Some((call.into(), 1));
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        (
            "release-chore/1.1.0/set-version".into(),
            "release/1.1.0".into(),
        ),
        landed(),
    );
    assert_git_failure(
        bump_version(&git, &hosting, None, &protected_cfg(BumpStrategy::Rc), 1, 1),
        &git,
        call,
    );
    assert!(!git.calls().iter().any(|c| c.starts_with("create_tag")));
}

#[test]
fn protected_bump_stops_before_any_git_change_when_hosting_lookup_fails() {
    let call = "merged_pr_to:release-chore/1.1.0/set-version:release/1.1.0";
    let git = release_git();
    let mut hosting = MockHosting::new();
    hosting.fail_call = Some((call.into(), 1));
    assert_hosting_failure(
        bump_version(&git, &hosting, None, &protected_cfg(BumpStrategy::Rc), 1, 1),
        &hosting,
        call,
    );
    assert!(git.calls().is_empty());
}

#[test]
fn protected_bump_requires_readable_tags_for_direct_and_scripted_patch_bumps() {
    for script in [None, Some(MockVersionScript::new())] {
        let call = "tags_on_branch:release/1.1.0";
        let mut git = release_git();
        git.fail_call = Some((call.into(), 1));
        let result = bump_version(
            &git,
            &MockHosting::new(),
            script
                .as_ref()
                .map(|s| s as &dyn gflow::version_script::VersionScript),
            &protected_cfg(BumpStrategy::Patch),
            1,
            1,
        );
        assert_git_failure(result, &git, call);
    }
}

#[test]
fn protected_bump_stops_after_failed_scripted_version_step() {
    for call in [
        "remote_branch_exists:release-chore/1.1.0/set-version",
        "is_working_tree_clean",
        "local_branch_exists:release-chore/1.1.0/set-version",
        "delete_branch_local:release-chore/1.1.0/set-version",
        "create_branch:release-chore/1.1.0/set-version:release/1.1.0",
        "push:release-chore/1.1.0/set-version",
        "checkout:release/1.1.0",
    ] {
        let mut git = release_git();
        git.existing_local_branches
            .insert("release-chore/1.1.0/set-version".into());
        git.working_tree_clean_seq.get_mut().extend([true, false]);
        git.fail_call = Some((call.into(), 1));
        let result = bump_version(
            &git,
            &MockHosting::new(),
            Some(&MockVersionScript::new()),
            &protected_cfg(BumpStrategy::Rc),
            1,
            1,
        );
        assert_git_failure(result, &git, call);
        assert!(!git.calls().iter().any(|c| c.starts_with("create_tag")));
    }
}

#[test]
fn protected_bump_does_not_tag_when_noop_script_cleanup_fails() {
    for call in [
        "checkout:release/1.1.0",
        "delete_branch_local:release-chore/1.1.0/set-version",
        "tags_on_branch:release/1.1.0",
    ] {
        let mut git = release_git();
        git.fail_call = Some((call.into(), 1));
        let result = bump_version(
            &git,
            &MockHosting::new(),
            Some(&MockVersionScript::new()),
            &protected_cfg(BumpStrategy::Rc),
            1,
            1,
        );
        assert_git_failure(result, &git, call);
        assert!(!git.calls().iter().any(|c| c.starts_with("create_tag")));
    }
}

#[test]
fn protected_bump_does_not_tag_after_version_pr_creation_fails() {
    for reuse in [false, true] {
        let call = "create_or_get_pr:release-chore/1.1.0/set-version:release/1.1.0:chore: set version 1.1.0";
        let mut git = release_git();
        if reuse {
            git.existing_remote_branches
                .insert("release-chore/1.1.0/set-version".into());
        }
        git.working_tree_clean_seq.get_mut().extend([true, false]);
        let mut hosting = MockHosting::new();
        hosting.fail_call = Some((call.into(), 1));
        let result = bump_version(
            &git,
            &hosting,
            Some(&MockVersionScript::new()),
            &protected_cfg(BumpStrategy::Rc),
            1,
            1,
        );
        assert_hosting_failure(result, &hosting, call);
        assert!(!git
            .calls()
            .iter()
            .any(|c| c.starts_with("create_tag") || c.starts_with("checkout:")));
    }
}

#[test]
fn free_sync_stops_after_each_failed_git_step() {
    for call in [
        "current_branch",
        "checkout:develop",
        "ff_merge:origin/develop",
        "merge:release/1.1.0:chore: sync release 1.1.0 with develop",
        "push:develop",
        "checkout:release/1.1.0",
    ] {
        let mut git = release_git();
        git.fail_call = Some((call.into(), 1));
        assert_git_failure(
            sync_with_develop(
                &git,
                &MockHosting::new(),
                &RepoConfig::default(),
                1,
                1,
                None,
                false,
            ),
            &git,
            call,
        );
    }
}

#[test]
fn free_release_stops_after_each_failed_finish_step() {
    for strategy in [BumpStrategy::Rc, BumpStrategy::Patch] {
        let mut calls = vec![
            "tags_on_branch:release/1.1.0",
            "is_ancestor:release/1.1.0:main",
            "is_pushed:main",
            "remote_tag_exists:v1.1.0",
            "is_pushed:develop",
            "is_linked_worktree",
        ];
        if strategy == BumpStrategy::Rc {
            calls.extend([
                "rev_list_count:v1.1.0-rc.1:release/1.1.0",
                "tag_exists:v1.1.0",
            ]);
        }
        for call in calls {
            let mut git = release_git();
            if strategy == BumpStrategy::Patch {
                git.tags_on_branch = vec!["v1.1.0".into()];
                git.existing_tags.insert("v1.1.0".into());
            }
            git.fail_call = Some((call.into(), 1));
            let cfg = RepoConfig {
                bump_strategy: strategy,
                ..RepoConfig::default()
            };
            assert_git_failure(
                finish_release(&git, &MockHosting::new(), &cfg, 1, 1, "main", None, false),
                &git,
                call,
            );
        }
    }
}

#[test]
fn free_hotfix_stops_after_each_failed_finish_step() {
    for call in [
        "is_ancestor:hotfix/1.1.1:main",
        "tag_exists:v1.1.1",
        "is_pushed:main",
        "remote_tag_exists:v1.1.1",
        "is_pushed:develop",
        "list_branches_matching:release/*",
        "is_pushed:release/1.2.0",
        "is_linked_worktree",
    ] {
        let mut git = MockGit::new();
        git.current_branch = "hotfix/1.1.1".into();
        git.existing_local_branches.insert("hotfix/1.1.1".into());
        git.existing_remote_branches.insert("hotfix/1.1.1".into());
        git.branches_matching = vec!["release/1.2.0".into()];
        git.fail_call = Some((call.into(), 1));
        assert_git_failure(
            finish_hotfix(
                &git,
                &MockHosting::new(),
                &RepoConfig::default(),
                1,
                1,
                1,
                "main",
                None,
                false,
            ),
            &git,
            call,
        );
    }
}

#[test]
fn protected_release_does_not_publish_when_staging_checks_fail() {
    for strategy in [BumpStrategy::Rc, BumpStrategy::Patch] {
        let count_call = match strategy {
            BumpStrategy::Rc => "rev_list_count:v1.1.0-rc.1:release/1.1.0",
            BumpStrategy::Patch => "rev_list_count:v1.1.0:release/1.1.0",
        };
        for call in [
            "tags_on_branch:release/1.1.0",
            count_call,
            "remote_branch_exists:release/1.1.0",
        ] {
            let mut git = release_git();
            if strategy == BumpStrategy::Patch {
                git.tags_on_branch = vec!["v1.1.0".into()];
                git.existing_tags.insert("v1.1.0".into());
            }
            git.fail_call = Some((call.into(), 1));
            let hosting = MockHosting::new();
            assert_git_failure(
                finish_release(
                    &git,
                    &hosting,
                    &protected_cfg(strategy),
                    1,
                    1,
                    "main",
                    None,
                    false,
                ),
                &git,
                call,
            );
            assert!(!hosting
                .calls()
                .iter()
                .any(|c| c.starts_with("create_or_get_pr:")));
            assert!(!git.calls().iter().any(|c| c.starts_with("delete_branch")));
        }
    }
}

#[test]
fn protected_release_stops_after_failed_main_pr_creation() {
    for strategy in [BumpStrategy::Rc, BumpStrategy::Patch] {
        let mut git = release_git();
        if strategy == BumpStrategy::Patch {
            git.tags_on_branch = vec!["v1.1.0".into()];
            git.existing_tags.insert("v1.1.0".into());
        }
        let mut hosting = MockHosting::new();
        let call = "create_or_get_pr:finish/release-1.1.0-into-main:main:chore: merge release 1.1.0 into main:empty-body";
        hosting.fail_call = Some((call.into(), 1));
        assert_hosting_failure(
            finish_release(
                &git,
                &hosting,
                &protected_cfg(strategy),
                1,
                1,
                "main",
                None,
                false,
            ),
            &hosting,
            call,
        );
        assert!(!git
            .calls()
            .iter()
            .any(|c| c.starts_with("create_tag") || c.starts_with("delete_branch")));
    }
}

fn landed_release() -> (MockGit, MockHosting) {
    let mut git = release_git();
    let mut hosting = MockHosting::new();
    for target in ["main", "develop"] {
        hosting
            .merged_prs_to
            .insert(("release/1.1.0".into(), target.into()), landed());
        git.ancestors
            .insert(("merged".into(), format!("origin/{target}")));
    }
    git.parent_counts.insert("merged".into(), 2);
    (git, hosting)
}

#[test]
fn protected_release_stops_after_failed_landed_finish_step() {
    for strategy in [BumpStrategy::Rc, BumpStrategy::Patch] {
        let mut failures = vec![
            ("remote_tag_exists:v1.1.0", 1),
            ("branch_sha:release/1.1.0", 1),
            ("branch_sha:release/1.1.0", 2),
            ("branch_sha:release/1.1.0", 3),
            ("list_branches_matching:finish/release-1.1.0-into-*", 1),
        ];
        if strategy == BumpStrategy::Rc {
            failures.push(("tag_exists:v1.1.0", 1));
        } else {
            failures.push(("tags_on_branch:release/1.1.0", 1));
        }
        for (call, occurrence) in failures {
            let (mut git, hosting) = landed_release();
            if strategy == BumpStrategy::Patch {
                git.tags_on_branch = vec!["v1.1.0".into()];
                git.existing_tags.insert("v1.1.0".into());
            }
            git.fail_call = Some((call.into(), occurrence));
            assert_git_failure(
                finish_release(
                    &git,
                    &hosting,
                    &protected_cfg(strategy),
                    1,
                    1,
                    "main",
                    None,
                    false,
                ),
                &git,
                call,
            );
            assert!(!git.calls().iter().any(|c| c.starts_with("delete_branch")));
        }
    }
}

#[test]
fn protected_release_stops_if_existing_tag_cannot_be_verified_or_pushed() {
    for call in [
        "tag_commit_sha:v1.1.0",
        "is_ancestor:merged:origin/main",
        "remote_tag_exists:v1.1.0",
    ] {
        let mut git = release_git();
        git.existing_tags.insert("v1.1.0".into());
        git.tag_commits.insert("v1.1.0".into(), "merged".into());
        git.ancestors
            .insert(("merged".into(), "origin/main".into()));
        git.fail_call = Some((call.into(), 1));
        assert_git_failure(
            finish_release(
                &git,
                &MockHosting::new(),
                &protected_cfg(BumpStrategy::Rc),
                1,
                1,
                "main",
                None,
                false,
            ),
            &git,
            call,
        );
        assert!(!git.calls().iter().any(|c| c.starts_with("delete_branch")));
    }
}

fn landed_hotfix() -> (MockGit, MockHosting) {
    let mut git = MockGit::new();
    git.current_branch = "hotfix/1.1.1".into();
    git.existing_local_branches.insert("hotfix/1.1.1".into());
    git.existing_remote_branches.insert("hotfix/1.1.1".into());
    git.branches_matching = vec!["release/1.2.0".into()];
    let mut hosting = MockHosting::new();
    for target in ["main", "develop", "release/1.2.0"] {
        hosting
            .merged_prs_to
            .insert(("hotfix/1.1.1".into(), target.into()), landed());
        git.ancestors
            .insert(("merged".into(), format!("origin/{target}")));
    }
    git.parent_counts.insert("merged".into(), 2);
    (git, hosting)
}

#[test]
fn protected_hotfix_stops_after_failed_landed_finish_step() {
    for (call, occurrence) in [
        ("is_ancestor:merged:origin/main", 1),
        ("tag_exists:v1.1.1", 1),
        ("create_tag_at:v1.1.1:chore: hotfix 1.1.1:merged", 1),
        ("remote_tag_exists:v1.1.1", 1),
        ("branch_sha:hotfix/1.1.1", 1),
        ("branch_sha:hotfix/1.1.1", 2),
        ("list_branches_matching:release/*", 1),
        ("branch_sha:hotfix/1.1.1", 3),
        ("branch_sha:hotfix/1.1.1", 4),
        ("list_branches_matching:finish/hotfix-1.1.1-into-*", 1),
    ] {
        let (mut git, hosting) = landed_hotfix();
        git.fail_call = Some((call.into(), occurrence));
        assert_git_failure(
            finish_hotfix(
                &git,
                &hosting,
                &protected_cfg(BumpStrategy::Rc),
                1,
                1,
                1,
                "main",
                None,
                false,
            ),
            &git,
            call,
        );
        assert!(!git.calls().iter().any(|c| c.starts_with("delete_branch")));
    }
}

#[test]
fn protected_hotfix_stops_if_existing_tag_cannot_be_verified_or_pushed() {
    for call in [
        "tag_commit_sha:v1.1.1",
        "is_ancestor:merged:origin/main",
        "remote_tag_exists:v1.1.1",
    ] {
        let mut git = MockGit::new();
        git.existing_tags.insert("v1.1.1".into());
        git.tag_commits.insert("v1.1.1".into(), "merged".into());
        git.ancestors
            .insert(("merged".into(), "origin/main".into()));
        git.fail_call = Some((call.into(), 1));
        assert_git_failure(
            finish_hotfix(
                &git,
                &MockHosting::new(),
                &protected_cfg(BumpStrategy::Rc),
                1,
                1,
                1,
                "main",
                None,
                false,
            ),
            &git,
            call,
        );
        assert!(!git.calls().iter().any(|c| c.starts_with("delete_branch")));
    }
}

#[test]
fn protected_hotfix_stops_before_publishing_when_main_landing_preparation_fails() {
    let mut git = MockGit::new();
    let call = "remote_branch_exists:hotfix/1.1.1";
    git.fail_call = Some((call.into(), 1));
    let hosting = MockHosting::new();
    assert_git_failure(
        finish_hotfix(
            &git,
            &hosting,
            &protected_cfg(BumpStrategy::Rc),
            1,
            1,
            1,
            "main",
            None,
            false,
        ),
        &git,
        call,
    );
    assert!(!hosting
        .calls()
        .iter()
        .any(|c| c.starts_with("create_or_get_pr:")));
}

#[test]
fn protected_hotfix_stops_after_hosting_errors_before_opening_later_legs() {
    for call in [
        "open_pr_to:hotfix/1.1.1:main",
        "merged_pr_to:finish/hotfix-1.1.1-into-main:main",
        "create_or_get_pr:finish/hotfix-1.1.1-into-main:main:chore: merge hotfix 1.1.1 into main:empty-body",
    ] {
        let git = MockGit::new();
        let mut hosting = MockHosting::new();
        hosting.fail_call = Some((call.into(), 1));
        assert_hosting_failure(finish_hotfix(&git, &hosting, &protected_cfg(BumpStrategy::Rc), 1, 1, 1, "main", None, false), &hosting, call);
        assert!(!git.calls().iter().any(|c| c.starts_with("create_tag") || c.starts_with("delete_branch")));
        assert!(!hosting.calls().iter().any(|c| c.contains(":develop")));
    }
}

fn work_git() -> MockGit {
    let mut git = MockGit::new();
    git.current_branch = "feature/example".into();
    git.existing_local_branches.insert("feature/example".into());
    git.existing_remote_branches
        .insert("feature/example".into());
    git.existing_remote_branches.insert("develop".into());
    git.parent_counts.insert("merged".into(), 1);
    git
}

fn completed_work() -> MockHosting {
    let mut hosting = MockHosting::new();
    hosting.merged_pr = Some(MergedPr {
        url: "https://example.com/pr/1".into(),
        head_sha: "headsha".into(),
        merge_commit_sha: "merged".into(),
        base: "develop".into(),
    });
    hosting
}

#[test]
fn work_finish_stops_after_failed_cleanup_step() {
    for linked_worktree in [false, true] {
        let mut failures = vec![
            "head_sha",
            "commit_parent_count:merged",
            "remote_branch_exists:feature/example",
            "delete_branch_remote:feature/example",
            "is_linked_worktree",
            "delete_branch_local:feature/example",
        ];
        if linked_worktree {
            failures.extend(["detach_head", "remove_current_worktree"]);
        } else {
            failures.push("checkout:develop");
        }
        for call in failures {
            let mut git = work_git();
            git.linked_worktree = linked_worktree;
            git.fail_call = Some((call.into(), 1));
            let hosting = completed_work();
            let result = finish_work_branch(
                &git,
                &hosting,
                &MockPrompter::new(),
                &BranchType::parse("feature/example"),
                None,
                None,
                None,
                false,
            );
            assert_git_failure(result, &git, call);
            assert_eq!(hosting.calls(), ["merged_pr:feature/example"]);
        }
    }
}

#[test]
fn work_finish_stops_before_pr_creation_after_failed_validation_or_push() {
    for call in [
        "current_branch",
        "remote_branch_exists:develop",
        "push:feature/example",
    ] {
        let mut git = work_git();
        git.fail_call = Some((call.into(), 1));
        let hosting = MockHosting::new();
        let result = finish_work_branch(
            &git,
            &hosting,
            &MockPrompter::new(),
            &BranchType::parse("feature/example"),
            Some(false),
            Some("develop".into()),
            None,
            false,
        );
        assert_git_failure(result, &git, call);
        assert!(!hosting
            .calls()
            .iter()
            .any(|c| c.starts_with("create_or_get_pr:")));
    }
}

#[test]
fn work_finish_stops_after_hosting_errors() {
    for call in [
        "merged_pr:feature/example",
        "create_or_get_pr:feature/example:develop:feat: example",
    ] {
        let git = work_git();
        let mut hosting = MockHosting::new();
        hosting.fail_call = Some((call.into(), 1));
        let result = finish_work_branch(
            &git,
            &hosting,
            &MockPrompter::new(),
            &BranchType::parse("feature/example"),
            Some(false),
            Some("develop".into()),
            None,
            false,
        );
        assert_hosting_failure(result, &hosting, call);
        assert!(!git.calls().iter().any(|c| c.starts_with("delete_branch")));
    }
}

#[test]
fn work_finish_stops_before_push_if_parent_detection_fails() {
    let mut git = work_git();
    let call = "list_remote_branches";
    git.fail_call = Some((call.into(), 1));
    let result = finish_work_branch(
        &git,
        &MockHosting::new(),
        &MockPrompter::new(),
        &BranchType::parse("feature/example"),
        Some(false),
        None,
        None,
        false,
    );
    assert_git_failure(result, &git, call);
    assert!(!git.calls().iter().any(|c| c.starts_with("push:")));
}

#[test]
fn work_finish_stops_before_push_when_either_prompt_is_aborted() {
    for branches in [vec![], vec!["develop".into(), "feature/parent".into()]] {
        let mut git = work_git();
        git.remote_branches = branches;
        let hosting = MockHosting::new();
        let result = finish_work_branch(
            &git,
            &hosting,
            &MockPrompter::aborting(),
            &BranchType::parse("feature/example"),
            None,
            None,
            None,
            false,
        );
        assert_eq!(result, Err("Aborted".into()));
        assert!(!git.calls().iter().any(|c| c.starts_with("push:")));
        assert_eq!(hosting.calls(), ["merged_pr:feature/example"]);
    }
}

#[test]
fn work_finish_rejects_branches_without_a_work_commit_type_before_git_calls() {
    for branch in [
        "main",
        "develop",
        "release/1.1.0",
        "hotfix/1.1.1",
        "release-fix/1.1.0/example",
        "release-chore/1.1.0/example",
        "hotfix-fix/1.1.1/example",
        "unknown",
    ] {
        let git = MockGit::new();
        let result = finish_work_branch(
            &git,
            &MockHosting::new(),
            &MockPrompter::new(),
            &BranchType::parse(branch),
            None,
            None,
            None,
            false,
        );
        assert_eq!(
            result,
            Err("Cannot finish: not on a work branch".into()),
            "{branch}"
        );
        assert!(git.calls().is_empty(), "{branch}");
    }
}

#[test]
fn fix_finish_stops_when_current_branch_or_merged_status_cannot_be_read() {
    let mut git = MockGit::new();
    git.current_branch = "release-fix/1.1.0/example".into();
    let kind = BranchType::parse(&git.current_branch);
    git.fail_call = Some(("current_branch".into(), 1));
    assert_git_failure(
        finish_release_fix(&git, &MockHosting::new(), &kind, None, false),
        &git,
        "current_branch",
    );

    let mut git = MockGit::new();
    git.current_branch = "release-fix/1.1.0/example".into();
    let mut hosting = MockHosting::new();
    let call = "merged_pr:release-fix/1.1.0/example";
    hosting.fail_call = Some((call.into(), 1));
    assert_hosting_failure(
        finish_release_fix(&git, &hosting, &kind, None, false),
        &hosting,
        call,
    );
    assert_eq!(git.calls(), ["current_branch"]);
}

mod common;

use common::{MockGit, MockHosting};
use gflow::flows::finish_hotfix::finish_hotfix;
use gflow::repo_config::{Mode, RepoConfig};

/// Configure a MockGit for a "fresh start" hotfix finish: nothing is yet merged,
/// no tags exist, source branch still exists locally and remotely.
fn fresh_hotfix_mock(major: u32, minor: u32, patch: u32) -> MockGit {
    let mut git = MockGit::new();
    let source = format!("hotfix/{major}.{minor}.{patch}");
    git.existing_local_branches.insert(source.clone());
    git.existing_remote_branches.insert(source);
    git
}

#[test]
fn finish_hotfix_full_sequence() {
    let git = fresh_hotfix_mock(1, 0, 1);

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "is_ancestor:hotfix/1.0.1:main",
        "worktree_of:main",
        "checkout:main",
        "ff_merge:origin/main",
        "merge:hotfix/1.0.1:chore: merge hotfix 1.0.1 into main",
        "tag_exists:v1.0.1",
        "create_tag:v1.0.1:chore: hotfix 1.0.1",
        "is_pushed:main",
        "push:main",
        "remote_tag_exists:v1.0.1",
        "push_tag:v1.0.1",
        "is_ancestor:hotfix/1.0.1:develop",
        "worktree_of:develop",
        "checkout:develop",
        "ff_merge:origin/develop",
        "merge:hotfix/1.0.1:chore: merge hotfix 1.0.1 into develop",
        "is_pushed:develop",
        "push:develop",
        "list_branches_matching:release/*",
        "current_branch",
        "local_branch_exists:hotfix/1.0.1",
        "delete_branch_local:hotfix/1.0.1",
        "remote_branch_exists:hotfix/1.0.1",
        "delete_branch_remote:hotfix/1.0.1",
    ]);
    assert!(hosting.calls().is_empty(), "free mode must make zero hosting calls; calls: {:?}", hosting.calls());
}

#[test]
fn finish_hotfix_targets_master_when_that_is_the_mainline() {
    let git = fresh_hotfix_mock(2, 3, 4);

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 2, 3, 4, "master", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "is_ancestor:hotfix/2.3.4:master",
        "worktree_of:master",
        "checkout:master",
        "ff_merge:origin/master",
        "merge:hotfix/2.3.4:chore: merge hotfix 2.3.4 into master",
        "tag_exists:v2.3.4",
        "create_tag:v2.3.4:chore: hotfix 2.3.4",
        "is_pushed:master",
        "push:master",
        "remote_tag_exists:v2.3.4",
        "push_tag:v2.3.4",
        "is_ancestor:hotfix/2.3.4:develop",
        "worktree_of:develop",
        "checkout:develop",
        "ff_merge:origin/develop",
        "merge:hotfix/2.3.4:chore: merge hotfix 2.3.4 into develop",
        "is_pushed:develop",
        "push:develop",
        "list_branches_matching:release/*",
        "current_branch",
        "local_branch_exists:hotfix/2.3.4",
        "delete_branch_local:hotfix/2.3.4",
        "remote_branch_exists:hotfix/2.3.4",
        "delete_branch_remote:hotfix/2.3.4",
    ]);
}

#[test]
fn finish_hotfix_propagates_to_open_release_branch() {
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.branches_matching = vec!["release/1.2.0".to_string()];

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "is_ancestor:hotfix/1.0.1:main",
        "worktree_of:main",
        "checkout:main",
        "ff_merge:origin/main",
        "merge:hotfix/1.0.1:chore: merge hotfix 1.0.1 into main",
        "tag_exists:v1.0.1",
        "create_tag:v1.0.1:chore: hotfix 1.0.1",
        "is_pushed:main",
        "push:main",
        "remote_tag_exists:v1.0.1",
        "push_tag:v1.0.1",
        "is_ancestor:hotfix/1.0.1:develop",
        "worktree_of:develop",
        "checkout:develop",
        "ff_merge:origin/develop",
        "merge:hotfix/1.0.1:chore: merge hotfix 1.0.1 into develop",
        "is_pushed:develop",
        "push:develop",
        "list_branches_matching:release/*",
        "tag_exists:v1.2.0",
        "is_ancestor:hotfix/1.0.1:release/1.2.0",
        "worktree_of:release/1.2.0",
        "checkout:release/1.2.0",
        "ff_merge:origin/release/1.2.0",
        "merge:hotfix/1.0.1:chore: merge hotfix 1.0.1 into release/1.2.0",
        "is_pushed:release/1.2.0",
        "push:release/1.2.0",
        "current_branch",
        "local_branch_exists:hotfix/1.0.1",
        "delete_branch_local:hotfix/1.0.1",
        "remote_branch_exists:hotfix/1.0.1",
        "delete_branch_remote:hotfix/1.0.1",
    ]);
}

#[test]
fn finish_hotfix_excludes_shipped_release_branch() {
    // Trap 1: release/1.2.0 already shipped (tagged v1.2.0) — fan-out must
    // not merge or push into it, only into the still-open release/1.3.0.
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.branches_matching = vec!["release/1.2.0".to_string(), "release/1.3.0".to_string()];
    git.existing_tags.insert("v1.2.0".to_string());

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"tag_exists:v1.2.0".to_string()));
    assert!(calls.contains(&"tag_exists:v1.3.0".to_string()));
    assert!(
        !calls.iter().any(|c| c.contains("release/1.2.0")),
        "shipped release/1.2.0 must receive no merge/push; calls: {calls:?}"
    );
    assert!(
        calls.contains(&"merge:hotfix/1.0.1:chore: merge hotfix 1.0.1 into release/1.3.0".to_string()),
        "open release/1.3.0 must still be merged into; calls: {calls:?}"
    );
    assert!(calls.contains(&"push:release/1.3.0".to_string()));
}

#[test]
fn finish_hotfix_propagates_to_multiple_release_branches_in_sorted_order() {
    let mut git = fresh_hotfix_mock(1, 0, 1);
    // Provide unsorted to verify the implementation sorts deterministically.
    git.branches_matching = vec![
        "release/2.0.0".to_string(),
        "release/1.5.0".to_string(),
    ];

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    let expected_tail = vec![
        "list_branches_matching:release/*",
        "tag_exists:v2.0.0",
        "tag_exists:v1.5.0",
        "is_ancestor:hotfix/1.0.1:release/1.5.0",
        "worktree_of:release/1.5.0",
        "checkout:release/1.5.0",
        "ff_merge:origin/release/1.5.0",
        "merge:hotfix/1.0.1:chore: merge hotfix 1.0.1 into release/1.5.0",
        "is_pushed:release/1.5.0",
        "push:release/1.5.0",
        "is_ancestor:hotfix/1.0.1:release/2.0.0",
        "worktree_of:release/2.0.0",
        "checkout:release/2.0.0",
        "ff_merge:origin/release/2.0.0",
        "merge:hotfix/1.0.1:chore: merge hotfix 1.0.1 into release/2.0.0",
        "is_pushed:release/2.0.0",
        "push:release/2.0.0",
        "current_branch",
        "local_branch_exists:hotfix/1.0.1",
        "delete_branch_local:hotfix/1.0.1",
        "remote_branch_exists:hotfix/1.0.1",
        "delete_branch_remote:hotfix/1.0.1",
    ];
    let tail_start = calls.len() - expected_tail.len();
    assert_eq!(&calls[tail_start..], &expected_tail[..]);
}

#[test]
fn finish_hotfix_excludes_release_fix_branches() {
    // The `release/*` pattern does the excluding; the flow has no filter of its own.
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.branches_matching = vec![
        "release/1.2.0".to_string(),
        "release-fix/1.2.0/foo".to_string(),
    ];

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(
        calls.iter().any(|c| c == "list_branches_matching:release/*"),
        "the pattern is what does the excluding; got: {calls:?}"
    );
    assert!(
        calls.iter().any(|c| c == "checkout:release/1.2.0"),
        "expected propagation to release/1.2.0; got: {calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c.contains("release-fix/")),
        "must not propagate into release-fix branches; got: {calls:?}"
    );
}

#[test]
fn finish_hotfix_aborts_on_release_merge_conflict_without_deleting_hotfix() {
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.branches_matching = vec!["release/1.2.0".to_string()];
    // 1st merge: into main (ok), 2nd: into develop (ok), 3rd: into release (fail).
    git.fail_nth_merge = Some(3);

    let hosting = MockHosting::new();
    let result = finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false);

    assert!(result.is_err(), "expected merge conflict to surface");
    let err = result.unwrap_err();
    assert!(err.contains("release/1.2.0"), "error should name the conflicting branch; got: {err}");
    assert!(err.contains("gflow finish"), "error should tell user to re-run gflow finish; got: {err}");

    let calls = git.calls();
    // Main + develop + tag must already be done before the conflict.
    assert!(calls.iter().any(|c| c == "create_tag:v1.0.1:chore: hotfix 1.0.1"));
    assert!(calls.iter().any(|c| c == "push_tag:v1.0.1"));
    assert!(calls.iter().any(|c| c == "push:develop"));
    // Hotfix branch must NOT be deleted — user needs it for retry.
    assert!(
        !calls.iter().any(|c| c.starts_with("delete_branch_local:hotfix/")),
        "hotfix branch must survive for retry; calls: {calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c.starts_with("delete_branch_remote:hotfix/")),
        "hotfix branch must survive for retry; calls: {calls:?}"
    );
}

// --- Idempotent resume tests ---

#[test]
fn finish_hotfix_resume_skips_already_merged_main_and_develop() {
    // Scenario: previous finish merged into main + develop + tagged + pushed,
    // but failed during release propagation. User resolves, re-runs.
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.branches_matching = vec!["release/1.2.0".to_string()];
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "main".to_string()));
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "develop".to_string()));
    git.existing_tags.insert("v1.0.1".to_string());
    git.existing_remote_tags.insert("v1.0.1".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    // No re-merges of main or develop
    assert!(!calls.iter().any(|c| c == "checkout:main"), "must not re-checkout main; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c == "checkout:develop"), "must not re-checkout develop; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("merge:hotfix/1.0.1:") && c.contains("into main")), "must not re-merge main");
    assert!(!calls.iter().any(|c| c.starts_with("merge:hotfix/1.0.1:") && c.contains("into develop")), "must not re-merge develop");
    // No re-tag, no re-push
    assert!(!calls.iter().any(|c| c.starts_with("create_tag:")), "must not re-create tag");
    assert!(!calls.iter().any(|c| c == "push:main"), "must not re-push main");
    assert!(!calls.iter().any(|c| c == "push_tag:v1.0.1"), "must not re-push tag");
    // Release propagation still runs
    assert!(calls.iter().any(|c| c == "merge:hotfix/1.0.1:chore: merge hotfix 1.0.1 into release/1.2.0"), "must merge into release; calls: {calls:?}");
    // Hotfix cleanup runs
    assert!(calls.iter().any(|c| c == "delete_branch_local:hotfix/1.0.1"));
    assert!(calls.iter().any(|c| c == "delete_branch_remote:hotfix/1.0.1"));
}

#[test]
fn finish_hotfix_resume_skips_already_propagated_release() {
    // Scenario: previous finish completed everything except cleanup.
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.2.0".to_string()];
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "main".to_string()));
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "develop".to_string()));
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "release/1.2.0".to_string()));
    git.existing_tags.insert("v1.0.1".to_string());
    git.existing_remote_tags.insert("v1.0.1".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    git.pushed_branches.insert("release/1.2.0".to_string());
    git.existing_local_branches.insert("hotfix/1.0.1".to_string());
    git.existing_remote_branches.insert("hotfix/1.0.1".to_string());

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("merge:")), "no merges should run; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("push:")), "no pushes should run; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("create_tag:")), "no tag creation");
    assert!(calls.iter().any(|c| c == "delete_branch_local:hotfix/1.0.1"));
    assert!(calls.iter().any(|c| c == "delete_branch_remote:hotfix/1.0.1"));
}

#[test]
fn finish_hotfix_switches_off_source_before_deleting_when_currently_on_it() {
    // Scenario: resume after develop-merge conflict resolved on develop.
    // User switches back to hotfix and re-runs. The develop merge is detected
    // as already done, so the flow never checks out develop — HEAD stays on
    // the hotfix branch. The cleanup step must switch off it before deleting.
    let mut git = MockGit::new();
    git.current_branch = "hotfix/1.0.1".to_string();
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "main".to_string()));
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "develop".to_string()));
    git.existing_tags.insert("v1.0.1".to_string());
    git.existing_remote_tags.insert("v1.0.1".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    git.existing_local_branches.insert("hotfix/1.0.1".to_string());
    git.existing_remote_branches.insert("hotfix/1.0.1".to_string());

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    let checkout_main_idx = calls.iter().position(|c| c == "checkout:main")
        .unwrap_or_else(|| panic!("expected a checkout:main before delete; calls: {calls:?}"));
    let delete_idx = calls.iter().position(|c| c == "delete_branch_local:hotfix/1.0.1")
        .expect("expected delete_branch_local");
    assert!(checkout_main_idx < delete_idx,
        "checkout:main must come before delete_branch_local; calls: {calls:?}");
}

#[test]
fn finish_hotfix_skips_cleanup_checkout_when_already_off_source() {
    // Sanity: when HEAD is not on the source branch (the happy path, where
    // the develop merge moved us to develop), no extra checkout fires.
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.current_branch = "develop".to_string();

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    // The only checkouts should be the ones the flow explicitly drives (main, develop).
    let checkouts: Vec<&String> = calls.iter().filter(|c| c.starts_with("checkout:")).collect();
    assert_eq!(checkouts, vec![
        &"checkout:main".to_string(),
        &"checkout:develop".to_string(),
    ], "unexpected extra checkout; calls: {calls:?}");
}

#[test]
fn finish_hotfix_resume_when_branch_already_deleted_is_idempotent() {
    // Scenario: everything done; user runs again. All deletions skipped.
    let mut git = MockGit::new();
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "main".to_string()));
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "develop".to_string()));
    git.existing_tags.insert("v1.0.1".to_string());
    git.existing_remote_tags.insert("v1.0.1".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    // Note: no entries in existing_local_branches / existing_remote_branches → already deleted

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("delete_branch_")), "deletions should be skipped; calls: {calls:?}");
}

#[test]
fn finish_hotfix_keeps_branch_when_configured() {
    // Same fully-completed world as finish_hotfix_resume_when_branch_already_deleted_is_idempotent,
    // except the source branch still exists and HEAD is still on it — proving
    // keep-release-branches=true skips delete_source_branch entirely, not just
    // its individual deletion calls.
    let mut git = MockGit::new();
    git.current_branch = "hotfix/1.0.1".to_string();
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "main".to_string()));
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "develop".to_string()));
    git.existing_tags.insert("v1.0.1".to_string());
    git.existing_remote_tags.insert("v1.0.1".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    git.existing_local_branches.insert("hotfix/1.0.1".to_string());
    git.existing_remote_branches.insert("hotfix/1.0.1".to_string());

    let hosting = MockHosting::new();
    let cfg = RepoConfig { mode: Mode::Free, keep_release_branches: true, ..RepoConfig::default() };
    finish_hotfix(&git, &hosting, &cfg, 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("delete_branch_")), "keep must skip deletion; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c == "checkout:main"), "keep must skip delete_source_branch's checkout; calls: {calls:?}");
}

// --- Conflict guidance: every merge step must tell the user to switch back ---

#[test]
fn finish_hotfix_main_merge_conflict_names_source_branch_to_switch_back() {
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.fail_nth_merge = Some(1); // main merge

    let hosting = MockHosting::new();
    let err = finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap_err();
    assert!(err.contains("git add . && git commit --no-edit"),
        "the commit step must come before git switch, which fails mid-merge; got: {err}");
    assert!(err.contains("git switch hotfix/1.0.1"),
        "main conflict should tell user to switch back to the hotfix branch; got: {err}");
    assert!(err.contains("gflow finish"), "should mention re-running gflow finish; got: {err}");
}

#[test]
fn finish_hotfix_develop_merge_conflict_names_source_branch_to_switch_back() {
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.fail_nth_merge = Some(2); // develop merge

    let hosting = MockHosting::new();
    let err = finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap_err();
    assert!(err.contains("git switch hotfix/1.0.1"),
        "develop conflict should tell user to switch back to the hotfix branch; got: {err}");
}

// --- Protected mode: sequential landing PRs (hotfixes have no RC gate) ---

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
fn protected_hotfix_opens_main_pr_and_stops() {
    let mut git = MockGit::new();
    git.existing_remote_branches.insert("hotfix/1.1.1".to_string());
    git.pushed_branches.insert("hotfix/1.1.1".to_string());

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "tag_exists:v1.1.1",
        "remote_branch_exists:hotfix/1.1.1",
        "is_pushed:hotfix/1.1.1",
        "remote_branch_exists:finish/hotfix-1.1.1-into-main",
        "local_branch_exists:finish/hotfix-1.1.1-into-main",
        "create_branch_no_checkout:finish/hotfix-1.1.1-into-main:hotfix/1.1.1",
        "is_ancestor:hotfix/1.1.1:finish/hotfix-1.1.1-into-main",
        "is_ancestor:origin/main:finish/hotfix-1.1.1-into-main",
        "current_branch",
        "checkout:finish/hotfix-1.1.1-into-main",
        "merge:origin/main:chore: merge main into finish/hotfix-1.1.1-into-main",
        "checkout:develop",
        "push:finish/hotfix-1.1.1-into-main",
    ]);
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:hotfix/1.1.1:main",
        "merged_pr_to:finish/hotfix-1.1.1-into-main:main",
        "merged_pr_to:hotfix/1.1.1:main",
        "create_or_get_pr:finish/hotfix-1.1.1-into-main:main:chore: merge hotfix 1.1.1 into main:empty-body",
        "copy_text:chore: merge hotfix 1.1.1 into main\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c == "checkout:main"), "must never land into the target locally; calls: {calls:?}");
    assert!(calls.iter().filter(|c| c.starts_with("merge:")).all(|c| c.ends_with("into finish/hotfix-1.1.1-into-main")),
        "merges may only append to the finish branch; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("create_tag")), "must not tag before main lands; calls: {calls:?}");
}

#[test]
fn protected_hotfix_tags_merge_commit_then_opens_develop_pr() {
    let mut git = MockGit::new();
    git.branch_shas.insert("hotfix/1.1.1".to_string(), "hfsha".to_string());
    git.existing_remote_branches.insert("hotfix/1.1.1".to_string());
    git.pushed_branches.insert("hotfix/1.1.1".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("hotfix/1.1.1".to_string(), "main".to_string()),
        landed("hfsha", "mc1"),
    );
    git.parent_counts.insert("mc1".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "is_ancestor:mc1:origin/main",
        "commit_parent_count:mc1",
        "tag_exists:v1.1.1",
        "tag_exists:v1.1.1",
        "create_tag_at:v1.1.1:chore: hotfix 1.1.1:mc1",
        "remote_tag_exists:v1.1.1",
        "push_tag:v1.1.1",
        "branch_sha:hotfix/1.1.1",
        "is_ancestor:hotfix/1.1.1:origin/develop",
        "remote_branch_exists:hotfix/1.1.1",
        "is_pushed:hotfix/1.1.1",
        "remote_branch_exists:finish/hotfix-1.1.1-into-develop",
        "local_branch_exists:finish/hotfix-1.1.1-into-develop",
        "create_branch_no_checkout:finish/hotfix-1.1.1-into-develop:hotfix/1.1.1",
        "is_ancestor:hotfix/1.1.1:finish/hotfix-1.1.1-into-develop",
        "is_ancestor:origin/develop:finish/hotfix-1.1.1-into-develop",
        "current_branch",
        "checkout:finish/hotfix-1.1.1-into-develop",
        "merge:origin/develop:chore: merge develop into finish/hotfix-1.1.1-into-develop",
        "checkout:develop",
        "push:finish/hotfix-1.1.1-into-develop",
    ]);
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:hotfix/1.1.1:main",
        "merged_pr_to:finish/hotfix-1.1.1-into-main:main",
        "merged_pr_to:hotfix/1.1.1:main",
        "open_pr_to:hotfix/1.1.1:develop",
        "merged_pr_to:finish/hotfix-1.1.1-into-develop:develop",
        "merged_pr_to:hotfix/1.1.1:develop",
        "create_or_get_pr:finish/hotfix-1.1.1-into-develop:develop:chore: merge hotfix 1.1.1 into develop:empty-body",
        "copy_text:chore: merge hotfix 1.1.1 into develop\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("rev_list_count")), "hotfixes carry no RC gate; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("tags_on_branch")), "hotfixes carry no RC gate; calls: {calls:?}");
}

#[test]
fn protected_hotfix_opens_one_release_pr_per_run() {
    let mut git = MockGit::new();
    git.branch_shas.insert("hotfix/1.1.1".to_string(), "hfsha".to_string());
    git.branches_matching = vec!["release/1.3.0".to_string(), "release/1.2.0".to_string()];
    git.existing_remote_branches.insert("hotfix/1.1.1".to_string());
    git.pushed_branches.insert("hotfix/1.1.1".to_string());
    git.existing_tags.insert("v1.1.1".to_string());
    git.tag_commits.insert("v1.1.1".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.1".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "main".to_string()), landed("hfsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "develop".to_string()), landed("hfsha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "is_ancestor:mc1:origin/main",
        "commit_parent_count:mc1",
        "tag_exists:v1.1.1",
        "tag_commit_sha:v1.1.1",
        "is_ancestor:mc1:origin/main",
        "remote_tag_exists:v1.1.1",
        "branch_sha:hotfix/1.1.1",
        "is_ancestor:mc2:origin/develop",
        "commit_parent_count:mc2",
        "branch_sha:hotfix/1.1.1",
        "list_branches_matching:release/*",
        "tag_exists:v1.3.0",
        "tag_exists:v1.2.0",
        "is_ancestor:hotfix/1.1.1:origin/release/1.2.0",
        "remote_branch_exists:hotfix/1.1.1",
        "is_pushed:hotfix/1.1.1",
        "remote_branch_exists:finish/hotfix-1.1.1-into-release-1.2.0",
        "local_branch_exists:finish/hotfix-1.1.1-into-release-1.2.0",
        "create_branch_no_checkout:finish/hotfix-1.1.1-into-release-1.2.0:hotfix/1.1.1",
        "is_ancestor:hotfix/1.1.1:finish/hotfix-1.1.1-into-release-1.2.0",
        "is_ancestor:origin/release/1.2.0:finish/hotfix-1.1.1-into-release-1.2.0",
        "current_branch",
        "checkout:finish/hotfix-1.1.1-into-release-1.2.0",
        "merge:origin/release/1.2.0:chore: merge release/1.2.0 into finish/hotfix-1.1.1-into-release-1.2.0",
        "checkout:develop",
        "push:finish/hotfix-1.1.1-into-release-1.2.0",
    ]);
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:hotfix/1.1.1:main",
        "merged_pr_to:finish/hotfix-1.1.1-into-main:main",
        "merged_pr_to:hotfix/1.1.1:main",
        "open_pr_to:hotfix/1.1.1:develop",
        "merged_pr_to:finish/hotfix-1.1.1-into-develop:develop",
        "merged_pr_to:hotfix/1.1.1:develop",
        "open_pr_to:hotfix/1.1.1:release/1.2.0",
        "merged_pr_to:finish/hotfix-1.1.1-into-release-1.2.0:release/1.2.0",
        "merged_pr_to:hotfix/1.1.1:release/1.2.0",
        "create_or_get_pr:finish/hotfix-1.1.1-into-release-1.2.0:release/1.2.0:chore: merge hotfix 1.1.1 into release/1.2.0:empty-body",
        "copy_text:chore: merge hotfix 1.1.1 into release/1.2.0\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

#[test]
fn protected_hotfix_next_run_opens_pr_for_the_remaining_release() {
    let mut git = MockGit::new();
    git.branch_shas.insert("hotfix/1.1.1".to_string(), "hfsha".to_string());
    git.branches_matching = vec!["release/1.3.0".to_string(), "release/1.2.0".to_string()];
    git.existing_remote_branches.insert("hotfix/1.1.1".to_string());
    git.pushed_branches.insert("hotfix/1.1.1".to_string());
    git.existing_tags.insert("v1.1.1".to_string());
    git.tag_commits.insert("v1.1.1".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.1".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    git.ancestors.insert(("mc3".to_string(), "origin/release/1.2.0".to_string()));
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "main".to_string()), landed("hfsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "develop".to_string()), landed("hfsha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "release/1.2.0".to_string()), landed("hfsha", "mc3"));
    git.parent_counts.insert("mc3".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "is_ancestor:mc1:origin/main",
        "commit_parent_count:mc1",
        "tag_exists:v1.1.1",
        "tag_commit_sha:v1.1.1",
        "is_ancestor:mc1:origin/main",
        "remote_tag_exists:v1.1.1",
        "branch_sha:hotfix/1.1.1",
        "is_ancestor:mc2:origin/develop",
        "commit_parent_count:mc2",
        "branch_sha:hotfix/1.1.1",
        "list_branches_matching:release/*",
        "tag_exists:v1.3.0",
        "tag_exists:v1.2.0",
        "is_ancestor:mc3:origin/release/1.2.0",
        "commit_parent_count:mc3",
        "branch_sha:hotfix/1.1.1",
        "is_ancestor:hotfix/1.1.1:origin/release/1.3.0",
        "remote_branch_exists:hotfix/1.1.1",
        "is_pushed:hotfix/1.1.1",
        "remote_branch_exists:finish/hotfix-1.1.1-into-release-1.3.0",
        "local_branch_exists:finish/hotfix-1.1.1-into-release-1.3.0",
        "create_branch_no_checkout:finish/hotfix-1.1.1-into-release-1.3.0:hotfix/1.1.1",
        "is_ancestor:hotfix/1.1.1:finish/hotfix-1.1.1-into-release-1.3.0",
        "is_ancestor:origin/release/1.3.0:finish/hotfix-1.1.1-into-release-1.3.0",
        "current_branch",
        "checkout:finish/hotfix-1.1.1-into-release-1.3.0",
        "merge:origin/release/1.3.0:chore: merge release/1.3.0 into finish/hotfix-1.1.1-into-release-1.3.0",
        "checkout:develop",
        "push:finish/hotfix-1.1.1-into-release-1.3.0",
    ]);
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:hotfix/1.1.1:main",
        "merged_pr_to:finish/hotfix-1.1.1-into-main:main",
        "merged_pr_to:hotfix/1.1.1:main",
        "open_pr_to:hotfix/1.1.1:develop",
        "merged_pr_to:finish/hotfix-1.1.1-into-develop:develop",
        "merged_pr_to:hotfix/1.1.1:develop",
        "open_pr_to:hotfix/1.1.1:release/1.2.0",
        "merged_pr_to:finish/hotfix-1.1.1-into-release-1.2.0:release/1.2.0",
        "merged_pr_to:hotfix/1.1.1:release/1.2.0",
        "open_pr_to:hotfix/1.1.1:release/1.3.0",
        "merged_pr_to:finish/hotfix-1.1.1-into-release-1.3.0:release/1.3.0",
        "merged_pr_to:hotfix/1.1.1:release/1.3.0",
        "create_or_get_pr:finish/hotfix-1.1.1-into-release-1.3.0:release/1.3.0:chore: merge hotfix 1.1.1 into release/1.3.0:empty-body",
        "copy_text:chore: merge hotfix 1.1.1 into release/1.3.0\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

#[test]
fn protected_hotfix_completes_after_all_landed() {
    let mut git = MockGit::new();
    git.current_branch = "hotfix/1.1.1".to_string();
    git.branch_shas.insert("hotfix/1.1.1".to_string(), "hfsha".to_string());
    git.existing_tags.insert("v1.1.1".to_string());
    git.tag_commits.insert("v1.1.1".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.1".to_string());
    git.existing_local_branches.insert("hotfix/1.1.1".to_string());
    git.existing_remote_branches.insert("hotfix/1.1.1".to_string());
    git.branches_matching = vec!["release/1.2.0".to_string()];
    git.branches_matching_by.insert("finish/hotfix-1.1.1-into-*".to_string(), vec![]);
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    git.ancestors.insert(("mc3".to_string(), "origin/release/1.2.0".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "main".to_string()), landed("hfsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "develop".to_string()), landed("hfsha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "release/1.2.0".to_string()), landed("hfsha", "mc3"));
    git.parent_counts.insert("mc3".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 1, 1, "main", None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "is_ancestor:mc1:origin/main",
        "commit_parent_count:mc1",
        "tag_exists:v1.1.1",
        "tag_commit_sha:v1.1.1",
        "is_ancestor:mc1:origin/main",
        "remote_tag_exists:v1.1.1",
        "branch_sha:hotfix/1.1.1",
        "is_ancestor:mc2:origin/develop",
        "commit_parent_count:mc2",
        "branch_sha:hotfix/1.1.1",
        "list_branches_matching:release/*",
        "tag_exists:v1.2.0",
        "is_ancestor:mc3:origin/release/1.2.0",
        "commit_parent_count:mc3",
        "branch_sha:hotfix/1.1.1",
        "branch_sha:hotfix/1.1.1",
        "list_branches_matching:finish/hotfix-1.1.1-into-*",
        "current_branch",
        "is_linked_worktree",
        "worktree_of:main",
        "checkout:main",
        "local_branch_exists:hotfix/1.1.1",
        "delete_branch_local:hotfix/1.1.1",
        "remote_branch_exists:hotfix/1.1.1",
        "delete_branch_remote:hotfix/1.1.1",
    ]);
    assert_eq!(hosting.calls(), vec![
        "open_pr_to:hotfix/1.1.1:main",
        "merged_pr_to:finish/hotfix-1.1.1-into-main:main",
        "merged_pr_to:hotfix/1.1.1:main",
        "open_pr_to:hotfix/1.1.1:develop",
        "merged_pr_to:finish/hotfix-1.1.1-into-develop:develop",
        "merged_pr_to:hotfix/1.1.1:develop",
        "open_pr_to:hotfix/1.1.1:release/1.2.0",
        "merged_pr_to:finish/hotfix-1.1.1-into-release-1.2.0:release/1.2.0",
        "merged_pr_to:hotfix/1.1.1:release/1.2.0",
    ]);
}

#[test]
fn protected_hotfix_keeps_branch_when_configured() {
    let mut git = MockGit::new();
    git.current_branch = "hotfix/1.1.1".to_string();
    git.branch_shas.insert("hotfix/1.1.1".to_string(), "hfsha".to_string());
    git.existing_tags.insert("v1.1.1".to_string());
    git.tag_commits.insert("v1.1.1".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.1".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "main".to_string()), landed("hfsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "develop".to_string()), landed("hfsha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(true), 1, 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c == "checkout:main"), "keep must skip delete_source_branch's checkout; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("delete_branch_")), "keep must skip deletion; calls: {calls:?}");
}

#[test]
fn protected_hotfix_reopens_the_develop_leg_when_the_branch_moved() {
    // Main landed (tag containment) and develop landed with an older head —
    // the branch tip has since moved, so develop is MISSING the new commits.
    // The strict develop-leg check must re-open that leg with a refreshed
    // finish branch instead of advancing past it (the old lenient behaviour
    // silently dropped the commits from develop).
    let mut git = MockGit::new();
    git.existing_tags.insert("v1.3.1".to_string());
    git.tag_commits.insert("v1.3.1".to_string(), "old-tip-sha".to_string());
    git.ancestors.insert(("old-tip-sha".to_string(), "origin/main".to_string()));
    git.branch_shas.insert("hotfix/1.3.1".to_string(), "moved-tip-sha".to_string());
    git.branches_matching = vec!["release/1.4.0".to_string()];
    git.existing_remote_branches.insert("hotfix/1.3.1".to_string());
    git.pushed_branches.insert("hotfix/1.3.1".to_string());
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("hotfix/1.3.1".to_string(), "develop".to_string()), landed("old-develop-head", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 3, 1, "main", None, false).unwrap();

    let hosting_calls = hosting.calls();
    assert_eq!(hosting_calls, vec![
        "open_pr_to:hotfix/1.3.1:main",
        "merged_pr_to:finish/hotfix-1.3.1-into-main:main",
        "merged_pr_to:hotfix/1.3.1:main",
        "open_pr_to:hotfix/1.3.1:develop",
        "merged_pr_to:finish/hotfix-1.3.1-into-develop:develop",
        "merged_pr_to:hotfix/1.3.1:develop",
        "create_or_get_pr:finish/hotfix-1.3.1-into-develop:develop:chore: merge hotfix 1.3.1 into develop:empty-body",
        "copy_text:chore: merge hotfix 1.3.1 into develop\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    assert!(!hosting_calls.iter().any(|c| c.starts_with("create_or_get_pr") && c.contains(":main:")),
        "must not reopen a main PR once main has landed; calls: {hosting_calls:?}");
    assert!(!hosting_calls.iter().any(|c| c.contains(":release/1.4.0")),
        "the release leg is not reached while develop misses commits; calls: {hosting_calls:?}");
}

#[test]
fn protected_hotfix_reopens_develop_instead_of_completing_when_the_tip_landed_nowhere() {
    // The branch tip is in no landed PR — commits that never reached any
    // target. The strict develop-leg check re-opens develop with a refreshed
    // finish branch, so nothing is deleted and the commits actually land.
    let mut git = MockGit::new();
    git.existing_tags.insert("v1.3.1".to_string());
    git.tag_commits.insert("v1.3.1".to_string(), "old-tip-sha".to_string());
    git.ancestors.insert(("old-tip-sha".to_string(), "origin/main".to_string()));
    git.branch_shas.insert("hotfix/1.3.1".to_string(), "unrelated-tip-sha".to_string());
    git.existing_local_branches.insert("hotfix/1.3.1".to_string());
    git.existing_remote_branches.insert("hotfix/1.3.1".to_string());
    git.pushed_branches.insert("hotfix/1.3.1".to_string());
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));

    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("hotfix/1.3.1".to_string(), "develop".to_string()), landed("old-develop-head", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    let result = finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 3, 1, "main", None, false);
    assert!(result.is_ok(), "re-opening the leg must not fail the run; got: {result:?}");

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("delete_branch_local")), "unlanded commits must not be deleted; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("delete_branch_remote")), "unlanded commits must not be deleted; calls: {calls:?}");
    let hosting_calls = hosting.calls();
    assert_eq!(hosting_calls[hosting_calls.len() - 3..], [
        "create_or_get_pr:finish/hotfix-1.3.1-into-develop:develop:chore: merge hotfix 1.3.1 into develop:empty-body".to_string(),
        "copy_text:chore: merge hotfix 1.3.1 into develop\nhttps://github.com/org/repo/pull/1".to_string(),
        "open_url:https://github.com/org/repo/pull/1".to_string(),
    ], "the develop leg must re-open with a finish-branch PR and put it in the browser and on the clipboard");
}

#[test]
fn finish_hotfix_merges_in_place_when_target_is_checked_out_in_another_worktree() {
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.current_branch = "hotfix/1.0.1".to_string();
    git.worktree_root = std::path::PathBuf::from("/repos/beans-api-hotfix-1.0.1");
    git.worktrees.insert("develop".to_string(), std::path::PathBuf::from("/repos/beans-api"));

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.contains(&"checkout:develop".to_string()), "must not check out a branch held by another worktree; calls: {calls:?}");
    let develop_leg: Vec<&String> = calls.iter().skip_while(|c| *c != "is_ancestor:hotfix/1.0.1:develop").take(5).collect();
    assert_eq!(develop_leg, vec![
        "is_ancestor:hotfix/1.0.1:develop",
        "worktree_of:develop",
        "is_working_tree_clean_at:/repos/beans-api",
        "ff_merge_at:/repos/beans-api:origin/develop",
        "merge_at:/repos/beans-api:hotfix/1.0.1:chore: merge hotfix 1.0.1 into develop",
    ]);
}

#[test]
fn finish_hotfix_refuses_to_merge_into_a_dirty_worktree_it_does_not_stand_in() {
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.current_branch = "hotfix/1.0.1".to_string();
    git.worktrees.insert("develop".to_string(), std::path::PathBuf::from("/repos/beans-api"));
    git.working_tree_clean = false;

    let hosting = MockHosting::new();
    let err = finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap_err();

    assert!(err.contains("/repos/beans-api") && err.contains("develop"), "got: {err}");
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("ff_merge_at") || c.starts_with("merge_at")), "must not touch a dirty tree; calls: {calls:?}");
}

#[test]
fn finish_hotfix_cleanup_detaches_when_main_is_held_by_another_worktree() {
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.current_branch = "hotfix/1.0.1".to_string();
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "main".to_string()));
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "develop".to_string()));
    git.existing_tags.insert("v1.0.1".to_string());
    git.existing_remote_tags.insert("v1.0.1".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    git.worktrees.insert("main".to_string(), std::path::PathBuf::from("/repos/beans-api-main"));

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(!calls.contains(&"checkout:main".to_string()), "must not check out a branch held by another worktree; calls: {calls:?}");
    let cleanup: Vec<&String> = calls.iter().skip_while(|c| *c != "current_branch").take(5).collect();
    assert_eq!(cleanup, vec![
        "current_branch",
        "is_linked_worktree",
        "worktree_of:main",
        "detach_head",
        "local_branch_exists:hotfix/1.0.1",
    ]);
}

#[test]
fn finish_hotfix_in_its_own_worktree_removes_the_worktree_last() {
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.current_branch = "hotfix/1.0.1".to_string();
    git.linked_worktree = true;
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "main".to_string()));
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "develop".to_string()));
    git.existing_tags.insert("v1.0.1".to_string());
    git.existing_remote_tags.insert("v1.0.1".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());

    let hosting = MockHosting::new();
    finish_hotfix(&git, &hosting, &RepoConfig::default(), 1, 0, 1, "main", None, false).unwrap();

    let calls = git.calls();
    let cleanup: Vec<&String> = calls.iter().skip_while(|c| *c != "current_branch").collect();
    assert_eq!(cleanup, vec![
        "current_branch",
        "is_linked_worktree",
        "detach_head",
        "local_branch_exists:hotfix/1.0.1",
        "delete_branch_local:hotfix/1.0.1",
        "remote_branch_exists:hotfix/1.0.1",
        "delete_branch_remote:hotfix/1.0.1",
        "remove_current_worktree",
    ]);
}

#[test]
fn finish_hotfix_keeping_the_branch_keeps_the_worktree() {
    let mut git = fresh_hotfix_mock(1, 0, 1);
    git.current_branch = "hotfix/1.0.1".to_string();
    git.linked_worktree = true;
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "main".to_string()));
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "develop".to_string()));
    git.existing_tags.insert("v1.0.1".to_string());
    git.existing_remote_tags.insert("v1.0.1".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    let cfg = RepoConfig { keep_release_branches: true, ..RepoConfig::default() };

    finish_hotfix(&git, &MockHosting::new(), &cfg, 1, 0, 1, "main", None, false).unwrap();

    assert!(!git.calls().iter().any(|c| c == "remove_current_worktree" || c == "detach_head"),
        "a kept branch keeps its worktree; calls: {:?}", git.calls());
}

// --- Protected landings via finish/* branches ---

#[test]
fn protected_hotfix_all_legs_use_finish_branches() {
    let mut git = MockGit::new();
    git.branch_shas.insert("hotfix/1.1.1".to_string(), "hfsha".to_string());
    git.existing_tags.insert("v1.1.1".to_string());
    git.tag_commits.insert("v1.1.1".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.1".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    git.branches_matching_by.insert("release/*".to_string(), vec!["release/1.2.0".to_string()]);
    git.existing_remote_branches.insert("hotfix/1.1.1".to_string());
    git.pushed_branches.insert("hotfix/1.1.1".to_string());
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("finish/hotfix-1.1.1-into-main".to_string(), "main".to_string()), landed("hfsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("finish/hotfix-1.1.1-into-develop".to_string(), "develop".to_string()), landed("hfsha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 1, 1, "main", None, false).unwrap();

    assert_eq!(hosting.calls(), vec![
        "open_pr_to:hotfix/1.1.1:main",
        "merged_pr_to:finish/hotfix-1.1.1-into-main:main",
        "open_pr_to:hotfix/1.1.1:develop",
        "merged_pr_to:finish/hotfix-1.1.1-into-develop:develop",
        "open_pr_to:hotfix/1.1.1:release/1.2.0",
        "merged_pr_to:finish/hotfix-1.1.1-into-release-1.2.0:release/1.2.0",
        "merged_pr_to:hotfix/1.1.1:release/1.2.0",
        "create_or_get_pr:finish/hotfix-1.1.1-into-release-1.2.0:release/1.2.0:chore: merge hotfix 1.1.1 into release/1.2.0:empty-body",
        "copy_text:chore: merge hotfix 1.1.1 into release/1.2.0\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

#[test]
fn protected_hotfix_completes_after_conflict_resolutions_and_cleans_every_finish_branch() {
    // Both legs' PRs carry conflict-resolution commits: their heads no longer
    // equal the hotfix tip, and containment is proven via origin/finish
    // ancestry. Cleanup must still delete the source — and every finish/*
    // branch, including an orphan for a release that shipped mid-finish.
    let mut git = MockGit::new();
    git.current_branch = "hotfix/1.1.1".to_string();
    git.branch_shas.insert("hotfix/1.1.1".to_string(), "hfsha".to_string());
    git.existing_tags.insert("v1.1.1".to_string());
    git.tag_commits.insert("v1.1.1".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.1".to_string());
    git.existing_local_branches.insert("hotfix/1.1.1".to_string());
    git.existing_remote_branches.insert("hotfix/1.1.1".to_string());
    git.existing_remote_branches.insert("finish/hotfix-1.1.1-into-main".to_string());
    git.existing_remote_branches.insert("finish/hotfix-1.1.1-into-develop".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    git.ancestors.insert(("hotfix/1.1.1".to_string(), "origin/finish/hotfix-1.1.1-into-main".to_string()));
    git.ancestors.insert(("hotfix/1.1.1".to_string(), "origin/finish/hotfix-1.1.1-into-develop".to_string()));
    git.branches_matching_by.insert("release/*".to_string(), vec![]);
    git.branches_matching_by.insert("finish/hotfix-1.1.1-into-*".to_string(), vec![
        "finish/hotfix-1.1.1-into-develop".to_string(),
        "finish/hotfix-1.1.1-into-main".to_string(),
        "finish/hotfix-1.1.1-into-release-1.9.0".to_string(),
    ]);
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("finish/hotfix-1.1.1-into-main".to_string(), "main".to_string()), landed("res-main", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("finish/hotfix-1.1.1-into-develop".to_string(), "develop".to_string()), landed("res-dev", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"delete_branch_local:hotfix/1.1.1".to_string()),
        "resolved conflicts must not block source deletion; calls: {calls:?}");
    assert!(calls.contains(&"list_branches_matching:finish/hotfix-1.1.1-into-*".to_string()), "calls: {calls:?}");
    for finish in ["finish/hotfix-1.1.1-into-develop", "finish/hotfix-1.1.1-into-main", "finish/hotfix-1.1.1-into-release-1.9.0"] {
        assert!(calls.contains(&format!("local_branch_exists:{finish}")),
            "every finish branch (orphans included) is cleaned; calls: {calls:?}");
    }
    let guard_read = calls.iter().position(|c| c == "is_ancestor:hotfix/1.1.1:origin/finish/hotfix-1.1.1-into-main").unwrap();
    let finish_delete = calls.iter().position(|c| c == "local_branch_exists:finish/hotfix-1.1.1-into-develop").unwrap();
    let source_delete = calls.iter().position(|c| c == "delete_branch_local:hotfix/1.1.1").unwrap();
    assert!(guard_read < finish_delete, "the tip-landed guard reads finish refs before they vanish; calls: {calls:?}");
    assert!(finish_delete < source_delete, "finish branches go before source cleanup, whose worktree removal must be the flow's last git call; calls: {calls:?}");
}

#[test]
fn protected_hotfix_falls_back_to_local_finish_refs_when_origin_pruned_them() {
    // The platform auto-deleted the merged finish branches and fetch --prune
    // removed origin/finish/*; only the local refs still prove the resolved
    // landings contain the tip.
    let mut git = MockGit::new();
    git.current_branch = "hotfix/1.1.1".to_string();
    git.branch_shas.insert("hotfix/1.1.1".to_string(), "hfsha".to_string());
    git.existing_tags.insert("v1.1.1".to_string());
    git.tag_commits.insert("v1.1.1".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.1".to_string());
    git.existing_local_branches.insert("hotfix/1.1.1".to_string());
    git.existing_remote_branches.insert("hotfix/1.1.1".to_string());
    git.existing_local_branches.insert("finish/hotfix-1.1.1-into-develop".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    git.ancestors.insert(("hotfix/1.1.1".to_string(), "finish/hotfix-1.1.1-into-develop".to_string()));
    git.branches_matching_by.insert("release/*".to_string(), vec![]);
    git.branches_matching_by.insert("finish/hotfix-1.1.1-into-*".to_string(), vec![
        "finish/hotfix-1.1.1-into-develop".to_string(),
    ]);
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("finish/hotfix-1.1.1-into-main".to_string(), "main".to_string()), landed("res-main", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("finish/hotfix-1.1.1-into-develop".to_string(), "develop".to_string()), landed("res-dev", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"is_ancestor:hotfix/1.1.1:finish/hotfix-1.1.1-into-develop".to_string()),
        "the local ref must prove containment when origin is pruned; calls: {calls:?}");
    assert!(calls.contains(&"delete_branch_local:hotfix/1.1.1".to_string()), "calls: {calls:?}");
}

#[test]
fn protected_hotfix_in_its_own_worktree_removes_the_worktree_last() {
    // The process stands in the hotfix's linked worktree: removing it destroys
    // the process cwd, so every other git call — finish-branch cleanup
    // included — must have happened before.
    let mut git = MockGit::new();
    git.current_branch = "hotfix/1.1.1".to_string();
    git.linked_worktree = true;
    git.branch_shas.insert("hotfix/1.1.1".to_string(), "hfsha".to_string());
    git.existing_tags.insert("v1.1.1".to_string());
    git.tag_commits.insert("v1.1.1".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.1".to_string());
    git.existing_local_branches.insert("hotfix/1.1.1".to_string());
    git.existing_remote_branches.insert("hotfix/1.1.1".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("mc2".to_string(), "origin/develop".to_string()));
    git.branches_matching_by.insert("release/*".to_string(), vec![]);
    git.branches_matching_by.insert("finish/hotfix-1.1.1-into-*".to_string(), vec![
        "finish/hotfix-1.1.1-into-develop".to_string(),
        "finish/hotfix-1.1.1-into-main".to_string(),
    ]);
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("finish/hotfix-1.1.1-into-main".to_string(), "main".to_string()), landed("hfsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);
    hosting.merged_prs_to.insert(("finish/hotfix-1.1.1-into-develop".to_string(), "develop".to_string()), landed("hfsha", "mc2"));
    git.parent_counts.insert("mc2".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 1, 1, "main", None, false).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"list_branches_matching:finish/hotfix-1.1.1-into-*".to_string()),
        "finish branches are still cleaned; calls: {calls:?}");
    assert_eq!(calls.last().map(String::as_str), Some("remove_current_worktree"),
        "no git call may follow worktree removal — the process cwd is gone; calls: {calls:?}");
}

#[test]
fn protected_hotfix_content_present_legs_complete_without_prs() {
    // develop and the open release already contain the hotfix (manual merges,
    // or landed PRs the platform lost track of): both legs skip PR machinery
    // entirely and the finish still completes.
    let mut git = MockGit::new();
    git.current_branch = "hotfix/1.1.1".to_string();
    git.branch_shas.insert("hotfix/1.1.1".to_string(), "hfsha".to_string());
    git.existing_tags.insert("v1.1.1".to_string());
    git.tag_commits.insert("v1.1.1".to_string(), "mc1".to_string());
    git.existing_remote_tags.insert("v1.1.1".to_string());
    git.existing_local_branches.insert("hotfix/1.1.1".to_string());
    git.existing_remote_branches.insert("hotfix/1.1.1".to_string());
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    git.ancestors.insert(("hotfix/1.1.1".to_string(), "origin/develop".to_string()));
    git.ancestors.insert(("hotfix/1.1.1".to_string(), "origin/release/1.2.0".to_string()));
    git.branches_matching_by.insert("release/*".to_string(), vec!["release/1.2.0".to_string()]);
    git.branches_matching_by.insert("finish/hotfix-1.1.1-into-*".to_string(), vec![]);
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(("hotfix/1.1.1".to_string(), "main".to_string()), landed("hfsha", "mc1"));
    git.parent_counts.insert("mc1".to_string(), 2);

    finish_hotfix(&git, &hosting, &protected_cfg(false), 1, 1, 1, "main", None, false).unwrap();

    assert!(!hosting.calls().iter().any(|c| c.starts_with("create_or_get_pr")),
        "content-present legs need no PR; calls: {:?}", hosting.calls());
    let calls = git.calls();
    assert!(calls.contains(&"delete_branch_local:hotfix/1.1.1".to_string()), "calls: {calls:?}");
}

#[test]
fn hotfix_cleanup_with_unlanded_tip_keeps_the_branch() {
    // Last-resort guard behind the strict legs (mirror of the release side):
    // completing with an unlanded tip must warn and keep the branch.
    use gflow::flows::finish_hotfix::finish_hotfix_cleanup;
    use gflow::version::SemVer;
    let git = MockGit::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    finish_hotfix_cleanup(&git, &cfg, "hotfix/1.0.1", "main", &SemVer::new(1, 0, 1), &[], false).unwrap();

    assert!(!git.calls().iter().any(|c| c.starts_with("delete_branch")),
        "an unlanded tip must never be deleted; calls: {:?}", git.calls());
}

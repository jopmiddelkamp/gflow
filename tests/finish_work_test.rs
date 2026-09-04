mod common;

use common::{MockGit, MockHosting, MockPrompter};
use gflow::flows::finish_work::{finish_release_fix, finish_hotfix_fix, finish_release_chore, finish_work_branch};
use gflow::git::branch::BranchType;

#[test]
fn finish_release_fix_pushes_and_creates_pr() {
    let mut git = MockGit::new();
    git.current_branch = "release-fix/1.1.0/login-bug".to_string();
    let hosting = MockHosting::new();
    let branch_type = BranchType::ReleaseFix { major: 1, minor: 1, patch: 0, name: "login-bug".to_string() };

    finish_release_fix(&git, &hosting, &branch_type, None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "push:release-fix/1.1.0/login-bug",
    ]);

    assert_eq!(hosting.calls(), vec![
        "merged_pr:release-fix/1.1.0/login-bug",
        "create_or_get_pr:release-fix/1.1.0/login-bug:release/1.1.0:fix: login bug",
        "copy_text:fix: login bug\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

#[test]
fn a_failed_clipboard_copy_never_fails_the_finish() {
    // The clipboard is a bonus on top of the printed block: a machine without
    // a clipboard tool (headless CI) silently skips it.
    let mut git = MockGit::new();
    git.current_branch = "release-fix/1.1.0/login-bug".to_string();
    let mut hosting = MockHosting::new();
    hosting.copy_text_error = Some("no clipboard tool".to_string());
    let branch_type = BranchType::ReleaseFix { major: 1, minor: 1, patch: 0, name: "login-bug".to_string() };

    finish_release_fix(&git, &hosting, &branch_type, None, false).unwrap();

    assert!(hosting.calls().iter().any(|c| c.starts_with("copy_text:")),
        "the copy must still be attempted; calls: {:?}", hosting.calls());
}

#[test]
fn a_failed_browser_open_never_fails_the_finish() {
    // The PR exists and its URL is printed by the time the browser opens, so a
    // headless environment (CI, no xdg-open) gets a warning, not an error.
    let mut git = MockGit::new();
    git.current_branch = "release-fix/1.1.0/login-bug".to_string();
    let mut hosting = MockHosting::new();
    hosting.open_url_error = Some("Failed to open URL: no browser".to_string());
    let branch_type = BranchType::ReleaseFix { major: 1, minor: 1, patch: 0, name: "login-bug".to_string() };

    finish_release_fix(&git, &hosting, &branch_type, None, false).unwrap();

    assert!(hosting.calls().contains(&"open_url:https://github.com/org/repo/pull/1".to_string()),
        "the open must still be attempted; calls: {:?}", hosting.calls());
}

#[test]
fn finish_hotfix_fix_pushes_and_creates_pr() {
    let mut git = MockGit::new();
    git.current_branch = "hotfix-fix/1.0.1/crash-fix".to_string();
    let hosting = MockHosting::new();
    let branch_type = BranchType::HotfixFix { major: 1, minor: 0, patch: 1, name: "crash-fix".to_string() };

    finish_hotfix_fix(&git, &hosting, &branch_type, None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "push:hotfix-fix/1.0.1/crash-fix",
    ]);

    assert_eq!(hosting.calls(), vec![
        "merged_pr:hotfix-fix/1.0.1/crash-fix",
        "create_or_get_pr:hotfix-fix/1.0.1/crash-fix:hotfix/1.0.1:fix: crash fix",
        "copy_text:fix: crash fix\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

#[test]
fn finish_release_fix_with_custom_pr_url() {
    let mut git = MockGit::new();
    git.current_branch = "release-fix/2.0.0/typo".to_string();
    let mut hosting = MockHosting::new();
    hosting.pr_url = "https://github.com/org/repo/pull/42".to_string();
    let branch_type = BranchType::ReleaseFix { major: 2, minor: 0, patch: 0, name: "typo".to_string() };

    finish_release_fix(&git, &hosting, &branch_type, None, false).unwrap();

    assert_eq!(hosting.calls(), vec![
        "merged_pr:release-fix/2.0.0/typo",
        "create_or_get_pr:release-fix/2.0.0/typo:release/2.0.0:fix: typo",
        "copy_text:fix: typo\nhttps://github.com/org/repo/pull/42",
        "open_url:https://github.com/org/repo/pull/42",
    ]);
}

#[test]
fn finish_release_chore_pushes_and_creates_pr() {
    let mut git = MockGit::new();
    git.current_branch = "release-chore/1.1.0/set-version".to_string();
    let hosting = MockHosting::new();
    let branch_type = BranchType::ReleaseChore { major: 1, minor: 1, patch: 0, name: "set-version".to_string() };

    finish_release_chore(&git, &hosting, &branch_type, None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "push:release-chore/1.1.0/set-version",
    ]);

    assert_eq!(hosting.calls(), vec![
        "merged_pr:release-chore/1.1.0/set-version",
        "create_or_get_pr:release-chore/1.1.0/set-version:release/1.1.0:chore: set version",
        "copy_text:chore: set version\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

// --- finish_work_branch tests ---

#[test]
fn finish_work_branch_feature_non_breaking() {
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "login".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), None, None, false).unwrap();

    let calls = hosting.calls();
    assert!(calls[1].starts_with("create_or_get_pr:feature/login:"));
    assert!(calls[1].ends_with(":feat: login"));
}

#[test]
fn finish_work_branch_feature_breaking() {
    let mut git = MockGit::new();
    git.current_branch = "feature/remove-api".to_string();
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "remove-api".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(true), None, None, false).unwrap();

    let calls = hosting.calls();
    assert!(calls[1].ends_with(":feat!: remove api"),
        "Expected PR title to end with 'feat!: remove api', got: {}", calls[1]);
}

#[test]
fn finish_work_branch_chore_breaking_honored() {
    let mut git = MockGit::new();
    git.current_branch = "chore/drop-node-16".to_string();
    let hosting = MockHosting::new();
    let branch_type = BranchType::Chore { name: "drop-node-16".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(true), None, None, false).unwrap();

    let calls = hosting.calls();
    assert!(calls[1].ends_with(":chore!: drop node 16"),
        "Explicit --breaking should be honored on chore, got: {}", calls[1]);
}

#[test]
fn finish_work_branch_docs_defaults_to_non_breaking() {
    let mut git = MockGit::new();
    git.current_branch = "docs/readme".to_string();
    let hosting = MockHosting::new();
    let branch_type = BranchType::Docs { name: "readme".to_string() };

    // No flag (None) — docs should NOT prompt, should default to non-breaking
    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, None, None, None, false).unwrap();

    let calls = hosting.calls();
    assert!(calls[1].ends_with(":docs: readme"),
        "Docs with None should default to non-breaking, got: {}", calls[1]);
}

#[test]
fn finish_work_branch_with_explicit_base_skips_detection() {
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    git.existing_remote_branches.insert("develop".to_string());
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "login".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), Some("develop".to_string()), None, false).unwrap();

    let git_calls = git.calls();
    assert!(!git_calls.contains(&"list_remote_branches".to_string()),
        "Explicit --base must skip parent detection, got: {git_calls:?}");
    let calls = hosting.calls();
    assert!(calls[1].starts_with("create_or_get_pr:feature/login:develop:"),
        "PR should target the explicit base, got: {}", calls[1]);
}

#[test]
fn finish_work_branch_with_local_only_base_errors() {
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    // Base exists locally but was never pushed: PR creation would fail on GitHub,
    // so gflow must reject it up-front instead of pushing and then failing.
    git.existing_local_branches.insert("feature/auth".to_string());
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "login".to_string() };

    let err = finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), Some("feature/auth".to_string()), None, false).unwrap_err();

    assert!(err.contains("feature/auth") && err.contains("origin"),
        "Error should name the branch and origin, got: {err}");
    assert!(!git.calls().iter().any(|c| c.starts_with("push:")),
        "Nothing should be pushed for an invalid base");
    assert!(hosting.calls().is_empty(), "No PR should be created for a local-only base");
}

#[test]
fn finish_work_branch_with_base_equal_to_current_errors() {
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    // The current branch trivially "exists", so without a dedicated guard this
    // would pass validation and fail later at `gh pr create` with head == base.
    git.existing_remote_branches.insert("feature/login".to_string());
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "login".to_string() };

    let err = finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), Some("feature/login".to_string()), None, false).unwrap_err();

    assert!(err.contains("feature/login"), "Error should name the branch, got: {err}");
    assert!(hosting.calls().is_empty(), "No PR should be created when base == current");
}

#[test]
fn finish_work_branch_with_unknown_base_errors() {
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "login".to_string() };

    let err = finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), Some("no-such-branch".to_string()), None, false).unwrap_err();

    assert!(err.contains("no-such-branch"), "Error should name the missing branch, got: {err}");
    assert!(hosting.calls().is_empty(), "No PR should be created for an unknown base");
}

#[test]
fn finish_work_branch_single_candidate_finishes_without_menu() {
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    // Exactly one candidate parent on the remote; equal rev-list counts keep it.
    git.remote_branches = vec!["develop".to_string()];
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "login".to_string() };

    // Passing proves show_select was never reached: it has no TTY here.
    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), None, None, false).unwrap();

    let calls = hosting.calls();
    assert!(calls[1].starts_with("create_or_get_pr:feature/login:develop:"),
        "Single candidate should be auto-selected, got: {}", calls[1]);
}

#[test]
fn finish_work_branch_fix_breaking() {
    let mut git = MockGit::new();
    git.current_branch = "fix/auth".to_string();
    let hosting = MockHosting::new();
    let branch_type = BranchType::Fix { name: "auth".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(true), None, None, false).unwrap();

    let calls = hosting.calls();
    assert!(calls[1].ends_with(":fix!: auth"),
        "Expected 'fix!: auth', got: {}", calls[1]);
}

#[test]
fn finish_work_branch_passes_resolved_template_to_hosting() {
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "login".to_string() };
    let template = std::path::Path::new(".github/pr-templates/gflow-feature.md");

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), None, Some(template), false).unwrap();

    let calls = hosting.calls();
    assert!(calls[1].ends_with(":template=.github/pr-templates/gflow-feature.md"),
        "template path must reach the hosting platform verbatim, got: {}", calls[1]);
}

// --- Parent-branch candidate ordering (reachable now that prompting goes
// --- through the Prompter port; previously required a TTY) ---

/// Wire up a candidate: merge base with `current`, our distance since
/// divergence, and the candidate's own commit count since divergence.
fn add_candidate(git: &mut MockGit, current: &str, branch: &str, base: &str, ours: u32, theirs: u32) {
    git.merge_bases.insert((current.to_string(), branch.to_string()), base.to_string());
    git.rev_list_counts.insert((base.to_string(), current.to_string()), ours);
    git.rev_list_counts.insert((base.to_string(), branch.to_string()), theirs);
}

#[test]
fn parent_candidates_sorted_by_merge_distance_ascending() {
    let mut git = MockGit::new();
    git.current_branch = "feature/child".to_string();
    git.remote_branches = vec!["develop".to_string(), "feature/near".to_string()];
    // develop diverged 5 commits ago, feature/near only 2 — nearest first.
    add_candidate(&mut git, "feature/child", "develop", "base-d", 5, 0);
    add_candidate(&mut git, "feature/child", "feature/near", "base-n", 2, 0);
    let hosting = MockHosting::new();
    let prompter = MockPrompter::scripted(&[0]);
    let branch_type = BranchType::Feature { name: "child".to_string() };

    finish_work_branch(&git, &hosting, &prompter, &branch_type, Some(false), None, None, false).unwrap();

    assert_eq!(prompter.calls(), vec!["select:PR target branch:[feature/near, develop]"]);
    assert!(hosting.calls()[1].starts_with("create_or_get_pr:feature/child:feature/near:"),
        "choosing index 0 must target the nearest candidate, got: {}", hosting.calls()[1]);
}

#[test]
fn parent_candidates_tie_prefers_develop_then_alphabetical() {
    let mut git = MockGit::new();
    git.current_branch = "feature/child".to_string();
    git.remote_branches = vec![
        "feature/bbb".to_string(),
        "develop".to_string(),
        "feature/aaa".to_string(),
    ];
    // All three candidates at the same distance.
    add_candidate(&mut git, "feature/child", "feature/bbb", "base-b", 3, 0);
    add_candidate(&mut git, "feature/child", "develop", "base-d", 3, 0);
    add_candidate(&mut git, "feature/child", "feature/aaa", "base-a", 3, 0);
    let hosting = MockHosting::new();
    let prompter = MockPrompter::scripted(&[0]);
    let branch_type = BranchType::Feature { name: "child".to_string() };

    finish_work_branch(&git, &hosting, &prompter, &branch_type, Some(false), None, None, false).unwrap();

    assert_eq!(prompter.calls(),
        vec!["select:PR target branch:[develop, feature/aaa, feature/bbb]"]);
}

#[test]
fn parent_detection_excludes_child_branches_and_skips_menu_for_single_candidate() {
    let mut git = MockGit::new();
    git.current_branch = "feature/parent".to_string();
    git.remote_branches = vec!["develop".to_string(), "feature/stacked".to_string()];
    add_candidate(&mut git, "feature/parent", "develop", "base-d", 4, 0);
    // feature/stacked branched from us: its merge base with us IS our tip, so
    // nothing of ours is missing from it while it carries 6 commits of its
    // own. It must not be offered as a PR target.
    add_candidate(&mut git, "feature/parent", "feature/stacked", "our-tip", 0, 6);
    let hosting = MockHosting::new();
    let prompter = MockPrompter::new(); // unscripted: any select would error
    let branch_type = BranchType::Feature { name: "parent".to_string() };

    finish_work_branch(&git, &hosting, &prompter, &branch_type, Some(false), None, None, false).unwrap();

    assert!(prompter.calls().is_empty(), "single surviving candidate must be auto-selected");
    assert!(hosting.calls()[1].starts_with("create_or_get_pr:feature/parent:develop:"),
        "child branch must be excluded, got: {}", hosting.calls()[1]);
}

#[test]
fn busy_develop_stays_a_candidate_even_when_far_ahead_of_us() {
    let mut git = MockGit::new();
    git.current_branch = "refactor/cleanup".to_string();
    git.remote_branches = vec!["develop".to_string(), "feature/sibling".to_string()];
    // develop moved 20 commits ahead (teammates' merges) while we made 3 —
    // being "ahead" must not mark our own base branch as a child of ours.
    add_candidate(&mut git, "refactor/cleanup", "develop", "base-d", 3, 20);
    // A sibling forked from an older develop: our count since that older base
    // is large, its own is small — the shape that used to out-rank develop.
    add_candidate(&mut git, "refactor/cleanup", "feature/sibling", "base-s", 15, 2);
    let hosting = MockHosting::new();
    let prompter = MockPrompter::scripted(&[0]);
    let branch_type = BranchType::Refactor { name: "cleanup".to_string() };

    finish_work_branch(&git, &hosting, &prompter, &branch_type, Some(false), None, None, false).unwrap();

    assert_eq!(prompter.calls(), vec!["select:PR target branch:[develop, feature/sibling]"]);
    assert!(hosting.calls()[1].starts_with("create_or_get_pr:refactor/cleanup:develop:"),
        "develop must be selectable as PR target, got: {}", hosting.calls()[1]);
}

#[test]
fn develop_is_offered_even_when_it_already_contains_our_tip() {
    let mut git = MockGit::new();
    git.current_branch = "fix/already-merged".to_string();
    git.remote_branches = vec!["develop".to_string(), "feature/sibling".to_string()];
    // develop was merged/fast-forwarded outside a PR the host reports as
    // merged, so it contains our tip: the child-branch shape, but develop is
    // never a child of a work branch and must stay selectable.
    add_candidate(&mut git, "fix/already-merged", "develop", "our-tip", 0, 5);
    add_candidate(&mut git, "fix/already-merged", "feature/sibling", "base-s", 4, 1);
    let hosting = MockHosting::new();
    let prompter = MockPrompter::scripted(&[0]);
    let branch_type = BranchType::Fix { name: "already-merged".to_string() };

    finish_work_branch(&git, &hosting, &prompter, &branch_type, Some(false), None, None, false).unwrap();

    assert_eq!(prompter.calls(), vec!["select:PR target branch:[develop, feature/sibling]"]);
}

#[test]
fn machine_owned_chore_set_version_branch_is_never_offered_as_pr_target() {
    // chore/set-version-* branches are gflow-created and get merged/deleted by
    // a protected-mode version bump; targeting one as a PR base would strand
    // the PR when that branch disappears out from under it.
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    git.remote_branches = vec!["chore/set-version-1.2.0".to_string(), "develop".to_string()];
    let hosting = MockHosting::new();
    let prompter = MockPrompter::new(); // unscripted: any select would error
    let branch_type = BranchType::Feature { name: "login".to_string() };

    finish_work_branch(&git, &hosting, &prompter, &branch_type, Some(false), None, None, false).unwrap();

    assert!(prompter.calls().is_empty(),
        "chore/set-version-* must be filtered, leaving develop as the sole/auto candidate");
    assert!(hosting.calls()[1].starts_with("create_or_get_pr:feature/login:develop:"),
        "chore/set-version-1.2.0 must never be offered as a PR target, got: {}", hosting.calls()[1]);
}

// --- Already-merged PR: finish is complete, clean up instead of a new PR ---

use gflow::hosting::MergedPr;

fn merged(url: &str, sha: &str, base: &str) -> Option<MergedPr> {
    Some(MergedPr { url: url.to_string(), head_sha: sha.to_string(), merge_commit_sha: format!("merge-of-{sha}"), base: base.to_string() })
}

#[test]
fn a_work_pr_completed_with_a_merge_commit_refuses_cleanup() {
    // Work PRs must be squashed. A 2-parent merge commit on the target means
    // the wrong button was used — the finish hard-stops with recovery steps
    // instead of cleaning up (the branch is still needed to redo the PR).
    let mut git = MockGit::new();
    git.current_branch = "feature/task-a".to_string();
    git.head_sha = "abc123".to_string();
    git.parent_counts.insert("merge-of-abc123".to_string(), 2);
    let mut hosting = MockHosting::new();
    hosting.merged_pr = merged("https://github.com/org/repo/pull/49", "abc123", "develop");
    let branch_type = BranchType::Feature { name: "task-a".to_string() };

    let err = finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, None, None, None, false).unwrap_err();

    assert!(err.contains("MERGE COMMIT"), "must name the actual type; got: {err}");
    assert!(err.contains("SQUASH"), "must name the expected type; got: {err}");
    assert!(err.contains("https://github.com/org/repo/pull/49"), "must name the PR; got: {err}");
    assert!(err.contains("git commit --amend --no-edit"), "must name the redo recipe; got: {err}");
    assert!(err.contains("--accept-merge-type"), "must name the override; got: {err}");
    assert!(!git.calls().iter().any(|c| c.starts_with("delete_") || c == "remove_current_worktree"),
        "nothing may be cleaned up; got: {:?}", git.calls());
    assert_eq!(hosting.calls(), vec!["merged_pr:feature/task-a"], "no new PR may be created");
}

#[test]
fn accept_merge_type_permits_cleanup_of_a_wrongly_completed_pr() {
    let mut git = MockGit::new();
    git.current_branch = "feature/task-a".to_string();
    git.head_sha = "abc123".to_string();
    git.parent_counts.insert("merge-of-abc123".to_string(), 2);
    let mut hosting = MockHosting::new();
    hosting.merged_pr = merged("https://github.com/org/repo/pull/49", "abc123", "develop");
    let branch_type = BranchType::Feature { name: "task-a".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, None, None, None, true).unwrap();

    assert!(git.calls().contains(&"delete_branch_local:feature/task-a".to_string()),
        "accepting the mistake finishes the branch; got: {:?}", git.calls());
}

#[test]
fn merged_pr_in_worktree_cleans_up_branch_and_worktree() {
    let mut git = MockGit::new();
    git.current_branch = "feature/task-a".to_string();
    git.head_sha = "abc123".to_string();
    git.parent_counts.insert("merge-of-abc123".to_string(), 1);
    git.linked_worktree = true;
    git.existing_remote_branches.insert("feature/task-a".to_string());
    let mut hosting = MockHosting::new();
    hosting.merged_pr = merged("https://github.com/org/repo/pull/49", "abc123", "develop");
    let branch_type = BranchType::Feature { name: "task-a".to_string() };

    // breaking=None + unscripted prompter: passing proves the breaking-changes
    // prompt (and parent detection) never ran on an already-finished branch.
    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, None, None, None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "head_sha",
        "commit_parent_count:merge-of-abc123",
        // Remote deletion comes first: after remove_current_worktree the process
        // cwd is gone, so it must be the last git call.
        "remote_branch_exists:feature/task-a",
        "delete_branch_remote:feature/task-a",
        "is_linked_worktree",
        "detach_head",
        "delete_branch_local:feature/task-a",
        "remove_current_worktree",
    ]);
    assert_eq!(hosting.calls(), vec!["merged_pr:feature/task-a"], "no new PR may be created");
}

#[test]
fn merged_pr_in_plain_checkout_returns_to_base_and_deletes_branch() {
    let mut git = MockGit::new();
    git.current_branch = "feature/task-a".to_string();
    git.head_sha = "abc123".to_string();
    git.parent_counts.insert("merge-of-abc123".to_string(), 1);
    // Remote branch already auto-deleted by the platform after merge.
    let mut hosting = MockHosting::new();
    hosting.merged_pr = merged("https://github.com/org/repo/pull/49", "abc123", "develop");
    let branch_type = BranchType::Feature { name: "task-a".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, None, None, None, false).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "head_sha",
        "commit_parent_count:merge-of-abc123",
        "remote_branch_exists:feature/task-a",
        "is_linked_worktree",
        "checkout:develop",
        "ff_merge:origin/develop",
        "delete_branch_local:feature/task-a",
    ]);
    assert_eq!(hosting.calls(), vec!["merged_pr:feature/task-a"]);
}

#[test]
fn merged_pr_with_new_commits_since_merge_creates_a_new_pr() {
    let mut git = MockGit::new();
    git.current_branch = "feature/task-a".to_string();
    git.head_sha = "newwork456".to_string();
    let mut hosting = MockHosting::new();
    hosting.merged_pr = merged("https://github.com/org/repo/pull/49", "abc123", "develop");
    let branch_type = BranchType::Feature { name: "task-a".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), None, None, false).unwrap();

    // New commits after the merge = new work: nothing may be deleted...
    assert!(!git.calls().iter().any(|c| c.starts_with("delete_") || c == "remove_current_worktree"),
        "diverged branch must not be cleaned up, got: {:?}", git.calls());
    // ...and the flow continues into a fresh PR.
    assert!(hosting.calls().iter().any(|c| c.starts_with("create_or_get_pr:feature/task-a:")),
        "a new PR should be created, got: {:?}", hosting.calls());
}

#[test]
fn merged_pr_cleans_up_release_fix_too() {
    let mut git = MockGit::new();
    git.current_branch = "release-fix/1.1.0/login-bug".to_string();
    git.head_sha = "abc123".to_string();
    git.parent_counts.insert("merge-of-abc123".to_string(), 1);
    git.linked_worktree = true;
    let mut hosting = MockHosting::new();
    hosting.merged_pr = merged("https://github.com/org/repo/pull/50", "abc123", "release/1.1.0");
    let branch_type = BranchType::ReleaseFix { major: 1, minor: 1, patch: 0, name: "login-bug".to_string() };

    finish_release_fix(&git, &hosting, &branch_type, None, false).unwrap();

    assert!(!git.calls().iter().any(|c| c.starts_with("push:")), "nothing to push on a finished branch");
    assert!(git.calls().contains(&"remove_current_worktree".to_string()));
    assert_eq!(hosting.calls(), vec!["merged_pr:release-fix/1.1.0/login-bug"]);
}

#[test]
fn merged_pr_cleans_up_release_chore_too() {
    let mut git = MockGit::new();
    git.current_branch = "release-chore/1.1.0/set-version".to_string();
    git.head_sha = "abc123".to_string();
    git.parent_counts.insert("merge-of-abc123".to_string(), 1);
    git.linked_worktree = true;
    let mut hosting = MockHosting::new();
    hosting.merged_pr = merged("https://github.com/org/repo/pull/50", "abc123", "release/1.1.0");
    let branch_type = BranchType::ReleaseChore { major: 1, minor: 1, patch: 0, name: "set-version".to_string() };

    finish_release_chore(&git, &hosting, &branch_type, None, false).unwrap();

    assert!(!git.calls().iter().any(|c| c.starts_with("push:")), "nothing to push on a finished branch");
    assert!(git.calls().contains(&"remove_current_worktree".to_string()));
    assert_eq!(hosting.calls(), vec!["merged_pr:release-chore/1.1.0/set-version"]);
}

// --- Cleanup: warn-and-continue when the work already succeeded ---

#[test]
fn stale_base_after_merge_warns_but_still_finishes() {
    // Error Model: "Warn-and-continue only when the work already succeeded."
    // The PR is merged on the remote; a local base that won't fast-forward is
    // the user's problem to sort out later, not a failed finish.
    let mut git = MockGit::new();
    git.current_branch = "feature/task-a".to_string();
    git.head_sha = "abc123".to_string();
    git.parent_counts.insert("merge-of-abc123".to_string(), 1);
    git.ff_merge_error = Some("fatal: Not possible to fast-forward, aborting.".to_string());
    let mut hosting = MockHosting::new();
    hosting.merged_pr = merged("https://github.com/org/repo/pull/49", "abc123", "develop");
    let branch_type = BranchType::Feature { name: "task-a".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, None, None, None, false).unwrap();

    assert!(git.calls().contains(&"delete_branch_local:feature/task-a".to_string()),
        "cleanup must continue past the failed fast-forward; calls: {:?}", git.calls());
}

// --- Parent detection: a candidate we cannot measure is skipped, never fatal ---

#[test]
fn the_current_branch_is_never_its_own_parent() {
    let mut git = MockGit::new();
    git.current_branch = "feature/child".to_string();
    git.remote_branches = vec!["feature/child".to_string()];
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "child".to_string() };

    // Unscripted prompter: no menu may appear, and with no other candidate the
    // fallback target is develop.
    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), None, None, false).unwrap();

    assert!(hosting.calls()[1].starts_with("create_or_get_pr:feature/child:develop:"),
        "got: {}", hosting.calls()[1]);
}

#[test]
fn a_candidate_with_no_common_ancestor_is_skipped() {
    let mut git = MockGit::new();
    git.current_branch = "feature/child".to_string();
    git.remote_branches = vec!["develop".to_string(), "feature/unrelated".to_string()];
    add_candidate(&mut git, "feature/child", "develop", "base-d", 3, 0);
    git.fail_merge_base_for = vec!["feature/unrelated".to_string()];
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "child".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), None, None, false).unwrap();

    assert!(hosting.calls()[1].starts_with("create_or_get_pr:feature/child:develop:"),
        "the unmeasurable branch drops out, leaving develop as the single auto-detected parent; got: {}",
        hosting.calls()[1]);
}

#[test]
fn a_candidate_is_skipped_when_our_own_distance_cannot_be_counted() {
    let mut git = MockGit::new();
    git.current_branch = "feature/child".to_string();
    git.remote_branches = vec!["develop".to_string(), "feature/broken".to_string()];
    add_candidate(&mut git, "feature/child", "develop", "base-d", 3, 0);
    git.merge_bases.insert(("feature/child".to_string(), "feature/broken".to_string()), "base-x".to_string());
    // Counting base-x..feature/child fails — we cannot rank this candidate.
    git.fail_rev_list_count_for = vec!["feature/child".to_string()];
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "child".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), None, None, false).unwrap();

    assert!(hosting.calls()[1].starts_with("create_or_get_pr:feature/child:develop:"),
        "got: {}", hosting.calls()[1]);
}

#[test]
fn a_candidate_is_skipped_when_its_own_distance_cannot_be_counted() {
    let mut git = MockGit::new();
    git.current_branch = "feature/child".to_string();
    git.remote_branches = vec!["develop".to_string(), "feature/broken".to_string()];
    add_candidate(&mut git, "feature/child", "develop", "base-d", 3, 0);
    git.merge_bases.insert(("feature/child".to_string(), "feature/broken".to_string()), "base-x".to_string());
    git.rev_list_counts.insert(("base-x".to_string(), "feature/child".to_string()), 2);
    git.fail_rev_list_count_for = vec!["feature/broken".to_string()];
    let hosting = MockHosting::new();
    let branch_type = BranchType::Feature { name: "child".to_string() };

    finish_work_branch(&git, &hosting, &MockPrompter::new(), &branch_type, Some(false), None, None, false).unwrap();

    assert!(hosting.calls()[1].starts_with("create_or_get_pr:feature/child:develop:"),
        "got: {}", hosting.calls()[1]);
}

// --- Breaking-change prompt (only when --breaking was omitted) ---

#[test]
fn omitted_breaking_flag_prompts_for_commonly_breaking_types() {
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    git.remote_branches = vec!["develop".to_string()];
    let hosting = MockHosting::new();
    let prompter = MockPrompter::scripted(&[1]); // "yes"
    let branch_type = BranchType::Feature { name: "login".to_string() };

    finish_work_branch(&git, &hosting, &prompter, &branch_type, None, None, None, false).unwrap();

    assert_eq!(prompter.calls(), vec!["select:Contains breaking changes?:[no, yes]"]);
    assert!(hosting.calls()[1].contains("feat!: login"),
        "answering yes must mark the PR title breaking; got: {}", hosting.calls()[1]);
}

#[test]
fn answering_no_to_the_breaking_prompt_leaves_the_title_plain() {
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    git.remote_branches = vec!["develop".to_string()];
    let hosting = MockHosting::new();
    let prompter = MockPrompter::scripted(&[0]);
    let branch_type = BranchType::Feature { name: "login".to_string() };

    finish_work_branch(&git, &hosting, &prompter, &branch_type, None, None, None, false).unwrap();

    assert!(hosting.calls()[1].contains("feat: login"), "got: {}", hosting.calls()[1]);
}

// --- Branch-type guards on the fix finishes ---

#[test]
fn finish_release_fix_rejects_a_branch_that_is_not_a_release_fix() {
    let git = MockGit::new();
    let hosting = MockHosting::new();

    let err = finish_release_fix(&git, &hosting, &BranchType::Develop, None, false).unwrap_err();

    assert_eq!(err, "Cannot finish: not on a release-fix branch");
    assert!(git.calls().is_empty(), "the guard runs before any git call; calls: {:?}", git.calls());
}

#[test]
fn finish_release_chore_rejects_a_branch_that_is_not_a_release_chore() {
    let git = MockGit::new();
    let hosting = MockHosting::new();

    let err = finish_release_chore(&git, &hosting, &BranchType::Develop, None, false).unwrap_err();

    assert_eq!(err, "Cannot finish: not on a release-chore branch");
    assert!(git.calls().is_empty(), "the guard runs before any git call; calls: {:?}", git.calls());
}

#[test]
fn finish_hotfix_fix_rejects_a_branch_that_is_not_a_hotfix_fix() {
    let git = MockGit::new();
    let hosting = MockHosting::new();

    let err = finish_hotfix_fix(&git, &hosting, &BranchType::Main, None, false).unwrap_err();

    assert_eq!(err, "Cannot finish: not on a hotfix-fix branch");
    assert!(git.calls().is_empty(), "calls: {:?}", git.calls());
}

#[test]
fn merged_pr_cleans_up_hotfix_fix_too() {
    // Cleanup is the same derived-completion path for every fix family.
    let mut git = MockGit::new();
    git.current_branch = "hotfix-fix/2.5.1/npe".to_string();
    git.head_sha = "abc123".to_string();
    git.parent_counts.insert("merge-of-abc123".to_string(), 1);
    let mut hosting = MockHosting::new();
    hosting.merged_pr = merged("https://github.com/org/repo/pull/50", "abc123", "hotfix/2.5.1");
    let branch_type = BranchType::HotfixFix { major: 2, minor: 5, patch: 1, name: "npe".to_string() };

    finish_hotfix_fix(&git, &hosting, &branch_type, None, false).unwrap();

    assert!(git.calls().contains(&"delete_branch_local:hotfix-fix/2.5.1/npe".to_string()),
        "calls: {:?}", git.calls());
    assert_eq!(hosting.calls(), vec!["merged_pr:hotfix-fix/2.5.1/npe"], "no new PR may be created");
}

#[test]
fn finish_branches_are_never_offered_as_pr_target() {
    let mut git = MockGit::new();
    git.current_branch = "feature/login".to_string();
    git.remote_branches = vec!["finish/release-1.2.0-into-main".to_string(), "develop".to_string()];
    let hosting = MockHosting::new();
    let prompter = MockPrompter::new(); // unscripted: any select would error
    let branch_type = BranchType::Feature { name: "login".to_string() };

    finish_work_branch(&git, &hosting, &prompter, &branch_type, Some(false), None, None, false).unwrap();

    assert!(prompter.calls().is_empty(),
        "finish/* is landing machinery and must be filtered; calls: {:?}", prompter.calls());
    assert!(hosting.calls()[1].starts_with("create_or_get_pr:feature/login:develop:"),
        "finish/* must never be a PR target, got: {}", hosting.calls()[1]);
}

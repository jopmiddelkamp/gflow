mod common;

use common::{MockEditor, MockGit, MockHosting, MockPrompter, MockVersionScript};
use gflow::flows::start::{start_work_branch, start_release, start_release_fix, start_hotfix_fix, ReleaseType, detect_breaking_changes};
use gflow::repo_config::{BumpStrategy, Mode, RepoConfig};
use gflow::version::SemVer;
use gflow::worktree::{open_worktree, WorktreeConfig, WorktreeContext, WorktreeEnv};
use gflow::worktree_setup::SetupCommands;
use common::MockWorktreeSetup;

fn patch_cfg() -> RepoConfig {
    RepoConfig { bump_strategy: BumpStrategy::Patch, ..RepoConfig::default() }
}

fn patch_protected_cfg() -> RepoConfig {
    RepoConfig { mode: Mode::Protected, bump_strategy: BumpStrategy::Patch, ..RepoConfig::default() }
}

// The exact-script assertions below are also what pins the mock's
// `call:arg:arg` recording format (decisions.md, Testing Strategy).

/// Build a worktree config whose base path is an existing temp dir, so the
/// flow's `create_dir_all` is a harmless no-op during tests.
fn test_worktree_config(editor: &str) -> WorktreeConfig {
    let base = std::env::temp_dir();
    WorktreeConfig {
        enabled: true,
        editor: editor.to_string(),
        base_path: Some(base.to_string_lossy().to_string()),
    }
}

#[test]
fn start_work_branch_creates_and_pushes() {
    let git = MockGit::new();
    start_work_branch(&git, "feature", "login-page", "develop", false, None).unwrap();

    assert_eq!(git.calls(), vec![
        "create_branch:feature/login-page:develop",
        "push:feature/login-page",
    ]);
}

#[test]
fn start_work_branch_with_fix_prefix() {
    let git = MockGit::new();
    start_work_branch(&git, "fix", "broken-auth", "main", false, None).unwrap();

    assert_eq!(git.calls(), vec![
        "create_branch:fix/broken-auth:main",
        "push:fix/broken-auth",
    ]);
}

#[test]
fn start_release_creates_new_when_no_release_exists_with_tags() {
    let mut git = MockGit::new();
    git.branches_matching = vec![]; // no existing release branches
    git.tags = vec!["v1.0.0".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
    ]);
}

#[test]
fn start_release_creates_new_when_no_release_exists_no_tags() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec![];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "checkout:develop",
        "create_branch:release/0.1.0:develop",
        "push:release/0.1.0",
        "create_tag:v0.1.0-rc.1:chore: create release branch 0.1.0",
        "push_tag:v0.1.0-rc.1",
    ]);
}

#[test]
fn start_release_patch_mode_reuses_branch_despite_its_clean_tag() {
    // Patch mode cuts v1.1.0 at branch creation — the tag's existence must not
    // mark the branch shipped (trap 1 inverted).
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".to_string()];
    git.existing_tags.insert("v1.1.0".to_string());

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &patch_cfg(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "remote_branch_exists:release/1.1.0",
        "is_ancestor:release/1.1.0:origin/main",
        "checkout:release/1.1.0",
    ]);
}

#[test]
fn start_release_patch_mode_checks_ancestry_via_the_remote_ref_when_available() {
    // A remote-only branch name is not a local rev — the ancestry check must
    // run against origin/{branch} or it hard-errors on a fresh clone.
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".to_string()];
    git.existing_remote_branches.insert("release/1.1.0".to_string());
    git.ancestors.insert(("origin/release/1.1.0".to_string(), "origin/main".to_string()));
    git.tags = vec!["v1.1.5".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &patch_cfg(), Some(ReleaseType::Minor), "main", None).unwrap();

    let calls = git.calls();
    assert!(calls.iter().any(|c| c == "is_ancestor:origin/release/1.1.0:origin/main"),
        "ancestry must be read from the remote ref when it exists; calls: {calls:?}");
    assert!(calls.iter().any(|c| c == "create_branch:release/1.2.0:develop"),
        "the landed branch is shipped, so a fresh release is cut; calls: {calls:?}");
}

#[test]
fn start_hotfix_fix_patch_mode_skips_in_flight_release_tags() {
    // Production is at v2.5.3; release/2.6.0 is open with staging tags v2.6.0
    // and v2.6.1 already pushed. The hotfix must patch production (2.5.4), not
    // steal the number the release's next bump would compute (2.6.2).
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/2.6.0".to_string()];
    git.tags = vec!["v2.5.3".to_string(), "v2.6.0".to_string(), "v2.6.1".to_string()];

    start_hotfix_fix(&git, &MockHosting::new(), &patch_cfg(), "urgent-crash", false, None, "main", None).unwrap();

    let calls = git.calls();
    assert!(calls.iter().any(|c| c == "create_branch:hotfix/2.5.4:main"),
        "hotfix version derives from shipped tags only; calls: {calls:?}");
}

#[test]
fn start_hotfix_fix_patch_mode_counts_tags_of_shipped_releases() {
    // Same layout, but release/2.6.0 already landed in main — its tags are
    // production history now, so the hotfix continues from v2.6.1.
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/2.6.0".to_string()];
    git.ancestors.insert(("release/2.6.0".to_string(), "origin/main".to_string()));
    git.tags = vec!["v2.5.3".to_string(), "v2.6.0".to_string(), "v2.6.1".to_string()];

    start_hotfix_fix(&git, &MockHosting::new(), &patch_cfg(), "urgent-crash", false, None, "main", None).unwrap();

    let calls = git.calls();
    assert!(calls.iter().any(|c| c == "create_branch:hotfix/2.6.2:main"),
        "a shipped release's tags are the production version; calls: {calls:?}");
}

#[test]
fn start_hotfix_fix_patch_mode_reuses_a_fresh_hotfix_branch() {
    // A hotfix cut from main with no version-script commit has main's tip —
    // ancestry would call it shipped on the spot. Hotfix tagging is identical
    // in both strategies (tag only at finish), so the tag stays the signal.
    let mut git = MockGit::new();
    git.branches_matching = vec!["hotfix/1.0.1".to_string()];
    git.ancestors.insert(("hotfix/1.0.1".to_string(), "origin/main".to_string()));

    start_hotfix_fix(&git, &MockHosting::new(), &patch_cfg(), "urgent-crash", false, None, "main", None).unwrap();

    let calls = git.calls();
    assert!(calls.iter().any(|c| c == "tag_exists:v1.0.1"),
        "hotfix shipped-detection is the clean tag in both strategies; calls: {calls:?}");
    assert!(calls.iter().any(|c| c == "checkout:hotfix/1.0.1"),
        "the untagged hotfix is still open and must be reused; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("is_ancestor:hotfix")),
        "ancestry must not decide hotfix shipped-ness; calls: {calls:?}");
}

#[test]
fn start_release_patch_mode_skips_branch_merged_to_main() {
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".to_string()];
    git.ancestors.insert(("release/1.1.0".to_string(), "origin/main".to_string()));
    git.tags = vec!["v1.1.0".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &patch_cfg(), Some(ReleaseType::Minor), "main", None).unwrap();

    let calls = git.calls();
    assert!(calls.iter().any(|c| c == "create_branch:release/1.2.0:develop"),
        "merged branch is shipped; a fresh release must be cut; calls: {calls:?}");
}

#[test]
fn start_release_patch_protected_skips_branch_landed_via_squash_pr() {
    // Squash merges leave no ancestry — the landing PR is the shipped signal.
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".to_string()];
    git.tags = vec!["v1.1.0".to_string()];
    git.ancestors.insert(("mc1".to_string(), "origin/main".to_string()));
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("release/1.1.0".to_string(), "main".to_string()),
        gflow::hosting::LandedPr {
            url: "https://github.com/org/repo/pull/1".to_string(),
            head_sha: "relsha".to_string(),
            merge_commit_sha: "mc1".to_string(),
        },
    );

    start_release(&git, &MockPrompter::new(), &hosting, None, &patch_protected_cfg(), Some(ReleaseType::Minor), "main", None).unwrap();

    let calls = git.calls();
    assert!(calls.iter().any(|c| c == "create_branch:release/1.2.0:develop"),
        "squash-landed branch is shipped; calls: {calls:?}");
    assert_eq!(hosting.calls(), vec!["merged_pr_to:release/1.1.0:main"]);
}

#[test]
fn start_release_patch_mode_cuts_a_clean_first_tag() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &patch_cfg(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "push:release/1.1.0",
        "create_tag:v1.1.0:chore: create release branch 1.1.0",
        "push_tag:v1.1.0",
    ]);
}

#[test]
fn start_release_checks_out_existing_release_branch() {
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "tag_exists:v1.1.0",
        "tag_exists:1.1.0",
        "checkout:release/1.1.0",
    ]);
}

#[test]
fn start_release_skips_shipped_release_branch() {
    // Trap 1: release/1.1.0 already has a v1.1.0 tag — it shipped and is not
    // open. Reuse must skip it and land on the still-open release/1.2.0.
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".to_string(), "release/1.2.0".to_string()];
    git.existing_tags.insert("v1.1.0".to_string());

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "tag_exists:v1.1.0",
        "tag_exists:v1.2.0",
        "tag_exists:1.2.0",
        "checkout:release/1.2.0",
    ]);
}

#[test]
fn start_release_creates_new_when_all_releases_shipped() {
    // Every candidate is shipped, so reuse falls through to the create path.
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".to_string()];
    git.existing_tags.insert("v1.1.0".to_string());
    git.tags = vec!["v1.1.0".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "tag_exists:v1.1.0",
        "list_tags",
        "checkout:develop",
        "create_branch:release/1.2.0:develop",
        "push:release/1.2.0",
        "create_tag:v1.2.0-rc.1:chore: create release branch 1.2.0",
        "push_tag:v1.2.0-rc.1",
    ]);
}

#[test]
fn start_release_reuse_keeps_unparseable_branch_open() {
    // A release branch whose version does not parse cannot be checked against
    // a tag, so it stays in the open set — today's behavior, unchanged by trap 1.
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/wip".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "checkout:release/wip",
    ]);
}

#[test]
fn start_release_fix_creates_and_pushes() {
    let mut git = MockGit::new();
    git.current_branch = "release/1.2.0".to_string();

    start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "broken-login", false, None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "create_branch:release-fix/1.2.0/broken-login:release/1.2.0",
        "push:release-fix/1.2.0/broken-login",
    ]);
}

#[test]
fn a_versionless_parent_branch_is_rejected_instead_of_naming_a_broken_child() {
    // Otherwise: `release-fix/wip/x`, which BranchType::parse reads as Other.
    let mut git = MockGit::new();
    git.current_branch = "release/wip".to_string();

    let err = start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "broken-login", false, None).unwrap_err();

    assert!(err.contains("does not carry a version"), "got: {err}");
    assert!(!git.calls().iter().any(|c| c.starts_with("create_branch")),
        "nothing may be created; calls: {:?}", git.calls());
}

#[test]
fn start_hotfix_fix_creates_and_pushes_existing_hotfix() {
    let mut git = MockGit::new();
    git.branches_matching = vec!["hotfix/1.0.1".to_string()];

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, None, "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "tag_exists:v1.0.1",
        "tag_exists:1.0.1",
        "checkout:hotfix/1.0.1",
        "create_branch:hotfix-fix/1.0.1/urgent-crash:hotfix/1.0.1",
        "push:hotfix-fix/1.0.1/urgent-crash",
    ]);
}

#[test]
fn start_hotfix_fix_skips_shipped_hotfix_branch() {
    // Trap 1: hotfix/1.0.1 already has a v1.0.1 tag — it shipped. Reuse must
    // skip it and land on the still-open hotfix/1.0.2.
    let mut git = MockGit::new();
    git.branches_matching = vec!["hotfix/1.0.1".to_string(), "hotfix/1.0.2".to_string()];
    git.existing_tags.insert("v1.0.1".to_string());

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, None, "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "tag_exists:v1.0.1",
        "tag_exists:v1.0.2",
        "tag_exists:1.0.2",
        "checkout:hotfix/1.0.2",
        "create_branch:hotfix-fix/1.0.2/urgent-crash:hotfix/1.0.2",
        "push:hotfix-fix/1.0.2/urgent-crash",
    ]);
}

#[test]
fn start_hotfix_fix_skips_a_hotfix_shipped_under_a_plain_tag() {
    // A pipeline that tags `1.0.1` instead of `v1.0.1` still shipped the hotfix:
    // SemVer::parse reads both spellings, so the shipped filter must too.
    let mut git = MockGit::new();
    git.branches_matching = vec!["hotfix/1.0.1".to_string()];
    git.existing_tags.insert("1.0.1".to_string());
    git.tags = vec!["1.0.1".to_string()];

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, None, "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "tag_exists:v1.0.1",
        "tag_exists:1.0.1",
        "list_tags",
        "checkout:main",
        "create_branch:hotfix/1.0.2:main",
        "push:hotfix/1.0.2",
        "create_branch:hotfix-fix/1.0.2/urgent-crash:hotfix/1.0.2",
        "push:hotfix-fix/1.0.2/urgent-crash",
    ]);
}

#[test]
fn start_hotfix_fix_creates_hotfix_branch_when_none_exists() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["1.0.0".to_string()];

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, None, "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "list_tags",
        "checkout:main",
        "create_branch:hotfix/1.0.1:main",
        "push:hotfix/1.0.1",
        "create_branch:hotfix-fix/1.0.1/urgent-crash:hotfix/1.0.1",
        "push:hotfix-fix/1.0.1/urgent-crash",
    ]);
}

#[test]
fn a_new_hotfix_branch_is_cut_from_the_configured_mainline() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["1.0.0".to_string()];

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, None, "master", None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "list_tags",
        "checkout:master",
        "create_branch:hotfix/1.0.1:master",
        "push:hotfix/1.0.1",
        "create_branch:hotfix-fix/1.0.1/urgent-crash:hotfix/1.0.1",
        "push:hotfix-fix/1.0.1/urgent-crash",
    ]);
}

#[test]
fn start_work_branch_no_checkout_creates_without_switching() {
    let git = MockGit::new();
    start_work_branch(&git, "feature", "login-page", "develop", true, None).unwrap();

    assert_eq!(git.calls(), vec![
        "create_branch_no_checkout:feature/login-page:develop",
        "push:feature/login-page",
    ]);
}

#[test]
fn start_release_fix_no_checkout_discovers_release_branch() {
    let mut git = MockGit::new();
    git.current_branch = "develop".to_string();
    git.branches_matching = vec!["release/1.2.0".to_string()];
    git.existing_local_branches.insert("release/1.2.0".to_string());

    start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "broken-login", true, None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:release/*",
        "tag_exists:v1.2.0",
        "tag_exists:1.2.0",
        "local_branch_exists:release/1.2.0",
        "create_branch_no_checkout:release-fix/1.2.0/broken-login:release/1.2.0",
        "push:release-fix/1.2.0/broken-login",
    ]);
}

#[test]
fn start_release_fix_no_checkout_skips_shipped_release_branch() {
    // Trap 1: release/1.1.0 already shipped (tagged v1.1.0). Discovery must
    // skip it and pick the still-open release/1.2.0.
    let mut git = MockGit::new();
    git.current_branch = "develop".to_string();
    git.branches_matching = vec!["release/1.1.0".to_string(), "release/1.2.0".to_string()];
    git.existing_tags.insert("v1.1.0".to_string());
    git.existing_local_branches.insert("release/1.2.0".to_string());

    start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "broken-login", true, None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:release/*",
        "tag_exists:v1.1.0",
        "tag_exists:v1.2.0",
        "tag_exists:1.2.0",
        "local_branch_exists:release/1.2.0",
        "create_branch_no_checkout:release-fix/1.2.0/broken-login:release/1.2.0",
        "push:release-fix/1.2.0/broken-login",
    ]);
}

#[test]
fn start_release_fix_no_checkout_errors_when_no_release_branch() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];

    let result = start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "broken-login", true, None);
    assert!(result.is_err());
}

#[test]
fn start_hotfix_fix_no_checkout_existing_hotfix() {
    let mut git = MockGit::new();
    git.branches_matching = vec!["hotfix/1.0.1".to_string()];
    git.existing_local_branches.insert("hotfix/1.0.1".to_string());

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", true, None, "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "tag_exists:v1.0.1",
        "tag_exists:1.0.1",
        "local_branch_exists:hotfix/1.0.1",
        "create_branch_no_checkout:hotfix-fix/1.0.1/urgent-crash:hotfix/1.0.1",
        "push:hotfix-fix/1.0.1/urgent-crash",
    ]);
}

#[test]
fn start_hotfix_fix_no_checkout_creates_hotfix_branch_when_none_exists() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["1.0.0".to_string()];

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", true, None, "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "list_tags",
        "create_branch_no_checkout:hotfix/1.0.1:main",
        "push:hotfix/1.0.1",
        "create_branch_no_checkout:hotfix-fix/1.0.1/urgent-crash:hotfix/1.0.1",
        "push:hotfix-fix/1.0.1/urgent-crash",
    ]);
}

#[test]
fn start_release_falls_back_to_rc_tags_when_no_clean_tags() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.1.0-rc.1".to_string(), "v1.1.0-rc.2".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    // Should use 1.1.0 (from RC tags) as base, bump to 1.2
    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "checkout:develop",
        "create_branch:release/1.2.0:develop",
        "push:release/1.2.0",
        "create_tag:v1.2.0-rc.1:chore: create release branch 1.2.0",
        "push_tag:v1.2.0-rc.1",
    ]);
}

#[test]
fn start_release_major_bumps_major_version() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.5.0".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Major), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "checkout:develop",
        "create_branch:release/2.0.0:develop",
        "push:release/2.0.0",
        "create_tag:v2.0.0-rc.1:chore: create release branch 2.0.0",
        "push_tag:v2.0.0-rc.1",
    ]);
}

#[test]
fn start_release_ignores_rc_tags_when_determining_next_version() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string(), "v1.1.0-rc.1".to_string(), "v1.1.0-rc.2".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
    ]);
}

// --- Worktree flow tests ---

#[test]
fn start_work_branch_worktree_active_forces_no_checkout_and_opens() {
    let git = MockGit::new(); // repo_root default "/repos/beans-gitflow"
    let config = test_worktree_config("code");
    let editor = MockEditor::new();
    let setup = MockWorktreeSetup::new();
    let prompter = MockPrompter::new();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: None };
    let ctx = WorktreeContext { env: &env, prompter: &prompter };

    // Pass no_checkout=false: worktree mode must still force the no-checkout path.
    start_work_branch(&git, "feature", "login-page", "develop", false, Some(ctx)).unwrap();

    let expected = std::env::temp_dir().join("beans-gitflow-feature-login-page");
    let expected = expected.display().to_string();
    assert_eq!(git.calls(), vec![
        "create_branch_no_checkout:feature/login-page:develop".to_string(),
        "push:feature/login-page".to_string(),
        "repo_root".to_string(),
        format!("add_worktree:{expected}:feature/login-page"),
    ]);
    assert_eq!(editor.calls(), vec![format!("open:{expected}")]);
}

#[test]
fn start_work_branch_worktree_editor_failure_is_not_fatal() {
    let git = MockGit::new();
    let config = test_worktree_config("code");
    let mut editor = MockEditor::new();
    editor.fail = true;
    let setup = MockWorktreeSetup::new();
    let prompter = MockPrompter::new();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: None };
    let ctx = WorktreeContext { env: &env, prompter: &prompter };

    let result = start_work_branch(&git, "feature", "login-page", "develop", false, Some(ctx));
    assert!(result.is_ok(), "editor failure should be a warning, not fatal");
    assert!(git.calls().iter().any(|c| c.starts_with("add_worktree:")), "worktree should still be created");
}

#[test]
fn start_work_branch_worktree_editor_none_skips_open() {
    let git = MockGit::new();
    let config = test_worktree_config("none");
    let editor = MockEditor::new();
    let setup = MockWorktreeSetup::new();
    let prompter = MockPrompter::new();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: None };
    let ctx = WorktreeContext { env: &env, prompter: &prompter };

    start_work_branch(&git, "feature", "login-page", "develop", false, Some(ctx)).unwrap();

    assert!(editor.calls().is_empty(), "editor 'none' should not open anything");
    assert!(git.calls().iter().any(|c| c.starts_with("add_worktree:")), "worktree should still be created");
}

#[test]
fn start_release_fix_worktree_active_discovers_and_opens() {
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.2.0".to_string()];
    git.existing_local_branches.insert("release/1.2.0".to_string());
    let config = test_worktree_config("code");
    let editor = MockEditor::new();
    let setup = MockWorktreeSetup::new();
    let prompter = MockPrompter::new();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: None };
    let ctx = WorktreeContext { env: &env, prompter: &prompter };

    // worktree mode forces the no-checkout discovery path even from develop.
    start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "broken-login", false, Some(ctx)).unwrap();

    let expected = std::env::temp_dir().join("beans-gitflow-release-fix-1.2.0-broken-login");
    let expected = expected.display().to_string();
    assert_eq!(git.calls(), vec![
        "current_branch".to_string(),
        "list_branches_matching:release/*".to_string(),
        "tag_exists:v1.2.0".to_string(),
        "tag_exists:1.2.0".to_string(),
        "local_branch_exists:release/1.2.0".to_string(),
        "create_branch_no_checkout:release-fix/1.2.0/broken-login:release/1.2.0".to_string(),
        "push:release-fix/1.2.0/broken-login".to_string(),
        "repo_root".to_string(),
        format!("add_worktree:{expected}:release-fix/1.2.0/broken-login"),
    ]);
    assert_eq!(editor.calls(), vec![format!("open:{expected}")]);
}

// --- Version script at branch creation (M1, M4) ---

#[test]
fn start_release_runs_version_script_on_new_branch() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, false]);
    let script = MockVersionScript::new();

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), Some(&script), &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
        // A configured script also bumps develop; the clean-tree answers run out
        // here so this falls to its no-op tail (see the m2_* tests for the
        // committed path).
        "checkout:develop",
        "ff_merge:origin/develop",
        "is_working_tree_clean",
        "is_working_tree_clean",
        "checkout:release/1.1.0",
    ]);
    assert_eq!(script.calls(), vec!["run:1.1.0", "run:1.2.0"]);
}

#[test]
fn start_release_script_noop_makes_no_commit() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, true]);
    let script = MockVersionScript::new();

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), Some(&script), &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "is_working_tree_clean",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
        // Also a no-op here (same reasoning as above).
        "checkout:develop",
        "ff_merge:origin/develop",
        "is_working_tree_clean",
        "is_working_tree_clean",
        "checkout:release/1.1.0",
    ]);
    assert_eq!(script.calls(), vec!["run:1.1.0", "run:1.2.0"]);
}

#[test]
fn start_release_dirty_tree_blocks_script() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([false]);
    let script = MockVersionScript::new();

    let err = start_release(&git, &MockPrompter::new(), &MockHosting::new(), Some(&script), &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap_err();

    assert_eq!(err, "Working tree is not clean. Commit or stash your changes, then re-run.");
    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
    ]);
    assert!(!git.calls().iter().any(|c| c.starts_with("create_branch")),
        "a dirty tree must be rejected before any branch is created; calls: {:?}", git.calls());
    assert!(script.calls().is_empty());
}

#[test]
fn start_release_reuse_path_never_runs_script() {
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".to_string()];
    let script = MockVersionScript::new();

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), Some(&script), &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "tag_exists:v1.1.0",
        "tag_exists:1.1.0",
        "checkout:release/1.1.0",
    ]);
    assert!(script.calls().is_empty());
}

// --- M2: develop version bump after release cut ---
// Only reachable from the create path (never the reuse path above), and only
// when a script is configured. Runs last, after the rc.1 tag is pushed.

#[test]
fn m2_free_mode_bumps_develop_and_returns_to_the_release_branch() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, false, true, false]);
    let script = MockVersionScript::new();

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), Some(&script), &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
        "checkout:develop",
        "ff_merge:origin/develop",
        "is_working_tree_clean",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.2.0",
        "push:develop",
        "checkout:release/1.1.0",
    ]);
    assert_eq!(script.calls(), vec!["run:1.1.0", "run:1.2.0"]);
}

#[test]
fn m2_free_mode_noop_skips_the_push() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, false, true, true]);
    let script = MockVersionScript::new();

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), Some(&script), &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
        "checkout:develop",
        "ff_merge:origin/develop",
        "is_working_tree_clean",
        "is_working_tree_clean",
        "checkout:release/1.1.0",
    ]);
    assert!(!git.calls().contains(&"push:develop".to_string()));
    assert_eq!(script.calls(), vec!["run:1.1.0", "run:1.2.0"]);
}

#[test]
fn m2_protected_mode_opens_a_version_pr_on_a_fresh_branch() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, false, true, false]);
    let script = MockVersionScript::new();
    let hosting = MockHosting::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    start_release(&git, &MockPrompter::new(), &hosting, Some(&script), &cfg, Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
        "checkout:develop",
        "ff_merge:origin/develop",
        "remote_branch_exists:chore/set-version-1.2.0",
        "local_branch_exists:chore/set-version-1.2.0",
        "create_branch:chore/set-version-1.2.0:develop",
        "is_working_tree_clean",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.2.0",
        "push:chore/set-version-1.2.0",
        "checkout:release/1.1.0",
    ]);
    assert!(!git.calls().contains(&"push:develop".to_string()),
        "protected mode must never push develop directly; calls: {:?}", git.calls());
    assert_eq!(hosting.calls(), vec![
        "create_or_get_pr:chore/set-version-1.2.0:develop:chore: set version 1.2.0",
        "copy_text:chore: set version 1.2.0\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    assert_eq!(script.calls(), vec!["run:1.1.0", "run:1.2.0"]);
}

#[test]
fn m2_protected_mode_noop_deletes_the_fresh_chore_branch() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, false, true, true]);
    let script = MockVersionScript::new();
    let hosting = MockHosting::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    start_release(&git, &MockPrompter::new(), &hosting, Some(&script), &cfg, Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
        "checkout:develop",
        "ff_merge:origin/develop",
        "remote_branch_exists:chore/set-version-1.2.0",
        "local_branch_exists:chore/set-version-1.2.0",
        "create_branch:chore/set-version-1.2.0:develop",
        "is_working_tree_clean",
        "is_working_tree_clean",
        "checkout:develop",
        "delete_branch_local:chore/set-version-1.2.0",
        "checkout:release/1.1.0",
    ]);
    assert!(hosting.calls().is_empty(), "a no-op version bump must never open a PR; calls: {:?}", hosting.calls());
    assert_eq!(script.calls(), vec!["run:1.1.0", "run:1.2.0"]);
}

#[test]
fn m2_protected_mode_reuses_a_leftover_chore_branch_without_recreating_it() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, false]);
    git.existing_remote_branches.insert("chore/set-version-1.2.0".to_string());
    let script = MockVersionScript::new();
    let hosting = MockHosting::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    start_release(&git, &MockPrompter::new(), &hosting, Some(&script), &cfg, Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
        "checkout:develop",
        "ff_merge:origin/develop",
        "remote_branch_exists:chore/set-version-1.2.0",
        "checkout:release/1.1.0",
    ]);
    assert!(!git.calls().iter().any(|c| c.starts_with("create_branch:chore")),
        "a leftover remote branch must be reused, never recreated; calls: {:?}", git.calls());
    assert_eq!(hosting.calls(), vec![
        "create_or_get_pr:chore/set-version-1.2.0:develop:chore: set version 1.2.0",
        "copy_text:chore: set version 1.2.0\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
    assert_eq!(script.calls(), vec!["run:1.1.0"], "the leftover-reuse path never re-runs the script");
}

#[test]
fn bump_develop_protected_deletes_leftover_local_chore_branch_before_recreating() {
    // Mirrors bump_protected's own leftover-local fix (finish_release.rs): a
    // prior M2 run can leave chore/set-version-{dev} behind locally only, and
    // re-running must not die on git's raw "branch already exists".
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, false, true, false]);
    git.existing_local_branches.insert("chore/set-version-1.2.0".to_string());
    let script = MockVersionScript::new();
    let hosting = MockHosting::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    start_release(&git, &MockPrompter::new(), &hosting, Some(&script), &cfg, Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
        "checkout:develop",
        "ff_merge:origin/develop",
        "remote_branch_exists:chore/set-version-1.2.0",
        "local_branch_exists:chore/set-version-1.2.0",
        "delete_branch_local:chore/set-version-1.2.0",
        "create_branch:chore/set-version-1.2.0:develop",
        "is_working_tree_clean",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.2.0",
        "push:chore/set-version-1.2.0",
        "checkout:release/1.1.0",
    ]);
    assert_eq!(hosting.calls(), vec![
        "create_or_get_pr:chore/set-version-1.2.0:develop:chore: set version 1.2.0",
        "copy_text:chore: set version 1.2.0\nhttps://github.com/org/repo/pull/1",
        "open_url:https://github.com/org/repo/pull/1",
    ]);
}

#[test]
fn bump_develop_protected_script_failure_restores_to_develop_then_release_branch() {
    // A failed M2 script run must not strand the operator on the chore
    // branch: gflow best-effort restores develop first, so the outer warn
    // path's own final checkout (back to the release branch) still works.
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, false, true]);
    let mut script = MockVersionScript::new();
    script.fail_nth_run = Some(2); // M1's run succeeds; M2's (the 2nd) fails
    let hosting = MockHosting::new();
    let cfg = RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() };

    let result = start_release(&git, &MockPrompter::new(), &hosting, Some(&script), &cfg, Some(ReleaseType::Minor), "main", None);

    assert!(result.is_ok(), "a failed M2 script run must not undo an already-created release: {result:?}");
    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
        "checkout:develop",
        "ff_merge:origin/develop",
        "remote_branch_exists:chore/set-version-1.2.0",
        "local_branch_exists:chore/set-version-1.2.0",
        "create_branch:chore/set-version-1.2.0:develop",
        "is_working_tree_clean",
        "checkout:develop",
        "checkout:release/1.1.0",
    ]);
    assert!(hosting.calls().is_empty(), "no PR call on script failure; calls: {:?}", hosting.calls());
    assert_eq!(script.calls(), vec!["run:1.1.0", "run:1.2.0"]);
}

#[test]
fn m2_failure_is_warn_and_continue_the_release_already_succeeded() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, false]);
    git.ff_merge_error = Some("fatal: not something we can merge".to_string());
    let script = MockVersionScript::new();

    let result = start_release(&git, &MockPrompter::new(), &MockHosting::new(), Some(&script), &RepoConfig::default(), Some(ReleaseType::Minor), "main", None);

    assert!(result.is_ok(), "a failed develop bump must not undo an already-created release: {result:?}");
    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:develop",
        "create_branch:release/1.1.0:develop",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.1.0",
        "push:release/1.1.0",
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
        "push_tag:v1.1.0-rc.1",
        "checkout:develop",
        "ff_merge:origin/develop",
        "checkout:release/1.1.0",
    ]);
    assert_eq!(script.calls(), vec!["run:1.1.0"], "M2's own script run never happens once ff_merge fails");
}

#[test]
fn start_hotfix_fix_runs_version_script_on_new_branch() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, false]);
    let script = MockVersionScript::new();

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, None, "main", Some(&script)).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:main",
        "create_branch:hotfix/1.0.1:main",
        "is_working_tree_clean",
        "stage_all",
        "commit:chore: set version 1.0.1",
        "push:hotfix/1.0.1",
        "create_branch:hotfix-fix/1.0.1/urgent-crash:hotfix/1.0.1",
        "push:hotfix-fix/1.0.1/urgent-crash",
    ]);
    assert_eq!(script.calls(), vec!["run:1.0.1"]);
}

#[test]
fn start_hotfix_fix_script_noop_makes_no_commit() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([true, true]);
    let script = MockVersionScript::new();

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, None, "main", Some(&script)).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "list_tags",
        "is_working_tree_clean",
        "checkout:main",
        "create_branch:hotfix/1.0.1:main",
        "is_working_tree_clean",
        "push:hotfix/1.0.1",
        "create_branch:hotfix-fix/1.0.1/urgent-crash:hotfix/1.0.1",
        "push:hotfix-fix/1.0.1/urgent-crash",
    ]);
    assert_eq!(script.calls(), vec!["run:1.0.1"]);
}

#[test]
fn start_hotfix_fix_dirty_tree_blocks_script() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["1.0.0".to_string()];
    git.working_tree_clean_seq.borrow_mut().extend([false]);
    let script = MockVersionScript::new();

    let err = start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, None, "main", Some(&script)).unwrap_err();

    assert_eq!(err, "Working tree is not clean. Commit or stash your changes, then re-run.");
    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "list_tags",
        "is_working_tree_clean",
    ]);
    assert!(!git.calls().iter().any(|c| c.starts_with("create_branch")),
        "a dirty tree must be rejected before any branch is created; calls: {:?}", git.calls());
    assert!(script.calls().is_empty());
}

#[test]
fn start_hotfix_fix_reuse_path_never_runs_script() {
    let mut git = MockGit::new();
    git.branches_matching = vec!["hotfix/1.0.1".to_string()];
    let script = MockVersionScript::new();

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, None, "main", Some(&script)).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "tag_exists:v1.0.1",
        "tag_exists:1.0.1",
        "checkout:hotfix/1.0.1",
        "create_branch:hotfix-fix/1.0.1/urgent-crash:hotfix/1.0.1",
        "push:hotfix-fix/1.0.1/urgent-crash",
    ]);
    assert!(script.calls().is_empty());
}

#[test]
fn hotfix_no_checkout_skips_script() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["1.0.0".to_string()];
    let script = MockVersionScript::new();

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", true, None, "main", Some(&script)).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "list_tags",
        "create_branch_no_checkout:hotfix/1.0.1:main",
        "push:hotfix/1.0.1",
        "create_branch_no_checkout:hotfix-fix/1.0.1/urgent-crash:hotfix/1.0.1",
        "push:hotfix-fix/1.0.1/urgent-crash",
    ]);
    assert!(script.calls().is_empty());
}

// Note: the pure `message_is_breaking` string-matching logic is tested
// as unit tests in src/flows/start.rs. These integration tests cover the
// git interaction — which ref is queried, and the develop → origin/develop
// fallback.

// --- Base-branch errors are rewritten into gflow's own guidance ---

#[test]
fn unknown_base_branch_error_is_rewritten_to_name_the_base_flag() {
    // decisions.md, Error Model: "Raw git errors are intercepted and rewritten
    // when gflow knows better". git's "not a commit" is opaque; the user needs
    // to be told the base does not exist and which flag fixes it.
    let mut git = MockGit::new();
    git.create_branch_error = Some("fatal: 'nope' is not a commit and a branch 'x' cannot be created from it".to_string());

    let err = start_work_branch(&git, "feature", "login", "nope", false, None).unwrap_err();

    assert!(err.contains("Branch 'nope' does not exist"), "must name the missing base; got: {err}");
    assert!(err.contains("--base"), "must name the exact next flag to use; got: {err}");
}

#[test]
fn other_create_branch_errors_are_passed_through_untouched() {
    // Only "not a commit" is rewritten — guessing at any other git failure would
    // send the user down the wrong path.
    let mut git = MockGit::new();
    git.create_branch_error = Some("fatal: a branch named 'feature/login' already exists".to_string());

    let err = start_work_branch(&git, "feature", "login", "develop", false, None).unwrap_err();

    assert_eq!(err, "fatal: a branch named 'feature/login' already exists");
}

#[test]
fn start_release_fix_requires_standing_on_a_release_branch() {
    // Without --no-checkout (or the worktree flow) the flow does not go hunting
    // for a release branch — you must be on the one you are fixing.
    let mut git = MockGit::new();
    git.current_branch = "develop".to_string();

    let err = start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "db-index", false, None).unwrap_err();

    assert_eq!(err, "Not on a release branch");
    assert!(!git.calls().iter().any(|c| c.starts_with("create_branch")),
        "nothing may be created after the guard; calls: {:?}", git.calls());
}

#[test]
fn start_hotfix_fix_worktree_active_discovers_and_opens() {
    // Worktree context implies no-checkout; HEAD is on develop, so the hotfix
    // branch is discovered from the branch list.
    let mut git = MockGit::new();
    git.current_branch = "develop".to_string();
    git.branches_matching = vec!["hotfix/2.5.1".to_string()];
    git.existing_local_branches.insert("hotfix/2.5.1".to_string());
    git.repo_root = std::env::temp_dir().join("beans-gitflow");
    let editor = MockEditor::new();
    let config = test_worktree_config("code");

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "npe", false, Some(WorktreeContext { env: &WorktreeEnv { config: &config, editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, prompter: &MockPrompter::new() }), "main", None).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"create_branch_no_checkout:hotfix-fix/2.5.1/npe:hotfix/2.5.1".to_string()),
        "a worktree start must never switch the current checkout; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("checkout:")), "calls: {calls:?}");
    assert_eq!(editor.calls(), vec![
        format!("open:{}", worktree_for("hotfix/2.5.1")),
        format!("open:{}", worktree_for("hotfix-fix/2.5.1/npe")),
    ], "the hotfix container opens first; the fix worktree opens last so it keeps focus");
}

#[test]
fn start_hotfix_fix_worktree_mode_opens_a_worktree_for_the_new_hotfix_branch_too() {
    // A hotfix container is only ever created by `start hotfix-fix` — there is
    // no `start hotfix` to hand it a worktree later. Mirror of the release
    // hand-off: the container gets its own worktree, so `gflow finish` has a
    // checkout to run from without touching the main tree.
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["1.0.0".to_string()];
    let config = test_worktree_config("code");
    let editor = MockEditor::new();
    let setup = MockWorktreeSetup::new();
    let prompter = MockPrompter::new();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: None };
    let ctx = WorktreeContext { env: &env, prompter: &prompter };

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, Some(ctx), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch".to_string(),
        "list_branches_matching:hotfix/*".to_string(),
        "list_tags".to_string(),
        "create_branch_no_checkout:hotfix/1.0.1:main".to_string(),
        "push:hotfix/1.0.1".to_string(),
        "worktree_of:hotfix/1.0.1".to_string(),
        "repo_root".to_string(),
        format!("add_worktree:{}:hotfix/1.0.1", worktree_for("hotfix/1.0.1")),
        "create_branch_no_checkout:hotfix-fix/1.0.1/urgent-crash:hotfix/1.0.1".to_string(),
        "push:hotfix-fix/1.0.1/urgent-crash".to_string(),
        "repo_root".to_string(),
        format!("add_worktree:{}:hotfix-fix/1.0.1/urgent-crash", worktree_for("hotfix-fix/1.0.1/urgent-crash")),
    ]);
    assert_eq!(editor.calls(), vec![
        format!("open:{}", worktree_for("hotfix/1.0.1")),
        format!("open:{}", worktree_for("hotfix-fix/1.0.1/urgent-crash")),
    ]);
}

#[test]
fn start_hotfix_fix_worktree_mode_points_at_the_worktree_that_already_holds_the_hotfix() {
    let mut git = MockGit::new();
    git.current_branch = "develop".to_string();
    git.branches_matching = vec!["hotfix/2.5.1".to_string()];
    git.worktrees.insert("hotfix/2.5.1".to_string(), std::path::PathBuf::from("/repos/beans-gitflow-hotfix-2.5.1"));
    let config = test_worktree_config("code");
    let editor = MockEditor::new();
    let setup = MockWorktreeSetup::new();
    let prompter = MockPrompter::new();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: None };
    let ctx = WorktreeContext { env: &env, prompter: &prompter };

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "npe", false, Some(ctx), "main", None).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"worktree_of:hotfix/2.5.1".to_string()),
        "the container's worktree must be looked up; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("add_worktree:") && c.ends_with(":hotfix/2.5.1")),
        "the container is announced, never re-created; calls: {calls:?}");
    assert!(calls.iter().any(|c| c.starts_with("add_worktree:") && c.ends_with(":hotfix-fix/2.5.1/npe")),
        "the fix branch still gets its worktree; calls: {calls:?}");
    assert_eq!(editor.calls(), vec![format!("open:{}", worktree_for("hotfix-fix/2.5.1/npe"))]);
}

// --- Release-type prompt: detection reorders the default, it never decides ---

#[test]
fn breaking_changes_put_major_first_in_the_prompt() {
    // decisions.md, Release Discipline: "Breaking-change detection reorders the
    // release-type menu default, it never decides for you."
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.2.0".to_string()];
    git.commit_messages = vec!["feat!: drop the v1 API".to_string()];
    let prompter = MockPrompter::scripted(&[0]); // take the default

    start_release(&git, &prompter, &MockHosting::new(), None, &RepoConfig::default(), None, "main", None).unwrap();

    assert_eq!(prompter.calls(), vec![
        "select:Release type:[major (v1.2.0 → v2.0.0), minor (v1.2.0 → v1.3.0)]",
    ]);
    assert!(git.calls().contains(&"create_branch:release/2.0.0:develop".to_string()),
        "the default selection must yield the major bump; calls: {:?}", git.calls());
}

#[test]
fn without_breaking_changes_minor_comes_first() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.2.0".to_string()];
    git.commit_messages = vec!["feat: add login page".to_string()];
    let prompter = MockPrompter::scripted(&[0]);

    start_release(&git, &prompter, &MockHosting::new(), None, &RepoConfig::default(), None, "main", None).unwrap();

    assert_eq!(prompter.calls(), vec![
        "select:Release type:[minor (v1.2.0 → v1.3.0), major (v1.2.0 → v2.0.0)]",
    ]);
    assert!(git.calls().contains(&"create_branch:release/1.3.0:develop".to_string()),
        "calls: {:?}", git.calls());
}

#[test]
fn the_prompt_still_decides_when_the_user_picks_the_non_default() {
    // Ordering is a hint. Picking "major" from second place must still bump major.
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.2.0".to_string()];
    git.commit_messages = vec!["fix: typo".to_string()];
    let prompter = MockPrompter::scripted(&[1]); // second item = major

    start_release(&git, &prompter, &MockHosting::new(), None, &RepoConfig::default(), None, "main", None).unwrap();

    assert!(git.calls().contains(&"create_branch:release/2.0.0:develop".to_string()),
        "calls: {:?}", git.calls());
}

#[test]
fn detect_breaking_returns_false_when_commits_exist_but_none_are_breaking() {
    let mut git = MockGit::new();
    git.commit_messages = vec![
        "feat: add login page".to_string(),
        "chore: bump deps".to_string(),
    ];

    assert!(!detect_breaking_changes(&git, &SemVer::new(1, 0, 0)));
}

#[test]
fn detect_breaking_queries_develop_not_head() {
    let mut git = MockGit::new();
    git.commit_messages = vec!["feat!: remove API".to_string()];

    let result = detect_breaking_changes(&git, &SemVer::new(1, 0, 0));

    assert!(result);
    // Must query develop, not HEAD — so start release works from any branch
    assert!(git.calls().iter().any(|c| c == "commit_messages:v1.0.0:develop"),
        "Expected commit_messages to be called with 'develop', got: {:?}", git.calls());
}

#[test]
fn detect_breaking_falls_back_to_origin_develop_when_develop_missing() {
    let mut git = MockGit::new();
    // Simulate a fresh clone / CI environment where local 'develop' doesn't exist
    git.fail_commit_messages_for = vec!["develop".to_string()];
    git.commit_messages = vec!["feat!: remove API".to_string()];

    let result = detect_breaking_changes(&git, &SemVer::new(1, 0, 0));

    assert!(result, "Fallback to origin/develop should detect the breaking change");
    assert_eq!(git.calls(), vec![
        "commit_messages:v1.0.0:develop",         // first attempt
        "commit_messages:v1.0.0:origin/develop",  // fallback
    ]);
}

#[test]
fn detect_breaking_returns_false_when_neither_develop_nor_origin_exist() {
    let mut git = MockGit::new();
    git.fail_commit_messages_for = vec!["develop".to_string(), "origin/develop".to_string()];

    let result = detect_breaking_changes(&git, &SemVer::new(1, 0, 0));

    assert!(!result, "Should return false when no refs are accessible");
    assert_eq!(git.calls(), vec![
        "commit_messages:v1.0.0:develop",
        "commit_messages:v1.0.0:origin/develop",
    ]);
}

#[test]
fn start_release_worktree_mode_creates_here_then_opens_a_worktree() {
    let mut git = MockGit::new();
    git.branches_matching = vec![];
    git.tags = vec!["v1.0.0".to_string()];
    let config = test_worktree_config("code");
    let editor = MockEditor::new();
    let setup = MockWorktreeSetup::new();
    let prompter = MockPrompter::new();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: None };
    let ctx = WorktreeContext { env: &env, prompter: &prompter };

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Minor), "main", Some(ctx)).unwrap();

    let expected = std::env::temp_dir().join("beans-gitflow-release-1.1.0").display().to_string();
    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*".to_string(),
        "list_tags".to_string(),
        "checkout:develop".to_string(),
        "create_branch:release/1.1.0:develop".to_string(),
        "push:release/1.1.0".to_string(),
        "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0".to_string(),
        "push_tag:v1.1.0-rc.1".to_string(),
        "checkout:develop".to_string(),
        "worktree_of:release/1.1.0".to_string(),
        "repo_root".to_string(),
        format!("add_worktree:{expected}:release/1.1.0"),
    ]);
    assert_eq!(editor.calls(), vec![format!("open:{expected}")]);
}

#[test]
fn start_release_worktree_mode_opens_a_worktree_for_an_existing_release() {
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".to_string()];
    let config = test_worktree_config("code");
    let editor = MockEditor::new();
    let setup = MockWorktreeSetup::new();
    let prompter = MockPrompter::new();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: None };
    let ctx = WorktreeContext { env: &env, prompter: &prompter };

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), None, "main", Some(ctx)).unwrap();

    let calls = git.calls();
    assert!(!calls.contains(&"checkout:release/1.1.0".to_string()), "must not switch the current tree; calls: {calls:?}");
    assert!(calls.iter().any(|c| c.starts_with("add_worktree:") && c.ends_with(":release/1.1.0")), "calls: {calls:?}");
}

#[test]
fn start_release_worktree_mode_points_at_the_worktree_that_already_holds_the_release() {
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".to_string()];
    git.worktrees.insert("release/1.1.0".to_string(), std::path::PathBuf::from("/repos/beans-gitflow-release-1.1.0"));
    let config = test_worktree_config("code");
    let editor = MockEditor::new();
    let setup = MockWorktreeSetup::new();
    let prompter = MockPrompter::new();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: None };
    let ctx = WorktreeContext { env: &env, prompter: &prompter };

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), None, "main", Some(ctx)).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("add_worktree:") || c.starts_with("checkout:")), "nothing to create or switch; calls: {calls:?}");
    assert!(editor.calls().is_empty(), "the existing worktree is announced, not re-opened");
}

// --- worktrees.json setup commands ---

fn setup_cmds() -> SetupCommands {
    SetupCommands { file: "worktrees.json".into(), commands: vec!["fvm use".into(), "dart pub get".into()] }
}

fn worktree_for(branch: &str) -> String {
    std::env::temp_dir().join(format!("beans-gitflow-{}", branch.replace('/', "-"))).display().to_string()
}

#[test]
fn open_worktree_runs_setup_commands_after_creation_and_before_the_editor() {
    let git = MockGit::new();
    let config = test_worktree_config("code");
    let (editor, setup, prompter) = (MockEditor::new(), MockWorktreeSetup::new(), MockPrompter::new());
    let cmds = setup_cmds();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: Some(&cmds) };

    open_worktree(&git, &WorktreeContext { env: &env, prompter: &prompter }, "feature/x").unwrap();

    let wt = worktree_for("feature/x");
    assert!(prompter.calls().is_empty(), "setup commands run without asking");
    assert_eq!(setup.calls(), vec![
        format!("run:{wt}:/repos/beans-gitflow:fvm use"),
        format!("run:{wt}:/repos/beans-gitflow:dart pub get"),
    ]);
    assert_eq!(editor.calls(), vec![format!("open:{wt}")]);
    assert_eq!(git.calls().last().unwrap(), &format!("add_worktree:{wt}:feature/x"), "commands run after the worktree exists");
}

#[test]
fn open_worktree_continues_after_a_failing_command() {
    let git = MockGit::new();
    let config = test_worktree_config("code");
    let (editor, mut setup, prompter) = (MockEditor::new(), MockWorktreeSetup::new(), MockPrompter::new());
    setup.fail.insert("fvm use".to_string());
    let cmds = setup_cmds();
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: Some(&cmds) };

    open_worktree(&git, &WorktreeContext { env: &env, prompter: &prompter }, "feature/x").unwrap();

    assert_eq!(setup.calls().len(), 2, "a failing command does not stop the rest; calls: {:?}", setup.calls());
    assert_eq!(editor.calls().len(), 1);
}

#[test]
fn open_worktree_runs_nothing_without_a_setup_file() {
    let git = MockGit::new();
    let config = test_worktree_config("code");
    let (editor, setup, prompter) = (MockEditor::new(), MockWorktreeSetup::new(), MockPrompter::new());
    let env = WorktreeEnv { config: &config, editor: &editor, setup: &setup, commands: None };

    open_worktree(&git, &WorktreeContext { env: &env, prompter: &prompter }, "feature/y").unwrap();

    assert!(setup.calls().is_empty());
    assert_eq!(editor.calls().len(), 1);
}

#[test]
fn start_release_fix_prefers_the_release_branch_you_stand_on_over_discovery() {
    // Standing on release/1.3.0 inside a worktree: discovery would sort
    // release/1.0.0 first and fix the wrong release.
    let mut git = MockGit::new();
    git.current_branch = "release/1.3.0".to_string();
    git.branches_matching = vec!["release/1.0.0".to_string(), "release/1.3.0".to_string()];

    start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "broken-login", true, None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "create_branch_no_checkout:release-fix/1.3.0/broken-login:release/1.3.0",
        "push:release-fix/1.3.0/broken-login",
    ]);
}

#[test]
fn start_release_fix_discovery_bases_a_remote_only_release_branch_on_origin() {
    // list_branches_matching strips the origin/ prefix, so the short name of a
    // remote-only branch is not a resolvable object for `git branch`.
    let mut git = MockGit::new();
    git.current_branch = "develop".to_string();
    git.branches_matching = vec!["release/1.2.0".to_string()];

    start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "broken-login", true, None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:release/*",
        "tag_exists:v1.2.0",
        "tag_exists:1.2.0",
        "local_branch_exists:release/1.2.0",
        "create_branch_no_checkout:release-fix/1.2.0/broken-login:origin/release/1.2.0",
        "push:release-fix/1.2.0/broken-login",
    ]);
}

#[test]
fn start_hotfix_fix_prefers_the_hotfix_branch_you_stand_on_over_discovery() {
    let mut git = MockGit::new();
    git.current_branch = "hotfix/1.0.2".to_string();
    git.branches_matching = vec!["hotfix/1.0.1".to_string(), "hotfix/1.0.2".to_string()];

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", false, None, "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "create_branch:hotfix-fix/1.0.2/urgent-crash:hotfix/1.0.2",
        "push:hotfix-fix/1.0.2/urgent-crash",
    ]);
}

#[test]
fn start_hotfix_fix_no_checkout_bases_a_remote_only_hotfix_branch_on_origin() {
    // Without a checkout nothing repairs the missing local branch, so the
    // reused hotfix must be addressed through origin/.
    let mut git = MockGit::new();
    git.current_branch = "main".to_string();
    git.branches_matching = vec!["hotfix/1.0.1".to_string()];

    start_hotfix_fix(&git, &MockHosting::new(), &RepoConfig::default(), "urgent-crash", true, None, "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:hotfix/*",
        "tag_exists:v1.0.1",
        "tag_exists:1.0.1",
        "local_branch_exists:hotfix/1.0.1",
        "create_branch_no_checkout:hotfix-fix/1.0.1/urgent-crash:origin/hotfix/1.0.1",
        "push:hotfix-fix/1.0.1/urgent-crash",
    ]);
}

#[test]
fn start_release_fix_discovery_picks_the_newest_open_release() {
    // Name order would put release/1.0.0 first; a stale old line must not win.
    let mut git = MockGit::new();
    git.current_branch = "develop".to_string();
    git.branches_matching = vec!["release/1.0.0".to_string(), "release/1.3.0".to_string()];
    git.existing_local_branches.insert("release/1.3.0".to_string());

    start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "broken-login", true, None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:release/*",
        "tag_exists:v1.0.0",
        "tag_exists:1.0.0",
        "tag_exists:v1.3.0",
        "tag_exists:1.3.0",
        "local_branch_exists:release/1.3.0",
        "create_branch_no_checkout:release-fix/1.3.0/broken-login:release/1.3.0",
        "push:release-fix/1.3.0/broken-login",
    ]);
}

#[test]
fn start_release_fix_discovery_ranks_an_unversioned_release_last() {
    // "release/1.2" sorts before "release/1.2.0" by name but carries no version.
    let mut git = MockGit::new();
    git.current_branch = "develop".to_string();
    git.branches_matching = vec!["release/1.2".to_string(), "release/1.2.0".to_string()];
    git.existing_local_branches.insert("release/1.2.0".to_string());

    start_release_fix(&git, &MockHosting::new(), &RepoConfig::default(), "main", "broken-login", true, None).unwrap();

    assert_eq!(git.calls(), vec![
        "current_branch",
        "list_branches_matching:release/*",
        "tag_exists:v1.2.0",
        "tag_exists:1.2.0",
        "local_branch_exists:release/1.2.0",
        "create_branch_no_checkout:release-fix/1.2.0/broken-login:release/1.2.0",
        "push:release-fix/1.2.0/broken-login",
    ]);
}

#[test]
fn start_release_reuses_the_newest_open_release_branch() {
    // Pins the consumer side of the ordering contract: reuse must never land on
    // a stale older line just because its name sorts first.
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.0.0".to_string(), "release/1.3.0".to_string()];

    start_release(&git, &MockPrompter::new(), &MockHosting::new(), None, &RepoConfig::default(), Some(ReleaseType::Minor), "main", None).unwrap();

    assert_eq!(git.calls(), vec![
        "list_branches_matching:release/*",
        "tag_exists:v1.0.0",
        "tag_exists:1.0.0",
        "tag_exists:v1.3.0",
        "tag_exists:1.3.0",
        "checkout:release/1.3.0",
    ]);
}

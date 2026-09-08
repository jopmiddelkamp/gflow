mod common;

use std::path::PathBuf;

use common::{MockEditor, MockGit, MockHosting, MockPrompter};
use gflow::action::Action;
use gflow::cli::{Commands, StartKind, StartOptions};
use gflow::git::branch::BranchType;
use gflow::lifecycle::{resolve_action_with_state, run};
use gflow::repo_config::{Mode, RepoConfig};
use gflow::state::{FinishKind, FinishState};
use gflow::worktree::{WorktreeConfig, WorktreeEnv};
use common::MockWorktreeSetup;

// The lifecycle (reject → stash → write-state → dispatch → clear/pop) used to
// live in main.rs, where tests/ could not link it — the system's most
// safety-critical ordering was enforced by a comment. These tests pin it.

fn wt_config() -> WorktreeConfig {
    WorktreeConfig { enabled: false, editor: "code".to_string(), base_path: None }
}

fn finish_cmd() -> Option<Commands> {
    Some(Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false })
}

/// A MockGit standing on release/2.5.0 with one RC tag and a fake `.git` dir,
/// ready for a `gflow finish`.
fn release_git() -> MockGit {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "release/2.5.0".to_string();
    git.tags_on_branch = vec!["v2.5.0-rc.1".to_string()];
    git.existing_local_branches.insert("release/2.5.0".to_string());
    git.existing_remote_branches.insert("release/2.5.0".to_string());
    git
}

fn state_path(git_dir: &PathBuf) -> PathBuf {
    FinishState::path(git_dir, FinishKind::Release, 2, 5, 0)
}

fn run_lifecycle(git: &MockGit, command: Option<Commands>) -> Result<(), String> {
    let hosting = MockHosting::new();
    let prompter = MockPrompter::new();
    let editor = MockEditor::new();
    run(git, &hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &RepoConfig::default(), None, command)
}

// --- The --accept-merge-type flag reaches the flows ---

fn wrongly_squash_checked_feature_git(name: &str) -> MockGit {
    let mut git = MockGit::with_tmp_git_dir(name);
    git.current_branch = "feature/x".to_string();
    git.head_sha = "abc".to_string();
    git.parent_counts.insert("merge-of-abc".to_string(), 2);
    git
}

fn merged_feature_hosting() -> MockHosting {
    let mut hosting = MockHosting::new();
    hosting.merged_pr = Some(gflow::hosting::MergedPr {
        url: "https://github.com/o/r/pull/9".to_string(),
        head_sha: "abc".to_string(),
        merge_commit_sha: "merge-of-abc".to_string(),
        base: "develop".to_string(),
    });
    hosting
}

fn run_with(git: &MockGit, hosting: &MockHosting, command: Option<Commands>) -> Result<(), String> {
    let prompter = MockPrompter::new();
    let editor = MockEditor::new();
    run(git, hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &RepoConfig::default(), None, command)
}

#[test]
fn finish_without_the_flag_refuses_a_wrongly_completed_pr() {
    let git = wrongly_squash_checked_feature_git("gflow-accept-mt-refuse");
    let hosting = merged_feature_hosting();

    let err = run_with(&git, &hosting, Some(Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false })).unwrap_err();

    assert!(err.contains("--accept-merge-type"), "got: {err}");
}

#[test]
fn finish_with_the_flag_accepts_a_wrongly_completed_pr() {
    let git = wrongly_squash_checked_feature_git("gflow-accept-mt-accept");
    let hosting = merged_feature_hosting();

    run_with(&git, &hosting, Some(Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: true })).unwrap();

    assert!(git.calls().contains(&"delete_branch_local:feature/x".to_string()),
        "the accepted finish must complete; got: {:?}", git.calls());
}

#[test]
fn sync_forwards_the_accept_flag_to_the_landing_check() {
    let mut git = MockGit::with_tmp_git_dir("gflow-accept-mt-sync");
    git.current_branch = "release/1.1.0".to_string();
    git.branch_shas.insert("release/1.1.0".to_string(), "relsha".to_string());
    git.parent_counts.insert("mc1".to_string(), 1);
    let mut hosting = MockHosting::new();
    hosting.merged_prs_to.insert(
        ("release/1.1.0".to_string(), "develop".to_string()),
        gflow::hosting::LandedPr { url: "u".to_string(), head_sha: "relsha".to_string(), merge_commit_sha: "mc1".to_string() },
    );
    git.ancestors.insert(("mc1".to_string(), "origin/develop".to_string()));
    let prompter = MockPrompter::new();
    let editor = MockEditor::new();
    let cfg = RepoConfig { mode: Mode::Protected, ..RepoConfig::default() };

    let refused = run(&git, &hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &cfg, None,
        Some(Commands::Sync { accept_merge_type: false }));
    assert!(refused.unwrap_err().contains("--accept-merge-type"));

    run(&git, &hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &cfg, None,
        Some(Commands::Sync { accept_merge_type: true })).unwrap();
}

// --- State-before-mutation ordering ---

#[test]
fn crashed_finish_leaves_resumable_state_on_disk() {
    let mut git = release_git();
    git.fail_nth_merge = Some(1); // conflict on the main merge, mid-flow

    let result = run_lifecycle(&git, finish_cmd());

    assert!(result.is_err(), "merge conflict must surface");
    let state = FinishState::load(&git.git_dir, FinishKind::Release, 2, 5, 0)
        .unwrap()
        .expect("state must have been written BEFORE the first mutating git call, so a crash mid-flow leaves a resumable record");
    assert_eq!(state.source_branch(), "release/2.5.0");
}

#[test]
fn successful_finish_clears_state() {
    let git = release_git();

    run_lifecycle(&git, finish_cmd()).unwrap();

    assert!(!state_path(&git.git_dir).exists(),
        "state must be cleared after a successful finish");
    assert!(git.calls().iter().any(|c| c.starts_with("merge:release/2.5.0:")),
        "the finish flow must actually have run");
    assert!(!git.calls().contains(&"worktree_root".to_string()),
        "free mode never opens a landing PR, so it must never resolve a landing template; calls: {:?}", git.calls());
}

#[test]
fn dirty_tree_finish_rejected_before_any_side_effect() {
    let mut git = release_git();
    git.working_tree_clean = false;

    let err = run_lifecycle(&git, finish_cmd()).unwrap_err();

    assert!(err.contains("Working tree is not clean"), "got: {err}");
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("stash_push_with_message")),
        "nothing may be stashed before the reject; calls: {calls:?}");
    assert!(!state_path(&git.git_dir).exists(),
        "no state may be written before the reject");
    assert!(!calls.iter().any(|c| c.starts_with("merge:")),
        "no mutation may run; calls: {calls:?}");
}

fn protected_cfg() -> RepoConfig {
    RepoConfig { mode: Mode::Protected, keep_release_branches: false, ..RepoConfig::default() }
}

#[test]
fn protected_finish_writes_no_state_file() {
    // Protected finishes never merge locally — they open a landing PR and stop
    // for a human to merge — so there is nothing to resume and no FinishState
    // is ever written, even though this run ends Pending (Ok) rather than
    // fully complete.
    let mut git = release_git();
    git.pushed_branches.insert("release/2.5.0".to_string());
    let hosting = MockHosting::new();
    let prompter = MockPrompter::new();
    let editor = MockEditor::new();

    let result = run(&git, &hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &protected_cfg(), None, finish_cmd());

    assert!(result.is_ok(), "a pending landing is not a failure: {result:?}");
    assert!(!state_path(&git.git_dir).exists(),
        "protected finishes never write FinishState");
    assert!(!FinishState::dir(&git.git_dir).exists(),
        "no per-branch state directory may be created; nothing to resume");
    assert!(git.calls().contains(&"worktree_root".to_string()),
        "protected finishes resolve a landing PR template from the current worktree; calls: {:?}", git.calls());
    assert!(!git.calls().contains(&"repo_root".to_string()),
        "a landing template belongs to the branch being finished, not the main tree — \
         must resolve via worktree_root, never repo_root; calls: {:?}", git.calls());
}

#[test]
fn protected_finish_rejects_dirty_tree() {
    let mut git = release_git();
    git.working_tree_clean = false;
    let hosting = MockHosting::new();
    let prompter = MockPrompter::new();
    let editor = MockEditor::new();

    let err = run(&git, &hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &protected_cfg(), None, finish_cmd()).unwrap_err();

    assert_eq!(err, "Working tree is not clean. Commit your changes before finishing.");
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("stash_push_with_message")),
        "nothing may be stashed before the reject; calls: {calls:?}");
    assert!(!state_path(&git.git_dir).exists(),
        "no state may be written before the reject");
    assert!(!calls.iter().any(|c| c.starts_with("merge:")),
        "no mutation may run; calls: {calls:?}");
}

#[test]
fn mid_merge_preflight_blocks_and_names_pending_resume() {
    let mut git = release_git();
    git.mid_merge = true;
    // An in-progress finish is waiting.
    FinishState {
        kind: FinishKind::Release, major: 2, minor: 5, patch: 0,
        started_at: "1".to_string(), stash_message: None,
    }.save(&git.git_dir).unwrap();

    let err = run_lifecycle(&git, finish_cmd()).unwrap_err();

    assert!(err.contains("Unresolved merge in progress"), "got: {err}");
    assert!(err.contains("release/2.5.0"), "must name the waiting finish; got: {err}");
    assert!(!git.calls().iter().any(|c| c == "fetch"),
        "preflight must block before fetch");
}

#[test]
fn unmerged_paths_block_even_when_the_merge_was_committed() {
    // `git commit` ends the MERGE_HEAD state, but conflict markers can still be
    // staged unresolved.
    let mut git = release_git();
    git.mid_merge = false;
    git.unmerged_paths = true;

    let err = run_lifecycle(&git, finish_cmd()).unwrap_err();

    assert!(err.contains("Unresolved merge in progress"), "got: {err}");
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c == "fetch"),
        "preflight must block before fetch; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("merge:") || c.starts_with("stash_push_with_message")),
        "no mutation may run; calls: {calls:?}");
}

// --- Re-run from a finish branch (post-conflict switch-back) ---
// A conflicted landing-branch merge leaves the worktree on the finish branch
// (see ensure_finish_branch). After the user commits the resolution, re-running
// the finish/sync must switch back to the source branch and continue — the
// conflict hint promises exactly that.

/// release_git() left on its own finish branch, resolution committed (clean,
/// not mid-merge), source pushed — ready for a protected re-run.
fn resolved_finish_branch_git(target: &str) -> MockGit {
    let mut git = release_git();
    git.current_branch = format!("finish/release-2.5.0-into-{target}");
    git.pushed_branches.insert("release/2.5.0".to_string());
    git
}

fn run_protected(git: &MockGit, command: Option<Commands>) -> Result<(), String> {
    let hosting = MockHosting::new();
    let prompter = MockPrompter::new();
    let editor = MockEditor::new();
    run(git, &hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &protected_cfg(), None, command)
}

#[test]
fn finish_on_a_finish_branch_switches_back_to_the_source_first() {
    let git = resolved_finish_branch_git("main");

    let result = run_protected(&git, finish_cmd());

    assert!(result.is_ok(), "the finish must continue from the source branch: {result:?}");
    let calls = git.calls();
    let switch = calls.iter().position(|c| c == "checkout:release/2.5.0")
        .unwrap_or_else(|| panic!("must switch back to the source branch; calls: {calls:?}"));
    let fetch = calls.iter().position(|c| c == "fetch")
        .unwrap_or_else(|| panic!("the finish must proceed after the switch; calls: {calls:?}"));
    assert!(switch < fetch, "switch-back must precede dispatch; calls: {calls:?}");
}

#[test]
fn sync_on_a_finish_branch_switches_back_to_the_source_first() {
    let git = resolved_finish_branch_git("develop");

    let result = run_protected(&git, Some(Commands::Sync { accept_merge_type: false }));

    assert!(result.is_ok(), "the sync must continue from the source branch: {result:?}");
    assert!(git.calls().contains(&"checkout:release/2.5.0".to_string()),
        "must switch back to the source branch; calls: {:?}", git.calls());
}

#[test]
fn mid_merge_on_a_finish_branch_blocks_with_the_merge_message_not_a_switch() {
    let mut git = resolved_finish_branch_git("main");
    git.mid_merge = true;

    let err = run_protected(&git, finish_cmd()).unwrap_err();

    assert!(err.contains("Unresolved merge in progress"),
        "the user mid-resolution must get the merge message, not 'Nothing to finish'; got: {err}");
    let calls = git.calls();
    assert!(!calls.contains(&"checkout:release/2.5.0".to_string()),
        "never switch away from an unfinished merge; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c == "fetch"),
        "preflight must block before fetch; calls: {calls:?}");
}

#[test]
fn unmerged_paths_on_a_finish_branch_block_without_a_switch() {
    let mut git = resolved_finish_branch_git("main");
    git.unmerged_paths = true;

    let err = run_protected(&git, finish_cmd()).unwrap_err();

    assert!(err.contains("Unresolved merge in progress"), "got: {err}");
    assert!(!git.calls().contains(&"checkout:release/2.5.0".to_string()),
        "never switch away from unresolved paths; calls: {:?}", git.calls());
}

#[test]
fn abort_on_a_finish_branch_does_not_switch() {
    let git = resolved_finish_branch_git("main");

    let result = run_protected(&git, Some(Commands::Finish { breaking: None, base: None, abort: true, accept_merge_type: false }));

    assert!(result.is_ok(), "abort is a no-op without state: {result:?}");
    assert!(!git.calls().contains(&"checkout:release/2.5.0".to_string()),
        "abort must not switch branches; calls: {:?}", git.calls());
}

#[test]
fn finish_with_explicit_base_on_a_finish_branch_does_not_switch() {
    let git = resolved_finish_branch_git("main");

    run_protected(&git, Some(Commands::Finish { breaking: None, base: Some("develop".to_string()), abort: false, accept_merge_type: false }))
        .expect_err("a finish branch is not a work branch, so --base must be rejected");
    assert!(!git.calls().contains(&"checkout:release/2.5.0".to_string()),
        "an explicit --base is not a finish-branch resume; calls: {:?}", git.calls());
}

// --- Stash policy (dirty start: stash before mutation, pop on success) ---

#[test]
fn dirty_start_stashes_before_mutating_and_pops_on_success() {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "develop".to_string();
    git.working_tree_clean = false;
    let start = Some(Commands::Start { kind: StartKind::Feature {
        name: "login".to_string(),
        base: "develop".to_string(),
        opts: StartOptions::default(),
    }});

    run_lifecycle(&git, start).unwrap();

    let calls = git.calls();
    let stash_idx = calls.iter().position(|c| c.starts_with("stash_push_with_message:gflow-finish:develop:"))
        .expect("dirty start must stash");
    let create_idx = calls.iter().position(|c| c.starts_with("create_branch:feature/login:"))
        .expect("branch must be created");
    let pop_idx = calls.iter().position(|c| c.starts_with("stash_pop_ref:"))
        .expect("stash must be popped on success");
    assert!(stash_idx < create_idx, "stash must precede the first mutation; calls: {calls:?}");
    assert!(pop_idx > create_idx, "pop must follow the flow; calls: {calls:?}");
    assert!(!state_path(&git.git_dir).exists(),
        "start actions never write finish state");
}

// --- Resume ---

#[test]
fn resume_runs_flow_from_state_and_clears_it_on_success() {
    // Fully-completed world: resume should re-drive the flow (all steps skip),
    // then clear the state file.
    let mut git = release_git();
    git.ancestors.insert(("release/2.5.0".to_string(), "main".to_string()));
    git.ancestors.insert(("release/2.5.0".to_string(), "develop".to_string()));
    git.existing_tags.insert("v2.5.0".to_string());
    git.existing_remote_tags.insert("v2.5.0".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    FinishState {
        kind: FinishKind::Release, major: 2, minor: 5, patch: 0,
        started_at: "1".to_string(), stash_message: None,
    }.save(&git.git_dir).unwrap();

    run_lifecycle(&git, finish_cmd()).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("merge:")),
        "resume of a completed finish must not re-merge; calls: {calls:?}");
    assert!(!state_path(&git.git_dir).exists(),
        "state must be cleared after a successful resume");
}

#[test]
fn failed_resume_keeps_state_for_the_next_attempt() {
    let mut git = release_git();
    git.fail_nth_merge = Some(1);
    FinishState {
        kind: FinishKind::Release, major: 2, minor: 5, patch: 0,
        started_at: "1".to_string(), stash_message: None,
    }.save(&git.git_dir).unwrap();

    let result = run_lifecycle(&git, finish_cmd());

    assert!(result.is_err());
    assert!(state_path(&git.git_dir).exists(),
        "a failed resume must keep the state file for the next attempt");
}

// --- Abort ---

#[test]
fn abort_clears_state_without_touching_the_repo() {
    let mut git = release_git();
    git.mid_merge = true; // abort must work even mid-merge
    FinishState {
        kind: FinishKind::Release, major: 2, minor: 5, patch: 0,
        started_at: "1".to_string(), stash_message: Some("gflow-finish:release/2.5.0:1".to_string()),
    }.save(&git.git_dir).unwrap();

    run_lifecycle(&git, Some(Commands::Finish { breaking: None, base: None, abort: true, accept_merge_type: false })).unwrap();

    assert!(!state_path(&git.git_dir).exists(), "abort must discard the state file");
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c == "fetch"), "abort must not fetch");
    assert!(!calls.iter().any(|c| c.starts_with("merge:") || c.starts_with("checkout:")
            || c.starts_with("stash_pop_ref:")),
        "abort must not mutate the repo or auto-pop the stash; calls: {calls:?}");
}

// --- Action resolution (moved from main.rs with the lifecycle) ---

fn release_state() -> FinishState {
    FinishState {
        kind: FinishKind::Release,
        major: 2,
        minor: 5,
        patch: 0,
        started_at: "0".to_string(),
        stash_message: None,
    }
}

#[test]
fn resume_state_resumes_finish_without_base() {
    let branch_type = BranchType::Release { major: 2, minor: 5, patch: 0 };
    let state = release_state();
    let cmd = Some(Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false });

    let action = resolve_action_with_state(cmd, &MockPrompter::new(), &branch_type, "release/2.5.0", Some(&state), false, "main").unwrap();

    assert!(matches!(action, Action::FinishRelease));
}

#[test]
fn abort_is_accepted_from_any_branch_including_unrecognized_ones() {
    // decisions.md: "--abort short-circuits before every check" — abort is
    // itself a recovery action, so no branch-type gate may fire for it.
    for branch_type in [
        BranchType::Other,
        BranchType::Main,
        BranchType::Develop,
        BranchType::Release { major: 2, minor: 5, patch: 0 },
    ] {
        let cmd = Some(Commands::Finish { breaking: None, base: None, abort: true, accept_merge_type: false });

        let action = resolve_action_with_state(cmd, &MockPrompter::new(), &branch_type, "whatever", None, false, "main").unwrap();

        assert!(matches!(action, Action::AbortFinish), "branch type {branch_type:?} rejected --abort");
    }
}

// --- The worktree-active predicate pair ---
//
// Two places encode "the worktree flow is active": cli::auto_discovers_target,
// which waives the must-be-standing-on-it gate, and lifecycle's
// `worktree_active`, which decides whether a WorktreeContext is built.

fn worktree_lifecycle_git() -> MockGit {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "develop".to_string();
    git.branches_matching = vec!["release/2.5.0".to_string()];
    git.existing_local_branches.insert("release/2.5.0".to_string());
    git
}

fn start_release_fix_cmd(no_worktree: bool) -> Option<Commands> {
    Some(Commands::Start { kind: StartKind::ReleaseFix {
        name: "db-index".to_string(),
        opts: StartOptions { no_worktree, ..Default::default() },
    }})
}

fn enabled_wt_config() -> WorktreeConfig {
    WorktreeConfig {
        enabled: true,
        editor: "none".to_string(),
        base_path: Some(std::env::temp_dir().to_string_lossy().to_string()),
    }
}

fn run_with_worktree(git: &MockGit, command: Option<Commands>) -> Result<(), String> {
    run(git, &MockHosting::new(), &MockPrompter::new(), &WorktreeEnv { config: &enabled_wt_config(), editor: &MockEditor::new(), setup: &MockWorktreeSetup::new(), commands: None }, &RepoConfig::default(), None, command)
}

#[test]
fn an_enabled_worktree_flow_waives_the_gate_and_actually_materializes_a_worktree() {
    let git = worktree_lifecycle_git();

    run_with_worktree(&git, start_release_fix_cmd(false)).unwrap();

    let calls = git.calls();
    assert!(calls.iter().any(|c| c == "create_branch_no_checkout:release-fix/2.5.0/db-index:release/2.5.0"),
        "the gate was waived, so the branch must be created without switching; calls: {calls:?}");
    assert!(calls.iter().any(|c| c.starts_with("add_worktree:")),
        "waiving the gate without materializing a worktree is the divergence this pins; calls: {calls:?}");
}

#[test]
fn no_worktree_re_arms_the_gate_it_waived() {
    let git = worktree_lifecycle_git();

    let err = run_with_worktree(&git, start_release_fix_cmd(true)).unwrap_err();

    assert_eq!(err, "This command is only valid on a release branch.");
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("add_worktree:") || c.starts_with("create_branch")),
        "nothing may be created; calls: {calls:?}");
}

// --- run_flow dispatch: every Action arm reaches its own flow ---
//
// decisions.md: "One Action enum is the single currency" — lifecycle::run_flow is
// where that currency is spent. An arm wired to the wrong flow (or to the wrong
// version source) is invisible until a user hits it, so each arm is pinned by the
// git calls only that flow makes.

#[test]
fn start_release_dispatches_to_the_release_flow() {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "develop".to_string();
    git.tags = vec!["v2.4.0".to_string()];

    run_lifecycle(&git, Some(Commands::Start {
        kind: StartKind::Release { major: true, minor: false, no_worktree: false },
    })).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"create_branch:release/3.0.0:develop".to_string()),
        "--major must cut release/3.0.0 from v2.4.0; calls: {calls:?}");
    assert!(calls.contains(&"push_tag:v3.0.0-rc.1".to_string()),
        "a new release branch is tagged rc.1; calls: {calls:?}");
}

#[test]
fn start_release_fix_dispatches_from_the_release_branch() {
    let git = release_git();

    run_lifecycle(&git, Some(Commands::Start {
        kind: StartKind::ReleaseFix {
            name: "db-index".to_string(),
            opts: StartOptions::default(),
        },
    })).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"create_branch:release-fix/2.5.0/db-index:release/2.5.0".to_string()),
        "release fixes branch off the release they fix; calls: {calls:?}");
}

#[test]
fn start_hotfix_fix_dispatches_and_creates_the_hotfix_branch() {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "main".to_string();
    git.tags = vec!["v2.5.0".to_string()];

    run_lifecycle(&git, Some(Commands::Start {
        kind: StartKind::HotfixFix {
            name: "npe".to_string(),
            opts: StartOptions::default(),
        },
    })).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"create_branch:hotfix/2.5.1:main".to_string()),
        "no hotfix branch yet — one is cut from main at the next patch; calls: {calls:?}");
    assert!(calls.contains(&"create_branch:hotfix-fix/2.5.1/npe:hotfix/2.5.1".to_string()),
        "the fix branches off that hotfix; calls: {calls:?}");
}

#[test]
fn finish_release_fix_dispatches_and_targets_its_release_branch() {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "release-fix/2.5.0/db-index".to_string();
    let hosting = MockHosting::new();
    let prompter = MockPrompter::new();
    let editor = MockEditor::new();

    run(&git, &hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &RepoConfig::default(), None, finish_cmd()).unwrap();

    assert!(hosting.calls().iter().any(|c| c.starts_with("create_or_get_pr:release-fix/2.5.0/db-index:release/2.5.0:")),
        "a release fix PRs back into its own release branch; calls: {:?}", hosting.calls());
}

#[test]
fn finish_release_chore_dispatches_and_targets_its_release_branch() {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "release-chore/2.5.0/set-version".to_string();
    let hosting = MockHosting::new();
    let prompter = MockPrompter::new();
    let editor = MockEditor::new();

    run(&git, &hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &RepoConfig::default(), None, finish_cmd()).unwrap();

    assert!(hosting.calls().iter().any(|c| c.starts_with("create_or_get_pr:release-chore/2.5.0/set-version:release/2.5.0:")),
        "a release chore PRs back into its own release branch; calls: {:?}", hosting.calls());
}

#[test]
fn finish_hotfix_fix_dispatches_and_targets_its_hotfix_branch() {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "hotfix-fix/2.5.1/npe".to_string();
    let hosting = MockHosting::new();
    let prompter = MockPrompter::new();
    let editor = MockEditor::new();

    run(&git, &hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &RepoConfig::default(), None, finish_cmd()).unwrap();

    assert!(hosting.calls().iter().any(|c| c.starts_with("create_or_get_pr:hotfix-fix/2.5.1/npe:hotfix/2.5.1:")),
        "a hotfix fix PRs back into its own hotfix branch; calls: {:?}", hosting.calls());
}

#[test]
fn bump_dispatches_and_cuts_the_next_rc() {
    let git = release_git(); // already carries v2.5.0-rc.1

    run_lifecycle(&git, Some(Commands::Bump)).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"push_tag:v2.5.0-rc.2".to_string()),
        "bump cuts the next RC on the release branch; calls: {calls:?}");
    assert!(!state_path(&git.git_dir).exists(), "bump writes no finish state");
}

#[test]
fn sync_dispatches_and_returns_to_the_release_branch() {
    let git = release_git();

    run_lifecycle(&git, Some(Commands::Sync { accept_merge_type: false })).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"merge:release/2.5.0:chore: sync release 2.5.0 with develop".to_string()),
        "sync merges the release branch into develop; calls: {calls:?}");
    assert!(calls.contains(&"push:develop".to_string()), "calls: {calls:?}");
    assert_eq!(calls.last().map(String::as_str), Some("checkout:release/2.5.0"),
        "sync must leave you where you started; calls: {calls:?}");
}

// --- Hotfix finish identity (the release half was covered; this half was not) ---

fn hotfix_git() -> MockGit {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "hotfix/2.5.1".to_string();
    git.existing_local_branches.insert("hotfix/2.5.1".to_string());
    git.existing_remote_branches.insert("hotfix/2.5.1".to_string());
    git
}

#[test]
fn crashed_hotfix_finish_writes_hotfix_state_not_release_state() {
    let mut git = hotfix_git();
    git.fail_nth_merge = Some(1);

    let result = run_lifecycle(&git, finish_cmd());

    assert!(result.is_err(), "merge conflict must surface");
    let state = FinishState::load(&git.git_dir, FinishKind::Hotfix, 2, 5, 1)
        .unwrap()
        .expect("a crashed hotfix finish must leave hotfix-kind state");
    assert_eq!(state.source_branch(), "hotfix/2.5.1");
    assert!(FinishState::load(&git.git_dir, FinishKind::Release, 2, 5, 1).unwrap().is_none(),
        "per-branch state identity: a hotfix finish must never be loadable as a release finish");
}

#[test]
fn hotfix_resume_takes_its_version_from_state_not_the_branch() {
    // decisions.md: "version comes from state, not the branch you're standing on".
    // HEAD is left on develop by a conflicted develop-merge; the resume must still
    // finish 2.5.1.
    let mut git = hotfix_git();
    git.ancestors.insert(("hotfix/2.5.1".to_string(), "main".to_string()));
    git.ancestors.insert(("hotfix/2.5.1".to_string(), "develop".to_string()));
    git.existing_tags.insert("v2.5.1".to_string());
    git.existing_remote_tags.insert("v2.5.1".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    FinishState {
        kind: FinishKind::Hotfix, major: 2, minor: 5, patch: 1,
        started_at: "1".to_string(), stash_message: None,
    }.save(&git.git_dir).unwrap();

    run_lifecycle(&git, finish_cmd()).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("merge:")),
        "resume of a completed hotfix must not re-merge; calls: {calls:?}");
    assert!(!FinishState::path(&git.git_dir, FinishKind::Hotfix, 2, 5, 1).exists(),
        "state must be cleared after a successful hotfix resume");
}

// --- Abort with nothing in progress ---

#[test]
fn abort_without_state_succeeds_and_does_nothing() {
    // decisions.md: "--abort ... succeeds on a clean repo — safe to run speculatively".
    let git = release_git();

    run_lifecycle(&git, Some(Commands::Finish { breaking: None, base: None, abort: true, accept_merge_type: false })).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c == "fetch"), "abort must not fetch; calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("merge:") || c.starts_with("checkout:")),
        "abort must not mutate the repo; calls: {calls:?}");
}

// --- Stash-pop policy: the failure branches ---

#[test]
fn failed_stash_pop_warns_and_still_reports_the_flow_result() {
    // Warn-and-continue: the work already succeeded, so a failed pop must not turn
    // a successful start into an error — the user is told where their changes are.
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "develop".to_string();
    git.working_tree_clean = false;
    git.fail_stash_pop = true;

    let result = run_lifecycle(&git, Some(Commands::Start { kind: StartKind::Feature {
        name: "login".to_string(),
        base: "develop".to_string(),
        opts: StartOptions::default(),
    }}));

    assert!(result.is_ok(), "a failed pop must not fail the finished work: {result:?}");
    let calls = git.calls();
    assert!(calls.iter().any(|c| c.starts_with("stash_pop_ref:")), "pop must be attempted; calls: {calls:?}");
}

#[test]
fn stash_lookup_failure_warns_and_never_pops_blindly() {
    // decisions.md, Stash Policy: never a blind `stash pop`. If the message lookup
    // fails we warn — popping stash@{0} could destroy a stash the user pushed.
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "develop".to_string();
    git.working_tree_clean = false;
    git.fail_find_stash = true;

    let result = run_lifecycle(&git, Some(Commands::Start { kind: StartKind::Feature {
        name: "login".to_string(),
        base: "develop".to_string(),
        opts: StartOptions::default(),
    }}));

    assert!(result.is_ok(), "a failed stash lookup must not fail the finished work: {result:?}");
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("stash_pop_ref:")),
        "no blind pop when the lookup failed; calls: {calls:?}");
}

#[test]
fn failed_release_finish_keeps_the_stash_for_resume() {
    // Three-way pop policy: a failed release/hotfix finish keeps the stash so the
    // resume inherits it instead of stashing twice.
    let mut git = release_git();
    git.working_tree_clean = false;
    git.fail_nth_merge = Some(1);
    FinishState {
        kind: FinishKind::Release, major: 2, minor: 5, patch: 0,
        started_at: "1".to_string(), stash_message: Some("gflow-finish:release/2.5.0:1".to_string()),
    }.save(&git.git_dir).unwrap();

    let result = run_lifecycle(&git, finish_cmd());

    assert!(result.is_err());
    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("stash_pop_ref:")),
        "a failed finish must keep the stash for the resume; calls: {calls:?}");
}

// --- Current-branch sync before the flow ---

#[test]
fn missing_upstream_is_swallowed_but_a_real_ff_merge_error_aborts() {
    // A brand-new local branch has no origin/<branch> — git says "not something we
    // can merge" and that is normal. Any other ff-merge failure is a real problem
    // and must abort before the flow mutates anything.
    let mut git = release_git();
    git.ff_merge_error = Some("fatal: refusing to merge unrelated histories".to_string());

    let err = run_lifecycle(&git, finish_cmd()).unwrap_err();

    assert!(err.contains("unrelated histories"), "the real error must surface; got: {err}");
    assert!(!git.calls().iter().any(|c| c.starts_with("merge:")),
        "the flow must not run after a real sync failure; calls: {:?}", git.calls());
}

#[test]
fn brand_new_branch_without_upstream_still_runs_its_flow() {
    let mut git = release_git();
    git.ff_merge_error = Some("merge: origin/release/2.5.0 - not something we can merge".to_string());

    let result = run_lifecycle(&git, finish_cmd());

    assert!(result.is_err(), "the release flow itself fails later — on the main ff_merge");
    assert!(git.calls().iter().any(|c| c == "ff_merge:origin/release/2.5.0"),
        "the current-branch sync must have been attempted; calls: {:?}", git.calls());
}

#[test]
fn explicit_base_rejected_even_when_resume_state_exists() {
    // Regression: the fixed-target --base guard lives in resolve_action, which the
    // resume early-return used to skip — silently swallowing an invalid --base.
    let branch_type = BranchType::Release { major: 2, minor: 5, patch: 0 };
    let state = release_state();
    let cmd = Some(Commands::Finish { breaking: None, base: Some("develop".to_string()), abort: false, accept_merge_type: false });

    let err = resolve_action_with_state(cmd, &MockPrompter::new(), &branch_type, "release/2.5.0", Some(&state), false, "main").unwrap_err();

    assert!(err.contains("--base"), "Expected the fixed-target --base error, got: {err}");
}

#[test]
fn finish_work_branch_dispatches_with_its_resolved_pr_template() {
    // The work-branch finish is the only arm that resolves a PR template before
    // dispatch — flows never probe the filesystem themselves.
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "feature/login".to_string();
    git.remote_branches = vec!["develop".to_string()];
    let hosting = MockHosting::new();
    let prompter = MockPrompter::scripted(&[0]); // "no" to breaking changes
    let editor = MockEditor::new();

    run(&git, &hosting, &prompter, &WorktreeEnv { config: &wt_config(), editor: &editor, setup: &MockWorktreeSetup::new(), commands: None }, &RepoConfig::default(), None, finish_cmd()).unwrap();

    assert!(hosting.calls().iter().any(|c| c.starts_with("create_or_get_pr:feature/login:develop:feat: login")),
        "calls: {:?}", hosting.calls());
    assert!(git.calls().contains(&"worktree_root".to_string()),
        "template resolution is anchored to the current worktree's root; calls: {:?}", git.calls());
    assert!(!git.calls().contains(&"repo_root".to_string()),
        "a PR template belongs to the branch being finished, not the main tree — \
         must resolve via worktree_root, never repo_root; calls: {:?}", git.calls());
}

#[test]
fn a_stash_that_vanished_before_the_pop_is_not_an_error() {
    // The state records a stash *message*, not an index. If the stash is already
    // gone (user popped it by hand between runs) there is nothing to restore and
    // nothing to complain about.
    let mut git = release_git();
    git.ancestors.insert(("release/2.5.0".to_string(), "main".to_string()));
    git.ancestors.insert(("release/2.5.0".to_string(), "develop".to_string()));
    git.existing_tags.insert("v2.5.0".to_string());
    git.existing_remote_tags.insert("v2.5.0".to_string());
    git.pushed_branches.insert("main".to_string());
    git.pushed_branches.insert("develop".to_string());
    FinishState {
        kind: FinishKind::Release, major: 2, minor: 5, patch: 0,
        started_at: "1".to_string(),
        stash_message: Some("gflow-finish:release/2.5.0:1".to_string()), // never actually stashed
    }.save(&git.git_dir).unwrap();

    run_lifecycle(&git, finish_cmd()).unwrap();

    let calls = git.calls();
    assert!(calls.contains(&"find_stash_by_message:gflow-finish:release/2.5.0:1".to_string()),
        "calls: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("stash_pop_ref:")),
        "nothing to pop — and never a blind pop; calls: {calls:?}");
}

#[test]
fn no_subcommand_falls_through_to_the_interactive_menu() {
    // Bare `gflow` has no command and no resume state: the branch-type menu
    // decides, and whatever it returns is the Action that runs.
    let prompter = MockPrompter::scripted(&[5]); // "start release" on develop

    let action = resolve_action_with_state(None, &prompter, &BranchType::Develop, "develop", None, false, "main").unwrap();

    assert!(matches!(action, Action::StartRelease { release_type: None, .. }), "{action:?}");
    assert_eq!(prompter.calls().len(), 1);
}

#[test]
fn a_worktree_mode_start_release_still_stashes_a_dirty_tree() {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "develop".to_string();
    git.tags = vec!["v2.4.0".to_string()];
    git.working_tree_clean = false;

    run_with_worktree(&git, Some(Commands::Start {
        kind: StartKind::Release { major: false, minor: true, no_worktree: false },
    })).unwrap();

    let calls = git.calls();
    assert!(calls.iter().any(|c| c.starts_with("stash_push_with_message:")),
        "the release is created in the current tree, so a dirty tree must be stashed first; calls: {calls:?}");
    assert!(calls.contains(&"create_branch:release/2.5.0:develop".to_string()), "calls: {calls:?}");
}

#[test]
fn a_worktree_mode_start_release_with_no_worktree_stays_in_the_current_tree() {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-test");
    git.current_branch = "develop".to_string();
    git.tags = vec!["v2.4.0".to_string()];

    run_with_worktree(&git, Some(Commands::Start {
        kind: StartKind::Release { major: false, minor: true, no_worktree: true },
    })).unwrap();

    let calls = git.calls();
    assert!(!calls.iter().any(|c| c.starts_with("add_worktree:")), "--no-worktree must opt out; calls: {calls:?}");
    assert!(calls.contains(&"create_branch:release/2.5.0:develop".to_string()), "calls: {calls:?}");
    let after_tag: Vec<&String> = calls.iter().skip_while(|c| *c != "push_tag:v2.5.0-rc.1").skip(1).collect();
    assert!(!after_tag.contains(&&"checkout:develop".to_string()), "no hand-off checkout without a worktree; calls: {calls:?}");
}

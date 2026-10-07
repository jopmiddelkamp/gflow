//! Cross-cutting lifecycle of a gflow invocation: resume lookup, action
//! resolution, the reject → stash → write-state → dispatch ordering contract,
//! state clearing, and the three-way stash-pop policy (see decisions.md,
//! State & Crash-Safety). Lives in the library — not `main.rs` — so the
//! crash-safety ordering is enforced by tests (`tests/lifecycle_test.rs`),
//! not by a comment. `main.rs` keeps only adapter construction and preflight.

use crate::action::Action;
use crate::cli::{resolve_action, Commands};
use crate::flows::{finish_hotfix, finish_release, finish_work, start};
use crate::git::branch::BranchType;
use crate::git::Git;
use crate::hosting::HostingPlatform;
use crate::mainline::resolve_main_branch;
use crate::menu;
use crate::prompt::Prompter;
use crate::repo_config::{Mode, RepoConfig};
use crate::state::{current_timestamp, FinishKind, FinishState};
use crate::version::finish_branch_source;
use crate::version_script::VersionScript;
use crate::worktree::{WorktreeContext, WorktreeEnv};

#[allow(clippy::too_many_arguments)]
pub fn run(
    git: &dyn Git,
    hosting: &dyn HostingPlatform,
    prompter: &dyn Prompter,
    worktree_env: &WorktreeEnv<'_>,
    repo_cfg: &RepoConfig,
    script: Option<&dyn VersionScript>,
    command: Option<Commands>,
) -> Result<(), String> {
    let branch_name = git.current_branch()?;
    let git_dir = git.git_dir()?;
    let wt_config = worktree_env.config;

    // One-time upgrade of any pre-2.4 global state file into the per-branch folder.
    FinishState::migrate_legacy(&git_dir)?;

    // A conflicted landing merge leaves the worktree on the finish branch
    // (ensure_finish_branch). An explicit finish/sync re-run from there means
    // the resolution is committed: switch back to the source branch and
    // continue — finish_merge_conflict_hint promises exactly this. Still
    // mid-merge, give the merge message instead of dispatch's "not a gitflow
    // branch" rejection. --abort and an explicit --base are not resumes.
    let branch_name = match finish_branch_source(&branch_name) {
        Some(source) if is_finish_branch_rerun(&command) => {
            if git.is_mid_merge()? || git.has_unmerged_paths()? {
                return Err(unresolved_merge_message(None));
            }
            println!("Conflict resolution on {branch_name} is committed — switching this worktree back to {source}...");
            git.checkout(&source)?;
            source
        }
        _ => branch_name,
    };

    let branch_type = BranchType::parse(&branch_name);

    let main_branch = resolve_main_branch(git)?;

    let identity = finish_identity(&branch_type);

    // Resume context: an in-progress finish only resumes when you are standing on
    // the source branch that started it. From develop/main/feature branches there
    // is no resume — gflow behaves normally — so a stalled finish never hijacks
    // other work. To continue after a conflict you switch back to the source
    // branch and re-run 'gflow finish'.
    let resume_state = match identity {
        Some((kind, major, minor, patch)) => FinishState::load(&git_dir, kind, major, minor, patch)?,
        None => None,
    };

    // The completion-type override is per-invocation, not part of the Action:
    // the menu path has no flags, so it always runs strict (false).
    let accept_merge_type = matches!(
        &command,
        Some(Commands::Finish { accept_merge_type: true, .. }) | Some(Commands::Sync { accept_merge_type: true })
    );

    if command.is_none() {
        git.start_fetch();
    }

    // Resolve the action up-front so we can decide whether to fetch / stash / etc.
    let action = resolve_action_with_state(command, prompter, &branch_type, &branch_name, resume_state.as_ref(), wt_config.enabled, &main_branch)?;

    // --abort short-circuits before any state-changing operation, even if the repo
    // is mid-merge — abort is itself a recovery action.
    if matches!(action, Action::AbortFinish) {
        return handle_abort(&git_dir, resume_state);
    }

    // Mid-merge / unmerged-paths preflight (all other paths).
    if git.is_mid_merge()? || git.has_unmerged_paths()? {
        return Err(unresolved_merge_message(resume_state.as_ref()));
    }

    println!("Fetching latest...");
    git.fetch()?;

    // Optional worktree flow: when enabled (and not opted out) for a start, treat
    // it like --no-checkout so the current working tree is left untouched and the
    // new branch is free to be checked out in its own worktree. A release is the
    // exception: it is created in the current tree (the version script needs the
    // branch checked out) and only then handed to a worktree, so it keeps the
    // normal stash protection instead of the no-checkout shortcut.
    let worktree_active = wt_config.enabled && !action.no_worktree() && action.is_start();

    let no_checkout = action.no_checkout()
        || (worktree_active && !matches!(action, Action::StartRelease { .. }));
    // Protected finishes never merge locally — there is nothing to resume — so
    // only free-mode finishes get a FinishState. Stale-state edge: a state file
    // left over from before a free→protected mode switch still resumes here
    // (resolve_action_with_state is mode-unaware), so the protected flow runs,
    // ignores the state's stash/progress, and a successful run (including a
    // "Pending" landing) clears the now-meaningless file below.
    let is_finish_with_state = matches!(action, Action::FinishRelease | Action::FinishHotfix)
        && repo_cfg.mode == Mode::Free;
    let needs_stash = !no_checkout && branch_type != BranchType::Other && !git.is_working_tree_clean()?;

    // Reject dirty-tree finishes BEFORE any side effects (stash, state write).
    // Start actions and resumes get to stash/inherit; everything else must be clean.
    if needs_stash && !action.is_start() && resume_state.is_none() {
        return Err("Working tree is not clean. Commit your changes before finishing.".to_string());
    }

    // Stash if needed. On resume, inherit the prior stash ref from state.
    let stash_msg = if resume_state.is_some() {
        resume_state.as_ref().and_then(|s| s.stash_message.clone())
    } else if needs_stash {
        println!("Stashing uncommitted changes...");
        let msg = format!("gflow-finish:{branch_name}:{}", current_timestamp());
        git.stash_push_with_message(&msg)?;
        Some(msg)
    } else {
        None
    };

    // Write state file BEFORE the first side effect of a release/hotfix finish.
    if is_finish_with_state && resume_state.is_none() {
        save_new_finish(&git_dir, identity, stash_msg.clone())?;
    }

    let worktree = if worktree_active { Some(WorktreeContext { env: worktree_env, prompter }) } else { None };

    let result = run_flow(git, hosting, prompter, &branch_type, &branch_name, &action, no_checkout, worktree, resume_state.as_ref(), &main_branch, repo_cfg, script, accept_merge_type);

    // Lifecycle: clear state on success of a release/hotfix finish. Both a fresh
    // finish and a resume run on the source branch, so its identity is available.
    if let Some((kind, major, minor, patch)) = identity {
        if result.is_ok() && (is_finish_with_state || resume_state.is_some()) {
            FinishState::clear(&git_dir, kind, major, minor, patch)?;
        }
    }

    // Stash pop policy:
    //   - On success: always pop (changes restored).
    //   - On failure of a release/hotfix finish: leave stash for resume.
    //   - On failure of any other action: pop (preserves prior gflow behavior of
    //     restoring the user's working tree even on errors).
    let keep_stash_for_resume = result.is_err() && (is_finish_with_state || resume_state.is_some());
    if let Some(msg) = &stash_msg {
        if keep_stash_for_resume {
            eprintln!("Your uncommitted changes remain stashed as '{msg}'. They will be restored on a successful 'gflow finish' resume (or after 'gflow finish --abort').");
        } else {
            println!("Restoring uncommitted changes...");
            match git.find_stash_by_message(msg) {
                Ok(Some(stash_ref)) => {
                    if let Err(e) = git.stash_pop_ref(&stash_ref) {
                        eprintln!("Warning: Failed to restore stashed changes: {e}");
                        eprintln!("Your changes are saved in git stash ({stash_ref}). Run 'git stash pop {stash_ref}' to restore them.");
                    }
                }
                Ok(None) => {} // already gone
                Err(e) => eprintln!("Warning: Could not look up stash by message: {e}"),
            }
        }
    }

    result
}

/// Decide which Action to run given the parsed command, current branch, and
/// any resume state. Resume state takes precedence over branch-based dispatch
/// for `gflow finish` (and the default interactive path) — a develop-merge
/// conflict leaves HEAD on develop, where the branch-eligibility check would
/// otherwise reject the resume with "Nothing to finish on this branch."
pub fn resolve_action_with_state(
    command: Option<Commands>,
    prompter: &dyn Prompter,
    branch_type: &BranchType,
    branch_name: &str,
    resume_state: Option<&FinishState>,
    worktree_enabled: bool,
    main_branch: &str,
) -> Result<Action, String> {
    // `--abort` wins unconditionally and never errors based on branch type.
    if let Some(Commands::Finish { abort: true, .. }) = &command {
        return Ok(Action::AbortFinish);
    }

    // For `gflow finish` (or the default interactive path), an in-progress finish
    // state takes precedence over branch-based dispatch. This state is only ever
    // present when standing on the source branch (resume is branch-scoped), so it
    // covers the case where a develop-merge conflict was resolved and the user has
    // switched back to the release/hotfix branch to continue.
    // An explicit --base never applies here: resume state only exists on
    // release/hotfix source branches (see finish_identity), whose finishes have a
    // fixed target. Skip the resume shortcut so resolve_action rejects the flag
    // instead of silently ignoring it.
    let has_explicit_base = matches!(&command, Some(Commands::Finish { base: Some(_), .. }));
    let is_finish_or_default = matches!(command, Some(Commands::Finish { .. }) | None);
    if is_finish_or_default && !has_explicit_base {
        if let Some(state) = resume_state {
            eprintln!(
                "↻ Resuming in-progress {} finish for {} (started_at={}). Use 'gflow finish --abort' to discard.",
                state.kind.as_str(),
                state.source_branch(),
                state.started_at,
            );
            return Ok(match state.kind {
                FinishKind::Release => Action::FinishRelease,
                FinishKind::Hotfix => Action::FinishHotfix,
            });
        }
    }

    // No resume state (or a non-finish command): fall through to normal dispatch.
    match command {
        None => menu::show_menu(prompter, branch_type, branch_name, main_branch),
        Some(cmd) => resolve_action(cmd, branch_type, worktree_enabled, main_branch),
    }
}

/// Map a source branch to the (kind, version) identity of its finish state.
/// Only release and hotfix branches carry finish state; everything else (develop,
/// main, feature/*) yields None and therefore never resumes.
fn finish_identity(branch_type: &BranchType) -> Option<(FinishKind, u32, u32, u32)> {
    match branch_type {
        BranchType::Release { major, minor, patch } => {
            Some((FinishKind::Release, *major, *minor, *patch))
        }
        BranchType::Hotfix { major, minor, patch } => {
            Some((FinishKind::Hotfix, *major, *minor, *patch))
        }
        _ => None,
    }
}

fn save_new_finish(
    git_dir: &std::path::Path,
    identity: Option<(FinishKind, u32, u32, u32)>,
    stash_message: Option<String>,
) -> Result<(), String> {
    let Some((kind, major, minor, patch)) = identity else {
        unreachable!("FinishRelease/FinishHotfix are only ever dispatched from their own release/hotfix branch, which always yields a finish identity");
    };
    FinishState {
        kind, major, minor, patch,
        started_at: current_timestamp(),
        stash_message,
    }.save(git_dir)
}

fn handle_abort(git_dir: &std::path::Path, state: Option<FinishState>) -> Result<(), String> {
    match state {
        None => {
            println!("No in-progress finish to abort.");
            Ok(())
        }
        Some(s) => {
            println!("Aborting in-progress {} finish for {} (started_at={}).",
                s.kind.as_str(), s.source_branch(), s.started_at);
            FinishState::clear(git_dir, s.kind, s.major, s.minor, s.patch)?;
            if let Some(msg) = &s.stash_message {
                println!("Your original uncommitted changes are still stashed as '{msg}'.");
                println!("Run 'git stash list' to find it, then 'git stash pop <ref>' to restore.");
            }
            Ok(())
        }
    }
}

/// The commands the conflict hint tells the user to re-run from the finish
/// branch. `--abort` recovers, it does not resume; an explicit `--base`
/// targets a work-branch finish, which never lands via a finish branch.
fn is_finish_branch_rerun(command: &Option<Commands>) -> bool {
    matches!(
        command,
        Some(Commands::Finish { abort: false, base: None, .. }) | Some(Commands::Sync { .. })
    )
}

fn unresolved_merge_message(resume_state: Option<&FinishState>) -> String {
    let mut msg = String::from(
        "Unresolved merge in progress. Resolve conflicts, run 'git commit', then re-run 'gflow finish'."
    );
    if let Some(s) = resume_state {
        msg.push_str(&format!(
            "\n(In-progress {} finish for {} is waiting for resume.)",
            s.kind.as_str(), s.source_branch(),
        ));
    }
    msg
}

#[allow(clippy::too_many_arguments)]
fn run_flow(
    git: &dyn Git,
    hosting: &dyn HostingPlatform,
    prompter: &dyn Prompter,
    branch_type: &BranchType,
    branch_name: &str,
    action: &Action,
    skip_current_branch_sync: bool,
    worktree: Option<WorktreeContext<'_>>,
    resume_state: Option<&FinishState>,
    main_branch: &str,
    repo_cfg: &RepoConfig,
    script: Option<&dyn VersionScript>,
    accept_merge_type: bool,
) -> Result<(), String> {
    // Fast-forward the current branch to origin when the flow will operate on
    // this checkout and we're not resuming (on resume the user may be on
    // main/develop after conflict resolution — syncing that branch is harmless
    // but produces noisy output).
    if !skip_current_branch_sync && resume_state.is_none() {
        if let Err(e) = git.ff_merge(&format!("origin/{branch_name}")) {
            if !e.contains("not something we can merge") {
                return Err(e);
            }
        }
    }

    match action {
        Action::StartWorkBranch { prefix, name, from, no_checkout, .. } => {
            start::start_work_branch(git, prefix, name, from, *no_checkout, worktree)?;
        }
        Action::StartRelease { release_type, .. } => {
            start::start_release(git, prompter, hosting, script, repo_cfg, *release_type, main_branch, worktree)?;
        }
        Action::StartReleaseFix { name, no_checkout, .. } => {
            start::start_release_fix(git, hosting, repo_cfg, main_branch, name, *no_checkout, worktree)?;
        }
        Action::StartHotfixFix { name, no_checkout, .. } => {
            start::start_hotfix_fix(git, hosting, repo_cfg, name, *no_checkout, worktree, main_branch, script)?;
        }
        Action::FinishWorkBranch { breaking, base } => {
            let template = resolve_pr_template(git, branch_type)?;
            finish_work::finish_work_branch(git, hosting, prompter, branch_type, *breaking, base.clone(), template.as_deref(), accept_merge_type)?;
        }
        Action::FinishReleaseFix => {
            let template = resolve_pr_template(git, branch_type)?;
            finish_work::finish_release_fix(git, hosting, branch_type, template.as_deref(), accept_merge_type)?;
        }
        Action::FinishHotfixFix => {
            let template = resolve_pr_template(git, branch_type)?;
            finish_work::finish_hotfix_fix(git, hosting, branch_type, template.as_deref(), accept_merge_type)?;
        }
        Action::FinishReleaseChore => {
            let template = resolve_pr_template(git, branch_type)?;
            finish_work::finish_release_chore(git, hosting, branch_type, template.as_deref(), accept_merge_type)?;
        }
        Action::BumpVersion => {
            let BranchType::Release { major, minor, .. } = branch_type else {
                unreachable!("BumpVersion action only from Release branch");
            };
            finish_release::bump_version(git, hosting, script, repo_cfg, *major, *minor)?;
        }
        Action::SyncWithDevelop => {
            let BranchType::Release { major, minor, .. } = branch_type else {
                unreachable!("SyncWithDevelop action only from Release branch");
            };
            let template = resolve_landing_template(git, repo_cfg, "release")?;
            finish_release::sync_with_develop(git, hosting, repo_cfg, *major, *minor, template.as_deref(), accept_merge_type)?;
        }
        Action::FinishRelease => {
            // On resume, prefer the state's version (we may not be on the release branch).
            let (major, minor) = if let Some(s) = resume_state {
                (s.major, s.minor)
            } else {
                let BranchType::Release { major, minor, .. } = branch_type else {
                    unreachable!("FinishRelease action only from Release branch");
                };
                (*major, *minor)
            };
            let template = resolve_landing_template(git, repo_cfg, "release")?;
            finish_release::finish_release(git, hosting, repo_cfg, major, minor, main_branch, template.as_deref(), accept_merge_type)?;
        }
        Action::FinishHotfix => {
            let (major, minor, patch) = if let Some(s) = resume_state {
                (s.major, s.minor, s.patch)
            } else {
                let BranchType::Hotfix { major, minor, patch } = branch_type else {
                    unreachable!("FinishHotfix action only from Hotfix branch");
                };
                (*major, *minor, *patch)
            };
            let template = resolve_landing_template(git, repo_cfg, "hotfix")?;
            finish_hotfix::finish_hotfix(git, hosting, repo_cfg, major, minor, patch, main_branch, template.as_deref(), accept_merge_type)?;
        }
        Action::AbortFinish => {
            unreachable!("AbortFinish is handled before run_flow");
        }
    }

    Ok(())
}

/// Resolve the PR template at the composition boundary, anchored to the
/// current worktree's root — resolution keeps working from subdirectories,
/// and flows never probe the filesystem themselves (they receive the
/// resolved path as a parameter). Anchored to `worktree_root`, not
/// `repo_root`: a linked worktree can have a different branch (and therefore
/// different templates) checked out than the main tree.
fn resolve_pr_template(git: &dyn Git, branch_type: &BranchType) -> Result<Option<std::path::PathBuf>, String> {
    Ok(crate::hosting::template::resolve(&git.worktree_root()?, branch_type))
}

/// Landing PR template for a release/hotfix finish or sync, resolved only in
/// protected mode — free mode never opens a landing PR, so it must not call
/// `git.worktree_root()` (free-mode lifecycle sequences are pinned
/// byte-for-byte).
fn resolve_landing_template(git: &dyn Git, repo_cfg: &RepoConfig, key: &str) -> Result<Option<std::path::PathBuf>, String> {
    if repo_cfg.mode == Mode::Protected {
        return Ok(crate::hosting::template::resolve_keys(&git.worktree_root()?, key, key));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mocks::{MockGit, MockHosting, MockPrompter};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    #[test]
    fn a_new_finish_preserves_its_full_identity_and_stash() {
        let directory = crate::test_support::mocks::tmp_dir("gflow-new-finish-identity");
        for (kind, major, minor, patch, stash) in [
            (FinishKind::Release, 2, 5, 0, None),
            (FinishKind::Hotfix, 2, 4, 7, Some("gflow-finish:hotfix/2.4.7:1234")),
        ] {
            let before = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
            save_new_finish(&directory, Some((kind, major, minor, patch)), stash.map(str::to_string)).unwrap();
            let saved = FinishState::load(&directory, kind, major, minor, patch).unwrap().unwrap();
            let after = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

            assert_eq!((saved.kind, saved.major, saved.minor, saved.patch), (kind, major, minor, patch));
            assert_eq!(saved.stash_message.as_deref(), stash);
            assert!((before..=after).contains(&saved.started_at.parse::<u64>().unwrap()));
        }
    }

    #[test]
    fn dispatch_runs_each_requested_workflow_and_preserves_failures() {
        use crate::flows::start::ReleaseType;

        let directory = crate::test_support::mocks::tmp_dir("gflow-dispatch-contract");
        for (branch, action, expected) in [
            ("develop", Action::StartWorkBranch {
                prefix: "feature".into(), name: "login".into(), from: "develop".into(),
                no_checkout: false, no_worktree: false,
            }, "create_branch:feature/login:origin/develop"),
            ("develop", Action::StartWorkBranch {
                prefix: "docs".into(), name: "guide".into(), from: "main".into(),
                no_checkout: true, no_worktree: false,
            }, "create_branch_no_checkout:docs/guide:main"),
            ("develop", Action::StartRelease {
                release_type: Some(ReleaseType::Minor), no_worktree: false,
            }, "create_branch:release/2.5.0:origin/develop"),
            ("release/2.5.0", Action::StartReleaseFix {
                name: "login".into(), no_checkout: false, no_worktree: false,
            }, "create_branch:release-fix/2.5.0/login:release/2.5.0"),
            ("hotfix/2.4.1", Action::StartHotfixFix {
                name: "login".into(), no_checkout: false, no_worktree: false,
            }, "create_branch:hotfix-fix/2.4.1/login:hotfix/2.4.1"),
            ("feature/login", Action::FinishWorkBranch {
                breaking: Some(true), base: Some("develop".into()),
            }, "create_or_get_pr:feature/login:develop:feat!: login"),
            ("release-fix/2.5.0/login", Action::FinishReleaseFix,
                "create_or_get_pr:release-fix/2.5.0/login:release/2.5.0:fix: login"),
            ("hotfix-fix/2.4.1/login", Action::FinishHotfixFix,
                "create_or_get_pr:hotfix-fix/2.4.1/login:hotfix/2.4.1:fix: login"),
            ("release-chore/2.5.0/version", Action::FinishReleaseChore,
                "create_or_get_pr:release-chore/2.5.0/version:release/2.5.0:chore: version"),
            ("release/2.5.0", Action::BumpVersion,
                "create_tag:v2.5.0-rc.2:chore: bump version to v2.5.0-rc.2"),
            ("release/2.5.0", Action::SyncWithDevelop,
                "merge:release/2.5.0:chore: sync release 2.5.0 with develop"),
            ("release/2.5.0", Action::FinishRelease,
                "create_tag:v2.5.0:chore: release 2.5.0"),
            ("hotfix/2.4.1", Action::FinishHotfix,
                "create_tag:v2.4.1:chore: hotfix 2.4.1"),
        ] {
            let mut git = MockGit::new();
            git.current_branch = branch.into();
            git.worktree_root = directory.to_path_buf();
            git.tags = vec!["v2.4.0".into()];
            git.tags_on_branch = vec!["v2.5.0-rc.1".into()];
            git.existing_remote_branches.insert("develop".into());
            let hosting = MockHosting::new();

            run_flow(&git, &hosting, &MockPrompter::aborting(), &BranchType::parse(branch), branch,
                &action, false, None, None, "main", &RepoConfig::default(), None, false).unwrap();

            let mut calls = git.calls();
            assert_eq!(calls.first(), Some(&format!("ff_merge:origin/{branch}")));
            calls.extend(hosting.calls());
            assert!(calls.iter().any(|call| call == expected), "{action:?}: {calls:?}");

            let failure = match &action {
                Action::StartWorkBranch { .. } => expected,
                Action::StartRelease { .. } => "list_branches_matching:release/*",
                Action::BumpVersion | Action::FinishRelease => "tags_on_branch:release/2.5.0",
                Action::FinishHotfix => "is_ancestor:hotfix/2.4.1:main",
                _ => "current_branch",
            };
            git.calls.borrow_mut().clear();
            git.fail_call = Some((failure.into(), 1));
            let hosting = MockHosting::new();

            let error = run_flow(&git, &hosting, &MockPrompter::aborting(), &BranchType::parse(branch), branch,
                &action, true, None, None, "main", &RepoConfig::default(), None, false).unwrap_err();

            assert_eq!(error, format!("injected git failure: {failure}"));
            assert_eq!(git.calls().last().unwrap(), failure);
            assert!(hosting.calls().is_empty());

            let mode = match action {
                Action::FinishWorkBranch { .. } | Action::FinishReleaseFix | Action::FinishHotfixFix
                    | Action::FinishReleaseChore => Mode::Free,
                Action::SyncWithDevelop | Action::FinishRelease | Action::FinishHotfix => Mode::Protected,
                _ => continue,
            };
            git.calls.borrow_mut().clear();
            git.fail_call = Some(("worktree_root".into(), 1));
            let error = run_flow(&git, &hosting, &MockPrompter::aborting(), &BranchType::parse(branch), branch,
                &action, true, None, None, "main", &RepoConfig { mode, ..RepoConfig::default() }, None, false).unwrap_err();

            assert_eq!(error, "injected git failure: worktree_root");
            assert_eq!(git.calls(), ["worktree_root"]);
            assert!(hosting.calls().is_empty());
        }
    }

    #[test]
    fn resumed_dispatch_uses_the_saved_version_without_syncing_the_current_branch() {
        for (action, kind, major, minor, patch, expected) in [
            (Action::FinishRelease, FinishKind::Release, 2, 5, 0,
                "create_tag:v2.5.0:chore: release 2.5.0"),
            (Action::FinishHotfix, FinishKind::Hotfix, 2, 4, 7,
                "create_tag:v2.4.7:chore: hotfix 2.4.7"),
        ] {
            let mut git = MockGit::new();
            git.tags_on_branch = vec!["v2.5.0-rc.1".into()];
            let state = FinishState { kind, major, minor, patch, started_at: "1234".into(), stash_message: None };

            run_flow(&git, &MockHosting::new(), &MockPrompter::aborting(), &BranchType::Develop,
                "develop", &action, false, None, Some(&state), "main", &RepoConfig::default(), None, false).unwrap();

            let calls = git.calls();
            assert!(!calls.first().unwrap().starts_with("ff_merge:"), "{calls:?}");
            assert!(calls.iter().any(|call| call == expected), "{calls:?}");
        }
    }

    #[test]
    fn dispatch_ignores_only_a_missing_upstream_and_honors_the_sync_skip() {
        for (error, skip_sync, succeeds) in [
            (None, false, true),
            (Some("origin/develop is not something we can merge"), false, true),
            (Some("local and remote branches diverged"), false, false),
            (Some("local and remote branches diverged"), true, true),
        ] {
            let mut git = MockGit::new();
            git.ff_merge_error = error.map(str::to_string);
            let hosting = MockHosting::new();
            let action = Action::StartWorkBranch {
                prefix: "feature".into(), name: "login".into(), from: "develop".into(),
                no_checkout: true, no_worktree: false,
            };

            let result = run_flow(&git, &hosting, &MockPrompter::aborting(), &BranchType::Develop,
                "develop", &action, skip_sync, None, None, "main", &RepoConfig::default(), None, false);

            let calls = git.calls();
            if succeeds {
                result.unwrap();
                assert!(calls.contains(&"create_branch_no_checkout:feature/login:develop".into()));
                assert!(calls.contains(&"push:feature/login".into()));
                assert_eq!(calls.iter().any(|call| call.starts_with("ff_merge:")), !skip_sync);
            } else {
                assert_eq!(result.unwrap_err(), "local and remote branches diverged");
                assert_eq!(calls, vec!["ff_merge:origin/develop"]);
            }
            assert!(hosting.calls().is_empty());
        }
    }

    #[test]
    fn missing_finish_identity_cannot_write_state() {
        let directory = crate::test_support::mocks::tmp_dir("gflow-missing-finish-identity");
        let panic = catch_unwind(|| save_new_finish(&directory, None, None))
            .expect_err("a state file requires a source branch identity");
        let message = panic.downcast_ref::<&str>().expect("invariant panic message");
        assert!(message.contains("always yields a finish identity"));
        assert_eq!(std::fs::read_dir(&*directory).unwrap().count(), 0);
    }

    #[test]
    fn invalid_dispatch_stops_before_any_git_or_hosting_operation() {
        let git = MockGit::new();
        let hosting = MockHosting::new();
        let prompter = MockPrompter::new();
        for (action, expected) in [
            (Action::BumpVersion, "BumpVersion action only from Release branch"),
            (Action::SyncWithDevelop, "SyncWithDevelop action only from Release branch"),
            (Action::FinishRelease, "FinishRelease action only from Release branch"),
            (Action::FinishHotfix, "FinishHotfix action only from Hotfix branch"),
            (Action::AbortFinish, "AbortFinish is handled before run_flow"),
        ] {
            let panic = catch_unwind(AssertUnwindSafe(|| run_flow(
                &git, &hosting, &prompter, &BranchType::Other, "unrecognized",
                &action, true, None, None, "main", &RepoConfig::default(), None, false,
            ))).expect_err("an invalid dispatch must fail before side effects");
            let message = panic.downcast_ref::<&str>().expect("invariant panic message");
            assert!(message.contains(expected), "{message}");
        }
        assert!(git.calls().is_empty());
        assert!(hosting.calls().is_empty());
        assert!(prompter.calls().is_empty());
    }
}

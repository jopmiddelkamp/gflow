mod common;

use std::cell::Cell;
use std::fs;

use common::{MockEditor, MockGit, MockHosting, MockPrompter, MockWorktreeSetup};
use gflow::cli::{Commands, StartKind, StartOptions};
use gflow::lifecycle;
use gflow::mainline::{resolve_main_branch, MAIN_BRANCH_KEY};
use gflow::prompt::Prompter;
use gflow::repo_config::{self, Mode, RepoConfig};
use gflow::state::{FinishKind, FinishState, LEGACY_STATE_FILE_NAME};
use gflow::worktree::{WorktreeConfig, WorktreeEnv};

fn finish() -> Commands {
    Commands::Finish {
        breaking: Some(false),
        base: None,
        abort: false,
        accept_merge_type: false,
    }
}

fn start_feature() -> Commands {
    Commands::Start {
        kind: StartKind::Feature {
            name: "login".into(),
            base: "develop".into(),
            opts: StartOptions::default(),
        },
    }
}

fn git_on(branch: &str) -> MockGit {
    let mut git = MockGit::with_tmp_git_dir("gflow-lifecycle-failures");
    git.current_branch = branch.into();
    git.config.insert(MAIN_BRANCH_KEY.into(), "main".into());
    git.tags_on_branch = vec!["v2.5.0-rc.1".into()];
    git
}

fn run(
    git: &MockGit,
    hosting: &MockHosting,
    command: Commands,
    cfg: &RepoConfig,
) -> Result<(), String> {
    lifecycle::run(
        git,
        hosting,
        &MockPrompter::aborting(),
        &WorktreeEnv {
            config: &WorktreeConfig {
                enabled: false,
                editor: "none".into(),
                base_path: None,
            },
            editor: &MockEditor::new(),
            setup: &MockWorktreeSetup::new(),
            commands: None,
        },
        cfg,
        None,
        Some(command),
    )
}

fn release_state() -> FinishState {
    FinishState {
        kind: FinishKind::Release,
        major: 2,
        minor: 5,
        patch: 0,
        started_at: "1234".into(),
        stash_message: None,
    }
}

#[test]
fn failed_preflight_stops_before_state_or_flow_mutations() {
    for (branch, failure) in [
        ("develop", "current_branch"),
        ("develop", "git_dir"),
        ("develop", "get_config:gflow.branch.main"),
        ("develop", "is_mid_merge"),
        ("develop", "has_unmerged_paths"),
        ("develop", "fetch"),
        ("develop", "is_working_tree_clean"),
        ("finish/release-2.5.0-into-main", "is_mid_merge"),
        ("finish/release-2.5.0-into-main", "has_unmerged_paths"),
        ("finish/release-2.5.0-into-main", "checkout:release/2.5.0"),
    ] {
        let mut git = git_on(branch);
        git.fail_call = Some((failure.into(), 1));
        let hosting = MockHosting::new();
        let command = if branch == "develop" {
            start_feature()
        } else {
            finish()
        };

        assert_eq!(
            run(&git, &hosting, command, &RepoConfig::default()).unwrap_err(),
            format!("injected git failure: {failure}")
        );
        assert_eq!(git.calls().last().unwrap(), failure);
        assert!(!FinishState::dir(&git.git_dir).exists());
        assert!(hosting.calls().is_empty());
    }
}

#[test]
fn unreadable_legacy_state_stops_before_mainline_detection() {
    let git = git_on("release/2.5.0");
    fs::create_dir(git.git_dir.join(LEGACY_STATE_FILE_NAME)).unwrap();
    let hosting = MockHosting::new();

    assert!(run(&git, &hosting, finish(), &RepoConfig::default())
        .unwrap_err()
        .contains("Failed to read"));
    assert_eq!(git.calls(), ["current_branch", "git_dir"]);
    assert!(hosting.calls().is_empty());
}

#[test]
fn unreadable_resume_state_stops_before_fetch_or_publication() {
    let git = git_on("release/2.5.0");
    let path = FinishState::path(&git.git_dir, FinishKind::Release, 2, 5, 0);
    fs::create_dir_all(&path).unwrap();
    let hosting = MockHosting::new();

    assert!(run(&git, &hosting, finish(), &RepoConfig::default())
        .unwrap_err()
        .contains("Failed to read"));
    assert_eq!(git.calls().last().unwrap(), "get_config:gflow.branch.main");
    assert!(path.is_dir());
    assert!(hosting.calls().is_empty());
}

#[test]
fn unsavable_finish_state_stops_before_the_first_merge() {
    let git = git_on("release/2.5.0");
    fs::write(FinishState::dir(&git.git_dir), "do not replace").unwrap();
    let hosting = MockHosting::new();

    assert!(run(&git, &hosting, finish(), &RepoConfig::default())
        .unwrap_err()
        .contains("Failed to create"));
    assert_eq!(git.calls().last().unwrap(), "is_working_tree_clean");
    assert_eq!(
        fs::read_to_string(FinishState::dir(&git.git_dir)).unwrap(),
        "do not replace"
    );
    assert!(hosting.calls().is_empty());
}

#[test]
fn failed_stash_save_keeps_dirty_changes_before_starting_a_branch() {
    let mut git = git_on("develop");
    git.working_tree_clean = false;
    git.fail_stash_push = true;
    let hosting = MockHosting::new();

    assert_eq!(
        run(&git, &hosting, start_feature(), &RepoConfig::default()).unwrap_err(),
        "could not save the stash"
    );
    assert!(git
        .calls()
        .last()
        .unwrap()
        .starts_with("stash_push_with_message:gflow-finish:develop:"));
    assert!(git.stashes.borrow().is_empty());
    assert!(!git
        .calls()
        .iter()
        .any(|call| call.starts_with("create_branch:") || call.starts_with("push:")));
    assert!(hosting.calls().is_empty());
}

#[test]
fn failed_start_flows_never_publish_an_uncreated_branch() {
    for (branch, kind, failure) in [
        (
            "develop",
            StartKind::Feature {
                name: "login".into(),
                base: "develop".into(),
                opts: StartOptions::default(),
            },
            "create_branch:feature/login:develop",
        ),
        (
            "develop",
            StartKind::Release {
                major: false,
                minor: true,
                no_worktree: false,
            },
            "list_branches_matching:release/*",
        ),
        (
            "release/2.5.0",
            StartKind::ReleaseFix {
                name: "login".into(),
                opts: StartOptions::default(),
            },
            "create_branch:release-fix/2.5.0/login:release/2.5.0",
        ),
        (
            "hotfix/2.4.1",
            StartKind::HotfixFix {
                name: "login".into(),
                opts: StartOptions::default(),
            },
            "create_branch:hotfix-fix/2.4.1/login:hotfix/2.4.1",
        ),
    ] {
        let mut git = git_on(branch);
        git.fail_call = Some((failure.into(), 1));
        let hosting = MockHosting::new();

        assert_eq!(
            run(
                &git,
                &hosting,
                Commands::Start { kind },
                &RepoConfig::default()
            )
            .unwrap_err(),
            format!("injected git failure: {failure}")
        );
        assert_eq!(git.calls().last().unwrap(), failure);
        assert!(hosting.calls().is_empty());
        assert!(!FinishState::dir(&git.git_dir).exists());
    }
}

#[test]
fn missing_template_root_stops_each_finish_before_publication() {
    for (branch, command, mode) in [
        ("feature/login", finish(), Mode::Free),
        ("release-fix/2.5.0/login", finish(), Mode::Free),
        ("hotfix-fix/2.4.1/login", finish(), Mode::Free),
        ("release-chore/2.5.0/version", finish(), Mode::Free),
        (
            "release/2.5.0",
            Commands::Sync {
                accept_merge_type: false,
            },
            Mode::Protected,
        ),
        ("release/2.5.0", finish(), Mode::Protected),
        ("hotfix/2.4.1", finish(), Mode::Protected),
    ] {
        let mut git = git_on(branch);
        git.fail_call = Some(("worktree_root".into(), 1));
        let hosting = MockHosting::new();

        assert_eq!(
            run(
                &git,
                &hosting,
                command,
                &RepoConfig {
                    mode,
                    ..RepoConfig::default()
                }
            )
            .unwrap_err(),
            "injected git failure: worktree_root"
        );
        assert_eq!(git.calls().last().unwrap(), "worktree_root");
        assert!(hosting.calls().is_empty());
    }
}

#[test]
fn failed_fix_finish_keeps_branches_and_reports_the_hosting_error() {
    for branch in [
        "release-fix/2.5.0/login",
        "hotfix-fix/2.4.1/login",
        "release-chore/2.5.0/version",
    ] {
        let git = git_on(branch);
        let mut hosting = MockHosting::new();
        let failure = format!("merged_pr:{branch}");
        hosting.fail_call = Some((failure.clone(), 1));

        assert_eq!(
            run(&git, &hosting, finish(), &RepoConfig::default()).unwrap_err(),
            format!("injected hosting failure: {failure}")
        );
        assert!(!git
            .calls()
            .iter()
            .any(|call| call.starts_with("delete_branch") || call.starts_with("push:")));
        assert_eq!(hosting.calls(), [failure]);
    }
}

#[test]
fn failed_bump_tag_lookup_leaves_the_release_untouched() {
    let mut git = git_on("release/2.5.0");
    git.fail_call = Some(("tags_on_branch:release/2.5.0".into(), 1));
    let hosting = MockHosting::new();

    assert_eq!(
        run(&git, &hosting, Commands::Bump, &RepoConfig::default()).unwrap_err(),
        "injected git failure: tags_on_branch:release/2.5.0"
    );
    assert_eq!(git.calls().last().unwrap(), "tags_on_branch:release/2.5.0");
    assert!(!FinishState::dir(&git.git_dir).exists());
    assert!(hosting.calls().is_empty());
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

struct CancelAt {
    question: usize,
    asked: Cell<usize>,
}

impl Prompter for CancelAt {
    fn select(&self, _: &str, _: &[&str]) -> Result<usize, String> {
        let asked = self.asked.get() + 1;
        self.asked.set(asked);
        if asked == self.question {
            Err("Aborted".into())
        } else {
            Ok(0)
        }
    }

    fn prompt_name(&self, _: &str) -> Result<String, String> {
        Err("unexpected name prompt".into())
    }
    fn prompt_line(&self, _: &str) -> Result<String, String> {
        Err("unexpected line prompt".into())
    }
}

#[test]
fn canceling_any_initialization_question_never_writes_partial_policy() {
    for question in 1..=3 {
        let root = common::tmp_dir("gflow-init-cancel");
        let prompter = CancelAt {
            question,
            asked: Cell::new(0),
        };

        assert_eq!(
            gflow::init::ensure(&prompter, None, &root, true).unwrap_err(),
            "Aborted"
        );
        assert_eq!(prompter.asked.get(), question);
        assert!(!root.join(".gflow").exists());
    }
}

#[test]
fn invalid_existing_policy_stops_initialization_before_questions() {
    let root = common::tmp_dir("gflow-init-invalid");
    fs::create_dir(root.join(".gflow")).unwrap();
    fs::write(root.join(".gflow/config"), "mode=invalid\n").unwrap();
    let prompter = MockPrompter::new();

    assert!(gflow::init::ensure(&prompter, None, &root, true)
        .unwrap_err()
        .contains("mode"));
    assert!(prompter.calls().is_empty());
    assert_eq!(
        fs::read_to_string(root.join(".gflow/config")).unwrap(),
        "mode=invalid\n"
    );
}

#[test]
fn initialization_write_failure_preserves_the_blocking_file() {
    let root = common::tmp_dir("gflow-init-write-failure");
    fs::write(root.join(".gflow"), "do not replace").unwrap();

    let error = gflow::init::wizard(&MockPrompter::scripted(&[0, 0, 0]), &root).unwrap_err();

    assert!(error.contains("Failed to create"), "{error}");
    assert_eq!(
        fs::read_to_string(root.join(".gflow")).unwrap(),
        "do not replace"
    );
}

#[cfg(unix)]
#[test]
fn failed_state_removal_preserves_resume_data_on_success_or_abort() {
    use std::os::unix::fs::PermissionsExt;

    for abort in [false, true] {
        let git = git_on("release/2.5.0");
        let state = release_state();
        state.save(&git.git_dir).unwrap();
        let directory = FinishState::dir(&git.git_dir);
        let permissions = fs::metadata(&directory).unwrap().permissions();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
        if fs::write(directory.join("write-probe"), "").is_ok() {
            fs::set_permissions(&directory, permissions).unwrap();
            eprintln!("Skipping deletion denial: this account or filesystem bypasses directory permissions.");
            return;
        }
        let hosting = MockHosting::new();
        let result = run(
            &git,
            &hosting,
            Commands::Finish {
                breaking: None,
                base: None,
                abort,
                accept_merge_type: false,
            },
            &RepoConfig::default(),
        );
        fs::set_permissions(&directory, permissions).unwrap();

        assert!(result.unwrap_err().contains("Failed to remove"));
        assert_eq!(
            FinishState::load(&git.git_dir, FinishKind::Release, 2, 5, 0).unwrap(),
            Some(state)
        );
        if abort {
            assert!(!git.calls().contains(&"fetch".into()));
            assert!(hosting.calls().is_empty());
        }
    }
}

#[test]
fn abort_without_a_saved_stash_only_clears_its_own_resume_state() {
    let git = git_on("release/2.5.0");
    release_state().save(&git.git_dir).unwrap();
    let other = FinishState {
        kind: FinishKind::Hotfix,
        major: 2,
        minor: 4,
        patch: 1,
        ..release_state()
    };
    other.save(&git.git_dir).unwrap();

    run(
        &git,
        &MockHosting::new(),
        Commands::Finish {
            breaking: None,
            base: None,
            abort: true,
            accept_merge_type: false,
        },
        &repo_config::RepoConfig::default(),
    )
    .unwrap();

    assert!(
        FinishState::load(&git.git_dir, FinishKind::Release, 2, 5, 0)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        FinishState::load(&git.git_dir, FinishKind::Hotfix, 2, 4, 1).unwrap(),
        Some(other)
    );
    assert!(!git
        .calls()
        .iter()
        .any(|call| call.starts_with("stash_") || call == "fetch"));
}

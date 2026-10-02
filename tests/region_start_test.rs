mod common;

use common::{
    MockEditor, MockGit, MockHosting, MockPrompter, MockVersionScript, MockWorktreeSetup,
};
use gflow::flows::start::{
    start_hotfix_fix, start_release, start_release_fix, start_work_branch, ReleaseType,
};
use gflow::repo_config::{BumpStrategy, Mode, RepoConfig};
use gflow::worktree::{WorktreeConfig, WorktreeContext, WorktreeEnv};

fn assert_stopped(git: &MockGit, result: Result<(), String>, call: &str) {
    assert_eq!(result, Err(format!("injected git failure: {call}")));
    assert_eq!(git.calls().last().map(String::as_str), Some(call));
}

fn release_git() -> MockGit {
    let mut git = MockGit::new();
    git.tags = vec!["v1.0.0".into()];
    git.working_tree_clean_seq
        .borrow_mut()
        .extend([true, false, true, false]);
    git
}

#[test]
fn creating_a_work_branch_stops_before_later_steps_when_git_fails() {
    for (no_checkout, call) in [
        (false, "create_branch:feature/example:develop"),
        (true, "create_branch_no_checkout:feature/example:develop"),
        (false, "push:feature/example"),
    ] {
        let mut git = MockGit::new();
        git.fail_call = Some((call.into(), 1));
        let result = start_work_branch(&git, "feature", "example", "develop", no_checkout, None);
        assert_stopped(&git, result, call);
    }
}

#[test]
fn creating_a_release_stops_at_every_failed_prepublication_step() {
    for (call, occurrence) in [
        ("list_branches_matching:release/*", 1),
        ("list_tags", 1),
        ("is_working_tree_clean", 1),
        ("checkout:develop", 1),
        ("create_branch:release/1.1.0:develop", 1),
        ("is_working_tree_clean", 2),
        ("stage_all", 1),
        ("commit:chore: set version 1.1.0", 1),
        ("push:release/1.1.0", 1),
        (
            "create_tag:v1.1.0-rc.1:chore: create release branch 1.1.0",
            1,
        ),
        ("push_tag:v1.1.0-rc.1", 1),
    ] {
        let mut git = release_git();
        git.fail_call = Some((call.into(), occurrence));
        let hosting = MockHosting::new();
        let result = start_release(
            &git,
            &MockPrompter::new(),
            &hosting,
            Some(&MockVersionScript::new()),
            &RepoConfig::default(),
            Some(ReleaseType::Minor),
            "main",
            None,
        );
        assert_stopped(&git, result, call);
        assert!(hosting.calls().is_empty());
    }
}

#[test]
fn aborting_release_type_selection_does_not_create_a_branch() {
    let git = MockGit::new();
    let result = start_release(
        &git,
        &MockPrompter::aborting(),
        &MockHosting::new(),
        None,
        &RepoConfig::default(),
        None,
        "main",
        None,
    );
    assert_eq!(result, Err("Aborted".into()));
    assert!(!git
        .calls()
        .iter()
        .any(|call| call.starts_with("checkout:") || call.starts_with("create_branch:")));
}

#[test]
fn release_reuse_and_fix_discovery_stop_when_repository_reads_fail() {
    for call in [
        "current_branch",
        "list_branches_matching:release/*",
        "tag_exists:v1.1.0",
        "local_branch_exists:release/1.1.0",
    ] {
        let mut git = MockGit::new();
        git.branches_matching = vec!["release/1.1.0".into()];
        git.fail_call = Some((call.into(), 1));
        let result = start_release_fix(
            &git,
            &MockHosting::new(),
            &RepoConfig::default(),
            "main",
            "example",
            true,
            None,
        );
        assert_stopped(&git, result, call);
    }

    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".into()];
    git.fail_call = Some(("checkout:release/1.1.0".into(), 1));
    let result = start_release(
        &git,
        &MockPrompter::new(),
        &MockHosting::new(),
        None,
        &RepoConfig::default(),
        None,
        "main",
        None,
    );
    assert_stopped(&git, result, "checkout:release/1.1.0");
}

#[test]
fn hotfix_discovery_and_creation_stop_before_creating_the_fix_on_failure() {
    for (no_checkout, call) in [
        (false, "current_branch"),
        (false, "list_branches_matching:hotfix/*"),
        (false, "list_tags"),
        (false, "checkout:main"),
        (false, "create_branch:hotfix/1.0.1:main"),
        (true, "create_branch_no_checkout:hotfix/1.0.1:main"),
        (false, "is_working_tree_clean"),
        (false, "push:hotfix/1.0.1"),
    ] {
        let mut git = release_git();
        git.fail_call = Some((call.into(), 1));
        let result = start_hotfix_fix(
            &git,
            &MockHosting::new(),
            &RepoConfig::default(),
            "example",
            no_checkout,
            None,
            "main",
            Some(&MockVersionScript::new()),
        );
        assert_stopped(&git, result, call);
        assert!(!git.calls().iter().any(|call| call.contains("hotfix-fix/")));
    }

    for (no_checkout, call) in [
        (false, "checkout:hotfix/1.0.1"),
        (true, "local_branch_exists:hotfix/1.0.1"),
    ] {
        let mut git = MockGit::new();
        git.branches_matching = vec!["hotfix/1.0.1".into()];
        git.fail_call = Some((call.into(), 1));
        let result = start_hotfix_fix(
            &git,
            &MockHosting::new(),
            &RepoConfig::default(),
            "example",
            no_checkout,
            None,
            "main",
            None,
        );
        assert_stopped(&git, result, call);
    }
}

#[test]
fn hotfix_creation_stops_when_the_version_script_fails() {
    let git = release_git();
    let mut script = MockVersionScript::new();
    script.fail = Some("cannot update version".into());
    let result = start_hotfix_fix(
        &git,
        &MockHosting::new(),
        &RepoConfig::default(),
        "example",
        false,
        None,
        "main",
        Some(&script),
    );
    assert_eq!(result, Err("cannot update version".into()));
    assert!(!git
        .calls()
        .iter()
        .any(|call| call.starts_with("push:") || call.contains("hotfix-fix/")));
}

#[test]
fn hotfix_with_no_version_is_rejected_before_a_fix_branch_is_created() {
    let mut git = MockGit::new();
    git.current_branch = "hotfix/not-a-version".into();
    let result = start_hotfix_fix(
        &git,
        &MockHosting::new(),
        &RepoConfig::default(),
        "example",
        false,
        None,
        "main",
        None,
    );
    assert!(result.unwrap_err().contains("does not carry a version"));
    assert_eq!(git.calls(), ["current_branch"]);
}

#[test]
fn patch_hotfix_discovery_stops_when_release_history_cannot_be_read() {
    let cfg = RepoConfig {
        bump_strategy: BumpStrategy::Patch,
        ..RepoConfig::default()
    };
    for call in ["list_branches_matching:release/*", "list_tags"] {
        let mut git = MockGit::new();
        git.fail_call = Some((call.into(), 1));
        let result = start_hotfix_fix(
            &git,
            &MockHosting::new(),
            &cfg,
            "example",
            false,
            None,
            "main",
            None,
        );
        assert_stopped(&git, result, call);
    }
}

#[test]
fn patch_release_discovery_does_not_reuse_a_branch_when_shipping_status_is_unknown() {
    let cfg = RepoConfig {
        bump_strategy: BumpStrategy::Patch,
        mode: Mode::Protected,
        ..RepoConfig::default()
    };
    for call in [
        "remote_branch_exists:release/1.1.0",
        "is_ancestor:release/1.1.0:origin/main",
    ] {
        let mut git = MockGit::new();
        git.branches_matching = vec!["release/1.1.0".into()];
        git.fail_call = Some((call.into(), 1));
        let result = start_release(
            &git,
            &MockPrompter::new(),
            &MockHosting::new(),
            None,
            &cfg,
            None,
            "main",
            None,
        );
        assert_stopped(&git, result, call);
    }
    let mut git = MockGit::new();
    git.branches_matching = vec!["release/1.1.0".into()];
    let mut hosting = MockHosting::new();
    hosting.fail_call = Some(("merged_pr_to:release/1.1.0:main".into(), 1));
    let result = start_release(
        &git,
        &MockPrompter::new(),
        &hosting,
        None,
        &cfg,
        None,
        "main",
        None,
    );
    assert_eq!(
        result,
        Err("injected hosting failure: merged_pr_to:release/1.1.0:main".into())
    );
    assert!(!git.calls().iter().any(|call| call.starts_with("checkout:")));
}

#[test]
fn published_release_survives_develop_version_bump_failures() {
    for (call, occurrence) in [
        ("checkout:develop", 2),
        ("is_working_tree_clean", 3),
        ("is_working_tree_clean", 4),
        ("stage_all", 2),
        ("commit:chore: set version 1.2.0", 1),
        ("push:develop", 1),
    ] {
        let mut git = release_git();
        git.fail_call = Some((call.into(), occurrence));
        start_release(
            &git,
            &MockPrompter::new(),
            &MockHosting::new(),
            Some(&MockVersionScript::new()),
            &RepoConfig::default(),
            Some(ReleaseType::Minor),
            "main",
            None,
        )
        .unwrap();
        let calls = git.calls();
        assert!(calls.contains(&"push_tag:v1.1.0-rc.1".into()));
        let failed = calls
            .iter()
            .enumerate()
            .filter(|(_, value)| *value == call)
            .nth(occurrence - 1)
            .unwrap()
            .0;
        assert_eq!(
            &calls[failed + 1..],
            ["checkout:release/1.1.0"],
            "failure: {call}"
        );
    }
}

#[test]
fn protected_develop_bump_failures_preserve_the_release_and_restore_its_checkout() {
    let cfg = RepoConfig {
        mode: Mode::Protected,
        ..RepoConfig::default()
    };
    for (call, occurrence, changed) in [
        ("remote_branch_exists:chore/set-version-1.2.0", 1, true),
        ("local_branch_exists:chore/set-version-1.2.0", 1, true),
        ("delete_branch_local:chore/set-version-1.2.0", 1, true),
        ("create_branch:chore/set-version-1.2.0:develop", 1, true),
        ("is_working_tree_clean", 3, true),
        ("push:chore/set-version-1.2.0", 1, true),
        ("checkout:develop", 3, false),
        ("delete_branch_local:chore/set-version-1.2.0", 2, false),
    ] {
        let mut git = release_git();
        git.existing_local_branches
            .insert("chore/set-version-1.2.0".into());
        *git.working_tree_clean_seq.borrow_mut() = [true, false, true, !changed].into();
        git.fail_call = Some((call.into(), occurrence));
        let hosting = MockHosting::new();
        start_release(
            &git,
            &MockPrompter::new(),
            &hosting,
            Some(&MockVersionScript::new()),
            &cfg,
            Some(ReleaseType::Minor),
            "main",
            None,
        )
        .unwrap();
        let calls = git.calls();
        let failed = calls
            .iter()
            .enumerate()
            .filter(|(_, value)| *value == call)
            .nth(occurrence - 1)
            .unwrap()
            .0;
        assert_eq!(
            &calls[failed + 1..],
            ["checkout:release/1.1.0"],
            "failure: {call}"
        );
        assert!(!calls.contains(&"push:develop".into()));
        assert!(hosting.calls().is_empty());
    }
}

#[test]
fn protected_develop_pr_failure_preserves_the_release_for_new_and_reused_version_branches() {
    let cfg = RepoConfig {
        mode: Mode::Protected,
        ..RepoConfig::default()
    };
    for remote_exists in [false, true] {
        let mut git = release_git();
        if remote_exists {
            git.existing_remote_branches
                .insert("chore/set-version-1.2.0".into());
        }
        let mut hosting = MockHosting::new();
        hosting.fail_call = Some((
            "create_or_get_pr:chore/set-version-1.2.0:develop:chore: set version 1.2.0".into(),
            1,
        ));
        start_release(
            &git,
            &MockPrompter::new(),
            &hosting,
            Some(&MockVersionScript::new()),
            &cfg,
            Some(ReleaseType::Minor),
            "main",
            None,
        )
        .unwrap();
        assert_eq!(
            git.calls().last().map(String::as_str),
            Some("checkout:release/1.1.0")
        );
        assert_eq!(hosting.calls().len(), 1);
        assert!(git.calls().contains(&"push_tag:v1.1.0-rc.1".into()));
        assert!(!git.calls().contains(&"push:develop".into()));
    }
}

#[test]
fn start_worktree_errors_stop_before_opening_the_editor_or_creating_a_fix() {
    let config = WorktreeConfig {
        enabled: true,
        editor: "none".into(),
        base_path: Some(std::env::temp_dir().display().to_string()),
    };
    for (kind, call) in [
        ("work", "repo_root"),
        ("release", "worktree_of:release/1.1.0"),
        ("hotfix", "worktree_of:hotfix/1.0.1"),
        ("handoff", "checkout:develop"),
    ] {
        let mut git = release_git();
        if kind == "release" {
            git.branches_matching = vec!["release/1.1.0".into()];
        }
        if kind == "hotfix" {
            git.current_branch = "hotfix/1.0.1".into();
        }
        git.fail_call = Some((call.into(), if kind == "handoff" { 2 } else { 1 }));
        let editor = MockEditor::new();
        let setup = MockWorktreeSetup::new();
        let prompter = MockPrompter::new();
        let env = WorktreeEnv {
            config: &config,
            editor: &editor,
            setup: &setup,
            commands: None,
        };
        let ctx = Some(WorktreeContext {
            env: &env,
            prompter: &prompter,
        });
        let result = match kind {
            "work" => start_work_branch(&git, "feature", "example", "develop", false, ctx),
            "hotfix" => start_hotfix_fix(
                &git,
                &MockHosting::new(),
                &RepoConfig::default(),
                "example",
                false,
                ctx,
                "main",
                None,
            ),
            _ => start_release(
                &git,
                &prompter,
                &MockHosting::new(),
                None,
                &RepoConfig::default(),
                Some(ReleaseType::Minor),
                "main",
                ctx,
            ),
        };
        assert_stopped(&git, result, call);
        assert!(editor.calls().is_empty());
        assert!(!git.calls().iter().any(|call| call.contains("hotfix-fix/")));
    }
}

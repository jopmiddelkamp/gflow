mod common;

use common::MockPrompter;
use gflow::action::Action;
use gflow::git::branch::BranchType;
use gflow::menu::show_menu;

// `show_menu` is the interactive half of "one Action enum is the single
// currency" (decisions.md, CLI/UX Conventions) — the branch-type gating table
// that decides which actions each branch offers and what Action a selection
// produces. Its CLI twin, `cli::resolve_action`, has always been tested; this
// side only became testable once it took the Prompter port instead of calling
// the terminal module directly.

#[test]
fn main_offers_only_a_hotfix_fix() {
    let prompter = MockPrompter::scripted(&[0]).with_lines(&["npe"]);

    let action = show_menu(&prompter, &BranchType::Main, "main", "main").unwrap();

    assert_eq!(prompter.calls(), vec![
        "select:What would you like to do?:[start hotfix fix]",
        "prompt_name:Name for hotfix-fix branch",
    ]);
    assert!(matches!(action, Action::StartHotfixFix { ref name, .. } if name == "npe"), "{action:?}");
}

#[test]
fn develop_offers_every_work_kind_then_start_release() {
    let prompter = MockPrompter::scripted(&[0]).with_lines(&["login"]);

    let action = show_menu(&prompter, &BranchType::Develop, "develop", "main").unwrap();

    assert_eq!(prompter.calls()[0],
        "select:What would you like to do?:[start feature, start fix, start chore, start docs, start refactor, start release]");
    match action {
        Action::StartWorkBranch { prefix, name, from, .. } => {
            assert_eq!((prefix.as_str(), name.as_str(), from.as_str()), ("feature", "login", "develop"));
        }
        other => panic!("expected a work branch, got {other:?}"),
    }
}

#[test]
fn the_last_develop_entry_is_start_release_and_needs_no_name() {
    // Index 5 is past the work-kind table — it must fall through to StartRelease
    // rather than index the table out of bounds.
    let prompter = MockPrompter::scripted(&[5]);

    let action = show_menu(&prompter, &BranchType::Develop, "develop", "main").unwrap();

    assert!(matches!(action, Action::StartRelease { release_type: None, no_worktree: false }), "{action:?}");
    assert_eq!(prompter.calls().len(), 1, "start release asks for nothing else");
}

#[test]
fn a_work_branch_offers_finish_first_using_its_own_kind() {
    let branch_type = BranchType::Refactor { name: "auth".to_string() };
    let prompter = MockPrompter::scripted(&[0]);

    let action = show_menu(&prompter, &branch_type, "refactor/auth", "main").unwrap();

    assert_eq!(prompter.calls()[0],
        "select:What would you like to do?:[finish refactor, start feature, start fix, start chore, start docs, start refactor]");
    assert!(matches!(action, Action::FinishWorkBranch { breaking: None, base: None }), "{action:?}");
}

#[test]
fn starting_a_branch_from_a_work_branch_asks_which_base_to_use() {
    // Stacked work: the current branch is offered as a base alongside develop.
    let branch_type = BranchType::Feature { name: "login".to_string() };
    let prompter = MockPrompter::scripted(&[1, 0]).with_lines(&["captcha"]); // "start feature", then base = current

    let action = show_menu(&prompter, &branch_type, "feature/login", "main").unwrap();

    assert_eq!(prompter.calls()[2], "select:Base branch:[feature/login (current), develop]");
    match action {
        Action::StartWorkBranch { prefix, name, from, .. } => {
            assert_eq!((prefix.as_str(), name.as_str(), from.as_str()), ("feature", "captcha", "feature/login"));
        }
        other => panic!("expected a work branch, got {other:?}"),
    }
}

#[test]
fn choosing_develop_as_the_base_overrides_the_current_branch() {
    let branch_type = BranchType::Feature { name: "login".to_string() };
    let prompter = MockPrompter::scripted(&[1, 1]).with_lines(&["captcha"]);

    let action = show_menu(&prompter, &branch_type, "feature/login", "main").unwrap();

    match action {
        Action::StartWorkBranch { from, .. } => assert_eq!(from, "develop"),
        other => panic!("expected a work branch, got {other:?}"),
    }
}

#[test]
fn a_release_branch_offers_its_four_actions() {
    let branch_type = BranchType::Release { major: 2, minor: 5, patch: 0 };

    for (idx, expected) in [
        (0, Action::FinishRelease),
        (2, Action::BumpVersion),
        (3, Action::SyncWithDevelop),
    ] {
        let prompter = MockPrompter::scripted(&[idx]);

        let action = show_menu(&prompter, &branch_type, "release/2.5.0", "main").unwrap();

        assert_eq!(prompter.calls()[0],
            "select:What would you like to do?:[finish release, start release fix, bump version, sync with develop]");
        assert_eq!(format!("{action:?}"), format!("{expected:?}"), "index {idx}");
    }
}

#[test]
fn the_release_fix_entry_asks_for_a_name() {
    let branch_type = BranchType::Release { major: 2, minor: 5, patch: 0 };
    let prompter = MockPrompter::scripted(&[1]).with_lines(&["db-index"]);

    let action = show_menu(&prompter, &branch_type, "release/2.5.0", "main").unwrap();

    assert!(matches!(action, Action::StartReleaseFix { ref name, .. } if name == "db-index"), "{action:?}");
}

#[test]
fn a_hotfix_branch_offers_finish_and_start_hotfix_fix() {
    // Principle 8: cli::resolve_action allows this on Main | Hotfix, so the
    // menu must offer it too.
    let branch_type = BranchType::Hotfix { major: 2, minor: 5, patch: 1 };

    let prompter = MockPrompter::scripted(&[0]);
    let action = show_menu(&prompter, &branch_type, "hotfix/2.5.1", "main").unwrap();
    assert_eq!(prompter.calls(), vec![
        "select:What would you like to do?:[finish hotfix, start hotfix fix]",
    ]);
    assert!(matches!(action, Action::FinishHotfix), "{action:?}");

    let prompter = MockPrompter::scripted(&[1]).with_lines(&["npe"]);
    let action = show_menu(&prompter, &branch_type, "hotfix/2.5.1", "main").unwrap();
    assert_eq!(prompter.calls()[1], "prompt_name:Name for hotfix-fix branch");
    assert!(matches!(action, Action::StartHotfixFix { ref name, .. } if name == "npe"), "{action:?}");
}

#[test]
fn single_item_menus_still_confirm_rather_than_auto_execute() {
    // decisions.md: "single-item menus still confirm rather than auto-execute".
    for (branch_type, branch, label, expected) in [
        (BranchType::ReleaseFix { major: 2, minor: 5, patch: 0, name: "db".to_string() },
         "release-fix/2.5.0/db", "finish release fix", Action::FinishReleaseFix),
        (BranchType::HotfixFix { major: 2, minor: 5, patch: 1, name: "npe".to_string() },
         "hotfix-fix/2.5.1/npe", "finish hotfix fix", Action::FinishHotfixFix),
    ] {
        let prompter = MockPrompter::scripted(&[0]);

        let action = show_menu(&prompter, &branch_type, branch, "main").unwrap();

        assert_eq!(prompter.calls(), vec![format!("select:What would you like to do?:[{label}]")]);
        assert_eq!(format!("{action:?}"), format!("{expected:?}"));
    }
}

#[test]
fn a_release_chore_branch_offers_only_finish_release_chore() {
    let branch_type = BranchType::ReleaseChore { major: 1, minor: 1, patch: 0, name: "set-version".to_string() };
    let prompter = MockPrompter::scripted(&[0]);

    let action = show_menu(&prompter, &branch_type, "release-chore/1.1.0/set-version", "main").unwrap();

    assert_eq!(prompter.calls(), vec!["select:What would you like to do?:[finish release chore]"]);
    assert!(matches!(action, Action::FinishReleaseChore), "{action:?}");
}

#[test]
fn an_unrecognized_branch_gets_the_interactive_error_with_a_next_step() {
    // The interactive and scripted variants differ on purpose: the menu adds
    // "Switch to main or develop first", the CLI stays terse.
    let prompter = MockPrompter::new();

    let err = show_menu(&prompter, &BranchType::Other, "wip", "main").unwrap_err();

    assert_eq!(err, "Not on a recognized gitflow branch. Switch to main or develop first.");
    assert!(prompter.calls().is_empty(), "nothing may be shown; calls: {:?}", prompter.calls());
}

#[test]
fn the_unrecognized_branch_error_names_the_configured_mainline() {
    let err = show_menu(&MockPrompter::new(), &BranchType::Other, "wip", "master").unwrap_err();

    assert_eq!(err, "Not on a recognized gitflow branch. Switch to master or develop first.");
}

#[test]
fn aborting_the_menu_propagates_instead_of_picking_something() {
    // Ctrl-C/Esc surface as Err("Aborted") through the normal path so terminal
    // cleanup and stash restore still run.
    let prompter = MockPrompter::aborting();

    let err = show_menu(&prompter, &BranchType::Develop, "develop", "main").unwrap_err();

    assert_eq!(err, "Aborted");
}

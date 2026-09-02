use gflow::action::{validate_branch_name, Action};

#[test]
fn start_actions_return_true() {
    let actions = vec![
        Action::StartWorkBranch { prefix: "feature".into(), name: "x".into(), from: "develop".into(), no_checkout: false, no_worktree: false },
        Action::StartRelease { release_type: None, no_worktree: false },
        Action::StartReleaseFix { name: "x".into(), no_checkout: false, no_worktree: false },
        Action::StartHotfixFix { name: "x".into(), no_checkout: false, no_worktree: false },
    ];
    for action in actions {
        assert!(action.is_start(), "Expected is_start() == true for {:?}", action);
    }
}

#[test]
fn finish_actions_return_false() {
    let actions: Vec<Action> = vec![
        Action::FinishWorkBranch { breaking: None, base: None },
        Action::FinishReleaseFix,
        Action::FinishRelease,
        Action::FinishHotfix,
        Action::FinishHotfixFix,
        Action::BumpVersion,
        Action::SyncWithDevelop,
    ];
    for action in actions {
        assert!(!action.is_start(), "Expected is_start() == false for {:?}", action);
    }
}

#[test]
fn no_checkout_returns_true_for_start_work_branch() {
    let action = Action::StartWorkBranch {
        prefix: "feature".into(),
        name: "x".into(),
        from: "develop".into(),
        no_checkout: true,
        no_worktree: false,
    };
    assert!(action.no_checkout());
}

#[test]
fn no_checkout_returns_false_for_start_work_branch_default() {
    let action = Action::StartWorkBranch {
        prefix: "feature".into(),
        name: "x".into(),
        from: "develop".into(),
        no_checkout: false,
        no_worktree: false,
    };
    assert!(!action.no_checkout());
}

#[test]
fn no_checkout_returns_true_for_start_release_fix() {
    let action = Action::StartReleaseFix { name: "x".into(), no_checkout: true, no_worktree: false };
    assert!(action.no_checkout());
}

#[test]
fn no_checkout_returns_true_for_start_hotfix_fix() {
    let action = Action::StartHotfixFix { name: "x".into(), no_checkout: true, no_worktree: false };
    assert!(action.no_checkout());
}

#[test]
fn no_checkout_returns_false_for_non_start_actions() {
    let actions: Vec<Action> = vec![
        Action::StartRelease { release_type: None, no_worktree: false },
        Action::FinishWorkBranch { breaking: None, base: None },
        Action::FinishReleaseFix,
        Action::FinishRelease,
        Action::FinishHotfix,
        Action::FinishHotfixFix,
        Action::BumpVersion,
        Action::SyncWithDevelop,
    ];
    for action in actions {
        assert!(!action.no_checkout(), "Expected no_checkout() == false for {:?}", action);
    }
}

#[test]
fn no_worktree_flag_is_honored_by_every_start_action() {
    // --no-worktree opts a single command out of an enabled worktree flow. Missing
    // it on one variant silently creates a worktree the user asked not to have.
    assert!(Action::StartWorkBranch {
        prefix: "feature".to_string(), name: "x".to_string(), from: "develop".to_string(),
        no_checkout: false, no_worktree: true,
    }.no_worktree());
    assert!(Action::StartReleaseFix {
        name: "x".to_string(), no_checkout: false, no_worktree: true,
    }.no_worktree());
    assert!(Action::StartHotfixFix {
        name: "x".to_string(), no_checkout: false, no_worktree: true,
    }.no_worktree());
    assert!(!Action::StartReleaseFix {
        name: "x".to_string(), no_checkout: false, no_worktree: false,
    }.no_worktree());
    // Non-start actions never opt out of anything.
    assert!(!Action::FinishRelease.no_worktree());
}

#[test]
fn an_empty_branch_name_is_rejected_with_its_own_message() {
    // Distinct from the special-character message: an empty name is what you get
    // from an accidental `--name ""`, and the fix is different.
    assert_eq!(validate_branch_name(""), Err("Name cannot be empty".to_string()));
}

#[test]
fn start_release_no_worktree_is_read_from_the_action() {
    assert!(Action::StartRelease { release_type: None, no_worktree: true }.no_worktree());
    assert!(!Action::StartRelease { release_type: None, no_worktree: false }.no_worktree());
}

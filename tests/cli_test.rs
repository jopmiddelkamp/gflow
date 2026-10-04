use gflow::cli::{Commands, StartKind, StartOptions, resolve_action};
use gflow::flows::start::ReleaseType;
use gflow::git::branch::BranchType;
use gflow::action::Action;

#[test]
#[should_panic(expected = "--abort is intercepted")]
fn direct_dispatch_rejects_abort_before_normal_finish_resolution() {
    let _ = resolve_action(
        Commands::Finish { breaking: None, base: None, abort: true, accept_merge_type: false },
        &BranchType::Release { major: 1, minor: 2, patch: 0 }, false, "main",
    );
}

#[test]
#[should_panic(expected = "worktree configuration is dispatched in main()")]
fn branch_dispatch_rejects_worktree_configuration() {
    let _ = resolve_action(
        Commands::Worktree { action: None, repo: false, local: false },
        &BranchType::Develop, false, "main",
    );
}

#[test]
#[should_panic(expected = "init is dispatched in main()")]
fn branch_dispatch_rejects_repository_initialization() {
    let _ = resolve_action(Commands::Init, &BranchType::Develop, false, "main");
}

// --- Start work branch tests ---

#[test]
fn start_feature_with_custom_base() {
    let cmd = Commands::Start { kind: StartKind::Feature { name: "login".to_string(), base: "feature/auth".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Feature { name: "auth".to_string() };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartWorkBranch { from, .. } if from == "feature/auth"));
}

#[test]
fn start_feature_rejects_invalid_name() {
    let cmd = Commands::Start { kind: StartKind::Feature { name: "bad..name".to_string(), base: "develop".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Develop;
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert!(result.is_err());
}

// --- Start release-fix tests ---

#[test]
fn start_release_fix_branch_gate() {
    for (case, name, opts, branch_type, worktree_enabled, expected) in [
        ("on_release_branch", "broken-login", StartOptions::default(), BranchType::Release { major: 1, minor: 2, patch: 0 }, false,
            Ok(Action::StartReleaseFix { name: "broken-login".to_string(), no_checkout: false, no_worktree: false })),
        ("on_wrong_branch", "fix", StartOptions::default(), BranchType::Develop, false,
            Err("This command is only valid on a release branch.".to_string())),
        ("no_checkout_skips_branch_check", "broken-login", StartOptions { no_checkout: true, ..Default::default() }, BranchType::Develop, false,
            Ok(Action::StartReleaseFix { name: "broken-login".to_string(), no_checkout: true, no_worktree: false })),
        // Worktree mode skips the branch-type gate (like --no-checkout).
        ("worktree_enabled_skips_branch_check", "broken-login", StartOptions::default(), BranchType::Develop, true,
            Ok(Action::StartReleaseFix { name: "broken-login".to_string(), no_checkout: false, no_worktree: false })),
        // --no-worktree opts out of the worktree flow, so the plain checkout path
        // (and its branch-type gate) applies again.
        ("no_worktree_optout_still_requires_release_branch", "broken-login", StartOptions { no_worktree: true, ..Default::default() }, BranchType::Develop, true,
            Err("This command is only valid on a release branch.".to_string())),
    ] {
        let cmd = Commands::Start { kind: StartKind::ReleaseFix { name: name.to_string(), opts } };
        assert_eq!(resolve_action(cmd, &branch_type, worktree_enabled, "main"), expected, "{case}");
    }
}

// --- Start hotfix-fix tests ---

#[test]
fn start_hotfix_fix_branch_gate() {
    for (case, name, opts, branch_type, worktree_enabled, main_branch, expected) in [
        ("on_main_branch", "urgent", StartOptions::default(), BranchType::Main, false, "main",
            Ok(Action::StartHotfixFix { name: "urgent".to_string(), no_checkout: false, no_worktree: false })),
        ("on_hotfix_branch", "urgent", StartOptions::default(), BranchType::Hotfix { major: 1, minor: 0, patch: 1 }, false, "main",
            Ok(Action::StartHotfixFix { name: "urgent".to_string(), no_checkout: false, no_worktree: false })),
        ("on_wrong_branch", "fix", StartOptions::default(), BranchType::Develop, false, "main",
            Err("This command is only valid on a main or hotfix branch.".to_string())),
        ("gate_names_the_configured_mainline", "fix", StartOptions::default(), BranchType::Develop, false, "master",
            Err("This command is only valid on a master or hotfix branch.".to_string())),
        ("no_checkout_skips_branch_check", "urgent", StartOptions { no_checkout: true, ..Default::default() }, BranchType::Develop, false, "main",
            Ok(Action::StartHotfixFix { name: "urgent".to_string(), no_checkout: true, no_worktree: false })),
        ("worktree_enabled_skips_branch_check", "urgent", StartOptions::default(), BranchType::Develop, true, "main",
            Ok(Action::StartHotfixFix { name: "urgent".to_string(), no_checkout: false, no_worktree: false })),
        ("no_worktree_optout_still_requires_main_or_hotfix", "urgent", StartOptions { no_worktree: true, ..Default::default() }, BranchType::Develop, true, "main",
            Err("This command is only valid on a main or hotfix branch.".to_string())),
    ] {
        let cmd = Commands::Start { kind: StartKind::HotfixFix { name: name.to_string(), opts } };
        assert_eq!(resolve_action(cmd, &branch_type, worktree_enabled, main_branch), expected, "{case}");
    }
}

// --- Finish tests ---

#[test]
fn finish_resolves_each_branch_type_to_its_action() {
    for (case, breaking, base, branch_type, expected) in [
        ("feature", None, None, BranchType::Feature { name: "login".to_string() },
            Ok(Action::FinishWorkBranch { breaking: None, base: None })),
        ("feature_with_base", Some(false), Some("develop"), BranchType::Feature { name: "login".to_string() },
            Ok(Action::FinishWorkBranch { breaking: Some(false), base: Some("develop".to_string()) })),
        ("feature_breaking", Some(true), None, BranchType::Feature { name: "remove-api".to_string() },
            Ok(Action::FinishWorkBranch { breaking: Some(true), base: None })),
        ("feature_explicit_non_breaking", Some(false), None, BranchType::Feature { name: "login".to_string() },
            Ok(Action::FinishWorkBranch { breaking: Some(false), base: None })),
        ("release", None, None, BranchType::Release { major: 1, minor: 2, patch: 0 },
            Ok(Action::FinishRelease)),
        ("hotfix", None, None, BranchType::Hotfix { major: 1, minor: 0, patch: 1 },
            Ok(Action::FinishHotfix)),
        ("release_fix", None, None, BranchType::ReleaseFix { major: 1, minor: 2, patch: 0, name: "broken-login".to_string() },
            Ok(Action::FinishReleaseFix)),
        ("hotfix_fix", None, None, BranchType::HotfixFix { major: 1, minor: 0, patch: 1, name: "urgent".to_string() },
            Ok(Action::FinishHotfixFix)),
        ("release_chore", None, None, BranchType::ReleaseChore { major: 1, minor: 1, patch: 0, name: "set-version".to_string() },
            Ok(Action::FinishReleaseChore)),
        ("main", None, None, BranchType::Main,
            Err("Nothing to finish on this branch.".to_string())),
        ("develop", None, None, BranchType::Develop,
            Err("Nothing to finish on this branch.".to_string())),
        ("other", None, None, BranchType::Other,
            Err("Not on a recognized gitflow branch.".to_string())),
    ] {
        let cmd = Commands::Finish { breaking, base: base.map(str::to_string), abort: false, accept_merge_type: false };
        assert_eq!(resolve_action(cmd, &branch_type, false, "main"), expected, "{case}");
    }
}

#[test]
fn finish_rejects_base_on_every_fixed_target_branch() {
    for (case, branch_type) in [
        ("release", BranchType::Release { major: 1, minor: 2, patch: 0 }),
        ("hotfix", BranchType::Hotfix { major: 1, minor: 0, patch: 1 }),
        ("release_fix", BranchType::ReleaseFix { major: 1, minor: 2, patch: 0, name: "broken-login".to_string() }),
        ("hotfix_fix", BranchType::HotfixFix { major: 1, minor: 0, patch: 1, name: "urgent".to_string() }),
        ("release_chore", BranchType::ReleaseChore { major: 1, minor: 1, patch: 0, name: "set-version".to_string() }),
    ] {
        let cmd = Commands::Finish { breaking: None, base: Some("develop".to_string()), abort: false, accept_merge_type: false };
        assert_eq!(
            resolve_action(cmd, &branch_type, false, "main"),
            Err("--base is only supported when finishing a work branch (feature/fix/chore/docs/refactor); this branch type has a fixed target.".to_string()),
            "{case}",
        );
    }
}

// --- Additional start tests ---

#[test]
fn start_release_returns_start_release_action() {
    let cmd = Commands::Start { kind: StartKind::Release { major: false, minor: false, no_worktree: false } };
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartRelease { release_type: None, .. }));
}

#[test]
fn start_release_major_flag() {
    let cmd = Commands::Start { kind: StartKind::Release { major: true, minor: false, no_worktree: false } };
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartRelease { release_type: Some(ReleaseType::Major), .. }));
}

#[test]
fn start_release_minor_flag() {
    let cmd = Commands::Start { kind: StartKind::Release { major: false, minor: true, no_worktree: false } };
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartRelease { release_type: Some(ReleaseType::Minor), .. }));
}

// --- Bump and Sync tests ---

#[test]
fn bump_on_release_branch() {
    let cmd = Commands::Bump;
    let branch_type = BranchType::Release { major: 1, minor: 2, patch: 0 };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::BumpVersion));
}

#[test]
fn bump_on_wrong_branch() {
    let cmd = Commands::Bump;
    let branch_type = BranchType::Develop;
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert_eq!(result.unwrap_err(), "This command is only valid on a release branch.");
}

#[test]
fn sync_on_release_branch() {
    let cmd = Commands::Sync { accept_merge_type: false };
    let branch_type = BranchType::Release { major: 1, minor: 2, patch: 0 };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::SyncWithDevelop));
}

#[test]
fn sync_on_wrong_branch() {
    let cmd = Commands::Sync { accept_merge_type: false };
    let branch_type = BranchType::Develop;
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert_eq!(result.unwrap_err(), "This command is only valid on a release branch.");
}

#[test]
fn start_feature_with_no_checkout_flag() {
    let cmd = Commands::Start { kind: StartKind::Feature {
        name: "login".to_string(),
        base: "develop".to_string(),
        opts: StartOptions { no_checkout: true, ..Default::default() },
    }};
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartWorkBranch { no_checkout: true, .. }));
}

#[test]
fn start_feature_with_no_worktree_flag() {
    let cmd = Commands::Start { kind: StartKind::Feature {
        name: "login".to_string(),
        base: "develop".to_string(),
        opts: StartOptions { no_worktree: true, ..Default::default() },
    }};
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartWorkBranch { no_worktree: true, .. }));
}

// --- The clap surface itself ---

#[derive(clap::Parser)]
#[command(name = "gflow")]
struct TestCli {
    #[command(subcommand)]
    command: Commands,
}

fn parse(args: &[&str]) -> Result<Commands, clap::Error> {
    let argv = std::iter::once("gflow").chain(args.iter().copied());
    clap::Parser::try_parse_from(argv).map(|c: TestCli| c.command)
}

#[test]
fn every_work_kind_in_the_table_has_a_working_start_subcommand() {
    for kind in BranchType::work_kinds() {
        let cmd = parse(&["start", kind, "--name", "x"])
            .unwrap_or_else(|e| panic!("`gflow start {kind}` must parse — the WORK_TYPES table \
                offers it in the menu, so the CLI must accept it too.\n{e}"));

        let action = resolve_action(cmd, &BranchType::Develop, false, "main").unwrap();

        match action {
            Action::StartWorkBranch { prefix, name, from, .. } => {
                assert_eq!(prefix, kind, "`start {kind}` must resolve to the same prefix");
                assert_eq!((name.as_str(), from.as_str()), ("x", "develop"),
                    "--base defaults to develop");
            }
            other => panic!("`start {kind}` produced {other:?}"),
        }
    }
}

#[test]
fn the_flag_surface_parses_what_it_promises() {
    assert!(matches!(parse(&["finish"]).unwrap(),
        Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false }));
    assert!(matches!(parse(&["finish", "--breaking"]).unwrap(),
        Commands::Finish { breaking: Some(true), .. }));
    assert!(matches!(parse(&["finish", "--breaking=false"]).unwrap(),
        Commands::Finish { breaking: Some(false), .. }));
    assert!(matches!(parse(&["finish", "--abort"]).unwrap(),
        Commands::Finish { abort: true, .. }));

    assert!(matches!(parse(&["start", "release", "--minor", "--no-worktree"]).unwrap(),
        Commands::Start { kind: StartKind::Release { no_worktree: true, minor: true, .. } }));

    // `--local` is `global = true`, so it parses either side of the subcommand.
    assert!(matches!(parse(&["worktree", "--local", "enable"]).unwrap(),
        Commands::Worktree { local: true, .. }));
    assert!(matches!(parse(&["worktree", "enable", "--local"]).unwrap(),
        Commands::Worktree { local: true, .. }));
}

#[test]
fn incompatible_flag_combinations_are_rejected_by_clap_not_by_the_flow() {
    // Declarative `conflicts_with`: rejected at parse time, before any branch
    // is touched.
    for args in [
        vec!["finish", "--breaking", "--abort"],
        vec!["finish", "--base", "develop", "--abort"],
        vec!["start", "release", "--major", "--minor"],
        vec!["start", "feature"], // --name is required
    ] {
        assert!(parse(&args).is_err(), "`gflow {}` must be rejected", args.join(" "));
    }
}

#[test]
fn start_release_no_worktree_reaches_the_action() {
    let cmd = Commands::Start { kind: StartKind::Release { major: false, minor: true, no_worktree: true } };
    let action = resolve_action(cmd, &BranchType::Develop, false, "main").unwrap();
    assert!(matches!(action, Action::StartRelease { release_type: Some(ReleaseType::Minor), no_worktree: true }), "{action:?}");
}

#[test]
fn init_parses_as_its_own_command() {
    assert!(matches!(parse(&["init"]).unwrap(), Commands::Init));
}

#[test]
fn fix_commands_reject_invalid_names_before_starting() {
    for kind in [
        StartKind::ReleaseFix {
            name: "".into(),
            opts: StartOptions::default(),
        },
        StartKind::HotfixFix {
            name: "".into(),
            opts: StartOptions::default(),
        },
    ] {
        let error =
            resolve_action(Commands::Start { kind }, &BranchType::Main, true, "main").unwrap_err();
        assert_eq!(error, "Name cannot be empty");
    }
}

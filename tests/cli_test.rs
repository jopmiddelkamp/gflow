use gflow::cli::{Commands, StartKind, StartOptions, resolve_action};
use gflow::flows::start::ReleaseType;
use gflow::git::branch::BranchType;
use gflow::action::Action;

// --- Start work branch tests ---

#[test]
fn start_feature_returns_start_work_branch_action() {
    let cmd = Commands::Start { kind: StartKind::Feature { name: "login".to_string(), base: "develop".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartWorkBranch { prefix, name, from, .. } if prefix == "feature" && name == "login" && from == "develop"));
}

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
fn start_release_fix_on_release_branch() {
    let cmd = Commands::Start { kind: StartKind::ReleaseFix { name: "broken-login".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Release { major: 1, minor: 2, patch: 0 };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartReleaseFix { name, .. } if name == "broken-login"));
}

#[test]
fn start_release_fix_on_wrong_branch() {
    let cmd = Commands::Start { kind: StartKind::ReleaseFix { name: "fix".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Develop;
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert_eq!(result.unwrap_err(), "This command is only valid on a release branch.");
}

// --- Start hotfix-fix tests ---

#[test]
fn start_hotfix_fix_on_main_branch() {
    let cmd = Commands::Start { kind: StartKind::HotfixFix { name: "urgent".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Main;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartHotfixFix { name, .. } if name == "urgent"));
}

#[test]
fn start_hotfix_fix_on_hotfix_branch() {
    let cmd = Commands::Start { kind: StartKind::HotfixFix { name: "urgent".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Hotfix { major: 1, minor: 0, patch: 1 };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartHotfixFix { name, .. } if name == "urgent"));
}

#[test]
fn start_hotfix_fix_on_wrong_branch() {
    let cmd = Commands::Start { kind: StartKind::HotfixFix { name: "fix".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Develop;
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert_eq!(result.unwrap_err(), "This command is only valid on a main or hotfix branch.");
}

#[test]
fn the_hotfix_fix_gate_names_the_configured_mainline() {
    let cmd = Commands::Start { kind: StartKind::HotfixFix { name: "fix".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Develop;
    let result = resolve_action(cmd, &branch_type, false, "master");
    assert_eq!(result.unwrap_err(), "This command is only valid on a master or hotfix branch.");
}

// --- Finish tests ---

#[test]
fn finish_on_feature_branch() {
    let cmd = Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::Feature { name: "login".to_string() };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::FinishWorkBranch { breaking: None, base: None }));
}

#[test]
fn finish_with_base_flag_on_work_branch() {
    let cmd = Commands::Finish { breaking: Some(false), base: Some("develop".to_string()), abort: false, accept_merge_type: false };
    let branch_type = BranchType::Feature { name: "login".to_string() };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::FinishWorkBranch { breaking: Some(false), base: Some(base) } if base == "develop"));
}

#[test]
fn finish_with_base_on_release_branch_errors() {
    let cmd = Commands::Finish { breaking: None, base: Some("develop".to_string()), abort: false, accept_merge_type: false };
    let branch_type = BranchType::Release { major: 1, minor: 2, patch: 0 };
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert_eq!(result.unwrap_err(), "--base is only supported when finishing a work branch (feature/fix/chore/docs/refactor); this branch type has a fixed target.");
}

#[test]
fn finish_with_base_on_hotfix_branch_errors() {
    let cmd = Commands::Finish { breaking: None, base: Some("develop".to_string()), abort: false, accept_merge_type: false };
    let branch_type = BranchType::Hotfix { major: 1, minor: 0, patch: 1 };
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert!(result.is_err());
}

#[test]
fn finish_with_base_on_release_fix_branch_errors() {
    let cmd = Commands::Finish { breaking: None, base: Some("develop".to_string()), abort: false, accept_merge_type: false };
    let branch_type = BranchType::ReleaseFix { major: 1, minor: 2, patch: 0, name: "broken-login".to_string() };
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert!(result.is_err());
}

#[test]
fn finish_with_base_on_hotfix_fix_branch_errors() {
    let cmd = Commands::Finish { breaking: None, base: Some("develop".to_string()), abort: false, accept_merge_type: false };
    let branch_type = BranchType::HotfixFix { major: 1, minor: 0, patch: 1, name: "urgent".to_string() };
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert!(result.is_err());
}

#[test]
fn finish_on_feature_branch_with_breaking_flag() {
    let cmd = Commands::Finish { breaking: Some(true), base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::Feature { name: "remove-api".to_string() };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::FinishWorkBranch { breaking: Some(true), base: None }));
}

#[test]
fn finish_on_feature_branch_with_explicit_non_breaking() {
    let cmd = Commands::Finish { breaking: Some(false), base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::Feature { name: "login".to_string() };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::FinishWorkBranch { breaking: Some(false), base: None }));
}

#[test]
fn finish_on_release_branch() {
    let cmd = Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::Release { major: 1, minor: 2, patch: 0 };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::FinishRelease));
}

#[test]
fn finish_on_hotfix_branch() {
    let cmd = Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::Hotfix { major: 1, minor: 0, patch: 1 };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::FinishHotfix));
}

#[test]
fn finish_on_main_errors() {
    let cmd = Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::Main;
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert_eq!(result.unwrap_err(), "Nothing to finish on this branch.");
}

#[test]
fn finish_on_develop_errors() {
    let cmd = Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::Develop;
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert_eq!(result.unwrap_err(), "Nothing to finish on this branch.");
}

#[test]
fn finish_on_other_branch_errors() {
    let cmd = Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::Other;
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert_eq!(result.unwrap_err(), "Not on a recognized gitflow branch.");
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

#[test]
fn finish_on_release_fix_branch() {
    let cmd = Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::ReleaseFix { major: 1, minor: 2, patch: 0, name: "broken-login".to_string() };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::FinishReleaseFix));
}

#[test]
fn finish_on_hotfix_fix_branch() {
    let cmd = Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::HotfixFix { major: 1, minor: 0, patch: 1, name: "urgent".to_string() };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::FinishHotfixFix));
}

#[test]
fn finish_with_base_on_release_chore_branch_errors() {
    let cmd = Commands::Finish { breaking: None, base: Some("develop".to_string()), abort: false, accept_merge_type: false };
    let branch_type = BranchType::ReleaseChore { major: 1, minor: 1, patch: 0, name: "set-version".to_string() };
    let result = resolve_action(cmd, &branch_type, false, "main");
    assert!(result.is_err());
}

#[test]
fn finish_on_release_chore_branch() {
    let cmd = Commands::Finish { breaking: None, base: None, abort: false, accept_merge_type: false };
    let branch_type = BranchType::ReleaseChore { major: 1, minor: 1, patch: 0, name: "set-version".to_string() };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::FinishReleaseChore));
}

#[test]
fn start_fix_returns_start_work_branch_action() {
    let cmd = Commands::Start { kind: StartKind::Fix { name: "bug".to_string(), base: "develop".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartWorkBranch { prefix, name, .. } if prefix == "fix" && name == "bug"));
}

#[test]
fn start_chore_returns_start_work_branch_action() {
    let cmd = Commands::Start { kind: StartKind::Chore { name: "deps".to_string(), base: "develop".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartWorkBranch { prefix, name, .. } if prefix == "chore" && name == "deps"));
}

#[test]
fn start_docs_returns_start_work_branch_action() {
    let cmd = Commands::Start { kind: StartKind::Docs { name: "readme".to_string(), base: "develop".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartWorkBranch { prefix, name, .. } if prefix == "docs" && name == "readme"));
}

#[test]
fn start_refactor_returns_start_work_branch_action() {
    let cmd = Commands::Start { kind: StartKind::Refactor { name: "cleanup".to_string(), base: "develop".to_string(), opts: StartOptions::default() } };
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartWorkBranch { prefix, name, .. } if prefix == "refactor" && name == "cleanup"));
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
fn start_release_fix_with_no_checkout_flag() {
    let cmd = Commands::Start { kind: StartKind::ReleaseFix {
        name: "broken-login".to_string(),
        opts: StartOptions { no_checkout: true, ..Default::default() },
    }};
    let branch_type = BranchType::Release { major: 1, minor: 2, patch: 0 };
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartReleaseFix { no_checkout: true, .. }));
}

#[test]
fn start_hotfix_fix_with_no_checkout_flag() {
    let cmd = Commands::Start { kind: StartKind::HotfixFix {
        name: "urgent".to_string(),
        opts: StartOptions { no_checkout: true, ..Default::default() },
    }};
    let branch_type = BranchType::Main;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartHotfixFix { no_checkout: true, .. }));
}

#[test]
fn start_release_fix_no_checkout_skips_branch_check() {
    let cmd = Commands::Start { kind: StartKind::ReleaseFix {
        name: "broken-login".to_string(),
        opts: StartOptions { no_checkout: true, ..Default::default() },
    }};
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartReleaseFix { no_checkout: true, .. }));
}

#[test]
fn start_hotfix_fix_no_checkout_skips_branch_check() {
    let cmd = Commands::Start { kind: StartKind::HotfixFix {
        name: "urgent".to_string(),
        opts: StartOptions { no_checkout: true, ..Default::default() },
    }};
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, false, "main").unwrap();
    assert!(matches!(action, Action::StartHotfixFix { no_checkout: true, .. }));
}

// --- Worktree mode skips the branch-type gate (like --no-checkout) ---

#[test]
fn start_release_fix_with_worktree_enabled_skips_branch_check() {
    let cmd = Commands::Start { kind: StartKind::ReleaseFix {
        name: "broken-login".to_string(),
        opts: StartOptions::default(),
    }};
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, true, "main").unwrap();
    assert!(matches!(action, Action::StartReleaseFix { .. }));
}

#[test]
fn start_hotfix_fix_with_worktree_enabled_skips_branch_check() {
    let cmd = Commands::Start { kind: StartKind::HotfixFix {
        name: "urgent".to_string(),
        opts: StartOptions::default(),
    }};
    let branch_type = BranchType::Develop;
    let action = resolve_action(cmd, &branch_type, true, "main").unwrap();
    assert!(matches!(action, Action::StartHotfixFix { .. }));
}

#[test]
fn start_release_fix_with_no_worktree_optout_still_requires_release_branch() {
    // --no-worktree opts out of the worktree flow, so the plain checkout path
    // (and its branch-type gate) applies again.
    let cmd = Commands::Start { kind: StartKind::ReleaseFix {
        name: "broken-login".to_string(),
        opts: StartOptions { no_worktree: true, ..Default::default() },
    }};
    let branch_type = BranchType::Develop;
    let result = resolve_action(cmd, &branch_type, true, "main");
    assert_eq!(result.unwrap_err(), "This command is only valid on a release branch.");
}

#[test]
fn start_hotfix_fix_with_no_worktree_optout_still_requires_main_or_hotfix() {
    let cmd = Commands::Start { kind: StartKind::HotfixFix {
        name: "urgent".to_string(),
        opts: StartOptions { no_worktree: true, ..Default::default() },
    }};
    let branch_type = BranchType::Develop;
    let result = resolve_action(cmd, &branch_type, true, "main");
    assert_eq!(result.unwrap_err(), "This command is only valid on a main or hotfix branch.");
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

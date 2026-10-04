mod common;

use std::fs;

use gflow::worktree_setup::{ShellSetup, WorktreeSetup};

#[cfg(unix)]
#[test]
fn setup_runs_in_the_new_worktree_with_the_main_root_available() {
    let root = common::tmp_dir("gflow-setup-execution");
    let worktree = root.join("new worktree");
    let main = root.join("main worktree");
    fs::create_dir(&worktree).unwrap();
    fs::create_dir(&main).unwrap();
    fs::write(main.join("config.txt"), "shared configuration").unwrap();

    ShellSetup
        .run_command(
            &worktree,
            &main,
            "cp \"$ROOT_WORKTREE_PATH/config.txt\" copied.txt",
        )
        .unwrap();

    assert_eq!(
        fs::read_to_string(worktree.join("copied.txt")).unwrap(),
        "shared configuration"
    );
    assert!(!main.join("copied.txt").exists());
}

#[test]
fn setup_reports_a_nonzero_command_exit() {
    let root = common::tmp_dir("gflow-setup-failure");

    let err = ShellSetup.run_command(&root, &root, "exit 7").unwrap_err();

    assert!(err.contains("exit 7"), "got: {err}");
    assert!(err.contains("exited with"), "got: {err}");
}

#[test]
fn setup_reports_when_its_worktree_is_missing() {
    let root = common::tmp_dir("gflow-setup-missing");

    let err = ShellSetup
        .run_command(&root.join("missing"), &root, "exit 0")
        .unwrap_err();

    assert!(err.contains("failed to run 'exit 0'"), "got: {err}");
}

#[test]
fn setup_file_read_errors_warn_and_skip_commands() {
    let root = common::tmp_dir("gflow-setup-read-error");
    fs::create_dir(root.join("worktrees.json")).unwrap();

    let (commands, warning) = gflow::worktree_setup::resolve(&root, true);

    assert!(commands.is_none());
    let warning = warning.unwrap();
    assert!(warning.contains("Failed to read"), "got: {warning}");
    assert!(warning.contains("worktrees.json"), "got: {warning}");
    assert!(warning.contains("Setup commands skipped"), "got: {warning}");
}

mod common;

use std::fs;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use gflow::version_script::{ScriptCli, VersionScript};
use gflow::worktree_setup::{ShellSetup, WorktreeSetup};

#[cfg(unix)]
fn write_script(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(unix)]
#[test]
fn version_script_receives_one_version_argument_inside_the_repository() {
    let root = common::tmp_dir("gflow-version-execution");
    let path = root.join("set-version.sh");
    write_script(
        &path,
        "#!/bin/sh\n[ \"$#\" = 1 ] || exit 8\nprintf '%s' \"$1\" > version.txt\n",
    );
    let script = ScriptCli::new(path, root.to_path_buf());

    script.run("2.3.4-rc.5").unwrap();

    assert_eq!(
        fs::read_to_string(root.join("version.txt")).unwrap(),
        "2.3.4-rc.5"
    );
}

#[cfg(unix)]
#[test]
fn a_failed_version_script_reports_its_stderr_and_exit_code() {
    let root = common::tmp_dir("gflow-version-failure");
    let path = root.join("set-version.sh");
    write_script(
        &path,
        "#!/bin/sh\nprintf '  cannot update manifest  \\n' >&2\nexit 7\n",
    );
    let script = ScriptCli::new(path, root.to_path_buf());

    let err = script.run("2.3.4").unwrap_err();

    assert!(err.contains("exit 7"), "got: {err}");
    assert!(err.contains(": cannot update manifest\n"), "got: {err}");
    assert!(err.contains("Fix the script"), "got: {err}");
}

#[test]
fn a_script_path_without_a_filename_still_has_a_display_name() {
    let script = ScriptCli::new(PathBuf::from("/"), PathBuf::from("/"));

    assert_eq!(script.display_name(), "/");
}

#[test]
fn version_script_names_the_chmod_remedy_when_it_cannot_be_spawned() {
    let root = common::tmp_dir("gflow-version-missing");
    let path = root.join("missing-set-version.sh");
    let script = ScriptCli::new(path.clone(), root.to_path_buf());

    let err = script.run("1.0.0").unwrap_err();

    let path_str = path.display().to_string();
    assert!(
        err.starts_with(&format!("Version script {path_str} could not be run: ")),
        "got: {err}"
    );
    assert!(err.contains(&format!(
        "Make it executable: chmod +x {path_str} && git update-index --chmod=+x {path_str}, then re-run the command."
    )), "got: {err}");
}

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

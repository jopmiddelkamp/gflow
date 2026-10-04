mod common;

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use gflow::version_script::{ScriptCli, VersionScript};

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

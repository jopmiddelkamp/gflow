//! Discovery and execution of the repo's optional version-bump script.
//!
//! gflow looks for `.gflow/set-version.sh` (or `.gflow/set-version.cmd` on
//! Windows) and, when present, runs it with the new version as the only
//! argument. A repo with neither file behaves exactly as if this module did
//! not exist — the caller sees `Ok(None)` and skips the step entirely.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const SCRIPT_UNIX: &str = ".gflow/set-version.sh";
pub const SCRIPT_WINDOWS: &str = ".gflow/set-version.cmd";

/// Port for running the version script. A trait so flows can be tested
/// without spawning a real process.
pub trait VersionScript {
    fn run(&self, version: &str) -> Result<(), String>;
    fn display_name(&self) -> String;
}

/// Resolve the version script for the current platform, anchored to
/// `repo_root` (not the process CWD, so it works from subdirectories).
pub fn resolve(repo_root: &Path) -> Result<Option<PathBuf>, String> {
    resolve_for(repo_root, cfg!(windows))
}

/// Platform pick factored out of `resolve` so both platforms are unit-testable
/// on any OS.
fn resolve_for(repo_root: &Path, windows: bool) -> Result<Option<PathBuf>, String> {
    let (own, other) = if windows { (SCRIPT_WINDOWS, SCRIPT_UNIX) } else { (SCRIPT_UNIX, SCRIPT_WINDOWS) };
    let own_path = repo_root.join(own);
    let other_path = repo_root.join(other);
    if own_path.exists() {
        Ok(Some(own_path))
    } else if other_path.exists() {
        Err(format!(
            "Found {} but this platform needs {}. Add {} (or remove {}).",
            other_path.display(),
            own_path.display(),
            own_path.display(),
            other_path.display(),
        ))
    } else {
        Ok(None)
    }
}

/// Turn a finished process outcome into the user-facing result. Pure, so it's
/// unit-tested directly rather than through a real spawn.
fn interpret(path: &Path, code: Option<i32>, stderr: &str) -> Result<(), String> {
    match code {
        Some(0) => Ok(()),
        Some(code) => Err(format!(
            "Version script {} failed (exit {code}): {stderr}\nFix the script, then re-run the command.",
            path.display(),
        )),
        None => Err(format!("Version script {} was terminated by a signal.", path.display())),
    }
}

/// Production `VersionScript`: runs the resolved script as a child process.
pub struct ScriptCli {
    path: PathBuf,
    repo_root: PathBuf,
}

impl ScriptCli {
    pub fn new(path: PathBuf, repo_root: PathBuf) -> Self {
        Self { path, repo_root }
    }
}

impl VersionScript for ScriptCli {
    fn run(&self, version: &str) -> Result<(), String> {
        // KISS: zero-policy shell, like CommandEditor::open — the spawn itself
        // has no decision to test; interpret() carries the outcome policy.
        let output = Command::new(&self.path)
            .arg(version)
            .current_dir(&self.repo_root)
            .output()
            .map_err(|e| format!(
                "Version script {} could not be run: {e}\nMake it executable: chmod +x {} && git update-index --chmod=+x {}, then re-run the command.",
                self.path.display(), self.path.display(), self.path.display(),
            ))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        interpret(&self.path, output.status.code(), stderr.trim())
    }

    fn display_name(&self) -> String {
        self.path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_dir() -> PathBuf {
        crate::test_support::tmp_dir("gflow-version-script-test")
    }

    #[test]
    fn resolve_picks_the_current_platforms_script() {
        // `resolve` is `resolve_for` with the platform baked in — with both
        // scripts present it must pick this platform's file, same as
        // resolve_for(root, cfg!(windows)) would.
        let root = tmp_dir();
        std::fs::create_dir_all(root.join(".gflow")).unwrap();
        std::fs::write(root.join(SCRIPT_UNIX), "#!/bin/sh\n").unwrap();
        std::fs::write(root.join(SCRIPT_WINDOWS), "@echo off\n").unwrap();
        let expected = if cfg!(windows) { SCRIPT_WINDOWS } else { SCRIPT_UNIX };
        assert_eq!(resolve(&root).unwrap(), Some(root.join(expected)));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_for_none_present_yields_none() {
        let root = tmp_dir();
        assert_eq!(resolve_for(&root, false).unwrap(), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_for_matching_platform_present_yields_its_path() {
        let root = tmp_dir();
        std::fs::create_dir_all(root.join(".gflow")).unwrap();
        std::fs::write(root.join(SCRIPT_UNIX), "#!/bin/sh\n").unwrap();
        assert_eq!(resolve_for(&root, false).unwrap(), Some(root.join(SCRIPT_UNIX)));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_for_only_other_platform_present_errors_naming_both() {
        let root = tmp_dir();
        std::fs::create_dir_all(root.join(".gflow")).unwrap();
        std::fs::write(root.join(SCRIPT_WINDOWS), "@echo off\n").unwrap();
        let err = resolve_for(&root, false).unwrap_err();
        assert!(err.contains(SCRIPT_UNIX), "got: {err}");
        assert!(err.contains(SCRIPT_WINDOWS), "got: {err}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_for_both_present_yields_own_platform() {
        let root = tmp_dir();
        std::fs::create_dir_all(root.join(".gflow")).unwrap();
        std::fs::write(root.join(SCRIPT_UNIX), "#!/bin/sh\n").unwrap();
        std::fs::write(root.join(SCRIPT_WINDOWS), "@echo off\n").unwrap();
        assert_eq!(resolve_for(&root, true).unwrap(), Some(root.join(SCRIPT_WINDOWS)));
        assert_eq!(resolve_for(&root, false).unwrap(), Some(root.join(SCRIPT_UNIX)));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn interpret_exit_zero_is_ok() {
        let path = Path::new(".gflow/set-version.sh");
        assert_eq!(interpret(path, Some(0), ""), Ok(()));
    }

    #[test]
    fn interpret_nonzero_exit_names_path_code_stderr_and_remedy() {
        let path = Path::new(".gflow/set-version.sh");
        let err = interpret(path, Some(3), "boom\n").unwrap_err();
        assert!(err.contains(".gflow/set-version.sh"), "got: {err}");
        assert!(err.contains("exit 3"), "got: {err}");
        assert!(err.contains("boom"), "got: {err}");
        assert!(err.contains("Fix the script, then re-run the command."), "got: {err}");
    }

    #[test]
    fn interpret_signal_termination_names_path() {
        let path = Path::new(".gflow/set-version.sh");
        let err = interpret(path, None, "").unwrap_err();
        assert_eq!(err, "Version script .gflow/set-version.sh was terminated by a signal.");
    }

    #[test]
    fn script_cli_display_name_is_the_file_name() {
        let script = ScriptCli::new(PathBuf::from("/repo/.gflow/set-version.sh"), PathBuf::from("/repo"));
        assert_eq!(script.display_name(), "set-version.sh");
    }

    #[test]
    fn run_names_the_chmod_remedy_when_the_script_cannot_be_spawned() {
        // A path that cannot be spawned (here: does not exist) drives the same
        // Command::output() Err branch a non-executable file would — the OS
        // refuses at exec time, so no process ever actually runs.
        let path = PathBuf::from("/definitely/does/not/exist/set-version.sh");
        let script = ScriptCli::new(path.clone(), std::env::temp_dir());

        let err = script.run("1.0.0").unwrap_err();

        let path_str = path.display().to_string();
        assert!(err.starts_with(&format!("Version script {path_str} could not be run: ")), "got: {err}");
        assert!(
            err.contains(&format!("Make it executable: chmod +x {path_str} && git update-index --chmod=+x {path_str}, then re-run the command.")),
            "got: {err}"
        );
    }
}

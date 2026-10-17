//! The one place gflow hands a user-written command line to the platform shell:
//! worktree setup commands and event hooks.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, ExitStatus};

pub(crate) fn argv(command: &str, windows: bool) -> (&'static str, Vec<String>) {
    if windows {
        ("cmd", vec!["/C".to_string(), command.to_string()])
    } else {
        ("sh", vec!["-c".to_string(), command.to_string()])
    }
}

/// Zero-policy spawn: inherited stdio, so the command can print progress or
/// ask questions. The caller decides what the status means.
pub(crate) fn status(command: &str, cwd: &Path, env: &[(&str, &OsStr)]) -> std::io::Result<ExitStatus> {
    let (program, args) = argv(command, cfg!(windows));
    Command::new(program)
        .args(&args)
        .current_dir(cwd)
        .envs(env.iter().copied())
        .status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_is_sh_c_on_unix_and_cmd_c_on_windows() {
        assert_eq!(argv("dart pub get", false), ("sh", vec!["-c".to_string(), "dart pub get".to_string()]));
        assert_eq!(argv("dart pub get", true), ("cmd", vec!["/C".to_string(), "dart pub get".to_string()]));
    }
}

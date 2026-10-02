mod common;

use common::{MockCommandRunner, MockGit};
use gflow::cli::{resolve_action, Commands, StartKind, StartOptions};
use gflow::git::branch::BranchType;
use gflow::git::{CliOutput, CommandRunner, Git, GitCli};
use gflow::hosting::detect::detect;
use gflow::prompt::Prompter;
use gflow::version::SemVer;
use gflow::worktree::{self, ConfigScope};
use std::cell::Cell;
use std::fmt::{self, Write};
use std::path::{Path, PathBuf};

type GitRead = fn(&dyn Git) -> Result<(), String>;

#[test]
fn failed_git_reads_never_become_empty_or_clean_results() {
    let cases: &[(&str, GitRead)] = &[
        ("git tag --list", |g| g.list_tags().map(|_| ())),
        ("git for-each-ref --format=%(refname:short) refs/remotes/origin/release/* refs/heads/release/*", |g| g.list_branches_matching("release/*").map(|_| ())),
        ("git status --porcelain", |g| g.is_working_tree_clean().map(|_| ())),
        ("git for-each-ref --format=%(refname:short) refs/remotes/origin/", |g| g.list_remote_branches().map(|_| ())),
        ("git rev-list --count a..b", |g| g.rev_list_count("a", "b").map(|_| ())),
        ("git rev-list --parents -n 1 a", |g| g.commit_parent_count("a").map(|_| ())),
        ("git log a..b --format=%B%x00", |g| g.commit_messages("a", "b").map(|_| ())),
        ("git ls-remote --tags origin v1.0.0", |g| g.remote_tag_exists("v1.0.0").map(|_| ())),
        ("git rev-parse develop", |g| g.is_pushed("develop").map(|_| ())),
        ("git rev-parse --git-dir", |g| g.is_mid_merge().map(|_| ())),
        ("git status --porcelain", |g| g.has_unmerged_paths().map(|_| ())),
        ("git worktree list --porcelain", |g| g.worktree_of("develop").map(|_| ())),
        ("git -C /tmp/tree status --porcelain", |g| g.is_working_tree_clean_at(Path::new("/tmp/tree")).map(|_| ())),
        ("git rev-parse --git-dir --git-common-dir", |g| g.is_linked_worktree().map(|_| ())),
        ("git stash list --format=%gd %s", |g| g.find_stash_by_message("saved").map(|_| ())),
    ];
    for (command, operation) in cases {
        let runner = MockCommandRunner::scripted(&[(128, "", "repository unavailable")]);
        let error = operation(&GitCli::new(&runner)).unwrap_err();
        assert_eq!(error, format!("{command} failed: repository unavailable"));
        assert_eq!(runner.calls(), [*command]);
    }
}

struct CannotSpawn;

impl CommandRunner for CannotSpawn {
    fn run(&self, _: &str, _: &[&str]) -> Result<CliOutput, String> {
        Err("git executable unavailable".into())
    }
}

#[test]
fn a_spawn_failure_is_never_an_unset_value_or_negative_answer() {
    let git = GitCli::new(&CannotSpawn);
    for result in [
        git.checkout("develop"),
        git.is_ancestor("a", "b").map(|_| ()),
        git.get_config("gflow.branch.main").map(|_| ()),
        git.unset_config("gflow.branch.main", false),
    ] {
        assert_eq!(result.unwrap_err(), "git executable unavailable");
    }
}

#[test]
fn worktree_removal_stops_when_a_required_command_fails() {
    let commands = [
        "git rev-parse --show-toplevel",
        "git worktree list --porcelain",
        "git -C /tmp/main worktree remove --force /tmp/linked",
    ];
    for failed in 0..3 {
        let mut responses = vec![
            (0, "/tmp/linked", ""),
            (0, "worktree /tmp/main\n", ""),
            (0, "", ""),
        ];
        responses[failed] = (128, "", "cannot access worktree");
        let runner = MockCommandRunner::scripted(&responses);
        let error = GitCli::new(&runner).remove_current_worktree().unwrap_err();
        assert_eq!(
            error,
            format!("{} failed: cannot access worktree", commands[failed])
        );
        assert_eq!(runner.calls(), commands[..=failed]);
    }
}

#[cfg(unix)]
#[test]
fn an_invalid_worktree_path_stops_before_git_runs() {
    use std::os::unix::ffi::OsStringExt;
    let path = PathBuf::from(std::ffi::OsString::from_vec(vec![0xff]));
    let runner = MockCommandRunner::scripted(&[]);
    let error = GitCli::new(&runner)
        .add_worktree(&path, "feature/new")
        .unwrap_err();
    assert_eq!(error, "Worktree path is not valid UTF-8");
    assert!(runner.calls().is_empty());
}

#[test]
fn malformed_short_status_rows_do_not_hide_later_conflicts() {
    for (status, expected) in [("X\nUU conflict", true), ("X", false)] {
        let runner = MockCommandRunner::ok(status);
        assert_eq!(GitCli::new(&runner).has_unmerged_paths().unwrap(), expected);
    }
}

#[test]
fn invalid_version_numbers_do_not_become_releases() {
    for version in [
        "x.2.3",
        "1.x.3",
        "1.2.x",
        "4294967296.2.3",
        "1.4294967296.3",
        "1.2.4294967296",
    ] {
        assert_eq!(SemVer::parse(version), None);
        assert_eq!(
            BranchType::parse(&format!("release/{version}")),
            BranchType::Other
        );
    }
    for version in ["1.2.3-rc.x", "1.2.3-rc.4294967296"] {
        assert_eq!(SemVer::parse(version), None);
    }
}

struct LimitedOutput {
    text: String,
    capacity: usize,
}

impl Write for LimitedOutput {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.text.len() + text.len() > self.capacity {
            return Err(fmt::Error);
        }
        self.text.push_str(text);
        Ok(())
    }
}

#[test]
fn version_display_reports_output_failures_in_both_version_parts() {
    for (capacity, prefix) in [(0, ""), (5, "1.2.3")] {
        let mut output = LimitedOutput {
            text: String::new(),
            capacity,
        };
        assert!(write!(output, "{}", SemVer::new(1, 2, 3).with_rc(1)).is_err());
        assert_eq!(output.text, prefix);
    }
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

#[test]
fn provider_detection_stops_when_the_override_cannot_be_read() {
    let mut git = MockGit::new();
    git.fail_call = Some(("get_config:gflow.hosting.provider".into(), 1));
    assert_eq!(
        detect(&git).unwrap_err(),
        "injected git failure: get_config:gflow.hosting.provider"
    );
    assert_eq!(git.calls(), ["get_config:gflow.hosting.provider"]);
}

struct ConfigDisappears<'a> {
    target: &'a Path,
    fail_at: usize,
    selects: Cell<usize>,
    custom_path: bool,
}

impl ConfigDisappears<'_> {
    fn block_config(&self) {
        std::fs::remove_file(self.target).unwrap();
        std::fs::create_dir(self.target).unwrap();
    }
}

impl Prompter for ConfigDisappears<'_> {
    fn select(&self, _: &str, _: &[&str]) -> Result<usize, String> {
        let call = self.selects.get() + 1;
        self.selects.set(call);
        if call == self.fail_at {
            self.block_config();
        }
        Ok(usize::from(call == 3 && self.custom_path))
    }

    fn prompt_name(&self, _: &str) -> Result<String, String> {
        panic!("worktree settings must not request a branch name")
    }

    fn prompt_line(&self, _: &str) -> Result<String, String> {
        self.block_config();
        Ok("/tmp/worktrees".into())
    }
}

#[test]
fn wizard_stops_when_a_later_setting_cannot_be_saved() {
    for (fail_at, custom_path, prompts) in [(2, false, 2), (3, false, 3), (4, true, 3)] {
        let dir = common::tmp_dir("gflow-wizard-late-write");
        let target = dir.join("config");
        let prompter = ConfigDisappears {
            target: &target,
            fail_at,
            selects: Cell::new(0),
            custom_path,
        };
        let error = worktree::wizard(&target, &prompter, ConfigScope::Global).unwrap_err();
        assert!(error.contains("Failed to read"));
        assert!(target.is_dir());
        assert_eq!(prompter.selects.get(), prompts);
    }
}

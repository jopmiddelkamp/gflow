#![cfg(unix)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use gflow::editor::{CommandEditor, Editor};
use gflow::git::{CommandRunner, SystemRunner};
use gflow::hosting::devops::AzureDevOps;
use gflow::hosting::github::GitHub;
use gflow::hosting::{CliRunner, HostingPlatform, PrBody, SystemCli};

struct ProcessFixture {
    root: common::TempDir,
}

impl ProcessFixture {
    fn new() -> Self {
        let root = common::tmp_dir("gflow-process");
        for path in ["bin", "home", "repo/.git", "repo/.gflow"] {
            fs::create_dir_all(root.join(path)).unwrap();
        }
        Self { root }
    }

    fn executable(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(self.root.join("repo"))
            .env("PATH", self.root.join("bin"))
            .env("HOME", self.root.join("home"))
            .env_remove("USERPROFILE")
            .env("GFLOW_FIXTURE", &*self.root)
            .env("GFLOW_CAPTURE", self.root.join("capture"));
        command
    }

    fn run_gflow(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_gflow"))
            .args(args)
            .output()
            .unwrap()
    }

    fn git(&self) {
        self.executable(
            "git",
            r#"
printf '%s\n' "$*" >> "$GFLOW_FIXTURE/git-calls"
case "$*" in
    '--version') printf 'git version fixture\n' ;;
    'rev-parse --show-toplevel')
        if [ "$GFLOW_FAIL_ROOT" = 'true' ]; then
            printf 'worktree root unavailable\n' >&2
            exit 128
        fi
        printf '%s/repo\n' "$GFLOW_FIXTURE"
        ;;
    'rev-parse --abbrev-ref HEAD')
        if [ "$GFLOW_NO_REPO" = 'true' ]; then
            printf 'not a repository\n' >&2
            exit 128
        fi
        printf 'feature/coverage\n'
        ;;
    'rev-parse --git-dir') printf '%s/repo/.git\n' "$GFLOW_FIXTURE" ;;
    'config --get gflow.branch.main') printf 'main\n' ;;
    'config --get gflow.hosting.provider')
        if [ -n "$GFLOW_PROVIDER" ]; then printf '%s\n' "$GFLOW_PROVIDER"; else exit 1; fi
        ;;
    'remote get-url origin') printf '%s\n' "${GFLOW_REMOTE:-https://github.com/example/fixture.git}" ;;
    'config --global --get '*|'config --local --get '*) exit 1 ;;
    *) printf 'unexpected git call: %s\n' "$*" >&2; exit 97 ;;
esac
"#,
        );
    }

    fn initialized(&self) {
        fs::write(self.root.join("repo/.gflow/config"), "mode=free\n").unwrap();
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        stdout(output),
        stderr(output)
    );
}

#[test]
fn missing_git_stops_before_any_repository_work() {
    let fixture = ProcessFixture::new();
    let output = fixture.run_gflow(&["finish", "--abort"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("'git' is not installed or not in PATH."));
    assert!(!fixture.root.join("git-calls").exists());
}

#[test]
fn a_non_repository_reports_the_repository_error() {
    let fixture = ProcessFixture::new();
    fixture.git();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_gflow"))
        .args(["finish", "--abort"])
        .env("GFLOW_NO_REPO", "true")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(stderr(&output).contains("Not in a git repository."));
    let calls = fs::read_to_string(fixture.root.join("git-calls")).unwrap();
    assert!(!calls.contains("remote get-url"));
}

#[test]
fn worktree_settings_write_before_repository_or_hosting_checks() {
    let fixture = ProcessFixture::new();
    fixture.git();
    let output = fixture.run_gflow(&["worktree", "enable", "--local"]);
    assert_success(&output);
    let config = fs::read_to_string(fixture.root.join("repo/.gflow/config.local")).unwrap();
    assert!(config.contains("worktree=true"));
    let calls = fs::read_to_string(fixture.root.join("git-calls")).unwrap();
    assert!(!calls.contains("rev-parse --abbrev-ref"));
    assert!(!calls.contains("remote get-url"));
}

#[test]
fn init_on_an_initialized_repository_does_not_contact_hosting() {
    let fixture = ProcessFixture::new();
    fixture.git();
    fixture.initialized();
    let output = fixture.run_gflow(&["init"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("Already initialised"));
    let calls = fs::read_to_string(fixture.root.join("git-calls")).unwrap();
    assert!(!calls.contains("remote get-url"));
}

#[test]
fn abort_runs_without_hosting_cli_authentication_or_fetch() {
    for remote in [
        "https://github.com/example/fixture.git",
        "https://dev.azure.com/example/project/_git/fixture",
    ] {
        let fixture = ProcessFixture::new();
        fixture.git();
        fixture.initialized();
        let output = fixture
            .command(env!("CARGO_BIN_EXE_gflow"))
            .args(["finish", "--abort"])
            .env("GFLOW_REMOTE", remote)
            .output()
            .unwrap();
        assert_success(&output);
        assert!(stdout(&output).contains("No in-progress finish"));
        let calls = fs::read_to_string(fixture.root.join("git-calls")).unwrap();
        assert!(calls.contains("remote get-url origin"));
        assert!(!calls.lines().any(|line| line == "fetch"));
    }
}

#[test]
fn config_warnings_reach_stderr_without_blocking_abort() {
    let fixture = ProcessFixture::new();
    fixture.git();
    fixture.initialized();
    fs::write(
        fixture.root.join("repo/.gflow/config.local"),
        "mode=protected\nworktree=true\n",
    )
    .unwrap();
    fs::write(fixture.root.join("repo/worktrees.json"), "{broken").unwrap();
    fs::write(fixture.root.join("repo/.gflow/set-version.sh"), "exit 99\n").unwrap();
    let output = fixture.run_gflow(&["finish", "--abort"]);
    assert_success(&output);
    assert!(stderr(&output).contains("Warning:"));
    assert!(stderr(&output).contains("mode"));
    assert!(stderr(&output).contains("worktrees.json"));
}

#[test]
fn invalid_global_configuration_stops_before_branch_discovery() {
    let fixture = ProcessFixture::new();
    fixture.git();
    fs::create_dir_all(fixture.root.join("home/.gflow")).unwrap();
    fs::write(fixture.root.join("home/.gflow/config"), "mode=invalid\n").unwrap();
    let output = fixture.run_gflow(&["finish", "--abort"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("mode"));
    let calls = fs::read_to_string(fixture.root.join("git-calls")).unwrap();
    assert!(!calls.contains("rev-parse --abbrev-ref"));
}

#[test]
fn a_failed_worktree_root_lookup_stops_before_hosting_detection() {
    let fixture = ProcessFixture::new();
    fixture.git();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_gflow"))
        .args(["finish", "--abort"])
        .env("GFLOW_FAIL_ROOT", "true")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(stderr(&output).contains("worktree root unavailable"));
    let calls = fs::read_to_string(fixture.root.join("git-calls")).unwrap();
    assert!(!calls.contains("remote get-url"));
}

#[test]
fn subcommands_require_an_initialized_configuration() {
    let fixture = ProcessFixture::new();
    fixture.git();
    let output = fixture.run_gflow(&["finish", "--abort"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("gflow init"));
    let calls = fs::read_to_string(fixture.root.join("git-calls")).unwrap();
    assert!(!calls.contains("remote get-url"));
}

#[test]
fn a_wrong_platform_version_script_stops_before_hosting_detection() {
    let fixture = ProcessFixture::new();
    fixture.git();
    fixture.initialized();
    fs::write(fixture.root.join("repo/.gflow/set-version.cmd"), "exit 0\n").unwrap();
    let output = fixture.run_gflow(&["finish", "--abort"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("set-version.sh"));
    let calls = fs::read_to_string(fixture.root.join("git-calls")).unwrap();
    assert!(!calls.contains("remote get-url"));
}

#[test]
fn an_invalid_hosting_provider_stops_before_lifecycle_work() {
    let fixture = ProcessFixture::new();
    fixture.git();
    fixture.initialized();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_gflow"))
        .args(["finish", "--abort"])
        .env("GFLOW_PROVIDER", "invalid")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(stderr(&output).contains("Invalid gflow.hosting.provider value"));
    let calls = fs::read_to_string(fixture.root.join("git-calls")).unwrap();
    assert!(!calls.contains("rev-parse --git-dir"));
}

#[test]
fn process_adapters_preserve_arguments_outputs_and_failures() {
    let fixture = ProcessFixture::new();
    fixture.executable(
        "successful-tool",
        r#"printf '%s\n' "$@" > "$GFLOW_CAPTURE"
printf '  output\377\n'
printf '  diagnostic\n' >&2"#,
    );
    fixture.executable("failed-tool", "printf '  rejected  \\n' >&2\nexit 23");
    fixture.executable("signaled-tool", "kill -TERM $$");
    fs::write(fixture.root.join("bin/not-executable"), "unusable").unwrap();
    let output = fixture
        .command(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process_adapter_child",
            "--ignored",
            "--nocapture",
        ])
        .env("GFLOW_CHILD", "adapters")
        .output()
        .unwrap();
    assert_success(&output);
}

#[test]
fn browser_and_clipboard_defaults_use_platform_tools() {
    for outcome in ["success", "failure", "missing"] {
        let fixture = ProcessFixture::new();
        if outcome != "missing" {
            for browser in ["open", "xdg-open"] {
                fixture.executable(
                    browser,
                    r#"printf '%s\n' "$@" > "$GFLOW_CAPTURE"
if [ "$GFLOW_CHILD" = 'failure' ]; then
    printf 'browser rejected URL\n' >&2
    exit 23
fi"#,
                );
            }
            for clipboard in ["pbcopy", "wl-copy", "xclip", "xsel"] {
                fixture.executable(
                    clipboard,
                    r#"/bin/cat > "$GFLOW_CAPTURE"
if [ "$GFLOW_CHILD" = 'failure' ]; then exit 23; fi"#,
                );
            }
        }
        let output = fixture
            .command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process_adapter_child",
                "--ignored",
                "--nocapture",
            ])
            .env("GFLOW_CHILD", outcome)
            .output()
            .unwrap();
        assert_success(&output);
    }
}

#[test]
fn clipboard_reports_a_tool_that_closes_its_input_early() {
    let fixture = ProcessFixture::new();
    for clipboard in ["pbcopy", "wl-copy", "xclip", "xsel"] {
        fixture.executable(clipboard, "exec 0<&-\nexit 0");
    }
    let output = fixture
        .command(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process_adapter_child",
            "--ignored",
            "--nocapture",
        ])
        .env("GFLOW_CHILD", "closed-pipe")
        .output()
        .unwrap();
    assert_success(&output);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn clipboard_requires_the_tools_exit_status_to_confirm_success() {
    let fixture = ProcessFixture::new();
    for clipboard in ["pbcopy", "wl-copy", "xclip", "xsel"] {
        fixture.executable(
            clipboard,
            "IFS= read -r text\nprintf '%s\\n' \"$text\" > \"$GFLOW_CAPTURE\"",
        );
    }
    let output = fixture
        .command(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process_adapter_child",
            "--ignored",
            "--nocapture",
        ])
        .env("GFLOW_CHILD", "reaped-clipboard")
        .output()
        .unwrap();
    assert_success(&output);
    assert_eq!(
        fs::read_to_string(fixture.root.join("capture")).unwrap(),
        "copied\n"
    );
}

#[test]
fn providers_use_the_repository_native_template_when_requested() {
    let fixture = ProcessFixture::new();
    for path in [
        ".github/PULL_REQUEST_TEMPLATE.md",
        ".azuredevops/pull_request_template.md",
    ] {
        let path = fixture.root.join("repo").join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "Summary\n\nValidation\n").unwrap();
    }
    let output = fixture
        .command(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process_adapter_child",
            "--ignored",
            "--nocapture",
        ])
        .env("GFLOW_CHILD", "templates")
        .output()
        .unwrap();
    assert_success(&output);
}

#[test]
#[ignore = "runs only inside controlled process tests"]
fn process_adapter_child() {
    let scenario = std::env::var("GFLOW_CHILD").expect("parent test must select a child scenario");
    let capture = PathBuf::from(std::env::var_os("GFLOW_CAPTURE").unwrap());
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if scenario == "reaped-clipboard" {
        unsafe extern "C" {
            fn signal(signal: std::ffi::c_int, handler: usize) -> usize;
        }
        #[cfg(target_os = "macos")]
        const SIGCHLD: std::ffi::c_int = 20;
        #[cfg(target_os = "linux")]
        const SIGCHLD: std::ffi::c_int = 17;
        // The isolated child makes the OS reap tools before wait can collect their status.
        assert_ne!(unsafe { signal(SIGCHLD, 1) }, usize::MAX);
        assert_eq!(
            GitHub::new(&SystemCli).copy_text("copied\n").unwrap_err(),
            "no clipboard tool available"
        );
        return;
    }
    if scenario == "closed-pipe" {
        let text = "x".repeat(1_000_000);
        assert_eq!(
            GitHub::new(&SystemCli).copy_text(&text).unwrap_err(),
            "no clipboard tool available"
        );
        return;
    }
    if scenario == "templates" {
        let runner =
            common::MockCliRunner::scripted(&[Ok(""), Ok("https://github.com/o/r/pull/1")]);
        let url = GitHub::new(&runner)
            .create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault)
            .unwrap();
        assert_eq!(url, "https://github.com/o/r/pull/1");
        assert!(runner.calls()[1].ends_with("--body-file .github/PULL_REQUEST_TEMPLATE.md"));
        let runner = common::MockCliRunner::scripted(&[Ok("extension"), Ok(""), Ok("42")]);
        let hosting = AzureDevOps::new("org".into(), "project".into(), "repo".into(), &runner);
        let url = hosting
            .create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault)
            .unwrap();
        assert_eq!(
            url,
            "https://dev.azure.com/org/project/_git/repo/pullrequest/42"
        );
        assert!(
            runner.calls()[2].contains("--description Summary  Validation  --query pullRequestId")
        );
        return;
    }
    if scenario == "adapters" {
        let output = SystemRunner
            .run("successful-tool", &["one argument", "two"])
            .unwrap();
        assert_eq!(output.code, Some(0));
        assert_eq!(output.stdout, "  output�\n");
        assert_eq!(output.stderr, "  diagnostic\n");
        assert_eq!(fs::read_to_string(&capture).unwrap(), "one argument\ntwo\n");
        let output = SystemRunner.run("failed-tool", &[]).unwrap();
        assert_eq!(output.code, Some(23));
        assert_eq!(output.stderr, "  rejected  \n");
        assert_eq!(SystemRunner.run("signaled-tool", &[]).unwrap().code, None);
        assert!(SystemRunner
            .run("missing-tool", &[])
            .err()
            .unwrap()
            .contains("Failed to run missing-tool"));

        assert_eq!(
            SystemCli.run("successful-tool", &["one argument"]).unwrap(),
            "output�"
        );
        assert_eq!(fs::read_to_string(&capture).unwrap(), "one argument\n");
        assert_eq!(
            SystemCli.run("failed-tool", &["arg"]).unwrap_err(),
            "failed-tool arg failed: rejected"
        );
        assert_eq!(
            SystemCli.run("missing-tool", &[]).unwrap_err(),
            "'missing-tool' is not installed or not in PATH."
        );
        assert!(SystemCli
            .run("not-executable", &[])
            .unwrap_err()
            .contains("Failed to run not-executable:"));

        CommandEditor::new("successful-tool".into())
            .open(Path::new("folder with spaces"))
            .unwrap();
        assert_eq!(
            fs::read_to_string(&capture).unwrap(),
            "folder with spaces\n"
        );
        assert!(CommandEditor::new("failed-tool".into())
            .open(Path::new("folder"))
            .unwrap_err()
            .contains("exited with"));
        assert!(CommandEditor::new("missing-tool".into())
            .open(Path::new("folder"))
            .unwrap_err()
            .contains("failed to run editor 'missing-tool'"));
        return;
    }

    let hosting = GitHub::new(&SystemCli);
    let opened = hosting.open_url("https://example.invalid/pull/1?a=b&c=d");
    if scenario == "success" {
        opened.unwrap();
        assert_eq!(
            fs::read_to_string(&capture).unwrap(),
            "https://example.invalid/pull/1?a=b&c=d\n"
        );
    } else {
        let error = opened.unwrap_err();
        assert!(error.starts_with("Failed to open URL:"));
        if scenario == "failure" {
            assert!(error.contains("browser rejected URL"));
        }
    }
    let copied = hosting.copy_text("Pull request\nhttps://example.invalid/pull/1\n");
    if scenario == "success" {
        copied.unwrap();
        assert_eq!(
            fs::read_to_string(&capture).unwrap(),
            "Pull request\nhttps://example.invalid/pull/1\n"
        );
    } else {
        assert_eq!(copied.unwrap_err(), "no clipboard tool available");
    }
}
